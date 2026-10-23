use super::*;
use std::sync::Arc;

const LATIN: &[u8] = include_bytes!("../../tests/fonts/SourceSans3-Regular.otf");
const ARABIC: &[u8] = include_bytes!("../../tests/fonts/NotoSansArabic.ttf");

fn system() -> TextSystem {
    let mut system = TextSystem::new();
    system.load_font(LATIN).unwrap();
    system.load_font(ARABIC).unwrap();
    system
}
fn style() -> TextStyle {
    TextStyle::new()
        .family("Source Sans 3")
        .font_size(20.)
        .line_height(28.)
}
fn layout(system: &mut TextSystem, text: &str, style: TextStyle) -> Arc<TextLayout> {
    let mut buffer = TextBuffer::new();
    buffer.set_text(text, style).unwrap();
    buffer.layout(system).unwrap()
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}
fn valid_clusters(layout: &TextLayout) {
    for (i, line) in layout.lines().iter().enumerate() {
        for glyph in &layout.glyphs()[line.glyph_range.clone()] {
            assert_eq!(glyph.line_index, i);
            assert!(glyph.font_index < layout.fonts().len());
            assert!(layout.text().get(glyph.cluster.clone()).is_some());
            assert!(glyph.cluster.start >= line.source.start);
            assert!(glyph.cluster.end <= line.source.end);
            assert!(glyph.position.iter().all(|v| v.is_finite()));
        }
    }
}

#[test]
fn loads_otf_and_variable_ttf_with_face_metadata() {
    let system = system();
    let fonts: Vec<_> = system.fonts().collect();
    assert_eq!(fonts.len(), 2);
    assert!(fonts[0].families.iter().any(|name| name == "Source Sans 3"));
    assert!(
        fonts[1]
            .families
            .iter()
            .any(|name| name == "Noto Sans Arabic")
    );
    for font in fonts {
        assert_eq!(font.face_index, 0);
        assert!(!font.postscript_name.is_empty());
        assert_eq!(system.font_info(font.id), Some(font));
    }
}

#[test]
fn invalid_font_loading_is_atomic() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    buffer.set_text("kept", style()).unwrap();
    let before = buffer.layout(&mut system).unwrap();
    let faces: Vec<_> = system.fonts().collect();
    for bytes in [&[][..], b"not a font", &LATIN[..32]] {
        assert_eq!(system.load_font(bytes), Err(TextError::InvalidFont));
        assert_eq!(system.fonts().collect::<Vec<_>>(), faces);
        assert!(Arc::ptr_eq(&before, &buffer.layout(&mut system).unwrap()));
    }
}

#[test]
fn shared_font_bytes_and_layout_survive_their_owners() {
    let bytes: Arc<[u8]> = Arc::from(LATIN);
    let retained = {
        let mut system = TextSystem::new();
        system.load_font_shared(bytes.clone()).unwrap();
        let snapshot = layout(&mut system, "snapshot", style());
        assert_eq!(snapshot.fonts()[0].data().as_ptr(), bytes.as_ptr());
        snapshot
    };
    drop(bytes);
    assert_eq!(retained.text(), "snapshot");
    assert_eq!(retained.fonts()[0].data(), LATIN);
    assert!(retained.glyphs().iter().all(|g| g.glyph_id != 0));
}

#[test]
fn accepts_a_two_face_opentype_collection() {
    // TTC table offsets are absolute. Package the real TTF as two faces sharing
    // the same tables; no parser or synthetic glyph data is used by the product.
    let mut collection = Vec::from(&b"ttcf\0\x01\0\0\0\0\0\x02\0\0\0\x14\0\0\0\x14"[..]);
    collection.extend_from_slice(ARABIC);
    let tables = u16::from_be_bytes(ARABIC[4..6].try_into().unwrap()) as usize;
    for table in 0..tables {
        let offset_pos = 20 + 12 + table * 16 + 8;
        let offset = u32::from_be_bytes(collection[offset_pos..offset_pos + 4].try_into().unwrap());
        collection[offset_pos..offset_pos + 4].copy_from_slice(&(offset + 20).to_be_bytes());
    }
    let mut system = TextSystem::new();
    let faces = system.load_font(&collection).unwrap();
    assert_eq!(faces.len(), 2);
    assert_eq!((faces[0].face_index, faces[1].face_index), (0, 1));
    assert_ne!(faces[0].id, faces[1].id);
    let result = layout(
        &mut system,
        "مرحبا",
        TextStyle::new().family("Noto Sans Arabic"),
    );
    assert!(result.missing_glyphs().is_empty());
    assert_eq!(result.fonts()[0].data(), collection);
}

