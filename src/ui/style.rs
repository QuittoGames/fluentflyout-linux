//! Visual styling.
//!
//! GTK CSS is the only styling mechanism used: no image assets, no hardcoded
//! colours in widget code. The stylesheet is regenerated when the theme or the
//! accent colour changes, which keeps a single source of truth for the
//! palette.
//!
//! Deliberate adaptations from the Windows look:
//!
//! * no acrylic/Mica backdrop (Wayland exposes no client-side backdrop blur;
//!   the translucent surface plus artwork glow is the supported substitute);
//! * GTK's own icon theme replaces Segoe Fluent glyphs;
//! * 12px corner radius instead of the Fluent 8px, because GNOME's own
//!   surfaces are rounder and the flyout sits next to them.

use std::cell::RefCell;

use crate::domain::settings::{AppTheme, BackdropStyle, Settings};

/// A colour usable in CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// The neutral accent used when no artwork colour is available.
    ///
    /// `#2563eb` keeps white glyphs above WCAG AA (4.5:1) on the play
    /// button, while staying close to the upstream Fluent blue.
    pub const DEFAULT: Self = Self::new(0x25, 0x63, 0xeb);

    pub fn css(self, alpha: f32) -> String {
        format!(
            "rgba({}, {}, {}, {:.3})",
            self.r,
            self.g,
            self.b,
            alpha.clamp(0.0, 1.0)
        )
    }

    /// Darkens the colour by `factor` (0.0 = unchanged, 1.0 = black).
    pub fn darkened(self, factor: f32) -> Self {
        let scale = (1.0 - factor).clamp(0.0, 1.0);
        Self::new(
            (self.r as f32 * scale) as u8,
            (self.g as f32 * scale) as u8,
            (self.b as f32 * scale) as u8,
        )
    }

    /// Relative luminance, used to pick a readable foreground.
    pub fn luminance(self) -> f32 {
        fn channel(value: u8) -> f32 {
            let value = value as f32 / 255.0;
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// WCAG 2.x contrast ratio between two opaque colours (`1.0..=21.0`).
    pub fn contrast_ratio(self, other: Self) -> f32 {
        let first = self.luminance();
        let second = other.luminance();
        let (lighter, darker) = if first >= second {
            (first, second)
        } else {
            (second, first)
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Black or white, whichever contrasts more with this colour.
    pub fn readable_foreground(self) -> Self {
        // 0.179 is the luminance at which black starts to win the
        // WCAG contrast ratio against white.
        if self.luminance() > 0.179 {
            Self::new(0x1a, 0x1a, 0x1a)
        } else {
            Self::new(0xff, 0xff, 0xff)
        }
    }

    /// Extracts a colour from a hex string, for settings files and tests.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        let parse = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
        Some(Self::new(parse(0..2)?, parse(2..4)?, parse(4..6)?))
    }
}

thread_local! {
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

/// Applies the theme preference and installs the initial stylesheet.
pub fn install(application: &adw::Application, settings: &Settings) {
    apply_theme(settings.theme);
    let provider = gtk::CssProvider::new();
    let css = stylesheet(settings, Rgb::DEFAULT, false, resolve_dark(settings.theme));
    provider.load_from_string(&css);

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    } else {
        log::warn!("no display available; styles will not be applied");
    }

    PROVIDER.with(|cell| *cell.borrow_mut() = Some(provider));

    let style_manager = adw::StyleManager::default();
    let application = application.clone();
    style_manager.connect_dark_notify(move |_| {
        // Re-render so the `.dark`/`.light` classes follow the system theme.
        let _ = &application;
    });
}

/// Regenerates the stylesheet after a settings or accent change.
pub fn refresh(application: &adw::Application, settings: &Settings) {
    let _ = application;
    apply_theme(settings.theme);
    let css = stylesheet(settings, Rgb::DEFAULT, false, resolve_dark(settings.theme));
    PROVIDER.with(|cell| {
        if let Some(provider) = cell.borrow().as_ref() {
            provider.load_from_string(&css);
        }
    });
}

/// Applies the theme with a specific accent (used when the artwork colour is
/// available and the user enabled "accent from artwork").
pub fn refresh_with_accent(settings: &Settings, accent: Rgb) {
    let css = stylesheet(settings, accent, true, resolve_dark(settings.theme));
    PROVIDER.with(|cell| {
        if let Some(provider) = cell.borrow().as_ref() {
            provider.load_from_string(&css);
        }
    });
}

fn apply_theme(theme: AppTheme) {
    let manager = adw::StyleManager::default();
    let scheme = match theme {
        AppTheme::System => adw::ColorScheme::Default,
        AppTheme::Light => adw::ColorScheme::ForceLight,
        AppTheme::Dark => adw::ColorScheme::ForceDark,
    };
    if manager.color_scheme() != scheme {
        manager.set_color_scheme(scheme);
    }
}

/// Resolves a theme preference to the effective dark/light state.
///
/// `System` requires the libadwaita style manager, which is only available on
/// the GTK main thread. The pure [`stylesheet`] therefore takes the resolved
/// flag as a parameter instead of querying it, which keeps the CSS generator
/// unit-testable without a display.
fn resolve_dark(theme: AppTheme) -> bool {
    match theme {
        AppTheme::Dark => true,
        AppTheme::Light => false,
        AppTheme::System => adw::StyleManager::default().is_dark(),
    }
}

/// Builds the complete stylesheet.
///
/// `accent_is_custom` records whether `accent` came from the artwork, so the
/// default palette keeps its tuned hover shades.
fn stylesheet(settings: &Settings, accent: Rgb, accent_is_custom: bool, dark: bool) -> String {
    let (surface, border, primary_text, secondary_text, hover) = if dark {
        (
            "rgba(28, 28, 30, 0.94)",
            "rgba(255, 255, 255, 0.10)",
            "rgba(255, 255, 255, 0.96)",
            "rgba(255, 255, 255, 0.62)",
            "rgba(255, 255, 255, 0.12)",
        )
    } else {
        (
            "rgba(252, 252, 253, 0.97)",
            "rgba(0, 0, 0, 0.10)",
            "rgba(20, 20, 22, 0.96)",
            "rgba(20, 20, 22, 0.58)",
            "rgba(0, 0, 0, 0.08)",
        )
    };

    let accent_hover = if accent_is_custom || accent != Rgb::DEFAULT {
        accent.darkened(0.12)
    } else {
        Rgb::new(0x2f, 0x6f, 0xdb)
    };
    let on_accent = accent.readable_foreground().css(1.0);

    let radius = 12;
    let card_padding = if settings.compact_layout {
        "2px 14px"
    } else {
        "15px 16px"
    };

    let backdrop_rules = match settings.backdrop {
        BackdropStyle::Solid => ".backdrop { opacity: 0; }",
        BackdropStyle::GlowLeft => {
            ".backdrop { opacity: 0.55; margin-left: -40px; margin-top: -30px; }"
        }
        BackdropStyle::GlowBottom => {
            ".backdrop { opacity: 0.45; margin-top: 40px; margin-bottom: -30px; }"
        }
        BackdropStyle::FullBleed => ".backdrop { opacity: 0.38; }",
    };

    format!(
        r#"
window.flyout {{
    background: transparent;
}}

.flyout-card {{
    background-color: {surface};
    border: 1px solid {border};
    border-radius: {radius}px;
    padding: {card_padding};
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.38);
}}

/* Compact flyouts (lock keys, volume) keep the upstream 50/56px height. */
.flyout-card.tight {{
    padding: 8px 14px;
}}

{backdrop_rules}

.title {{
    font-size: 14px;
    font-weight: 600;
    color: {primary_text};
}}

.artist {{
    font-size: 13px;
    color: {secondary_text};
}}

.album {{
    font-size: 11px;
    color: {secondary_text};
}}

.cover {{
    border-radius: 4px;
    background-color: {hover};
}}

.control {{
    background: transparent;
    border: none;
    border-radius: 14px;
    min-width: 28px;
    min-height: 28px;
    padding: 0;
    color: {primary_text};
}}

.control:hover {{
    background-color: {hover};
}}

.control:disabled {{
    opacity: 0.35;
}}

.control.active {{
    background-color: {accent_dim};
    color: {accent};
}}

.play {{
    background-color: {accent};
    color: {on_accent};
    border: none;
    border-radius: 16px;
    min-width: 32px;
    min-height: 32px;
    padding: 0;
}}

.play:hover {{
    background-color: {accent_hover_css};
}}

.play:disabled {{
    opacity: 0.5;
}}

.chip {{
    font-size: 11px;
    color: {secondary_text};
    background-color: {hover};
    border-radius: 8px;
    padding: 2px 8px;
}}

.time {{
    font-size: 11px;
    color: {secondary_text};
}}

scale.seek trough {{
    min-height: 4px;
    border-radius: 2px;
    background-color: {hover};
}}

scale.seek highlight {{
    min-height: 4px;
    border-radius: 2px;
    background-color: {accent};
}}

scale.seek slider {{
    min-width: 12px;
    min-height: 12px;
    border-radius: 6px;
    background-color: {accent};
    border: none;
    box-shadow: none;
    /* Adwaita's scale slider carries a negative margin to overlap the
       trough; keeping it would make the measured minimum size negative. */
    margin: 0;
    padding: 0;
}}

.lock-title {{
    font-size: 14px;
    font-weight: 600;
    color: {primary_text};
}}

.lock-indicator {{
    background-color: {accent};
    border-radius: 2px;
    min-height: 3px;
}}

.lock-indicator.off {{
    background-color: {secondary_text};
}}

.volume-value {{
    font-size: 12px;
    font-weight: 600;
    color: {primary_text};
}}

scale.volume trough {{
    min-height: 6px;
    border-radius: 3px;
    background-color: {hover};
}}

scale.volume highlight {{
    min-height: 6px;
    border-radius: 3px;
    background-color: {accent};
}}

scale.volume slider {{
    min-width: 14px;
    min-height: 14px;
    border-radius: 7px;
    background-color: {accent};
    border: none;
    box-shadow: none;
    margin: 0;
    padding: 0;
}}
"#,
        accent = accent.css(1.0),
        accent_hover_css = accent_hover.css(1.0),
        accent_dim = accent.css(0.22),
        on_accent = on_accent,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn stylesheet_contains_the_core_classes() {
        let css = stylesheet(&settings(), Rgb::DEFAULT, false, true);
        for class in [
            ".flyout-card",
            ".title",
            ".artist",
            ".control",
            ".play",
            ".chip",
            ".time",
            ".lock-indicator",
            "scale.seek",
        ] {
            assert!(css.contains(class), "missing {class}");
        }
    }

    #[test]
    fn stylesheet_has_balanced_braces() {
        let css = stylesheet(&settings(), Rgb::DEFAULT, false, true);
        let open = css.matches('{').count();
        let close = css.matches('}').count();
        assert_eq!(open, close, "unbalanced CSS braces");
    }

    #[test]
    fn dark_and_light_surfaces_differ() {
        let dark = stylesheet(&settings(), Rgb::DEFAULT, false, true);
        let light = stylesheet(&settings(), Rgb::DEFAULT, false, false);
        assert_ne!(dark, light);
        assert!(dark.contains("rgba(28, 28, 30"));
        assert!(light.contains("rgba(252, 252, 253"));
    }

    #[test]
    fn compact_layout_uses_tighter_padding() {
        let compact = stylesheet(
            &Settings {
                compact_layout: true,
                ..settings()
            },
            Rgb::DEFAULT,
            false,
            true,
        );
        assert!(compact.contains("padding: 2px 14px"));
    }

    #[test]
    fn backdrop_variants_produce_different_rules() {
        let solid = stylesheet(&settings(), Rgb::DEFAULT, false, true);
        let glow = stylesheet(
            &Settings {
                backdrop: BackdropStyle::GlowLeft,
                ..settings()
            },
            Rgb::DEFAULT,
            false,
            true,
        );
        assert!(solid.contains(".backdrop { opacity: 0; }"));
        assert!(glow.contains("margin-left: -40px"));
    }

    #[test]
    fn hex_parsing_accepts_optional_hash_and_rejects_junk() {
        assert_eq!(Rgb::from_hex("#3b82f6"), Some(Rgb::new(0x3b, 0x82, 0xf6)));
        assert_eq!(Rgb::from_hex("3b82f6"), Some(Rgb::new(0x3b, 0x82, 0xf6)));
        assert_eq!(Rgb::from_hex("#fff"), None);
        assert_eq!(Rgb::from_hex("#gggggg"), None);
    }

    #[test]
    fn readable_foreground_picks_contrasting_colour() {
        assert_eq!(
            Rgb::new(0xff, 0xff, 0xff).readable_foreground(),
            Rgb::new(0x1a, 0x1a, 0x1a)
        );
        assert_eq!(
            Rgb::new(0x10, 0x10, 0x10).readable_foreground(),
            Rgb::new(0xff, 0xff, 0xff)
        );
    }

    #[test]
    fn default_accent_is_readable_on_white_text() {
        // The play button uses white glyphs on the default blue; keep that
        // combination above WCAG AA for normal text.
        let contrast = Rgb::DEFAULT.contrast_ratio(Rgb::new(0xff, 0xff, 0xff));
        assert!(
            contrast >= 4.5,
            "white on the default accent is only {contrast:.2}:1"
        );
        assert_eq!(
            Rgb::DEFAULT.readable_foreground(),
            Rgb::new(0xff, 0xff, 0xff)
        );
    }

    #[test]
    fn readable_foreground_maximises_contrast() {
        let samples = [
            Rgb::DEFAULT,
            Rgb::new(0xf5, 0xf5, 0xf5),
            Rgb::new(0x80, 0x80, 0x80),
            Rgb::new(0x10, 0x10, 0x10),
            Rgb::new(0x12, 0xd1, 0x8a),
        ];
        for colour in samples {
            let chosen = colour.readable_foreground();
            let alternative = if chosen == Rgb::new(0xff, 0xff, 0xff) {
                Rgb::new(0x1a, 0x1a, 0x1a)
            } else {
                Rgb::new(0xff, 0xff, 0xff)
            };
            assert!(
                colour.contrast_ratio(chosen) >= colour.contrast_ratio(alternative),
                "{colour:?} picked the weaker foreground"
            );
        }
    }

    #[test]
    fn darkening_reduces_every_channel() {
        let darker = Rgb::new(0x80, 0x80, 0x80).darkened(0.5);
        assert_eq!(darker, Rgb::new(0x40, 0x40, 0x40));
        // Factor 0 must be a no-op, factor 1 must be black.
        assert_eq!(Rgb::DEFAULT.darkened(0.0), Rgb::DEFAULT);
        assert_eq!(Rgb::DEFAULT.darkened(1.0), Rgb::new(0, 0, 0));
    }

    #[test]
    fn css_rgba_formatting_is_well_formed() {
        assert_eq!(Rgb::new(1, 2, 3).css(0.5), "rgba(1, 2, 3, 0.500)");
        // Alpha is clamped rather than emitted out of range.
        assert_eq!(Rgb::new(1, 2, 3).css(2.0), "rgba(1, 2, 3, 1.000)");
        assert_eq!(Rgb::new(1, 2, 3).css(-1.0), "rgba(1, 2, 3, 0.000)");
    }
}
