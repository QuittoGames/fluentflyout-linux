//! The media flyout.
//!
//! Layout is a direct port of the upstream XAML: an 82x82 cover, a text column
//! with title and artist, and a control row with previous, play/pause, next
//! and the optional repeat/shuffle toggles. An optional seekbar row adds 36px.
//! The player chip is a Linux-side addition: MPRIS users often have several
//! players open, so naming the active one matters more than on Windows.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use async_channel::Sender;
use gtk::gdk;
use gtk::prelude::*;

use crate::domain::media::{ArtworkSource, LoopStatus, MediaSnapshot};
use crate::domain::settings::Settings;
use crate::media::MediaCommand;
use crate::settings::SettingsStore;
use crate::ui::animation::{self, Easing};
use crate::ui::cover_art;
use crate::ui::style::{self, Rgb};

/// Edge length of the album-art thumbnail, from the upstream XAML.
const COVER_SIZE: i32 = 82;

/// Builds the media flyout. The window is created hidden.
pub struct MediaFlyout {
    pub window: gtk::Window,
    card: gtk::Box,
    backdrop: gtk::Picture,
    cover: gtk::Picture,
    cover_frame: gtk::Overlay,
    title: gtk::Label,
    artist: gtk::Label,
    chip: gtk::Label,
    play: gtk::Button,
    previous: gtk::Button,
    next: gtk::Button,
    repeat: gtk::Button,
    shuffle: gtk::Button,
    seek_row: gtk::Box,
    elapsed: gtk::Label,
    total: gtk::Label,
    seek: gtk::Scale,

    settings: Rc<RefCell<SettingsStore>>,
    commands: Sender<MediaCommand>,
    /// Guards against reacting to our own `set_value` calls.
    updating_seek: Rc<Cell<bool>>,
    length_us: Rc<Cell<i64>>,
    loop_status: Rc<Cell<LoopStatus>>,
    shuffle_on: Rc<Cell<bool>>,
    accent: RefCell<Option<Rgb>>,
}

impl MediaFlyout {
    pub fn new(
        application: &adw::Application,
        commands: Sender<MediaCommand>,
        settings: Rc<RefCell<SettingsStore>>,
    ) -> Self {
        let window = gtk::Window::new();
        window.set_application(Some(application));
        window.set_decorated(false);
        window.set_resizable(false);
        window.set_hide_on_close(true);
        window.add_css_class("flyout");
        window.set_title(Some("FluentFlyout"));

        let overlay = gtk::Overlay::new();

        let backdrop = gtk::Picture::new();
        backdrop.add_css_class("backdrop");
        backdrop.set_content_fit(gtk::ContentFit::Cover);
        backdrop.set_can_shrink(true);
        backdrop.set_hexpand(true);
        backdrop.set_vexpand(true);
        overlay.set_child(Some(&backdrop));

        let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
        card.add_css_class("flyout-card");
        overlay.add_overlay(&card);

        // ---- cover + text + controls -----------------------------------
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);

        let cover_frame = gtk::Overlay::new();
        cover_frame.add_css_class("cover");
        cover_frame.set_overflow(gtk::Overflow::Hidden);
        // `gtk::Overlay` measures from its main child, so a fixed spacer pins
        // the cover to exactly 82x82 regardless of the artwork's natural size.
        // Without it a 400px texture would claim the whole card and squeeze
        // the text column to nothing.
        let cover_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        cover_spacer.set_size_request(COVER_SIZE, COVER_SIZE);
        cover_frame.set_child(Some(&cover_spacer));

