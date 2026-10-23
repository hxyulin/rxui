use super::{TextError, TextLayout};
use crate::Rect;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Logical attachment at a wrap or bidi boundary with two visual caret positions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TextAffinity {
    /// Attach to the preceding logical grapheme (the earlier line at a soft wrap).
    Upstream,
    /// Attach to the following logical grapheme (the later line at a soft wrap).
    #[default]
    Downstream,
}
/// A whole-buffer UTF-8 byte offset and logical caret affinity.
///
/// Geometry accepts extended-grapheme boundaries only. CRLF/LFCR paragraph breaks
/// are indivisible. Positions belong to the snapshot whose text they index;
/// the application must remap positions after editing. Glyph IDs are not positions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextPosition {
    /// Byte offset in TextLayout::text(), including explicit line endings.
    pub byte_offset: usize,
    /// Which neighboring logical grapheme determines the visual position.
    pub affinity: TextAffinity,
}
impl TextPosition {
    /// Creates a downstream position. Query methods validate the offset.
    pub const fn new(byte_offset: usize) -> Self {
        Self {
            byte_offset,
            affinity: TextAffinity::Downstream,
        }
    }
    /// Selects logical affinity; endpoints with only one caret fall back to that caret.
    pub const fn affinity(mut self, affinity: TextAffinity) -> Self {
        self.affinity = affinity;
        self
    }
}
/// A caret in untransformed layout units. Drawing thickness belongs to the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextCaret {
    /// Effective position; affinity can be canonicalized at a document/paragraph edge.
    pub position: TextPosition,
    /// Index in TextLayout::lines().
    pub line_index: usize,
    /// [X, line-box top], in local units, X right/Y down.
    pub origin: [f32; 2],
    /// Line-box height, independent of glyph ink and raster density.
    pub height: f32,
}
#[derive(Debug, PartialEq)]
struct Cell {
    source: Range<usize>,
    edges: [f32; 2],
    line: usize,
}
#[derive(Debug, Default, PartialEq)]
struct Line {
    cells: Range<usize>,
    stops: Range<usize>,
}
#[derive(Debug, PartialEq)]
pub(super) struct Interaction {
    boundaries: Vec<usize>,
    cells: Vec<Cell>,
    visual: Vec<usize>,
    stops: Vec<usize>,
    lines: Vec<Line>,
}
#[derive(Clone)]
struct Group {
    begin: usize,
    end: usize,
    left: f32,
    right: f32,
    rtl: bool,
}
impl Interaction {
    fn build(layout: &TextLayout) -> Result<Self, TextError> {
        let dense = layout
            .text()
            .bytes()
            .all(|byte| (b' '..=b'~').contains(&byte));
        Self::build_index(layout, dense)
    }

