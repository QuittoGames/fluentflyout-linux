//! Binary entry point.
//!
//! Responsibilities, in order:
//!
//! 1. parse the command line by hand (GTK must not see our flags);
//! 2. pick a renderer before GTK initialises, preferring Vulkan when the
//!    system can actually provide it;
//! 3. hand a requested action to an already-running instance, if any;
//! 4. start the background services, then the GTK application.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use async_channel::Sender;
use gtk::glib;

use fluentflyout_linux::app::{App, AppEvent, Services, ShowTarget, control};
use fluentflyout_linux::audio::{AudioCommand, AudioService};
use fluentflyout_linux::diagnostics::{CapabilityReport, Logger};
use fluentflyout_linux::domain::lock_keys::LockKeyKind;
use fluentflyout_linux::lock_keys::{LockKeyCommand, LockKeyService};
use fluentflyout_linux::media::{MediaCommand, MediaService};
use fluentflyout_linux::settings::SettingsStore;
use fluentflyout_linux::{audio, lock_keys};

const HELP: &str = "\
FluentFlyout for Linux — native media, lock-key and volume flyouts

USAGE:
    fluentflyout [OPTIONS]

OPTIONS:
    --show-media        Show the media flyout (forwards to a running instance)
    --show-lock         Show the lock-key flyout
    --show-volume       Show the volume flyout
    --settings          Open the settings window
    --diagnose          Print a capability report and exit
    --screenshot <DIR>  Render every flyout to PNG files in DIR and exit
    --hidden            Start without opening any window (default)
    --config <PATH>     Use an alternative settings file
    --verbose           Enable debug logging (also: RUST_LOG)
    --version           Print the version and exit
    -h, --help          Print this help and exit

TRIGGERING FLYOUTS FROM THE DESKTOP
    A Wayland client cannot register global shortcuts. Bind a GNOME custom
    shortcut (Settings -> Keyboard -> Custom Shortcuts) to:

        gdbus call --session --dest org.fluentflyout.Linux \\
          --object-path /org/fluentflyout/Linux \\
          --method org.fluentflyout.Linux.ShowMediaFlyout

    The application also opens the flyout automatically when the active
    player changes track or playback state (see Settings -> Media).
";

/// Parsed command line.
#[derive(Debug, Clone, Default)]
struct Args {
    diagnose: bool,
    verbose: bool,
    version: bool,
    help: bool,
    show: Option<ShowTarget>,
    config: Option<PathBuf>,
    screenshot: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--diagnose" => args.diagnose = true,
            "--verbose" | "-v" => args.verbose = true,
            "--version" | "-V" => args.version = true,
            "--help" | "-h" => args.help = true,
            "--hidden" => {}
            "--show-media" => args.show = Some(ShowTarget::Media),
            "--show-lock" => args.show = Some(ShowTarget::LockKeys),
            "--show-volume" => args.show = Some(ShowTarget::Volume),
            "--settings" => args.show = Some(ShowTarget::Settings),
            "--config" => {
                let path = iter
                    .next()
                    .ok_or_else(|| "--config requires a path".to_string())?;
                args.config = Some(PathBuf::from(path));
            }
            "--screenshot" => {
                let path = iter
                    .next()
                    .ok_or_else(|| "--screenshot requires a directory".to_string())?;
                args.screenshot = Some(PathBuf::from(path));
            }
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
    }
    Ok(args)
}

