//! Application orchestration.
//!
//! Owns the GTK windows, the in-memory view of every subsystem and the
//! auto-hide timers. Subsystems publish [`AppEvent`]s; this module decides
//! what the user sees. Windows never talk to D-Bus or `/sys` directly.

pub mod control;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use gtk::glib;
use gtk::prelude::*;

use crate::audio::{AudioCommand, AudioStatus};
use crate::domain::lock_keys::{LockKeyKind, LockKeySupport};
use crate::domain::media::MediaSnapshot;
use crate::domain::volume::VolumeState;
use crate::lock_keys::LockKeyCommand;
use crate::media::{MediaCommand, ServiceStatus};
use crate::settings::SettingsStore;
use crate::ui::animation::Easing;
use crate::ui::flyout_lock::LockFlyout;
use crate::ui::flyout_media::MediaFlyout;
use crate::ui::flyout_volume::VolumeFlyout;
use crate::ui::settings_window;
use crate::ui::style;

/// Which flyout a timer or request refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlyoutKind {
    Media,
    LockKeys,
    Volume,
}

/// A flyout the user (or another process) asked to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowTarget {
    Media,
    LockKeys,
    Volume,
    Settings,
}

/// Everything subsystems can tell the UI.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Boxed because a snapshot is ~256 bytes while the other variants are a
    /// handful of bytes; every event queue entry pays for the largest one.
    MediaSnapshot(Option<Box<MediaSnapshot>>),
    /// Position-only update from the seekbar poll; must never raise a flyout.
    MediaPosition(i64),
    MediaStatus(ServiceStatus),
    AudioStatus(AudioStatus),
    Volume(Option<VolumeState>),
    LockKeyState {
        kind: LockKeyKind,
        on: bool,
    },
    LockKeyPressed(LockKeyKind),
    LockKeySupport(LockKeySupport),
    Show(ShowTarget),
    /// Media transport command issued through the D-Bus control interface,
    /// so a desktop shortcut can drive playback.
    MediaControl(crate::media::MediaCommand),
    SettingsChanged,
    Quit,
}

/// Outbound channels for the running services.
#[derive(Clone)]
pub struct Services {
    pub media: Sender<MediaCommand>,
    pub audio: Sender<AudioCommand>,
    pub lock: Sender<LockKeyCommand>,
}

/// The application state machine.
pub struct App {
    application: adw::Application,
    settings: Rc<RefCell<SettingsStore>>,
    services: Services,
    events: Sender<AppEvent>,
    status_slot: control::StatusSlot,

    snapshot: Option<MediaSnapshot>,
    media_status: ServiceStatus,
    audio_status: AudioStatus,
    volume: Option<VolumeState>,
    lock_support: LockKeySupport,
    lock_states: HashMap<LockKeyKind, bool>,

    media: MediaFlyout,
    lock: LockFlyout,
    volume_flyout: VolumeFlyout,
    settings_window: Option<adw::ApplicationWindow>,

    timers: HashMap<FlyoutKind, glib::SourceId>,
    save_timer: Option<glib::SourceId>,
    /// Suppresses auto-show until the first real change after startup.
    startup_grace: bool,
}

impl App {
    /// Builds the UI and wires the event pump.
    ///
    /// `unknown_audio` / `unknown_lock` are the initial capability values,
    /// replaced as soon as the services report.
    pub fn new(
        application: adw::Application,
        settings: Rc<RefCell<SettingsStore>>,
        services: Services,
        events: Sender<AppEvent>,
        status_slot: control::StatusSlot,
    ) -> Rc<RefCell<Self>> {
        style::install(&application, settings.borrow().current());
        let media = MediaFlyout::new(&application, services.media.clone(), settings.clone());
        let lock = LockFlyout::new(&application);
        let volume_flyout = VolumeFlyout::new(&application, services.audio.clone());

        let app = Rc::new(RefCell::new(Self {
            application,
            settings,
            services,
            events,
            status_slot,
            snapshot: None,
            media_status: ServiceStatus::Ready,
            audio_status: AudioStatus::Unavailable("probing".into()),
            volume: None,
            lock_support: LockKeySupport::Unavailable {
                reason: "probing".into(),
            },
            lock_states: HashMap::new(),
            media,
            lock,
            volume_flyout,
            settings_window: None,
            timers: HashMap::new(),
            save_timer: None,
            startup_grace: true,
        }));

        Self::connect_hover(&app);
        app
    }

