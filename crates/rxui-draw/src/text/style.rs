use super::{TextError, cosmic};

/// Preferred family; advanced shaping can use other loaded faces for fallback.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FontFamily {
    /// A preferred named family.
    Named(String),
    /// Generic sans-serif family, the default.
    #[default]
    SansSerif,
    /// Generic serif family.
    Serif,
    /// Generic monospace family.
    Monospace,
    /// Generic cursive family.
    Cursive,
    /// Generic fantasy family.
    Fantasy,
}
impl From<&str> for FontFamily {
    fn from(value: &str) -> Self {
        Self::Named(value.into())
    }
}
impl From<String> for FontFamily {
    fn from(value: String) -> Self {
        Self::Named(value)
    }
}
impl FontFamily {
    pub(crate) fn backend(&self) -> cosmic::Family<'_> {
        match self {
            Self::Named(name) => cosmic::Family::Name(name),
            Self::SansSerif => cosmic::Family::SansSerif,
            Self::Serif => cosmic::Family::Serif,
            Self::Monospace => cosmic::Family::Monospace,
            Self::Cursive => cosmic::Family::Cursive,
            Self::Fantasy => cosmic::Family::Fantasy,
        }
    }
}
/// Requested font slope; fallback may select a nearby available style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontSlant {
    /// Upright, the default.
    #[default]
    Normal,
    /// Italic.
    Italic,
    /// Oblique.
    Oblique,
}
impl FontSlant {
    pub(crate) fn backend(self) -> cosmic::Style {
        match self {
            Self::Normal => cosmic::Style::Normal,
            Self::Italic => cosmic::Style::Italic,
            Self::Oblique => cosmic::Style::Oblique,
        }
    }
    pub(crate) fn from_backend(value: cosmic::Style) -> Self {
        match value {
            cosmic::Style::Normal => Self::Normal,
            cosmic::Style::Italic => Self::Italic,
            cosmic::Style::Oblique => Self::Oblique,
        }
    }
}
/// Requested width class from the backend's font matching types.
pub use cosmic::Stretch as FontStretch;

/// CPU shaping strategy. Advanced shaping remains the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextShaping {
    /// OpenType shaping and font fallback for general Unicode text.
    #[default]
    Advanced,
    /// Direct character-to-glyph mapping without OpenType substitutions or
    /// positioning. Use only when the caller controls the text and font, such as
    /// printable ASCII in a known monospace face. Complex scripts need Advanced.
    Basic,
}
impl TextShaping {
    pub(crate) fn backend(self) -> cosmic::Shaping {
        match self {
            Self::Advanced => cosmic::Shaping::Advanced,
            Self::Basic => cosmic::Shaping::Basic,
        }
    }
}

/// Minimum vertical metrics for each laid-out line. Useful when a horizontal
/// slice must preserve the baseline of a larger logical line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLineMetrics {
    /// Minimum ascent above the baseline, in local units.
    pub ascent: f32,
    /// Minimum descent below the baseline, in local units.
    pub descent: f32,
    /// Minimum line-box height, in local units; leading is centered.
    pub height: f32,
}
impl TextLineMetrics {
    pub(crate) fn validate(self) -> Result<(), TextError> {
        if !self.ascent.is_finite()
            || self.ascent < 0.
            || !self.descent.is_finite()
            || self.descent < 0.
            || !self.height.is_finite()
            || self.height <= 0.
            || !(self.ascent + self.descent).is_finite()
        {
            return Err(TextError::InvalidStyle);
        }
        Ok(())
    }
}

/// Wrapping policy. Breaks are calculated by the shaping/layout backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrap {
    /// Respect explicit line breaks only.
    None,
    /// Break at glyph clusters when needed.
    Glyph,
    /// Break at words; a long word can exceed the width.
    Word,
    /// Break at words, falling back to glyph clusters for long words.
    #[default]
    WordOrGlyph,
}
impl TextWrap {
    pub(crate) fn backend(self) -> cosmic::Wrap {
        match self {
            Self::None => cosmic::Wrap::None,
            Self::Glyph => cosmic::Wrap::Glyph,
            Self::Word => cosmic::Wrap::Word,
            Self::WordOrGlyph => cosmic::Wrap::WordOrGlyph,
        }
    }
}
/// Line alignment within the width constraint. Direction is detected per paragraph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// Left for LTR paragraphs, right for RTL paragraphs.
    #[default]
    Start,
    /// Right for LTR paragraphs, left for RTL paragraphs.
    End,
    /// Always left.
    Left,
    /// Always right.
    Right,
    /// Center.
    Center,
    /// Justify eligible lines using the backend's paragraph rules.
    Justified,
}
impl TextAlign {
    pub(crate) fn backend(self) -> Option<cosmic::Align> {
        match self {
            Self::Start => None,
            Self::End => Some(cosmic::Align::End),
            Self::Left => Some(cosmic::Align::Left),
            Self::Right => Some(cosmic::Align::Right),
            Self::Center => Some(cosmic::Align::Center),
            Self::Justified => Some(cosmic::Align::Justified),
        }
    }
}

