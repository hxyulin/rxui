use super::{
    FontId, TextAlign, TextDecoration, TextDecorationKind, TextError, TextFont, TextGlyph,
    TextLayout, TextLine, TextLineMetrics, TextShaping, TextSpan, TextStyle, TextSystem, TextWrap,
    cosmic,
    span::{Segment, paragraph_attrs, segments},
};
use crate::Rect;
use std::{collections::HashMap, ops::Range, sync::Arc};

/// Retained UTF-8 text and paragraph constraints, evaluated explicitly on the CPU.
///
/// Setters do no shaping. [`Self::layout`] applies validated changes and returns a
/// shared immutable snapshot. Unchanged evaluation clones its Arc only; width,
/// wrap, alignment and default metric changes preserve backend shaped runs. Font
/// selection/features changes reshape. Content edits preserve matching leading and
/// trailing paragraphs and reshape changed paragraphs. Loading fonts or evaluating with a
/// different system also reshapes, avoiding stale fallback or face identities.
/// Every snapshot remains usable after later edits.
///
/// [`Self::set_rich_text`] adds [`TextSpan`] overrides of family, size, weight,
/// slant, tracking, color and decorations over byte ranges of the same buffer.
///
/// [`Self::measure`] and [`Self::intrinsic_widths`] size the content at other widths
/// from the same shaped runs, without changing the configured width or snapshot.
#[derive(Debug)]
pub struct TextBuffer {
    text: Arc<str>,
    style: TextStyle,
    spans: Vec<TextSpan>,
    segments: Vec<Segment>,
    width: Option<f32>,
    wrap: TextWrap,
    align: TextAlign,
    shaping: TextShaping,
    minimum_line_metrics: Option<TextLineMetrics>,
    backend: cosmic::Buffer,
    evaluated_system: Option<(u64, u64)>,
    text_dirty: bool,
    layout_dirty: bool,
    cached: Option<Arc<TextLayout>>,
    viewport_cached: Option<Arc<TextLayout>>,
    // Sizes at other widths for the current snapshot, most recent last.
    measured: Vec<(Option<u32>, [f32; 2])>,
}
// Layout engines usually ask for min-content, max-content and one available width.
const MEASURE_CACHE: usize = 4;
impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}
impl TextBuffer {
    /// Creates empty text with default 16/22-unit style, unconstrained width,
    /// word-or-glyph wrapping, and paragraph-start alignment. Does not require fonts.
    pub fn new() -> Self {
        let style = TextStyle::new();
        Self {
            text: Arc::from(""),
            backend: cosmic::Buffer::new_empty(cosmic::Metrics::new(
                style.font_size,
                style.line_height,
            )),
            style,
            spans: Vec::new(),
            segments: Vec::new(),
            width: None,
            wrap: TextWrap::default(),
            align: TextAlign::default(),
            shaping: TextShaping::default(),
            minimum_line_metrics: None,
            evaluated_system: None,
            text_dirty: true,
            layout_dirty: true,
            cached: None,
            viewport_cached: None,
            measured: Vec::new(),
        }
    }
    #[cfg(test)]
    pub(super) fn has_full_snapshot(&self) -> bool {
        self.cached.is_some()
    }
    /// Current original content; explicit line ending bytes are preserved.
    pub fn text(&self) -> &str {
        &self.text
    }
    /// Current font style and local-unit metrics.
    pub fn style(&self) -> &TextStyle {
        &self.style
    }
    /// Current wrapping/alignment width constraint, distinct from measured advance width.
    pub fn width(&self) -> Option<f32> {
        self.width
    }
    /// Current wrap policy.
    pub fn wrap(&self) -> TextWrap {
        self.wrap
    }
    /// Current paragraph alignment.
    pub fn align(&self) -> TextAlign {
        self.align
    }
    /// Style spans applied over the buffer style, in override order.
    pub fn spans(&self) -> &[TextSpan] {
        &self.spans
    }
    /// Replaces text and style after validation, without shaping, and removes any
    /// spans. Identical input retains the snapshot and text allocation. Rejected
    /// input changes nothing.
    pub fn set_text(&mut self, text: &str, style: TextStyle) -> Result<(), TextError> {
        self.set_rich_text(text, style, Vec::new())
    }
    /// Replaces text, base style and per-range [`TextSpan`] overrides after validation,
    /// without shaping. Spans must lie on `char` boundaries within `text`; later
    /// spans override fields set by earlier ones. Identical input retains the snapshot.
    /// Rejected input changes nothing.
    pub fn set_rich_text(
        &mut self,
        text: &str,
        style: TextStyle,
        spans: impl Into<Vec<TextSpan>>,
    ) -> Result<(), TextError> {
        style.validate()?;
        let spans = spans.into();
        for span in &spans {
            span.validate(text)?;
        }
        if self.text.as_ref() != text {
            self.text = Arc::from(text);
            self.text_dirty = true;
            self.layout_dirty = true;
        }
        if self.spans != spans {
            self.spans = spans;
            self.text_dirty = true;
            self.layout_dirty = true;
        }
        self.apply_style(style);
        Ok(())
    }
    /// Replaces style after validation; text stays unchanged. Metric-only changes
    /// relayout existing shaped runs; font selection/features/tracking changes reshape.
    pub fn set_style(&mut self, style: TextStyle) -> Result<(), TextError> {
        style.validate()?;
        self.apply_style(style);
        Ok(())
    }
    fn apply_style(&mut self, style: TextStyle) {
        if style != self.style {
            // Span attributes derive from the base style, including scaled line heights.
            self.text_dirty |= !self.style.same_shaping(&style) || !self.spans.is_empty();
            self.layout_dirty = true;
            self.style = style;
        }
    }
    /// Current CPU shaping strategy.
    pub fn shaping(&self) -> TextShaping {
        self.shaping
    }
    /// Selects CPU shaping. Changing strategy rebuilds backend paragraphs on the
    /// next evaluation; existing snapshots remain valid. Identical input is free.
    pub fn set_shaping(&mut self, shaping: TextShaping) {
        if self.shaping != shaping {
            self.shaping = shaping;
            self.evaluated_system = None;
            self.text_dirty = true;
            self.layout_dirty = true;
        }
    }
    /// Sets minimum ascent/descent/height for every visual line without reshaping.
    /// `None` restores natural metrics. Validation is atomic, and unchanged input
    /// retains the cached snapshot. Glyph and decoration baselines move together.
    pub fn set_minimum_line_metrics(
        &mut self,
        metrics: Option<TextLineMetrics>,
    ) -> Result<(), TextError> {
        if let Some(metrics) = metrics {
            metrics.validate()?;
        }
        if self.minimum_line_metrics != metrics {
            self.minimum_line_metrics = metrics;
            self.layout_dirty = true;
        }
        Ok(())
    }
    /// Selects a finite nonnegative width or `None`. Zero width is supported;
    /// indivisible glyph clusters can exceed it. This does not clip or limit height.
    pub fn set_width(&mut self, width: Option<f32>) -> Result<(), TextError> {
        if width.is_some_and(|w| !w.is_finite() || w < 0.) {
            return Err(TextError::InvalidWidth);
        }
        if self.width != width {
            self.width = width;
            self.layout_dirty = true;
        }
        Ok(())
    }
    /// Selects wrapping; backend shaped runs are reused on next evaluation.
    pub fn set_wrap(&mut self, wrap: TextWrap) {
        if self.wrap != wrap {
            self.wrap = wrap;
            self.layout_dirty = true;
        }
    }
    /// Selects alignment; backend shaped runs are reused on next evaluation.
    pub fn set_align(&mut self, align: TextAlign) {
        if self.align != align {
            self.align = align;
            self.layout_dirty = true;
        }
    }
    /// Evaluates all paragraphs and wrapped lines, returning retained local-unit
    /// metrics/glyphs. No viewport-height culling, pixel snapping, DPI or GPU work occurs.
    /// Missing glyphs are reported within a usable snapshot; an empty font database
    /// with nonempty paragraph content returns `NoFonts` before backend shaping.
    /// A previous snapshot remains valid if evaluation fails.
    pub fn layout(&mut self, system: &mut TextSystem) -> Result<Arc<TextLayout>, TextError> {
        self.evaluate(system, None)
    }
    /// Builds glyphs, decorations and interaction line boxes only for visual lines
    /// intersecting the half-open document Y range `visible`. Coordinates and UTF-8
    /// offsets remain document-relative; [`TextLayout::size`] is the exact full size.
    /// Pass overscan in `visible` to retain lines whose ink extends past their boxes.
    /// Empty or out-of-document ranges produce no lines. Scroll is never clamped.
    ///
    /// This bounds snapshot/GPU geometry, not backend shaping storage or reflow:
    /// cold evaluation shapes all paragraphs for exact size, scrolling scans line
    /// metrics, and edits reshape complete changed paragraphs (including long single
    /// paragraphs). The last full and last visible snapshots are cached separately.
    /// No GPU work or scheduling occurs. Use the result with ordinary renderer or
    /// worker glyph preparation APIs, and apply translation/scissoring when drawing.
    ///
    /// ```
    /// use rxui_draw::{TextBuffer, TextStyle, TextSystem};
    /// let mut fonts = TextSystem::new();
    /// let mut document = TextBuffer::new();
    /// document.set_text(&"\n".repeat(1000), TextStyle::new())?;
    /// let visible = document.layout_viewport(&mut fonts, 220.0..440.0)?;
    /// assert_eq!(visible.line_range(), 10..20);
    /// assert_eq!(visible.total_line_count(), 1001);
    /// assert_eq!(visible.size()[1], 1001.0 * 22.0);
    /// # Ok::<(), rxui_draw::TextError>(())
    /// ```
    pub fn layout_viewport(
        &mut self,
        system: &mut TextSystem,
        visible: Range<f32>,
    ) -> Result<Arc<TextLayout>, TextError> {
        if !visible.start.is_finite() || !visible.end.is_finite() || visible.start > visible.end {
            return Err(TextError::InvalidViewport);
        }
        self.evaluate(system, Some(visible))
    }
    fn evaluate(
        &mut self,
        system: &mut TextSystem,
        visible: Option<Range<f32>>,
    ) -> Result<Arc<TextLayout>, TextError> {
        let source = (system.id, system.generation);
        let changed = self.evaluated_system != Some(source) || self.layout_dirty || self.text_dirty;
        if !changed {
            let cached = if visible.is_some() {
                &self.viewport_cached
            } else {
                &self.cached
            };
            if let Some(layout) = cached
                && layout.viewport == visible
            {
                return Ok(layout.clone());
            }
        }
        profiling::scope!("rxui_draw::TextBuffer::layout");
        if system.backend.db().is_empty() && self.text.chars().any(|c| c != '\r' && c != '\n') {
            return Err(TextError::NoFonts);
        }
        self.backend.set_metrics(cosmic::Metrics::new(
            self.style.font_size,
            self.style.line_height,
        ));
        self.backend.set_size(self.width, None);
        self.backend.set_wrap(self.wrap.backend());
        if self.text_dirty || self.evaluated_system != Some(source) {
            self.segments = segments(self.text.len(), &self.style, &self.spans);
        }
        if self.evaluated_system != Some(source) {
            self.backend.set_text(
                &self.text,
                &self.style.attrs(),
                self.shaping.backend(),
                self.align.backend(),
            );
            if !self.segments.is_empty() {
                update_paragraphs(
                    &mut self.backend,
                    &self.text,
                    &self.style,
                    self.align,
                    self.shaping,
                    &self.segments,
                );
            }
        } else if self.text_dirty {
            update_paragraphs(
                &mut self.backend,
                &self.text,
                &self.style,
                self.align,
                self.shaping,
                &self.segments,
            );
        } else if changed {
            for line in &mut self.backend.lines {
                line.set_align(self.align.backend());
            }
        }
        let options = SnapshotOptions {
            minimum: self.minimum_line_metrics,
            viewport: visible.clone(),
            glyph_capacity: if visible.is_none() {
                self.cached
                    .as_ref()
                    .map_or(0, |layout| layout.glyphs().len().min(self.text.len()))
            } else {
                0
            },
        };
        let layout = if system.backend.db().is_empty() {
            // cosmic-text requests a default face even for empty paragraphs.
            // Blank text needs only line boxes and must work without a font.
            blank_snapshot(
                &self.backend,
                system.id,
                self.text.clone(),
                self.style.clone(),
                self.align,
                self.width,
                options,
            )?
        } else {
            if changed {
                self.backend.shape_until_scroll(&mut system.backend, false);
            }
            snapshot(
                &self.backend,
                system,
                self.text.clone(),
                self.style.clone(),
                &self.segments,
                options,
            )?
        };
        let layout = Arc::new(layout);
        if changed {
            self.cached = None;
            self.viewport_cached = None;
            self.measured.clear();
        }
        if visible.is_some() {
            self.viewport_cached = Some(layout.clone());
        } else {
            self.cached = Some(layout.clone());
        }
        self.evaluated_system = Some(source);
        self.text_dirty = false;
        self.layout_dirty = false;
        Ok(layout)
    }
}

