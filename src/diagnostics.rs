//! Diagnostics: logging, `--diagnose` output and the capability report.
//!
//! Everything here is evidence-based. The diagnostic report never claims
//! that a subsystem works merely because the code path exists.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use gtk::glib;

use crate::audio::AudioStatus;
use crate::domain::lock_keys::LockKeySupport;
use crate::media::mpris::probe_session_bus;
use crate::platform::{self, RendererConclusion};

/// Minimal stderr + file logger.
///
/// `env_logger` would be another dependency for a feature this small; the
/// app needs timestamps, a level and a stable file for bug reports.
pub struct Logger {
    level: log::LevelFilter,
    file: Option<Mutex<std::fs::File>>,
}

impl Logger {
    /// Installs the global logger.
    ///
    /// `RUST_LOG` overrides the default level so that users can debug a
    /// specific subsystem without a rebuild.
    pub fn install(verbose: bool) {
        let level = std::env::var("RUST_LOG")
            .ok()
            .and_then(|value| value.parse::<log::LevelFilter>().ok())
            .unwrap_or(if verbose {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            });

        let file = Self::log_path().and_then(|path| {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok()?;
            }
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(Mutex::new)
        });

        let logger = Self { level, file };
        if log::set_boxed_logger(Box::new(logger)).is_ok() {
            log::set_max_level(level);
        }
    }

    fn log_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(dirs::state_dir)
            .or_else(dirs::cache_dir)?;
        Some(base.join("fluentflyout-linux").join("fluentflyout.log"))
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let timestamp = glib::DateTime::now_local()
            .ok()
            .and_then(|value| value.format("%H:%M:%S%.3f").ok())
            .map(|value| value.to_string())
            .unwrap_or_default();
        let line = format!(
            "{timestamp} {:<5} {}: {}\n",
            record.level(),
            record.target(),
            record.args()
        );
        let _ = std::io::stderr().write_all(line.as_bytes());
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = file.flush();
        }
    }
}

/// Snapshot of everything the app can and cannot do on this machine.
pub struct CapabilityReport {
    pub version: &'static str,
    pub session: platform::SessionReport,
    pub renderer: platform::RendererReport,
    pub gtk_vulkan_built: Option<bool>,
    pub layer_shell_probable: bool,
    pub mpris_players: Result<Vec<String>, String>,
    pub audio: AudioStatus,
    pub lock_keys: LockKeySupport,
    pub vulkan_devices: Vec<String>,
}

