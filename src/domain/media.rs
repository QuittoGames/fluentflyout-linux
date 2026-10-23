//! Media domain model.
//!
//! Mirrors the subset of `GlobalSystemMediaTransportControlsSession` that
//! FluentFlyout consumes on Windows, expressed in terms of what MPRIS can
//! actually deliver on Linux. Fields that MPRIS cannot provide reliably
//! (for example a queue-based "Up Next" item) are deliberately absent.

use std::path::PathBuf;

/// Playback state as reported by `org.mpris.MediaPlayer2.Player.PlaybackStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

impl PlaybackState {
    /// Parses the MPRIS `PlaybackStatus` string.
    pub fn from_mpris(value: &str) -> Self {
        match value {
            "Playing" => Self::Playing,
            "Paused" => Self::Paused,
            _ => Self::Stopped,
        }
    }

    pub fn is_playing(self) -> bool {
        matches!(self, Self::Playing)
    }
}

/// `org.mpris.MediaPlayer2.Player.LoopStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopStatus {
    #[default]
    None,
    Track,
    Playlist,
}

impl LoopStatus {
    pub fn from_mpris(value: &str) -> Self {
        match value {
            "Track" => Self::Track,
            "Playlist" => Self::Playlist,
            _ => Self::None,
        }
    }

    pub fn as_mpris(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Track => "Track",
            Self::Playlist => "Playlist",
        }
    }

    /// Cycles `None -> Playlist -> Track -> None`, matching the order the
    /// upstream flyout button steps through.
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Playlist,
            Self::Playlist => Self::Track,
            Self::Track => Self::None,
        }
    }
}

/// Where a player's cover art can be obtained.
///
/// MPRIS exposes `mpris:artUrl`, which is usually a `file://` URI (Spotify,
/// local players) but can be `http(s)://` (browser players). Both cases are
/// kept distinct so the UI can load local files synchronously and remote
/// resources asynchronously without the media service depending on GTK/GIO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtworkSource {
    Local(PathBuf),
    Remote(String),
}

/// Track information extracted from the MPRIS `Metadata` dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// `mpris:trackid`, required for absolute seeks.
    pub track_id: Option<String>,
    pub artwork: Option<ArtworkSource>,
    /// Track length in microseconds, when the player advertises it.
    pub length_us: Option<i64>,
}

impl TrackMetadata {
    /// Human-readable label used when a player exposes no title.
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            "Unknown title"
        } else {
            &self.title
        }
    }
}

/// Optional player actions advertised through MPRIS `Can*` properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerCapabilities {
    pub can_play_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_seek: bool,
    pub can_set_loop: bool,
    pub can_set_shuffle: bool,
}

/// Identity of the owning MPRIS player.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerInfo {
    /// Well-known bus name, e.g. `org.mpris.MediaPlayer2.spotify`.
    pub bus_name: String,
    /// `Identity` property, e.g. `Spotify`.
    pub identity: String,
    /// `DesktopEntry` property when present.
    pub desktop_entry: Option<String>,
}

impl PlayerInfo {
    /// Value used by the app filter: prefer the desktop entry, fall back to
    /// the bus name suffix. Matching is case-insensitive on the caller side.
    pub fn filter_key(&self) -> String {
        match &self.desktop_entry {
            Some(entry) if !entry.is_empty() => entry.to_ascii_lowercase(),
            _ => self
                .bus_name
                .rsplit('.')
                .next()
                .unwrap_or(&self.bus_name)
                .to_ascii_lowercase(),
        }
    }
}

/// Immutable view of the active player at a point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaSnapshot {
    pub player: PlayerInfo,
    pub state: PlaybackState,
    pub track: TrackMetadata,
    pub caps: PlayerCapabilities,
    pub loop_status: LoopStatus,
    pub shuffle: bool,
    /// Current position in microseconds.
    pub position_us: i64,
    /// Length in microseconds, taken from metadata or `mpris:length`.
    pub length_us: Option<i64>,
}

impl MediaSnapshot {
    pub fn is_playing(&self) -> bool {
        self.state.is_playing()
    }

    /// Seekbar usability: mirrors the upstream heuristic that a session only
    /// gets a seekbar when it advertises a meaningful length.
    pub fn seekbar_available(&self) -> bool {
        self.caps.can_seek && self.length_us.is_some_and(|length| length >= 1_000_000)
    }

