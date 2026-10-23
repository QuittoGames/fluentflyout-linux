//! Runtime capability detection.
//!
//! The Windows original can assume a desktop it fully controls. Here every
//! capability has to be *probed*, and the result must be reported honestly:
//! a feature that silently does nothing is worse than one that explains
//! itself.

use std::path::Path;
use std::process::Command;

/// Graphics backend actually in use.
///
/// GTK 4.22 removed the public renderer accessor, so the conclusion is drawn
/// from two pieces of evidence: the requested renderer (`GSK_RENDERER`) and
/// the shared objects the process has actually mapped. A mapped
/// `libvulkan.so` means GTK loaded Vulkan for this process, which is only
/// true when the GSK Vulkan renderer was selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererReport {
    /// Value of `GSK_RENDERER`, when set.
    pub requested: Option<String>,
    /// `true` when `libvulkan` is among the process's mapped libraries.
    pub vulkan_loaded: bool,
    /// Human-readable conclusion.
    pub conclusion: RendererConclusion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererConclusion {
    Vulkan,
    OpenGl,
    Cairo,
    Unknown,
}

impl RendererConclusion {
    pub fn label(self) -> &'static str {
        match self {
            Self::Vulkan => "Vulkan",
            Self::OpenGl => "OpenGL (ngl)",
            Self::Cairo => "Cairo (software)",
            Self::Unknown => "unknown",
        }
    }
}

impl RendererReport {
    /// Reads the evidence available to the current process.
    pub fn probe() -> Self {
        let requested = std::env::var("GSK_RENDERER")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());

        let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
        let vulkan_loaded = maps.contains("libvulkan.so");
        let gl_loaded = maps.contains("libGL.so")
            || maps.contains("libGLX.so")
            || maps.contains("libEGL.so")
            || maps.contains("libgallium");

        let conclusion = if vulkan_loaded {
            RendererConclusion::Vulkan
        } else if gl_loaded {
            RendererConclusion::OpenGl
        } else if requested.as_deref() == Some("cairo") {
            RendererConclusion::Cairo
        } else {
            RendererConclusion::Unknown
        };

        Self {
            requested,
            vulkan_loaded,
            conclusion,
        }
    }
}

/// Session / display-server facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReport {
    /// `wayland`, `x11` or `unknown`, derived from the environment GDK uses.
    pub backend: String,
    pub wayland_display: Option<String>,
    pub x11_display: Option<String>,
    pub desktop: Option<String>,
    /// `true` when running under GNOME Shell, which does not implement
    /// wlr-layer-shell and does not let clients position their own windows.
    pub gnome_shell: bool,
}

impl SessionReport {
    pub fn probe() -> Self {
        let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
        let x11_display = std::env::var("DISPLAY").ok();
        // GDK prefers Wayland when both are set, and so do we.
        let backend = if wayland_display.is_some() {
            "wayland"
        } else if x11_display.is_some() {
            "x11"
        } else {
            "unknown"
        };

        let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
        let gnome_shell = desktop
            .as_deref()
            .is_some_and(|value| value.to_ascii_uppercase().contains("GNOME"));

        Self {
            backend: backend.into(),
            wayland_display,
            x11_display,
            desktop,
            gnome_shell,
        }
    }

    /// Whether a client is allowed to choose its own window position.
    ///
    /// Wayland deliberately forbids it; X11 allows it, and wlroots/KDE
    /// compositors allow it through the layer-shell protocol.
    pub fn can_position_windows(&self, layer_shell: bool) -> bool {
        match self.backend.as_str() {
            "x11" => true,
            // Mutter implements neither client-side placement nor
            // wlr-layer-shell, so even the layer-shell path cannot place a
            // flyout on GNOME Wayland.
            "wayland" => layer_shell && !self.gnome_shell,
            // Unknown backend: assume the conservative answer.
            _ => false,
        }
    }
}