impl TextBuffer {
    /// Content size `[width, height]` when wrapped at `width` (`None` is unconstrained),
    /// matching [`TextLayout::size`] of a buffer configured with that width.
    ///
    /// Evaluates the configured snapshot first if needed (see [`Self::layout`]), then
    /// lays out its shaped runs at `width` without reshaping, building glyph snapshots,
    /// or changing [`Self::width`] or the snapshot returned by [`Self::layout`]. The
    /// configured width reads the snapshot; the last few other widths are cached until
    /// the next change. Wrapping and paragraph alignment apply as configured.
    pub fn measure(
        &mut self,
        system: &mut TextSystem,
        width: Option<f32>,
    ) -> Result<[f32; 2], TextError> {
        if width.is_some_and(|w| !w.is_finite() || w < 0.) {
            return Err(TextError::InvalidWidth);
        }
        let layout = self.layout(system)?;
        if width == self.width || system.backend.db().is_empty() {
            // Blank snapshots have no shaped runs; their size does not depend on width.
            return Ok(layout.size());
        }
        let key = width.map(f32::to_bits);
        if let Some(index) = self.measured.iter().position(|(k, _)| *k == key) {
            let entry = self.measured.remove(index);
            self.measured.push(entry);
            return Ok(entry.1);
        }
        profiling::scope!("rxui_draw::TextBuffer::measure");
        let metrics = self.backend.metrics();
        let scratch = &mut system.measure;
        let mut size = [0f32, 0f32];
        for line in &self.backend.lines {
            let Some(shape) = line.shape_opt() else {
                size[1] += self
                    .minimum_line_metrics
                    .map_or(metrics.line_height, |minimum| {
                        metrics.line_height.max(minimum.height)
                    });
                continue;
            };
            scratch.lines.clear();
            shape.layout_to_buffer(
                &mut scratch.shape,
                metrics.font_size,
                width,
                self.wrap.backend(),
                self.backend.ellipsize(),
                line.align(),
                &mut scratch.lines,
                self.backend.monospace_width(),
                self.backend.hinting(),
            );
            for laid in &scratch.lines {
                size[0] = size[0].max(laid.w);
                size[1] += laid.line_height_opt.unwrap_or(metrics.line_height).max(
                    self.minimum_line_metrics
                        .map_or(0., |minimum| minimum.height),
                );
            }
        }
        if self.measured.len() == MEASURE_CACHE {
            self.measured.remove(0);
        }
        self.measured.push((key, size));
        Ok(size)
    }
    /// `[min_content, max_content]` widths: the narrowest width the wrap policy allows
    /// (the widest word, or widest glyph for [`TextWrap::WordOrGlyph`]) and the width
    /// with no wrapping. Both equal the unwrapped width for [`TextWrap::None`].
    /// Uses [`Self::measure`], so repeated queries are cached.
    pub fn intrinsic_widths(&mut self, system: &mut TextSystem) -> Result<[f32; 2], TextError> {
        let min = self.measure(system, Some(0.))?[0];
        let max = self.measure(system, None)?[0];
        Ok([min, max])
    }
}

