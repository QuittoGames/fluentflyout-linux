//! MPRIS client implementation.
//!
//! Discovery, property watching and command dispatch for every player that
//! exports `org.mpris.MediaPlayer2.Player` on the session bus.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use async_channel::{Receiver, Sender, TrySendError};
use futures_util::FutureExt;
use futures_util::StreamExt;
use zbus::Connection;
use zbus::MatchRule;
use zbus::MessageStream;
use zbus::OwnedVariant as OwnedValue;
use zbus::Proxy;
use zbus::Variant;

use crate::app::AppEvent;
use crate::domain::media::{
    ArtworkSource, LoopStatus, MediaSnapshot, PlaybackState, PlayerCapabilities, PlayerInfo,
    TrackMetadata,
};

/// Object path every MPRIS player exposes.
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
/// Player interface.
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
/// Root interface, used for `Identity` and `DesktopEntry`.
const MPRIS_ROOT: &str = "org.mpris.MediaPlayer2";
/// Bus-name prefix used by every MPRIS player.
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2";
/// `org.freedesktop.DBus.Properties` interface.
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// How often the service re-reads `Position` while a track is playing.
///
/// MPRIS has no property-changed signal for position (only `Seeked`), so a
/// light poll is required for a moving seekbar. It only runs while something
/// is actually playing.
const POSITION_POLL: Duration = Duration::from_millis(500);

/// Position ticks between discovery reconciles (6 × 500 ms ≈ 3 s).
const RECONCILE_TICKS: u64 = 6;

/// Requests the UI sends to the media service.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaCommand {
    TogglePlayPause,
    Next,
    Previous,
    /// Seek relative to the current position, in microseconds.
    SeekRelative(i64),
    /// Seek to an absolute position, in microseconds.
    SeekAbsolute(i64),
    SetLoop(LoopStatus),
    SetShuffle(bool),
    /// Pin the service to a specific bus name (empty string clears the pin).
    SetPinnedPlayer(String),
    SetPauseOthers(bool),
    /// Force a re-read of the active player, e.g. when a flyout opens.
    Refresh,
    Shutdown,
}

/// Reported once, so the UI can explain that no player integration exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceStatus {
    Ready,
    Unavailable(String),
}

/// Handle-free service entry point.
pub struct MediaService;

impl MediaService {
    /// Starts the MPRIS service on its own thread.
    ///
    /// Events are published on `events`; commands are read from `commands`.
    /// The thread exits when the command channel closes or `Shutdown` arrives.
    pub fn spawn(
        events: Sender<AppEvent>,
        commands: Receiver<MediaCommand>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::Builder::new()
            .name("mpris-service".into())
            .spawn(move || {
                // zbus drives its own I/O on the calling thread, so its
                // `block_on` must be used instead of an external executor.
                let runtime = zbus::block_on(run(events.clone(), commands));
                if let Err(error) = runtime {
                    log::error!("MPRIS service stopped: {error}");
                    let _ = events.try_send(AppEvent::MediaStatus(ServiceStatus::Unavailable(
                        error.to_string(),
                    )));
                }
            })
            .expect("spawn mpris thread")
    }
}

/// A player the service is currently tracking.
struct PlayerHandle {
    info: PlayerInfo,
    proxy: Proxy<'static>,
}

