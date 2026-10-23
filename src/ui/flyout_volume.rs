//! The volume flyout.
//!
//! Upstream shows a Windows volume OSD. The Linux equivalent is the master
//! sink of the running PipeWire/WirePlumber graph, so the flyout is a compact
//! slider that reads and writes that sink.

use std::cell::Cell;
use std::rc::Rc;

use async_channel::Sender;
use gtk::prelude::*;

use crate::audio::AudioCommand;
use crate::domain::volume::{VolumeCommand, VolumeState};
use crate::ui::animation::{self, Easing};

const WIDTH: i32 = 320;
const HEIGHT: i32 = 56;

pub struct VolumeFlyout {
    pub window: gtk::Window,
    icon: gtk::Image,
    scale: gtk::Scale,
    value: gtk::Label,
    commands: Sender<AudioCommand>,
    /// Prevents the slider from echoing our own programmatic updates.
    updating: Rc<Cell<bool>>,
    muted: Cell<bool>,
}

impl VolumeFlyout {
    pub fn new(application: &adw::Application, commands: Sender<AudioCommand>) -> Self {
        let window = gtk::Window::new();
        window.set_application(Some(application));
        window.set_decorated(false);
        window.set_resizable(false);
        window.set_hide_on_close(true);
        window.add_css_class("flyout");
        window.set_title(Some("FluentFlyout Volume"));
        window.set_default_size(WIDTH, HEIGHT);
        window.set_size_request(WIDTH, HEIGHT);

        let card = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        card.add_css_class("flyout-card");
        card.add_css_class("tight");
        card.set_valign(gtk::Align::Center);

        let icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
        icon.set_pixel_size(20);

        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
        scale.add_css_class("volume");
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_valign(gtk::Align::Center);

        let value = gtk::Label::new(Some("0%"));
        value.add_css_class("volume-value");
        value.set_width_chars(4);
        value.set_xalign(1.0);

        card.append(&icon);
        card.append(&scale);
        card.append(&value);
        window.set_child(Some(&card));

        let updating = Rc::new(Cell::new(false));
        let commands_clone = commands.clone();
        let updating_clone = updating.clone();
        scale.connect_value_changed(move |scale| {
            if updating_clone.get() {
                return;
            }
            let _ = commands_clone.try_send(AudioCommand::Apply(VolumeCommand::Set(scale.value())));
        });

        // Clicking the speaker icon mutes, which is the behaviour users expect
        // from a volume OSD.
        let gesture = gtk::GestureClick::new();
        let commands_mute = commands.clone();
        gesture.connect_released(move |_, _, _, _| {
            let _ = commands_mute.try_send(AudioCommand::Apply(VolumeCommand::ToggleMute));
        });
        icon.add_controller(gesture);

        Self {
            window,
            icon,
            scale,
            value,
            commands,
            updating,
            muted: Cell::new(false),
        }
    }

    /// Applies a new sink state. `None` means the sink is unavailable.
    pub fn update(&self, state: Option<VolumeState>) {
        let Some(state) = state else {
            self.value.set_text("--");
            self.scale.set_sensitive(false);
            self.icon.set_icon_name(Some("audio-volume-muted-symbolic"));
            return;
        };

        self.scale.set_sensitive(true);
        self.muted.set(state.muted);

        self.updating.set(true);
        self.scale.set_value(state.display_percent() as f64 / 100.0);
        self.updating.set(false);

        self.value
            .set_text(&format!("{}%", state.display_percent()));
        self.icon.set_icon_name(Some(icon_for(state)));
    }

    pub fn present(&self) {
        self.window.set_visible(true);
        self.window.present();
        // Ask for a fresh level: the sink may have changed while hidden.
        let _ = self.commands.try_send(AudioCommand::Refresh);
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

    /// Whether the last state was muted, for tests.
    pub fn is_muted(&self) -> bool {
        self.muted.get()
    }
}

fn icon_for(state: VolumeState) -> &'static str {
    if state.muted || state.percent == 0 {
        "audio-volume-muted-symbolic"
    } else if state.percent < 34 {
        "audio-volume-low-symbolic"
    } else if state.percent < 67 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_follows_the_level() {
        assert_eq!(
            icon_for(VolumeState::new(0, false)),
            "audio-volume-muted-symbolic"
        );
        assert_eq!(
            icon_for(VolumeState::new(20, false)),
            "audio-volume-low-symbolic"
        );
        assert_eq!(
            icon_for(VolumeState::new(50, false)),
            "audio-volume-medium-symbolic"
        );
        assert_eq!(
            icon_for(VolumeState::new(100, false)),
            "audio-volume-high-symbolic"
        );
    }

    #[test]
    fn muted_always_uses_the_muted_icon() {
        assert_eq!(
            icon_for(VolumeState::new(80, true)),
            "audio-volume-muted-symbolic"
        );
    }
}
