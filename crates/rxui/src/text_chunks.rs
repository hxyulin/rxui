//! Long wrapped text split into chunks of whole paragraphs, each shaped, laid out
//! and prepared by its own buffer.
//!
//! Astrelis reshapes only changed paragraphs, but each layout snapshot, interaction
//! index and prepared glyph geometry still covers its whole buffer. A text area of
//! several thousand lines would rebuild all three for every keystroke. Chunks bound
//! that work to the paragraphs an edit touches; the rest of the document costs one
//! comparison of its bytes. Paragraphs wrap, align and resolve bidirectional order
//! independently, so stacking chunk layouts reproduces the single-buffer layout.
//!
//! Each chunk excludes the line break that ends its last paragraph; that byte sits
//! between two chunks. Offsets and geometry translate between whole-text and
//! chunk-local units here, so the rest of the painter sees one layout.
use super::{FamilyMap, TextResource, text_width};
use crate::{TextRequest, UiError};
use astrelis::{PreparedText, Rect, TextCaret, TextLayout, TextPosition, TextSystem};
use std::{ops::Range, sync::Arc};

/// Target paragraphs per chunk. Text with at least twice as many is chunked; a
/// chunked text falls back to one buffer below half as many.
pub(super) const PARAGRAPHS: usize = 8;

/// Whether `text` should be chunked, given whether it currently is. Counting stops
/// at the threshold, so a long text costs no more than a short one.
pub(super) fn wanted(text: &str, chunked: bool) -> bool {
    let threshold = if chunked {
        PARAGRAPHS / 2
    } else {
        2 * PARAGRAPHS
    };
    text.bytes()
        .filter(|b| *b == b'\n')
        .take(threshold - 1)
        .count()
        == threshold - 1
}
fn paragraphs(text: &str) -> usize {
    text.as_bytes()
        .iter()
        .map(|b| usize::from(*b == b'\n'))
        .sum::<usize>()
        + 1
}

#[derive(Default)]
pub(super) struct Chunk {
    /// Byte offset of the chunk's first paragraph in the whole text.
    pub start: usize,
    /// Byte length, excluding the line break after the last paragraph.
    pub len: usize,
    /// Top of the chunk's first line box in whole-text local units.
    pub top: f32,
    /// Layout at the last laid-out width; prepared geometry follows it.
    pub layout: Option<Arc<TextLayout>>,
    pub resource: TextResource,
}
impl Chunk {
    fn end(&self) -> usize {
        self.start + self.len
    }
    fn height(&self) -> f32 {
        self.layout.as_ref().map_or(0., |l| l.size()[1])
    }
}
/// Key of the last layout pass: text revision, width and font generation.
type LayoutKey = (u64, Option<f32>, u64);