fn main() -> std::process::ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("error: {error}\n\n{HELP}");
            return std::process::ExitCode::from(2);
        }
    };

    if args.help {
        print!("{HELP}");
        return std::process::ExitCode::SUCCESS;
    }
    if args.version {
        println!(
            "fluentflyout {} (GTK {}.{}.{})",
            env!("CARGO_PKG_VERSION"),
            gtk::major_version(),
            gtk::minor_version(),
            gtk::micro_version()
        );
        return std::process::ExitCode::SUCCESS;
    }

    Logger::install(args.verbose);
    prefer_vulkan();

    // Diagnostics are process-local: they must not be forwarded to a running
    // instance (which would print nothing) and must not claim the
    // single-instance bus name.
    if args.diagnose {
        return run_diagnose_standalone();
    }

    // A running instance owns the D-Bus name, so the requested action is
    // forwarded there instead of starting a second copy.
    if let Some(target) = args.show
        && let Err(error) = control::forward(target)
    {
        log::debug!("no running instance to forward to: {error}");
    } else if args.show.is_some() {
        return std::process::ExitCode::SUCCESS;
    }

    let (event_tx, event_rx) = async_channel::unbounded::<AppEvent>();
    let status_slot = control::status_slot();

    match control::acquire(event_tx.clone(), status_slot.clone()) {
        control::Acquire::Owned => log::info!("D-Bus control interface ready"),
        control::Acquire::AlreadyRunning => {
            // Another instance won the race after our forward attempt.
            if let Some(target) = args.show {
                let _ = control::forward(target);
            }
            return std::process::ExitCode::SUCCESS;
        }
        control::Acquire::Failed(reason) => {
            log::warn!("D-Bus control interface unavailable: {reason}");
            log::warn!("flyouts can still be shown from the settings window");
        }
    }

    let store = Rc::new(RefCell::new(SettingsStore::load(
        args.config
            .clone()
            .unwrap_or_else(SettingsStore::default_path),
    )));

    let services = spawn_services(&store, event_tx.clone());

    let application = adw::Application::builder()
        .application_id("io.github.quitto.FluentFlyout")
        .build();

    let app_slot: Rc<RefCell<Option<Rc<RefCell<App>>>>> = Rc::new(RefCell::new(None));
    let started = Rc::new(Cell::new(false));

    {
        let app_slot = app_slot.clone();
        let started = started.clone();
        let store = store.clone();
        let services = services.clone();
        let event_tx = event_tx.clone();
        let args = args.clone();
        application.connect_activate(move |application| {
            // Activation can fire again when a second launch is dispatched;
            // only the first activation builds the UI.
            if started.replace(true) {
                return;
            }

            let state = App::new(
                application.clone(),
                store.clone(),
                services.clone(),
                event_tx.clone(),
                status_slot.clone(),
            );
            App::spawn_pump(state.clone(), event_rx.clone());
            *app_slot.borrow_mut() = Some(state.clone());

            if let Some(directory) = args.screenshot.clone() {
                App::capture_all(state, directory);
            } else if let Some(target) = args.show {
                let _ = event_tx.try_send(AppEvent::Show(target));
            }
        });
    }

    {
        let app_slot = app_slot.clone();
        application.connect_shutdown(move |_| {
            if let Some(state) = app_slot.borrow().clone() {
                App::shutdown(&state);
            }
        });
    }

    // GTK parses its own arguments; ours are already consumed.
    // `glib::ExitCode` is a distinct type from `std::process::ExitCode`.
    application.run_with_args(&["fluentflyout"]).into()
}

/// Starts the three background services and returns their command channels.
fn spawn_services(store: &Rc<RefCell<SettingsStore>>, events: Sender<AppEvent>) -> Services {
    let (media_tx, media_rx) = async_channel::unbounded::<MediaCommand>();
    let (audio_tx, audio_rx) = async_channel::unbounded::<AudioCommand>();
    let (lock_tx, lock_rx) = async_channel::unbounded::<LockKeyCommand>();

    MediaService::spawn(events.clone(), media_rx);
    AudioService::spawn(events.clone(), audio_rx);

    let settings = store.borrow().current().clone();
    let kinds = enabled_lock_keys(&settings);
    LockKeyService::spawn(events, lock_rx, kinds);

    let _ = media_tx.try_send(MediaCommand::SetPauseOthers(settings.pause_other_sessions));
    let _ = media_tx.try_send(MediaCommand::SetPinnedPlayer(
        settings.pinned_player.clone(),
    ));

    Services {
        media: media_tx,
        audio: audio_tx,
        lock: lock_tx,
    }
}