#[test]
fn empty_and_blank_paragraphs_do_not_require_fonts() {
    let mut system = TextSystem::new();
    let empty = layout(&mut system, "", style());
    assert_eq!(empty.size(), [0., 28.]);
    assert_eq!(empty.lines().len(), 1);
    let blank = layout(&mut system, "\r\n\n", style());
    assert_eq!(blank.lines().len(), 3);
    assert_eq!(blank.size(), [0., 84.]);
    let mut buffer = TextBuffer::new();
    buffer.set_text("needs fonts", style()).unwrap();
    assert_eq!(buffer.layout(&mut system).unwrap_err(), TextError::NoFonts);
    system.load_font(LATIN).unwrap();
    assert!(!buffer.layout(&mut system).unwrap().glyphs().is_empty());
}

#[test]
fn kerning_and_ligature_features_affect_advanced_shaping() {
    let mut system = system();
    let kerned = layout(&mut system, "AV", style());
    let unkerned = layout(&mut system, "AV", style().feature(*b"kern", 0));
    assert!(kerned.size()[0] < unkerned.size()[0]);
    let joined = layout(&mut system, "ffi", style());
    let separate = layout(&mut system, "ffi", style().feature(*b"liga", 0));
    assert!(joined.glyphs().len() < separate.glyphs().len());
    assert!(
        joined.glyphs().iter().any(|g| g.cluster.len() > 1),
        "{:?}",
        joined.glyphs()
    );
    assert_eq!(separate.glyphs().len(), 3);
}

#[test]
fn combining_marks_preserve_clusters_and_position() {
    let mut system = system();
    let decomposed = layout(&mut system, "e\u{301}", style());
    let composed = layout(&mut system, "é", style());
    near(decomposed.size()[0], composed.size()[0]);
    assert!(decomposed.glyphs().iter().all(|g| g.cluster == (0..3)));
    assert!(decomposed.missing_glyphs().is_empty());
    valid_clusters(&decomposed);
}

#[test]
fn arabic_joins_and_mixed_bidi_uses_fallback() {
    let mut system = system();
    let joined = layout(&mut system, "بب", style());
    let isolated = layout(&mut system, "ب", style());
    assert!(
        joined
            .glyphs()
            .iter()
            .any(|g| g.glyph_id != isolated.glyphs()[0].glyph_id)
    );
    assert!(joined.glyphs().iter().all(|g| g.bidi_level % 2 == 1));
    assert!(joined.lines()[0].rtl);
    let mixed = layout(&mut system, "Hello العربية 123", style());
    assert_eq!(mixed.fonts().len(), 2);
    assert!(mixed.missing_glyphs().is_empty());
    assert!(mixed.glyphs().iter().any(|g| g.bidi_level % 2 == 0));
    assert!(mixed.glyphs().iter().any(|g| g.bidi_level % 2 == 1));
    valid_clusters(&mixed);
}

#[test]
fn reports_missing_source_clusters_without_rejecting_layout() {
    let mut system = system();
    let result = layout(&mut system, "a\u{10ffff}b", style());
    assert_eq!(result.missing_glyphs().len(), 1);
    assert_eq!(result.missing_glyphs()[0], 1..5);
    assert!(result.glyphs().iter().any(|g| g.glyph_id == 0));
    valid_clusters(&result);
}

