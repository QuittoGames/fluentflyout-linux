//! Volume domain model.
//!
//! FluentFlyout's volume flyout shows the master output level and lets the
//! user change it. On Linux the master output is a PipeWire/WirePlumber
//! concept, so the app talks to WirePlumber and keeps a small, testable model
//! here.

/// A snapshot of the default audio sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeState {
    /// 0..=100, already converted from the 0.0..=1.0 value WirePlumber uses.
    pub percent: u8,
    pub muted: bool,
}

impl VolumeState {
    pub const fn new(percent: u8, muted: bool) -> Self {
        Self { percent, muted }
    }

    /// Percentage to display: zero when muted, so the UI never shows a level
    /// that is not actually audible.
    pub fn display_percent(self) -> u8 {
        if self.muted { 0 } else { self.percent }
    }
}

/// A requested change to the master volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VolumeCommand {
    /// Absolute level as a fraction of full scale.
    Set(f64),
    /// Relative step, e.g. `+0.05` or `-0.05`.
    Adjust(f64),
    ToggleMute,
}

/// Parses `wpctl get-volume` output.
///
/// The command prints `Volume: 0.45` and appends ` [MUTED]` when muted.
/// Parsing is deliberately tolerant: a missing percentage yields `None`
/// instead of guessing, so the caller can report the sink as unavailable.
pub fn parse_wpctl_volume(output: &str) -> Option<VolumeState> {
    let line = output.lines().find(|line| line.contains("Volume:"))?;
    let value = line.split("Volume:").nth(1)?;
    let number = value.split_whitespace().next()?;
    let fraction: f64 = number.trim().parse().ok()?;
    if !fraction.is_finite() {
        return None;
    }
    let percent = (fraction * 100.0).round().clamp(0.0, 100.0) as u8;
    Some(VolumeState::new(percent, value.contains("[MUTED]")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_normal_level() {
        let state = parse_wpctl_volume("Volume: 0.45\n").expect("level");
        assert_eq!(state, VolumeState::new(45, false));
    }

    #[test]
    fn parses_a_muted_level() {
        let state = parse_wpctl_volume("Volume: 0.45 [MUTED]\n").expect("level");
        assert_eq!(state, VolumeState::new(45, true));
        assert_eq!(state.display_percent(), 0);
    }

    #[test]
    fn parses_surrounding_noise_from_wireplumber() {
        // wpctl can emit warnings before the value line.
        let output = "Some warning\nVolume: 1.00\n";
        assert_eq!(
            parse_wpctl_volume(output),
            Some(VolumeState::new(100, false))
        );
    }

    #[test]
    fn missing_sink_returns_none() {
        assert_eq!(parse_wpctl_volume(""), None);
        assert_eq!(parse_wpctl_volume("No sink found"), None);
        assert_eq!(parse_wpctl_volume("Volume: not-a-number"), None);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        assert_eq!(
            parse_wpctl_volume("Volume: 1.75"),
            Some(VolumeState::new(100, false))
        );
        assert_eq!(
            parse_wpctl_volume("Volume: -0.2"),
            Some(VolumeState::new(0, false))
        );
    }

    #[test]
    fn nan_is_rejected() {
        assert_eq!(parse_wpctl_volume("Volume: NaN"), None);
    }

    #[test]
    fn unmuted_display_percent_is_the_real_level() {
        assert_eq!(VolumeState::new(70, false).display_percent(), 70);
    }
}