/// Font selection and paragraph metrics. Colors and DPI are not shaping properties.
///
/// Setters on [`super::TextBuffer`] validate this value before changing retained
/// state. Metrics use application-selected local units. Defaults are 16-unit font
/// size, 22-unit line height, normal 400-weight sans-serif, and no extra tracking.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// Preferred family, with advanced fallback for missing glyphs.
    pub family: FontFamily,
    /// Positive finite font size in local units.
    pub font_size: f32,
    /// Positive finite distance between line tops in local units.
    pub line_height: f32,
    /// OpenType/CSS weight in `1..=1000`.
    pub weight: u16,
    /// Requested slope.
    pub slant: FontSlant,
    /// Requested width class.
    pub stretch: FontStretch,
    /// Extra tracking in EM units, before local-unit scaling.
    pub letter_spacing: f32,
    /// OpenType feature overrides, in order. Builders replace an existing tag.
    pub features: Vec<([u8; 4], u32)>,
}
impl Default for TextStyle {
    fn default() -> Self {
        Self::new()
    }
}
impl TextStyle {
    /// Creates the default font style and local-unit metrics.
    pub const fn new() -> Self {
        Self {
            family: FontFamily::SansSerif,
            font_size: 16.,
            line_height: 22.,
            weight: 400,
            slant: FontSlant::Normal,
            stretch: FontStretch::Normal,
            letter_spacing: 0.,
            features: Vec::new(),
        }
    }
    /// Selects a named or generic preferred family.
    pub fn family(mut self, family: impl Into<FontFamily>) -> Self {
        self.family = family.into();
        self
    }
    /// Selects font size; buffer setters validate it.
    pub const fn font_size(mut self, value: f32) -> Self {
        self.font_size = value;
        self
    }
    /// Selects line height; buffer setters validate it.
    pub const fn line_height(mut self, value: f32) -> Self {
        self.line_height = value;
        self
    }
    /// Selects weight; buffer setters validate it.
    pub const fn weight(mut self, value: u16) -> Self {
        self.weight = value;
        self
    }
    /// Selects requested slope.
    pub const fn slant(mut self, value: FontSlant) -> Self {
        self.slant = value;
        self
    }
    /// Selects requested width class.
    pub const fn stretch(mut self, value: FontStretch) -> Self {
        self.stretch = value;
        self
    }
    /// Selects extra tracking in EM units.
    pub const fn letter_spacing(mut self, value: f32) -> Self {
        self.letter_spacing = value;
        self
    }
    /// Overrides an OpenType feature, for example `.feature(*b"liga", 0)`.
    pub fn feature(mut self, tag: [u8; 4], value: u32) -> Self {
        if let Some(feature) = self.features.iter_mut().find(|(t, _)| *t == tag) {
            feature.1 = value;
        } else {
            self.features.push((tag, value));
        }
        self
    }
    /// Validates metrics, weight, tracking, and printable OpenType feature tags.
    pub fn validate(&self) -> Result<(), TextError> {
        if !self.font_size.is_finite()
            || self.font_size <= 0.
            || !self.line_height.is_finite()
            || self.line_height <= 0.
            || !(1..=1000).contains(&self.weight)
            || !self.letter_spacing.is_finite()
            || self
                .features
                .iter()
                .any(|(tag, _)| tag.iter().any(|b| !(0x20..=0x7e).contains(b)))
        {
            return Err(TextError::InvalidStyle);
        }
        Ok(())
    }
    pub(crate) fn attrs(&self) -> cosmic::Attrs<'_> {
        let mut features = cosmic::FontFeatures::new();
        for (tag, value) in &self.features {
            features.set(cosmic::FeatureTag::new(tag), *value);
        }
        cosmic::Attrs::new()
            .family(self.family.backend())
            .weight(cosmic::Weight(self.weight))
            .style(self.slant.backend())
            .stretch(self.stretch)
            .letter_spacing(self.letter_spacing)
            .font_features(features)
    }
    pub(crate) fn same_shaping(&self, other: &Self) -> bool {
        self.family == other.family
            && self.weight == other.weight
            && self.slant == other.slant
            && self.stretch == other.stretch
            && self.letter_spacing == other.letter_spacing
            && self.features == other.features
    }
}
