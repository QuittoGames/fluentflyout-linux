//! Flyout placement rules.
//!
//! This is a faithful port of `MainWindow.GetFinalPosition` from the upstream
//! WPF application, expressed independently of any windowing toolkit so that
//! it can be verified without a display server.
//!
//! On Linux the caller decides whether these coordinates can be honoured:
//! wlroots/KDE expose anchored surfaces (layer-shell), GNOME Wayland does not
//! allow clients to place toplevels at arbitrary coordinates.

use serde::{Deserialize, Serialize};

/// Edge margin used by the upstream flyout (`16` logical pixels).
pub const EDGE_MARGIN: f64 = 16.0;

/// Extra bottom margin the upstream app reserves on bottom-center placement
/// to avoid overlapping the native volume OSD.
pub const VOLUME_OSD_RESERVED_MARGIN: f64 = 80.0;

/// The six positions exposed by the upstream `Position` setting, in the same
/// numeric order (`0..=5`) so that imported settings keep their meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FlyoutAnchor {
    #[default]
    BottomLeft,
    BottomCenter,
    BottomRight,
    TopLeft,
    TopCenter,
    TopRight,
}

impl FlyoutAnchor {
    pub const ALL: [Self; 6] = [
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
    ];

    /// Upstream stores the setting as an integer index.
    pub fn from_index(index: i64) -> Self {
        match index {
            1 => Self::BottomCenter,
            2 => Self::BottomRight,
            3 => Self::TopLeft,
            4 => Self::TopCenter,
            5 => Self::TopRight,
            _ => Self::BottomLeft,
        }
    }

    pub fn index(self) -> i64 {
        match self {
            Self::BottomLeft => 0,
            Self::BottomCenter => 1,
            Self::BottomRight => 2,
            Self::TopLeft => 3,
            Self::TopCenter => 4,
            Self::TopRight => 5,
        }
    }

    pub fn is_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopCenter | Self::TopRight)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::BottomLeft => "Bottom left",
            Self::BottomCenter => "Bottom center",
            Self::BottomRight => "Bottom right",
            Self::TopLeft => "Top left",
            Self::TopCenter => "Top center",
            Self::TopRight => "Top right",
        }
    }
}

/// A rectangle in logical pixels, origin at the top-left of the work area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Computes the top-left corner for a flyout of `size` inside `work_area`.
///
/// `reserve_volume_osd` reproduces the upstream rule: bottom-center flyouts
/// sit 80px above the bottom edge when they must not cover the system volume
/// OSD, unless the app draws its own volume flyout directly above them.
pub fn compute_origin(
    anchor: FlyoutAnchor,
    work_area: Rect,
    size: (f64, f64),
    reserve_volume_osd: bool,
) -> (f64, f64) {
    let (width, height) = size;

    let x = match anchor {
        FlyoutAnchor::BottomLeft | FlyoutAnchor::TopLeft => work_area.x + EDGE_MARGIN,
        FlyoutAnchor::BottomRight | FlyoutAnchor::TopRight => {
            work_area.x + work_area.width - width - EDGE_MARGIN
        }
        FlyoutAnchor::BottomCenter | FlyoutAnchor::TopCenter => {
            work_area.x + work_area.width / 2.0 - width / 2.0
        }
    };

    let bottom_margin = if reserve_volume_osd {
        VOLUME_OSD_RESERVED_MARGIN
    } else {
        EDGE_MARGIN
    };

    let y = match anchor {
        FlyoutAnchor::BottomLeft | FlyoutAnchor::BottomRight => {
            work_area.y + work_area.height - height - EDGE_MARGIN
        }
        FlyoutAnchor::BottomCenter => work_area.y + work_area.height - height - bottom_margin,
        FlyoutAnchor::TopLeft | FlyoutAnchor::TopCenter | FlyoutAnchor::TopRight => {
            work_area.y + EDGE_MARGIN
        }
    };

    (x, y)
}