fn update_paragraphs(
    buffer: &mut cosmic::Buffer,
    text: &str,
    style: &TextStyle,
    align: TextAlign,
    shaping: TextShaping,
    segments: &[Segment],
) {
    let mut ranges: Vec<_> = cosmic::LineIter::new(text).collect();
    if ranges
        .last()
        .is_none_or(|(_, ending)| *ending != cosmic::LineEnding::None)
    {
        ranges.push((text.len()..text.len(), cosmic::LineEnding::None));
    }
    let paragraphs: Vec<_> = ranges
        .iter()
        .map(|(range, ending)| (&text[range.clone()], *ending))
        .collect();
    let matches = |line: &cosmic::BufferLine, &(text, ending): &(&str, cosmic::LineEnding)| {
        line.text() == text && line.ending() == ending
    };
    let prefix = buffer
        .lines
        .iter()
        .zip(&paragraphs)
        .take_while(|(a, b)| matches(a, b))
        .count();
    let suffix = buffer.lines[prefix..]
        .iter()
        .rev()
        .zip(paragraphs[prefix..].iter().rev())
        .take_while(|(a, b)| matches(a, b))
        .count();
    let old_end = buffer.lines.len() - suffix;
    let new_end = paragraphs.len() - suffix;
    let attrs = style.attrs();
    if old_end - prefix == new_end - prefix {
        // Common label/paragraph edits keep String and backend shaping scratch.
        for (line, &(text, ending)) in buffer.lines[prefix..old_end]
            .iter_mut()
            .zip(&paragraphs[prefix..new_end])
        {
            line.set_text(text, ending, cosmic::AttrsList::new(&attrs));
        }
    } else {
        // Move matching suffix paragraphs rather than invalidating them when lines
        // are inserted or removed. Source offsets are rebuilt by the snapshot.
        buffer.lines.splice(
            prefix..old_end,
            paragraphs[prefix..new_end].iter().map(|&(text, ending)| {
                cosmic::BufferLine::new(
                    text,
                    ending,
                    cosmic::AttrsList::new(&attrs),
                    shaping.backend(),
                )
            }),
        );
    }
    for (line, (range, _)) in buffer.lines.iter_mut().zip(&ranges) {
        if segments.is_empty() {
            // Unstyled text avoids allocating a list per paragraph.
            if line.attrs_list().defaults() != attrs
                || line.attrs_list().spans_iter().next().is_some()
            {
                line.set_attrs_list(cosmic::AttrsList::new(&attrs));
            }
        } else {
            line.set_attrs_list(paragraph_attrs(style, &attrs, segments, range.clone()));
        }
        line.set_align(align.backend());
    }
    // Mark structural edits for evaluation without globally invalidating cached
    // shaping/layout. Newly inserted cosmic lines are Empty rather than Unused,
    // so the backend's individual-invalidation scan alone would miss them.
    // No evaluation occurs at the temporary scroll position.
    buffer.set_scroll(cosmic::Scroll {
        line: 1,
        ..Default::default()
    });
    buffer.set_scroll(cosmic::Scroll::default());
}