    /// Fraction of the track already played, clamped to `0.0..=1.0`.
    pub fn progress_fraction(&self) -> f64 {
        match self.length_us {
            Some(length) if length > 0 => (self.position_us as f64 / length as f64).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// Formats a duration in microseconds as `m:ss`, or `h:mm:ss` past one hour.
///
/// Returns `"-:--"` for negative or absent values, matching the placeholder
/// the upstream flyout shows before the first timeline update.
pub fn format_duration(micros: i64) -> String {
    if micros < 0 {
        return "-:--".to_string();
    }
    let total_seconds = micros / 1_000_000;
    let seconds = total_seconds % 60;
    let minutes = (total_seconds / 60) % 60;
    let hours = total_seconds / 3600;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_state_parsing_is_total() {
        assert_eq!(PlaybackState::from_mpris("Playing"), PlaybackState::Playing);
        assert_eq!(PlaybackState::from_mpris("Paused"), PlaybackState::Paused);
        assert_eq!(PlaybackState::from_mpris("Stopped"), PlaybackState::Stopped);
        // Unknown values must not panic; MPRIS implementations vary.
        assert_eq!(
            PlaybackState::from_mpris("nonsense"),
            PlaybackState::Stopped
        );
    }

    #[test]
    fn loop_status_cycles_through_all_states() {
        let mut status = LoopStatus::None;
        status = status.next();
        assert_eq!(status, LoopStatus::Playlist);
        status = status.next();
        assert_eq!(status, LoopStatus::Track);
        status = status.next();
        assert_eq!(status, LoopStatus::None);
    }

    #[test]
    fn loop_status_round_trips_through_mpris_names() {
        for status in [LoopStatus::None, LoopStatus::Track, LoopStatus::Playlist] {
            assert_eq!(LoopStatus::from_mpris(status.as_mpris()), status);
        }
    }

    #[test]
    fn duration_formatting_matches_the_flyout_placeholders() {
        assert_eq!(format_duration(-1), "-:--");
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(59_000_000), "0:59");
        assert_eq!(format_duration(61_000_000), "1:01");
        assert_eq!(format_duration(3_600_000_000), "1:00:00");
        assert_eq!(format_duration(3_723_000_000), "1:02:03");
    }

    #[test]
    fn progress_fraction_is_clamped() {
        let mut snapshot = MediaSnapshot {
            player: PlayerInfo::default(),
            state: PlaybackState::Playing,
            track: TrackMetadata::default(),
            caps: PlayerCapabilities::default(),
            loop_status: LoopStatus::None,
            shuffle: false,
            position_us: 0,
            length_us: Some(100_000_000),
        };
        assert_eq!(snapshot.progress_fraction(), 0.0);
        snapshot.position_us = 50_000_000;
        assert_eq!(snapshot.progress_fraction(), 0.5);
        // A player reporting a position past the length must not overflow.
        snapshot.position_us = 400_000_000;
        assert_eq!(snapshot.progress_fraction(), 1.0);
    }

    #[test]
    fn seekbar_requires_both_capability_and_length() {
        let mut snapshot = MediaSnapshot {
            player: PlayerInfo::default(),
            state: PlaybackState::Playing,
            track: TrackMetadata::default(),
            caps: PlayerCapabilities {
                can_seek: true,
                ..Default::default()
            },
            loop_status: LoopStatus::None,
            shuffle: false,
            position_us: 0,
            length_us: Some(500_000),
        };
        // Under one second: the upstream heuristic treats this as "unknown".
        assert!(!snapshot.seekbar_available());
        snapshot.length_us = Some(180_000_000);
        assert!(snapshot.seekbar_available());
        snapshot.caps.can_seek = false;
        assert!(!snapshot.seekbar_available());
    }

    #[test]
    fn filter_key_prefers_desktop_entry_then_bus_suffix() {
        let with_entry = PlayerInfo {
            bus_name: "org.mpris.MediaPlayer2.Spotify".into(),
            identity: "Spotify".into(),
            desktop_entry: Some("Spotify".into()),
        };
        assert_eq!(with_entry.filter_key(), "spotify");

        let without_entry = PlayerInfo {
            bus_name: "org.mpris.MediaPlayer2.vlc".into(),
            identity: "VLC".into(),
            desktop_entry: None,
        };
        assert_eq!(without_entry.filter_key(), "vlc");
    }

    #[test]
    fn missing_title_falls_back_to_placeholder() {
        let track = TrackMetadata::default();
        assert_eq!(track.display_title(), "Unknown title");
    }
}
