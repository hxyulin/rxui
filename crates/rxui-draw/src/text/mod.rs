//! Font loading, CPU shaping/layout, and explicit GPU coverage/color and distance-field text.
//!
//! Rich text uses one [`TextBuffer`] with [`TextSpan`] overrides over byte ranges;
//! span colors and underline/strikethrough decorations render in a single draw.
//!
//! Keep one [`TextSystem`] per application and one [`TextBuffer`] per retained text
//! item. Font discovery is explicit. [`TextBuffer::layout`] evaluates dirty state
//! and returns an immutable, shared [`TextLayout`]; an unchanged buffer returns
//! the same snapshot without reshaping. Width/wrap/alignment changes retain shaped
//! runs. Content edits retain matching leading/trailing paragraphs and reshape the
//! changed paragraphs. [`TextBuffer::layout_viewport`] copies only visible visual
//! lines into snapshots and GPU preparation, with exact total size; backend reflow
//! and scrolling metric scans still visit the document. Snapshots own
//! their source text and font references, so they survive later
//! edits, font loading, and dropping the system. No GPU initialization is required.
//!
//! Advanced shaping is the default. [`TextBuffer::set_shaping`] can opt controlled
//! text into [`TextShaping::Basic`], which maps characters directly to glyphs and
//! skips OpenType substitution/positioning. Callers must retain advanced shaping
//! for text or fonts requiring those operations. Printable ASCII interaction
//! indexing is accelerated independently of the shaping strategy.
//!
//! Horizontal slices can preserve offscreen typography with
//! [`TextBuffer::set_minimum_line_metrics`] and [`TextLineMetrics`]. Minimum
//! ascent/descent/height move glyph and decoration baselines together without
//! changing horizontal shaping, and apply consistently to measurement and interaction.
//!
//! All metrics use application-selected local units, X right/Y down. Layout does
//! not apply DPI, rasterize glyphs, or compute pixel ink bounds. Source clusters are
//! UTF-8 byte ranges, not character indices or caret boundaries. Glyphs can combine
//! multiple characters or share a source cluster. Missing glyphs are reported by
//! source range, while available fonts' `.notdef` glyphs remain in the layout.
//!
//! ```
//! use rxui_draw::text::{TextBuffer, TextStyle, TextSystem};
//! let mut text = TextSystem::new();
//! text.load_font(include_bytes!("../../tests/fonts/SourceSans3-Regular.otf"))?;
//! let mut label = TextBuffer::new();
//! label.set_text("Hello, office!", TextStyle::new().family("Source Sans 3"))?;
//! label.set_width(Some(200.0))?;
//! let layout = label.layout(&mut text)?;
//! assert!(!layout.glyphs().is_empty());
//! assert!(layout.size()[0] > 0.0);
//! assert!(std::sync::Arc::ptr_eq(&layout, &label.layout(&mut text)?));
//! # Ok::<(), rxui_draw::text::TextError>(())
//! ```
//!
//! Selectable text uses [`TextLayout::hit_test`], [`TextLayout::caret`] and
//! [`TextLayout::selection_rects`] on the same snapshot used for drawing. The
//! optional CPU index is built lazily; [`TextLayout::prepare_interaction`] can warm
//! it before input. Ordinary labels allocate no interaction vectors. Queries use
//! local layout units, so the host first undoes its draw translation/transform/DPI.
//! Positions are whole-buffer UTF-8 byte offsets at extended-grapheme boundaries,
//! with [`TextAffinity`] distinguishing soft-wrap/bidi edges. Ligature interior
//! carets divide the advance evenly; font GDEF caret positions are not yet used.
//! Applications retain anchor/focus, remap offsets after edits, and draw selection
//! rectangles/carets independently of immutable prepared glyph geometry.
//!
//! ```
//! use rxui_draw::{TextSystem, TextBuffer, TextStyle, TextPosition, TextAffinity};
//! let mut fonts = TextSystem::new();
//! fonts.load_font(include_bytes!("../../tests/fonts/SourceSans3-Regular.otf"))?;
//! let mut buffer = TextBuffer::new();
//! buffer.set_text("office e\u{301}", TextStyle::new().family("Source Sans 3"))?;
//! let layout = buffer.layout(&mut fonts)?;
//! assert_eq!(layout.interaction_bytes(), 0);
//! layout.prepare_interaction()?; // CPU only; no GPU resource preparation.
//! let position = layout.hit_test([10., 10.]).unwrap();
//! let caret = layout.caret(position).unwrap();
//! assert!(caret.height > 0.);
//! assert!(!layout.is_text_boundary(layout.text().len() - 1));
//! let end = TextPosition::new(layout.text().len()).affinity(TextAffinity::Upstream);
//! assert!(layout.caret(end).is_some());
//! let rectangles: Vec<_> = layout.selection_rects(0..6)?.collect();
//! assert_eq!(rectangles.len(), 1);
//! # Ok::<(), rxui_draw::TextError>(())
//! ```
//!
//! The CPU backend is re-exported as [`cosmic`] for font inspection/custom consumers.
//! [`TextSystem::as_cosmic`] and [`TextFont::as_cosmic`] expose read-only backend
//! access. Buffer mutation goes through validated setters to preserve invalidation.
//! [`TextRenderer::prepare_text`] rasterizes a retained layout into immutable
//! [`PreparedText`] geometry and atlas leases. [`TextRenderer::draw`] records those
//! batches into existing frame-owned passes, without shaping/rasterizing.
//! [`TextRenderer::prepare_texts`] prepares multiple layouts with shared geometry
//! uploads and returns independently drawable resources in input order. Raster
//! density is explicit and never scales layout-unit geometry. Draw/session transforms
//! apply geometry scaling consistently with other 2D content. Mask colors are linear,
//! while color glyph RGB is intrinsic. Pipeline preparation and drawing return
//! [`crate::Error`]; resource preparation returns [`TextRenderError`].
//! Cache budgets include prepared texts, recordings, and GPU completion leases.
//! [`crate::Painter`] exposes the same explicit preparation and drawing contract.
//! Select [`MtsdfOptions`] for scalable outline fill; coverage remains the default.
//! [`TextRenderer::request_glyphs`] captures CPU-only work for application workers.
//! [`TextRenderer::commit_glyphs`] uploads under explicit key/byte limits while
//! existing prepared text remains drawable; no runtime-owned executor is introduced.
//!
//! ```no_run
//! use rxui_draw::{GraphicsContext, FramebufferOptions, TextSystem, TextBuffer,
//!     TextStyle, TextRenderer, TextRasterOptions, TextDraw};
//! # fn draw() -> Result<(), Box<dyn std::error::Error>> {
//! let graphics = pollster::block_on(GraphicsContext::headless())?;
//! let mut target = graphics.create_framebuffer(FramebufferOptions::new(320, 100))?;
//! let mut fonts = TextSystem::new();
//! fonts.load_font(include_bytes!("../../tests/fonts/SourceSans3-Regular.otf"))?;
//! let mut label = TextBuffer::new();
//! label.set_text("Hello, office!", TextStyle::new().family("Source Sans 3"))?;
//! let layout = label.layout(&mut fonts)?;
//! let mut renderer = TextRenderer::new(&graphics);
//! renderer.prepare(&target.render_format())?;
//! let prepared = renderer.prepare_text(&layout, TextRasterOptions::new())?;
//! let mut frame = target.begin_frame()?;
//! {
//!     let mut pass = frame.render_pass().begin()?;
//!     renderer.draw(&mut pass, &prepared, TextDraw::new([20., 20.]))?;
//! }
//! frame.finish()?;
//! # Ok(()) }
//! ```

