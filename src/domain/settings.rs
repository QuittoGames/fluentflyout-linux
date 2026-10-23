//! Persistent user settings.
//!
//! The field set mirrors the upstream `UserSettings` model, restricted to the
//! behaviour that has a real Linux equivalent. Every field carries the
//! upstream default so that a settings file written by an older build keeps
//! working (`#[serde(default)]` at the container level fills missing keys).
//!
//! Deliberate omissions, all documented in `docs/rea-report.md`:
//!
//! * taskbar widget, visualizer and their styling — require Windows taskbar
//!   embedding; no supported Wayland equivalent;
//! * `NextUp*` — MPRIS exposes no reliable queue look-ahead;
//! * acrylic/backdrop blur opacity — no client-side backdrop blur on Wayland;
//! * per-monitor selection and language override — see the spec.

use serde::{Deserialize, Serialize};

use crate::domain::geometry::FlyoutAnchor;

/// Theme preference. The upstream integer encoding (`0/1/2`) is preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppTheme {
    #[default]
    System,
    Light,
    Dark,
}

impl AppTheme {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn from_index(index: i64) -> Self {
        match index {
            1 => Self::Light,
            2 => Self::Dark,
            _ => Self::System,
        }
    }

    pub fn index(self) -> i64 {
        match self {
            Self::System => 0,
            Self::Light => 1,
            Self::Dark => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Follow system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// `flyout_animation_easing_style` from upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EasingStyle {
    Linear,
    Sine,
    #[default]
    Quadratic,
    Cubic,
}

impl EasingStyle {
    pub const ALL: [Self; 4] = [Self::Linear, Self::Sine, Self::Quadratic, Self::Cubic];

    pub fn from_index(index: i64) -> Self {
        match index {
            0 => Self::Linear,
            1 => Self::Sine,
            3 => Self::Cubic,
            _ => Self::Quadratic,
        }
    }

    pub fn index(self) -> i64 {
        match self {
            Self::Linear => 0,
            Self::Sine => 1,
            Self::Quadratic => 2,
            Self::Cubic => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Sine => "Sine",
            Self::Quadratic => "Quadratic",
            Self::Cubic => "Cubic",
        }
    }
}

/// Background treatment for the media flyout (`media_flyout_background_blur`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackdropStyle {
    /// Flat translucent surface (upstream default).
    #[default]
    Solid,
    /// Soft blurred artwork anchored to the left.
    GlowLeft,
    /// Artwork bloom rising from the bottom edge.
    GlowBottom,
    /// Dimmed full-bleed artwork.
    FullBleed,
}

impl BackdropStyle {
    pub const ALL: [Self; 4] = [
        Self::Solid,
        Self::GlowLeft,
        Self::GlowBottom,
        Self::FullBleed,
    ];

    pub fn from_index(index: i64) -> Self {
        match index {
            1 => Self::GlowLeft,
            2 => Self::GlowBottom,
            3 => Self::FullBleed,
            _ => Self::Solid,
        }
    }

    pub fn index(self) -> i64 {
        match self {
            Self::Solid => 0,
            Self::GlowLeft => 1,
            Self::GlowBottom => 2,
            Self::FullBleed => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::GlowLeft => "Glow (left)",
            Self::GlowBottom => "Glow (bottom)",
            Self::FullBleed => "Full bleed",
        }
    }
}

/// How the app filter interprets its lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilterMode {
    /// Only listed players are shown.
    #[default]
    AllowList,
    /// Listed players are hidden.
    BlockList,
}

impl FilterMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::AllowList => "Only allow listed players",
            Self::BlockList => "Block listed players",
        }
    }
}