        let cover = gtk::Picture::new();
        cover.set_content_fit(gtk::ContentFit::Cover);
        cover.set_can_shrink(true);
        cover_frame.add_overlay(&cover);
        row.append(&cover_frame);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Fill);

        let title = gtk::Label::new(None);
        title.add_css_class("title");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_lines(1);
        text.append(&title);

        let artist = gtk::Label::new(None);
        artist.add_css_class("artist");
        artist.set_xalign(0.0);
        artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
        artist.set_lines(1);
        text.append(&artist);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        controls.set_valign(gtk::Align::End);
        controls.set_vexpand(true);
        controls.set_halign(gtk::Align::Fill);

        let previous = icon_button("media-skip-backward-symbolic", "control");
        let play = icon_button("media-playback-start-symbolic", "play");
        let next = icon_button("media-skip-forward-symbolic", "control");
        let repeat = icon_button("media-playlist-repeat-symbolic", "control");
        let shuffle = icon_button("media-playlist-shuffle-symbolic", "control");

        controls.append(&previous);
        controls.append(&play);
        controls.append(&next);
        controls.append(&repeat);
        controls.append(&shuffle);

        let chip = gtk::Label::new(None);
        chip.add_css_class("chip");
        chip.set_ellipsize(gtk::pango::EllipsizeMode::End);
        chip.set_max_width_chars(14);
        chip.set_halign(gtk::Align::End);
        chip.set_valign(gtk::Align::Center);
        chip.set_hexpand(true);
        controls.append(&chip);

        text.append(&controls);
        row.append(&text);
        card.append(&row);

        // ---- seekbar row -----------------------------------------------
        let seek_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        seek_row.set_visible(false);

        let elapsed = gtk::Label::new(Some("0:00"));
        elapsed.add_css_class("time");
        elapsed.set_width_chars(5);
        elapsed.set_xalign(0.0);

        let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.001);
        seek.add_css_class("seek");
        seek.set_draw_value(false);
        seek.set_hexpand(true);
        seek.set_valign(gtk::Align::Center);

        let total = gtk::Label::new(Some("0:00"));
        total.add_css_class("time");
        total.set_width_chars(5);
        total.set_xalign(1.0);

        seek_row.append(&elapsed);
        seek_row.append(&seek);
        seek_row.append(&total);
        card.append(&seek_row);

        window.set_child(Some(&overlay));

        let flyout = Self {
            window,
            card,
            backdrop,
            cover,
            cover_frame,
            title,
            artist,
            chip,
            play,
            previous,
            next,
            repeat,
            shuffle,
            seek_row,
            elapsed,
            total,
            seek,
            settings,
            commands,
            updating_seek: Rc::new(Cell::new(false)),
            length_us: Rc::new(Cell::new(0)),
            loop_status: Rc::new(Cell::new(LoopStatus::None)),
            shuffle_on: Rc::new(Cell::new(false)),
            accent: RefCell::new(None),
        };

        flyout.connect_signals();
        let current = flyout.settings.borrow().current().clone();
        flyout.apply_settings(&current);
        flyout
    }

    fn connect_signals(&self) {
        let commands = self.commands.clone();
        self.play.connect_clicked(move |_| {
            let _ = commands.try_send(MediaCommand::TogglePlayPause);
        });

        let commands = self.commands.clone();
        self.previous.connect_clicked(move |_| {
            let _ = commands.try_send(MediaCommand::Previous);
        });

        let commands = self.commands.clone();
        self.next.connect_clicked(move |_| {
            let _ = commands.try_send(MediaCommand::Next);
        });

        let commands = self.commands.clone();
        let loop_status = self.loop_status.clone();
        self.repeat.connect_clicked(move |_| {
            let next = loop_status.get().next();
            let _ = commands.try_send(MediaCommand::SetLoop(next));
        });

        let commands = self.commands.clone();
        let shuffle_on = self.shuffle_on.clone();
        self.shuffle.connect_clicked(move |_| {
            let _ = commands.try_send(MediaCommand::SetShuffle(!shuffle_on.get()));
        });

        let commands = self.commands.clone();
        let length = self.length_us.clone();
        let updating = self.updating_seek.clone();
        self.seek.connect_value_changed(move |scale| {
            if updating.get() {
                return;
            }
            let length = length.get();
            if length <= 0 {
                return;
            }
            let target = (scale.value() * length as f64) as i64;
            let _ = commands.try_send(MediaCommand::SeekAbsolute(target));
        });
    }

    /// Applies the parts of the settings that change on their own (layout,
    /// which optional controls exist).
    pub fn apply_settings(&self, settings: &Settings) {
        if settings.compact_layout {
            self.card.add_css_class("compact");
        } else {
            self.card.remove_css_class("compact");
        }

        self.repeat.set_visible(settings.repeat_enabled);
        self.shuffle.set_visible(settings.shuffle_enabled);
        self.chip.set_visible(settings.player_info_enabled);

        let (align, justify) = if settings.center_title_artist {
            (0.5, gtk::Justification::Center)
        } else {
            (0.0, gtk::Justification::Left)
        };
        self.title.set_xalign(align);
        self.title.set_justify(justify);
        self.artist.set_xalign(align);
        self.artist.set_justify(justify);

        self.cover_frame.set_visible(!settings.compact_layout);
        self.apply_size(settings, self.seek_row.is_visible());
    }

    /// Full refresh from a snapshot.
    pub fn update(&self, snapshot: Option<&MediaSnapshot>, settings: &Settings) {
        let Some(snapshot) = snapshot else {
            self.title.set_text("No media playing");
            self.artist.set_text("");
            self.artist.set_visible(false);
            self.cover.set_paintable(None::<&gdk::Texture>);
            self.backdrop.set_paintable(None::<&gdk::Texture>);
            self.chip.set_text("");
            for button in [
                &self.play,
                &self.previous,
                &self.next,
                &self.repeat,
                &self.shuffle,
            ] {
                button.set_sensitive(false);
            }
            self.seek_row.set_visible(false);
            self.apply_settings(settings);
            return;
        };

        self.title.set_text(snapshot.track.display_title());
        self.artist.set_text(&snapshot.track.artist);
        self.artist
            .set_visible(!settings.compact_layout && !snapshot.track.artist.is_empty());

        self.loop_status.set(snapshot.loop_status);
        self.shuffle_on.set(snapshot.shuffle);
        self.chip.set_text(&snapshot.player.identity);

        self.play.set_sensitive(snapshot.caps.can_play_pause);
        set_icon(
            &self.play,
            if snapshot.state.is_playing() {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            },
        );
        self.previous.set_sensitive(snapshot.caps.can_go_previous);
        self.next.set_sensitive(snapshot.caps.can_go_next);

        self.repeat.set_sensitive(snapshot.caps.can_set_loop);
        set_toggle(&self.repeat, snapshot.loop_status != LoopStatus::None);
        self.shuffle.set_sensitive(snapshot.caps.can_set_shuffle);
        set_toggle(&self.shuffle, snapshot.shuffle);

        self.apply_artwork(snapshot, settings);

        // Seekbar.
        let show_seek = settings.seekbar_enabled && snapshot.seekbar_available();
        self.seek_row.set_visible(show_seek);
        self.length_us
            .set(snapshot.length_us.filter(|length| *length > 0).unwrap_or(0));
        if show_seek {
            self.updating_seek.set(true);
            self.seek.set_range(0.0, 1.0);
            self.seek.set_value(snapshot.progress_fraction());
            self.updating_seek.set(false);
            self.total.set_text(&crate::domain::media::format_duration(
                snapshot.length_us.unwrap_or(0),
            ));
            self.elapsed
                .set_text(&crate::domain::media::format_duration(snapshot.position_us));
        }

        self.cover_frame.set_visible(!settings.compact_layout);
        self.apply_size(settings, show_seek);
    }

    fn apply_artwork(&self, snapshot: &MediaSnapshot, settings: &Settings) {
        let Some(source) = &snapshot.track.artwork else {
            self.cover.set_paintable(None::<&gdk::Texture>);
            self.backdrop.set_paintable(None::<&gdk::Texture>);
            return;
        };

        if let Some(texture) = cover_art::texture_for(source) {
            self.cover.set_paintable(Some(&texture));
            self.backdrop.set_paintable(Some(&texture));
            if settings.accent_from_artwork {
                self.set_accent(&texture, settings);
            }
            return;
        }

        // Remote artwork: load off the main thread, then update the two
        // pictures directly. Widgets are reference counted, so capturing them
        // is safe even if the flyout has moved on to another track by then.
        let ArtworkSource::Remote(url) = source else {
            self.cover.set_paintable(None::<&gdk::Texture>);
            self.backdrop.set_paintable(None::<&gdk::Texture>);
            return;
        };

        let cover = self.cover.clone();
        let backdrop = self.backdrop.clone();
        let settings = settings.clone();
        let requested_url = url.clone();
        cover_art::load_remote(url, move |texture| {
            let Some(texture) = texture else {
                return;
            };
            // The user may already be listening to something else; only apply
            // the result if the URL is still the one on screen.
            log::trace!("loaded remote artwork from {requested_url}");
            cover.set_paintable(Some(&texture));
            backdrop.set_paintable(Some(&texture));
            if settings.accent_from_artwork
                && let Some(color) = cover_art::dominant_color(&texture)
            {
                style::refresh_with_accent(&settings, color);
            }
        });
    }

    fn set_accent(&self, texture: &gdk::Texture, settings: &Settings) {
        let Some(color) = cover_art::dominant_color(texture) else {
            return;
        };
        if self.accent.borrow().as_ref() == Some(&color) {
            return;
        }
        *self.accent.borrow_mut() = Some(color);
        style::refresh_with_accent(settings, color);
    }

    /// Position-only update: never touches the accent, artwork or size.
    pub fn update_position(&self, snapshot: &MediaSnapshot) {
        if !self.seek_row.is_visible() {
            return;
        }
        self.updating_seek.set(true);
        self.seek.set_value(snapshot.progress_fraction());
        self.updating_seek.set(false);
        self.elapsed
            .set_text(&crate::domain::media::format_duration(snapshot.position_us));
    }

    fn apply_size(&self, settings: &Settings, seekbar_visible: bool) {
        let (width, height) = settings.media_flyout_size(seekbar_visible);
        self.window
            .set_default_size(width.round() as i32, height.round() as i32);
        self.window
            .set_size_request(width.round() as i32, height.round() as i32);
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
}

fn icon_button(icon: &str, class: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class(class);
    button.set_child(Some(&gtk::Image::from_icon_name(icon)));
    button.set_valign(gtk::Align::Center);
    button
}

fn set_icon(button: &gtk::Button, icon: &str) {
    if let Some(image) = button
        .child()
        .and_then(|child| child.downcast::<gtk::Image>().ok())
    {
        image.set_icon_name(Some(icon));
    }
}

fn set_toggle(button: &gtk::Button, active: bool) {
    if active {
        button.add_css_class("active");
    } else {
        button.remove_css_class("active");
    }
}
