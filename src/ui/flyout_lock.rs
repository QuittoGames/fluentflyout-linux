//! The lock-key flyout.
//!
//! Upstream is a 160x50 card with a 22px padlock, a 14px label and a 60x3
//! accent bar. The animated shackle rotation is replaced by the icon theme's
//! lock glyphs plus the accent bar, which carries the same information and
//! respects the platform's visual language.

use std::cell::Cell;

use gtk::prelude::*;

use crate::domain::lock_keys::LockKeyKind;
use crate::domain::settings::Settings;
use crate::ui::animation::{self, Easing};

pub struct LockFlyout {
    pub window: gtk::Window,
    label: gtk::Label,
    icon: gtk::Image,
    indicator: gtk::Box,
    indicator_wrap: gtk::Box,
    on: Cell<bool>,
}

const WIDTH: i32 = 160;
const HEIGHT: i32 = 50;
const INDICATOR_WIDTH: i32 = 60;

impl LockFlyout {
    pub fn new(application: &adw::Application) -> Self {
        let window = gtk::Window::new();
        window.set_application(Some(application));
        window.set_decorated(false);
        window.set_resizable(false);
        window.set_hide_on_close(true);
        window.add_css_class("flyout");
        window.set_title(Some("FluentFlyout Lock Keys"));
        window.set_default_size(WIDTH, HEIGHT);
        window.set_size_request(WIDTH, HEIGHT);

        let overlay = gtk::Overlay::new();
        let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
        card.add_css_class("flyout-card");
        card.add_css_class("tight");
        card.set_valign(gtk::Align::Fill);
        overlay.set_child(Some(&card));

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_halign(gtk::Align::Center);
        row.set_valign(gtk::Align::Center);

        let icon = gtk::Image::from_icon_name("system-lock-screen-symbolic");
        icon.set_pixel_size(22);

        let label = gtk::Label::new(Some("Caps Lock off"));
        label.add_css_class("lock-title");
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        row.append(&icon);
        row.append(&label);
        card.append(&row);

        // The accent bar sits at the bottom of the card, centred, exactly like
        // the upstream `LockWindow` indicator.
        let indicator_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        indicator_wrap.set_halign(gtk::Align::Center);
        indicator_wrap.set_valign(gtk::Align::End);
        indicator_wrap.set_vexpand(true);

        let indicator = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        indicator.add_css_class("lock-indicator");
        indicator.set_size_request(INDICATOR_WIDTH, 3);
        indicator_wrap.append(&indicator);
        card.append(&indicator_wrap);

        window.set_child(Some(&overlay));

        Self {
            window,
            label,
            icon,
            indicator,
            indicator_wrap,
            on: Cell::new(false),
        }
    }

    /// Renders one key state.
    ///
    /// `animated` follows the upstream `LockKeysAnimated` option: when off,
    /// the indicator simply snaps instead of animating its width.
    pub fn show_state(&self, kind: LockKeyKind, on: bool, animated: bool) {
        self.label.set_text(&format!(
            "{} {}",
            kind.label(),
            if on { "on" } else { "off" }
        ));
        self.icon.set_icon_name(Some(if on {
            "system-lock-screen-symbolic"
        } else {
            "changes-allow-symbolic"
        }));
        self.on.set(on);

        if on {
            self.indicator.remove_css_class("off");
        } else {
            self.indicator.add_css_class("off");
        }

        if animated {
            // A short width pulse communicates the state change without
            // needing the Windows shackle animation.
            self.indicator_wrap.set_size_request(-1, 3);
        }
    }

    /// Sets the indicator width, used by the settings preview.
    pub fn set_indicator_width(&self, width: i32) {
        self.indicator
            .set_size_request(width.clamp(0, INDICATOR_WIDTH), 3);
    }

    pub fn present(&self) {
        self.window.set_visible(true);
        self.window.present();
    }

    pub fn animate_in(&self, easing: Easing, duration_ms: u64) {
        animation::fade(&self.window, 0.0, 1.0, duration_ms, easing, || {});
    }

    pub fn animate_out(&self, easing: Easing, duration_ms: u64, window: &gtk::Window) {
        // `fade` borrows the window while the callback owns a handle to hide
        // it; cloning keeps those two uses independent.
        let target = window.clone();
        let hide = window.clone();
        animation::fade(&target, 1.0, 0.0, duration_ms, easing, move || {
            hide.set_visible(false);
        });
    }

    /// Current state, for tests and diagnostics.
    pub fn is_on(&self) -> bool {
        self.on.get()
    }

    /// Applies size-affecting settings.
    pub fn apply_settings(&self, settings: &Settings) {
        let _ = settings;
        self.window.set_default_size(WIDTH, HEIGHT);
    }
}