/// The full persisted configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // ---- general -------------------------------------------------------
    pub theme: AppTheme,
    pub compact_layout: bool,
    pub position: FlyoutAnchor,
    /// 0 = off, 1 = 0.5x, 2 = 1x, 3 = 1.5x, 4 = 2x, 5 = 3x.
    pub animation_speed: i64,
    pub easing: EasingStyle,
    pub autostart: bool,

    // ---- media flyout --------------------------------------------------
    pub media_flyout_enabled: bool,
    /// Keep the flyout on screen until explicitly dismissed.
    pub media_flyout_always_display: bool,
    /// Display duration in milliseconds, `0..=10000`.
    pub duration_ms: i64,
    pub player_info_enabled: bool,
    pub seekbar_enabled: bool,
    pub repeat_enabled: bool,
    pub shuffle_enabled: bool,
    pub center_title_artist: bool,
    pub accent_from_artwork: bool,
    pub backdrop: BackdropStyle,
    /// Pause other players when the active one starts playing.
    pub pause_other_sessions: bool,
    /// Show the flyout when the active player changes track or state.
    ///
    /// This is a Linux adaptation: Windows shows the flyout on media-key
    /// presses, which a Wayland client cannot intercept globally.
    pub show_on_media_change: bool,
    /// Optional user pin on a specific player bus name.
    pub pinned_player: String,

    // ---- app filtering ------------------------------------------------
    pub app_filtering_enabled: bool,
    pub app_filter_mode: FilterMode,
    pub allowed_apps: Vec<String>,
    pub blocked_apps: Vec<String>,

    // ---- lock keys -----------------------------------------------------
    pub lock_keys_enabled: bool,
    pub lock_keys_caps: bool,
    pub lock_keys_num: bool,
    pub lock_keys_scroll: bool,
    pub lock_keys_insert: bool,
    pub lock_keys_duration_ms: i64,
    pub lock_keys_animated: bool,

    // ---- volume --------------------------------------------------------
    pub volume_control_enabled: bool,
    pub volume_duration_ms: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: AppTheme::System,
            compact_layout: false,
            position: FlyoutAnchor::BottomLeft,
            animation_speed: 2,
            easing: EasingStyle::Quadratic,
            // Upstream ships `Startup = true` because the MSIX installer
            // registers a startup task. A Linux app must not silently add
            // itself to the session, so the default is opt-in.
            autostart: false,

            media_flyout_enabled: true,
            media_flyout_always_display: false,
            duration_ms: 3_000,
            player_info_enabled: true,
            seekbar_enabled: false,
            repeat_enabled: false,
            shuffle_enabled: false,
            center_title_artist: false,
            accent_from_artwork: false,
            backdrop: BackdropStyle::Solid,
            pause_other_sessions: false,
            show_on_media_change: true,
            pinned_player: String::new(),

            app_filtering_enabled: false,
            app_filter_mode: FilterMode::AllowList,
            allowed_apps: Vec::new(),
            blocked_apps: Vec::new(),

            lock_keys_enabled: true,
            lock_keys_caps: true,
            lock_keys_num: true,
            lock_keys_scroll: true,
            lock_keys_insert: true,
            lock_keys_duration_ms: 2_000,
            lock_keys_animated: true,

            volume_control_enabled: false,
            volume_duration_ms: 3_000,
        }
    }
}

/// Maximum duration accepted by the upstream duration text boxes.
pub const MAX_DURATION_MS: i64 = 10_000;

impl Settings {
    /// Milliseconds for one flyout animation, derived from `animation_speed`
    /// exactly as `MainWindow.getDuration()` does upstream.
    pub fn animation_duration_ms(&self) -> i64 {
        match self.animation_speed {
            0 => 0,
            1 => 150,
            2 => 300,
            3 => 450,
            4 => 600,
            _ => 900,
        }
    }

    /// Repairs values that are out of contract (hand-edited files, older
    /// builds, future settings written by hand). Returns `true` when
    /// something was corrected.
    pub fn sanitize(&mut self) -> bool {
        let before = self.clone();

        self.animation_speed = self.animation_speed.clamp(0, 5);
        self.duration_ms = self.duration_ms.clamp(0, MAX_DURATION_MS);
        self.lock_keys_duration_ms = self.lock_keys_duration_ms.clamp(0, MAX_DURATION_MS);
        self.volume_duration_ms = self.volume_duration_ms.clamp(0, MAX_DURATION_MS);

        // Empty entries only make the filter list confusing.
        self.allowed_apps = normalize_filter_list(&self.allowed_apps);
        self.blocked_apps = normalize_filter_list(&self.blocked_apps);

        // Compact layout cannot host the seekbar and control row at their
        // upstream sizes; upstream hides both in that mode. Keep the stored
        // preference, but the UI must not crash on it.
        *self != before
    }