async fn run(events: Sender<AppEvent>, commands: Receiver<MediaCommand>) -> anyhow::Result<()> {
    let connection = zbus::Connection::session()
        .await
        .map_err(|error| anyhow::anyhow!("no session bus: {error}"))?;

    // One stream for everything we care about: property changes on any
    // player, and players appearing/disappearing on the bus.
    let properties_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(PROPERTIES)
        .member("PropertiesChanged")
        .path(MPRIS_PATH)
        .build()?;
    let name_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus")
        .member("NameOwnerChanged")
        .arg0ns("org.mpris.MediaPlayer2")
        .build()?;

    let capacity = std::num::NonZeroUsize::new(64);
    let mut stream = MessageStream::for_match_rule(properties_rule, &connection, capacity).await?;
    let mut name_stream = MessageStream::for_match_rule(name_rule, &connection, capacity).await?;

    let mut state = ServiceState::default();
    emit_status(&events, ServiceStatus::Ready);

    // Initial scan.
    state.rebind(&connection).await;
    state.publish(&events).await;

    // Position polling. `zbus::block_on` drives the connection itself, so an
    // external async timer would never wake this loop; a small thread poking a
    // bounded channel is simpler and cannot desynchronise from the reactor.
    let (tick_tx, tick_rx) = async_channel::bounded::<()>(1);
    std::thread::Builder::new()
        .name("mpris-position-tick".into())
        .spawn(move || {
            loop {
                std::thread::sleep(POSITION_POLL);
                match tick_tx.try_send(()) {
                    Ok(()) => {}
                    // A tick is already pending; the loop will consume it.
                    Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Closed(_)) => break,
                }
            }
        })
        .ok();

    let mut ticks: u64 = 0;
    loop {
        let tick = tick_rx.recv().fuse();
        let properties_signal = stream.next().fuse();
        let name_signal = name_stream.next().fuse();
        let command = commands.recv().fuse();

        futures_util::pin_mut!(tick, properties_signal, name_signal, command);

        let event = futures_util::select! {
            _ = tick => LoopEvent::Tick,
            message = properties_signal => LoopEvent::Properties(message),
            message = name_signal => LoopEvent::Names(message),
            command = command => LoopEvent::Command(command),
        };

        match event {
            LoopEvent::Tick => {
                ticks = ticks.wrapping_add(1);
                // Cheap guard: the position only moves while playing.
                if state
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.state.is_playing())
                {
                    state.refresh_position(&events).await;
                }
                // Self-healing discovery: a missed bus signal must not leave a
                // dead player on screen forever. Only a changed player set
                // falls through to re-read and publish.
                if ticks % RECONCILE_TICKS == 0 && state.reconcile(&connection).await {
                    // fall through
                } else {
                    continue;
                }
            }
            LoopEvent::Properties(message) => {
                match message {
                    // Any property change on an MPRIS path invalidates the
                    // snapshot; re-reading it is simpler and more correct than
                    // tracking individual keys, and avoids comparing the
                    // message's unique sender name against well-known names.
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        log::debug!("properties stream error: {error}");
                        continue;
                    }
                    None => {
                        // The bus went away; nothing more can be observed.
                        emit_status(
                            &events,
                            ServiceStatus::Unavailable("session bus closed".into()),
                        );
                        return Ok(());
                    }
                }
            }
            LoopEvent::Names(message) => {
                let message = match message {
                    Some(Ok(message)) => message,
                    Some(Err(error)) => {
                        log::debug!("name stream error: {error}");
                        continue;
                    }
                    None => {
                        emit_status(
                            &events,
                            ServiceStatus::Unavailable("session bus closed".into()),
                        );
                        return Ok(());
                    }
                };
                if let Some((name, _, new_owner)) = parse_name_owner_changed(&message) {
                    log::debug!(
                        "bus name {name} {}",
                        if new_owner.is_empty() {
                            "released"
                        } else {
                            "acquired"
                        }
                    );
                }
                if !state.reconcile(&connection).await {
                    continue;
                }
            }
            LoopEvent::Command(command) => {
                let command = match command {
                    Ok(command) => command,
                    // The UI dropped its sender: shut the service down.
                    Err(_) => return Ok(()),
                };
                match command {
                    MediaCommand::Shutdown => return Ok(()),
                    MediaCommand::Refresh => {
                        state.rebind(&connection).await;
                    }
                    MediaCommand::SetPinnedPlayer(bus_name) => {
                        state.pinned = if bus_name.is_empty() {
                            None
                        } else {
                            Some(bus_name)
                        };
                    }
                    MediaCommand::SetPauseOthers(enabled) => {
                        state.pause_others = enabled;
                    }
                    other => state.apply(&other).await,
                }
            }
        }

        state.rebuild_snapshot().await;
        state.pause_other_players().await;
        state.publish(&events).await;
    }
}

