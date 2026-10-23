//! Settings window.
//!
//! One scrolling page of preference groups rather than the upstream tabbed
//! window: the option set is small enough that navigation chrome would add
//! clicks without adding clarity.
//!
//! Every control writes through [`SettingsStore`] and then notifies the main
//! loop, which owns debounced persistence and applies theme changes. The
//! window never touches D-Bus or the filesystem directly.

use std::cell::RefCell;
use std::rc::Rc;

use async_channel::Sender;
use gtk::prelude::*;

use adw::prelude::*;

use crate::app::AppEvent;
use crate::autostart;
use crate::domain::geometry::FlyoutAnchor;
use crate::domain::settings::{
    AppTheme, BackdropStyle, EasingStyle, FilterMode, MAX_DURATION_MS, Settings,
};
use crate::settings::SettingsStore;

/// Builds the settings window. Caller keeps the reference alive.
pub fn build(
    application: &adw::Application,
    store: Rc<RefCell<SettingsStore>>,
    events: Sender<AppEvent>,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("FluentFlyout Settings")
        .default_width(620)
        .default_height(760)
        .build();

    let header = adw::HeaderBar::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(12);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let current = store.borrow().current().clone();

    content.append(&general_group(&store, &events, &current));
    content.append(&media_group(&store, &events, &current));
    content.append(&lock_group(&store, &events, &current));
    content.append(&volume_group(&store, &events, &current));
    content.append(&filter_group(&store, &events, &current));
    content.append(&about_group());

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scrolled));
    window.set_content(Some(&toolbar));
    window
}

fn save<F>(store: &Rc<RefCell<SettingsStore>>, events: &Sender<AppEvent>, mutate: F)
where
    F: FnOnce(&mut Settings),
{
    if store.borrow_mut().update(mutate) {
        let _ = events.try_send(AppEvent::SettingsChanged);
    }
}

fn general_group(
    store: &Rc<RefCell<SettingsStore>>,
    events: &Sender<AppEvent>,
    settings: &Settings,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("General").build();

    group.add(&combo_row(
        "Theme",
        AppTheme::ALL
            .iter()
            .map(|theme| (theme.label(), theme.index()))
            .collect(),
        settings.theme.index(),
        {
            let store = store.clone();
            let events = events.clone();
            move |index| {
                save(&store, &events, |settings| {
                    settings.theme = AppTheme::from_index(index);
                });
            }
        },
    ));

    group.add(&combo_row(
        "Position",
        FlyoutAnchor::ALL
            .iter()
            .map(|anchor| (anchor.label(), anchor.index()))
            .collect(),
        settings.position.index(),
        {
            let store = store.clone();
            let events = events.clone();
            move |index| {
                save(&store, &events, |settings| {
                    settings.position = FlyoutAnchor::from_index(index);
                });
            }
        },
    ));

    group.add(&switch_row(
        "Compact layout",
        Some("A single wide row without artwork, 400px across"),
        settings.compact_layout,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| settings.compact_layout = active);
            }
        },
    ));

    group.add(&combo_row(
        "Animation speed",
        vec![
            ("Off", 0),
            ("0.5x", 1),
            ("1x", 2),
            ("1.5x", 3),
            ("2x", 4),
            ("3x", 5),
        ],
        settings.animation_speed,
        {
            let store = store.clone();
            let events = events.clone();
            move |index| {
                save(&store, &events, |settings| settings.animation_speed = index);
            }
        },
    ));

    group.add(&combo_row(
        "Easing",
        EasingStyle::ALL
            .iter()
            .map(|style| (style.label(), style.index()))
            .collect(),
        settings.easing.index(),
        {
            let store = store.clone();
            let events = events.clone();
            move |index| {
                save(&store, &events, |settings| {
                    settings.easing = EasingStyle::from_index(index);
                });
            }
        },
    ));

    group.add(&switch_row(
        "Start automatically",
        Some("Adds an XDG autostart entry for the current executable"),
        autostart::is_enabled(),
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                if let Err(error) = autostart::set_enabled(active) {
                    log::error!("cannot change autostart: {error}");
                }
                save(&store, &events, |settings| settings.autostart = active);
            }
        },
    ));

    group
}