    /// Flyout size in logical pixels, ported from `MainWindow.UpdateUILayout`.
    pub fn media_flyout_size(&self, seekbar_visible: bool) -> (f64, f64) {
        if self.compact_layout {
            let height = 60.0
                + if self.seekbar_enabled && seekbar_visible {
                    36.0
                } else {
                    0.0
                };
            (400.0, height)
        } else {
            let mut extra_width = 0.0_f64;
            if self.repeat_enabled {
                extra_width += 36.0;
            }
            if self.shuffle_enabled {
                extra_width += 36.0;
            }
            if self.player_info_enabled {
                extra_width += 90.0;
            }
            // Upstream keeps a 72px floor so the flyout never collapses.
            extra_width = extra_width.max(72.0);
            let height = 112.0
                + if self.seekbar_enabled && seekbar_visible {
                    36.0
                } else {
                    0.0
                };
            (310.0 - 72.0 + extra_width, height)
        }
    }

    /// Whether a player is visible under the current filter configuration.
    pub fn allows_player(&self, filter_key: &str, identity: &str) -> bool {
        if !self.app_filtering_enabled {
            return true;
        }
        let key = filter_key.trim().to_ascii_lowercase();
        let identity = identity.trim().to_ascii_lowercase();
        let matches = |entry: &String| {
            let entry = entry.trim().to_ascii_lowercase();
            !entry.is_empty() && (key == entry || identity == entry)
        };
        match self.app_filter_mode {
            FilterMode::AllowList => self.allowed_apps.iter().any(matches),
            FilterMode::BlockList => !self.blocked_apps.iter().any(matches),
        }
    }
}

