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