#[derive(Default)]
pub(super) struct Chunks {
    /// Text the chunk ranges describe.
    text: String,
    revision: Option<u64>,
    pub list: Vec<Chunk>,
    laid_out: Option<LayoutKey>,
    /// Recent measurements, so layout probes do not revisit every chunk.
    measured: Vec<(LayoutKey, [f32; 2])>,
    /// Union of prepared glyph ink in whole-text local units.
    pub ink: Option<Rect>,
}
fn chunk_request<'a>(request: TextRequest<'a>, range: Range<usize>) -> TextRequest<'a> {
    TextRequest {
        text: &request.text[range],
        ..request
    }
}
/// Splits `text[range]`, which starts and ends at paragraph boundaries, into chunks
/// of about [`PARAGRAPHS`] paragraphs. The first reuses `first`'s buffer, so an edit
/// inside it keeps the shaping of its unchanged paragraphs.
fn split(text: &str, range: Range<usize>, first: TextResource) -> Vec<Chunk> {
    let count = paragraphs(&text[range.clone()]);
    let pieces = ((count + PARAGRAPHS / 2) / PARAGRAPHS).max(1);
    let per_piece = count.div_ceil(pieces);
    let mut chunks = Vec::with_capacity(pieces);
    let mut first = Some(first);
    let mut start = range.start;
    let mut remaining = count;
    while remaining > 0 {
        let take = per_piece.min(remaining);
        remaining -= take;
        let end = if remaining == 0 {
            range.end
        } else {
            // The line break ending this piece's last paragraph.
            let mut end = start;
            for _ in 0..take {
                end += text[end..range.end].find('\n').unwrap() + 1;
            }
            end - 1
        };
        chunks.push(Chunk {
            start,
            len: end - start,
            resource: first.take().unwrap_or_default(),
            ..Chunk::default()
        });
        start = end + 1;
    }
    chunks
}
impl Chunks {
    /// Re-chunks only the region an edit touched: chunks before and after it keep
    /// their buffers, layouts and prepared geometry and only shift their offsets.
    fn sync(&mut self, text: &str, revision: u64) {
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        if self.list.is_empty() {
            self.list = split(text, 0..text.len(), TextResource::default());
        } else if self.text != text {
            let (prefix, suffix) = crate::editing::common_ends(&self.text, text);
            let changed_end = self.text.len() - suffix;
            // First chunk whose paragraphs or following break reach the change, and
            // one past the last chunk starting within it.
            let a = self.list.partition_point(|c| c.end() < prefix);
            let mut b = self
                .list
                .partition_point(|c| c.start <= changed_end)
                .max(a + 1);
            let grow = |end: usize| (end + text.len()).checked_sub(self.text.len()).unwrap();
            // A small region absorbs the next chunk, so chunks do not fragment.
            while b < self.list.len()
                && paragraphs(&text[self.list[a].start..grow(self.list[b - 1].end())])
                    < PARAGRAPHS / 2
            {
                b += 1;
            }
            let region = self.list[a].start..grow(self.list[b - 1].end());
            let delta = text.len() as isize - self.text.len() as isize;
            for chunk in &mut self.list[b..] {
                chunk.start = chunk.start.checked_add_signed(delta).unwrap();
            }
            let first = std::mem::take(&mut self.list[a].resource);
            let pieces = split(text, region, first);
            self.list.splice(a..b, pieces);
        }
        self.text.clear();
        self.text.push_str(text);
        self.laid_out = None;
        self.measured.clear();
    }
    /// Size at the requested width: the widest chunk and the sum of their heights.
    pub fn measure(
        &mut self,
        fonts: &mut TextSystem,
        families: &FamilyMap,
        request: TextRequest<'_>,
        generation: u64,
    ) -> Result<[f32; 2], UiError> {
        self.sync(request.text, request.revision);
        let key = (request.revision, text_width(request.width), generation);
        if let Some((_, size)) = self.measured.iter().find(|(k, _)| *k == key) {
            return Ok(*size);
        }
        let mut size = [0f32, 0f32];
        for chunk in &mut self.list {
            let [w, h] = chunk.resource.measure_at(
                fonts,
                families,
                chunk_request(request, chunk.start..chunk.end()),
                chunk.start,
            )?;
            size = [size[0].max(w), size[1] + h];
        }
        if self.measured.len() == 4 {
            self.measured.remove(0);
        }
        self.measured.push((key, size));
        Ok(size)
    }
    /// Lays out every chunk at the request width and stacks them. Unchanged chunks
    /// return their retained snapshots; an unchanged request does no work.
    pub fn layout(
        &mut self,
        fonts: &mut TextSystem,
        families: &FamilyMap,
        request: TextRequest<'_>,
        generation: u64,
    ) -> Result<(), UiError> {
        self.sync(request.text, request.revision);
        let key = (request.revision, text_width(request.width), generation);
        if self.laid_out == Some(key) {
            return Ok(());
        }
        let mut top = 0.;
        for chunk in &mut self.list {
            let layout = chunk.resource.layout_at(
                fonts,
                families,
                chunk_request(request, chunk.start..chunk.end()),
                chunk.start,
            )?;
            chunk.top = top;
            top += layout.size()[1];
            chunk.layout = Some(layout);
        }
        self.laid_out = Some(key);
        Ok(())
    }
    /// Prepares glyph geometry for chunks whose layout or raster density changed.
    pub fn prepare(
        &mut self,
        painter: &mut astrelis::Painter,
        raster_scale: f32,
        origin: [f32; 2],
        clip: crate::Bounds,
    ) -> Result<(), UiError> {
        let mut ink: Option<Rect> = None;
        for chunk in &mut self.list {
            let layout = chunk.layout.as_ref().ok_or(UiError::InvalidGeometry)?;
            let resource = &mut chunk.resource;
            let origin = [origin[0], origin[1] + chunk.top];
            let phase = super::raster_phase(origin, raster_scale);
            let phase_changed = resource.raster_phase != Some(phase)
                && resource
                    .ink_bounds()
                    .is_some_and(|ink| super::ink_intersects(ink, origin, clip, raster_scale));
            if resource
                .prepared_layout
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, layout))
                || resource.raster_scale != raster_scale
                || phase_changed
                || resource.prepared.as_ref().is_none_or(|p| p.atlas_full())
            {
                resource.prepared = None;
                resource.prepared = Some(
                    painter.prepare_text(layout, super::raster_options(origin, raster_scale))?,
                );
                resource.prepared_layout = Some(layout.clone());
                resource.raster_scale = raster_scale;
                resource.raster_phase = Some(phase);
            }
            if let Some(i) = resource.prepared.as_ref().and_then(|p| p.ink_bounds()) {
                let i = Rect::new(i.x, i.y + chunk.top, i.width, i.height);
                ink = Some(ink.map_or(i, |u| {
                    let x = u.x.min(i.x);
                    let y = u.y.min(i.y);
                    Rect::new(
                        x,
                        y,
                        (u.x + u.width).max(i.x + i.width) - x,
                        (u.y + u.height).max(i.y + i.height) - y,
                    )
                }));
            }
        }
        self.ink = ink;
        Ok(())
    }
    /// Whether every chunk has geometry prepared from its current layout.
    pub fn prepared(&self) -> bool {
        self.list.iter().all(|c| {
            c.resource.prepared.is_some()
                && c.layout
                    .as_ref()
                    .zip(c.resource.prepared_layout.as_ref())
                    .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
        })
    }
    /// Whether any retained chunk needs preparation again after atlas space frees.
    pub fn atlas_full(&self) -> bool {
        self.list
            .iter()
            .any(|c| c.resource.prepared.as_ref().is_some_and(|p| p.atlas_full()))
    }
    /// Chunk holding a whole-text offset; a chunk's end precedes its line break.
    fn at_offset(&self, offset: usize) -> Option<usize> {
        let index = self.list.partition_point(|c| c.end() < offset);
        self.list
            .get(index)
            .filter(|c| c.start <= offset)
            .map(|_| index)
    }
    fn local(&self, index: usize, position: TextPosition) -> TextPosition {
        TextPosition::new(position.byte_offset - self.list[index].start).affinity(position.affinity)
    }
    fn global(&self, index: usize, position: TextPosition) -> TextPosition {
        TextPosition::new(position.byte_offset + self.list[index].start).affinity(position.affinity)
    }
    fn layout_of(&self, index: usize) -> Option<&TextLayout> {
        self.list[index].layout.as_deref()
    }
    pub fn hit_test(&self, point: [f32; 2]) -> Option<TextPosition> {
        let index = self
            .list
            .partition_point(|c| c.top <= point[1])
            .saturating_sub(1);
        let chunk = &self.list[index];
        let local = self
            .layout_of(index)?
            .hit_test([point[0], point[1] - chunk.top])?;
        Some(self.global(index, local))
    }
    pub fn caret(&self, position: TextPosition) -> Option<TextCaret> {
        let index = self.at_offset(position.byte_offset)?;
        let caret = self.layout_of(index)?.caret(self.local(index, position))?;
        Some(TextCaret {
            position: self.global(index, caret.position),
            origin: [caret.origin[0], caret.origin[1] + self.list[index].top],
            ..caret
        })
    }
    /// Visual neighbor. Past a chunk's first or last line it crosses the paragraph
    /// break as a single layout does: in logical order, entering the adjacent line
    /// at the edge stop its own direction implies.
    pub fn neighbor(&self, position: TextPosition, right: bool) -> Option<TextPosition> {
        let index = self.at_offset(position.byte_offset)?;
        let layout = self.layout_of(index)?;
        let local = self.local(index, position);
        if let Some(next) = layout.visual_neighbor(local, right) {
            return Some(self.global(index, next));
        }
        let caret = layout.caret(local)?;
        let forward = right != layout.lines()[caret.line_index].rtl;
        let target = if forward {
            index + 1
        } else {
            index.checked_sub(1)?
        };
        let lines = self.list.get(target)?.layout.as_ref()?.lines();
        let line = if forward { lines.first() } else { lines.last() }?;
        // Hit testing beyond a line's edge clamps to that edge's stop.
        let x = if forward == line.rtl { 1e9 } else { -1e9 };
        let stop = self
            .layout_of(target)?
            .hit_test([x, line.top + line.height / 2.])?;
        Some(self.global(target, stop))
    }
    /// Selection rectangles in whole-text units, each with the chunk it lies in.
    pub fn selection_rects(&self, range: Range<usize>) -> Result<Vec<(Rect, usize)>, UiError> {
        let mut rects = Vec::new();
        let first = self.list.partition_point(|c| c.end() < range.start);
        for (index, chunk) in self.list.iter().enumerate().skip(first) {
            if chunk.start > range.end {
                break;
            }
            let start = range.start.max(chunk.start);
            let end = range.end.min(chunk.end());
            if start >= end {
                continue;
            }
            let layout = self.layout_of(index).ok_or(UiError::InvalidGeometry)?;
            for r in layout.selection_rects(start - chunk.start..end - chunk.start)? {
                rects.push((Rect::new(r.x, r.y + chunk.top, r.width, r.height), index));
            }
        }
        Ok(rects)
    }
    /// Prepared chunks whose line boxes intersect `top..bottom`, with their offsets.
    pub fn visible(&self, top: f32, bottom: f32) -> impl Iterator<Item = (&PreparedText, f32)> {
        let first = self.list.partition_point(|c| c.top + c.height() <= top);
        self.list[first..]
            .iter()
            .take_while(move |c| c.top < bottom)
            .filter_map(|c| c.resource.prepared.as_ref().map(|p| (p, c.top)))
    }
    pub fn chunk(&self, index: usize) -> Option<(&PreparedText, f32)> {
        let chunk = self.list.get(index)?;
        Some((chunk.resource.prepared.as_ref()?, chunk.top))
    }
    #[cfg(test)]
    pub fn ranges(&self) -> Vec<Range<usize>> {
        self.list.iter().map(|c| c.start..c.end()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn check(chunks: &Chunks, text: &str) {
        let ranges = chunks.ranges();
        assert_eq!(ranges.first().unwrap().start, 0);
        assert_eq!(ranges.last().unwrap().end, text.len());
        for pair in ranges.windows(2) {
            assert_eq!(&text[pair[0].end..pair[1].start], "\n");
        }
        for range in &ranges {
            assert!(
                paragraphs(&text[range.clone()]) <= 2 * PARAGRAPHS,
                "{range:?}"
            );
        }
    }
    #[test]
    fn edits_rechunk_only_the_touched_region_and_keep_paragraph_boundaries() {
        let mut text: String = (0..1000).map(|i| format!("paragraph {i}\n")).collect();
        let mut chunks = Chunks::default();
        let mut revision = 1;
        chunks.sync(&text, revision);
        check(&chunks, &text);
        let expected = 1001_usize.div_ceil(1001_usize.div_ceil(1001 / PARAGRAPHS));
        assert_eq!(chunks.list.len(), expected);
        // Typing inside one paragraph changes one chunk's length and shifts the rest.
        let before = chunks.ranges();
        let at = text.find("paragraph 500").unwrap();
        text.insert(at + 3, 'x');
        revision += 1;
        chunks.sync(&text, revision);
        check(&chunks, &text);
        let after = chunks.ranges();
        assert_eq!(before.len(), after.len());
        let changed: Vec<_> = before
            .iter()
            .zip(&after)
            .filter(|(a, b)| a.len() != b.len())
            .collect();
        assert_eq!(changed.len(), 1);
        assert!(changed[0].0.contains(&at));
        // Joining two chunks by deleting the break between them, inserting many
        // lines, and deleting most of the text all keep a valid partition.
        text.remove(after[3].end);
        revision += 1;
        chunks.sync(&text, revision);
        check(&chunks, &text);
        let lines: String = (0..500).map(|i| format!("new {i}\n")).collect();
        text.insert_str(text.len() / 3, &lines);
        revision += 1;
        chunks.sync(&text, revision);
        check(&chunks, &text);
        text.replace_range(10..text.len() - 10, "");
        revision += 1;
        chunks.sync(&text, revision);
        check(&chunks, &text);
        assert_eq!(chunks.list.len(), 1);
        // An unchanged revision does no work; a font-only revision keeps chunks.
        chunks.sync("ignored", revision);
        check(&chunks, &text);
        revision += 1;
        chunks.sync(&text, revision);
        check(&chunks, &text);
    }
    #[test]
    fn chunking_has_hysteresis() {
        let long = "\n".repeat(2 * PARAGRAPHS);
        assert!(!wanted(&long[..2 * PARAGRAPHS - 2], false));
        assert!(wanted(&long[..2 * PARAGRAPHS - 1], false));
        assert!(wanted(&long[..PARAGRAPHS], true));
        assert!(!wanted(&long[..PARAGRAPHS / 2 - 2], true));
    }
}
