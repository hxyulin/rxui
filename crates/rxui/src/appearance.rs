//! Operating-system appearance preferences and application theme modes.
//!
//! These types are portable data: the native host detects them, while headless
//! hosts and tests construct them directly. [`ThemeSet::resolve`] turns a mode and
//! a preference snapshot into the theme a following window uses.
use crate::theme_seed::{
    AccentFill, HOVER_STEP, PRESSED_STEP, away_from, from_oklch, oklch, reach,
};
use crate::{Color, Theme};

/// Light or dark system appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    /// Dark text on light surfaces.
    Light,
    /// Light text on dark surfaces.
    Dark,
}
impl ColorScheme {
    /// The scheme a theme's background suggests: dark below 18% relative luminance.
    /// Native hosts use it to match title bars to explicit application themes.
    pub fn of(theme: &Theme) -> Self {
        if luminance(theme.palette().background) < 0.18 {
            Self::Dark
        } else {
            Self::Light
        }
    }
}

/// One snapshot of the user's operating-system appearance and accessibility
/// preferences. Unknown values keep their defaults: no scheme, no accent and all
/// accessibility flags off. Platform support is documented in `docs/native-feel.md`.
///
/// The native host refreshes the snapshot when the system appearance changes and
/// when a window regains focus, and reports changes through
/// `Application::system_preferences_changed`. Animation and other consumers should
/// treat it as a value, not a subscription.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemPreferences {
    /// System light/dark appearance, when the platform reports one.
    pub color_scheme: Option<ColorScheme>,
    /// The user asked for increased contrast (macOS "Increase contrast", Windows
    /// high-contrast themes, the XDG portal `contrast` setting).
    pub high_contrast: bool,
    /// The user asked for less motion. Animations should shorten, cross-fade or skip.
    pub reduce_motion: bool,
    /// The user asked for less transparency/blur. Translucent chrome should be opaque.
    pub reduce_transparency: bool,
    /// System accent color in linear RGBA, when the platform exposes one.
    pub accent_color: Option<Color>,
}

/// How an application chooses its default theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    /// Follow the system light/dark appearance, high contrast and accent color.
    #[default]
    System,
    /// Always use the light theme.
    Light,
    /// Always use the dark theme.
    Dark,
}

/// Themes a [`ThemeMode`] chooses between. The defaults are the built-in presets.
///
/// ```
/// use rxui::{ColorScheme, SystemPreferences, Theme, ThemeMode, ThemeSet};
/// let themes = ThemeSet::default().map(Theme::compact);
/// let prefs = SystemPreferences { color_scheme: Some(ColorScheme::Light), ..Default::default() };
/// assert_eq!(themes.resolve(ThemeMode::System, &prefs), Theme::light().compact());
/// assert_eq!(themes.resolve(ThemeMode::Dark, &prefs), Theme::dark().compact());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeSet {
    /// Theme for a light appearance.
    pub light: Theme,
    /// Theme for a dark appearance.
    pub dark: Theme,
    /// Theme used in System mode while the user asks for increased contrast.
    pub high_contrast: Theme,
    /// Whether System mode replaces the accent with the system accent color.
    pub follow_accent: bool,
}
impl Default for ThemeSet {
    fn default() -> Self {
        Self {
            light: Theme::light(),
            dark: Theme::dark(),
            high_contrast: Theme::high_contrast(),
            follow_accent: true,
        }
    }
}
impl ThemeSet {
    /// Applies `f` to all three themes, for example to switch density.
    #[must_use]
    pub fn map(mut self, mut f: impl FnMut(Theme) -> Theme) -> Self {
        self.light = f(self.light);
        self.dark = f(self.dark);
        self.high_contrast = f(self.high_contrast);
        self
    }
    /// Theme for `mode` under `preferences`.
    ///
    /// Light and Dark always pick their theme unchanged. System picks by the
    /// reported scheme (dark when unknown), switches to the high-contrast theme when
    /// the user asks for more contrast, and otherwise applies the system accent
    /// when [`Self::follow_accent`] is set. The high-contrast theme never takes the
    /// system accent.
    ///
    /// Following an accent keeps the theme's accent text (white in the presets)
    /// and moves the accent's OKLCH lightness, keeping its hue and chroma, until
    /// the text reaches 4.5:1; hover and pressed step further away. Only when that
    /// would cost the accent more than 0.25 of lightness, as for the macOS yellow,
    /// does the text flip to black. The focus ring keeps 3:1 against the surfaces,
    /// and the selection and selected rows take the accent's hue at the theme's
    /// lightness with their text kept at 4.5:1.
    pub fn resolve(&self, mode: ThemeMode, preferences: &SystemPreferences) -> Theme {
        let scheme = match mode {
            ThemeMode::Light => return self.light.clone(),
            ThemeMode::Dark => return self.dark.clone(),
            ThemeMode::System => preferences.color_scheme.unwrap_or(ColorScheme::Dark),
        };
        if preferences.high_contrast {
            return self.high_contrast.clone();
        }
        let theme = match scheme {
            ColorScheme::Light => &self.light,
            ColorScheme::Dark => &self.dark,
        };
        match preferences.accent_color {
            Some(accent) if self.follow_accent && valid(accent) => with_accent(theme, accent),
            _ => theme.clone(),
        }
    }
}
fn valid(color: Color) -> bool {
    color
        .iter()
        .all(|v| v.is_finite() && (0. ..=1.).contains(v))
}