#[test]
fn unchanged_evaluation_and_idempotent_setters_reuse_the_snapshot() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    buffer.set_text("reused ffi العربية", style()).unwrap();
    buffer.set_width(Some(240.)).unwrap();
    let before = buffer.layout(&mut system).unwrap();
    buffer.set_text(before.text(), style()).unwrap();
    buffer.set_style(style()).unwrap();
    buffer.set_width(Some(240.)).unwrap();
    buffer.set_wrap(TextWrap::WordOrGlyph);
    buffer.set_align(TextAlign::Start);
    assert!(Arc::ptr_eq(&before, &buffer.layout(&mut system).unwrap()));
    buffer.set_text("edited", style()).unwrap();
    let after = buffer.layout(&mut system).unwrap();
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(before.text(), "reused ffi العربية");
}

#[test]
fn paragraph_edits_insertions_and_deletions_match_fresh_layouts() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    buffer.set_width(Some(130.)).unwrap();
    let initial = "office AV\r\ne\u{301} العربية\ntrailing ffi\n";
    buffer.set_text(initial, style()).unwrap();
    let retained = buffer.layout(&mut system).unwrap();
    for text in [
        "office AV\r\nchanged العربية\ntrailing ffi\n",
        "office AV\r\ninserted\nchanged العربية\ntrailing ffi\n",
        "office AV\r\ntrailing ffi\n",
        "new first\r\ntrailing ffi\n",
        "new first\r\ntrailing ffi\nlast",
        "\r\n\n\r\r",
        "",
        initial,
    ] {
        buffer.set_text(text, style()).unwrap();
        let edited = buffer.layout(&mut system).unwrap();
        let mut fresh = TextBuffer::new();
        fresh.set_width(Some(130.)).unwrap();
        fresh.set_text(text, style()).unwrap();
        let fresh = fresh.layout(&mut system).unwrap();
        assert_eq!(edited.glyphs().len(), fresh.glyphs().len(), "{text:?}");
        for (i, (actual, expected)) in edited.glyphs().iter().zip(fresh.glyphs()).enumerate() {
            assert_eq!(actual, expected, "glyph {i}, text {text:?}");
        }
        assert_eq!(edited.lines(), fresh.lines(), "{text:?}");
        assert_eq!(edited.size(), fresh.size());
        assert_eq!(edited.missing_glyphs(), fresh.missing_glyphs());
        assert_eq!(edited.fonts().len(), fresh.fonts().len());
        valid_clusters(&edited);
    }
    assert_eq!(retained.text(), initial);
    valid_clusters(&retained);

    // Matching paragraph text must still invalidate font selection and layout
    // when style, wrapping, alignment, or the font database changes.
    let changed_style = style().weight(700).font_size(24.).line_height(32.);
    buffer.set_text(initial, changed_style.clone()).unwrap();
    buffer.set_width(Some(180.)).unwrap();
    buffer.set_align(TextAlign::Center);
    buffer.set_wrap(TextWrap::Word);
    let edited = buffer.layout(&mut system).unwrap();
    let mut fresh = TextBuffer::new();
    fresh.set_text(initial, changed_style.clone()).unwrap();
    fresh.set_width(Some(180.)).unwrap();
    fresh.set_align(TextAlign::Center);
    fresh.set_wrap(TextWrap::Word);
    let fresh = fresh.layout(&mut system).unwrap();
    assert_eq!(edited.glyphs(), fresh.glyphs());
    assert_eq!(edited.lines(), fresh.lines());
    system.load_font(LATIN).unwrap();
    let regenerated = buffer.layout(&mut system).unwrap();
    let mut fresh = TextBuffer::new();
    fresh.set_text(initial, changed_style).unwrap();
    fresh.set_width(Some(180.)).unwrap();
    fresh.set_align(TextAlign::Center);
    fresh.set_wrap(TextWrap::Word);
    let fresh = fresh.layout(&mut system).unwrap();
    assert_eq!(regenerated.glyphs(), fresh.glyphs());
    assert_eq!(regenerated.lines(), fresh.lines());
}

