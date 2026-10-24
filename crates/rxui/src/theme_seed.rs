//! Complete themes generated from a scheme, an accent and a neutral tint.
//!
//! Lightness steps are chosen in OKLCH, where equal lightness looks equally light
//! across hues, so one ramp serves every accent and neutral tint.
use crate::appearance::{contrast, luminance};
use crate::{Color, ColorScheme, Theme, ThemeColors, ThemeMetrics, ThemeShadows, rgba8};

/// Inputs of [`Theme::generate`].
///
/// ```
/// use rxui::{ColorScheme, Theme, ThemeSeed, rgb8};
/// let theme = Theme::generate(
///     ThemeSeed::new(ColorScheme::Dark, rgb8(16, 163, 127)).neutral(250., 0.012),
/// );
/// theme.validate().unwrap();
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeSeed {
    /// Light or dark surfaces.
    pub scheme: ColorScheme,
    /// Accent in linear RGBA; alpha is ignored. Its lightness may be adjusted so
    /// accent text stays readable.
    pub accent: Color,
    /// OKLCH hue in degrees that tints the neutral surfaces, borders and text.
    pub neutral_hue: f32,
    /// OKLCH chroma of the neutral tint: 0 for pure grays, about 0.02 for a
    /// clearly tinted neutral.
    pub neutral_chroma: f32,
}
impl ThemeSeed {
    /// A seed whose neutrals carry a faint tint of the accent's hue.
    pub fn new(scheme: ColorScheme, accent: Color) -> Self {
        Self {
            scheme,
            accent,
            neutral_hue: oklch(accent)[2],
            neutral_chroma: 0.006,
        }
    }
    /// Selects the neutral tint's OKLCH hue in degrees and chroma.
    #[must_use]
    pub fn neutral(mut self, hue: f32, chroma: f32) -> Self {
        self.neutral_hue = hue;
        self.neutral_chroma = chroma;
        self
    }
}

pub(crate) const WHITE: Color = [1., 1., 1., 1.];
pub(crate) const BLACK: Color = [0., 0., 0., 1.];

