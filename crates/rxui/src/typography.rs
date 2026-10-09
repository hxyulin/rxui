//! Inherited font selection, line height and alignment for text leaves.
use std::sync::Arc;

/// Preferred font family. Missing glyphs fall back to other loaded faces.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontFamily {
    /// A family name, such as one loaded through `UiPainter::fonts_mut`.
    Named(Arc<str>),
    /// Generic sans-serif family, the default.
    #[default]
    SansSerif,
    /// Generic serif family.
    Serif,
    /// Generic monospace family.
    Monospace,
}
impl From<&str> for FontFamily {
    fn from(value: &str) -> Self {
        Self::Named(value.into())
    }
}
impl From<String> for FontFamily {
    fn from(value: String) -> Self {
        Self::Named(value.into())
    }
}
/// One face added by registered font data, as reported by the font itself.
/// Select it with [`FontFamily::Named`] and one of its family names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFace {
    /// Family names the face declares, without duplicate localized names.
    pub families: Vec<String>,
    /// PostScript name of the face.
    pub postscript_name: String,
    /// Weight the face reports.
    pub weight: FontWeight,
    /// Slope the face reports.
    pub style: FontStyle,
    /// Whether the face reports a fixed pitch.
    pub monospaced: bool,
}
/// OpenType/CSS font weight in `1..=1000`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontWeight(pub u16);
impl FontWeight {
    /// 100.
    pub const THIN: Self = Self(100);
    /// 300.
    pub const LIGHT: Self = Self(300);
    /// 400, the default.
    pub const NORMAL: Self = Self(400);
    /// 500.
    pub const MEDIUM: Self = Self(500);
    /// 600.
    pub const SEMIBOLD: Self = Self(600);
    /// 700.
    pub const BOLD: Self = Self(700);
    /// 900.
    pub const BLACK: Self = Self(900);
}
impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}
/// Requested slope; font matching can select a nearby available face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontStyle {
    /// Upright, the default.
    #[default]
    Normal,
    /// Italic.
    Italic,
    /// Oblique.
    Oblique,
}
/// Horizontal line alignment within a text leaf's content width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    /// Left for left-to-right paragraphs, right for right-to-left ones, the default.
    #[default]
    Start,
    /// Right for left-to-right paragraphs, left for right-to-left ones.
    End,
    /// Always left.
    Left,
    /// Always right.
    Right,
    /// Centered.
    Center,
    /// Justified, except each paragraph's last line.
    Justified,
}
/// Resolved text attributes of one element, besides its size and color.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// Preferred family.
    pub family: FontFamily,
    /// Weight.
    pub weight: FontWeight,
    /// Slope.
    pub style: FontStyle,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    /// Line alignment. Single-line text inputs always start-align.
    pub align: TextAlign,
}
impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: FontFamily::default(),
            weight: FontWeight::default(),
            style: FontStyle::default(),
            line_height: 1.4,
            align: TextAlign::default(),
        }
    }
}
/// Element-level overrides; unset fields inherit from the parent.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Overrides {
    pub family: Option<FontFamily>,
    pub weight: Option<FontWeight>,
    pub style: Option<FontStyle>,
    pub line_height: Option<f32>,
    pub align: Option<TextAlign>,
}
impl Overrides {
    /// This element's overrides layered over inherited ones.
    pub fn over(&self, inherited: &Self) -> Self {
        Self {
            family: self.family.clone().or_else(|| inherited.family.clone()),
            weight: self.weight.or(inherited.weight),
            style: self.style.or(inherited.style),
            line_height: self.line_height.or(inherited.line_height),
            align: self.align.or(inherited.align),
        }
    }
    /// Fills unset fields with defaults; line height defaults to the theme's.
    pub fn resolve(&self, theme: &crate::Theme) -> TextStyle {
        TextStyle {
            family: self.family.clone().unwrap_or_default(),
            weight: self.weight.unwrap_or_default(),
            style: self.style.unwrap_or_default(),
            line_height: self.line_height.unwrap_or(theme.sizes().line_height),
            align: self.align.unwrap_or_default(),
        }
    }
    pub fn valid(&self) -> bool {
        self.weight.is_none_or(|w| (1..=1000).contains(&w.0))
            && self.line_height.is_none_or(|v| v.is_finite() && v > 0.)
            && !matches!(&self.family, Some(FontFamily::Named(name)) if name.is_empty())
    }
}