#[test]
fn wrapping_alignment_and_metrics_invalidate_layout() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    buffer.set_text("one two three four five", style()).unwrap();
    let wide = buffer.layout(&mut system).unwrap();
    buffer.set_width(Some(70.)).unwrap();
    let narrow = buffer.layout(&mut system).unwrap();
    assert!(narrow.lines().len() > wide.lines().len());
    assert!(narrow.lines().iter().all(|line| line.width <= 70.));
    buffer.set_wrap(TextWrap::None);
    assert_eq!(buffer.layout(&mut system).unwrap().lines().len(), 1);
    buffer.set_text("abc", style()).unwrap();
    buffer.set_width(Some(200.)).unwrap();
    let left = buffer.layout(&mut system).unwrap();
    buffer.set_align(TextAlign::Center);
    let centered = buffer.layout(&mut system).unwrap();
    near(
        centered.glyphs()[0].position[0] - left.glyphs()[0].position[0],
        (200. - left.size()[0]) / 2.,
    );
    buffer.set_align(TextAlign::Right);
    let right = buffer.layout(&mut system).unwrap();
    near(
        right.glyphs()[0].position[0] - left.glyphs()[0].position[0],
        200. - left.size()[0],
    );
    buffer.set_align(TextAlign::Start);
    buffer
        .set_style(style().font_size(40.).line_height(56.))
        .unwrap();
    let doubled = buffer.layout(&mut system).unwrap();
    near(doubled.size()[0], left.size()[0] * 2.);
    near(doubled.size()[1], left.size()[1] * 2.);
    assert_eq!(doubled.glyphs()[0].glyph_id, left.glyphs()[0].glyph_id);
    assert_eq!(wide.lines().len(), 1);
}

#[test]
fn start_and_end_alignment_follow_rtl_direction() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    buffer.set_text("مرحبا", style()).unwrap();
    buffer.set_width(Some(200.)).unwrap();
    let start = buffer.layout(&mut system).unwrap();
    buffer.set_align(TextAlign::End);
    let end = buffer.layout(&mut system).unwrap();
    near(
        start.glyphs()[0].position[0] - end.glyphs()[0].position[0],
        200. - end.size()[0],
    );
}

#[test]
fn whole_buffer_clusters_preserve_all_line_ending_bytes() {
    let mut system = system();
    let text = "é\r\nالعربية\n\re\u{301}\rxyz\n";
    let result = layout(&mut system, text, style());
    let expected = ["é", "العربية", "e\u{301}", "xyz", ""];
    assert_eq!(result.lines().len(), expected.len());
    for (line, expected) in result.lines().iter().zip(expected) {
        assert_eq!(&text[line.source.clone()], expected);
    }
    assert_eq!(
        result.lines().last().unwrap().source,
        text.len()..text.len()
    );
    near(result.size()[1], 5. * 28.);
    for pair in result.lines().windows(2) {
        near(pair[1].top - pair[0].top, 28.);
    }
    valid_clusters(&result);
}

#[test]
fn font_generation_retries_fallback_and_retains_old_fonts() {
    let mut system = TextSystem::new();
    system.load_font(LATIN).unwrap();
    let mut buffer = TextBuffer::new();
    buffer.set_text("a العربية", style()).unwrap();
    let before = buffer.layout(&mut system).unwrap();
    assert!(!before.missing_glyphs().is_empty());
    system.load_font(ARABIC).unwrap();
    let after = buffer.layout(&mut system).unwrap();
    assert!(!Arc::ptr_eq(&before, &after));
    assert!(after.missing_glyphs().is_empty());
    assert_eq!(after.fonts().len(), 2);
    assert_eq!(before.fonts()[0].data(), LATIN);
    assert!(system.owns_layout(&before));
}

#[test]
fn distinct_systems_cannot_alias_font_or_layout_identity() {
    let mut a = system();
    let mut b = system();
    let a_id = a.fonts().next().unwrap().id;
    let b_id = b.fonts().next().unwrap().id;
    assert_ne!(a_id, b_id);
    assert_eq!(b.font_info(a_id), None);
    let mut buffer = TextBuffer::new();
    buffer.set_text("abc العربية", style()).unwrap();
    let first = buffer.layout(&mut a).unwrap();
    let second = buffer.layout(&mut b).unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(a.owns_layout(&first));
    assert!(!b.owns_layout(&first));
    assert!(b.owns_layout(&second));
    assert_ne!(first.fonts()[0].id(), second.fonts()[0].id());
    near(first.size()[0], second.size()[0]);
}