//! ```no_run
//! use rxui_draw::{Painter, TextLayout, MtsdfOptions, PreparedText, TextRenderError};
//! fn prepare(painter: &mut Painter, layout: &TextLayout, dpi: f32)
//!     -> Result<PreparedText, TextRenderError> {
//!     painter.prepare_text(layout,
//!         MtsdfOptions::new().pixels_per_em(64).range_em(0.25).raster_scale(dpi))
//! }
//! ```

mod buffer;
mod distance_field;
mod interaction;
mod layout;
mod renderer;
mod span;
mod style;
mod system;

pub use buffer::TextBuffer;
pub use cosmic_text as cosmic;
pub use interaction::{SelectionRects, TextAffinity, TextCaret, TextPosition};
pub use layout::{TextDecoration, TextDecorationKind, TextFont, TextGlyph, TextLayout, TextLine};
pub use renderer::{
    GeneratedGlyphs, GlyphPreparationLimits, GlyphPreparationRequest, GlyphUploadBudget,
    GlyphUploadProgress, MtsdfOptions, PreparedText, TextDraw, TextPreparation, TextRasterOptions,
    TextRenderError, TextRenderer, TextRendererOptions, TextRendererStats, TextTrim, TextUpload,
};
pub use span::TextSpan;
pub use style::{
    FontFamily, FontSlant, FontStretch, TextAlign, TextLineMetrics, TextShaping, TextStyle,
    TextWrap,
};
pub use system::{FontId, FontInfo, TextSystem};

/// Font loading, paragraph layout, or text interaction failure. Rejected setters leave the buffer unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextError {
    /// The provided data contains no faces usable by the shaping backend.
    InvalidFont,
    /// Font size/line height must be positive and finite, weight in `1..=1000`,
    /// tracking finite, and feature tags four printable ASCII bytes.
    InvalidStyle,
    /// Width must be finite and nonnegative, or `None` for unconstrained width.
    InvalidWidth,
    /// Viewport endpoints must be finite and ordered.
    InvalidViewport,
    /// Nonempty content requires at least one usable font.
    NoFonts,
    /// Selected font data cannot be loaded or parsed.
    FontUnavailable,
    /// Computed positions or line metrics overflowed finite floating-point units.
    LayoutOverflow,
    /// Selection endpoints must be ordered, in bounds, and at grapheme/paragraph-break boundaries.
    InvalidSelection,
    /// Span ranges must be ordered and on `char` boundaries within the text, with
    /// positive finite sizes, weight in `1..=1000`, finite tracking and valid colors.
    InvalidSpan,
}
impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidFont => "font data contains no usable faces",
            Self::InvalidStyle => "text metrics must be positive and finite, weight in 1..=1000, tracking finite, and feature tags printable ASCII",
            Self::InvalidViewport => "text viewport must use finite ordered endpoints",
            Self::InvalidWidth => "text width must be finite and nonnegative",
            Self::NoFonts => "load a font before laying out nonempty text",
            Self::FontUnavailable => "selected font data is unavailable or invalid",
            Self::LayoutOverflow => "text layout positions or metrics exceed finite units",
            Self::InvalidSelection => "selection must use ordered valid text boundaries",
            Self::InvalidSpan => "text spans must use ordered char boundaries within the text and valid style values",
        })
    }
}
impl std::error::Error for TextError {}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod interaction_tests;

#[cfg(test)]
mod viewport_tests;
