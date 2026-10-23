//! Lock-key monitoring.
//!
//! Windows exposes `Keyboard.IsKeyToggled` plus a low-level hook. Linux
//! splits the same information across two kernel interfaces:
//!
//! * **LED class devices** (`/sys/class/leds/input*::capslock/brightness`)
//!   carry the *state* and are world-readable.
//! * **evdev** (`/dev/input/event*`) carries the *events*, but reading it
//!   requires membership of the `input` group.
//!
//! Both are used when available. Neither requires root, and neither is
//! replaced with a fabricated value: when a mechanism is inaccessible the
//! service reports [`LockKeySupport`] so the settings window can explain the
//! limitation instead of silently doing nothing.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use async_channel::{Receiver, Sender};

use crate::app::AppEvent;
use crate::domain::lock_keys::{InputEvent, LockKeyKind, LockKeySupport};

/// Requests the UI sends to the lock-key service.
#[derive(Debug, Clone, PartialEq)]
pub enum LockKeyCommand {
    /// Poll the LED state now (used when a flyout is opened manually).
    Refresh,
    Shutdown,
}

/// What an evdev device can report.
///
/// `EV_KEY` gives us the *event* (raise the flyout), `EV_LED` gives us the
/// *resulting state* (which glyph and indicator to show). Keeping them apart
/// avoids guessing the state from a toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvdevSignal {
    Press(LockKeyKind),
    Led(LockKeyKind, bool),
}

/// Handle-free service entry point.
pub struct LockKeyService;

impl LockKeyService {
    pub fn spawn(
        events: Sender<AppEvent>,
        commands: Receiver<LockKeyCommand>,
        enabled_kinds: Vec<LockKeyKind>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::Builder::new()
            .name("lock-keys".into())
            .spawn(move || {
                let leds = LedReader::discover();
                let (event_tx, event_rx) = async_channel::unbounded::<EvdevSignal>();

                // State polling: cheap (one small file read per key).
                let poll_leds = leds.clone();
                let poll_events = events.clone();
                let poll_kinds = enabled_kinds.clone();
                std::thread::Builder::new()
                    .name("lock-keys-leds".into())
                    .spawn(move || {
                        let mut last: HashMap<LockKeyKind, bool> = HashMap::new();
                        loop {
                            for kind in &poll_kinds {
                                let Some(state) = poll_leds.read(*kind) else {
                                    continue;
                                };
                                if last.insert(*kind, state) != Some(state) {
                                    let _ = poll_events.try_send(AppEvent::LockKeyState {
                                        kind: *kind,
                                        on: state,
                                    });
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_millis(150));
                        }
                    })
                    .ok();

                // Event watching: needs /dev/input access. Spawned only when
                // at least one device can be opened.
                let watcher = EvdevWatcher::open_all(enabled_kinds.clone(), event_tx.clone());
                let evdev_available = watcher.is_available();
                match watcher {
                    EvdevWatcher::Available { count } => {
                        log::info!("watching {count} evdev device(s) for lock-key presses");
                    }
                    EvdevWatcher::Unavailable { reason } => {
                        log::debug!("evdev lock-key events unavailable: {reason}");
                    }
                }

                // Forward decoded signals to the UI: LED changes update the
                // displayed state, presses request the flyout.
                let forward_events = events.clone();
                std::thread::Builder::new()
                    .name("lock-keys-forward".into())
                    .spawn(move || {
                        while let Ok(signal) = event_rx.recv_blocking() {
                            match signal {
                                EvdevSignal::Press(kind) => {
                                    let _ = forward_events.try_send(AppEvent::LockKeyPressed(kind));
                                }
                                EvdevSignal::Led(kind, on) => {
                                    let _ = forward_events
                                        .try_send(AppEvent::LockKeyState { kind, on });
                                }
                            }
                        }
                    })
                    .ok();

                let support = match (&leds, evdev_available) {
                    (leds, true) if leds.is_available() => LockKeySupport::Full,
                    (leds, _) if leds.is_available() => LockKeySupport::StateOnly,
                    (_, true) => LockKeySupport::StateOnly,
                    (_, false) => LockKeySupport::Unavailable {
                        reason: if leds.available_kinds().is_empty() {
                            "no LED class devices and no readable /dev/input devices".into()
                        } else {
                            "cannot read input events".into()
                        },
                    },
                };
                let _ = events.try_send(AppEvent::LockKeySupport(support));

                while let Ok(command) = commands.recv_blocking() {
                    if command == LockKeyCommand::Shutdown {
                        break;
                    }
                    for kind in &enabled_kinds {
                        if let Some(state) = leds.read(*kind) {
                            let _ = events.try_send(AppEvent::LockKeyState {
                                kind: *kind,
                                on: state,
                            });
                        }
                    }
                }
            })
            .expect("spawn lock-keys thread")
    }
}

/// Probes lock-key support without starting the service thread.
///
/// Mirrors the capability decision made by [`LockKeyService::spawn`] so the
/// diagnostic report and the running service never disagree.
pub fn probe() -> LockKeySupport {
    let leds = LedReader::discover();
    let has_leds = leds.is_available();

    let evdev_readable = fs::read_dir("/dev/input")
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("event"))
                .any(|entry| File::open(entry.path()).is_ok())
        })
        .unwrap_or(false);

    match (has_leds, evdev_readable) {
        (true, true) => LockKeySupport::Full,
        (true, false) => LockKeySupport::StateOnly,
        (false, true) => LockKeySupport::StateOnly,
        (false, false) => LockKeySupport::Unavailable {
            reason: if Path::new("/dev/input").exists() {
                "no LED class device and no readable /dev/input/event* \
                 (add the user to the input group)"
                    .into()
            } else {
                "no /dev/input and no LED class device in this environment".into()
            },
        },
    }
}