    fn build_index(layout: &TextLayout, dense: bool) -> Result<Self, TextError> {
        // Printable ASCII has exactly one grapheme per byte, independently of
        // shaping/ligatures. Cluster endpoints are direct boundary indices.
        let mut boundaries: Vec<_> = if dense {
            (0..=layout.text().len()).collect()
        } else {
            layout
                .text()
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([layout.text().len()])
                .collect()
        };
        // Preserve the backend's paired LFCR paragraph break as one boundary too.
        let breaks: Vec<_> = super::cosmic::LineIter::new(layout.text())
            .filter_map(|(range, ending)| {
                (ending.as_str().len() > 1).then_some(range.end..range.end + ending.as_str().len())
            })
            .collect();
        let mut b = 0;
        boundaries.retain(|&i| {
            while b < breaks.len() && breaks[b].end <= i {
                b += 1;
            }
            b == breaks.len() || i <= breaks[b].start || i >= breaks[b].end
        });
        let mut cells = Vec::new();
        let mut scratch = Vec::<Group>::new();
        let mut combined = Vec::<Group>::new();
        for (line_index, line) in layout.lines.iter().enumerate() {
            scratch.clear();
            combined.clear();
            for glyph in &layout.glyphs[line.glyph_range.clone()] {
                if glyph.cluster.is_empty() {
                    continue;
                }
                let (begin, end) = if dense {
                    (glyph.cluster.start, glyph.cluster.end)
                } else {
                    (
                        boundaries
                            .partition_point(|&b| b <= glyph.cluster.start)
                            .saturating_sub(1),
                        boundaries.partition_point(|&b| b < glyph.cluster.end),
                    )
                };
                if begin >= end {
                    continue;
                }
                let a = glyph.advance_origin[0];
                let z = a + glyph.advance;
                scratch.push(Group {
                    begin,
                    end,
                    left: a.min(z),
                    right: a.max(z),
                    rtl: glyph.bidi_level % 2 == 1,
                });
            }
            scratch.sort_unstable_by_key(|g| (g.begin, g.end));
            for g in scratch.iter().cloned() {
                if let Some(last) = combined.last_mut()
                    && g.begin < last.end
                {
                    last.end = last.end.max(g.end);
                    last.left = last.left.min(g.left);
                    last.right = last.right.max(g.right);
                } else {
                    combined.push(g);
                }
            }
            let raw_begin = layout.line_sources[line_index].start;
            let begin = boundaries[boundaries
                .partition_point(|&b| b <= raw_begin)
                .saturating_sub(1)];
            let end = layout.line_sources[line_index].end;
            let mut offset = begin;
            let mut trailing = combined
                .first()
                .map_or(line.left, |g| if g.rtl { g.right } else { g.left });
            // Include omitted wrap whitespace/controls as zero-advance cells. They
            // remain valid source boundaries without inventing selection ink.
            let fill = |from: usize, to: usize, x: f32, cells: &mut Vec<Cell>| {
                if from >= to {
                    return;
                }
                let a = boundaries.partition_point(|&b| b < from);
                let z = boundaries.partition_point(|&b| b <= to);
                for pair in boundaries[a..z].windows(2) {
                    cells.push(Cell {
                        source: pair[0]..pair[1],
                        edges: [x, x],
                        line: line_index,
                    });
                }
            };
            for g in &combined {
                fill(offset, boundaries[g.begin], trailing, &mut cells);
                let width = f64::from(g.right) - f64::from(g.left);
                if width > f64::from(f32::MAX) {
                    return Err(TextError::LayoutOverflow);
                }
                let count = g.end - g.begin;
                for i in 0..count {
                    let x = |i: usize| {
                        (f64::from(g.left)
                            + width * (if g.rtl { count - i } else { i }) as f64 / count as f64)
                            as f32
                    };
                    cells.push(Cell {
                        source: boundaries[g.begin + i]..boundaries[g.begin + i + 1],
                        edges: [x(i), x(i + 1)],
                        line: line_index,
                    });
                }
                trailing = if g.rtl { g.left } else { g.right };
                offset = boundaries[g.end];
            }
            fill(offset, end, trailing, &mut cells);
            if begin == end && combined.is_empty() {
                cells.push(Cell {
                    source: begin..end,
                    edges: [line.left; 2],
                    line: line_index,
                });
            }
        }
        cells.sort_unstable_by_key(|c| (c.source.start, c.source.end, c.line));
        let mut per_line: Vec<Vec<usize>> = (0..layout.lines.len()).map(|_| Vec::new()).collect();
        for (i, c) in cells.iter().enumerate() {
            per_line[c.line].push(i);
        }
        let mut visual = Vec::with_capacity(cells.len());
        let mut stops = Vec::with_capacity(cells.len() * 2);
        let mut lines = Vec::with_capacity(per_line.len());
        for mut indices in per_line {
            indices.sort_unstable_by(|&a, &b| {
                cells[a].edges[0]
                    .min(cells[a].edges[1])
                    .total_cmp(&cells[b].edges[0].min(cells[b].edges[1]))
            });
            let a = visual.len();
            visual.extend_from_slice(&indices);
            let z = visual.len();
            let s = stops.len();
            stops.extend(indices.iter().flat_map(|&i| [i * 2, i * 2 + 1]));
            stops[s..].sort_unstable_by(|&a, &b| {
                let ca = &cells[a / 2];
                let cb = &cells[b / 2];
                ca.edges[a % 2]
                    .total_cmp(&cb.edges[b % 2])
                    .then_with(|| a.cmp(&b))
            });
            lines.push(Line {
                cells: a..z,
                stops: s..stops.len(),
            });
        }
        Ok(Self {
            boundaries,
            cells,
            visual,
            stops,
            lines,
        })
    }
    fn position(&self, stop: usize) -> TextPosition {
        let c = &self.cells[stop / 2];
        if stop.is_multiple_of(2) {
            TextPosition::new(c.source.start)
        } else {
            TextPosition::new(c.source.end).affinity(TextAffinity::Upstream)
        }
    }
    fn x(&self, stop: usize) -> f32 {
        self.cells[stop / 2].edges[stop % 2]
    }
    fn resolve(&self, p: TextPosition) -> Option<usize> {
        if self.boundaries.binary_search(&p.byte_offset).is_err() {
            return None;
        }
        let after = self
            .cells
            .partition_point(|c| c.source.start <= p.byte_offset);
        let downstream = after
            .checked_sub(1)
            .filter(|&i| self.cells[i].source.start == p.byte_offset)
            .map(|i| i * 2);
        let before = self.cells.partition_point(|c| c.source.end < p.byte_offset);
        let upstream = (before < self.cells.len()
            && self.cells[before].source.end == p.byte_offset)
            .then_some(before * 2 + 1);
        match p.affinity {
            TextAffinity::Downstream => downstream.or(upstream),
            TextAffinity::Upstream => upstream.or(downstream),
        }
    }
}
impl TextLayout {
    fn interaction(&self) -> Result<&Interaction, TextError> {
        self.interaction
            .get_or_init(|| Interaction::build(self).map(Box::new))
            .as_ref()
            .map(Box::as_ref)
            .map_err(Clone::clone)
    }
    /// Builds the optional CPU interaction index ahead of input handling.
    /// Ordinary layout/rendering never builds it. No shaping, rasterization or GPU
    /// upload occurs. Construction sorts clusters and per-line visual stops, with
    /// O(text + glyphs) retained storage. The index belongs to this immutable
    /// snapshot; initialization is safe to call concurrently. Viewport snapshots
    /// still segment the full source for boundary validation, but build cells only
    /// for retained lines.
    /// Ligature interiors divide the advance evenly between extended graphemes;
    /// font-specific GDEF ligature caret positions are not used in this milestone.
    pub fn prepare_interaction(&self) -> Result<(), TextError> {
        self.interaction().map(|_| ())
    }
    /// Retained index allocation bytes, excluding allocator/OnceLock overhead.
    /// Returns zero before first interaction. Ordinary labels pay no vector storage.
    pub fn interaction_bytes(&self) -> usize {
        match self.interaction.get() {
            Some(Ok(i)) => {
                std::mem::size_of::<Interaction>()
                    + i.boundaries.capacity() * std::mem::size_of::<usize>()
                    + i.cells.capacity() * std::mem::size_of::<Cell>()
                    + (i.visual.capacity() + i.stops.capacity()) * std::mem::size_of::<usize>()
                    + i.lines.capacity() * std::mem::size_of::<Line>()
            }
            _ => 0,
        }
    }
    /// Whether an offset is a valid extended-grapheme/atomic-paragraph-break boundary.
    pub fn is_text_boundary(&self, byte_offset: usize) -> bool {
        self.interaction()
            .is_ok_and(|i| i.boundaries.binary_search(&byte_offset).is_ok())
    }
    /// Next boundary after an in-bounds offset, or None at the document end.
    pub fn next_boundary(&self, byte_offset: usize) -> Option<usize> {
        if byte_offset > self.text.len() {
            return None;
        }
        let i = self.interaction().ok()?;
        i.boundaries
            .get(i.boundaries.partition_point(|&b| b <= byte_offset))
            .copied()
    }
    /// Previous boundary before an in-bounds offset, or None at the document start.
    pub fn previous_boundary(&self, byte_offset: usize) -> Option<usize> {
        if byte_offset > self.text.len() {
            return None;
        }
        let i = self.interaction().ok()?;
        i.boundaries
            .partition_point(|&b| b < byte_offset)
            .checked_sub(1)
            .map(|n| i.boundaries[n])
    }
    /// Maps a finite local-unit point to the nearest line and visual caret stop.
    /// Points outside line boxes clamp to the nearest line; X clamps to its end stops.
    /// Equally positioned stops at bidi boundaries have a deterministic affinity.
    /// Returns None for nonfinite input or overflowing interaction geometry.
    pub fn hit_test(&self, point: [f32; 2]) -> Option<TextPosition> {
        if !point.iter().all(|v| v.is_finite()) {
            return None;
        }
        let i = self.interaction().ok()?;
        let line = self
            .lines
            .partition_point(|l| l.top + l.height <= point[1])
            .min(self.lines.len().checked_sub(1)?);
        let stops = &i.stops[i.lines[line].stops.clone()];
        let right = stops.partition_point(|&s| i.x(s) < point[0]);
        let stop = if right == 0 {
            *stops.first()?
        } else if right == stops.len()
            || f64::from(point[0]) - f64::from(i.x(stops[right - 1]))
                < f64::from(i.x(stops[right])) - f64::from(point[0])
        {
            stops[right - 1]
        } else {
            stops[right]
        };
        Some(i.position(stop))
    }
    /// Resolves an offset/affinity to its line-box caret in local units.
    /// Invalid/nonboundary offsets return None. If only one side exists at an edge,
    /// falls back to that side and returns its effective affinity.
    pub fn caret(&self, position: TextPosition) -> Option<TextCaret> {
        let i = self.interaction().ok()?;
        let s = i.resolve(position)?;
        let line = i.cells[s / 2].line;
        let l = &self.lines[line];
        Some(TextCaret {
            position: i.position(s),
            line_index: line,
            origin: [i.x(s), l.top],
            height: l.height,
        })
    }
    /// Neighboring visually distinct caret to the left/right, including across lines.
    /// Same-X duplicate affinities are skipped; paragraph direction controls crossing
    /// a line edge. Vertical movement can use hit_test at the desired X/adjacent line.
    pub fn visual_neighbor(&self, position: TextPosition, to_right: bool) -> Option<TextPosition> {
        let i = self.interaction().ok()?;
        let s = i.resolve(position)?;
        let line = i.cells[s / 2].line;
        let stops = &i.stops[i.lines[line].stops.clone()];
        let x = i.x(s);
        let local = if to_right {
            stops.get(stops.partition_point(|&s| i.x(s) <= x)).copied()
        } else {
            stops
                .partition_point(|&s| i.x(s) < x)
                .checked_sub(1)
                .map(|n| stops[n])
        };
        if let Some(s) = local {
            return Some(i.position(s));
        }
        let forward = to_right != self.lines[line].rtl;
        let next = if forward {
            line.checked_add(1).filter(|&n| n < self.lines.len())?
        } else {
            line.checked_sub(1)?
        };
        let stops = &i.stops[i.lines[next].stops.clone()];
        // Cross paragraph boundaries at the target's logical start/end even when
        // its base direction differs from the paragraph we just left.
        let enter_on_right = forward == self.lines[next].rtl;
        Some(i.position(if enter_on_right {
            *stops.last()?
        } else {
            *stops.first()?
        }))
    }
    /// Visual advance rectangles for an ordered logical byte range.
    /// Both endpoints must be valid text boundaries, including for an empty range.
    /// Mixed bidi selections can yield disjoint rectangles on one line. Rectangles
    /// are ordered by line, then left-to-right, and use untransformed layout units.
    /// Breaks/zero-advance controls alone have no area; end-of-line indicators are
    /// application policy. No allocation occurs after the lazy index is warmed.
    pub fn selection_rects(&self, range: Range<usize>) -> Result<SelectionRects<'_>, TextError> {
        let i = self.interaction()?;
        if range.start > range.end
            || i.boundaries.binary_search(&range.start).is_err()
            || i.boundaries.binary_search(&range.end).is_err()
        {
            return Err(TextError::InvalidSelection);
        }
        let start = i.cells.partition_point(|c| c.source.end <= range.start);
        let end = i.cells.partition_point(|c| c.source.start < range.end);
        let (line, last) = if range.is_empty() || start >= end {
            (0, 0)
        } else {
            (i.cells[start].line, i.cells[end - 1].line + 1)
        };
        Ok(SelectionRects {
            layout: self,
            index: i,
            range,
            line,
            last,
            cursor: 0,
        })
    }
}
/// Borrowed, allocation-free iterator over a validated selection's visual rectangles.
#[derive(Debug)]
pub struct SelectionRects<'a> {
    layout: &'a TextLayout,
    index: &'a Interaction,
    range: Range<usize>,
    line: usize,
    last: usize,
    cursor: usize,
}
impl Iterator for SelectionRects<'_> {
    type Item = Rect;
    fn next(&mut self) -> Option<Rect> {
        while self.line < self.last {
            let cells = &self.index.visual[self.index.lines[self.line].cells.clone()];
            let mut span: Option<(f32, f32)> = None;
            while let Some(&index) = cells.get(self.cursor) {
                let c = &self.index.cells[index];
                let left = c.edges[0].min(c.edges[1]);
                let right = c.edges[0].max(c.edges[1]);
                let selected = c.source.start < self.range.end
                    && c.source.end > self.range.start
                    && right > left;
                if span.is_some() && !selected && right > left {
                    break;
                }
                self.cursor += 1;
                if selected {
                    span = Some(span.map_or((left, right), |(a, b)| (a.min(left), b.max(right))));
                }
            }
            if let Some((a, b)) = span {
                let line = &self.layout.lines[self.line];
                return Some(Rect::new(a, line.top, b - a, line.height));
            }
            self.line += 1;
            self.cursor = 0;
        }
        None
    }
}

#[cfg(test)]
mod ascii_tests {
    use super::*;
    use crate::{TextBuffer, TextStyle, TextSystem, TextWrap};
    #[test]
    fn dense_ascii_index_matches_general_index_including_ligatures_and_wraps() {
        let mut fonts = TextSystem::new();
        fonts
            .load_font(include_bytes!("../../tests/fonts/SourceSans3-Regular.otf"))
            .unwrap();
        for text in [
            "",
            "office ffi AV",
            "  spaces and punctuation !?  ",
            "abcdefghij",
        ] {
            for width in [None, Some(30.)] {
                let mut buffer = TextBuffer::new();
                buffer
                    .set_text(text, TextStyle::new().family("Source Sans 3"))
                    .unwrap();
                buffer.set_width(width).unwrap();
                buffer.set_wrap(TextWrap::WordOrGlyph);
                let layout = buffer.layout(&mut fonts).unwrap();
                assert_eq!(
                    Interaction::build(&layout).unwrap(),
                    Interaction::build_index(&layout, false).unwrap()
                );
            }
        }
    }
}