struct SnapshotOptions {
    glyph_capacity: usize,
    minimum: Option<TextLineMetrics>,
    viewport: Option<Range<f32>>,
}

fn blank_snapshot(
    buffer: &cosmic::Buffer,
    system_id: u64,
    text: Arc<str>,
    style: TextStyle,
    align: TextAlign,
    width: Option<f32>,
    options: SnapshotOptions,
) -> Result<TextLayout, TextError> {
    let SnapshotOptions {
        minimum, viewport, ..
    } = options;
    let mut lines = Vec::new();
    let mut line_offset = 0;
    let mut line_sources = Vec::new();
    let mut offset = 0;
    let mut top = 0.;
    for (paragraph_index, line) in buffer.lines.iter().enumerate() {
        let (baseline, ascent, descent, height) = adjusted_metrics(
            top,
            top + style.line_height / 2.,
            style.line_height / 2.,
            style.line_height / 2.,
            style.line_height,
            minimum,
        );
        if !baseline.is_finite() || !(top + height).is_finite() {
            return Err(TextError::LayoutOverflow);
        }
        if selected(&viewport, top, height) {
            if lines.is_empty() {
                line_offset = paragraph_index;
            }
            line_sources.push(offset..offset);
            lines.push(TextLine {
                paragraph_index,
                source: offset..offset,
                glyph_range: 0..0,
                top,
                baseline,
                ascent,
                descent,
                height,
                width: 0.,
                left: empty_anchor(align, width, false),
                rtl: false,
            });
        }
        offset += line.ending().as_str().len();
        top += height;
    }
    debug_assert_eq!(offset, text.len());
    Ok(TextLayout {
        system_id,
        text,
        style,
        lines,
        glyphs: Vec::new(),
        fonts: Vec::new(),
        missing: Vec::new(),
        decorations: Vec::new(),
        size: [0., top],
        interaction: Default::default(),
        viewport,
        line_sources,
        line_offset,
        total_lines: buffer.lines.len(),
    })
}