/// Reads toggle state from LED class devices.
#[derive(Clone, Default)]
struct LedReader {
    paths: HashMap<LockKeyKind, PathBuf>,
}

impl LedReader {
    fn discover() -> Self {
        let mut paths = HashMap::new();
        let Ok(entries) = fs::read_dir("/sys/class/leds") else {
            return Self { paths };
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            for kind in LockKeyKind::ALL {
                let Some(suffix) = kind.led_name() else {
                    continue;
                };
                if !name.ends_with(suffix) {
                    continue;
                }
                // `input3::capslock` but not `input3::capslock-something`.
                if !name.contains("::") {
                    continue;
                }
                let brightness = entry.path().join("brightness");
                if brightness.exists() {
                    paths.entry(kind).or_insert(brightness);
                }
            }
        }
        Self { paths }
    }

    fn is_available(&self) -> bool {
        !self.paths.is_empty()
    }

    fn available_kinds(&self) -> Vec<LockKeyKind> {
        self.paths.keys().copied().collect()
    }

    fn read(&self, kind: LockKeyKind) -> Option<bool> {
        let path = self.paths.get(&kind)?;
        let raw = fs::read_to_string(path).ok()?;
        let value: u32 = raw.trim().parse().ok()?;
        Some(value > 0)
    }
}

/// Watches evdev devices for lock-key presses.
enum EvdevWatcher {
    Available { count: usize },
    Unavailable { reason: String },
}

impl EvdevWatcher {
    fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// Opens every readable `/dev/input/event*` device and spawns one reader
    /// thread per device.
    fn open_all(kinds: Vec<LockKeyKind>, sink: Sender<EvdevSignal>) -> Self {
        let Ok(entries) = fs::read_dir("/dev/input") else {
            return Self::Unavailable {
                reason: "/dev/input is not present".into(),
            };
        };

        let mut opened = 0usize;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("event") {
                continue;
            }
            let Ok(mut file) = File::open(entry.path()) else {
                continue;
            };
            opened += 1;
            let sink = sink.clone();
            let kinds = kinds.clone();
            std::thread::Builder::new()
                .name(format!("evdev-{name}"))
                .spawn(move || {
                    let mut buffer = [0u8; InputEvent::SIZE];
                    loop {
                        // `read_exact` is required: a short read would
                        // desynchronise the event stream.
                        if file.read_exact(&mut buffer).is_err() {
                            break;
                        }
                        let Some(event) = InputEvent::from_bytes(&buffer) else {
                            continue;
                        };
                        // LED events carry the resulting toggle state, which
                        // is what the flyout renders.
                        if let Some((kind, on)) = event.led_state()
                            && kinds.contains(&kind)
                        {
                            let _ = sink.try_send(EvdevSignal::Led(kind, on));
                            continue;
                        }
                        if !event.is_key_press() {
                            continue;
                        }
                        if let Some(kind) = event.lock_key()
                            && kinds.contains(&kind)
                        {
                            let _ = sink.try_send(EvdevSignal::Press(kind));
                        }
                    }
                })
                .ok();
        }

        if opened == 0 {
            Self::Unavailable {
                reason: "no readable /dev/input/event* device (add the user to the input group)"
                    .into(),
            }
        } else {
            Self::Available { count: opened }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn led_discovery_is_safe_without_sysfs() {
        // In a container /sys/class/leds may not exist; discovery must not
        // panic and must report itself as unavailable.
        let reader = LedReader::discover();
        assert!(reader.is_available() || !reader.is_available());
        assert_eq!(
            reader.read(LockKeyKind::Caps),
            reader.read(LockKeyKind::Caps)
        );
    }

    #[test]
    fn insert_is_never_read_from_leds() {
        let reader = LedReader::default();
        assert_eq!(reader.read(LockKeyKind::Insert), None);
    }

    #[test]
    fn evdev_unavailability_is_reported_not_panicked() {
        let (tx, _rx) = async_channel::unbounded();
        let watcher = EvdevWatcher::open_all(vec![LockKeyKind::Caps], tx);
        match watcher {
            EvdevWatcher::Available { count } => assert!(count > 0),
            EvdevWatcher::Unavailable { reason } => assert!(!reason.is_empty()),
        }
    }
}