    /// Starts consuming events on the GTK main context.
    pub fn spawn_pump(state: Rc<RefCell<Self>>, events: Receiver<AppEvent>) {
        glib::MainContext::default().spawn_local(async move {
            while let Ok(event) = events.recv().await {
                let quit = Self::dispatch(&state, event);
                if quit {
                    state.borrow().application.quit();
                    break;
                }
            }
        });
    }

    /// Applies one event. Returns `true` when the app should quit.
    fn dispatch(state: &Rc<RefCell<Self>>, event: AppEvent) -> bool {
        match event {
            AppEvent::Quit => return true,
            AppEvent::MediaSnapshot(snapshot) => {
                let auto_show = Self::apply_media(state, snapshot.map(|boxed| *boxed));
                if auto_show
                    && state
                        .borrow()
                        .settings
                        .borrow()
                        .current()
                        .show_on_media_change
                    && !state.borrow().startup_grace
                {
                    Self::present(state, FlyoutKind::Media);
                }
            }
            AppEvent::MediaPosition(position) => {
                state.borrow_mut().on_position(position);
            }
            AppEvent::MediaStatus(status) => {
                if status != ServiceStatus::Ready {
                    log::warn!("media service: {status:?}");
                }
                state.borrow_mut().media_status = status;
            }
            AppEvent::AudioStatus(status) => {
                state.borrow_mut().audio_status = status;
            }
            AppEvent::Volume(volume) => {
                state.borrow_mut().on_volume(volume);
            }
            AppEvent::LockKeyState { kind, on } => {
                Self::on_lock_state(state, kind, on);
            }
            AppEvent::LockKeyPressed(kind) => Self::on_lock_pressed(state, kind),
            AppEvent::LockKeySupport(support) => {
                log::info!("lock-key support: {support:?}");
                state.borrow_mut().lock_support = support;
            }
            AppEvent::Show(target) => Self::on_show_request(state, target),
            AppEvent::MediaControl(command) => {
                let _ = state.borrow().services.media.try_send(command);
            }
            AppEvent::SettingsChanged => Self::on_settings_changed(state),
        }
        Self::publish_status(state);
        false
    }

    /// Refreshes the JSON document served by the D-Bus `Status` property.
    fn publish_status(state: &Rc<RefCell<Self>>) {
        let text = Self::status_snapshot(state);
        let slot = state.borrow().status_slot.clone();
        if let Ok(mut guard) = slot.lock()
            && *guard != text
        {
            *guard = text;
        }
    }

    /// Stores a new media snapshot. Returns whether the change should raise
    /// the flyout (track or playback state changed, not just the position).
    fn apply_media(state: &Rc<RefCell<Self>>, snapshot: Option<MediaSnapshot>) -> bool {
        let mut app = state.borrow_mut();
        let previous = app.snapshot.clone();
        app.snapshot = snapshot.clone();
        // Cloned so the settings borrow ends before the (mutable) app field
        // is used again; `RefCell` would otherwise panic at runtime.
        let settings = app.settings.borrow().current().clone();
        app.media.update(snapshot.as_ref(), &settings);

        // The first snapshot after startup is the baseline, not a change.
        if app.startup_grace {
            app.startup_grace = false;
            return false;
        }

        match (previous, snapshot) {
            (Some(before), Some(after)) => {
                before.player.bus_name != after.player.bus_name
                    || before.track.track_id != after.track.track_id
                    || before.track.title != after.track.title
                    || before.state != after.state
            }
            (None, Some(_)) => true,
            _ => false,
        }
    }

    fn on_position(&mut self, position: i64) {
        if let Some(snapshot) = self.snapshot.as_mut() {
            snapshot.position_us = position;
            self.media.update_position(snapshot);
        }
    }

    fn on_volume(&mut self, volume: Option<VolumeState>) {
        self.volume = volume;
        self.volume_flyout.update(volume);
    }

    fn on_lock_state(state: &Rc<RefCell<Self>>, kind: LockKeyKind, on: bool) {
        {
            let mut app = state.borrow_mut();
            app.lock_states.insert(kind, on);
        }
        Self::on_lock_pressed(state, kind);
    }

