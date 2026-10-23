//! Offscreen widget rendering.
//!
//! GTK 4.22 removed the public display-renderer accessor, but a `GtkNative`
//! still exposes the `GskRenderer` it was realised with. Rendering a widget
//! through that renderer gives a texture that is identical to the on-screen
//! frame, which makes it useful for two different jobs:
//!
//! * producing documentation screenshots without a compositor screenshot
//!   tool (and without depending on X11);
//! * proving at runtime that the GSK renderer really rasterises the flyouts,
//!   instead of only trusting `GSK_RENDERER` / `/proc/self/maps`.

use std::path::Path;

use gtk::graphene;
use gtk::prelude::*;

/// Renders `widget` at its current allocation into a PNG.
///
/// Returns the pixel size that was written. The widget must be realised (its
/// window mapped at least once) and allocated, otherwise there is nothing to
/// paint and an explanatory error is returned instead of a blank image.
pub fn save_widget_png<W: IsA<gtk::Widget>>(widget: &W, path: &Path) -> Result<(i32, i32), String> {
    let native = widget
        .dynamic_cast_ref::<gtk::Native>()
        .ok_or_else(|| "widget is not a GtkNative, so it has no renderer".to_string())?;
    let renderer = native
        .renderer()
        .ok_or_else(|| "window has no GSK renderer yet (not realised)".to_string())?;

    let width = widget.width();
    let height = widget.height();
    if width <= 0 || height <= 0 {
        return Err(format!("widget has no allocation ({width}x{height})"));
    }

    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
    let node = snapshot
        .to_node()
        .ok_or_else(|| "the widget produced no render node".to_string())?;

    let viewport = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(node, Some(&viewport));
    texture
        .save_to_png(path)
        .map_err(|error| error.to_string())?;

    Ok((width, height))
}