/// Clamps a rectangle so it stays fully inside the work area.
///
/// Window sizes are computed from settings, but a very narrow monitor (or a
/// future compact layout variant) could otherwise push a flyout off-screen.
pub fn clamp_into(work_area: Rect, position: (f64, f64), size: (f64, f64)) -> (f64, f64) {
    let max_x = (work_area.x + work_area.width - size.0).max(work_area.x);
    let max_y = (work_area.y + work_area.height - size.1).max(work_area.y);
    (
        position.0.clamp(work_area.x, max_x),
        position.1.clamp(work_area.y, max_y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work_area() -> Rect {
        Rect::new(0.0, 0.0, 1920.0, 1080.0)
    }

    #[test]
    fn bottom_left_matches_upstream_formula() {
        let (x, y) = compute_origin(FlyoutAnchor::BottomLeft, work_area(), (310.0, 116.0), false);
        assert_eq!(x, 16.0);
        assert_eq!(y, 1080.0 - 116.0 - 16.0);
    }

    #[test]
    fn bottom_center_is_horizontally_centred() {
        let (x, _) = compute_origin(
            FlyoutAnchor::BottomCenter,
            work_area(),
            (310.0, 116.0),
            false,
        );
        assert_eq!(x, 1920.0 / 2.0 - 310.0 / 2.0);
    }

    #[test]
    fn bottom_right_sits_against_the_right_edge() {
        let (x, _) = compute_origin(
            FlyoutAnchor::BottomRight,
            work_area(),
            (310.0, 116.0),
            false,
        );
        assert_eq!(x, 1920.0 - 310.0 - 16.0);
    }

    #[test]
    fn top_anchors_pin_to_the_top_edge() {
        for anchor in [
            FlyoutAnchor::TopLeft,
            FlyoutAnchor::TopCenter,
            FlyoutAnchor::TopRight,
        ] {
            let (_, y) = compute_origin(anchor, work_area(), (310.0, 116.0), false);
            assert_eq!(y, 16.0, "{anchor:?} should pin to the top");
        }
    }

    #[test]
    fn volume_osd_reservation_only_affects_bottom_center() {
        let size = (310.0, 116.0);
        let (_, reserved) = compute_origin(FlyoutAnchor::BottomCenter, work_area(), size, true);
        let (_, plain) = compute_origin(FlyoutAnchor::BottomCenter, work_area(), size, false);
        assert_eq!(plain - reserved, VOLUME_OSD_RESERVED_MARGIN - EDGE_MARGIN);

        let (_, left_reserved) = compute_origin(FlyoutAnchor::BottomLeft, work_area(), size, true);
        assert_eq!(left_reserved, 1080.0 - 116.0 - 16.0);
    }

    #[test]
    fn origin_accounts_for_work_area_offset() {
        // A second monitor placed to the right of the primary one.
        let secondary = Rect::new(1920.0, 0.0, 1280.0, 720.0);
        let (x, y) = compute_origin(FlyoutAnchor::BottomRight, secondary, (310.0, 116.0), false);
        assert_eq!(x, 1920.0 + 1280.0 - 310.0 - 16.0);
        assert_eq!(y, 720.0 - 116.0 - 16.0);
    }

    #[test]
    fn anchor_index_round_trips() {
        for anchor in FlyoutAnchor::ALL {
            assert_eq!(FlyoutAnchor::from_index(anchor.index()), anchor);
        }
        // Out-of-range values fall back to the upstream default.
        assert_eq!(FlyoutAnchor::from_index(99), FlyoutAnchor::BottomLeft);
        assert_eq!(FlyoutAnchor::from_index(-1), FlyoutAnchor::BottomLeft);
    }

    #[test]
    fn clamping_keeps_the_window_inside_the_work_area() {
        let area = Rect::new(0.0, 0.0, 800.0, 600.0);
        assert_eq!(clamp_into(area, (-40.0, -10.0), (320.0, 120.0)), (0.0, 0.0));
        assert_eq!(
            clamp_into(area, (700.0, 590.0), (320.0, 120.0)),
            (480.0, 480.0)
        );
    }

    #[test]
    fn clamping_handles_a_window_larger_than_the_screen() {
        let area = Rect::new(0.0, 0.0, 300.0, 200.0);
        // Must not produce inverted bounds.
        assert_eq!(clamp_into(area, (50.0, 50.0), (400.0, 300.0)), (0.0, 0.0));
    }
}