/// One styled run of a [`crate::rich_text`] paragraph. Unset attributes inherit the
/// element's resolved text style and color; theme colors follow theme switches.
///
/// ```
/// use rxui::prelude::*;
/// let notice = rich_text([
///     span("Saved "),
///     span("3 files").bold(),
///     span(" to "),
///     span("~/notes").monospace().color(ThemeColor::Accent),
/// ]);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpan {
    pub(crate) text: String,
    pub(crate) style: SpanStyle,
}
/// Attribute overrides of one span; see [`TextSpan`].
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SpanStyle {
    pub family: Option<FontFamily>,
    pub weight: Option<FontWeight>,
    pub style: Option<FontStyle>,
    pub font_size: Option<f32>,
    pub color: Option<crate::StyleColor>,
    pub underline: bool,
    pub strikethrough: bool,
}
/// A span of text for [`crate::rich_text`], inheriting every attribute until set.
pub fn span(text: impl Into<String>) -> TextSpan {
    TextSpan {
        text: text.into(),
        style: SpanStyle::default(),
    }
}
impl TextSpan {
    /// Preferred family, such as `FontFamily::Monospace` for inline code.
    pub fn family(mut self, family: impl Into<FontFamily>) -> Self {
        self.style.family = Some(family.into());
        self
    }
    /// The generic monospace family.
    pub fn monospace(self) -> Self {
        self.family(FontFamily::Monospace)
    }
    /// Weight in `1..=1000`.
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.style.weight = Some(weight);
        self
    }
    /// `FontWeight::BOLD`.
    pub fn bold(self) -> Self {
        self.weight(FontWeight::BOLD)
    }
    /// Slope.
    pub fn style(mut self, style: FontStyle) -> Self {
        self.style.style = Some(style);
        self
    }
    /// `FontStyle::Italic`.
    pub fn italic(self) -> Self {
        self.style(FontStyle::Italic)
    }
    /// Font size in logical units; a line grows to fit its tallest span.
    pub fn font_size(mut self, size: f32) -> Self {
        self.style.font_size = Some(size);
        self
    }
    /// Literal or theme-bound color, replacing the element's text color.
    pub fn color(mut self, color: impl Into<crate::StyleColor>) -> Self {
        self.style.color = Some(color.into());
        self
    }
    /// Draws a single underline in the span's color.
    pub fn underline(mut self) -> Self {
        self.style.underline = true;
        self
    }
    /// Draws a line through the text in the span's color.
    pub fn strikethrough(mut self) -> Self {
        self.style.strikethrough = true;
        self
    }
}
impl From<&str> for TextSpan {
    fn from(text: &str) -> Self {
        span(text)
    }
}
impl From<String> for TextSpan {
    fn from(text: String) -> Self {
        span(text)
    }
}
impl SpanStyle {
    pub fn valid(&self) -> bool {
        self.weight.is_none_or(|w| (1..=1000).contains(&w.0))
            && self.font_size.is_none_or(|s| s.is_finite() && s > 0.)
            && self.color.is_none_or(|c| c.valid())
            && !matches!(&self.family, Some(FontFamily::Named(name)) if name.is_empty())
    }
    pub fn resolve(&self, range: std::ops::Range<usize>, theme: &crate::Theme) -> ResolvedSpan {
        ResolvedSpan {
            range,
            family: self.family.clone(),
            weight: self.weight,
            style: self.style,
            font_size: self.font_size,
            color: self.color.map(|c| c.resolve(theme)),
            underline: self.underline,
            strikethrough: self.strikethrough,
        }
    }
}
/// Rich text content: the concatenated text and each styled span's byte range.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RichText {
    pub text: String,
    pub spans: Vec<(std::ops::Range<usize>, SpanStyle)>,
}
impl RichText {
    pub fn new(spans: impl IntoIterator<Item = TextSpan>) -> Self {
        let mut text = String::new();
        let mut styled = Vec::new();
        for span in spans {
            let start = text.len();
            text.push_str(&span.text);
            if span.style != SpanStyle::default() && !span.text.is_empty() {
                styled.push((start..text.len(), span.style));
            }
        }
        Self {
            text,
            spans: styled,
        }
    }
}
/// A styled byte range with theme colors resolved, as passed to text measurement.
/// Unset attributes inherit the request's [`TextStyle`] and the element's color.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSpan {
    /// Byte range in the request text.
    pub range: std::ops::Range<usize>,
    /// Family override.
    pub family: Option<FontFamily>,
    /// Weight override.
    pub weight: Option<FontWeight>,
    /// Slope override.
    pub style: Option<FontStyle>,
    /// Font size override in logical units.
    pub font_size: Option<f32>,
    /// Linear RGBA color override.
    pub color: Option<crate::Color>,
    /// Single underline.
    pub underline: bool,
    /// Strikethrough line.
    pub strikethrough: bool,
}
