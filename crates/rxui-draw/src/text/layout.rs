use super::{FontId, TextStyle, cosmic};
use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

/// Retained selected font instance. Its source bytes survive dropping [`super::TextSystem`].
/// Instance weight matters for variable fonts; the source face and requested weight
/// together identify rasterization data. Generic variation-axis controls are not exposed yet.
#[derive(Clone, Debug)]
pub struct TextFont {
    pub(crate) id: FontId,
    pub(crate) face_index: u32,
    pub(crate) weight: u16,
    pub(crate) backend: Arc<cosmic::Font>,
}
impl TextFont {
    /// Source-scoped selected face identity.
    pub fn id(&self) -> FontId {
        self.id
    }
    /// Index in the source file/collection.
    pub fn face_index(&self) -> u32 {
        self.face_index
    }
    /// Requested instance weight used by the shaping backend.
    pub fn weight(&self) -> u16 {
        self.weight
    }
    /// Original immutable font file/collection bytes.
    pub fn data(&self) -> &[u8] {
        self.backend.data()
    }
    /// Read-only parsed font for custom rasterizers/outline consumers.
    pub fn as_cosmic(&self) -> &cosmic::Font {
        &self.backend
    }
}

/// One positioned glyph in a visual layout line. A glyph is not a character or caret.
#[derive(Clone, Debug, PartialEq)]
pub struct TextGlyph {
    /// Index in [`TextLayout::fonts`].
    pub font_index: usize,
    /// Face-local glyph ID; zero is the font's missing-glyph `.notdef` entry.
    pub glyph_id: u16,
    /// Whole-buffer UTF-8 byte range of the source cluster, excluding line endings.
    /// Multiple glyphs can share a range; one range can contain several characters.
    pub cluster: Range<usize>,
    /// Index in [`TextLayout::lines`].
    pub line_index: usize,
    /// Glyph origin in local units, including shaping offsets and the line baseline.
    /// X right, Y down; no pixel snapping or DPI has been applied.
    pub position: [f32; 2],
    /// Advance/hitbox origin before shaping offsets, in local units.
    pub advance_origin: [f32; 2],
    /// Horizontal advance/hitbox width, in local units, distinct from ink width.
    pub advance: f32,
    /// Shaping offsets in local units, X right/Y down.
    pub offset: [f32; 2],
    /// Glyph font size in local units.
    pub font_size: f32,
    /// Unicode bidi embedding level; odd levels are RTL.
    pub bidi_level: u8,
    /// Whether a renderer should synthesize italic slant for the selected upright face.
    /// Font fallback can require synthesis even when the requested style is italic.
    pub synthetic_italic: bool,
    /// Linear, straight RGBA from a [`super::TextSpan`], or `None` for the draw color.
    pub color: Option<[f32; 4]>,
}

/// Whether a [`TextDecoration`] is drawn below the baseline or through the glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDecorationKind {
    /// A line below the baseline at the font's underline position.
    Underline,
    /// A line through the glyphs at the font's strikeout position.
    Strikethrough,
}

/// One decoration line under or through a run of glyphs with the same span color.
///
/// Position and thickness come from the font's underline/strikeout metrics at the
/// glyphs' size. Rendering draws these filled rectangles after the glyphs.
#[derive(Clone, Debug, PartialEq)]
pub struct TextDecoration {
    /// Underline or strikethrough.
    pub kind: TextDecorationKind,
    /// Index in [`TextLayout::lines`].
    pub line_index: usize,
    /// Filled area in local units; width spans the glyph advances.
    pub rect: crate::Rect,
    /// Span color, or `None` for the draw color.
    pub color: Option<[f32; 4]>,
}

/// One visual line, including empty lines and wrapped portions of a source paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    /// Original paragraph/explicit-line index. Wrapped lines share this index.
    pub paragraph_index: usize,
    /// Whole-buffer source range for that entire paragraph, excluding its line ending.
    pub source: Range<usize>,
    /// Range in [`TextLayout::glyphs`] for this visual line, in backend drawing order.
    pub glyph_range: Range<usize>,
    /// Top of the line box in local units.
    pub top: f32,
    /// Baseline Y in local units.
    pub baseline: f32,
    /// Largest font ascent on the line, above the baseline, in local units. Editors
    /// can size carets and selections from `baseline - ascent` to `baseline + descent`.
    pub ascent: f32,
    /// Largest font descent on the line, below the baseline, in local units.
    pub descent: f32,
    /// Line box height in local units. Ink may exceed this box.
    pub height: f32,
    /// Advance width of the visual line, not its aligned position or ink bounds.
    pub width: f32,
    /// Leftmost advance position in local units. Empty lines retain their alignment anchor.
    pub left: f32,
    /// Paragraph direction detected by the Unicode bidi algorithm.
    pub rtl: bool,
}