/// Whether the `zwlr_layer_shell_v1` protocol is exported on the socket.
///
/// Without wayland-scanner generated bindings the protocol cannot be
/// inspected directly; the layer-shell crate performs its own handshake, so
/// this is only a cheap pre-check used before attempting it.
pub fn layer_shell_probable() -> bool {
    // The protocol is advertised by wlroots-based compositors and KWin.
    // GNOME/Mutter does not implement it.
    match std::env::var("XDG_CURRENT_DESKTOP") {
        Ok(desktop) => {
            let desktop = desktop.to_ascii_uppercase();
            desktop.contains("SWAY")
                || desktop.contains("HYPRLAND")
                || desktop.contains("KDE")
                || desktop.contains("RIVER")
                || desktop.contains("WAYFIRE")
                || desktop.contains("LABWC")
        }
        Err(_) => false,
    }
}

/// Vulkan device summary produced by `vulkaninfo --summary`, when available.
///
/// Kept as raw text lines: the goal is evidence for diagnostics, not parsing
/// every ICD field.
pub fn vulkan_devices() -> Vec<String> {
    let output = match Command::new("vulkaninfo").arg("--summary").output() {
        Ok(output) if output.status.success() => output,
        _ => return vec!["vulkaninfo not available".into()],
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with("deviceName")
                || line.starts_with("deviceType")
                || line.starts_with("apiVersion")
                || line.starts_with("GPU id")
        })
        .map(str::to_string)
        .collect()
}

/// `true` when the GTK build in use was compiled with the Vulkan renderer.
///
/// Checked against the shared object itself rather than assumed from the
/// runtime version.
pub fn gtk_built_with_vulkan() -> Option<bool> {
    // Ask pkg-config for the library directory, then inspect the ELF.
    let output = Command::new("pkg-config")
        .args(["--variable=libdir", "gtk4"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let libdir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let candidate = Path::new(&libdir).join("libgtk-4.so.1");
    let bytes = std::fs::read(&candidate).ok()?;
    // Cheap substring scan over the dynamic symbol/string tables.
    Some(contains(&bytes, b"libvulkan.so.1"))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substring_scan_handles_empty_and_missing_needles() {
        assert!(!contains(b"", b"x"));
        assert!(!contains(b"abc", b"abcd"));
        assert!(contains(b"libvulkan.so.1", b"libvulkan.so.1"));
        assert!(contains(b"--libvulkan.so.1--", b"libvulkan.so.1"));
    }

    #[test]
    fn session_probe_never_panics_without_a_display() {
        let report = SessionReport::probe();
        assert!(matches!(
            report.backend.as_str(),
            "wayland" | "x11" | "unknown"
        ));
    }

    #[test]
    fn x11_sessions_may_position_windows() {
        let report = SessionReport {
            backend: "x11".into(),
            wayland_display: None,
            x11_display: Some(":0".into()),
            desktop: None,
            gnome_shell: false,
        };
        assert!(report.can_position_windows(false));
    }

    #[test]
    fn gnome_wayland_cannot_position_without_layer_shell() {
        let report = SessionReport {
            backend: "wayland".into(),
            wayland_display: Some("wayland-0".into()),
            x11_display: None,
            desktop: Some("GNOME".into()),
            gnome_shell: true,
        };
        assert!(!report.can_position_windows(false));
        assert!(!report.can_position_windows(true));
    }

    #[test]
    fn wlroots_wayland_can_position_with_layer_shell() {
        let report = SessionReport {
            backend: "wayland".into(),
            wayland_display: Some("wayland-1".into()),
            x11_display: None,
            desktop: Some("sway".into()),
            gnome_shell: false,
        };
        assert!(report.can_position_windows(true));
        assert!(!report.can_position_windows(false));
    }

    #[test]
    fn renderer_probe_reads_the_current_process() {
        // The test binary never loads GTK, so Vulkan must not be mapped
        // unless the harness itself pulled it in.
        let report = RendererReport::probe();
        assert_eq!(
            report.vulkan_loaded,
            report.conclusion == RendererConclusion::Vulkan
        );
    }
}