fn media_group(
    store: &Rc<RefCell<SettingsStore>>,
    events: &Sender<AppEvent>,
    settings: &Settings,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Media flyout")
        .description("Appears when the active player changes track or state, or on request")
        .build();

    group.add(&switch_row(
        "Enable the media flyout",
        None,
        settings.media_flyout_enabled,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.media_flyout_enabled = active;
                });
            }
        },
    ));

    group.add(&switch_row(
        "Show on player changes",
        Some("Linux adaptation: media keys are owned by the desktop, so the flyout reacts to the player instead"),
        settings.show_on_media_change,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.show_on_media_change = active;
                });
            }
        },
    ));

    group.add(&switch_row(
        "Keep on screen",
        Some("Do not hide automatically"),
        settings.media_flyout_always_display,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.media_flyout_always_display = active;
                });
            }
        },
    ));

    group.add(&spin_row(
        "Display duration",
        Some("Milliseconds before the flyout hides"),
        settings.duration_ms,
        0,
        MAX_DURATION_MS,
        100,
        {
            let store = store.clone();
            let events = events.clone();
            move |value| {
                save(&store, &events, |settings| settings.duration_ms = value);
            }
        },
    ));

    group.add(&switch_row(
        "Show the active player",
        None,
        settings.player_info_enabled,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.player_info_enabled = active;
                });
            }
        },
    ));

    group.add(&switch_row(
        "Show the seekbar",
        Some("Adds 36px when the player exposes a length"),
        settings.seekbar_enabled,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.seekbar_enabled = active
                });
            }
        },
    ));

    group.add(&switch_row(
        "Repeat button",
        None,
        settings.repeat_enabled,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| settings.repeat_enabled = active);
            }
        },
    ));

    group.add(&switch_row(
        "Shuffle button",
        None,
        settings.shuffle_enabled,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.shuffle_enabled = active
                });
            }
        },
    ));

    group.add(&switch_row(
        "Centre title and artist",
        None,
        settings.center_title_artist,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.center_title_artist = active;
                });
            }
        },
    ));

    group.add(&switch_row(
        "Pause other players",
        Some("Pause every other MPRIS player when one starts playing"),
        settings.pause_other_sessions,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.pause_other_sessions = active;
                });
            }
        },
    ));

    group.add(&combo_row(
        "Backdrop",
        BackdropStyle::ALL
            .iter()
            .map(|style| (style.label(), style.index()))
            .collect(),
        settings.backdrop.index(),
        {
            let store = store.clone();
            let events = events.clone();
            move |index| {
                save(&store, &events, |settings| {
                    settings.backdrop = BackdropStyle::from_index(index);
                });
            }
        },
    ));

    group.add(&switch_row(
        "Accent from artwork",
        Some("Tint the play button with the cover's dominant colour"),
        settings.accent_from_artwork,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.accent_from_artwork = active;
                });
            }
        },
    ));

    group
}

fn lock_group(
    store: &Rc<RefCell<SettingsStore>>,
    events: &Sender<AppEvent>,
    settings: &Settings,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Lock keys")
        .description("Read from LED class devices; Insert has no kernel LED and is event-only")
        .build();

    group.add(&switch_row(
        "Enable lock-key flyouts",
        None,
        settings.lock_keys_enabled,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.lock_keys_enabled = active
                });
            }
        },
    ));

    for (label, get, set) in [
        (
            "Caps Lock",
            settings.lock_keys_caps,
            (|settings: &mut Settings, value: bool| settings.lock_keys_caps = value)
                as fn(&mut Settings, bool),
        ),
        (
            "Num Lock",
            settings.lock_keys_num,
            (|settings: &mut Settings, value: bool| settings.lock_keys_num = value)
                as fn(&mut Settings, bool),
        ),
        (
            "Scroll Lock",
            settings.lock_keys_scroll,
            (|settings: &mut Settings, value: bool| settings.lock_keys_scroll = value)
                as fn(&mut Settings, bool),
        ),
        (
            "Insert",
            settings.lock_keys_insert,
            (|settings: &mut Settings, value: bool| settings.lock_keys_insert = value)
                as fn(&mut Settings, bool),
        ),
    ] {
        let store = store.clone();
        let events = events.clone();
        group.add(&switch_row(label, None, get, move |active| {
            save(&store, &events, |settings| set(settings, active));
        }));
    }

    group.add(&spin_row(
        "Display duration",
        Some("Milliseconds before the lock flyout hides"),
        settings.lock_keys_duration_ms,
        0,
        MAX_DURATION_MS,
        100,
        {
            let store = store.clone();
            let events = events.clone();
            move |value| {
                save(&store, &events, |settings| {
                    settings.lock_keys_duration_ms = value;
                });
            }
        },
    ));

    group.add(&switch_row("Animate", None, settings.lock_keys_animated, {
        let store = store.clone();
        let events = events.clone();
        move |active| {
            save(&store, &events, |settings| {
                settings.lock_keys_animated = active;
            });
        }
    }));

    group
}