/// Immutable CPU layout snapshot returned as `Arc<TextLayout>`.
///
/// [`super::TextBuffer::layout_viewport`] retains only intersecting visual lines.
/// Text offsets and positions remain document-relative; size and total line count
/// describe the full document. Missing glyph reports cover retained lines only.
/// Interaction returns geometry only for retained content; hit testing clamps to
/// retained lines and caret affinity can fall back at viewport edges. Selection
/// validates whole-document boundaries but emits only retained rectangles.
///
/// Retains text and selected font instances independently of its buffer/system.
/// Measurement uses advances and line boxes, not rasterized ink. Width is the
/// maximum line advance; height includes all line boxes, including a trailing empty
/// line after an explicit break. A width constraint controls wrapping/alignment;
/// it does not force the measured width to equal the constraint. Empty text has
/// zero width and one configured line box. Interaction geometry is indexed lazily;
/// it retains no backend buffer and requires no fonts/GPU after layout evaluation.
#[derive(Debug)]
pub struct TextLayout {
    pub(crate) line_sources: Vec<Range<usize>>,
    pub(crate) viewport: Option<Range<f32>>,
    pub(crate) line_offset: usize,
    pub(crate) total_lines: usize,
    pub(crate) system_id: u64,
    pub(crate) text: Arc<str>,
    pub(crate) style: TextStyle,
    pub(crate) glyphs: Vec<TextGlyph>,
    pub(crate) lines: Vec<TextLine>,
    pub(crate) fonts: Vec<TextFont>,
    pub(crate) missing: Vec<Range<usize>>,
    pub(crate) decorations: Vec<super::TextDecoration>,
    pub(crate) size: [f32; 2],
    pub(super) interaction:
        OnceLock<Result<Box<super::interaction::Interaction>, super::TextError>>,
}
impl TextLayout {
    /// Requested document Y range, or `None` for a full layout. Only intersecting
    /// line boxes have glyph/decoration geometry. Ink can extend past line boxes.
    pub fn viewport(&self) -> Option<Range<f32>> {
        self.viewport.clone()
    }
    /// Indices of retained lines in the full layout. Empty snapshots return `0..0`.
    /// Glyph, decoration and caret line indices instead index this snapshot's lines.
    pub fn line_range(&self) -> Range<usize> {
        self.line_offset..self.line_offset + self.lines.len()
    }
    /// Logical byte extent assigned to a retained visual line, including omitted
    /// wrap whitespace. Unlike [`TextLine::source`], this covers the wrapped portion,
    /// preserving interaction when adjacent lines are outside the viewport.
    /// The index is local to this snapshot. Returns `None` for an absent line.
    pub fn visual_line_source(&self, line_index: usize) -> Option<Range<usize>> {
        self.line_sources.get(line_index).cloned()
    }
    /// Exact number of visual lines in the document, including offscreen lines.
    pub fn total_line_count(&self) -> usize {
        self.total_lines
    }

    /// Original UTF-8 content, preserving explicit line endings.
    pub fn text(&self) -> &str {
        &self.text
    }
    /// Font selection and local-unit metrics used for this snapshot.
    pub fn style(&self) -> &TextStyle {
        &self.style
    }
    /// Positioned glyphs in per-line backend drawing order.
    pub fn glyphs(&self) -> &[TextGlyph] {
        &self.glyphs
    }
    /// Retained visual line boxes, including empty lines; all lines for full layouts.
    pub fn lines(&self) -> &[TextLine] {
        &self.lines
    }
    /// Selected, retained font instances used by glyphs.
    pub fn fonts(&self) -> &[TextFont] {
        &self.fonts
    }
    /// Distinct source clusters with `.notdef` glyphs, sorted by source byte index.
    /// The layout is still usable. This reports backend missing glyphs rather than
    /// guaranteeing that every Unicode codepoint has a visible representation.
    pub fn missing_glyphs(&self) -> &[Range<usize>] {
        &self.missing
    }
    /// Underlines and strikethroughs from spans, in line order.
    pub fn decorations(&self) -> &[super::TextDecoration] {
        &self.decorations
    }
    /// `[maximum_line_advance, total_line_box_height]` in local units.
    pub fn size(&self) -> [f32; 2] {
        self.size
    }
}

impl AsRef<TextLayout> for TextLayout {
    fn as_ref(&self) -> &TextLayout {
        self
    }
}