#[test]
fn invalid_style_and_width_changes_are_atomic() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    buffer.set_text("retained", style()).unwrap();
    let before = buffer.layout(&mut system).unwrap();
    for invalid in [
        style().font_size(0.),
        style().font_size(f32::NAN),
        style().line_height(-1.),
        style().weight(0),
        style().weight(1001),
        style().letter_spacing(f32::INFINITY),
        style().feature([0, 0, 0, 0], 1),
    ] {
        assert_eq!(
            buffer.set_text("bad replacement", invalid.clone()),
            Err(TextError::InvalidStyle)
        );
        assert_eq!(buffer.set_style(invalid), Err(TextError::InvalidStyle));
    }
    for width in [-1., f32::INFINITY, f32::NAN] {
        assert_eq!(buffer.set_width(Some(width)), Err(TextError::InvalidWidth));
    }
    assert_eq!(buffer.text(), "retained");
    assert!(Arc::ptr_eq(&before, &buffer.layout(&mut system).unwrap()));
}

#[test]
fn zero_width_and_word_wrapping_keep_indivisible_clusters() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    buffer.set_text("ffi xyz", style()).unwrap();
    buffer.set_width(Some(0.)).unwrap();
    let glyph_wrap = buffer.layout(&mut system).unwrap();
    valid_clusters(&glyph_wrap);
    assert!(
        glyph_wrap.glyphs().iter().any(|g| g.cluster.len() > 1),
        "{:?}",
        glyph_wrap.glyphs()
    );
    buffer.set_wrap(TextWrap::Word);
    let word_wrap = buffer.layout(&mut system).unwrap();
    assert!(word_wrap.lines().iter().any(|line| line.width > 0.));
    valid_clusters(&word_wrap);
}

#[test]
fn selected_instances_retain_weight_and_synthetic_italic() {
    let mut system = system();
    let italic = layout(
        &mut system,
        "upright face fallback",
        style().slant(FontSlant::Italic),
    );
    assert!(italic.glyphs().iter().all(|g| g.synthetic_italic));
    let bold = layout(
        &mut system,
        "العربية",
        style().family("Noto Sans Arabic").weight(700),
    );
    assert!(bold.fonts().iter().all(|font| font.weight() == 700));
    assert!(bold.glyphs().iter().all(|g| !g.synthetic_italic));
    assert!(bold.missing_glyphs().is_empty());
}

#[test]
fn overflowing_blank_line_boxes_return_an_error_and_keep_old_snapshot() {
    let mut system = TextSystem::new();
    let mut buffer = TextBuffer::new();
    buffer.set_text("\n\n", style()).unwrap();
    let before = buffer.layout(&mut system).unwrap();
    buffer.set_style(style().line_height(f32::MAX)).unwrap();
    assert_eq!(
        buffer.layout(&mut system).unwrap_err(),
        TextError::LayoutOverflow
    );
    assert_eq!(before.size(), [0., 84.]);
    buffer.set_style(style()).unwrap();
    assert_eq!(buffer.layout(&mut system).unwrap().size(), before.size());
}

