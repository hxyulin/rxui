use super::*;
use std::sync::Arc;

fn fonts() -> TextSystem {
    let mut fonts = TextSystem::new();
    fonts
        .load_font(include_bytes!("../../tests/fonts/SourceSans3-Regular.otf"))
        .unwrap();
    fonts
        .load_font(include_bytes!("../../tests/fonts/NotoSansArabic.ttf"))
        .unwrap();
    fonts
}
fn buffer(text: &str) -> TextBuffer {
    let mut buffer = TextBuffer::new();
    buffer
        .set_text(text, TextStyle::new().family("Source Sans 3"))
        .unwrap();
    buffer.set_width(Some(95.)).unwrap();
    buffer
}

// Compare against independently evaluated full geometry, including remapped local
// line/font indices and interaction. This catches accidental clipping after a full
// snapshot as well as incorrect document positions/source ranges when scrolling.
fn compare(full: &TextLayout, view: &TextLayout, visible: std::ops::Range<f32>) {
    assert_eq!(view.text(), full.text());
    assert_eq!(view.size(), full.size());
    assert_eq!(view.total_line_count(), full.lines().len());
    assert_eq!(view.viewport(), Some(visible.clone()));
    let indices: Vec<_> = full
        .lines()
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            visible.start < visible.end
                && line.top < visible.end
                && line.top + line.height > visible.start
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(view.lines().len(), indices.len());
    assert_eq!(
        view.line_range(),
        indices.first().map_or(0..0, |&i| i..i + indices.len())
    );
    for (local, &global) in indices.iter().enumerate() {
        let mut expected = full.lines()[global].clone();
        let expected_glyphs = &full.glyphs()[expected.glyph_range.clone()];
        expected.glyph_range = view.lines()[local].glyph_range.clone();
        assert_eq!(&expected, &view.lines()[local]);
        assert_eq!(
            view.visual_line_source(local),
            full.visual_line_source(global)
        );
        let glyphs = &view.glyphs()[expected.glyph_range.clone()];
        assert_eq!(glyphs.len(), expected_glyphs.len());
        for (glyph, expected) in glyphs.iter().zip(expected_glyphs) {
            let mut adjusted = expected.clone();
            adjusted.line_index = local;
            adjusted.font_index = glyph.font_index;
            assert_eq!(glyph, &adjusted);
            assert_eq!(
                view.fonts()[glyph.font_index].id(),
                full.fonts()[expected.font_index].id()
            );
        }
        let expected_decorations: Vec<_> = full
            .decorations()
            .iter()
            .filter(|d| d.line_index == global)
            .map(|d| {
                let mut d = d.clone();
                d.line_index = local;
                d
            })
            .collect();
        let decorations: Vec<_> = view
            .decorations()
            .iter()
            .filter(|d| d.line_index == local)
            .cloned()
            .collect();
        assert_eq!(decorations, expected_decorations);
        for x in [-10., 0., 15., 45., 100., 200.] {
            let point = [x, expected.top + expected.height * 0.5];
            assert_eq!(
                view.hit_test(point),
                full.hit_test(point),
                "point={point:?}"
            );
        }
        for glyph in glyphs {
            for affinity in [TextAffinity::Upstream, TextAffinity::Downstream] {
                let p = TextPosition::new(glyph.cluster.start).affinity(affinity);
                // At slice edges the absent affinity may fall back. For carets
                // owned by this line, exact position/geometry must agree.
                if let Some(expected) = full.caret(p)
                    && expected.line_index == global
                {
                    let mut expected = expected;
                    expected.line_index = local;
                    assert_eq!(view.caret(p), Some(expected));
                }
            }
        }
    }
    let expected: Vec<_> = full
        .selection_rects(0..full.text().len())
        .unwrap()
        .filter(|r| indices.iter().any(|&i| full.lines()[i].top == r.y))
        .collect();
    let actual: Vec<_> = view
        .selection_rects(0..view.text().len())
        .unwrap()
        .collect();
    assert_eq!(actual, expected);
    for offset in 0..=full.text().len() {
        assert_eq!(view.is_text_boundary(offset), full.is_text_boundary(offset));
        assert_eq!(view.next_boundary(offset), full.next_boundary(offset));
        assert_eq!(
            view.previous_boundary(offset),
            full.previous_boundary(offset)
        );
        if full.is_text_boundary(offset) {
            let expected: Vec<_> = full
                .selection_rects(offset..full.text().len())
                .unwrap()
                .filter(|r| indices.iter().any(|&i| full.lines()[i].top == r.y))
                .collect();
            assert_eq!(
                view.selection_rects(offset..view.text().len())
                    .unwrap()
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
    let expected_missing: Vec<_> = full
        .missing_glyphs()
        .iter()
        .filter(|r| {
            view.glyphs()
                .iter()
                .any(|g| g.glyph_id == 0 && g.cluster == **r)
        })
        .cloned()
        .collect();
    assert_eq!(view.missing_glyphs(), expected_missing);
}

#[test]
fn viewport_matches_full_scroll_wrap_bidi_decorations_and_metrics() {
    let text =
        "first office\r\n\nالعربية 123 abc e\u{301} office words wrap here\n\r\nlast \u{10ffff}\n";
    let mut fonts = fonts();
    for width in [Some(45.), Some(95.), None] {
        for minimum in [
            None,
            Some(TextLineMetrics {
                ascent: 18.,
                descent: 9.,
                height: 40.,
            }),
        ] {
            let mut b = buffer(text);
            b.set_width(width).unwrap();
            b.set_minimum_line_metrics(minimum).unwrap();
            b.set_rich_text(
                text,
                b.style().clone(),
                vec![TextSpan::new(0..text.len()).underline().strikethrough()],
            )
            .unwrap();
            let full = b.layout(&mut fonts).unwrap();
            for line in full.lines() {
                for range in [
                    line.top..line.top + line.height,
                    line.top + 1.25..line.top + line.height * 1.5,
                ] {
                    let view = b.layout_viewport(&mut fonts, range.clone()).unwrap();
                    compare(&full, &view, range.clone());
                    assert!(Arc::ptr_eq(
                        &view,
                        &b.layout_viewport(&mut fonts, range).unwrap()
                    ));
                    assert!(Arc::ptr_eq(&full, &b.layout(&mut fonts).unwrap()));
                }
            }
            for range in [-100.0..-10., 0.0..0., full.size()[1]..full.size()[1] + 100.] {
                compare(
                    &full,
                    &b.layout_viewport(&mut fonts, range.clone()).unwrap(),
                    range,
                );
            }
        }
    }
}

#[test]
fn viewport_first_edits_and_font_generation_keep_stable_document_offsets() {
    let mut fonts = fonts();
    let mut b = buffer("leading\r\nold wrapped words words\nالعربية\ntrailing\n");
    let old = b.layout_viewport(&mut fonts, 22.0..88.).unwrap();
    assert!(!b.has_full_snapshot());
    for text in [
        "leading\r\nnew text\nالعربية\ntrailing\n",
        "inserted\nleading\r\nnew text\nالعربية\ntrailing\n",
        "العربية\ntrailing\n",
    ] {
        b.set_text(text, b.style().clone()).unwrap();
        let view = b.layout_viewport(&mut fonts, 22.0..88.).unwrap();
        let mut reference = buffer(text);
        let full = reference.layout(&mut fonts).unwrap();
        compare(&full, &view, 22.0..88.);
    }
    assert_eq!(
        old.text(),
        "leading\r\nold wrapped words words\nالعربية\ntrailing\n"
    );
    let before = b.layout_viewport(&mut fonts, 0.0..44.).unwrap();
    fonts
        .load_font(include_bytes!("../../tests/fonts/SourceSans3-Regular.otf"))
        .unwrap();
    let after = b.layout_viewport(&mut fonts, 0.0..44.).unwrap();
    assert!(!Arc::ptr_eq(&before, &after));
    compare(&b.layout(&mut fonts).unwrap(), &after, 0.0..44.);
}

#[test]
fn empty_lines_no_fonts_invalid_ranges_and_partial_interaction() {
    let mut fonts = TextSystem::new();
    for text in ["", "\n", "\r\n\n\r\n"] {
        let mut b = buffer(text);
        let view = b.layout_viewport(&mut fonts, 22.0..44.).unwrap();
        compare(&b.layout(&mut fonts).unwrap(), &view, 22.0..44.);
    }
    let mut b = buffer("requires fonts");
    assert!(matches!(
        b.layout_viewport(&mut fonts, 0.0..0.),
        Err(TextError::NoFonts)
    ));
    let mut fonts = self::fonts();
    let mut b = buffer("offscreen\nvisible e\u{301}\nlast\n");
    let v = b.layout_viewport(&mut fonts, 22.0..44.).unwrap();
    for range in [f32::NAN..1., 0.0..f32::INFINITY, 2.0..1.] {
        assert!(matches!(
            b.layout_viewport(&mut fonts, range),
            Err(TextError::InvalidViewport)
        ));
    }
    assert!(Arc::ptr_eq(
        &v,
        &b.layout_viewport(&mut fonts, 22.0..44.).unwrap()
    ));
    assert_eq!(v.caret(TextPosition::new(0)), None);
    assert_eq!(v.selection_rects(0..1).unwrap().count(), 0);
    assert!(
        v.selection_rects(0..v.text().find('\u{301}').unwrap())
            .is_err()
    );
    let empty = b.layout_viewport(&mut fonts, 900.0..1000.).unwrap();
    assert_eq!(empty.hit_test([0., 900.]), None);
    assert_eq!(
        empty
            .selection_rects(0..empty.text().len())
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn large_document_snapshot_has_only_visible_geometry() {
    let mut fonts = fonts();
    let mut b = buffer(&"log entry office 123\n".repeat(10_000));
    b.set_width(None).unwrap();
    let view = b.layout_viewport(&mut fonts, 110_000.0..110_220.).unwrap();
    assert_eq!(view.total_line_count(), 10_001);
    assert_eq!(view.line_range(), 5000..5010);
    assert_eq!(view.lines().len(), 10);
    assert!(view.glyphs().len() < 220);
    assert!(
        !b.has_full_snapshot(),
        "viewport must never construct/cache a full glyph snapshot"
    );
    assert!(view.glyphs().iter().all(|g| g.cluster.start >= 5000 * 21));
}

#[test]
#[ignore = "requires a GPU; run with -- --ignored"]
fn viewport_preparation_and_worker_draw_match_full_pixels() {
    use crate::{
        FramebufferOptions, GlyphPreparationLimits, GlyphUploadBudget, GraphicsContext, TextDraw,
        wgpu,
    };
    let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
    let mut target = graphics
        .create_framebuffer(
            FramebufferOptions::new(160, 44)
                .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
        )
        .unwrap();
    let mut fonts = fonts();
    let mut b = buffer(&"office العربية 123\n".repeat(200));
    b.set_width(None).unwrap();
    // Include an extra line above/below the visible area for ink overshoot.
    let view = b.layout_viewport(&mut fonts, 2178.0..2266.).unwrap();
    let full = b.layout(&mut fonts).unwrap();
    let mut renderer = TextRenderer::new(&graphics);
    renderer.prepare(&target.render_format()).unwrap();
    let prepared_full = renderer
        .prepare_text(&full, TextRasterOptions::new())
        .unwrap();
    let request = renderer
        .request_glyphs(
            view.clone(),
            TextRasterOptions::new(),
            GlyphPreparationLimits::default(),
        )
        .unwrap();
    let generated = std::thread::spawn(move || request.generate())
        .join()
        .unwrap()
        .unwrap();
    let mut upload = renderer.begin_glyph_upload(generated).unwrap();
    while upload.remaining() > 0 {
        renderer
            .commit_glyphs(
                &mut upload,
                GlyphUploadBudget {
                    max_glyphs: 4,
                    max_bytes: 65536,
                },
            )
            .unwrap();
    }
    let prepared_view = renderer.finish_glyph_upload(&upload, 1024 * 1024).unwrap();
    assert_eq!(prepared_view.size(), prepared_full.size());
    assert!(prepared_view.glyph_count() < prepared_full.glyph_count() / 20);
    let mut draw = |prepared: &PreparedText| {
        crate::framebuffer::tests::pixels(&graphics, &mut target, |frame| {
            let mut pass = frame.render_pass().begin().unwrap();
            renderer
                .draw(&mut pass, prepared, TextDraw::new([0., -2200.]))
                .unwrap();
        })
    };
    let expected = draw(&prepared_full);
    assert!(expected.iter().any(|&byte| byte != 0));
    assert_eq!(draw(&prepared_view), expected);
}