    fn on_lock_pressed(state: &Rc<RefCell<Self>>, kind: LockKeyKind) {
        let (enabled, bold) = {
            let app = state.borrow();
            let settings = app.settings.borrow();
            let settings = settings.current();
            let enabled = settings.lock_keys_enabled && kind_is_enabled(settings, kind);
            (enabled, settings.lock_keys_animated)
        };
        if !enabled {
            return;
        }
        let on = state
            .borrow()
            .lock_states
            .get(&kind)
            .copied()
            .unwrap_or(false);
        state.borrow().lock.show_state(kind, on, bold);
        Self::present(state, FlyoutKind::LockKeys);
    }

    fn on_show_request(state: &Rc<RefCell<Self>>, target: ShowTarget) {
        match target {
            ShowTarget::Settings => Self::present_settings(state),
            ShowTarget::Media => {
                // Refresh first: the flyout may have been opened long after
                // the last signal.
                let _ = state
                    .borrow()
                    .services
                    .media
                    .try_send(MediaCommand::Refresh);
                state.borrow_mut().startup_grace = false;
                Self::present(state, FlyoutKind::Media);
            }
            ShowTarget::LockKeys => {
                let _ = state
                    .borrow()
                    .services
                    .lock
                    .try_send(LockKeyCommand::Refresh);
                Self::present(state, FlyoutKind::LockKeys);
            }
            ShowTarget::Volume => {
                let _ = state
                    .borrow()
                    .services
                    .audio
                    .try_send(AudioCommand::Refresh);
                Self::present(state, FlyoutKind::Volume);
            }
        }
    }

    /// Makes a flyout visible and starts (or restarts) its auto-hide timer.
    fn present(state: &Rc<RefCell<Self>>, kind: FlyoutKind) {
        let (duration, always) = {
            let app = state.borrow();
            let settings = app.settings.borrow();
            let settings = settings.current();
            let duration = match kind {
                FlyoutKind::Media => settings.duration_ms,
                FlyoutKind::LockKeys => settings.lock_keys_duration_ms,
                FlyoutKind::Volume => settings.volume_duration_ms,
            };
            let always = kind == FlyoutKind::Media && settings.media_flyout_always_display;
            (duration.clamp(0, 10_000) as u64, always)
        };

        {
            let app = state.borrow();
            match kind {
                FlyoutKind::Media => app.media.present(),
                FlyoutKind::LockKeys => app.lock.present(),
                FlyoutKind::Volume => app.volume_flyout.present(),
            }
        }

        let easing = Easing::from(state.borrow().settings.borrow().current().easing);
        let duration_ms = state
            .borrow()
            .settings
            .borrow()
            .current()
            .animation_duration_ms() as u64;
        match kind {
            FlyoutKind::Media => state.borrow().media.animate_in(easing, duration_ms),
            FlyoutKind::LockKeys => state.borrow().lock.animate_in(easing, duration_ms),
            FlyoutKind::Volume => state.borrow().volume_flyout.animate_in(easing, duration_ms),
        }

        if always {
            Self::cancel_timer(state, kind);
            return;
        }
        Self::schedule_hide(state, kind, duration);
    }

    fn schedule_hide(state: &Rc<RefCell<Self>>, kind: FlyoutKind, after_ms: u64) {
        Self::cancel_timer(state, kind);
        let weak = Rc::downgrade(state);
        let source = glib::timeout_add_local_once(Duration::from_millis(after_ms), move || {
            if let Some(state) = weak.upgrade() {
                let (easing, duration) = {
                    let app = state.borrow();
                    let settings = app.settings.borrow();
                    let settings = settings.current();
                    (
                        Easing::from(settings.easing),
                        settings.animation_duration_ms() as u64,
                    )
                };
                let app = state.borrow();
                match kind {
                    FlyoutKind::Media => app.media.animate_out(easing, duration, &app.media.window),
                    FlyoutKind::LockKeys => {
                        app.lock.animate_out(easing, duration, &app.lock.window)
                    }
                    FlyoutKind::Volume => {
                        app.volume_flyout
                            .animate_out(easing, duration, &app.volume_flyout.window)
                    }
                }
            }
        });
        state.borrow_mut().timers.insert(kind, source);
    }