#[test]
fn rich_text_spans_merge_overrides_color_glyphs_and_scale_lines() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    let red = [1., 0., 0., 1.];
    let text = "plain bold red\nbig";
    buffer
        .set_rich_text(
            text,
            style(),
            vec![
                TextSpan::new(6..14).weight(700),
                TextSpan::new(11..14).color(red),
                TextSpan::new(15..18).font_size(40.),
            ],
        )
        .unwrap();
    let layout = buffer.layout(&mut system).unwrap();
    valid_clusters(&layout);
    for glyph in layout.glyphs() {
        let weight = layout.fonts()[glyph.font_index].weight();
        let cluster = glyph.cluster.start;
        assert_eq!(weight, if (6..14).contains(&cluster) { 700 } else { 400 });
        assert_eq!(
            glyph.color,
            (11..14).contains(&cluster).then_some(red),
            "{cluster}"
        );
        near(glyph.font_size, if cluster >= 15 { 40. } else { 20. });
    }
    // Only the font size is set, so the line height scales from 28 to 56.
    let lines = layout.lines();
    near(lines[0].height, 28.);
    near(lines[1].height, 56.);
    for line in lines {
        assert!(line.ascent > 0. && line.descent > 0.);
        assert!(line.baseline - line.ascent >= line.top - 0.001);
    }
    assert!(lines[1].ascent > lines[0].ascent * 1.9);
    // Identical input keeps the snapshot; a span change produces a new one.
    let spans = buffer.spans().to_vec();
    buffer.set_rich_text(text, style(), spans).unwrap();
    assert!(Arc::ptr_eq(&layout, &buffer.layout(&mut system).unwrap()));
    buffer
        .set_rich_text(text, style(), vec![TextSpan::new(0..5).color(red)])
        .unwrap();
    let recolored = buffer.layout(&mut system).unwrap();
    assert!(recolored.glyphs()[..5].iter().all(|g| g.color == Some(red)));
    assert!(recolored.glyphs()[5..].iter().all(|g| g.color.is_none()));
    // Plain set_text removes spans.
    buffer.set_text(text, style()).unwrap();
    assert!(buffer.spans().is_empty());
    let plain = buffer.layout(&mut system).unwrap();
    assert!(plain.glyphs().iter().all(|g| g.color.is_none()));
    assert!(plain.decorations().is_empty());
}

#[test]
fn rich_text_spans_cross_paragraphs_survive_edits_and_reject_invalid_ranges() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    let green = [0., 1., 0., 1.];
    buffer
        .set_rich_text(
            "ab\ncd\nef",
            style(),
            vec![TextSpan::new(1..4).color(green)],
        )
        .unwrap();
    let layout = buffer.layout(&mut system).unwrap();
    let colored: Vec<_> = layout
        .glyphs()
        .iter()
        .filter(|g| g.color.is_some())
        .map(|g| g.cluster.start)
        .collect();
    assert_eq!(colored, [1, 3]);
    // Edits through the incremental paragraph path keep spans in whole-buffer bytes.
    buffer
        .set_rich_text(
            "ab\ncd\neXf",
            style(),
            vec![TextSpan::new(7..8).color(green)],
        )
        .unwrap();
    let edited = buffer.layout(&mut system).unwrap();
    let colored: Vec<_> = edited
        .glyphs()
        .iter()
        .filter(|g| g.color.is_some())
        .map(|g| g.cluster.start)
        .collect();
    assert_eq!(colored, [7]);
    let before = buffer.spans().to_vec();
    for span in [
        TextSpan::new(0..100),
        TextSpan::new(std::ops::Range { start: 3, end: 1 }),
        TextSpan::new(0..1).font_size(0.),
        TextSpan::new(0..1).weight(0),
        TextSpan::new(0..1).color([1., 1., 1., 2.]),
        TextSpan::new(0..1).letter_spacing(f32::NAN),
    ] {
        assert_eq!(
            buffer.set_rich_text("ab\ncd\neXf", style(), vec![span]),
            Err(TextError::InvalidSpan)
        );
    }
    assert_eq!(
        buffer.set_rich_text("é", style(), vec![TextSpan::new(0..1)]),
        Err(TextError::InvalidSpan)
    );
    assert_eq!(buffer.spans(), before);
    assert_eq!(buffer.text(), "ab\ncd\neXf");
}