fn normalize_filter_list(entries: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(entries.len());
    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lowered = trimmed.to_ascii_lowercase();
        if !out
            .iter()
            .any(|existing| existing.to_ascii_lowercase() == lowered)
        {
            out.push(trimmed.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_upstream_baseline() {
        let settings = Settings::default();
        assert_eq!(settings.theme, AppTheme::System);
        assert_eq!(settings.position, FlyoutAnchor::BottomLeft);
        assert_eq!(settings.animation_speed, 2);
        assert_eq!(settings.duration_ms, 3_000);
        assert!(settings.media_flyout_enabled);
        assert!(settings.player_info_enabled);
        assert!(!settings.seekbar_enabled);
        assert!(settings.lock_keys_enabled);
        assert!(!settings.volume_control_enabled);
        // Documented deviation: Linux never auto-starts by default.
        assert!(!settings.autostart);
    }

    #[test]
    fn animation_duration_matches_upstream_table() {
        let mut settings = Settings::default();
        let expected = [(0, 0), (1, 150), (2, 300), (3, 450), (4, 600), (5, 900)];
        for (speed, millis) in expected {
            settings.animation_speed = speed;
            assert_eq!(settings.animation_duration_ms(), millis, "speed {speed}");
        }
        // Out-of-range values fall into the 3x bucket upstream.
        settings.animation_speed = 99;
        assert_eq!(settings.animation_duration_ms(), 900);
    }

    #[test]
    fn default_media_flyout_matches_the_upstream_formula() {
        let settings = Settings::default();
        // `player_info_enabled` (on by default) reserves the 90px chip
        // column, so the shipped flyout is 328px wide, not 310px.
        assert_eq!(settings.media_flyout_size(false), (328.0, 112.0));
    }

    #[test]
    fn enabling_all_optional_controls_grows_the_flyout_to_400() {
        let settings = Settings {
            repeat_enabled: true,
            shuffle_enabled: true,
            player_info_enabled: true,
            ..Settings::default()
        };
        assert_eq!(settings.media_flyout_size(false).0, 400.0);
    }

    #[test]
    fn seekbar_adds_a_row_only_when_it_is_actually_visible() {
        let settings = Settings {
            seekbar_enabled: true,
            ..Settings::default()
        };
        assert_eq!(settings.media_flyout_size(false).1, 112.0);
        assert_eq!(settings.media_flyout_size(true).1, 148.0);
    }

    #[test]
    fn disabling_every_optional_control_keeps_the_width_floor() {
        let settings = Settings {
            player_info_enabled: false,
            repeat_enabled: false,
            shuffle_enabled: false,
            ..Settings::default()
        };
        assert_eq!(settings.media_flyout_size(false).0, 310.0);
    }

    #[test]
    fn compact_layout_is_400_wide() {
        let compact = Settings {
            compact_layout: true,
            ..Settings::default()
        };
        assert_eq!(compact.media_flyout_size(false), (400.0, 60.0));

        let compact_seekbar = Settings {
            compact_layout: true,
            seekbar_enabled: true,
            ..Settings::default()
        };
        assert_eq!(compact_seekbar.media_flyout_size(true), (400.0, 96.0));
    }

    #[test]
    fn sanitize_clamps_out_of_contract_values() {
        let mut settings = Settings {
            animation_speed: 42,
            duration_ms: 999_999,
            lock_keys_duration_ms: -5,
            volume_duration_ms: 12_345,
            ..Settings::default()
        };
        assert!(settings.sanitize());
        assert_eq!(settings.animation_speed, 5);
        assert_eq!(settings.duration_ms, MAX_DURATION_MS);
        assert_eq!(settings.lock_keys_duration_ms, 0);
        assert_eq!(settings.volume_duration_ms, MAX_DURATION_MS);
    }

    #[test]
    fn sanitize_is_idempotent_for_valid_settings() {
        let mut settings = Settings::default();
        assert!(!settings.sanitize());
    }

    #[test]
    fn filter_lists_are_normalized() {
        let mut settings = Settings {
            app_filtering_enabled: true,
            allowed_apps: vec![
                "  Spotify ".into(),
                "spotify".into(),
                "".into(),
                "vlc".into(),
            ],
            ..Settings::default()
        };
        assert!(settings.sanitize());
        assert_eq!(settings.allowed_apps, vec!["Spotify", "vlc"]);
    }

    #[test]
    fn allow_list_hides_unlisted_players() {
        let settings = Settings {
            app_filtering_enabled: true,
            app_filter_mode: FilterMode::AllowList,
            allowed_apps: vec!["spotify".into()],
            ..Settings::default()
        };
        assert!(settings.allows_player("spotify", "Spotify"));
        assert!(!settings.allows_player("vlc", "VLC"));
    }

    #[test]
    fn block_list_hides_listed_players() {
        let settings = Settings {
            app_filtering_enabled: true,
            app_filter_mode: FilterMode::BlockList,
            blocked_apps: vec!["firefox".into()],
            ..Settings::default()
        };
        assert!(!settings.allows_player("firefox", "Mozilla Firefox"));
        assert!(settings.allows_player("spotify", "Spotify"));
    }

    #[test]
    fn filtering_disabled_allows_everything() {
        let settings = Settings {
            app_filtering_enabled: false,
            app_filter_mode: FilterMode::AllowList,
            allowed_apps: vec!["spotify".into()],
            ..Settings::default()
        };
        assert!(settings.allows_player("vlc", "VLC"));
    }

    #[test]
    fn matching_is_case_insensitive_on_identity_too() {
        let settings = Settings {
            app_filtering_enabled: true,
            app_filter_mode: FilterMode::AllowList,
            allowed_apps: vec!["Spotify".into()],
            ..Settings::default()
        };
        assert!(settings.allows_player("", "SPOTIFY"));
    }

    #[test]
    fn json_round_trip_preserves_every_field() {
        let settings = Settings {
            theme: AppTheme::Dark,
            position: FlyoutAnchor::TopRight,
            backdrop: BackdropStyle::FullBleed,
            allowed_apps: vec!["spotify".into()],
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let parsed: Settings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, settings);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // A settings file from an older build.
        let json = r#"{"theme":"dark","compact_layout":true}"#;
        let parsed: Settings = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.theme, AppTheme::Dark);
        assert!(parsed.compact_layout);
        assert_eq!(parsed.duration_ms, 3_000);
        assert!(parsed.media_flyout_enabled);
    }

    #[test]
    fn theme_encoding_round_trips() {
        for theme in AppTheme::ALL {
            assert_eq!(AppTheme::from_index(theme.index()), theme);
        }
    }

    #[test]
    fn easing_encoding_round_trips() {
        for easing in EasingStyle::ALL {
            assert_eq!(EasingStyle::from_index(easing.index()), easing);
        }
    }

    #[test]
    fn backdrop_encoding_round_trips() {
        for style in BackdropStyle::ALL {
            assert_eq!(BackdropStyle::from_index(style.index()), style);
        }
    }
}