impl Theme {
    /// A complete theme generated from a seed, with balanced metrics and the
    /// preset shadows of its scheme.
    ///
    /// Neutrals follow a fixed OKLCH lightness ramp. Accent text is white, as in
    /// the presets, and the accent's OKLCH lightness moves away from it, keeping
    /// hue and as much chroma as the gamut allows, until they reach 4.5:1; hover
    /// and pressed step further away. When that would cost the accent more than
    /// 0.25 of lightness, as for yellow or lime, the text turns black instead.
    /// The fill stays short of pure white or black. The focus ring keeps 3:1
    /// against the surfaces, and status colors use fixed hues. The result does not reproduce the presets
    /// exactly, and a non-finite seed yields a theme that fails [`Theme::validate`].
    pub fn generate(seed: ThemeSeed) -> Self {
        let dark = seed.scheme == ColorScheme::Dark;
        let (hue, chroma) = (seed.neutral_hue, seed.neutral_chroma.max(0.));
        let n = |l: f32| from_oklch(l, chroma, hue);
        // Text carries less tint so it reads as neutral.
        let t = |l: f32| from_oklch(l, chroma * 0.5, hue);
        let pick = |dark_l: f32, light_l: f32| if dark { dark_l } else { light_l };

        let [al, ac, ah] = oklch(seed.accent);
        let accent = AccentFill::new([al, ac, ah], WHITE);

        let background = n(pick(0.18, 0.975));
        let surface = n(pick(0.205, 1.));
        let control = n(pick(0.255, 1.));
        let input = n(pick(0.155, 1.));
        let focus_l = reach(
            al,
            ac,
            ah,
            if dark { 0.01 } else { -0.01 },
            &[background, surface, control, input],
            3.,
        );
        let text = t(pick(0.93, 0.22));
        // Status colors at fixed OKLCH lightness, chroma and hue per scheme.
        let status = |dark: [f32; 3], light: [f32; 3]| {
            let [l, c, h] = if seed.scheme == ColorScheme::Dark {
                dark
            } else {
                light
            };
            from_oklch(l, c, h)
        };
        let subtle = |c: Color| [c[0], c[1], c[2], if dark { 41. } else { 31. } / 255.];
        let danger = status([0.64, 0.2, 23.], [0.58, 0.2, 25.]);
        let warning = status([0.79, 0.16, 72.], [0.64, 0.14, 70.]);
        let success = status([0.65, 0.14, 157.], [0.54, 0.12, 160.]);
        let colors = ThemeColors {
            background,
            surface,
            raised: n(pick(0.235, 1.)),
            control,
            control_hover: n(pick(0.285, 0.965)),
            control_pressed: n(pick(0.31, 0.94)),
            control_disabled: n(pick(0.22, 0.975)),
            text,
            text_muted: t(pick(0.72, 0.51)),
            text_disabled: t(pick(0.5, 0.74)),
            border: n(pick(0.33, 0.875)),
            border_disabled: n(pick(0.27, 0.925)),
            input,
            input_border: n(pick(0.33, 0.855)),
            divider: n(pick(0.27, 0.925)),
            accent: accent.step(0.),
            accent_hover: accent.step(HOVER_STEP),
            accent_pressed: accent.step(PRESSED_STEP),
            accent_text: accent.text,
            focus: from_oklch(focus_l, ac, ah),
            selection: from_oklch(pick(0.42, 0.88), ac.min(pick(0.12, 0.07)), ah),
            selection_text: text,
            caret: text,
            preedit: text,
            row_hover: if dark {
                rgba8(255, 255, 255, 12)
            } else {
                rgba8(0, 0, 0, 10)
            },
            row_selected: from_oklch(pick(0.32, 0.94), ac.min(pick(0.07, 0.04)), ah),
            row_selected_inactive: n(pick(0.27, 0.945)),
            danger,
            warning,
            success,
            danger_subtle: subtle(danger),
            warning_subtle: subtle(warning),
            success_subtle: subtle(success),
        };
        let shadows = if dark {
            ThemeShadows::dark()
        } else {
            ThemeShadows::light()
        };
        Self::from_parts(colors, ThemeMetrics::default(), shadows)
    }
}

/// Most OKLCH lightness an accent gives up so its preferred text reaches 4.5:1.
/// Mid-tone blues, purples, pinks, reds, greens, oranges and grays need well under
/// it; bright yellows and limes need about 0.29 and would turn olive or brown, so
/// they take the opposite text color instead.
pub(crate) const MAX_ACCENT_SHIFT: f32 = 0.25;
/// OKLCH lightness steps of the accent's hover and pressed states away from the
/// accent text.
pub(crate) const HOVER_STEP: f32 = 0.04;
pub(crate) const PRESSED_STEP: f32 = 0.08;

/// An accent fill readable under its text: OKLCH hue and chroma of the source,
/// and a lightness that reaches 4.5:1 with `text`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AccentFill {
    /// Text over the fill.
    pub(crate) text: Color,
    lightness: f32,
    chroma: f32,
    hue: f32,
    /// Lightness direction away from the text: -1 darkens, 1 lightens.
    away: f32,
}
impl AccentFill {
    /// Keeps `preferred` text unless the fill would have to move more than
    /// [`MAX_ACCENT_SHIFT`] in lightness to reach 4.5:1 with it; then the
    /// opposite of black and white is used when that needs a smaller move.
    pub(crate) fn new([l, c, h]: [f32; 3], preferred: Color) -> Self {
        let fill = |text: Color| {
            let away = away_from(text);
            let lightness = reach(l, c, h, away * 0.01, &[text], 4.5);
            Self {
                text,
                lightness,
                chroma: c,
                hue: h,
                away,
            }
        };
        let shift = |f: &Self| (f.lightness - l).abs();
        let mut chosen = fill(preferred);
        if shift(&chosen) > MAX_ACCENT_SHIFT {
            let other = fill(if luminance(preferred) > 0.5 {
                BLACK
            } else {
                WHITE
            });
            if shift(&other) < shift(&chosen) {
                chosen = other;
            }
        }
        // Leave room for the hover and pressed steps, so they stay distinct from
        // a near-white or near-black accent.
        chosen.lightness = chosen.lightness.clamp(0.08, 0.92);
        chosen
    }
    /// The fill moved `d` further away from the text: 0 for the accent itself,
    /// [`HOVER_STEP`] and [`PRESSED_STEP`] for its states.
    pub(crate) fn step(&self, d: f32) -> Color {
        from_oklch(
            (self.lightness + self.away * d).clamp(0., 1.),
            self.chroma,
            self.hue,
        )
    }
}