#[test]
fn decorations_use_font_metrics_and_split_by_span_color() {
    let mut system = system();
    let mut buffer = TextBuffer::new();
    let (red, blue) = ([1., 0., 0., 1.], [0., 0., 1., 1.]);
    buffer
        .set_rich_text(
            "under strike",
            style(),
            vec![
                TextSpan::new(0..5).underline(),
                TextSpan::new(0..2).color(red),
                TextSpan::new(2..5).color(blue),
                TextSpan::new(6..12).strikethrough(),
            ],
        )
        .unwrap();
    let layout = buffer.layout(&mut system).unwrap();
    let line = &layout.lines()[0];
    let decorations = layout.decorations();
    assert_eq!(decorations.len(), 3);
    let advance = |range: std::ops::Range<usize>| -> f32 {
        layout
            .glyphs()
            .iter()
            .filter(|g| range.contains(&g.cluster.start))
            .map(|g| g.advance)
            .sum()
    };
    for (decoration, (kind, color, range)) in decorations.iter().zip([
        (TextDecorationKind::Underline, Some(red), 0..2),
        (TextDecorationKind::Underline, Some(blue), 2..5),
        (TextDecorationKind::Strikethrough, None, 6..12),
    ]) {
        assert_eq!(decoration.kind, kind);
        assert_eq!(decoration.color, color);
        assert_eq!(decoration.line_index, 0);
        near(decoration.rect.width, advance(range));
        // Source Sans 3 metrics: underline below the baseline, strikeout near x-height.
        assert!(decoration.rect.height > 0.5 && decoration.rect.height < 2.);
        if kind == TextDecorationKind::Underline {
            assert!(decoration.rect.y > line.baseline);
        } else {
            assert!(decoration.rect.bottom() < line.baseline - 2.);
        }
    }
    // Adjacent colors share one continuous underline.
    near(decorations[0].rect.right(), decorations[1].rect.x);
}

#[test]
fn measurement_matches_configured_widths_without_replacing_the_snapshot() {
    let mut system = system();
    let text = "Wrapped words and العربية text\n\nsecond paragraph wraps too";
    let spans = vec![
        TextSpan::new(0..7).font_size(30.),
        TextSpan::new(8..13).weight(700),
    ];
    for wrap in [TextWrap::WordOrGlyph, TextWrap::Word, TextWrap::None] {
        let mut buffer = TextBuffer::new();
        buffer.set_rich_text(text, style(), spans.clone()).unwrap();
        buffer.set_wrap(wrap);
        buffer.set_width(Some(90.)).unwrap();
        let snapshot = buffer.layout(&mut system).unwrap();
        for width in [
            Some(0.),
            Some(45.),
            Some(90.),
            Some(160.),
            Some(1000.),
            None,
        ] {
            let mut fresh = TextBuffer::new();
            fresh.set_rich_text(text, style(), spans.clone()).unwrap();
            fresh.set_wrap(wrap);
            fresh.set_width(width).unwrap();
            let expected = fresh.layout(&mut system).unwrap().size();
            for _ in 0..2 {
                let size = buffer.measure(&mut system, width).unwrap();
                near(size[0], expected[0]);
                near(size[1], expected[1]);
            }
        }
        assert_eq!(buffer.width(), Some(90.));
        assert!(Arc::ptr_eq(&snapshot, &buffer.layout(&mut system).unwrap()));

        let [min, max] = buffer.intrinsic_widths(&mut system).unwrap();
        near(max, buffer.measure(&mut system, None).unwrap()[0]);
        assert!(min > 0. && min <= max);
        if wrap == TextWrap::None {
            near(min, max);
        } else {
            // Min-content wraps more lines than max-content.
            assert!(min < max);
            assert!(
                buffer.measure(&mut system, Some(min)).unwrap()[1]
                    > buffer.measure(&mut system, Some(max)).unwrap()[1]
            );
        }
        // Content edits invalidate cached measurements.
        buffer.set_rich_text("x", style(), Vec::new()).unwrap();
        let single = layout(&mut system, "x", style()).size();
        let measured = buffer.measure(&mut system, Some(0.)).unwrap();
        near(measured[0], single[0]);
        near(measured[1], single[1]);
    }
    let mut buffer = TextBuffer::new();
    assert!(matches!(
        buffer.measure(&mut system, Some(-1.)),
        Err(TextError::InvalidWidth)
    ));
    assert!(matches!(
        buffer.measure(&mut system, Some(f32::NAN)),
        Err(TextError::InvalidWidth)
    ));
    // Blank text needs no fonts and ignores width.
    buffer.set_text("\n", style()).unwrap();
    assert_eq!(
        buffer.measure(&mut TextSystem::new(), Some(5.)).unwrap(),
        [0., 56.]
    );
}

