use crate::{Color, UiError};
use std::sync::{Arc, OnceLock};

/// Converts sRGB byte channels to the linear RGBA used by RXUI drawing. Alpha is opaque.
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
    /// Strong control boundary.
    Border,
    /// Disabled boundary.
    BorderDisabled,
    /// Text input fill.
    Input,
    /// Text input boundary.
    InputBorder,
    /// Subtle separator between regions, weaker than Border.
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
    /// Hover fill of list, tree and menu rows; may be translucent.
    RowHover,
    /// Selected row fill while its list has focus. Row text stays Text.
    RowSelected,
    /// Selected row fill while its list is unfocused.
    RowSelectedInactive,
    /// Error foreground, also used for borders and icons.
    Danger,
    /// Warning foreground, also used for borders and icons.
    Warning,
    /// Success foreground, also used for borders and icons.
    Success,
    /// Tinted, possibly translucent, accent background for accent badges and
    /// banners.
    AccentSubtle,
    /// Tinted, possibly translucent, danger background for badges, banners and the
    /// hovered danger button.
    DangerSubtle,
    /// Tinted, possibly translucent, warning background.
    WarningSubtle,
    /// Tinted, possibly translucent, success background.
    SuccessSubtle,
    /// Application-defined color from [`Theme::define_color`]. A name the theme
    /// does not define resolves to transparent.
    Custom(&'static str),
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
    /// Strong control boundary.
    pub border: Color,
    /// Disabled boundary.
    pub border_disabled: Color,
    /// Text input fill.
    pub input: Color,
    /// Text input boundary.
    pub input_border: Color,
    /// Subtle separator between regions, weaker than border.
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
    /// Row hover fill; may be translucent.
    pub row_hover: Color,
    /// Selected row fill while its list has focus.
    pub row_selected: Color,
    /// Selected row fill while its list is unfocused.
    pub row_selected_inactive: Color,
    /// Error foreground.
    pub danger: Color,
    /// Warning foreground.
    pub warning: Color,
    /// Success foreground.
    pub success: Color,
    /// Tinted accent background; may be translucent.
    pub accent_subtle: Color,
    /// Tinted danger background; may be translucent.
    pub danger_subtle: Color,
    /// Tinted warning background; may be translucent.
    pub warning_subtle: Color,
    /// Tinted success background; may be translucent.
    pub success_subtle: Color,
}
impl ThemeColors {
    fn values(&self) -> [Color; 34] {
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
            self.row_hover,
            self.row_selected,
            self.row_selected_inactive,
            self.danger,
            self.warning,
            self.success,
            self.accent_subtle,
            self.danger_subtle,
            self.warning_subtle,
            self.success_subtle,
        ]
    }
}
/// Default typography/control metrics in logical units. Explicit element builders
/// take precedence. Changing font size or box metrics invalidates affected layout.
///
/// Text lines are `font_size * line_height` tall (1.4 in the presets), so a button is
/// `font_size * line_height + 2 * (button_padding_y + border_width)` tall. The presets
/// pick paddings that make buttons exactly `input_height`, so they line up in a row.
/// List rows are denser: [`crate::Element::list_row`] gives rows at least
/// `row_height` (24 balanced, 22 compact) and `row_padding_x` of horizontal padding.
///
/// The type scale sizes [`crate::TextRole`]s: `caption_size` for secondary small
/// text, `font_size` for body text, and `subheading_size`, `heading_size` and
/// `title_size` (drawn at `heading_weight`) for headings. The radius scale pairs
/// `radius` for controls with `radius_small` for small chips and indicators and
/// `radius_large` for popovers and dialogs.
///
/// `indicator_size`, `switch_width` and `switch_height` size the standard check
/// boxes, radios, switches, sliders and progress bars, so [`ThemeMetrics::compact`]
/// shrinks them with the text. They stay below the text line, so a captioned
/// check box or switch is as tall as a button. `icon_size` sizes
/// [`crate::icon`]s (16 balanced, 14 compact).
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeMetrics {
    /// Root/inherited body text size; strictly positive.
    pub font_size: f32,
    /// Caption text size; strictly positive.
    pub caption_size: f32,
    /// Smallest heading size; strictly positive.
    pub subheading_size: f32,
    /// Section heading size; strictly positive.
    pub heading_size: f32,
    /// Page or dialog title size; strictly positive.
    pub title_size: f32,
    /// Weight of the heading roles, in `1..=1000`.
    pub heading_weight: crate::FontWeight,
    /// Default line height as a multiple of the font size; strictly positive.
    pub line_height: f32,
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
    /// Minimum height of an [`crate::Element::list_row`] without an explicit
    /// height, tighter than `input_height` as desktop lists are.
    pub row_height: f32,
    /// Default horizontal padding of a list row; rows have no vertical padding.
    pub row_padding_x: f32,
    /// Uniform control border width, included in layout.
    pub border_width: f32,
    /// Control corner radius, the middle of the radius scale; painting only.
    pub radius: f32,
    /// Small corner radius for chips, badges and indicators; painting only.
    pub radius_small: f32,
    /// Large corner radius for popovers, menus and dialogs; painting only.
    pub radius_large: f32,
    /// Focus ring width; painting only.
    pub focus_width: f32,
    /// Gap between the border box and the focus ring, which surrounds the box
    /// outside it. A negative value of `-focus_width` draws the ring inside the
    /// box instead, as list rows do; painting only.
    pub focus_offset: f32,
    /// Side of check boxes and radio buttons; their marks scale with it.
    pub indicator_size: f32,
    /// Width of a switch track.
    pub switch_width: f32,
    /// Height of a switch track. Its knob, the slider thumb, the slider rail and
    /// progress bars scale with it.
    pub switch_height: f32,
    /// Side of an [`crate::icon`]; icon buttons stay `input_height` square.
    pub icon_size: f32,
    /// Durations of implicit paint transitions and overlay motion.
    pub motion: crate::ThemeMotion,
}
impl ThemeMetrics {
    /// Default desktop-tool density: 13pt text and 28-unit controls.
    pub fn balanced() -> Self {
        Self {
            font_size: 13.,
            caption_size: 11.,
            subheading_size: 15.,
            heading_size: 18.,
            title_size: 24.,
            heading_weight: crate::FontWeight::SEMIBOLD,
            line_height: 1.4,
            button_padding_x: 12.,
            button_padding_y: 3.9,
            input_padding_x: 10.,
            input_padding_y: 3.9,
            input_width: 220.,
            input_height: 28.,
            row_height: 24.,
            row_padding_x: 8.,
            border_width: 1.,
            radius: 6.,
            radius_small: 4.,
            radius_large: 8.,
            focus_width: 2.,
            focus_offset: 1.,
            indicator_size: 14.,
            switch_width: 26.,
            switch_height: 16.,
            icon_size: 16.,
            motion: Default::default(),
        }
    }
    /// Dense preset for tool-heavy windows: 12pt text and 24-unit controls.
    pub fn compact() -> Self {
        Self {
            font_size: 12.,
            caption_size: 11.,
            subheading_size: 14.,
            heading_size: 16.,
            title_size: 20.,
            heading_weight: crate::FontWeight::SEMIBOLD,
            line_height: 1.4,
            button_padding_x: 8.,
            button_padding_y: 2.6,
            input_padding_x: 8.,
            input_padding_y: 2.6,
            input_width: 200.,
            input_height: 24.,
            row_height: 22.,
            row_padding_x: 6.,
            border_width: 1.,
            radius: 5.,
            radius_small: 3.,
            radius_large: 8.,
            focus_width: 2.,
            focus_offset: 1.,
            indicator_size: 12.,
            switch_width: 22.,
            switch_height: 13.,
            icon_size: 14.,
            motion: Default::default(),
        }
    }
    /// Font size of a text role on this scale.
    pub fn role_size(&self, role: crate::TextRole) -> f32 {
        match role {
            crate::TextRole::Caption => self.caption_size,
            crate::TextRole::Body => self.font_size,
            crate::TextRole::Subheading => self.subheading_size,
            crate::TextRole::Heading => self.heading_size,
            crate::TextRole::Title => self.title_size,
        }
    }
    /// Font weight of a text role: `heading_weight` for headings, normal otherwise.
    pub fn role_weight(&self, role: crate::TextRole) -> crate::FontWeight {
        match role {
            crate::TextRole::Caption | crate::TextRole::Body => crate::FontWeight::NORMAL,
            crate::TextRole::Subheading | crate::TextRole::Heading | crate::TextRole::Title => {
                self.heading_weight
            }
        }
    }
}
impl Default for ThemeMetrics {
    fn default() -> Self {
        Self::balanced()
    }
}
/// Theme-defined shadow level. Elements bound to a level with
/// [`crate::Element::elevation`] resolve it in their nearest theme and follow theme
/// switches, as [`ThemeColor`] tokens do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Elevation {
    /// Resting controls such as buttons; subtle or absent.
    Control,
    /// Menus, popovers, tooltips and dropdowns.
    Popover,
    /// Modal dialogs, deeper than popovers.
    Modal,
}
/// Shadow of each [`Elevation`]. `None` paints no shadow at that level, as in the
/// dark control level and every high-contrast level. Mutate with [`Theme::shadows`].
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeShadows {
    /// Shadow under resting controls.
    pub control: Option<BoxShadow>,
    /// Shadow under popovers and menus.
    pub popover: Option<BoxShadow>,
    /// Shadow under modal dialogs.
    pub modal: Option<BoxShadow>,
}
impl ThemeShadows {
    /// No shadow at any level; themes that rely on borders, such as high contrast.
    pub fn none() -> Self {
        Self {
            control: None,
            popover: None,
            modal: None,
        }
    }
    /// The light preset's shadows: soft, cool-tinted and translucent, matching
    /// CSS `0 1px 2px`, `0 6px 20px` and `0 16px 40px` in `rgba(16, 24, 40, a)`
    /// at 6%, 18% and 22% opacity.
    ///
    /// Blur radii follow CSS, but RXUI blends in linear light, where a dark
    /// translucent shadow over a light surface looks about half as strong as the
    /// same alpha blended in sRGB by browsers. Each alpha is therefore raised to
    /// the linear alpha that darkens a white surface as much as the CSS one does.
    pub fn light() -> Self {
        let shadow = |y, blur, css_alpha: f32| {
            let mut color = rgb8(16, 24, 40);
            // The sRGB green channel CSS shows over white, converted to linear;
            // green dominates luminance.
            let shown = 1. - css_alpha * (1. - 24. / 255.);
            let shown = ((shown + 0.055) / 1.055).powf(2.4);
            color[3] = (1. - shown) / (1. - color[1]);
            Some(BoxShadow::new(color).offset(0., y).blur(blur))
        };
        Self {
            control: shadow(1., 2., 0.06),
            popover: shadow(6., 20., 0.18),
            modal: shadow(16., 40., 0.22),
        }
    }
    /// The dark preset's shadows: no control shadow and dense black overlays.
    pub fn dark() -> Self {
        let shadow = |y, blur, alpha| {
            Some(
                BoxShadow::new(rgba8(0, 0, 0, alpha))
                    .offset(0., y)
                    .blur(blur),
            )
        };
        Self {
            control: None,
            popover: shadow(8., 24., 128),
            modal: shadow(16., 48., 153),
        }
    }
    /// Shadow of one level.
    pub fn get(&self, level: Elevation) -> Option<BoxShadow> {
        match level {
            Elevation::Control => self.control,
            Elevation::Popover => self.popover,
            Elevation::Modal => self.modal,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
struct Data {
    colors: ThemeColors,
    metrics: ThemeMetrics,
    shadows: ThemeShadows,
    custom_colors: Vec<(&'static str, Color)>,
    custom_metrics: Vec<(&'static str, f32)>,
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
/// assert_eq!(dense.sizes().input_height, 24.);
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
    /// Switches to [`ThemeMetrics::compact`], keeping this theme's focus ring and
    /// motion.
    pub fn compact(self) -> Self {
        self.metrics(|m| {
            *m = ThemeMetrics {
                focus_width: m.focus_width,
                focus_offset: m.focus_offset,
                motion: m.motion.clone(),
                ..ThemeMetrics::compact()
            }
        })
    }
    fn preset(preset: Preset) -> Self {
        let hex = |v: u32| rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8);
        // A status color at `alpha` (linear, out of 255) for subtle tinted fills.
        let tint = |v: u32, alpha: u8| rgba8((v >> 16) as u8, (v >> 8) as u8, v as u8, alpha);
        let mut metrics = ThemeMetrics::default();
        let (colors, shadows) = match preset {
            Preset::Light => (
                ThemeColors {
                    background: hex(0xF6F6F7),
                    surface: hex(0xFFFFFF),
                    raised: hex(0xFFFFFF),
                    control: hex(0xFFFFFF),
                    control_hover: hex(0xF3F3F5),
                    control_pressed: hex(0xEAEBEE),
                    control_disabled: hex(0xF6F6F7),
                    text: hex(0x1A1B1E),
                    text_muted: hex(0x646770),
                    text_disabled: hex(0xA4A6AD),
                    border: hex(0xD3D5DA),
                    border_disabled: hex(0xE4E5E8),
                    input: hex(0xFFFFFF),
                    input_border: hex(0xC9CBD1),
                    divider: hex(0xE4E5E8),
                    accent: hex(0x2F6BEB),
                    accent_hover: hex(0x245FDD),
                    accent_pressed: hex(0x1F52C2),
                    accent_text: hex(0xFFFFFF),
                    focus: hex(0x2F6BEB),
                    selection: hex(0xCFDDFB),
                    selection_text: hex(0x1A1B1E),
                    caret: hex(0x1A1B1E),
                    preedit: hex(0x1A1B1E),
                    row_hover: rgba8(0, 0, 0, 10),
                    row_selected: hex(0xE3ECFF),
                    row_selected_inactive: hex(0xEDEEF0),
                    danger: hex(0xD93036),
                    warning: hex(0xC27C0E),
                    success: hex(0x218358),
                    accent_subtle: tint(0x2F6BEB, 31),
                    danger_subtle: tint(0xD93036, 31),
                    warning_subtle: tint(0xC27C0E, 31),
                    success_subtle: tint(0x218358, 31),
                },
                ThemeShadows::light(),
            ),
            Preset::Dark => (
                ThemeColors {
                    background: hex(0x111214),
                    surface: hex(0x17181B),
                    raised: hex(0x1E1F23),
                    control: hex(0x24262A),
                    control_hover: hex(0x2C2E33),
                    control_pressed: hex(0x33353B),
                    control_disabled: hex(0x1B1C1F),
                    text: hex(0xE6E7EA),
                    text_muted: hex(0x9A9DA5),
                    text_disabled: hex(0x5C5F66),
                    border: hex(0x34363C),
                    border_disabled: hex(0x26282C),
                    input: hex(0x0D0E10),
                    input_border: hex(0x34363C),
                    divider: hex(0x26282C),
                    accent: hex(0x3A6FE5),
                    // Darker than the accent so white text keeps 4.5:1 on hover.
                    accent_hover: hex(0x2F63DB),
                    accent_pressed: hex(0x335FC8),
                    accent_text: hex(0xFFFFFF),
                    focus: hex(0x4A7CEF),
                    selection: hex(0x2B4580),
                    selection_text: hex(0xFFFFFF),
                    caret: hex(0xE6E7EA),
                    preedit: hex(0xE6E7EA),
                    row_hover: rgba8(255, 255, 255, 12),
                    row_selected: hex(0x22314F),
                    row_selected_inactive: hex(0x26282D),
                    danger: hex(0xE5484D),
                    warning: hex(0xF5A524),
                    success: hex(0x30A46C),
                    accent_subtle: tint(0x3A6FE5, 41),
                    danger_subtle: tint(0xE5484D, 41),
                    warning_subtle: tint(0xF5A524, 41),
                    success_subtle: tint(0x30A46C, 41),
                },
                ThemeShadows::dark(),
            ),
            Preset::HighContrast => {
                metrics.focus_width = 3.;
                (
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
                        // Opaque fills: a saturated blue selection keeps white row
                        // text readable while standing out clearly from black.
                        row_hover: hex(0x1F1F1F),
                        row_selected: hex(0x0050FF),
                        row_selected_inactive: hex(0x3A3A3A),
                        danger: hex(0xFF4040),
                        warning: hex(0xFF9900),
                        success: hex(0x00E060),
                        accent_subtle: hex(0x403500),
                        danger_subtle: hex(0x400000),
                        warning_subtle: hex(0x402600),
                        success_subtle: hex(0x003818),
                    },
                    ThemeShadows::none(),
                )
            }
        };
        Self::from_parts(colors, metrics, shadows)
    }
    pub(crate) fn from_parts(
        colors: ThemeColors,
        metrics: ThemeMetrics,
        shadows: ThemeShadows,
    ) -> Self {
        Self(Arc::new(Data {
            colors,
            metrics,
            shadows,
            custom_colors: Vec::new(),
            custom_metrics: Vec::new(),
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
    /// Customizes the elevation shadows, preserving existing clones.
    ///
    /// ```
    /// use rxui::{BoxShadow, Elevation, Theme, rgba8};
    /// let theme = Theme::dark().shadows(|s| {
    ///     s.control = Some(BoxShadow::new(rgba8(0, 0, 0, 64)).offset(0., 1.).blur(2.));
    /// });
    /// assert!(theme.shadow(Elevation::Control).is_some());
    /// assert_eq!(Theme::dark().shadow(Elevation::Control), None);
    /// ```
    pub fn shadows(mut self, f: impl FnOnce(&mut ThemeShadows)) -> Self {
        f(&mut Arc::make_mut(&mut self.0).shadows);
        self
    }
    /// Defines or replaces an application color token, used through
    /// `ThemeColor::Custom(name)` wherever a color is accepted. Define the same names
    /// in every theme the application switches between.
    ///
    /// ```
    /// use rxui::{Theme, ThemeColor, rgb8};
    /// let theme = Theme::dark()
    ///     .define_color("sidebar", rgb8(24, 26, 31))
    ///     .define_metric("gutter", 12.);
    /// assert_eq!(theme.color(ThemeColor::Custom("sidebar")), rgb8(24, 26, 31));
    /// assert_eq!(theme.custom_metric("gutter"), Some(12.));
    /// ```
    pub fn define_color(mut self, name: &'static str, color: Color) -> Self {
        let colors = &mut Arc::make_mut(&mut self.0).custom_colors;
        match colors.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = color,
            None => colors.push((name, color)),
        }
        self
    }
    /// Defines or replaces an application metric, such as a spacing or size that
    /// views read through `ViewContext::theme`.
    pub fn define_metric(mut self, name: &'static str, value: f32) -> Self {
        let metrics = &mut Arc::make_mut(&mut self.0).custom_metrics;
        match metrics.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = value,
            None => metrics.push((name, value)),
        }
        self
    }
    /// An application color defined with [`Self::define_color`].
    pub fn custom_color(&self, name: &str) -> Option<Color> {
        self.0
            .custom_colors
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, c)| *c)
    }
    /// An application metric defined with [`Self::define_metric`].
    pub fn custom_metric(&self, name: &str) -> Option<f32> {
        self.0
            .custom_metrics
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| *v)
    }
    /// Current palette in linear RGBA.
    pub fn palette(&self) -> &ThemeColors {
        &self.0.colors
    }
    /// Current logical metrics.
    pub fn sizes(&self) -> &ThemeMetrics {
        &self.0.metrics
    }
    /// Current elevation shadows.
    pub fn elevations(&self) -> &ThemeShadows {
        &self.0.shadows
    }
    /// The shadow of one elevation level, or `None` when the level is flat.
    pub fn shadow(&self, level: Elevation) -> Option<BoxShadow> {
        self.0.shadows.get(level)
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
            ThemeColor::RowHover => c.row_hover,
            ThemeColor::RowSelected => c.row_selected,
            ThemeColor::RowSelectedInactive => c.row_selected_inactive,
            ThemeColor::Danger => c.danger,
            ThemeColor::Warning => c.warning,
            ThemeColor::Success => c.success,
            ThemeColor::AccentSubtle => c.accent_subtle,
            ThemeColor::DangerSubtle => c.danger_subtle,
            ThemeColor::WarningSubtle => c.warning_subtle,
            ThemeColor::SuccessSubtle => c.success_subtle,
            ThemeColor::Custom(name) => self.custom_color(name).unwrap_or([0.; 4]),
        }
    }
    /// Checks finite/ranged colors, positive text sizes, nonnegative metrics, font
    /// weights in `1..=1000` and valid shadows. Installation is fallible so invalid
    /// customization cannot replace a placement's working theme.
    pub fn validate(&self) -> Result<(), UiError> {
        let m = self.sizes();
        let s = self.elevations();
        if !self.palette().values().into_iter().all(valid_color)
            || !self.0.custom_colors.iter().all(|(_, c)| valid_color(*c))
            || self.0.custom_metrics.iter().any(|(_, v)| !v.is_finite())
            || [
                m.font_size,
                m.caption_size,
                m.subheading_size,
                m.heading_size,
                m.title_size,
                m.line_height,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
            || !(1..=1000).contains(&m.heading_weight.0)
            || [s.control, s.popover, s.modal]
                .iter()
                .flatten()
                .any(|s| !s.valid())
            || [
                m.button_padding_x,
                m.button_padding_y,
                m.input_padding_x,
                m.input_padding_y,
                m.input_width,
                m.input_height,
                m.row_height,
                m.row_padding_x,
                m.border_width,
                m.radius,
                m.radius_small,
                m.radius_large,
                m.focus_width,
                m.indicator_size,
                m.switch_width,
                m.switch_height,
                m.icon_size,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.)
            || !m.motion.overlay_offset.is_finite()
            || !m.focus_offset.is_finite()
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
    pub(crate) focus_offset: Option<f32>,
    pub(crate) selection: Option<StyleColor>,
    pub(crate) selection_text: Option<StyleColor>,
    pub(crate) caret: Option<StyleColor>,
    pub(crate) preedit: Option<StyleColor>,
    pub(crate) shadow: Option<Option<StyleShadow>>,
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
    fn resolve(self, theme: &Theme) -> ResolvedShadow {
        ResolvedShadow {
            offset: self.offset,
            blur: self.blur,
            spread: self.spread,
            color: self.color.resolve(theme),
        }
    }
}
/// A literal [`BoxShadow`] or a theme [`Elevation`]. Elevations resolve again on a
/// theme switch, like [`StyleColor::Theme`]; a flat level paints no shadow.
///
/// ```
/// use rxui::prelude::*;
/// let menu = column().radius(8.).background(ThemeColor::Raised).elevation(Elevation::Popover);
/// let card = column().shadow(BoxShadow::new(rgba8(0, 0, 0, 64)).blur(4.));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StyleShadow {
    /// Explicit shadow; theme-bound colors still follow the theme.
    Literal(BoxShadow),
    /// Shadow of a level in the nearest theme.
    Theme(Elevation),
}
impl From<BoxShadow> for StyleShadow {
    fn from(shadow: BoxShadow) -> Self {
        Self::Literal(shadow)
    }
}
impl From<Elevation> for StyleShadow {
    fn from(level: Elevation) -> Self {
        Self::Theme(level)
    }
}
impl StyleShadow {
    pub(crate) fn resolve(self, theme: &Theme) -> Option<ResolvedShadow> {
        match self {
            Self::Literal(shadow) => Some(shadow.resolve(theme)),
            Self::Theme(level) => theme.shadow(level).map(|s| s.resolve(theme)),
        }
    }
    fn valid(&self) -> bool {
        match self {
            Self::Literal(shadow) => shadow.valid(),
            Self::Theme(_) => true,
        }
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
    /// Focus ring width, independent of box metrics.
    pub fn focus_width(mut self, width: f32) -> Self {
        self.focus_width = Some(width);
        self
    }
    /// Gap between the border box and the focus ring; see
    /// [`ThemeMetrics::focus_offset`]. `-width` places the ring inside the box.
    pub fn focus_offset(mut self, offset: f32) -> Self {
        self.focus_offset = Some(offset);
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
    /// Box shadow behind the element's background, literal or a theme elevation.
    pub fn shadow(mut self, shadow: impl Into<StyleShadow>) -> Self {
        self.shadow = Some(Some(shadow.into()));
        self
    }
    /// The theme's shadow for `level`; the same as `shadow(level)`.
    pub fn elevation(self, level: Elevation) -> Self {
        self.shadow(level)
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
            focus_offset,
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
            || self.focus_offset.is_some_and(|v| !v.is_finite())
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
        if let Some(v) = self.focus_offset {
            paint.focus_offset = v;
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
            paint.shadow = shadow.and_then(|s| s.resolve(theme));
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StateStyles {
    pub hover: PaintStyle,
    pub pressed: PaintStyle,
    pub disabled: PaintStyle,
    /// Explicit paint/overlay transition; `None` uses the element kind's default.
    pub transition: Option<crate::Transition>,
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
    /// Focus ring width.
    pub focus_width: f32,
    /// Gap between the border box and the focus ring; negative values move the
    /// ring inward. See [`ThemeMetrics::focus_offset`].
    pub focus_offset: f32,
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
    /// Logical area a focus ring around `bounds` covers, or `None` when the ring
    /// is empty. It extends beyond `bounds` unless the ring is inside the box.
    pub fn focus_extent(&self, bounds: crate::Bounds) -> Option<crate::Bounds> {
        if self.focus_width <= 0. || bounds.width <= 0. || bounds.height <= 0. {
            return None;
        }
        let grow = (self.focus_offset + self.focus_width).max(0.);
        Some(crate::Bounds {
            x: bounds.x - grow,
            y: bounds.y - grow,
            width: bounds.width + 2. * grow,
            height: bounds.height + 2. * grow,
        })
    }
    pub(crate) fn new(theme: &Theme, color: Color) -> Self {
        let c = theme.palette();
        Self {
            color,
            background: None,
            border_color: None,
            radii: [0.; 4],
            focus_color: c.focus,
            focus_width: theme.sizes().focus_width,
            focus_offset: theme.sizes().focus_offset,
            selection_color: c.selection,
            selection_text_color: c.selection_text,
            caret_color: c.caret,
            preedit_color: c.preedit,
            shadow: None,
        }
    }
}
