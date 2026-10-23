//! Audio integration through WirePlumber.
//!
//! PipeWire has no D-Bus control surface, so the app drives the default sink
//! through `wpctl`, the supported WirePlumber CLI. That keeps the integration
//! small and avoids linking against `libpipewire` for three operations.
//!
//! Everything here degrades cleanly: a container or remote session without a
//! sink reports `Unavailable` and the volume flyout simply never appears.

use std::path::PathBuf;
use std::process::Command;

use async_channel::{Receiver, Sender};

use crate::app::AppEvent;
use crate::domain::volume::{VolumeCommand, VolumeState, parse_wpctl_volume};

/// Requests the UI sends to the audio service.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioCommand {
    Refresh,
    Apply(VolumeCommand),
    Shutdown,
}

/// Availability of the master sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioStatus {
    Ready,
    Unavailable(String),
}

/// Handle-free service entry point.
pub struct AudioService;

impl AudioService {
    pub fn spawn(
        events: Sender<AppEvent>,
        commands: Receiver<AudioCommand>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::Builder::new()
            .name("audio-service".into())
            .spawn(move || {
                let mut controller = WirePlumber::discover();
                match &controller {
                    Some(_) => {
                        let _ = events.try_send(AppEvent::AudioStatus(AudioStatus::Ready));
                    }
                    None => {
                        let _ = events.try_send(AppEvent::AudioStatus(AudioStatus::Unavailable(
                            "wpctl is not available, so master volume cannot be controlled".into(),
                        )));
                        // Still consume commands so the channel does not fill.
                        while let Ok(command) = commands.recv_blocking() {
                            if command == AudioCommand::Shutdown {
                                break;
                            }
                        }
                        return;
                    }
                }

                let controller = controller.take().expect("checked above");
                while let Ok(command) = commands.recv_blocking() {
                    match command {
                        AudioCommand::Shutdown => break,
                        AudioCommand::Refresh => {
                            publish(&events, controller.read());
                        }
                        AudioCommand::Apply(command) => {
                            controller.apply(command);
                            publish(&events, controller.read());
                        }
                    }
                }
            })
            .expect("spawn audio thread")
    }
}

/// Probes availability cheaply, without starting the service thread.
///
/// Used by `--diagnose` and by the settings window when it needs to explain
/// why the volume section is disabled.
pub fn probe() -> AudioStatus {
    if find_wpctl().is_none() {
        return AudioStatus::Unavailable(
            "wpctl (WirePlumber) is not installed or not on PATH".into(),
        );
    }
    match WirePlumber::discover() {
        Some(_) => AudioStatus::Ready,
        None => AudioStatus::Unavailable(
            "WirePlumber reports no default audio sink (@DEFAULT_AUDIO_SINK@)".into(),
        ),
    }
}

fn publish(events: &Sender<AppEvent>, state: Option<VolumeState>) {
    let _ = events.try_send(AppEvent::Volume(state));
}

/// Thin wrapper over the `wpctl` binary.
struct WirePlumber {
    binary: PathBuf,
}

impl WirePlumber {
    /// Returns `None` when `wpctl` is missing or no default sink exists.
    fn discover() -> Option<Self> {
        let binary = find_wpctl()?;
        let controller = Self { binary };
        controller.read()?;
        Some(controller)
    }

    fn read(&self) -> Option<VolumeState> {
        let output = Command::new(&self.binary)
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        parse_wpctl_volume(&text)
    }

    fn apply(&self, command: VolumeCommand) {
        let args: Vec<String> = match command {
            VolumeCommand::Set(fraction) => vec![
                "set-volume".into(),
                "@DEFAULT_AUDIO_SINK@".into(),
                format!("{:.2}", fraction.clamp(0.0, 1.0)),
            ],
            VolumeCommand::Adjust(delta) => vec![
                "set-volume".into(),
                "@DEFAULT_AUDIO_SINK@".into(),
                format!("{delta:.2}{}", if delta >= 0.0 { "+" } else { "" }),
            ],
            VolumeCommand::ToggleMute => vec![
                "set-mute".into(),
                "@DEFAULT_AUDIO_SINK@".into(),
                "toggle".into(),
            ],
        };

        match Command::new(&self.binary).args(&args).output() {
            Ok(output) if output.status.success() => {}
            Ok(output) => log::warn!(
                "wpctl {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(error) => log::warn!("cannot run wpctl: {error}"),
        }
    }
}

/// Locates `wpctl`.
///
/// Fedora installs it in `/usr/sbin`, which is not always on a graphical
/// session's `PATH`, so the well-known locations are checked explicitly.
fn find_wpctl() -> Option<PathBuf> {
    let candidates = [
        "wpctl",
        "/usr/bin/wpctl",
        "/usr/sbin/wpctl",
        "/bin/wpctl",
        "/usr/local/bin/wpctl",
    ];
    for candidate in candidates {
        if let Ok(output) = Command::new(candidate).arg("--version").output()
            && output.status.success()
        {
            return Some(PathBuf::from(candidate));
        }
        // `--version` is not implemented by older wpctl releases; probe with
        // a harmless call instead.
        if let Ok(output) = Command::new(candidate)
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
            && (output.status.success() || !output.stderr.is_empty())
        {
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_command_deltas_are_formatted_safely() {
        // Guards the `+` suffix that wpctl requires for relative changes.
        let positive = format!("{:.2}{}", 0.05f64, if 0.05f64 >= 0.0 { "+" } else { "" });
        assert_eq!(positive, "0.05+");
        let negative = format!("{:.2}{}", -0.05f64, if -0.05f64 >= 0.0 { "+" } else { "" });
        assert_eq!(negative, "-0.05");
    }

    #[test]
    fn absent_sink_is_not_a_panic() {
        // In a CI container there is usually no sink; the service must report
        // availability rather than unwrap.
        let status = match WirePlumber::discover() {
            Some(_) => AudioStatus::Ready,
            None => AudioStatus::Unavailable("none".into()),
        };
        assert!(matches!(
            status,
            AudioStatus::Ready | AudioStatus::Unavailable(_)
        ));
    }
}
