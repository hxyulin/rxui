use super::*;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;
fn layout(text: &str, width: Option<f32>) -> Arc<TextLayout> {
    let mut fonts = TextSystem::new();
    fonts
        .load_font(include_bytes!("../../tests/fonts/SourceSans3-Regular.otf"))
        .unwrap();
    fonts
        .load_font(include_bytes!("../../tests/fonts/NotoSansArabic.ttf"))
        .unwrap();
    let mut buffer = TextBuffer::new();
    buffer
        .set_text(
            text,
            TextStyle::new()
                .family("Source Sans 3")
                .font_size(20.)
                .line_height(28.),
        )
        .unwrap();
    buffer.set_width(width).unwrap();
    buffer.layout(&mut fonts).unwrap()
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.002, "{a} != {b}");
}
#[test]
fn lazy_index_survives_owners_and_has_all_unicode_boundaries() {
    let text = "office e\u{301} 👩‍👩‍👧‍👦 🇭🇰 العربية";
    let l = layout(text, None);
    assert_eq!(l.interaction_bytes(), 0);
    l.prepare_interaction().unwrap();
    let bytes = l.interaction_bytes();
    assert!(bytes > 0);
    let boundaries: Vec<_> = text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    for i in 0..=text.len() {
        assert_eq!(l.is_text_boundary(i), boundaries.contains(&i));
        if boundaries.contains(&i) {
            assert!(l.caret(TextPosition::new(i)).is_some(), "missing {i}");
        } else {
            assert!(l.caret(TextPosition::new(i)).is_none());
        }
    }
    for (i, &byte) in boundaries.iter().enumerate() {
        assert_eq!(
            l.previous_boundary(byte),
            i.checked_sub(1).map(|n| boundaries[n])
        );
        assert_eq!(l.next_boundary(byte), boundaries.get(i + 1).copied());
    }
    for _ in 0..50 {
        let _ = l.hit_test([31., 10.]);
        let _ = l.selection_rects(0..text.len()).unwrap().count();
    }
    assert_eq!(bytes, l.interaction_bytes());
    assert_eq!(l.text(), text);
}
#[test]
fn ligature_advance_is_subdivided_once_and_combining_marks_are_atomic() {
    let l = layout("office a\u{301}", None);
    let glyph = l
        .glyphs()
        .iter()
        .find(|g| g.cluster.end - g.cluster.start > 1 && l.text()[g.cluster.clone()].is_ascii())
        .expect("font must form a ligature");
    let range = glyph.cluster.clone();
    let n = range.len();
    let mut xs = Vec::new();
    for b in range.clone().chain([range.end]) {
        xs.push(l.caret(TextPosition::new(b)).unwrap().origin[0]);
    }
    for pair in xs.windows(2) {
        near(pair[1] - pair[0], glyph.advance / n as f32);
    }
    let rects: Vec<_> = l.selection_rects(range.clone()).unwrap().collect();
    near(rects.iter().map(|r| r.width).sum(), glyph.advance);
    let a = l.text().find("a").unwrap();
    assert!(l.caret(TextPosition::new(a + 1)).is_none());
    assert!(l.selection_rects(a..a + 1).is_err());
    for k in 0..n {
        let x = (xs[k] + xs[k + 1]) * 0.5;
        let p = l.hit_test([x, 10.]).unwrap();
        assert!(p.byte_offset == range.start + k || p.byte_offset == range.start + k + 1);
    }
}
#[test]
fn wrap_affinity_resolves_both_lines_and_hits_round_trip_geometry() {
    let l = layout(
        "A long paragraph with office ligatures and words to wrap.",
        Some(90.),
    );
    assert!(l.lines().len() > 2);
    let mut wraps = 0;
    for b in 0..=l.text().len() {
        if !l.is_text_boundary(b) {
            continue;
        }
        let a = l
            .caret(TextPosition::new(b).affinity(TextAffinity::Upstream))
            .unwrap();
        let z = l.caret(TextPosition::new(b)).unwrap();
        if a.line_index != z.line_index {
            assert_eq!(a.line_index + 1, z.line_index);
            wraps += 1;
        }
        for c in [a, z] {
            let p = l
                .hit_test([c.origin[0], c.origin[1] + c.height * 0.5])
                .unwrap();
            let hit = l.caret(p).unwrap();
            assert_eq!(hit.line_index, c.line_index);
            near(hit.origin[0], c.origin[0]);
        }
    }
    assert!(wraps > 0);
    let all: Vec<_> = l.selection_rects(0..l.text().len()).unwrap().collect();
    assert!(all.len() >= l.lines().len());
    assert!(all.windows(2).all(|r| r[0].y <= r[1].y));
}
#[test]
fn rtl_ligatures_and_mixed_bidi_selection_follow_visual_positions() {
    let l = layout("abc العربية xyz", None);
    let start = l.text().find('ا').unwrap();
    let end = start + "العربية".len();
    let a = l.caret(TextPosition::new(start)).unwrap();
    let z = l
        .caret(TextPosition::new(end).affinity(TextAffinity::Upstream))
        .unwrap();
    assert!(a.origin[0] > z.origin[0]);
    let selected: Vec<_> = l.selection_rects(0..start + 2).unwrap().collect();
    assert!(selected.len() >= 2, "{selected:?}");
    for byte in l.text()[start..end]
        .grapheme_indices(true)
        .map(|(i, _)| start + i)
    {
        let a = l.caret(TextPosition::new(byte)).unwrap();
        let end = l.next_boundary(byte).unwrap();
        let z = l
            .caret(TextPosition::new(end).affinity(TextAffinity::Upstream))
            .unwrap();
        assert!(a.origin[0] >= z.origin[0]);
        let x = (a.origin[0] + z.origin[0]) * 0.5;
        let hit = l.hit_test([x, a.origin[1] + a.height * 0.5]).unwrap();
        assert!(
            hit.byte_offset == byte || hit.byte_offset == end,
            "byte={byte} hit={hit:?}"
        );
        let rects: Vec<_> = l.selection_rects(byte..end).unwrap().collect();
        near(
            rects.iter().map(|r| r.width).sum(),
            a.origin[0] - z.origin[0],
        );
    }
    let next = l.visual_neighbor(TextPosition::new(start), false).unwrap();
    assert!(l.caret(next).unwrap().origin[0] < a.origin[0]);
}
#[test]
fn blank_lines_alignment_and_paired_breaks_need_no_fonts() {
    for (align, anchor) in [
        (TextAlign::Left, 0.),
        (TextAlign::Center, 50.),
        (TextAlign::Right, 100.),
    ] {
        let mut fonts = TextSystem::new();
        let mut b = TextBuffer::new();
        b.set_text("\r\n\n\r", TextStyle::new()).unwrap();
        b.set_width(Some(100.)).unwrap();
        b.set_align(align);
        let l = b.layout(&mut fonts).unwrap();
        assert_eq!(l.lines().len(), 3);
        for (line, byte) in [0, 2, 4].into_iter().enumerate() {
            let c = l.caret(TextPosition::new(byte)).unwrap();
            assert_eq!(c.line_index, line);
            near(c.origin[0], anchor);
            assert_eq!(
                l.hit_test([500., c.origin[1] + c.height / 2.])
                    .unwrap()
                    .byte_offset,
                byte
            );
        }
        assert!(!l.is_text_boundary(1));
        assert!(!l.is_text_boundary(3));
        assert_eq!(l.selection_rects(0..4).unwrap().count(), 0);
    }
    let l = layout("a\r\nb\n\rc", None);
    assert!(!l.is_text_boundary(2));
    assert!(!l.is_text_boundary(5));
    assert_eq!(l.next_boundary(1), Some(3));
    assert_eq!(l.previous_boundary(6), Some(4));
    assert_eq!(l.selection_rects(1..3).unwrap().count(), 0);
}
#[test]
fn invalid_queries_and_empty_ranges_are_explicit_and_snapshot_reuse_is_unchanged() {
    let l = layout("e\u{301} abc", Some(100.));
    assert!(l.hit_test([f32::NAN, 0.]).is_none());
    assert!(l.hit_test([0., f32::INFINITY]).is_none());
    assert!(l.caret(TextPosition::new(usize::MAX)).is_none());
    assert!(
        l.selection_rects(std::ops::Range { start: 3, end: 1 })
            .is_err()
    );
    assert!(l.selection_rects(1..1).is_err());
    assert!(l.selection_rects(0..usize::MAX).is_err());
    assert_eq!(l.selection_rects(0..0).unwrap().count(), 0);
    let mut fonts = TextSystem::new();
    fonts
        .load_font(include_bytes!("../../tests/fonts/SourceSans3-Regular.otf"))
        .unwrap();
    let mut b = TextBuffer::new();
    b.set_text("unchanged", TextStyle::new().family("Source Sans 3"))
        .unwrap();
    let a = b.layout(&mut fonts).unwrap();
    a.prepare_interaction().unwrap();
    assert!(Arc::ptr_eq(&a, &b.layout(&mut fonts).unwrap()));
    b.set_text("different", b.style().clone()).unwrap();
    let z = b.layout(&mut fonts).unwrap();
    assert_eq!(z.interaction_bytes(), 0);
    assert_eq!(a.text(), "unchanged");
    assert!(a.caret(TextPosition::new(5)).is_some());
}

