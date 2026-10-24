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
/// How labels and button captions fit their available content width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextOverflow {
    /// Wrap at word boundaries and preserve paragraph breaks.
    #[default]
    Wrap,
    /// Show one line, replacing hidden text with an ellipsis. Explicit paragraph
    /// breaks end the visible line. Inputs and selectable labels keep their editing behavior.
    Ellipsis,
}
/// Step of the theme's type scale; see [`crate::ThemeMetrics`]. Set it with
/// [`crate::Element::text_role`], which resolves the size and weight in the
/// element's theme and follows theme switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextRole {
    /// Small secondary text, such as hints and status bar items.
    Caption,
    /// Ordinary text at the theme's font size, the default.
    #[default]
    Body,
    /// Smallest heading, such as a group or panel header.
    Subheading,
    /// Section heading.
    Heading,
    /// Page or dialog title.
    Title,
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
    /// Wrapping or single-line truncation.
    pub overflow: TextOverflow,
}
impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: FontFamily::default(),
            weight: FontWeight::default(),
            style: FontStyle::default(),
            line_height: 1.4,
            align: TextAlign::default(),
            overflow: TextOverflow::default(),
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
    pub overflow: Option<TextOverflow>,
    /// Own type-scale role. It never inherits: descendants inherit the size and
    /// weight it resolves to instead.
    pub role: Option<TextRole>,
}
impl Overrides {
    /// This element's overrides layered over inherited ones, without its role.
    pub fn over(&self, inherited: &Self) -> Self {
        Self {
            role: None,
            family: self.family.clone().or_else(|| inherited.family.clone()),
            weight: self.weight.or(inherited.weight),
            style: self.style.or(inherited.style),
            line_height: self.line_height.or(inherited.line_height),
            align: self.align.or(inherited.align),
            overflow: self.overflow.or(inherited.overflow),
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
            overflow: self.overflow.unwrap_or_default(),
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
/// A span with [`TextSpan::link`] is also an activatable link; see [`link`].
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
#[derive(Clone, Debug)]
pub struct TextSpan {
    pub(crate) text: String,
    pub(crate) style: SpanStyle,
    pub(crate) link: Option<Box<LinkTarget>>,
}
/// Link spans are equal when they share one listener and the same metadata.
impl PartialEq for TextSpan {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.style == other.style && self.link == other.link
    }
}
/// Attribute overrides of one span; see [`TextSpan`].
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SpanStyle {
    pub family: Option<FontFamily>,
    pub weight: Option<FontWeight>,
    pub style: Option<FontStyle>,
    pub font_size: Option<f32>,
    pub color: Option<crate::StyleColor>,
    /// Explicit underline; unset means underlined only for links.
    pub underline: Option<bool>,
    pub strikethrough: bool,
    /// Link spans default to the accent color and an underline.
    pub link: bool,
}
/// Activation target of a link span: its listener and published metadata.
#[derive(Clone, Debug)]
pub(crate) struct LinkTarget {
    pub listener: crate::Listener<LinkEvent>,
    pub url: Option<String>,
    pub description: Option<String>,
}
impl PartialEq for LinkTarget {
    fn eq(&self, other: &Self) -> bool {
        self.listener.same(&other.listener)
            && self.url == other.url
            && self.description == other.description
    }
}
/// Activation of a link span in a [`crate::rich_text`] element, delivered by a
/// primary click, Enter on the focused link, or an assistive Click action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkEvent {
    /// Zero-based position of the link among the element's link spans, in text
    /// order, so one listener can serve generated links.
    pub index: usize,
}
/// A span of text for [`crate::rich_text`], inheriting every attribute until set.
pub fn span(text: impl Into<String>) -> TextSpan {
    TextSpan {
        text: text.into(),
        style: SpanStyle::default(),
        link: None,
    }
}
/// A link span for [`crate::rich_text`], the same as `span(text).link(listener)`.
/// It defaults to the theme's accent color and an underline, both overridable like
/// any span attribute.
///
/// Each link is a Tab stop in text order. A primary click, Enter on the focused
/// link or an assistive Click action dispatches its listener. Hovering a link
/// shows the pointer cursor, and assistive technology sees a Link node named by
/// its text. The element stays one text leaf: only elements with links pay for
/// link hit testing and semantics.
///
/// ```
/// use rxui::prelude::*;
/// struct Hover { opened: Option<usize> }
/// impl View for Hover {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         let open = cx.listener(|this, event: &LinkEvent, _| this.opened = Some(event.index));
///         rich_text([
///             span("expected "),
///             span("Vec<u8>").monospace(),
///             span(", see "),
///             link("Vec", open.clone())
///                 .link_url("https://doc.rust-lang.org/std/vec/struct.Vec.html"),
///             span(" and "),
///             span("u8").monospace().link(open),
///         ])
///     }
/// }
/// ```
pub fn link(text: impl Into<String>, listener: crate::Listener<LinkEvent>) -> TextSpan {
    span(text).link(listener)
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
    pub fn underline(self) -> Self {
        self.underlined(true)
    }
    /// Sets or clears the underline. Links are underlined unless this clears it.
    pub fn underlined(mut self, value: bool) -> Self {
        self.style.underline = Some(value);
        self
    }
    /// Draws a line through the text in the span's color.
    pub fn strikethrough(mut self) -> Self {
        self.style.strikethrough = true;
        self
    }
    /// Makes this span a link that dispatches `listener` with its [`LinkEvent`];
    /// see [`link`]. Unless set, the span takes the theme's accent color and an
    /// underline.
    pub fn link(mut self, listener: crate::Listener<LinkEvent>) -> Self {
        self.style.link = true;
        match &mut self.link {
            Some(target) => target.listener = listener,
            None => {
                self.link = Some(Box::new(LinkTarget {
                    listener,
                    url: None,
                    description: None,
                }))
            }
        }
        self
    }
    /// Destination published to assistive technology as the link's URL. RXUI never
    /// opens it; the link listener decides what activation does.
    ///
    /// # Panics
    ///
    /// Panics unless [`Self::link`] was called first.
    pub fn link_url(mut self, url: impl Into<String>) -> Self {
        self.link
            .as_mut()
            .expect("link_url requires a link span")
            .url = Some(url.into());
        self
    }
    /// Additional help published to assistive technology, separate from the
    /// link's name (its text), such as "Opens the definition of Vec".
    ///
    /// # Panics
    ///
    /// Panics unless [`Self::link`] was called first.
    pub fn link_description(mut self, description: impl Into<String>) -> Self {
        self.link
            .as_mut()
            .expect("link_description requires a link span")
            .description = Some(description.into());
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
        // Links without their own color take the accent.
        let color = self.color.or(self
            .link
            .then_some(crate::StyleColor::Theme(crate::ThemeColor::Accent)));
        ResolvedSpan {
            range,
            family: self.family.clone(),
            weight: self.weight,
            style: self.style,
            font_size: self.font_size,
            color: color.map(|c| c.resolve(theme)),
            underline: self.underline.unwrap_or(self.link),
            strikethrough: self.strikethrough,
        }
    }
}
/// One link span of a rich text element.
#[derive(Clone, Debug)]
pub(crate) struct Link {
    pub target: LinkTarget,
    /// Whether the span uses the default accent color, so hover and press can
    /// recolor it with the accent's interaction states.
    pub accent: bool,
}
/// Rich text content: the concatenated text, each styled span's byte range, and
/// the link spans in text order.
#[derive(Clone, Debug)]
pub(crate) struct RichText {
    pub text: String,
    pub spans: Vec<(std::ops::Range<usize>, SpanStyle)>,
    /// Byte ranges of link spans, indexed like [`LinkEvent::index`].
    pub link_ranges: Vec<std::ops::Range<usize>>,
    pub links: Vec<Link>,
}
impl RichText {
    pub fn new(spans: impl IntoIterator<Item = TextSpan>) -> Self {
        let mut text = String::new();
        let mut styled = Vec::new();
        let mut link_ranges = Vec::new();
        let mut links = Vec::new();
        for span in spans {
            let start = text.len();
            text.push_str(&span.text);
            if let Some(target) = span.link {
                link_ranges.push(start..text.len());
                links.push(Link {
                    target: *target,
                    accent: span.style.color.is_none(),
                });
            }
            if span.style != SpanStyle::default() && !span.text.is_empty() {
                styled.push((start..text.len(), span.style));
            }
        }
        Self {
            text,
            spans: styled,
            link_ranges,
            links,
        }
    }
    /// Whether any link can take focus or a click; empty links cannot.
    pub fn has_links(&self) -> bool {
        self.link_ranges.iter().any(|r| !r.is_empty())
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