    fn cancel_timer(state: &Rc<RefCell<Self>>, kind: FlyoutKind) {
        if let Some(source) = state.borrow_mut().timers.remove(&kind) {
            source.remove();
        }
    }

    /// Pauses the auto-hide while the pointer is over a flyout, and restarts
    /// it when the pointer leaves (upstream polls the same condition).
    fn connect_hover(state: &Rc<RefCell<Self>>) {
        let targets: [(gtk::Window, FlyoutKind); 3] = {
            let app = state.borrow();
            [
                (app.media.window.clone(), FlyoutKind::Media),
                (app.lock.window.clone(), FlyoutKind::LockKeys),
                (app.volume_flyout.window.clone(), FlyoutKind::Volume),
            ]
        };

        for (window, kind) in targets {
            let motion = gtk::EventControllerMotion::new();
            let weak = Rc::downgrade(state);
            motion.connect_enter(move |_, _, _| {
                if let Some(state) = weak.upgrade() {
                    Self::cancel_timer(&state, kind);
                }
            });

            let weak = Rc::downgrade(state);
            motion.connect_leave(move |_| {
                if let Some(state) = weak.upgrade() {
                    let (duration, always, visible) = {
                        let app = state.borrow();
                        let settings = app.settings.borrow();
                        let settings = settings.current();
                        let duration = match kind {
                            FlyoutKind::Media => settings.duration_ms,
                            FlyoutKind::LockKeys => settings.lock_keys_duration_ms,
                            FlyoutKind::Volume => settings.volume_duration_ms,
                        };
                        let always =
                            kind == FlyoutKind::Media && settings.media_flyout_always_display;
                        let visible = match kind {
                            FlyoutKind::Media => app.media.window.is_visible(),
                            FlyoutKind::LockKeys => app.lock.window.is_visible(),
                            FlyoutKind::Volume => app.volume_flyout.window.is_visible(),
                        };
                        (duration.clamp(0, 10_000) as u64, always, visible)
                    };
                    if visible && !always {
                        Self::schedule_hide(&state, kind, duration);
                    }
                }
            });

            window.add_controller(motion);
        }
    }

    fn present_settings(state: &Rc<RefCell<Self>>) {
        {
            let app = state.borrow();
            if let Some(window) = &app.settings_window {
                window.present();
                return;
            }
        }
        let (application, store, events) = {
            let app = state.borrow();
            (
                app.application.clone(),
                app.settings.clone(),
                app.events.clone(),
            )
        };
        let window = settings_window::build(&application, store, events);
        state.borrow_mut().settings_window = Some(window.clone());
        window.present();
    }

    fn on_settings_changed(state: &Rc<RefCell<Self>>) {
        let (settings, save_needed) = {
            let app = state.borrow();
            let store = app.settings.borrow();
            (store.current().clone(), true)
        };

        {
            let app = state.borrow();
            style::refresh(&app.application, &settings);
            app.media.update(app.snapshot.as_ref(), &settings);
            app.volume_flyout.update(app.volume);
        }

        let _ = state
            .borrow()
            .services
            .media
            .try_send(MediaCommand::SetPauseOthers(settings.pause_other_sessions));
        let _ = state
            .borrow()
            .services
            .media
            .try_send(MediaCommand::SetPinnedPlayer(
                settings.pinned_player.clone(),
            ));

        if save_needed {
            Self::schedule_save(state);
        }
    }

    /// Debounced persistence, mirroring the upstream 500 ms delay.
    fn schedule_save(state: &Rc<RefCell<Self>>) {
        if let Some(source) = state.borrow_mut().save_timer.take() {
            source.remove();
        }
        let weak = Rc::downgrade(state);
        let source = glib::timeout_add_local_once(
            Duration::from_millis(crate::settings::SAVE_DEBOUNCE_MS),
            move || {
                if let Some(state) = weak.upgrade() {
                    let store = state.borrow().settings.clone();
                    let store = store.borrow();
                    if let Err(error) = store.save() {
                        log::error!(
                            "cannot save settings to {}: {error}",
                            store.path().display()
                        );
                    }
                }
            },
        );
        state.borrow_mut().save_timer = Some(source);
    }

