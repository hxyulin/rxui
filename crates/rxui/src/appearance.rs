//! Operating-system appearance preferences and application theme modes.
//!
//! These types are portable data: the native host detects them, while headless
//! hosts and tests construct them directly. [`ThemeSet::resolve`] turns a mode and
//! a preference snapshot into the theme a following window uses.
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
    /// when [`Self::follow_accent`] is set. The derived accent keeps accent text,
    /// hover/pressed fills and the focus ring readable against the theme.
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
fn mix(a: Color, b: Color, t: f32) -> Color {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        1.,
    ]
}
/// Moves `color` toward `target` until it reaches `min` contrast with every fill.
fn ensure_contrast(mut color: Color, fills: &[Color], min: f32, target: Color) -> Color {
    for _ in 0..20 {
        if fills.iter().all(|f| contrast(color, *f) >= min) {
            break;
        }
        color = mix(color, target, 0.1);
    }
    color
}
const WHITE: Color = [1., 1., 1., 1.];
const BLACK: Color = [0., 0., 0., 1.];

fn with_accent(theme: &Theme, accent: Color) -> Theme {
    let accent = [accent[0], accent[1], accent[2], 1.];
    let dark = ColorScheme::of(theme) == ColorScheme::Dark;
    theme.clone().colors(|c| {
        // White text where it reads at least as well as black, as system buttons do.
        let text = if contrast(WHITE, accent) >= contrast(BLACK, accent) {
            WHITE
        } else {
            BLACK
        };
        let away = if text == WHITE { BLACK } else { WHITE };
        let base = ensure_contrast(accent, &[text], 4.5, away);
        // Hover/pressed step away from the text color so states stay readable.
        let hover = ensure_contrast(mix(base, away, 0.08), &[text], 4.5, away);
        let pressed = ensure_contrast(mix(base, away, 0.18), &[text], 4.5, away);
        let fills = [
            c.background,
            c.surface,
            c.control,
            c.control_hover,
            c.control_pressed,
            c.input,
        ];
        c.focus = ensure_contrast(accent, &fills, 3., if dark { WHITE } else { BLACK });
        c.accent = base;
        c.accent_hover = hover;
        c.accent_pressed = pressed;
        c.accent_text = text;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rgb8;

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
        let accents = [
            rgb8(0, 122, 255),   // macOS blue
            rgb8(255, 199, 0),   // macOS yellow
            rgb8(152, 152, 157), // macOS graphite
            rgb8(98, 186, 70),   // macOS green
            rgb8(255, 255, 255),
            rgb8(0, 0, 0),
        ];
        for scheme in [ColorScheme::Light, ColorScheme::Dark] {
            for accent in accents {
                let mut p = prefs(Some(scheme));
                p.accent_color = Some(accent);
                let theme = themes.resolve(ThemeMode::System, &p);
                theme.validate().unwrap();
                let c = theme.palette();
                for fill in [c.accent, c.accent_hover, c.accent_pressed] {
                    assert!(
                        contrast(c.accent_text, fill) >= 4.5,
                        "{scheme:?} {accent:?}"
                    );
                }
                for fill in [c.background, c.surface, c.control, c.input] {
                    assert!(contrast(c.focus, fill) >= 3., "{scheme:?} {accent:?}");
                }
                assert_eq!(ColorScheme::of(&theme), scheme);
            }
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