enum LoopEvent {
    Tick,
    Properties(Option<Result<zbus::Message, zbus::Error>>),
    Names(Option<Result<zbus::Message, zbus::Error>>),
    Command(Result<MediaCommand, async_channel::RecvError>),
}

#[derive(Default)]
struct ServiceState {
    players: HashMap<String, PlayerHandle>,
    /// Bus names ordered by most recent activity, newest last.
    activity: Vec<String>,
    pinned: Option<String>,
    pause_others: bool,
    snapshot: Option<MediaSnapshot>,
    published: Option<MediaSnapshot>,
    paused_others_for: Option<String>,
}

impl ServiceState {
    fn active_name(&self) -> Option<String> {
        if let Some(pinned) = &self.pinned
            && self.players.contains_key(pinned)
        {
            return Some(pinned.clone());
        }
        // Most recently active player that is playing, else the most recent
        // player that is paused, else nothing.
        let by_recency = || self.activity.iter().rev().cloned();
        let playing = by_recency().find(|name| {
            self.snapshot
                .as_ref()
                .filter(|snapshot| snapshot.player.bus_name == *name)
                .is_some_and(|snapshot| snapshot.state.is_playing())
        });
        playing.or_else(|| by_recency().find(|name| self.players.contains_key(name)))
    }

    /// Rebuilds the tracked player set from the bus.
    ///
    /// Proxies are cheap to create but expensive to keep stale, so this is
    /// only called when the bus changes or a refresh is requested.
    async fn rebind(&mut self, connection: &Connection) {
        let mpris_names = match list_mpris_names(connection).await {
            Ok(names) => names,
            Err(error) => {
                log::warn!("cannot list bus names: {error}");
                return;
            }
        };

        let mut fresh: HashMap<String, PlayerHandle> = HashMap::new();
        for name in mpris_names {
            if let Some(existing) = self.players.remove(&name) {
                fresh.insert(name, existing);
                continue;
            }
            match build_handle(connection, &name).await {
                Ok(handle) => {
                    fresh.insert(name.clone(), handle);
                    // Track the name for the PropertiesChanged rule; the
                    // stream is matched by path, so an explicit AddMatch per
                    // name is not needed, but we do log unknown senders.
                    log::debug!("tracking MPRIS player {name}");
                }
                Err(error) => {
                    log::debug!("ignoring {name}: {error}");
                }
            }
        }

        self.players = fresh;
        self.activity.retain(|name| self.players.contains_key(name));
        for name in self.players.keys() {
            if !self.activity.contains(name) {
                self.activity.push(name.clone());
            }
        }

        self.rebuild_snapshot().await;
    }

    /// Re-lists players and rebinds only when the set actually changed.
    ///
    /// `NameOwnerChanged` is the primary discovery signal, but it is a
    /// broadcast: a restarted bus, a missed message while the loop was busy or
    /// a player that appeared before the match rule was installed would
    /// otherwise leave a dead entry on screen. Reconciling on a slow timer
    /// costs one `ListNames` call and keeps discovery self-healing.
    async fn reconcile(&mut self, connection: &Connection) -> bool {
        let Ok(names) = list_mpris_names(connection).await else {
            return false;
        };
        let mut known: Vec<&str> = self.players.keys().map(String::as_str).collect();
        known.sort_unstable();
        let mut incoming: Vec<&str> = names.iter().map(String::as_str).collect();
        incoming.sort_unstable();
        if known == incoming {
            return false;
        }
        self.rebind(connection).await;
        true
    }

