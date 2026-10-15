//! Placement regressions for the adapter, above Astrelis's direct-raster tests.
use super::image_gpu_tests::{Page, pixels, read_pixel};
use super::*;
use crate::*;
use astrelis::{FramebufferOptions, wgpu};

fn target(graphics: &GraphicsContext) -> astrelis::Framebuffer {
    graphics
        .create_framebuffer(
            FramebufferOptions::new(64, 64)
                .format(wgpu::TextureFormat::Rgba8Unorm)
                .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
        )
        .unwrap()
}
fn painter(graphics: &GraphicsContext) -> UiPainter {
    let mut painter = UiPainter::new(graphics);
    painter
        .fonts_mut()
        .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
        .unwrap();
    painter.set_sans_serif_family(Some(Arc::from("Source Sans 3")));
    painter
}
fn scene(origin: [f32; 2], opacity: f32) -> Element {
    stack().size(64., 64.).child(
        stack()
            .absolute()
            .left(origin[0])
            .top(origin[1])
            .size(56., 32.)
            .opacity(opacity)
            .child(
                label("ill MW")
                    .key("text")
                    .font_size(13.13)
                    .color([1.; 4])
                    .width(56.)
                    .height(32.),
            ),
    )
}
fn render(
    graphics: &GraphicsContext,
    target: &mut astrelis::Framebuffer,
    painter: &mut UiPainter,
    ui: &Ui<Page>,
    scale: f32,
) -> Vec<u8> {
    let texture = target.color_texture().unwrap().clone();
    let mut frame = target.begin_frame().unwrap();
    painter
        .compose(ui, &mut frame, scale, |frame, composed| {
            let mut pass = frame
                .render_pass()
                .clear_color(wgpu::Color::TRANSPARENT)
                .begin()?;
            composed.paint(&mut pass)
        })
        .unwrap();
    let read = read_pixel(graphics, &mut frame, &texture);
    pixels(graphics, &read, frame.finish().unwrap())
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn phased_ui_coverage_matches_placed_astrelis_and_reuses_integer_moves() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = target(&graphics);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Page(scene([4.125, 3.375], 1.))));
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut painter = painter(&graphics);
        let mut layout = None;
        for (scale, origin, changed) in [
            (1., [4.125, 3.375], true),
            (1., [5.125, 4.375], false),
            (1., [5.375, 4.625], true),
            (2., [5.375, 4.625], true),
            (2., [5.875, 5.125], false),
        ] {
            runtime.update(|cx| root.update(cx, |s, _| s.0 = scene(origin, 1.)));
            ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
            let id = ui
                .elements()
                .find(|e| e.key == Some(&Key::from("text")))
                .unwrap()
                .id;
            let before = painter.painter.text().stats().geometry_bytes;
            painter
                .prepare(&ui, &target.render_format(), scale)
                .unwrap();
            assert_eq!(
                painter.painter.text().stats().geometry_bytes > before,
                changed
            );
            let resource = &painter.texts[&id];
            let current = resource.prepared_layout.as_ref().unwrap().clone();
            if let Some(previous) = layout {
                assert!(Arc::ptr_eq(&previous, &current));
            }
            layout = Some(current.clone());
            assert_eq!(resource.raster_phase, Some(raster_phase(origin, scale)));
            let actual = render(&graphics, &mut target, &mut painter, &ui, scale);
            let reference = painter
                .painter
                .prepare_text(
                    &current,
                    TextRasterOptions::new()
                        .raster_scale(scale)
                        .physical_origin(origin.map(|v| v * scale)),
                )
                .unwrap();
            let texture = target.color_texture().unwrap().clone();
            let mut frame = target.begin_frame().unwrap();
            {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::TRANSPARENT)
                    .begin()
                    .unwrap();
                let mut paint = painter.painter.begin(&mut pass).unwrap();
                paint
                    .transformed(Transform2D::scale(scale, scale))
                    .unwrap()
                    .draw_text(&reference, TextDraw::new(origin).color([1.; 4]))
                    .unwrap();
            }
            let read = read_pixel(&graphics, &mut frame, &texture);
            let expected = pixels(&graphics, &read, frame.finish().unwrap());
            assert_eq!(actual, expected, "origin={origin:?}, scale={scale}");
        }
        assert!(errors.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn phased_coverage_keeps_alignment_in_cropped_opacity_layers() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = target(&graphics);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Page(scene([4.375, 3.625], 1.))));
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut painter = painter(&graphics);
        for scale in [1., 2.] {
            runtime.update(|cx| root.update(cx, |s, _| s.0 = scene([4.375, 3.625], 1.)));
            ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
            painter
                .prepare(&ui, &target.render_format(), scale)
                .unwrap();
            let solid = render(&graphics, &mut target, &mut painter, &ui, scale);
            let before = painter.painter.text().stats().geometry_bytes;
            runtime.update(|cx| root.update(cx, |s, _| s.0 = scene([4.375, 3.625], 0.5)));
            ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
            painter
                .prepare(&ui, &target.render_format(), scale)
                .unwrap();
            let faded = render(&graphics, &mut target, &mut painter, &ui, scale);
            assert_eq!(painter.painter.text().stats().geometry_bytes, before);
            assert!(solid.iter().any(|v| *v > 128));
            assert!(
                solid
                    .iter()
                    .zip(faded)
                    .all(|(&a, b)| b.abs_diff((f32::from(a) * 0.5).round() as u8) <= 1)
            );
        }
        assert!(errors.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn phased_chunks_placeholders_and_input_scroll_use_their_own_origins() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let target = target(&graphics);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| {
            cx.new(|_| {
                Page(
                    stack().size(64., 64.).child(
                        label("row\n".repeat(32))
                            .key("chunks")
                            .absolute()
                            .left(2.375)
                            .top(1.625)
                            .width(56.)
                            .height(700.)
                            .font_size(13.13),
                    ),
                )
            })
        });
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut painter = painter(&graphics);
        ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
        painter.prepare(&ui, &target.render_format(), 2.).unwrap();
        let id = ui
            .elements()
            .find(|e| e.key == Some(&Key::from("chunks")))
            .unwrap()
            .id;
        let origin = ui.text_origin(id, false).unwrap();
        let chunks = painter.texts[&id].chunks.as_ref().unwrap();
        assert!(chunks.list.len() > 1);
        for chunk in &chunks.list {
            assert_eq!(
                chunk.resource.raster_phase,
                Some(raster_phase([origin[0], origin[1] + chunk.top], 2.))
            );
        }
        let initial_bytes = painter.painter.text().stats().geometry_bytes;
        // The second move preserves the base phase but reveals different chunks.
        // Their deferred phase preparation must still run.
        for top in [-200.125, -400.125] {
            runtime.update(|cx| {
                root.update(cx, |s, _| {
                    s.0 = stack().size(64., 64.).child(
                        label("row\n".repeat(32))
                            .key("chunks")
                            .absolute()
                            .left(2.375)
                            .top(top)
                            .width(56.)
                            .height(700.)
                            .font_size(13.13),
                    )
                })
            });
            ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
            let before = painter.painter.text().stats().geometry_bytes;
            painter.prepare(&ui, &target.render_format(), 2.).unwrap();
            assert!(painter.painter.text().stats().geometry_bytes - before < initial_bytes);
            let origin = ui.text_origin(id, false).unwrap();
            let clip = ui.ink_extent(id).unwrap().clip;
            for chunk in &painter.texts[&id].chunks.as_ref().unwrap().list {
                let origin = [origin[0], origin[1] + chunk.top];
                if chunk
                    .resource
                    .ink_bounds()
                    .is_some_and(|ink| ink_intersects(ink, origin, clip, 2.))
                {
                    assert_eq!(chunk.resource.raster_phase, Some(raster_phase(origin, 2.)));
                }
            }
        }
        runtime.update(|cx| {
            root.update(cx, |s, _| {
                s.0 = text_input("")
                    .key("input")
                    .placeholder("ill MW")
                    .absolute()
                    .left(2.375)
                    .top(1.625)
                    .width(32.)
                    .font_size(13.13)
            })
        });
        ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
        painter.prepare(&ui, &target.render_format(), 2.).unwrap();
        let id = ui
            .elements()
            .find(|e| e.key == Some(&Key::from("input")))
            .unwrap()
            .id;
        assert_eq!(
            painter.placeholders[&id].resource.raster_phase,
            Some(raster_phase(ui.text_origin(id, true).unwrap(), 2.))
        );
        runtime.update(|cx| {
            root.update(cx, |s, _| {
                s.0 = text_input("lots of fractional glyph advances here")
                    .key("input")
                    .width(32.)
                    .font_size(13.13)
            })
        });
        ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
        painter.prepare(&ui, &target.render_format(), 2.).unwrap();
        assert!(ui.focus(id));
        let key = ui.composition_key();
        let layout = painter.texts[&id].prepared_layout.as_ref().unwrap().clone();
        ui.text_input(
            &mut runtime,
            TextInputEvent::Move {
                movement: TextMovement::End,
                extend: false,
            },
            &mut painter,
        )
        .unwrap();
        assert_ne!(
            ui.composition_key(),
            key,
            "caret scrolling must invalidate the retained pass"
        );
        painter.prepare(&ui, &target.render_format(), 2.).unwrap();
        assert_eq!(
            painter.texts[&id].raster_phase,
            Some(raster_phase(ui.text_origin(id, false).unwrap(), 2.))
        );
        assert!(Arc::ptr_eq(
            &layout,
            painter.texts[&id].prepared_layout.as_ref().unwrap()
        ));
    });
}