#[test]
fn shaping_strategy_changes_rebuild_paragraphs_and_retain_old_snapshots() {
    let mut fonts = system();
    let mut buffer = TextBuffer::new();
    buffer.set_text("office\nAV", style()).unwrap();
    assert_eq!(buffer.shaping(), TextShaping::Advanced);
    let advanced = buffer.layout(&mut fonts).unwrap();
    buffer.set_shaping(TextShaping::Basic);
    let basic = buffer.layout(&mut fonts).unwrap();
    assert!(!Arc::ptr_eq(&advanced, &basic));
    assert_eq!(basic.glyphs().len(), 8);
    assert!(advanced.glyphs().len() < basic.glyphs().len());
    buffer.set_shaping(TextShaping::Basic);
    assert!(Arc::ptr_eq(&basic, &buffer.layout(&mut fonts).unwrap()));
    buffer.set_text("office\nAV\nx", style()).unwrap();
    let inserted = buffer.layout(&mut fonts).unwrap();
    assert_eq!(inserted.glyphs().len(), 9);
    buffer.set_shaping(TextShaping::Advanced);
    let restored = buffer.layout(&mut fonts).unwrap();
    assert!(restored.glyphs().len() < inserted.glyphs().len());
    assert_eq!(advanced.text(), "office\nAV");
    assert_eq!(buffer.measure(&mut fonts, None).unwrap(), restored.size());
}

#[test]
fn minimum_line_metrics_preserve_baselines_decorations_and_measurement() {
    let mut fonts = system();
    let mut buffer = TextBuffer::new();
    buffer
        .set_rich_text(
            "hello world\nsecond line",
            style(),
            vec![TextSpan::new(0..5).strikethrough()],
        )
        .unwrap();
    buffer.set_width(Some(80.)).unwrap();
    let natural = buffer.layout(&mut fonts).unwrap();
    let minimum = TextLineMetrics {
        ascent: 40.,
        descent: 10.,
        height: 64.,
    };
    buffer.set_minimum_line_metrics(Some(minimum)).unwrap();
    let expanded = buffer.layout(&mut fonts).unwrap();
    assert_eq!(expanded.lines().len(), natural.lines().len());
    for (old, new) in natural.lines().iter().zip(expanded.lines()) {
        assert_eq!(new.height, 64.);
        assert_eq!(new.baseline - new.top, 47.);
        let delta = new.baseline - old.baseline;
        for (a, b) in natural.glyphs()[old.glyph_range.clone()]
            .iter()
            .zip(&expanded.glyphs()[new.glyph_range.clone()])
        {
            assert_eq!(a.position[0], b.position[0]);
            assert!((a.position[1] + delta - b.position[1]).abs() < 0.0001);
        }
    }
    for (old, new) in natural.decorations().iter().zip(expanded.decorations()) {
        let delta =
            expanded.lines()[new.line_index].baseline - natural.lines()[old.line_index].baseline;
        assert!((old.rect.y + delta - new.rect.y).abs() < 0.0001);
    }
    expanded.prepare_interaction().unwrap();
    assert_eq!(expanded.caret(TextPosition::new(0)).unwrap().height, 64.);
    assert_eq!(buffer.measure(&mut fonts, Some(40.)).unwrap()[1] % 64., 0.);
    buffer.set_minimum_line_metrics(Some(minimum)).unwrap();
    assert!(Arc::ptr_eq(&expanded, &buffer.layout(&mut fonts).unwrap()));
    assert!(
        buffer
            .set_minimum_line_metrics(Some(TextLineMetrics {
                height: f32::NAN,
                ..minimum
            }))
            .is_err()
    );
    assert!(Arc::ptr_eq(&expanded, &buffer.layout(&mut fonts).unwrap()));
    buffer.set_minimum_line_metrics(None).unwrap();
    assert_eq!(buffer.layout(&mut fonts).unwrap().lines(), natural.lines());
}