    /// Re-reads the active player's full state.
    async fn rebuild_snapshot(&mut self) {
        let Some(name) = self.active_name() else {
            self.snapshot = None;
            return;
        };
        let Some(handle) = self.players.get(&name) else {
            self.snapshot = None;
            return;
        };

        match read_snapshot(handle).await {
            Ok(snapshot) => {
                if let Some(previous) = &self.snapshot
                    && previous.player.bus_name != snapshot.player.bus_name
                {
                    // New active player: promote it in the recency list.
                    self.activity
                        .retain(|existing| existing != &snapshot.player.bus_name);
                    self.activity.push(snapshot.player.bus_name.clone());
                }
                self.snapshot = Some(snapshot);
            }
            Err(error) => {
                log::debug!("player {name} disappeared while reading: {error}");
                self.players.remove(&name);
                self.activity.retain(|existing| existing != &name);
                self.snapshot = None;
            }
        }
    }

    /// Cheap position refresh used by the seekbar poll.
    async fn refresh_position(&mut self, events: &Sender<AppEvent>) {
        let Some(snapshot) = self.snapshot.as_mut() else {
            return;
        };
        let Some(handle) = self.players.get(&snapshot.player.bus_name) else {
            return;
        };
        match handle.proxy.get_property::<i64>("Position").await {
            Ok(position) if position != snapshot.position_us => {
                snapshot.position_us = position;
                // A position-only change must not raise the flyout; the UI
                // distinguishes this event from a real state change.
                let _ = events.try_send(AppEvent::MediaPosition(position));
            }
            Ok(_) => {}
            Err(error) => log::trace!("position refresh failed: {error}"),
        }
    }

    async fn apply(&mut self, command: &MediaCommand) {
        let Some(name) = self.active_name() else {
            return;
        };
        let Some(handle) = self.players.get(&name) else {
            return;
        };

        // Every arm yields `Result<(), _>`: `call_method` returns the reply
        // message, `set_property` returns nothing.
        let result: Result<(), zbus::Error> = match command {
            MediaCommand::TogglePlayPause => {
                let method = match self.snapshot.as_ref().map(|s| s.state) {
                    Some(PlaybackState::Playing) => "Pause",
                    _ => "Play",
                };
                handle.proxy.call_method(method, &()).await.map(|_| ())
            }
            MediaCommand::Next => handle.proxy.call_method("Next", &()).await.map(|_| ()),
            MediaCommand::Previous => handle.proxy.call_method("Previous", &()).await.map(|_| ()),
            MediaCommand::SeekRelative(offset) => {
                handle.proxy.call_method("Seek", offset).await.map(|_| ())
            }
            MediaCommand::SeekAbsolute(position) => {
                let track_id = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.track.track_id.clone());
                match track_id {
                    Some(track_id) => match zbus::ObjectPath::try_from(track_id) {
                        Ok(path) => handle
                            .proxy
                            .call_method("SetPosition", &(path, *position))
                            .await
                            .map(|_| ()),
                        Err(error) => {
                            log::warn!("invalid mpris:trackid: {error}");
                            return;
                        }
                    },
                    // Spec allows "/org/mpris/MediaPlayer2/TrackList/NoTrack";
                    // without a track id an absolute seek cannot be expressed.
                    None => return,
                }
            }
            MediaCommand::SetLoop(status) => handle
                .proxy
                .set_property("LoopStatus", status.as_mpris())
                .await
                .map_err(zbus::Error::from),
            MediaCommand::SetShuffle(enabled) => handle
                .proxy
                .set_property("Shuffle", *enabled)
                .await
                .map_err(zbus::Error::from),
            MediaCommand::Refresh
            | MediaCommand::SetPinnedPlayer(_)
            | MediaCommand::SetPauseOthers(_)
            | MediaCommand::Shutdown => return,
        };