fn enabled_lock_keys(
    settings: &fluentflyout_linux::domain::settings::Settings,
) -> Vec<LockKeyKind> {
    let mut kinds = Vec::new();
    if settings.lock_keys_enabled {
        if settings.lock_keys_caps {
            kinds.push(LockKeyKind::Caps);
        }
        if settings.lock_keys_num {
            kinds.push(LockKeyKind::Num);
        }
        if settings.lock_keys_scroll {
            kinds.push(LockKeyKind::Scroll);
        }
        if settings.lock_keys_insert {
            kinds.push(LockKeyKind::Insert);
        }
    }
    kinds
}

/// Prints the capability report after the compositor has had time to realize
/// a window.
///
/// The delay matters: renderer evidence comes from the libraries the process
/// has actually mapped, and GSK only loads them once a surface exists.
fn run_diagnose_standalone() -> std::process::ExitCode {
    let application = adw::Application::builder()
        .application_id("io.github.quitto.FluentFlyout.Diagnose")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    application.connect_activate(|application| {
        let window = gtk::Window::builder()
            .application(application)
            .title("FluentFlyout diagnostics")
            .default_width(360)
            .default_height(160)
            .build();

        let label = gtk::Label::new(Some("Collecting renderer evidence…"));
        window.set_child(Some(&label));
        window.present();

        let application = application.clone();
        glib::timeout_add_local_once(Duration::from_millis(700), move || {
            let report =
                CapabilityReport::collect().with_services(audio::probe(), lock_keys::probe());
            println!("{}", report.render());
            application.quit();
        });
    });

    application.run_with_args(&["fluentflyout-diagnose"]);
    std::process::ExitCode::SUCCESS
}

/// Selects the Vulkan GSK renderer when the host can provide it.
///
/// Vulkan is the explicit graphics target of this port. Setting
/// `GSK_RENDERER` is the only supported way to request a renderer in GTK 4.22
/// (the programmatic API was removed), and it must happen before GTK
/// initialises.
///
/// The value is not forced when the loader or any ICD manifest is missing:
/// in that case GTK would fail to create a renderer at all, so the app keeps
/// GSK's automatic selection and the diagnostics report the real outcome.
fn prefer_vulkan() {
    if std::env::var_os("GSK_RENDERER").is_some() {
        return;
    }
    if !vulkan_available() {
        log::warn!("Vulkan loader or ICD manifests not found; letting GSK choose a renderer");
        return;
    }

    // SAFETY: no other thread has been started yet (the services are spawned
    // after this call) and GTK has not been initialised, so no concurrent
    // reader of the environment exists.
    unsafe {
        std::env::set_var("GSK_RENDERER", "vulkan");
    }
    log::info!("requesting the GSK Vulkan renderer");
}

fn vulkan_available() -> bool {
    const LOADERS: [&str; 4] = [
        "/usr/lib64/libvulkan.so.1",
        "/usr/lib/libvulkan.so.1",
        "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
        "/usr/lib/aarch64-linux-gnu/libvulkan.so.1",
    ];
    if !LOADERS.iter().any(|path| Path::new(path).exists()) {
        return false;
    }
    std::fs::read_dir("/usr/share/vulkan/icd.d")
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
            })
        })
        .unwrap_or(false)
}

/// Documents the channel direction used by `App::spawn_pump`; only the test
/// module needs the alias, and only tests import `Receiver`.
#[cfg(test)]
type EventReceiver = async_channel::Receiver<AppEvent>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vulkan_probe_never_panics() {
        // Whether Vulkan exists is environment-specific; the probe must only
        // answer, never fail.
        let _ = vulkan_available();
    }

    #[test]
    fn enabled_lock_keys_follows_settings() {
        let mut settings = fluentflyout_linux::domain::settings::Settings::default();
        assert_eq!(enabled_lock_keys(&settings).len(), 4);

        settings.lock_keys_insert = false;
        assert!(!enabled_lock_keys(&settings).contains(&LockKeyKind::Insert));

        settings.lock_keys_enabled = false;
        assert!(enabled_lock_keys(&settings).is_empty());
    }

    #[test]
    fn event_receiver_alias_matches_the_pump_signature() {
        let (tx, rx): (Sender<AppEvent>, EventReceiver) = async_channel::unbounded();
        tx.try_send(AppEvent::Quit).expect("send");
        assert!(matches!(rx.try_recv(), Ok(AppEvent::Quit)));
    }
}