fn volume_group(
    store: &Rc<RefCell<SettingsStore>>,
    events: &Sender<AppEvent>,
    settings: &Settings,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Volume")
        .description("Controls the WirePlumber default sink")
        .build();

    group.add(&switch_row(
        "Enable the volume flyout",
        Some("Uses wpctl; unavailable when no PipeWire sink exists"),
        settings.volume_control_enabled,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.volume_control_enabled = active;
                });
            }
        },
    ));

    group.add(&spin_row(
        "Display duration",
        None,
        settings.volume_duration_ms,
        0,
        MAX_DURATION_MS,
        100,
        {
            let store = store.clone();
            let events = events.clone();
            move |value| {
                save(&store, &events, |settings| {
                    settings.volume_duration_ms = value
                });
            }
        },
    ));

    group
}

fn filter_group(
    store: &Rc<RefCell<SettingsStore>>,
    events: &Sender<AppEvent>,
    settings: &Settings,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Application filtering")
        .description("Match MPRIS players by desktop entry or bus name")
        .build();

    group.add(&switch_row(
        "Enable filtering",
        None,
        settings.app_filtering_enabled,
        {
            let store = store.clone();
            let events = events.clone();
            move |active| {
                save(&store, &events, |settings| {
                    settings.app_filtering_enabled = active;
                });
            }
        },
    ));

    group.add(&combo_row(
        "Mode",
        vec![
            ("Only allow listed players", 0),
            ("Block listed players", 1),
        ],
        match settings.app_filter_mode {
            FilterMode::AllowList => 0,
            FilterMode::BlockList => 1,
        },
        {
            let store = store.clone();
            let events = events.clone();
            move |index| {
                save(&store, &events, |settings| {
                    settings.app_filter_mode = if index == 0 {
                        FilterMode::AllowList
                    } else {
                        FilterMode::BlockList
                    };
                });
            }
        },
    ));

    group.add(&filter_list_editor(
        "Allowed players",
        true,
        store.clone(),
        events.clone(),
    ));
    group.add(&filter_list_editor(
        "Blocked players",
        false,
        store.clone(),
        events.clone(),
    ));

    group
}

/// Entry row plus the current list, rebuilt whenever an entry is added or
/// removed.
fn filter_list_editor(
    title: &str,
    allow_list: bool,
    store: Rc<RefCell<SettingsStore>>,
    events: Sender<AppEvent>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title(title).build();

    let entry = adw::EntryRow::builder().title("Add a player").build();
    let add = gtk::Button::from_icon_name("list-add-symbolic");
    add.add_css_class("flat");
    add.set_valign(gtk::Align::Center);
    entry.add_suffix(&add);

    let rebuild: Rc<dyn Fn()> = Rc::new({
        let group = group.clone();
        let store = store.clone();
        let events_for_rows = events.clone();
        move || {
            let entries = {
                let store = store.borrow();
                let settings = store.current();
                if allow_list {
                    settings.allowed_apps.clone()
                } else {
                    settings.blocked_apps.clone()
                }
            };

            // Drop the rows that represent entries, keep the entry editor.
            let mut child = group.first_child();
            while let Some(widget) = child {
                let next = widget.next_sibling();
                if widget.downcast_ref::<adw::EntryRow>().is_none()
                    && widget.downcast_ref::<adw::ActionRow>().is_some()
                {
                    group.remove(&widget);
                }
                child = next;
            }

            for name in entries {
                let row = adw::ActionRow::builder().title(&name).build();
                let remove = gtk::Button::from_icon_name("user-trash-symbolic");
                remove.add_css_class("flat");
                remove.set_valign(gtk::Align::Center);
                {
                    let store = store.clone();
                    let group = group.clone();
                    let name = name.clone();
                    let events = events_for_rows.clone();
                    remove.connect_clicked(move |_| {
                        save(&store, &events, |settings| {
                            let list = if allow_list {
                                &mut settings.allowed_apps
                            } else {
                                &mut settings.blocked_apps
                            };
                            list.retain(|existing| existing != &name);
                        });
                        // The row disappears on the next rebuild.
                        let mut child = group.first_child();
                        while let Some(widget) = child {
                            let next = widget.next_sibling();
                            if let Some(row) = widget.downcast_ref::<adw::ActionRow>()
                                && row.title() == name
                            {
                                group.remove(&widget);
                            }
                            child = next;
                        }
                    });
                }
                row.add_suffix(&remove);
                group.add(&row);
            }
        }
    });

    group.add(&entry);
    rebuild();

    {
        let entry = entry.clone();
        let store = store.clone();
        let events = events.clone();
        let rebuild = rebuild.clone();
        add.connect_clicked(move |_| {
            let text = entry.text().trim().to_string();
            if text.is_empty() {
                return;
            }
            entry.set_text("");
            save(&store, &events, |settings| {
                let list = if allow_list {
                    &mut settings.allowed_apps
                } else {
                    &mut settings.blocked_apps
                };
                if !list
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&text))
                {
                    list.push(text);
                }
            });
            rebuild();
        });
    }

    {
        let store = store.clone();
        let events = events.clone();
        let rebuild = rebuild.clone();
        entry.connect_apply(move |entry| {
            let text = entry.text().trim().to_string();
            if text.is_empty() {
                return;
            }
            entry.set_text("");
            save(&store, &events, |settings| {
                let list = if allow_list {
                    &mut settings.allowed_apps
                } else {
                    &mut settings.blocked_apps
                };
                if !list
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&text))
                {
                    list.push(text);
                }
            });
            rebuild();
        });
    }

    group
}