        if let Err(error) = result {
            log::warn!("MPRIS command {command:?} failed: {error}");
        }
    }

    /// Mirrors the upstream "pause other sessions" option.
    async fn pause_other_players(&mut self) {
        if !self.pause_others {
            self.paused_others_for = None;
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        if !snapshot.state.is_playing() {
            return;
        }
        // Only act once per transition, otherwise we would fight a player
        // the user explicitly resumed.
        if self.paused_others_for.as_deref() == Some(snapshot.player.bus_name.as_str()) {
            return;
        }
        self.paused_others_for = Some(snapshot.player.bus_name.clone());

        for (name, handle) in &self.players {
            if *name == snapshot.player.bus_name {
                continue;
            }
            let is_playing = handle
                .proxy
                .get_property::<String>("PlaybackStatus")
                .await
                .map(|status| status == "Playing")
                .unwrap_or(false);
            if is_playing && let Err(error) = handle.proxy.call_method("Pause", &()).await {
                log::debug!("could not pause {name}: {error}");
            }
        }
    }

    async fn publish(&mut self, events: &Sender<AppEvent>) {
        let Some(snapshot) = &self.snapshot else {
            if self.published.is_some() {
                self.published = None;
                let _ = events.try_send(AppEvent::MediaSnapshot(None));
            }
            return;
        };
        if self.published.as_ref() == Some(snapshot) {
            return;
        }
        self.published = Some(snapshot.clone());
        match events.try_send(AppEvent::MediaSnapshot(Some(Box::new(snapshot.clone())))) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => log::debug!("event channel full; dropping snapshot"),
            Err(TrySendError::Closed(_)) => {}
        }
    }
}

async fn build_handle(connection: &Connection, name: &str) -> anyhow::Result<PlayerHandle> {
    // `new_owned` keeps the proxy independent of the borrow of `connection`,
    // which lets the handle outlive this call.
    let proxy = Proxy::new_owned(
        connection.clone(),
        name.to_string(),
        MPRIS_PATH,
        MPRIS_PLAYER,
    )
    .await?;
    let root_proxy =
        Proxy::new_owned(connection.clone(), name.to_string(), MPRIS_PATH, MPRIS_ROOT).await?;

    let identity = root_proxy
        .get_property::<String>("Identity")
        .await
        .unwrap_or_else(|_| name.rsplit('.').next().unwrap_or(name).to_string());
    let desktop_entry = root_proxy
        .get_property::<String>("DesktopEntry")
        .await
        .ok()
        .filter(|entry| !entry.is_empty());

    Ok(PlayerHandle {
        info: PlayerInfo {
            bus_name: name.to_string(),
            identity,
            desktop_entry,
        },
        proxy,
    })
}

async fn read_snapshot(handle: &PlayerHandle) -> anyhow::Result<MediaSnapshot> {
    let state = handle
        .proxy
        .get_property::<String>("PlaybackStatus")
        .await
        .map(|status| PlaybackState::from_mpris(&status))
        .unwrap_or_default();

    let metadata: HashMap<String, OwnedValue> = handle
        .proxy
        .get_property("Metadata")
        .await
        .unwrap_or_default();
    let track = parse_metadata(&metadata);

    let caps = PlayerCapabilities {
        can_play_pause: handle
            .proxy
            .get_property::<bool>("CanPlay")
            .await
            .unwrap_or(false)
            || handle
                .proxy
                .get_property::<bool>("CanPause")
                .await
                .unwrap_or(false),
        can_go_next: handle
            .proxy
            .get_property::<bool>("CanGoNext")
            .await
            .unwrap_or(false),
        can_go_previous: handle
            .proxy
            .get_property::<bool>("CanGoPrevious")
            .await
            .unwrap_or(false),
        can_seek: handle
            .proxy
            .get_property::<bool>("CanSeek")
            .await
            .unwrap_or(false),
        can_set_loop: handle
            .proxy
            .get_property::<bool>("CanControl")
            .await
            .unwrap_or(true),
        can_set_shuffle: handle
            .proxy
            .get_property::<bool>("CanControl")
            .await
            .unwrap_or(true),
    };

    let loop_status = handle
        .proxy
        .get_property::<String>("LoopStatus")
        .await
        .map(|value| LoopStatus::from_mpris(&value))
        .unwrap_or_default();
    let shuffle = handle
        .proxy
        .get_property::<bool>("Shuffle")
        .await
        .unwrap_or(false);
    let position_us = handle
        .proxy
        .get_property::<i64>("Position")
        .await
        .unwrap_or(0);

    Ok(MediaSnapshot {
        player: handle.info.clone(),
        state,
        length_us: track.length_us,
        track,
        caps,
        loop_status,
        shuffle,
        position_us,
    })
}

