use crate::{Color, UiError};
use std::sync::{Arc, OnceLock};

/// Converts sRGB byte channels to the linear RGBA used by Astrelis. Alpha is opaque.
pub fn rgb8(r: u8, g: u8, b: u8) -> Color {
    rgba8(r, g, b, 255)
}
/// Converts sRGB byte channels and a linear alpha byte to linear RGBA.
pub fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Color {
    fn channel(v: u8) -> f32 {
        let v = f32::from(v) / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    [channel(r), channel(g), channel(b), f32::from(a) / 255.]
}
/// Semantic color binding resolved in the element's nearest theme scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeColor {
    /// Window canvas.
    Background,
    /// Panel fill.
    Surface,
    /// Menu, popover and dialog fill.
    Raised,
    /// Ordinary control fill.
    Control,
    /// Hovered control fill.
    ControlHover,
    /// Pressed control fill.
    ControlPressed,
    /// Disabled control fill.
    ControlDisabled,
    /// Main text.
    Text,
    /// Secondary text.
    TextMuted,
    /// Disabled control text.
    TextDisabled,
    /// Control boundary.
    Border,
    /// Disabled boundary.
    BorderDisabled,
    /// Text input fill.
    Input,
    /// Text input boundary; stronger than Border.
    InputBorder,
    /// Subtle separator between regions.
    Divider,
    /// Primary action fill.
    Accent,
    /// Hovered primary action fill.
    AccentHover,
    /// Pressed primary action fill.
    AccentPressed,
    /// Text painted over Accent.
    AccentText,
    /// Keyboard focus outline.
    Focus,
    /// Text selection background.
    Selection,
    /// Text painted over a selection.
    SelectionText,
    /// Input caret.
    Caret,
    /// IME composition underline.
    Preedit,
}
/// A literal linear RGBA value or a retained semantic token. Tokens resolve again
/// on a theme switch; literals remain unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StyleColor {
    /// Explicit linear RGBA color.
    Literal(Color),
    /// Color from the nearest theme.
    Theme(ThemeColor),
}
impl From<Color> for StyleColor {
    fn from(color: Color) -> Self {
        Self::Literal(color)
    }
}
impl From<ThemeColor> for StyleColor {
    fn from(color: ThemeColor) -> Self {
        Self::Theme(color)
    }
}
impl StyleColor {
    pub(crate) fn resolve(self, theme: &Theme) -> Color {
        match self {
            Self::Literal(c) => c,
            Self::Theme(token) => theme.color(token),
        }
    }
    pub(crate) fn valid(self) -> bool {
        match self {
            Self::Literal(c) => valid_color(c),
            Self::Theme(_) => true,
        }
    }
}
pub(crate) fn valid_color(color: Color) -> bool {
    color
        .iter()
        .all(|v| v.is_finite() && (0. ..=1.).contains(v))
}
/// Linear RGBA palette. Mutate with Theme::colors; validation occurs when installing
/// a theme or preparing a scoped theme. rgb8/rgba8 accept familiar sRGB byte values.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeColors {
    /// Window canvas.
    pub background: Color,
    /// Panel fill.
    pub surface: Color,
    /// Menu, popover and dialog fill.
    pub raised: Color,
    /// Ordinary control fill.
    pub control: Color,
    /// Hovered control fill.
    pub control_hover: Color,
    /// Pressed control fill.
    pub control_pressed: Color,
    /// Disabled control fill.
    pub control_disabled: Color,
    /// Main text.
    pub text: Color,
    /// Secondary text.
    pub text_muted: Color,
    /// Disabled control text.
    pub text_disabled: Color,
    /// Control boundary.
    pub border: Color,
    /// Disabled boundary.
    pub border_disabled: Color,
    /// Text input fill.
    pub input: Color,
    /// Text input boundary.
    pub input_border: Color,
    /// Subtle separator between regions.
    pub divider: Color,
    /// Primary action fill.
    pub accent: Color,
    /// Hovered primary action fill.
    pub accent_hover: Color,
    /// Pressed primary action fill.
    pub accent_pressed: Color,
    /// Text over accent fills.
    pub accent_text: Color,
    /// Focus outline.
    pub focus: Color,
    /// Selection background, paired with selection_text.
    pub selection: Color,
    /// Foreground over a selection, independent of ordinary text.
    pub selection_text: Color,
    /// Input caret.
    pub caret: Color,
    /// IME underline.
    pub preedit: Color,
}
impl ThemeColors {
    fn values(&self) -> [Color; 24] {
        [
            self.background,
            self.surface,
            self.raised,
            self.control,
            self.control_hover,
            self.control_pressed,
            self.control_disabled,
            self.text,
            self.text_muted,
            self.text_disabled,
            self.border,
            self.border_disabled,
            self.input,
            self.input_border,
            self.divider,
            self.accent,
            self.accent_hover,
            self.accent_pressed,
            self.accent_text,
            self.focus,
            self.selection,
            self.selection_text,
            self.caret,
            self.preedit,
        ]
    }
}
/// Default typography/control metrics in logical units. Explicit element builders
/// take precedence. Changing font size or box metrics invalidates affected layout.
///
/// Text lines are `font_size * 1.4` tall, so a button is
/// `font_size * 1.4 + 2 * (button_padding_y + border_width)` tall. The presets pick
/// paddings that make buttons exactly `input_height`, so they line up in a row.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeMetrics {
    /// Root/inherited text size; strictly positive.
    pub font_size: f32,
    /// Default horizontal button padding.
    pub button_padding_x: f32,
    /// Default vertical button padding.
    pub button_padding_y: f32,
    /// Default horizontal input padding.
    pub input_padding_x: f32,
    /// Default vertical input padding.
    pub input_padding_y: f32,
    /// Default input width.
    pub input_width: f32,
    /// Default input height.
    pub input_height: f32,
    /// Uniform control border width, included in layout.
    pub border_width: f32,
    /// Control corner radius; painting only.
    pub radius: f32,
    /// Focus stroke width, inside the box; painting only.
    pub focus_width: f32,
}
impl ThemeMetrics {
    /// Default desktop density: 14pt text and 36-unit controls.
    pub fn balanced() -> Self {
        Self {
            font_size: 14.,
            button_padding_x: 14.,
            button_padding_y: 7.2,
            input_padding_x: 12.,
            input_padding_y: 7.2,
            input_width: 240.,
            input_height: 36.,
            border_width: 1.,
            radius: 6.,
            focus_width: 2.,
        }
    }
    /// Dense preset for tool-heavy windows: 13pt text and 28-unit controls.
    pub fn compact() -> Self {
        Self {
            font_size: 13.,
            button_padding_x: 10.,
            button_padding_y: 3.9,
            input_padding_x: 8.,
            input_padding_y: 3.9,
            input_width: 200.,
            input_height: 28.,
            border_width: 1.,
            radius: 4.,
            focus_width: 2.,
        }
    }
}
impl Default for ThemeMetrics {
    fn default() -> Self {
        Self::balanced()
    }
}
#[derive(Clone, Debug, PartialEq)]
struct Data {
    colors: ThemeColors,
    metrics: ThemeMetrics,
}
/// Immutable, cheaply cloned theme shared by placements and subtree scopes.
/// Customization creates a new value; existing clones are unaffected. Dark is the
/// default. Presets use balanced metrics; [`Theme::compact`] switches density.
///
/// ```
/// use rxui::{Theme, rgb8};
/// let theme = Theme::dark().colors(|colors| colors.focus = rgb8(180, 215, 255));
/// assert_ne!(theme, Theme::dark());
/// let dense = Theme::light().compact();
/// assert_eq!(dense.sizes().input_height, 28.);
/// ```
#[derive(Clone, Debug)]
pub struct Theme(Arc<Data>);
impl PartialEq for Theme {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}
impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
#[derive(Clone, Copy)]
enum Preset {
    Light,
    Dark,
    HighContrast,
}
impl Theme {
    /// Neutral dark preset with a blue accent. Shares an immutable preset allocation.
    pub fn dark() -> Self {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME.get_or_init(|| Self::preset(Preset::Dark)).clone()
    }
    /// Neutral light preset using the same metrics and semantic color bindings.
    pub fn light() -> Self {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME.get_or_init(|| Self::preset(Preset::Light)).clone()
    }
    /// Black and white preset with a yellow accent/focus and a 3-unit focus stroke.
    pub fn high_contrast() -> Self {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME
            .get_or_init(|| Self::preset(Preset::HighContrast))
            .clone()
    }
    /// Switches to [`ThemeMetrics::compact`], keeping this theme's focus width.
    pub fn compact(self) -> Self {
        self.metrics(|m| {
            *m = ThemeMetrics {
                focus_width: m.focus_width,
                ..ThemeMetrics::compact()
            }
        })
    }
    fn preset(preset: Preset) -> Self {
        let hex = |v: u32| rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8);
        let mut metrics = ThemeMetrics::default();
        let colors = match preset {
            Preset::Light => ThemeColors {
                background: hex(0xF3F4F6),
                surface: hex(0xFFFFFF),
                raised: hex(0xFFFFFF),
                control: hex(0xFFFFFF),
                control_hover: hex(0xF1F2F4),
                control_pressed: hex(0xE4E6EA),
                control_disabled: hex(0xF3F4F6),
                text: hex(0x17181C),
                text_muted: hex(0x5B606A),
                text_disabled: hex(0xA0A4AB),
                border: hex(0xC3C7CE),
                border_disabled: hex(0xDDE0E4),
                input: hex(0xFFFFFF),
                input_border: hex(0x8F949D),
                divider: hex(0xE3E5E9),
                accent: hex(0x2563EB),
                accent_hover: hex(0x1D4FD8),
                accent_pressed: hex(0x1E44B8),
                accent_text: hex(0xFFFFFF),
                focus: hex(0x2563EB),
                selection: hex(0xCFE0FF),
                selection_text: hex(0x17181C),
                caret: hex(0x17181C),
                preedit: hex(0x17181C),
            },
            Preset::Dark => ThemeColors {
                background: hex(0x131418),
                surface: hex(0x1B1C21),
                raised: hex(0x23252B),
                control: hex(0x25272D),
                control_hover: hex(0x2E3037),
                control_pressed: hex(0x383A42),
                control_disabled: hex(0x1E1F24),
                text: hex(0xECEDF0),
                text_muted: hex(0xA3A7B0),
                text_disabled: hex(0x60646C),
                border: hex(0x3E414A),
                border_disabled: hex(0x2C2E34),
                input: hex(0x16171B),
                input_border: hex(0x6A6E78),
                divider: hex(0x2A2C32),
                accent: hex(0x2563EB),
                accent_hover: hex(0x1F5AE0),
                accent_pressed: hex(0x1A4CC2),
                accent_text: hex(0xFFFFFF),
                focus: hex(0x6EA2FF),
                selection: hex(0x2B4E8F),
                selection_text: hex(0xFFFFFF),
                caret: hex(0xECEDF0),
                preedit: hex(0xECEDF0),
            },
            Preset::HighContrast => {
                metrics.focus_width = 3.;
                ThemeColors {
                    background: hex(0x000000),
                    surface: hex(0x000000),
                    raised: hex(0x000000),
                    control: hex(0x000000),
                    control_hover: hex(0x1F1F1F),
                    control_pressed: hex(0x3A3A3A),
                    control_disabled: hex(0x000000),
                    text: hex(0xFFFFFF),
                    text_muted: hex(0xE6E6E6),
                    text_disabled: hex(0x9A9A9A),
                    border: hex(0xFFFFFF),
                    border_disabled: hex(0x9A9A9A),
                    input: hex(0x000000),
                    input_border: hex(0xFFFFFF),
                    divider: hex(0xFFFFFF),
                    accent: hex(0xFFD400),
                    accent_hover: hex(0xFFE34D),
                    accent_pressed: hex(0xE6BF00),
                    accent_text: hex(0x000000),
                    focus: hex(0xFFD400),
                    selection: hex(0x00E5FF),
                    selection_text: hex(0x000000),
                    caret: hex(0xFFFFFF),
                    preedit: hex(0xFFFFFF),
                }
            }
        };
        Self(Arc::new(Data { colors, metrics }))
    }
    /// Customizes a cloned palette; no placement or existing clone is mutated.
    pub fn colors(mut self, f: impl FnOnce(&mut ThemeColors)) -> Self {
        f(&mut Arc::make_mut(&mut self.0).colors);
        self
    }
    /// Customizes default typography/control metrics, preserving existing clones.
    pub fn metrics(mut self, f: impl FnOnce(&mut ThemeMetrics)) -> Self {
        f(&mut Arc::make_mut(&mut self.0).metrics);
        self
    }
    /// Current palette in linear RGBA.
    pub fn palette(&self) -> &ThemeColors {
        &self.0.colors
    }
    /// Current logical metrics.
    pub fn sizes(&self) -> &ThemeMetrics {
        &self.0.metrics
    }
    /// Resolves one semantic color.
    pub fn color(&self, color: ThemeColor) -> Color {
        let c = self.palette();
        match color {
            ThemeColor::Background => c.background,
            ThemeColor::Surface => c.surface,
            ThemeColor::Raised => c.raised,
            ThemeColor::Control => c.control,
            ThemeColor::ControlHover => c.control_hover,
            ThemeColor::ControlPressed => c.control_pressed,
            ThemeColor::ControlDisabled => c.control_disabled,
            ThemeColor::Text => c.text,
            ThemeColor::TextMuted => c.text_muted,
            ThemeColor::TextDisabled => c.text_disabled,
            ThemeColor::Border => c.border,
            ThemeColor::BorderDisabled => c.border_disabled,
            ThemeColor::Input => c.input,
            ThemeColor::InputBorder => c.input_border,
            ThemeColor::Divider => c.divider,
            ThemeColor::Accent => c.accent,
            ThemeColor::AccentHover => c.accent_hover,
            ThemeColor::AccentPressed => c.accent_pressed,
            ThemeColor::AccentText => c.accent_text,
            ThemeColor::Focus => c.focus,
            ThemeColor::Selection => c.selection,
            ThemeColor::SelectionText => c.selection_text,
            ThemeColor::Caret => c.caret,
            ThemeColor::Preedit => c.preedit,
        }
    }
    /// Checks finite/ranged colors and nonnegative metrics. Installation is fallible
    /// so invalid customization cannot replace a placement's working theme.
    pub fn validate(&self) -> Result<(), UiError> {
        let m = self.sizes();
        if !self.palette().values().into_iter().all(valid_color)
            || !m.font_size.is_finite()
            || m.font_size <= 0.
            || [
                m.button_padding_x,
                m.button_padding_y,
                m.input_padding_x,
                m.input_padding_y,
                m.input_width,
                m.input_height,
                m.border_width,
                m.radius,
                m.focus_width,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(UiError::InvalidStyle);
        }
        Ok(())
    }
}

