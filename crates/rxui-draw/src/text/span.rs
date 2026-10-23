use super::{FontFamily, FontSlant, FontStretch, TextError, TextStyle, cosmic};
use std::{collections::BTreeSet, ops::Range};

/// Style overrides for one UTF-8 byte range of a [`super::TextBuffer`].
///
/// Unset fields inherit the buffer's [`TextStyle`]. Where spans overlap, later spans
/// override the fields they set, so a bold span and a colored span can share text.
/// Ranges must lie on `char` boundaries; they may cross line breaks.
///
/// Color is linear, straight RGBA. It replaces the draw color's RGB for these glyphs,
/// while the draw's alpha and opacity still multiply it; see [`super::TextDraw`].
/// Intrinsic color glyphs keep their artwork. Underline and strikethrough are drawn by
/// the text renderer in the span color, positioned from each font's own metrics.
/// Changing any span reshapes the affected paragraphs on the next evaluation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextSpan {
    /// Whole-buffer UTF-8 byte range.
    pub range: Range<usize>,
    /// Preferred family.
    pub family: Option<FontFamily>,
    /// Positive finite font size in local units.
    pub font_size: Option<f32>,
    /// Positive finite line height in local units. When only the font size is set,
    /// the buffer's line height scales with it. A line uses its tallest span.
    pub line_height: Option<f32>,
    /// OpenType/CSS weight in `1..=1000`.
    pub weight: Option<u16>,
    /// Requested slope.
    pub slant: Option<FontSlant>,
    /// Requested width class.
    pub stretch: Option<FontStretch>,
    /// Extra tracking in EM units.
    pub letter_spacing: Option<f32>,
    /// Linear, straight RGBA with alpha in `0..=1`.
    pub color: Option<[f32; 4]>,
    /// Draws a single underline.
    pub underline: Option<bool>,
    /// Draws a strikethrough line.
    pub strikethrough: Option<bool>,
}
impl TextSpan {
    /// A span over `range` that overrides nothing yet.
    pub fn new(range: Range<usize>) -> Self {
        Self {
            range,
            ..Self::default()
        }
    }
    /// Selects a family.
    pub fn family(mut self, family: impl Into<FontFamily>) -> Self {
        self.family = Some(family.into());
        self
    }
    /// Selects a font size.
    pub fn font_size(mut self, value: f32) -> Self {
        self.font_size = Some(value);
        self
    }
    /// Selects a line height.
    pub fn line_height(mut self, value: f32) -> Self {
        self.line_height = Some(value);
        self
    }
    /// Selects a weight.
    pub fn weight(mut self, value: u16) -> Self {
        self.weight = Some(value);
        self
    }
    /// Selects a slope.
    pub fn slant(mut self, value: FontSlant) -> Self {
        self.slant = Some(value);
        self
    }
    /// Selects a width class.
    pub fn stretch(mut self, value: FontStretch) -> Self {
        self.stretch = Some(value);
        self
    }
    /// Selects tracking in EM units.
    pub fn letter_spacing(mut self, value: f32) -> Self {
        self.letter_spacing = Some(value);
        self
    }
    /// Selects a linear, straight RGBA color.
    pub fn color(mut self, value: [f32; 4]) -> Self {
        self.color = Some(value);
        self
    }
    /// Underlines the range.
    pub fn underline(mut self) -> Self {
        self.underline = Some(true);
        self
    }
    /// Strikes through the range.
    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = Some(true);
        self
    }
    pub(crate) fn validate(&self, text: &str) -> Result<(), TextError> {
        let positive = |v: Option<f32>| v.is_none_or(|v| v.is_finite() && v > 0.);
        if self.range.start > self.range.end
            || !text.is_char_boundary(self.range.start)
            || !text.is_char_boundary(self.range.end)
            || !positive(self.font_size)
            || !positive(self.line_height)
            || self.weight.is_some_and(|w| !(1..=1000).contains(&w))
            || self.letter_spacing.is_some_and(|v| !v.is_finite())
            || self
                .color
                .is_some_and(|c| !c.iter().all(|v| v.is_finite()) || !(0. ..=1.).contains(&c[3]))
        {
            return Err(TextError::InvalidSpan);
        }
        Ok(())
    }
}