#[test]
fn narrow_wrapping_and_control_clusters_have_finite_complete_caret_geometry() {
    for text in [
        "ffi e\u{301} 👩‍👩‍👧‍👦 🇭🇰",
        "العربية abc مرحبا ffi 123 العربية",
        "a\t\u{200e}b\u{200f}\u{202a}office\u{202c} c\n\n",
        "مرحبا\r\nhello\n\rالعربية",
    ] {
        for width in [0., 1., 8., 36., 90., 180.] {
            let l = layout(text, Some(width));
            l.prepare_interaction().unwrap();
            for byte in text
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([text.len()])
            {
                if !l.is_text_boundary(byte) {
                    continue;
                }
                for affinity in [TextAffinity::Upstream, TextAffinity::Downstream] {
                    let c = l
                        .caret(TextPosition::new(byte).affinity(affinity))
                        .unwrap_or_else(|| {
                            panic!("missing {byte} {affinity:?} width={width} text={text}")
                        });
                    assert!(c.origin.iter().all(|v| v.is_finite()));
                    assert!(c.height.is_finite());
                }
            }
            for r in l.selection_rects(0..text.len()).unwrap() {
                assert!(r.width.is_finite() && r.width > 0.);
                assert!(r.height > 0.);
            }
        }
    }
}