impl CapabilityReport {
    pub fn collect() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            session: platform::SessionReport::probe(),
            renderer: platform::RendererReport::probe(),
            gtk_vulkan_built: platform::gtk_built_with_vulkan(),
            layer_shell_probable: platform::layer_shell_probable(),
            mpris_players: probe_session_bus(),
            audio: AudioStatus::Unavailable("not probed".into()),
            lock_keys: LockKeySupport::Unavailable {
                reason: "not probed".into(),
            },
            vulkan_devices: platform::vulkan_devices(),
        }
    }

    /// Fills in async subsystems after their services have reported.
    pub fn with_services(mut self, audio: AudioStatus, lock_keys: LockKeySupport) -> Self {
        self.audio = audio;
        self.lock_keys = lock_keys;
        self
    }

    /// Renders the human-facing diagnostic report.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "FluentFlyout for Linux {}\n===============================\n\n",
            self.version
        ));

        out.push_str("Session\n-------\n");
        out.push_str(&format!("  backend        : {}\n", self.session.backend));
        if let Some(value) = &self.session.wayland_display {
            out.push_str(&format!("  WAYLAND_DISPLAY: {value}\n"));
        }
        if let Some(value) = &self.session.x11_display {
            out.push_str(&format!("  DISPLAY        : {value}\n"));
        }
        out.push_str(&format!(
            "  desktop        : {}\n",
            self.session.desktop.as_deref().unwrap_or("unknown")
        ));
        out.push_str(&format!(
            "  layer-shell    : {}\n",
            if self.layer_shell_probable {
                "compositor looks compatible"
            } else {
                "not advertised by this desktop"
            }
        ));
        out.push_str(&format!(
            "  window placement: {}\n\n",
            if self.session.can_position_windows(self.layer_shell_probable) {
                "available"
            } else {
                "not permitted by the compositor (windows are placed by the shell)"
            }
        ));

        out.push_str("Rendering\n---------\n");
        out.push_str(&format!(
            "  GSK_RENDERER   : {}\n",
            self.renderer
                .requested
                .as_deref()
                .unwrap_or("(unset: GSK auto-selects)")
        ));
        out.push_str(&format!(
            "  in use         : {}\n",
            self.renderer.conclusion.label()
        ));
        out.push_str(&format!(
            "  libvulkan mapped: {}\n",
            if self.renderer.vulkan_loaded {
                "yes"
            } else {
                "no"
            }
        ));
        out.push_str(&format!(
            "  GTK built with Vulkan: {}\n",
            match self.gtk_vulkan_built {
                Some(true) => "yes",
                Some(false) => "no",
                None => "unknown (pkg-config unavailable)",
            }
        ));
        if self.renderer.conclusion != RendererConclusion::Vulkan
            && self.gtk_vulkan_built == Some(true)
        {
            out.push_str(
                "  note           : Vulkan is available but not selected; set GSK_RENDERER=vulkan\n",
            );
        }
        out.push('\n');

        out.push_str("Vulkan devices\n--------------\n");
        if self.vulkan_devices.is_empty() {
            out.push_str("  (vulkaninfo returned no devices)\n");
        } else {
            for line in &self.vulkan_devices {
                out.push_str(&format!("  {line}\n"));
            }
        }
        out.push('\n');

        out.push_str("Media (MPRIS)\n-------------\n");
        match &self.mpris_players {
            Ok(players) if players.is_empty() => {
                out.push_str("  no MPRIS player is currently registered\n");
            }
            Ok(players) => {
                for player in players {
                    out.push_str(&format!("  {player}\n"));
                }
            }
            Err(error) => out.push_str(&format!("  unavailable: {error}\n")),
        }
        out.push('\n');

        out.push_str("Audio\n-----\n");
        match &self.audio {
            AudioStatus::Ready => out.push_str("  WirePlumber default sink: ready\n"),
            AudioStatus::Unavailable(reason) => {
                out.push_str(&format!("  unavailable: {reason}\n"));
            }
        }
        out.push('\n');

        out.push_str("Lock keys\n---------\n");
        match &self.lock_keys {
            LockKeySupport::Full => out.push_str("  LED state and key events both available\n"),
            LockKeySupport::StateOnly => out.push_str(
                "  LED state available; key events need read access to /dev/input/event*\n",
            ),
            LockKeySupport::Unavailable { reason } => {
                out.push_str(&format!("  unavailable: {reason}\n"));
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> CapabilityReport {
        CapabilityReport {
            version: "0.1.0",
            session: platform::SessionReport {
                backend: "wayland".into(),
                wayland_display: Some("wayland-0".into()),
                x11_display: None,
                desktop: Some("GNOME".into()),
                gnome_shell: true,
            },
            renderer: platform::RendererReport {
                requested: Some("vulkan".into()),
                vulkan_loaded: true,
                conclusion: RendererConclusion::Vulkan,
            },
            gtk_vulkan_built: Some(true),
            layer_shell_probable: false,
            mpris_players: Ok(vec!["org.mpris.MediaPlayer2.spotify".into()]),
            audio: AudioStatus::Ready,
            lock_keys: LockKeySupport::StateOnly,
            vulkan_devices: vec!["deviceName = Test GPU".into()],
        }
    }

    #[test]
    fn report_mentions_every_subsystem() {
        let rendered = report().render();
        for section in [
            "Session",
            "Rendering",
            "Vulkan devices",
            "Media (MPRIS)",
            "Audio",
            "Lock keys",
        ] {
            assert!(rendered.contains(section), "missing section: {section}");
        }
    }

    #[test]
    fn unavailable_subsystems_are_explained_not_hidden() {
        let mut report = report();
        report.audio = AudioStatus::Unavailable("no default sink".into());
        report.lock_keys = LockKeySupport::Unavailable {
            reason: "no /dev/input".into(),
        };
        let rendered = report.render();
        assert!(rendered.contains("no default sink"));
        assert!(rendered.contains("no /dev/input"));
    }

    #[test]
    fn gnome_wayland_reports_placement_limitation() {
        let rendered = report().render();
        assert!(rendered.contains("not permitted by the compositor"));
    }

    #[test]
    fn vulkan_is_reported_from_evidence_not_intention() {
        let mut report = report();
        report.renderer.vulkan_loaded = false;
        report.renderer.conclusion = RendererConclusion::OpenGl;
        let rendered = report.render();
        assert!(rendered.contains("libvulkan mapped: no"));
        assert!(rendered.contains("in use         : OpenGL (ngl)"));
    }
}