/// Lightness direction that raises contrast with `text`: -1 darkens under light
/// text, 1 lightens under dark text. 0.18 is the luminance with equal contrast
/// against white and black.
pub(crate) fn away_from(text: Color) -> f32 {
    if luminance(text) > 0.18 { -1. } else { 1. }
}

/// OKLCH lightness from `l`, moving by `step` until `(l, c, h)` reaches `min`
/// contrast with every color in `against` or the lightness range ends.
pub(crate) fn reach(mut l: f32, c: f32, h: f32, step: f32, against: &[Color], min: f32) -> f32 {
    for _ in 0..100 {
        let color = from_oklch(l, c, h);
        if against.iter().all(|a| contrast(color, *a) >= min) {
            break;
        }
        l = (l + step).clamp(0., 1.);
    }
    l
}

/// Linear sRGB to OKLab `[L, a, b]`.
fn oklab(c: Color) -> [f32; 3] {
    let [r, g, b] = [c[0], c[1], c[2]];
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}
/// OKLab to linear sRGB, unclamped.
fn linear_from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ]
}
/// Linear sRGB to OKLCH `[L, C, h°]`.
pub(crate) fn oklch(c: Color) -> [f32; 3] {
    let [l, a, b] = oklab(c);
    [l, a.hypot(b), b.atan2(a).to_degrees().rem_euclid(360.)]
}
/// Opaque linear sRGB for an OKLCH color, reducing chroma until it fits the gamut.
pub(crate) fn from_oklch(l: f32, c: f32, h: f32) -> Color {
    let (sin, cos) = h.to_radians().sin_cos();
    let rgb = |c: f32| linear_from_oklab([l, c * cos, c * sin]);
    let inside = |v: [f32; 3]| v.iter().all(|x| (-1e-4..=1. + 1e-4).contains(x));
    let mut best = rgb(c);
    if !inside(best) {
        let (mut lo, mut hi) = (0., c);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.;
            if inside(rgb(mid)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        best = rgb(lo);
    }
    let [r, g, b] = best.map(|v| if v.is_nan() { v } else { v.clamp(0., 1.) });
    [r, g, b, 1.]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rgb8;

    #[test]
    fn oklch_round_trips_and_maps_out_of_gamut_chroma() {
        for color in [rgb8(58, 111, 229), rgb8(255, 255, 255), rgb8(17, 18, 20)] {
            let [l, c, h] = oklch(color);
            let back = from_oklch(l, c, h);
            for i in 0..3 {
                assert!((back[i] - color[i]).abs() < 1e-3, "{color:?} {back:?}");
            }
        }
        let white = oklch(WHITE);
        assert!((white[0] - 1.).abs() < 1e-3 && white[1] < 1e-3);
        // Pure chroma far beyond sRGB still yields a valid, in-gamut color.
        let mapped = from_oklch(0.7, 0.5, 150.);
        assert!(mapped.iter().all(|v| (0. ..=1.).contains(v)));
    }
}