/// Parses an `a{sv}` metadata dictionary.
///
/// MPRIS types the dictionary as `variant`, and implementations disagree on
/// the concrete type of the same key (`xesam:artist` is normally `as` but
/// sometimes `s`; `mpris:length` is `x` or `t`). Matching on the variant
/// directly handles every shape without guessing.
fn parse_metadata(metadata: &HashMap<String, OwnedValue>) -> TrackMetadata {
    let string_field = |key: &str| metadata.get(key).and_then(|value| string_of(value));

    let artist = metadata
        .get("xesam:artist")
        .map(|value| string_list(value).join(", "))
        .unwrap_or_default();

    let length_us = metadata.get("mpris:length").and_then(|value| {
        number_of(value).or_else(|| string_field("mpris:length")?.parse::<i64>().ok())
    });

    let artwork = string_field("mpris:artUrl").and_then(|url| resolve_artwork(&url));

    TrackMetadata {
        title: string_field("xesam:title").unwrap_or_default(),
        artist,
        album: string_field("xesam:album").unwrap_or_default(),
        track_id: string_field("mpris:trackid"),
        artwork,
        length_us,
    }
}

/// Extracts a string from the subset of D-Bus types players actually use for
/// metadata text.
fn string_of(value: &Variant<'_>) -> Option<String> {
    match value {
        Variant::Str(text) => Some(text.as_str().to_string()),
        Variant::ObjectPath(path) => Some(path.as_str().to_string()),
        Variant::Signature(signature) => Some(signature.to_string()),
        _ => None,
    }
}

/// Extracts a list of strings, accepting either an array or a single value.
fn string_list(value: &Variant<'_>) -> Vec<String> {
    match value {
        Variant::Array(items) => items.iter().filter_map(string_of).collect(),
        other => string_of(other).into_iter().collect(),
    }
}

/// Extracts an integer regardless of the width a player chose.
fn number_of(value: &Variant<'_>) -> Option<i64> {
    match value {
        Variant::I64(number) => Some(*number),
        Variant::U64(number) => i64::try_from(*number).ok(),
        Variant::I32(number) => Some(i64::from(*number)),
        Variant::U32(number) => Some(i64::from(*number)),
        Variant::I16(number) => Some(i64::from(*number)),
        Variant::U16(number) => Some(i64::from(*number)),
        _ => None,
    }
}

/// Turns `mpris:artUrl` into a loadable source.
///
/// Only `file://` URIs are resolved here; HTTP(S) URLs are handed to the UI,
/// which loads them through GIO on the GTK thread.
fn resolve_artwork(url: &str) -> Option<ArtworkSource> {
    if url.is_empty() {
        return None;
    }
    if let Some(rest) = url.strip_prefix("file://") {
        let decoded = percent_decode(rest);
        let path = PathBuf::from(decoded);
        if path.exists() {
            return Some(ArtworkSource::Local(path));
        }
        return None;
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return Some(ArtworkSource::Remote(url.to_string()));
    }
    // Some players hand out a bare absolute path.
    let path = PathBuf::from(url);
    if path.is_absolute() && path.exists() {
        return Some(ArtworkSource::Local(path));
    }
    None
}

/// Minimal percent-decoding for `file://` URIs.
///
/// MPRIS art URLs are frequently produced by `glib::filename_to_uri`, so
/// spaces and non-ASCII characters appear as `%XX` sequences.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                out.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Extracts `(name, old_owner, new_owner)` from a `NameOwnerChanged` signal.
fn parse_name_owner_changed(message: &zbus::Message) -> Option<(String, String, String)> {
    let body = message.body();
    let (name, old_owner, new_owner): (String, String, String) = body.deserialize().ok()?;
    Some((name, old_owner, new_owner))
}

