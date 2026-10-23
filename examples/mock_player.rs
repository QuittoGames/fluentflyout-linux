//! A deliberately small MPRIS player used to exercise the media integration.
//!
//! It is not part of the shipped application: it exists so the flyout can be
//! verified end to end (discovery, metadata, artwork, playback state and
//! control calls) without depending on Spotify, Firefox or a real desktop.
//!
//! ```text
//! cargo run --example mock_player -- --art /path/to/cover.png
//! ```
//!
//! Every incoming method call is printed, which makes the control path
//! observable from the terminal.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use zbus::object_server::SignalEmitter;
use zbus::{ObjectPath, OwnedVariant, Variant};

const BUS_NAME: &str = "org.mpris.MediaPlayer2.fluentflyoutmock";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

/// Track length used by the mock, in microseconds.
const TRACK_LENGTH_US: i64 = 214_000_000;

struct RootInterface {
    identity: String,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl RootInterface {
    #[zbus(property)]
    fn identity(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "fluentflyout-mock".to_string()
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

#[derive(Clone)]
struct PlayerInterface {
    playing: Arc<AtomicBool>,
    position_us: Arc<AtomicI64>,
    title: String,
    artist: String,
    album: String,
    art_url: String,
    loop_status: Arc<std::sync::Mutex<String>>,
    shuffle: Arc<AtomicBool>,
}

impl PlayerInterface {
    fn build_metadata(&self) -> HashMap<String, OwnedVariant> {
        let mut map: HashMap<String, OwnedVariant> = HashMap::new();
        let mut insert = |key: &str, value: Variant<'static>| {
            map.insert(
                key.to_string(),
                OwnedVariant::try_from(value).expect("metadata value is ownable"),
            );
        };

        insert("xesam:title", Variant::from(self.title.clone()));
        insert("xesam:artist", Variant::from(vec![self.artist.clone()]));
        insert("xesam:album", Variant::from(self.album.clone()));
        insert("mpris:length", Variant::from(TRACK_LENGTH_US));
        insert(
            "mpris:trackid",
            Variant::from(
                ObjectPath::try_from("/org/fluentflyout/MockTrack/1").expect("static object path"),
            ),
        );
        if !self.art_url.is_empty() {
            insert("mpris:artUrl", Variant::from(self.art_url.clone()));
        }
        map
    }

    /// Emits `PropertiesChanged` with explicit new values.
    async fn notify(
        emitter: &SignalEmitter<'_>,
        changed: HashMap<&str, Variant<'_>>,
    ) -> zbus::Result<()> {
        zbus::fdo::Properties::properties_changed(
            emitter,
            PLAYER_INTERFACE.try_into()?,
            changed,
            Cow::Borrowed(&[] as &[&str]),
        )
        .await
    }

    /// Emits `PropertiesChanged` with the listed properties invalidated.
    ///
    /// Invalidating is spec-correct and lets clients re-read the value, which
    /// avoids building a nested `a{sv}` variant by hand.
    async fn invalidate(emitter: &SignalEmitter<'_>, properties: &[&str]) -> zbus::Result<()> {
        zbus::fdo::Properties::properties_changed(
            emitter,
            PLAYER_INTERFACE.try_into()?,
            HashMap::new(),
            Cow::Owned(properties.to_vec()),
        )
        .await
    }

    fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::SeqCst);
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerInterface {
    async fn play(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        println!("mock: Play");
        self.set_playing(true);
        let _ = Self::notify(
            &emitter,
            HashMap::from([("PlaybackStatus", Variant::from("Playing"))]),
        )
        .await;
    }

    async fn pause(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        println!("mock: Pause");
        self.set_playing(false);
        let _ = Self::notify(
            &emitter,
            HashMap::from([("PlaybackStatus", Variant::from("Paused"))]),
        )
        .await;
    }

    async fn play_pause(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        println!("mock: PlayPause");
        let next = !self.playing.load(Ordering::SeqCst);
        self.set_playing(next);
        let status = if next { "Playing" } else { "Paused" };
        let _ = Self::notify(
            &emitter,
            HashMap::from([("PlaybackStatus", Variant::from(status))]),
        )
        .await;
    }

    async fn next(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        println!("mock: Next");
        self.position_us.store(0, Ordering::SeqCst);
        let _ = Self::invalidate(&emitter, &["Metadata"]).await;
    }

    async fn previous(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        println!("mock: Previous");
        self.position_us.store(0, Ordering::SeqCst);
        let _ = Self::invalidate(&emitter, &["Metadata"]).await;
    }

    async fn seek(&self, offset: i64) {
        println!("mock: Seek {offset}");
        let next = (self.position_us.load(Ordering::SeqCst) + offset).clamp(0, TRACK_LENGTH_US);
        self.position_us.store(next, Ordering::SeqCst);
    }

    async fn set_position(&self, _track_id: ObjectPath<'_>, position: i64) {
        println!("mock: SetPosition {position}");
        self.position_us
            .store(position.clamp(0, TRACK_LENGTH_US), Ordering::SeqCst);
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        if self.playing.load(Ordering::SeqCst) {
            "Playing".to_string()
        } else {
            "Paused".to_string()
        }
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedVariant> {
        self.build_metadata()
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        self.position_us.load(Ordering::SeqCst)
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        self.loop_status
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| "None".to_string())
    }

    #[zbus(property)]
    fn set_loop_status(&self, value: String) {
        println!("mock: LoopStatus = {value}");
        if let Ok(mut slot) = self.loop_status.lock() {
            *slot = value;
        }
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.shuffle.load(Ordering::SeqCst)
    }

    #[zbus(property)]
    fn set_shuffle(&self, value: bool) {
        println!("mock: Shuffle = {value}");
        self.shuffle.store(value, Ordering::SeqCst);
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

fn main() {
    let mut art_url = String::new();
    let mut identity = "Mock Player".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--art" => {
                if let Some(path) = args.next() {
                    let absolute = std::fs::canonicalize(&path).unwrap_or_else(|_| path.into());
                    art_url = format!("file://{}", absolute.display());
                }
            }
            "--identity" => {
                if let Some(value) = args.next() {
                    identity = value;
                }
            }
            other => eprintln!("mock_player: ignoring unknown argument {other}"),
        }
    }

    let playing = Arc::new(AtomicBool::new(true));
    let position_us = Arc::new(AtomicI64::new(42_000_000));

    // Advance the position while "playing" so the seekbar has visible motion.
    {
        let playing = playing.clone();
        let position_us = position_us.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if playing.load(Ordering::SeqCst) {
                    let next = (position_us.load(Ordering::SeqCst) + 250_000) % TRACK_LENGTH_US;
                    position_us.store(next, Ordering::SeqCst);
                }
            }
        });
    }

    let player = PlayerInterface {
        playing,
        position_us,
        title: "Northern Lights".to_string(),
        artist: "Aurora Field".to_string(),
        album: "Signal".to_string(),
        art_url,
        loop_status: Arc::new(std::sync::Mutex::new("None".to_string())),
        shuffle: Arc::new(AtomicBool::new(false)),
    };
    let root = RootInterface { identity };

    let connection = zbus::block_on(async {
        zbus::connection::Builder::session()
            .name(BUS_NAME)
            .serve_at(OBJECT_PATH, root)
            .serve_at(OBJECT_PATH, player)
            .build()
            .await
    });

    match connection {
        Ok(_) => {
            println!("mock: {BUS_NAME} ready on {OBJECT_PATH}");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        Err(error) => {
            eprintln!("mock: cannot own {BUS_NAME}: {error}");
            std::process::exit(1);
        }
    }
}