#[test]
fn visual_navigation_crosses_paragraph_direction_changes_at_logical_edges() {
    let text = "abc\nالعربية\nxyz";
    let l = layout(text, None);
    let rtl_start = text.find('ا').unwrap();
    let rtl_end = text.find("\nxyz").unwrap();
    let p = l
        .visual_neighbor(TextPosition::new(3).affinity(TextAffinity::Upstream), true)
        .unwrap();
    assert_eq!(p.byte_offset, rtl_start);
    assert_eq!(l.caret(p).unwrap().line_index, 1);
    let p = l
        .visual_neighbor(TextPosition::new(rtl_start), true)
        .unwrap();
    assert_eq!(p.byte_offset, 3);
    assert_eq!(l.caret(p).unwrap().line_index, 0);
    let p = l
        .visual_neighbor(
            TextPosition::new(rtl_end).affinity(TextAffinity::Upstream),
            false,
        )
        .unwrap();
    assert_eq!(p.byte_offset, rtl_end + 1);
    assert_eq!(l.caret(p).unwrap().line_index, 2);
    let p = l
        .visual_neighbor(TextPosition::new(rtl_end + 1), false)
        .unwrap();
    assert_eq!(p.byte_offset, rtl_end);
    assert_eq!(l.caret(p).unwrap().line_index, 1);
}