fn emit_status(events: &Sender<AppEvent>, status: ServiceStatus) {
    let _ = events.try_send(AppEvent::MediaStatus(status));
}

/// Lists every well-known bus name that exports an MPRIS player.
///
/// The bare `org.mpris.MediaPlayer2` name is excluded: it is the interface
/// and bus-name prefix, never a player itself.
async fn list_mpris_names(connection: &Connection) -> zbus::Result<Vec<String>> {
    let dbus = zbus::fdo::DBusProxy::new(connection).await?;
    let names = dbus.list_names().await?;
    Ok(names
        .into_iter()
        .map(|name| name.to_string())
        .filter(|name| name.starts_with(MPRIS_PREFIX) && name.as_str() != MPRIS_PREFIX)
        .collect())
}

/// Exposed for the diagnostics command: reports whether a session bus exists
/// and whether any MPRIS player is currently registered.
pub fn probe_session_bus() -> Result<Vec<String>, String> {
    zbus::block_on(async {
        let connection = zbus::Connection::session()
            .await
            .map_err(|error| error.to_string())?;
        list_mpris_names(&connection)
            .await
            .map_err(|error| error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::Variant as Value;

    #[test]
    fn percent_decoding_handles_spaces_and_utf8() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("Caf%C3%A9"), "Café");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn malformed_percent_sequences_are_left_alone() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%ZZ"), "%ZZ");
        // Truncated escape at the end must not index out of bounds.
        assert_eq!(percent_decode("%4"), "%4");
    }

    #[test]
    fn artwork_resolution_honours_file_uris() {
        // The temp dir always exists, so this exercises the happy path.
        let dir = std::env::temp_dir();
        let uri = format!("file://{}", dir.display());
        match resolve_artwork(&uri) {
            Some(ArtworkSource::Local(path)) => assert_eq!(path, dir),
            other => panic!("expected a local artwork source, got {other:?}"),
        }
    }

    #[test]
    fn missing_local_artwork_is_dropped() {
        assert_eq!(
            resolve_artwork("file:///definitely/not/here/cover.png"),
            None
        );
    }

    #[test]
    fn remote_artwork_is_kept_as_a_url() {
        assert_eq!(
            resolve_artwork("https://example.test/cover.jpg"),
            Some(ArtworkSource::Remote(
                "https://example.test/cover.jpg".into()
            ))
        );
    }

    #[test]
    fn empty_and_relative_artwork_is_ignored() {
        assert_eq!(resolve_artwork(""), None);
        assert_eq!(resolve_artwork("relative/cover.png"), None);
    }

    #[test]
    fn metadata_parsing_uses_standard_keys() {
        let mut metadata: HashMap<String, OwnedValue> = HashMap::new();
        metadata.insert(
            "xesam:title".into(),
            OwnedValue::try_from(Value::from("Song")).expect("value"),
        );
        metadata.insert(
            "xesam:artist".into(),
            OwnedValue::try_from(Value::from(vec!["A".to_string(), "B".to_string()]))
                .expect("value"),
        );
        metadata.insert(
            "mpris:length".into(),
            OwnedValue::try_from(Value::from(180_000_000i64)).expect("value"),
        );

        let track = parse_metadata(&metadata);
        assert_eq!(track.title, "Song");
        assert_eq!(track.artist, "A, B");
        assert_eq!(track.length_us, Some(180_000_000));
        assert!(track.artwork.is_none());
    }

    #[test]
    fn metadata_parsing_tolerates_an_empty_dictionary() {
        let track = parse_metadata(&HashMap::new());
        assert_eq!(track, TrackMetadata::default());
    }

    #[test]
    fn artist_as_plain_string_is_supported() {
        let mut metadata: HashMap<String, OwnedValue> = HashMap::new();
        metadata.insert(
            "xesam:artist".into(),
            OwnedValue::try_from(Value::from("Solo")).expect("value"),
        );
        assert_eq!(parse_metadata(&metadata).artist, "Solo");
    }
}
