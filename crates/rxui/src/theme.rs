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
    fn values(&self) -> [Color; 16] {
        [
            self.background,
            self.surface,
            self.control,
            self.control_hover,
            self.control_pressed,
            self.control_disabled,
            self.text,
            self.text_muted,
            self.text_disabled,
            self.border,
            self.border_disabled,
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
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeMetrics {
    /// Root/inherited text size; strictly positive.
    pub font_size: f32,
    /// Default uniform button padding.
    pub button_padding: f32,
    /// Default input padding.
    pub input_padding: f32,
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
impl Default for ThemeMetrics {
    fn default() -> Self {
        Self {
            font_size: 16.,
            button_padding: 12.,
            input_padding: 10.,
            input_width: 240.,
            input_height: 44.,
            border_width: 1.,
            radius: 4.,
            focus_width: 2.,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
struct Data {
    colors: ThemeColors,
    metrics: ThemeMetrics,
}
/// Immutable, cheaply cloned theme shared by placements and subtree scopes.
/// Customization creates a new value; existing clones are unaffected. Dark is the
/// default. Both presets use grayscale chrome and opaque selection colors.
///
/// ```
/// use rxui::{Theme, rgb8};
/// let theme = Theme::dark().colors(|colors| colors.focus = rgb8(180, 215, 255));
/// assert_ne!(theme, Theme::dark());
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
impl Theme {
    /// High-contrast neutral dark preset. Shares an immutable preset allocation.
    pub fn dark() -> Self {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME.get_or_init(|| Self::preset(false)).clone()
    }
    /// Neutral light preset using the same metrics and semantic color bindings.
    pub fn light() -> Self {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME.get_or_init(|| Self::preset(true)).clone()
    }
    fn preset(light: bool) -> Self {
        let gray = |v| rgb8(v, v, v);
        let colors = if light {
            ThemeColors {
                background: gray(250),
                surface: gray(255),
                control: gray(242),
                control_hover: gray(230),
                control_pressed: gray(214),
                control_disabled: gray(246),
                text: gray(24),
                text_muted: gray(76),
                text_disabled: gray(105),
                border: gray(110),
                border_disabled: gray(165),
                focus: gray(24),
                selection: gray(76),
                selection_text: gray(250),
                caret: gray(24),
                preedit: gray(24),
            }
        } else {
            ThemeColors {
                background: gray(18),
                surface: gray(27),
                control: gray(38),
                control_hover: gray(48),
                control_pressed: gray(58),
                control_disabled: gray(30),
                text: gray(245),
                text_muted: gray(179),
                text_disabled: gray(145),
                border: gray(133),
                border_disabled: gray(90),
                focus: gray(245),
                selection: gray(179),
                selection_text: gray(18),
                caret: gray(245),
                preedit: gray(245),
            }
        };
        Self(Arc::new(Data {
            colors,
            metrics: ThemeMetrics::default(),
        }))
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
            ThemeColor::Control => c.control,
            ThemeColor::ControlHover => c.control_hover,
            ThemeColor::ControlPressed => c.control_pressed,
            ThemeColor::ControlDisabled => c.control_disabled,
            ThemeColor::Text => c.text,
            ThemeColor::TextMuted => c.text_muted,
            ThemeColor::TextDisabled => c.text_disabled,
            ThemeColor::Border => c.border,
            ThemeColor::BorderDisabled => c.border_disabled,
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
                m.button_padding,
                m.input_padding,
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
    pub(crate) radius: Option<f32>,
    pub(crate) focus_color: Option<StyleColor>,
    pub(crate) focus_width: Option<f32>,
    pub(crate) selection: Option<StyleColor>,
    pub(crate) selection_text: Option<StyleColor>,
    pub(crate) caret: Option<StyleColor>,
    pub(crate) preedit: Option<StyleColor>,
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
    /// Uniform corner radius; does not clip descendants or change hit testing.
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
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
    pub(crate) fn merge(&mut self, patch: Self) {
        macro_rules! merge { ($($field:ident),*) => { $(if patch.$field.is_some() { self.$field=patch.$field; })* }; }
        merge!(
            color,
            background,
            border_color,
            radius,
            focus_color,
            focus_width,
            selection,
            selection_text,
            caret,
            preedit
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
            || [self.radius, self.focus_width]
                .into_iter()
                .flatten()
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
        if let Some(v) = self.radius {
            paint.radius = v;
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
    /// Uniform corner radius. Does not imply rounded descendant clipping.
    pub radius: f32,
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
}
impl ResolvedPaint {
    pub(crate) fn new(theme: &Theme, color: Color) -> Self {
        let c = theme.palette();
        Self {
            color,
            background: None,
            border_color: None,
            radius: 0.,
            focus_color: c.focus,
            focus_width: theme.sizes().focus_width,
            selection_color: c.selection,
            selection_text_color: c.selection_text,
            caret_color: c.caret,
            preedit_color: c.preedit,
        }
    }
}