/// Paint-only overrides. Omitted fields preserve defaults; no_background explicitly
/// removes a fill. State overrides cannot change layout or typography.
#[derive(Clone, Debug, Default, PartialEq)]
#[must_use]
pub struct PaintStyle {
    pub(crate) color: Option<StyleColor>,
    pub(crate) background: Option<Option<StyleColor>>,
    pub(crate) border_color: Option<Option<StyleColor>>,
    pub(crate) radii: Option<[f32; 4]>,
    pub(crate) focus_color: Option<StyleColor>,
    pub(crate) focus_width: Option<f32>,
    pub(crate) selection: Option<StyleColor>,
    pub(crate) selection_text: Option<StyleColor>,
    pub(crate) caret: Option<StyleColor>,
    pub(crate) preedit: Option<StyleColor>,
    pub(crate) shadow: Option<Option<BoxShadow>>,
}
/// Blurred shadow cast by an element's rounded box, painted behind its background.
/// It follows the element's corner radii and is clipped like the element itself;
/// it does not affect layout or hit testing.
///
/// ```
/// use rxui::prelude::*;
/// let card = column().padding(16.).radius(8.).background(ThemeColor::Raised)
///     .shadow(BoxShadow::new(rgba8(0, 0, 0, 96)).offset(0., 4.).blur(12.));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    /// Displacement from the element in logical units.
    pub offset: [f32; 2],
    /// Nonnegative blur radius in logical units; the Gaussian deviation is half of it.
    pub blur: f32,
    /// Outset of the shadow shape before blurring; negative values shrink it.
    pub spread: f32,
    /// Shadow color, literal or theme-bound.
    pub color: StyleColor,
}
impl BoxShadow {
    /// A sharp shadow directly beneath the element.
    pub fn new(color: impl Into<StyleColor>) -> Self {
        Self {
            offset: [0.; 2],
            blur: 0.,
            spread: 0.,
            color: color.into(),
        }
    }
    /// Selects the displacement.
    pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.offset = [x, y];
        self
    }
    /// Selects the blur radius.
    pub fn blur(mut self, blur: f32) -> Self {
        self.blur = blur;
        self
    }
    /// Selects the spread distance.
    pub fn spread(mut self, spread: f32) -> Self {
        self.spread = spread;
        self
    }
    fn valid(&self) -> bool {
        self.offset
            .iter()
            .chain([&self.spread])
            .all(|v| v.is_finite())
            && self.blur.is_finite()
            && self.blur >= 0.
            && self.color.valid()
    }
}
/// A [`BoxShadow`] with its color resolved in the element's theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedShadow {
    /// Displacement from the element in logical units.
    pub offset: [f32; 2],
    /// Blur radius in logical units.
    pub blur: f32,
    /// Outset before blurring.
    pub spread: f32,
    /// Linear RGBA color.
    pub color: Color,
}
impl ResolvedShadow {
    /// Logical area the blurred shadow of `bounds` can cover, including its tail.
    pub fn extent(&self, bounds: crate::Bounds) -> crate::Bounds {
        // The renderer stops at three standard deviations (1.5 blur radii).
        let grow = self.spread + 1.5 * self.blur;
        crate::Bounds {
            x: bounds.x + self.offset[0] - grow,
            y: bounds.y + self.offset[1] - grow,
            width: (bounds.width + 2. * grow).max(0.),
            height: (bounds.height + 2. * grow).max(0.),
        }
    }
}
impl PaintStyle {
    /// Empty patch, inheriting every default.
    pub fn new() -> Self {
        Self::default()
    }
    /// Text color, literal or semantic.
    pub fn color(mut self, color: impl Into<StyleColor>) -> Self {
        self.color = Some(color.into());
        self
    }
    /// Background fill, literal or semantic.
    pub fn background(mut self, color: impl Into<StyleColor>) -> Self {
        self.background = Some(Some(color.into()));
        self
    }
    /// Explicitly remove the background fill.
    pub fn no_background(mut self) -> Self {
        self.background = Some(None);
        self
    }
    /// Border color; widths are configured on the element's layout.
    pub fn border_color(mut self, color: impl Into<StyleColor>) -> Self {
        self.border_color = Some(Some(color.into()));
        self
    }
    /// Hide border painting while preserving layout widths.
    pub fn no_border(mut self) -> Self {
        self.border_color = Some(None);
        self
    }
    /// Uniform corner radius. A clipping element also rounds its descendant painting.
    pub fn radius(self, radius: f32) -> Self {
        self.corner_radii(radius, radius, radius, radius)
    }
    /// Per-corner radii, clockwise from the top-left. Large radii shrink proportionally
    /// to fit the box when painted.
    pub fn corner_radii(
        mut self,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
    ) -> Self {
        self.radii = Some([top_left, top_right, bottom_right, bottom_left]);
        self
    }
    /// Focus indicator color; focus remains separate from hover/pressed state.
    pub fn focus_color(mut self, color: impl Into<StyleColor>) -> Self {
        self.focus_color = Some(color.into());
        self
    }
    /// Inside focus stroke width, independent of box metrics.
    pub fn focus_width(mut self, width: f32) -> Self {
        self.focus_width = Some(width);
        self
    }
    /// Opaque/readable selection fill drawn behind ordinary text.
    pub fn selection_color(mut self, color: impl Into<StyleColor>) -> Self {
        self.selection = Some(color.into());
        self
    }
    /// Text color over the selection fill. Reuses the prepared glyph geometry.
    pub fn selection_text_color(mut self, color: impl Into<StyleColor>) -> Self {
        self.selection_text = Some(color.into());
        self
    }
    /// Input caret color.
    pub fn caret_color(mut self, color: impl Into<StyleColor>) -> Self {
        self.caret = Some(color.into());
        self
    }
    /// IME composition underline color.
    pub fn preedit_color(mut self, color: impl Into<StyleColor>) -> Self {
        self.preedit = Some(color.into());
        self
    }
    /// Box shadow behind the element's background.
    pub fn shadow(mut self, shadow: BoxShadow) -> Self {
        self.shadow = Some(Some(shadow));
        self
    }
    /// Explicitly remove an earlier shadow, for example in a state patch.
    pub fn no_shadow(mut self) -> Self {
        self.shadow = Some(None);
        self
    }
    pub(crate) fn merge(&mut self, patch: Self) {
        macro_rules! merge { ($($field:ident),*) => { $(if patch.$field.is_some() { self.$field=patch.$field; })* }; }
        merge!(
            color,
            background,
            border_color,
            radii,
            focus_color,
            focus_width,
            selection,
            selection_text,
            caret,
            preedit,
            shadow
        );
    }
    pub(crate) fn validate(&self) -> Result<(), UiError> {
        if [
            self.color,
            self.background.flatten(),
            self.border_color.flatten(),
            self.focus_color,
            self.selection,
            self.selection_text,
            self.caret,
            self.preedit,
        ]
        .into_iter()
        .flatten()
        .any(|c| !c.valid())
            || self.shadow.flatten().is_some_and(|s| !s.valid())
            || self
                .radii
                .into_iter()
                .flatten()
                .chain(self.focus_width)
                .any(|v| !v.is_finite() || v < 0.)
        {
            return Err(UiError::InvalidStyle);
        }
        Ok(())
    }
    pub(crate) fn apply(&self, theme: &Theme, paint: &mut ResolvedPaint) {
        if let Some(c) = self.color {
            paint.color = c.resolve(theme);
        }
        if let Some(c) = self.background {
            paint.background = c.map(|c| c.resolve(theme));
        }
        if let Some(c) = self.border_color {
            paint.border_color = c.map(|c| c.resolve(theme));
        }
        if let Some(v) = self.radii {
            paint.radii = v;
        }
        if let Some(c) = self.focus_color {
            paint.focus_color = c.resolve(theme);
        }
        if let Some(v) = self.focus_width {
            paint.focus_width = v;
        }
        if let Some(c) = self.selection {
            paint.selection_color = c.resolve(theme);
        }
        if let Some(c) = self.selection_text {
            paint.selection_text_color = c.resolve(theme);
        }
        if let Some(c) = self.caret {
            paint.caret_color = c.resolve(theme);
        }
        if let Some(c) = self.preedit {
            paint.preedit_color = c.resolve(theme);
        }
        if let Some(shadow) = self.shadow {
            paint.shadow = shadow.map(|s| ResolvedShadow {
                offset: s.offset,
                blur: s.blur,
                spread: s.spread,
                color: s.color.resolve(theme),
            });
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StateStyles {
    pub hover: PaintStyle,
    pub pressed: PaintStyle,
    pub disabled: PaintStyle,
}
/// Fully resolved paint values for one element's current interaction state. No token
/// lookup, ancestor traversal or theme allocation is required by the painter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedPaint {
    /// Text color.
    pub color: Color,
    /// Optional box fill.
    pub background: Option<Color>,
    /// Optional border color; resolved widths are exposed by ElementInfo::border.
    pub border_color: Option<Color>,
    /// Corner radii, clockwise from the top-left. A clipping element also rounds its
    /// descendant painting by these radii, inset by its border and padding.
    pub radii: [f32; 4],
    /// Focus outline color.
    pub focus_color: Color,
    /// Inside focus stroke width.
    pub focus_width: f32,
    /// Selection fill, drawn behind text.
    pub selection_color: Color,
    /// Text over the selection fill.
    pub selection_text_color: Color,
    /// Input caret color.
    pub caret_color: Color,
    /// IME underline color.
    pub preedit_color: Color,
    /// Optional box shadow behind the background.
    pub shadow: Option<ResolvedShadow>,
}
impl ResolvedPaint {
    pub(crate) fn new(theme: &Theme, color: Color) -> Self {
        let c = theme.palette();
        Self {
            color,
            background: None,
            border_color: None,
            radii: [0.; 4],
            focus_color: c.focus,
            focus_width: theme.sizes().focus_width,
            selection_color: c.selection,
            selection_text_color: c.selection_text,
            caret_color: c.caret,
            preedit_color: c.preedit,
            shadow: None,
        }
    }
}