/// Relative luminance of a linear RGB color.
pub(crate) fn luminance(c: Color) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}
/// WCAG contrast ratio between two opaque linear colors.
pub(crate) fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
/// The theme with its accent-derived tokens rebuilt from the system `accent`.
///
/// The fill keeps the accent's OKLCH hue and as much chroma as the gamut allows,
/// moving only in lightness until the theme's own accent text reaches 4.5:1; see
/// [`AccentFill`] for when the text flips to black or white instead. Hover and
/// pressed step further from the text. The focus ring starts at the accent and
/// lightens (dark) or darkens (light) until it has 3:1 against the surfaces and
/// controls. Selection and selected rows keep the theme's OKLCH lightness and
/// chroma and take the accent's hue, with the chroma scaled down for accents less
/// colorful than the theme's (graphite gives neutral rows), then move away from
/// their text until it reads at 4.5:1.
fn with_accent(theme: &Theme, accent: Color) -> Theme {
    let [l, c, h] = oklch(accent);
    let dark = ColorScheme::of(theme) == ColorScheme::Dark;
    let fill = AccentFill::new([l, c, h], theme.palette().accent_text);
    let tint = (c / oklch(theme.palette().accent)[1].max(1e-3)).min(1.);
    // A theme token at its own lightness and scaled chroma in the accent's hue,
    // readable under `text`.
    let retint = |token: Color, text: Color| {
        let [tl, tc, _] = oklch(token);
        let tc = tc * tint;
        let away = away_from(text) * 0.01;
        let mut color = from_oklch(reach(tl, tc, h, away, &[text], 4.5), tc, h);
        color[3] = token[3];
        color
    };
    theme.clone().colors(|p| {
        let fills = [
            p.background,
            p.surface,
            p.control,
            p.control_hover,
            p.control_pressed,
            p.input,
        ];
        p.focus = from_oklch(
            reach(l, c, h, if dark { 0.01 } else { -0.01 }, &fills, 3.),
            c,
            h,
        );
        p.accent = fill.step(0.);
        p.accent_hover = fill.step(HOVER_STEP);
        p.accent_pressed = fill.step(PRESSED_STEP);
        p.accent_text = fill.text;
        p.selection = retint(p.selection, p.selection_text);
        p.row_selected = retint(p.row_selected, p.text);
        p.row_selected_inactive = retint(p.row_selected_inactive, p.text);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rgb8;
    use crate::theme_seed::{BLACK, WHITE};

    fn prefs(scheme: Option<ColorScheme>) -> SystemPreferences {
        SystemPreferences {
            color_scheme: scheme,
            ..Default::default()
        }
    }

    #[test]
    fn explicit_modes_ignore_the_system_and_system_mode_follows_it() {
        let themes = ThemeSet::default();
        let mut light = prefs(Some(ColorScheme::Light));
        light.high_contrast = true;
        light.accent_color = Some(rgb8(255, 0, 0));
        assert_eq!(themes.resolve(ThemeMode::Light, &light), Theme::light());
        assert_eq!(themes.resolve(ThemeMode::Dark, &light), Theme::dark());
        assert_eq!(
            themes.resolve(ThemeMode::System, &light),
            Theme::high_contrast()
        );
        assert_eq!(
            themes.resolve(ThemeMode::System, &prefs(Some(ColorScheme::Light))),
            Theme::light()
        );
        assert_eq!(
            themes.resolve(ThemeMode::System, &prefs(None)),
            Theme::dark()
        );
        assert_eq!(ColorScheme::of(&Theme::light()), ColorScheme::Light);
        assert_eq!(ColorScheme::of(&Theme::dark()), ColorScheme::Dark);
        assert_eq!(ColorScheme::of(&Theme::high_contrast()), ColorScheme::Dark);
    }

    #[test]
    fn system_accents_keep_text_states_and_focus_readable() {
        let themes = ThemeSet::default();
        // Approximate macOS accents and whether they keep the presets' white text.
        let accents = [
            ("blue", rgb8(0, 122, 255), true),
            ("purple", rgb8(149, 61, 150), true),
            ("pink", rgb8(247, 79, 158), true),
            ("red", rgb8(224, 56, 62), true),
            ("orange", rgb8(247, 130, 27), true),
            ("yellow", rgb8(255, 198, 0), false),
            ("green", rgb8(98, 186, 70), true),
            ("graphite", rgb8(140, 140, 140), true),
            ("white", rgb8(255, 255, 255), false),
            ("black", rgb8(0, 0, 0), true),
        ];
        for scheme in [ColorScheme::Light, ColorScheme::Dark] {
            let preset = match scheme {
                ColorScheme::Light => Theme::light(),
                ColorScheme::Dark => Theme::dark(),
            };
            for (name, accent, white) in accents {
                let mut p = prefs(Some(scheme));
                p.accent_color = Some(accent);
                let theme = themes.resolve(ThemeMode::System, &p);
                theme.validate().unwrap();
                let c = theme.palette();
                let case = format!("{scheme:?} {name}");
                let text = if white { WHITE } else { BLACK };
                assert_eq!(c.accent_text, text, "{case}");
                for fill in [c.accent, c.accent_hover, c.accent_pressed] {
                    assert!(contrast(c.accent_text, fill) >= 4.5, "{case}");
                }
                assert_ne!(c.accent_hover, c.accent, "{case}");
                assert_ne!(c.accent_pressed, c.accent_hover, "{case}");
                // States step away from the text: darker under white text.
                let away = |fill: Color| contrast(c.accent_text, fill);
                assert!(away(c.accent_hover) > away(c.accent), "{case}");
                assert!(away(c.accent_pressed) > away(c.accent_hover), "{case}");
                for fill in [c.background, c.surface, c.control, c.input] {
                    assert!(contrast(c.focus, fill) >= 3., "{case}");
                }
                assert!(contrast(c.selection_text, c.selection) >= 4.5, "{case}");
                for row in [c.row_selected, c.row_selected_inactive] {
                    assert!(contrast(c.text, row) >= 4.5, "{case}");
                }
                // Colorful accents keep their hue and tint the selection with it.
                let [_, chroma, hue] = oklch(accent);
                if chroma > 0.05 {
                    for token in [c.accent, c.focus, c.selection, c.row_selected] {
                        let [_, tc, th] = oklch(token);
                        let turn = (th - hue).rem_euclid(360.);
                        assert!(tc > 0.02 && turn.min(360. - turn) < 6., "{case} {token:?}");
                    }
                    assert_ne!(c.selection, preset.palette().selection, "{case}");
                } else {
                    // Neutral accents give neutral selections.
                    assert!(oklch(c.row_selected)[1] < 0.01, "{case}");
                }
                assert_eq!(ColorScheme::of(&theme), scheme);
            }
        }
        // An accent equal to the preset's reproduces its accent-tinted tokens.
        for preset in [Theme::light(), Theme::dark()] {
            let followed = with_accent(&preset, preset.palette().accent);
            let (a, b) = (followed.palette(), preset.palette());
            for (x, y) in [
                (a.accent, b.accent),
                (a.selection, b.selection),
                (a.row_selected, b.row_selected),
            ] {
                assert!(
                    x.iter().zip(y).all(|(x, y)| (x - y).abs() < 0.01),
                    "{x:?} {y:?}"
                );
            }
            assert_eq!(a.accent_text, WHITE);
        }
        let blue = SystemPreferences {
            accent_color: Some(rgb8(0, 122, 255)),
            ..prefs(Some(ColorScheme::Dark))
        };
        let followed = themes.resolve(ThemeMode::System, &blue);
        assert_ne!(followed, Theme::dark());
        let ignored = ThemeSet {
            follow_accent: false,
            ..ThemeSet::default()
        };
        assert_eq!(ignored.resolve(ThemeMode::System, &blue), Theme::dark());
        let invalid = SystemPreferences {
            accent_color: Some([f32::NAN, 0., 0., 1.]),
            ..blue
        };
        assert_eq!(themes.resolve(ThemeMode::System, &invalid), Theme::dark());
    }
}