fn about_group() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("About").build();

    let name = adw::ActionRow::builder()
        .title("FluentFlyout for Linux")
        .subtitle(format!("Version {}", env!("CARGO_PKG_VERSION")))
        .build();
    group.add(&name);

    let upstream = adw::ActionRow::builder()
        .title("Based on FluentFlyout")
        .subtitle("unchihugo/FluentFlyout — GPL-3.0-or-later")
        .build();
    upstream.add_suffix(&gtk::Image::from_icon_name("external-link-symbolic"));
    group.add(&upstream);

    let license = adw::ActionRow::builder()
        .title("License")
        .subtitle("GPL-3.0-or-later")
        .build();
    group.add(&license);

    let note = adw::ActionRow::builder()
        .title("Platform notes")
        .subtitle(
            "Wayland does not let a client position its own windows. Anchored placement needs a \
             compositor with layer-shell; GNOME places the flyout itself.",
        )
        .build();
    note.set_subtitle_lines(3);
    group.add(&note);

    group
}

fn switch_row(
    title: &str,
    subtitle: Option<&str>,
    active: bool,
    on_change: impl Fn(bool) + 'static,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
        row.set_subtitle_lines(2);
    }

    let toggle = gtk::Switch::new();
    toggle.set_active(active);
    toggle.set_valign(gtk::Align::Center);
    toggle.connect_active_notify(move |toggle| on_change(toggle.is_active()));

    row.add_suffix(&toggle);
    row.set_activatable_widget(Some(&toggle));
    row
}

fn combo_row(
    title: &str,
    options: Vec<(&str, i64)>,
    current: i64,
    on_change: impl Fn(i64) + 'static,
) -> adw::ComboRow {
    let labels: Vec<&str> = options.iter().map(|(label, _)| *label).collect();
    let list = gtk::StringList::new(&labels);
    let values: Vec<i64> = options.iter().map(|(_, value)| *value).collect();

    let row = adw::ComboRow::builder().title(title).model(&list).build();
    if let Some(index) = values.iter().position(|value| *value == current) {
        row.set_selected(index as u32);
    }

    row.connect_selected_notify(move |row| {
        if let Some(value) = values.get(row.selected() as usize) {
            on_change(*value);
        }
    });
    row
}

fn spin_row(
    title: &str,
    subtitle: Option<&str>,
    value: i64,
    min: i64,
    max: i64,
    step: i64,
    on_change: impl Fn(i64) + 'static,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
        row.set_subtitle_lines(2);
    }

    let spin = gtk::SpinButton::with_range(min as f64, max as f64, step as f64);
    spin.set_value(value as f64);
    spin.set_numeric(true);
    spin.set_valign(gtk::Align::Center);
    spin.connect_value_changed(move |spin| on_change(spin.value() as i64));

    row.add_suffix(&spin);
    row.set_activatable_widget(Some(&spin));
    row
}