/// The merged style of a maximal byte range covered by the same spans.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Segment {
    pub range: Range<usize>,
    family: FontFamily,
    metrics: Option<(f32, f32)>,
    weight: u16,
    slant: FontSlant,
    stretch: FontStretch,
    letter_spacing: f32,
    pub color: Option<[f32; 4]>,
    underline: bool,
    strikethrough: bool,
}
impl Segment {
    /// Shaping attributes; `metadata` identifies the segment in laid-out glyphs.
    pub(crate) fn attrs<'a>(&'a self, base: &'a TextStyle, metadata: usize) -> cosmic::Attrs<'a> {
        let mut attrs = base
            .attrs()
            .family(self.family.backend())
            .weight(cosmic::Weight(self.weight))
            .style(self.slant.backend())
            .stretch(self.stretch)
            .letter_spacing(self.letter_spacing)
            .metadata(metadata);
        if let Some((size, line_height)) = self.metrics {
            attrs = attrs.metrics(cosmic::Metrics::new(size, line_height));
        }
        if self.underline {
            attrs = attrs.underline(cosmic::UnderlineStyle::Single);
        }
        if self.strikethrough {
            attrs = attrs.strikethrough();
        }
        attrs
    }
}

/// Splits spans into non-overlapping segments, later spans overriding earlier fields.
/// Ranges covered by no span are omitted; they use the buffer style.
pub(crate) fn segments(text_len: usize, base: &TextStyle, spans: &[TextSpan]) -> Vec<Segment> {
    // Sweep boundaries once. Each field tracks provider indices independently:
    // the last active provider setting that field wins, even for nested spans.
    let mut events = Vec::with_capacity(spans.len() * 2);
    for (index, span) in spans.iter().enumerate() {
        if !span.range.is_empty() {
            events.push((span.range.start, index, true));
            events.push((span.range.end, index, false));
        }
    }
    events.sort_unstable();
    let mut active: [BTreeSet<usize>; 10] = std::array::from_fn(|_| BTreeSet::new());
    let mut covering = 0usize;
    let mut cursor = 0;
    let mut result: Vec<Segment> = Vec::new();
    while cursor < events.len() {
        let start = events[cursor].0;
        while cursor < events.len() && events[cursor].0 == start {
            let (_, index, entering) = events[cursor];
            let span = &spans[index];
            let fields = [
                span.family.is_some(),
                span.font_size.is_some(),
                span.line_height.is_some(),
                span.weight.is_some(),
                span.slant.is_some(),
                span.stretch.is_some(),
                span.letter_spacing.is_some(),
                span.color.is_some(),
                span.underline.is_some(),
                span.strikethrough.is_some(),
            ];
            for (set, present) in active.iter_mut().zip(fields) {
                if present {
                    if entering {
                        set.insert(index);
                    } else {
                        set.remove(&index);
                    }
                }
            }
            if entering {
                covering += 1;
            } else {
                covering -= 1;
            }
            cursor += 1;
        }
        let end = events.get(cursor).map_or(text_len, |event| event.0);
        if covering == 0 || start == end {
            continue;
        }
        let range = start..end;
        let winner = |field: usize| active[field].last().map(|&index| &spans[index]);
        let mut segment = Segment {
            range: range.clone(),
            family: base.family.clone(),
            metrics: None,
            weight: base.weight,
            slant: base.slant,
            stretch: base.stretch,
            letter_spacing: base.letter_spacing,
            color: None,
            underline: false,
            strikethrough: false,
        };
        segment.family = winner(0)
            .and_then(|s| s.family.clone())
            .unwrap_or(segment.family);
        let size = winner(1).and_then(|s| s.font_size);
        let line_height = winner(2).and_then(|s| s.line_height);
        segment.weight = winner(3).and_then(|s| s.weight).unwrap_or(segment.weight);
        segment.slant = winner(4).and_then(|s| s.slant).unwrap_or(segment.slant);
        segment.stretch = winner(5).and_then(|s| s.stretch).unwrap_or(segment.stretch);
        segment.letter_spacing = winner(6)
            .and_then(|s| s.letter_spacing)
            .unwrap_or(segment.letter_spacing);
        segment.color = winner(7).and_then(|s| s.color);
        segment.underline = winner(8).and_then(|s| s.underline).unwrap_or(false);
        segment.strikethrough = winner(9).and_then(|s| s.strikethrough).unwrap_or(false);
        if size.is_some() || line_height.is_some() {
            let size = size.unwrap_or(base.font_size);
            let line_height = line_height.unwrap_or(base.line_height * size / base.font_size);
            segment.metrics = Some((size, line_height));
        }
        // Merge with an identical neighbour so shaping runs stay long.
        let same_style = |last: &Segment| {
            let mut candidate = segment.clone();
            candidate.range = last.range.clone();
            candidate == *last
        };
        if let Some(last) = result.last_mut()
            && last.range.end == range.start
            && same_style(last)
        {
            last.range.end = range.end;
        } else {
            result.push(segment);
        }
    }
    result
}

