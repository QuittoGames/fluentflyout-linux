//! Flyout animation.
//!
//! The Windows original animates the window's `Top` and `Opacity` with a
//! configurable duration and easing curve. A Wayland client cannot move its
//! own toplevel, so the vertical motion happens *inside* the window: the
//! content slides in with a `GtkRevealer` while this module fades it. Both
//! honour the same `animation_speed` and `easing` settings.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gtk::glib;
use gtk::prelude::*;

use crate::domain::settings::EasingStyle;

/// Easing curves exposed by the upstream settings combo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    Linear,
    Sine,
    Quadratic,
    Cubic,
}

impl From<EasingStyle> for Easing {
    fn from(value: EasingStyle) -> Self {
        match value {
            EasingStyle::Linear => Self::Linear,
            EasingStyle::Sine => Self::Sine,
            EasingStyle::Quadratic => Self::Quadratic,
            EasingStyle::Cubic => Self::Cubic,
        }
    }
}

impl Easing {
    /// Maps linear progress `0.0..=1.0` onto the curve.
    ///
    /// All curves are ease-out, matching the upstream flyout, which decelerates
    /// as it settles.
    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            // 1 - cos(pi*t/2): gentle start, soft landing.
            Self::Sine => (t * std::f64::consts::FRAC_PI_2).sin(),
            Self::Quadratic => 1.0 - (1.0 - t) * (1.0 - t),
            Self::Cubic => 1.0 - (1.0 - t).powi(3),
        }
    }
}

/// Boxed completion callback shared between the tick closure and the caller.
type Completion = Rc<Cell<Option<Box<dyn FnOnce()>>>>;

/// Fades `widget` from `from` to `to` opacity over `duration_ms`.
///
/// `on_done` runs once at the end (or immediately when the duration is zero).
/// The tick callback removes itself, so no signal handler leaks.
pub fn fade<F>(
    widget: &impl IsA<gtk::Widget>,
    from: f64,
    to: f64,
    duration_ms: u64,
    easing: Easing,
    on_done: F,
) where
    F: FnOnce() + 'static,
{
    let widget = widget.clone().upcast::<gtk::Widget>();
    widget.set_opacity(from.clamp(0.0, 1.0));

    if duration_ms == 0 {
        widget.set_opacity(to.clamp(0.0, 1.0));
        on_done();
        return;
    }

    let callback: Completion = Rc::new(Cell::new(Some(Box::new(on_done))));
    let started = Instant::now();

    widget.add_tick_callback(move |widget, _clock| {
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let progress = (elapsed_ms / duration_ms as f64).min(1.0);
        let eased = easing.apply(progress);
        widget.set_opacity(from + (to - from) * eased);

        if progress >= 1.0 {
            if let Some(callback) = callback.take() {
                callback();
            }
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curve_starts_at_zero_and_ends_at_one() {
        for easing in [
            Easing::Linear,
            Easing::Sine,
            Easing::Quadratic,
            Easing::Cubic,
        ] {
            assert!(easing.apply(0.0).abs() < 1e-9, "{easing:?} at 0");
            assert!((easing.apply(1.0) - 1.0).abs() < 1e-9, "{easing:?} at 1");
        }
    }

    #[test]
    fn curves_are_monotonic() {
        for easing in [
            Easing::Linear,
            Easing::Sine,
            Easing::Quadratic,
            Easing::Cubic,
        ] {
            let mut previous = 0.0;
            for step in 0..=100 {
                let value = easing.apply(step as f64 / 100.0);
                assert!(value >= previous - 1e-9, "{easing:?} went backwards");
                previous = value;
            }
        }
    }

    #[test]
    fn values_are_clamped_outside_the_unit_interval() {
        assert_eq!(Easing::Quadratic.apply(-5.0), 0.0);
        assert_eq!(Easing::Quadratic.apply(5.0), 1.0);
    }

    #[test]
    fn ease_out_curves_start_faster_than_linear() {
        // This is what "ease-out" means and what makes the flyout feel light.
        for easing in [Easing::Sine, Easing::Quadratic, Easing::Cubic] {
            assert!(
                easing.apply(0.25) > 0.25,
                "{easing:?} should be ahead of linear at t=0.25"
            );
        }
    }

    #[test]
    fn easing_style_mapping_covers_all_variants() {
        assert_eq!(Easing::from(EasingStyle::Linear), Easing::Linear);
        assert_eq!(Easing::from(EasingStyle::Sine), Easing::Sine);
        assert_eq!(Easing::from(EasingStyle::Quadratic), Easing::Quadratic);
        assert_eq!(Easing::from(EasingStyle::Cubic), Easing::Cubic);
    }
}