fn snapshot(
    buffer: &cosmic::Buffer,
    system: &mut TextSystem,
    text: Arc<str>,
    style: TextStyle,
    segments: &[Segment],
    options: SnapshotOptions,
) -> Result<TextLayout, TextError> {
    let SnapshotOptions {
        minimum,
        viewport,
        glyph_capacity,
    } = options;
    // Glyph metadata is the index of its segment plus one; zero is the buffer style.
    let color = |metadata: usize| metadata.checked_sub(1).and_then(|i| segments.get(i)?.color);
    // Match the backend's explicit CR/LF/CRLF/LFCR splitting exactly, including a
    // terminal empty paragraph. This avoids normalizing away source byte offsets.
    let mut offset = 0;
    let mut paragraphs = Vec::with_capacity(buffer.lines.len());
    for line in &buffer.lines {
        paragraphs.push(offset..offset + line.text().len());
        offset += line.text().len() + line.ending().as_str().len();
    }
    debug_assert_eq!(offset, text.len());
    let mut output = TextLayout {
        system_id: system.id,
        text,
        style,
        glyphs: Vec::with_capacity(glyph_capacity),
        lines: if viewport.is_none() {
            Vec::with_capacity(buffer.lines.len())
        } else {
            Vec::new()
        },
        fonts: Vec::new(),
        missing: Vec::new(),
        decorations: Vec::new(),
        size: [0., 0.],
        interaction: Default::default(),
        viewport,
        line_offset: 0,
        line_sources: Vec::new(),
        total_lines: 0,
    };
    let mut font_indices = HashMap::new();
    let mut last_font = None;
    // Runs are emitted in order for every layout line of every paragraph.
    let mut layout_line = (usize::MAX, 0);
    let mut runs = buffer.layout_runs().peekable();
    let mut top = 0.;
    while let Some(run) = runs.next() {
        layout_line = if layout_line.0 == run.line_i {
            (run.line_i, layout_line.1 + 1)
        } else {
            (run.line_i, 0)
        };
        let metrics = buffer.lines[run.line_i]
            .layout_opt()
            .and_then(|lines| lines.get(layout_line.1))
            .map_or((0., 0.), |line| (line.max_ascent, line.max_descent));
        if [
            run.line_y,
            run.line_top,
            run.line_height,
            run.line_w,
            run.line_top + run.line_height,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err(TextError::LayoutOverflow);
        }
        let (baseline, ascent, descent, height) = adjusted_metrics(
            top,
            run.line_y,
            metrics.0,
            metrics.1,
            run.line_height,
            minimum,
        );
        if !baseline.is_finite() || !(top + height).is_finite() {
            return Err(TextError::LayoutOverflow);
        }
        output.size[0] = output.size[0].max(run.line_w);
        output.size[1] = top + height;
        let line_top = top;
        top += height;
        let global_line = output.total_lines;
        output.total_lines += 1;
        if !selected(&output.viewport, line_top, height) {
            continue;
        }
        if output.lines.is_empty() {
            output.line_offset = global_line;
        }
        let line_index = output.lines.len();
        let glyph_start = output.glyphs.len();
        let mut left = f32::INFINITY;
        let paragraph = &paragraphs[run.line_i];
        for glyph in run.glyphs {
            let key = (glyph.font_id, glyph.font_weight.0);
            let font_index = if let Some((last, index)) = last_font
                && last == key
            {
                index
            } else if let Some(&index) = font_indices.get(&key) {
                index
            } else {
                let font = system
                    .backend
                    .get_font(glyph.font_id, glyph.font_weight)
                    .ok_or(TextError::FontUnavailable)?;
                let face_index = system
                    .backend
                    .db()
                    .face(glyph.font_id)
                    .ok_or(TextError::FontUnavailable)?
                    .index;
                let index = output.fonts.len();
                output.fonts.push(TextFont {
                    id: FontId {
                        system: system.id,
                        face: glyph.font_id,
                    },
                    face_index,
                    weight: glyph.font_weight.0,
                    backend: font,
                });
                // Single-face labels need neither per-glyph hashing nor a hash
                // allocation. Add the first face only when a second one appears.
                if index == 1 {
                    let first = &output.fonts[0];
                    font_indices.insert((first.id.face, first.weight), 0);
                }
                if index > 0 {
                    font_indices.insert(key, index);
                }
                index
            };
            last_font = Some((key, font_index));
            let offset = [
                glyph.font_size * glyph.x_offset,
                -glyph.font_size * glyph.y_offset,
            ];
            let advance_origin = [glyph.x, baseline + glyph.y];
            left = left.min(glyph.x.min(glyph.x + glyph.w));
            let position = [advance_origin[0] + offset[0], advance_origin[1] + offset[1]];
            if position
                .into_iter()
                .chain(advance_origin)
                .chain(offset)
                .chain([glyph.w, glyph.font_size, glyph.x + glyph.w])
                .any(|v| !v.is_finite())
            {
                return Err(TextError::LayoutOverflow);
            }
            let cluster = paragraph.start + glyph.start..paragraph.start + glyph.end;
            debug_assert!(output.text.get(cluster.clone()).is_some());
            if glyph.glyph_id == 0 {
                output.missing.push(cluster.clone());
            }
            output.glyphs.push(TextGlyph {
                font_index,
                glyph_id: glyph.glyph_id,
                cluster,
                line_index,
                position,
                advance_origin,
                advance: glyph.w,
                offset,
                font_size: glyph.font_size,
                bidi_level: glyph.level.number(),
                synthetic_italic: glyph
                    .cache_key_flags
                    .contains(cosmic::CacheKeyFlags::FAKE_ITALIC),
                color: color(glyph.metadata),
            });
        }
        for span in run.decorations {
            let data = &span.data;
            let size = span.font_size;
            let glyphs = &run.glyphs[span.glyph_range.clone()];
            // Split by span metadata so each color keeps its own line.
            for group in glyphs.chunk_by(|a, b| a.metadata == b.metadata) {
                let left = group.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
                let right = group
                    .iter()
                    .map(|g| g.x + g.w)
                    .fold(f32::NEG_INFINITY, f32::max);
                if right.partial_cmp(&left) != Some(std::cmp::Ordering::Greater) {
                    continue;
                }
                let lines = [
                    (
                        data.text_decoration.underline != cosmic::UnderlineStyle::None,
                        TextDecorationKind::Underline,
                        data.underline_metrics,
                    ),
                    (
                        data.text_decoration.strikethrough,
                        TextDecorationKind::Strikethrough,
                        data.strikethrough_metrics,
                    ),
                ];
                for (enabled, kind, metrics) in lines {
                    // Offsets are the top of the line above the baseline, in EM.
                    let rect = Rect::new(
                        left,
                        baseline - metrics.offset * size,
                        right - left,
                        metrics.thickness * size,
                    );
                    if !enabled || rect.height.partial_cmp(&0.) != Some(std::cmp::Ordering::Greater)
                    {
                        continue;
                    }
                    if !rect.bottom().is_finite() || !rect.right().is_finite() {
                        return Err(TextError::LayoutOverflow);
                    }
                    output.decorations.push(TextDecoration {
                        kind,
                        line_index,
                        rect,
                        color: color(group[0].metadata),
                    });
                }
            }
        }
        let begin = if layout_line.1 == 0 {
            paragraph.start
        } else {
            run.glyphs
                .iter()
                .map(|g| paragraph.start + g.start)
                .min()
                .unwrap_or(paragraph.start)
        };
        let end = runs
            .peek()
            .filter(|next| next.line_i == run.line_i)
            .and_then(|next| next.glyphs.iter().map(|g| paragraph.start + g.start).min())
            .unwrap_or(paragraph.end);
        output.line_sources.push(begin..end);
        output.lines.push(TextLine {
            paragraph_index: run.line_i,
            source: paragraph.clone(),
            glyph_range: glyph_start..output.glyphs.len(),
            top: line_top,
            baseline,
            ascent,
            descent,
            height,
            width: run.line_w,
            left: if left.is_finite() {
                left
            } else {
                empty_anchor(
                    buffer.lines[run.line_i]
                        .align()
                        .map_or(TextAlign::Start, |a| match a {
                            cosmic::Align::Left => TextAlign::Left,
                            cosmic::Align::Right => TextAlign::Right,
                            cosmic::Align::Center => TextAlign::Center,
                            cosmic::Align::End => TextAlign::End,
                            cosmic::Align::Justified => TextAlign::Justified,
                        }),
                    buffer.size().0,
                    run.rtl,
                )
            },
            rtl: run.rtl,
        });
    }
    output.missing.sort_by_key(|range| (range.start, range.end));
    output.missing.dedup();
    Ok(output)
}

fn empty_anchor(align: TextAlign, width: Option<f32>, rtl: bool) -> f32 {
    let width = width.unwrap_or(0.);
    match align {
        TextAlign::Center => width * 0.5,
        TextAlign::Right => width,
        TextAlign::Start if rtl => width,
        TextAlign::End if !rtl => width,
        _ => 0.,
    }
}

fn selected(viewport: &Option<Range<f32>>, top: f32, height: f32) -> bool {
    viewport
        .as_ref()
        .is_none_or(|v| v.start < v.end && top < v.end && top + height > v.start)
}

fn adjusted_metrics(
    top: f32,
    baseline: f32,
    ascent: f32,
    descent: f32,
    height: f32,
    minimum: Option<TextLineMetrics>,
) -> (f32, f32, f32, f32) {
    if let Some(minimum) = minimum {
        let ascent = ascent.max(minimum.ascent);
        let descent = descent.max(minimum.descent);
        let height = height.max(minimum.height);
        (
            top + (height - ascent - descent) * 0.5 + ascent,
            ascent,
            descent,
            height,
        )
    } else {
        (baseline, ascent, descent, height)
    }
}