    /// Shuts the services down before the GTK loop ends.
    pub fn shutdown(state: &Rc<RefCell<Self>>) {
        let (services, store) = {
            let app = state.borrow();
            (app.services.clone(), app.settings.clone())
        };
        if let Err(error) = store.borrow().save() {
            log::error!("final settings save failed: {error}");
        }
        let _ = services.media.try_send(MediaCommand::Shutdown);
        let _ = services.audio.try_send(AudioCommand::Shutdown);
        let _ = services.lock.try_send(LockKeyCommand::Shutdown);
    }

    /// Renders every flyout and the settings window into `directory`.
    ///
    /// This doubles as documentation tooling and as hard evidence that the
    /// GSK renderer really rasterises our widget trees: the capture goes
    /// through `GtkNative`'s `GskRenderer` and `render_texture`, the same
    /// path used for on-screen frames.
    pub fn capture_all(state: Rc<RefCell<Self>>, directory: std::path::PathBuf) {
        {
            let app = state.borrow();
            let settings = app.settings.borrow();
            app.media.update(app.snapshot.as_ref(), settings.current());
            app.volume_flyout.update(Some(VolumeState::new(45, false)));
            app.lock.show_state(LockKeyKind::Caps, true, true);
            app.media.present();
            app.lock.present();
            app.volume_flyout.present();
        }
        Self::present_settings(&state);

        let weak = Rc::downgrade(&state);
        glib::timeout_add_local_once(Duration::from_millis(700), move || {
            let Some(state) = weak.upgrade() else {
                return;
            };
            if let Err(error) = std::fs::create_dir_all(&directory) {
                log::error!("cannot create {}: {error}", directory.display());
            }

            let (targets, application) = {
                let app = state.borrow();
                let targets: Vec<(&str, gtk::Widget)> = vec![
                    ("media-flyout.png", app.media.window.clone().upcast()),
                    ("lock-flyout.png", app.lock.window.clone().upcast()),
                    (
                        "volume-flyout.png",
                        app.volume_flyout.window.clone().upcast(),
                    ),
                ];
                (targets, app.application.clone())
            };

            for (name, widget) in targets {
                let path = directory.join(name);
                match crate::ui::capture::save_widget_png(&widget, &path) {
                    Ok((width, height)) => {
                        println!("wrote {} ({width}x{height})", path.display());
                    }
                    Err(error) => eprintln!("could not capture {name}: {error}"),
                }
            }

            let settings_window = state.borrow().settings_window.clone();
            if let Some(window) = settings_window {
                let path = directory.join("settings.png");
                match crate::ui::capture::save_widget_png(&window, &path) {
                    Ok((width, height)) => {
                        println!("wrote {} ({width}x{height})", path.display());
                    }
                    Err(error) => eprintln!("could not capture settings: {error}"),
                }
            }

            application.quit();
        });
    }

    /// Capability summary used by the D-Bus `Status` method.
    pub fn status_snapshot(state: &Rc<RefCell<Self>>) -> String {
        let app = state.borrow();
        let snapshot = app
            .snapshot
            .as_ref()
            .map(|snapshot| {
                format!(
                    "{{\"player\":\"{}\",\"state\":\"{:?}\",\"title\":\"{}\",\"artist\":\"{}\"}}",
                    snapshot.player.identity.replace('"', "'"),
                    snapshot.state,
                    snapshot.track.title.replace('"', "'"),
                    snapshot.track.artist.replace('"', "'"),
                )
            })
            .unwrap_or_else(|| "null".to_string());
        let media_service_ready = matches!(app.media_status, ServiceStatus::Ready);
        format!(
            "{{\"media\":{snapshot},\"media_service\":{media_service_ready},\"audio\":{},\"lock_keys\":{}}}",
            match &app.audio_status {
                AudioStatus::Ready => "true",
                AudioStatus::Unavailable(_) => "false",
            },
            match &app.lock_support {
                LockKeySupport::Full | LockKeySupport::StateOnly => "true",
                LockKeySupport::Unavailable { .. } => "false",
            }
        )
    }
}

fn kind_is_enabled(settings: &crate::domain::settings::Settings, kind: LockKeyKind) -> bool {
    match kind {
        LockKeyKind::Caps => settings.lock_keys_caps,
        LockKeyKind::Num => settings.lock_keys_num,
        LockKeyKind::Scroll => settings.lock_keys_scroll,
        LockKeyKind::Insert => settings.lock_keys_insert,
    }
}