/// Attribute list for one paragraph at `paragraph` in the whole buffer.
pub(crate) fn paragraph_attrs(
    base: &TextStyle,
    defaults: &cosmic::Attrs<'_>,
    segments: &[Segment],
    paragraph: Range<usize>,
) -> cosmic::AttrsList {
    let mut list = cosmic::AttrsList::new(defaults);
    let first = segments.partition_point(|segment| segment.range.end <= paragraph.start);
    for (index, segment) in segments.iter().enumerate().skip(first) {
        if segment.range.start >= paragraph.end {
            break;
        }
        let start = segment.range.start.max(paragraph.start);
        let end = segment.range.end.min(paragraph.end);
        if start < end {
            list.add_span(
                start - paragraph.start..end - paragraph.start,
                &segment.attrs(base, index + 1),
            );
        }
    }
    list
}

#[cfg(test)]
mod sweep_tests {
    use super::*;
    fn reference_segments(text_len: usize, base: &TextStyle, spans: &[TextSpan]) -> Vec<Segment> {
        let mut bounds: Vec<usize> = spans
            .iter()
            .flat_map(|s| [s.range.start, s.range.end])
            .chain([0, text_len])
            .collect();
        bounds.sort_unstable();
        bounds.dedup();
        let mut result: Vec<Segment> = Vec::new();
        for pair in bounds.windows(2) {
            let range = pair[0]..pair[1];
            let mut covering = spans
                .iter()
                .filter(|s| s.range.start <= range.start && range.end <= s.range.end)
                .peekable();
            if covering.peek().is_none() {
                continue;
            }
            let mut segment = Segment {
                range: range.clone(),
                family: base.family.clone(),
                metrics: None,
                weight: base.weight,
                slant: base.slant,
                stretch: base.stretch,
                letter_spacing: base.letter_spacing,
                color: None,
                underline: false,
                strikethrough: false,
            };
            let (mut size, mut line_height) = (None, None);
            for span in covering {
                if let Some(family) = &span.family {
                    segment.family = family.clone();
                }
                size = span.font_size.or(size);
                line_height = span.line_height.or(line_height);
                segment.weight = span.weight.unwrap_or(segment.weight);
                segment.slant = span.slant.unwrap_or(segment.slant);
                segment.stretch = span.stretch.unwrap_or(segment.stretch);
                segment.letter_spacing = span.letter_spacing.unwrap_or(segment.letter_spacing);
                segment.color = span.color.or(segment.color);
                segment.underline = span.underline.unwrap_or(segment.underline);
                segment.strikethrough = span.strikethrough.unwrap_or(segment.strikethrough);
            }
            if size.is_some() || line_height.is_some() {
                let size = size.unwrap_or(base.font_size);
                let line_height = line_height.unwrap_or(base.line_height * size / base.font_size);
                segment.metrics = Some((size, line_height));
            }
            // Merge with an identical neighbour so shaping runs stay long.
            let same_style = |last: &Segment| {
                let mut candidate = segment.clone();
                candidate.range = last.range.clone();
                candidate == *last
            };
            if let Some(last) = result.last_mut()
                && last.range.end == range.start
                && same_style(last)
            {
                last.range.end = range.end;
            } else {
                result.push(segment);
            }
        }
        result
    }

    #[test]
    fn sweep_matches_reference_for_overlaps_and_field_precedence() {
        let base = TextStyle::new();
        let mut seed = 19u64;
        let mut random = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 32) as usize
        };
        for count in [0, 1, 2, 10, 100, 1000] {
            for _ in 0..12 {
                let mut spans = Vec::new();
                for _ in 0..count {
                    let a = random() % 101;
                    let b = random() % 101;
                    let mask = random();
                    let mut span = TextSpan::new(a.min(b)..a.max(b));
                    if mask & 1 != 0 {
                        span.family = Some(FontFamily::from("Menlo"));
                    }
                    if mask & 2 != 0 {
                        span.font_size = Some(10. + (random() % 20) as f32);
                    }
                    if mask & 4 != 0 {
                        span.line_height = Some(20. + (random() % 20) as f32);
                    }
                    if mask & 8 != 0 {
                        span.weight = Some(400 + (random() % 300) as u16);
                    }
                    if mask & 16 != 0 {
                        span.slant = Some(FontSlant::Italic);
                    }
                    if mask & 32 != 0 {
                        span.stretch = Some(FontStretch::Condensed);
                    }
                    if mask & 64 != 0 {
                        span.letter_spacing = Some((random() % 3) as f32);
                    }
                    if mask & 128 != 0 {
                        span.color = Some([0.2, (random() % 10) as f32 / 10., 0.5, 1.]);
                    }
                    if mask & 256 != 0 {
                        span.underline = Some(random() % 2 == 0);
                    }
                    if mask & 512 != 0 {
                        span.strikethrough = Some(random() % 2 == 0);
                    }
                    spans.push(span);
                }
                assert_eq!(
                    segments(100, &base, &spans),
                    reference_segments(100, &base, &spans)
                );
            }
        }
    }
}
