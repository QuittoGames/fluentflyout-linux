//! D-Bus control interface and single-instance handling.
//!
//! A Wayland client cannot register global shortcuts, so the supported way to
//! trigger a flyout from outside the process is a bus call. The same
//! interface also solves single-instance: launching the binary again forwards
//! the requested action to the running instance and exits.
//!
//! Users bind a GNOME custom shortcut to:
//!
//! ```text
//! gdbus call --session --dest org.fluentflyout.Linux \
//!   --object-path /org/fluentflyout/Linux \
//!   --method org.fluentflyout.Linux.ShowMediaFlyout
//! ```

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_channel::Sender;
use zbus::Connection;

use crate::app::{AppEvent, ShowTarget};
use crate::media::MediaCommand;

/// Live status shared with the bus interface.
///
/// The GTK side rewrites this string whenever application state changes, so a
/// bus client can read the current media/audio/lock status without the
/// control thread ever touching GTK state.
pub type StatusSlot = Arc<Mutex<String>>;

/// Creates an empty status slot.
pub fn status_slot() -> StatusSlot {
    Arc::new(Mutex::new(String::from("{}")))
}

/// Well-known bus name owned by the application.
pub const BUS_NAME: &str = "org.fluentflyout.Linux";
/// Object path of the control interface.
pub const OBJECT_PATH: &str = "/org/fluentflyout/Linux";
/// Interface name.
pub const INTERFACE: &str = "org.fluentflyout.Linux";

/// Outcome of the startup handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acquire {
    /// This process owns the name and can serve requests.
    Owned,
    /// Another instance is already running.
    AlreadyRunning,
    /// The session bus is unusable; the app still runs, without IPC.
    Failed(String),
}

struct ControlInterface {
    events: Sender<AppEvent>,
    status: StatusSlot,
}

#[zbus::interface(name = "org.fluentflyout.Linux")]
impl ControlInterface {
    fn show_media_flyout(&self) -> bool {
        self.events
            .try_send(AppEvent::Show(ShowTarget::Media))
            .is_ok()
    }

    fn show_lock_keys(&self) -> bool {
        self.events
            .try_send(AppEvent::Show(ShowTarget::LockKeys))
            .is_ok()
    }

    fn show_volume(&self) -> bool {
        self.events
            .try_send(AppEvent::Show(ShowTarget::Volume))
            .is_ok()
    }

    fn show_settings(&self) -> bool {
        self.events
            .try_send(AppEvent::Show(ShowTarget::Settings))
            .is_ok()
    }

    fn quit(&self) -> bool {
        self.events.try_send(AppEvent::Quit).is_ok()
    }

    fn play_pause(&self) -> bool {
        self.events
            .try_send(AppEvent::MediaControl(MediaCommand::TogglePlayPause))
            .is_ok()
    }

    fn next_track(&self) -> bool {
        self.events
            .try_send(AppEvent::MediaControl(MediaCommand::Next))
            .is_ok()
    }

    fn previous_track(&self) -> bool {
        self.events
            .try_send(AppEvent::MediaControl(MediaCommand::Previous))
            .is_ok()
    }

    #[zbus(property)]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    #[zbus(property)]
    fn capabilities(&self) -> String {
        // A static summary; the live values are in `--diagnose`.
        "media,lock-keys,volume,settings".to_string()
    }

    /// Current runtime status as JSON.
    #[zbus(property)]
    fn status(&self) -> String {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|_| "{}".to_string())
    }
}

/// Attempts to become the primary instance.
///
/// Returns immediately; when the name is taken the caller should call
/// [`forward`] and exit.
pub fn acquire(events: Sender<AppEvent>, status: StatusSlot) -> Acquire {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("dbus-control".into())
        .spawn(move || {
            zbus::block_on(async move {
                let connection = match zbus::conn::Builder::session().build().await {
                    Ok(connection) => connection,
                    Err(error) => {
                        let _ = tx.send(Acquire::Failed(error.to_string()));
                        return;
                    }
                };

                match connection.request_name(BUS_NAME).await {
                    Ok(()) => {}
                    Err(zbus::Error::NameTaken) => {
                        let _ = tx.send(Acquire::AlreadyRunning);
                        return;
                    }
                    Err(error) => {
                        let _ = tx.send(Acquire::Failed(error.to_string()));
                        return;
                    }
                }

                let interface = ControlInterface { events, status };
                if let Err(error) = connection.object_server().at(OBJECT_PATH, interface).await {
                    let _ = tx.send(Acquire::Failed(error.to_string()));
                    return;
                }

                let _ = tx.send(Acquire::Owned);
                // Keep the connection (and therefore the exported object)
                // alive for the lifetime of the process.
                futures_util::future::pending::<()>().await;
            })
        })
        .ok();

    rx.recv_timeout(Duration::from_secs(3))
        .unwrap_or_else(|_| Acquire::Failed("D-Bus control interface timed out".into()))
}

/// Asks the running instance to perform `target`.
pub fn forward(target: ShowTarget) -> Result<(), String> {
    zbus::block_on(async move {
        let connection = Connection::session().await.map_err(|e| e.to_string())?;
        let proxy = zbus::Proxy::new(&connection, BUS_NAME, OBJECT_PATH, INTERFACE)
            .await
            .map_err(|e| e.to_string())?;
        let method = match target {
            ShowTarget::Media => "ShowMediaFlyout",
            ShowTarget::LockKeys => "ShowLockKeys",
            ShowTarget::Volume => "ShowVolume",
            ShowTarget::Settings => "ShowSettings",
        };
        proxy
            .call_method(method, &())
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

/// Asks the running instance to exit.
pub fn request_quit() -> Result<(), String> {
    zbus::block_on(async move {
        let connection = Connection::session().await.map_err(|e| e.to_string())?;
        let proxy = zbus::Proxy::new(&connection, BUS_NAME, OBJECT_PATH, INTERFACE)
            .await
            .map_err(|e| e.to_string())?;
        proxy
            .call_method("Quit", &())
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}
