use super::image_gpu_tests::{Page, pixels, prepare, read_pixel};
use super::*;
use crate::*;
use astrelis::{FramebufferOptions, wgpu};

fn target(graphics: &GraphicsContext, samples: u32) -> astrelis::Framebuffer {
    graphics
        .create_framebuffer(
            FramebufferOptions::new(64, 64)
                .format(wgpu::TextureFormat::Rgba8Unorm)
                .sample_count(samples)
                .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
        )
        .unwrap()
}
fn scene(alpha: f32, child_alpha: f32) -> Element {
    stack().size(64., 64.).child(
        stack()
            .key("group")
            .absolute()
            .left(8.)
            .top(8.)
            .size(24., 24.)
            .opacity(alpha)
            .background([1., 0., 0., 1.])
            .child(
                column()
                    .key("child")
                    .absolute()
                    .left(12.)
                    .top(12.)
                    .size(24., 24.)
                    .opacity(child_alpha)
                    .background([0., 1., 0., 1.]),
            ),
    )
}
fn draw(
    painter: &mut UiPainter,
    ui: &Ui<Page>,
    graphics: &GraphicsContext,
    target: &mut astrelis::Framebuffer,
    scale: f32,
) -> Vec<u8> {
    let texture = target.color_texture().unwrap().clone();
    let mut frame = target.begin_frame().unwrap();
    painter
        .compose(ui, &mut frame, scale, |frame, ui| {
            let mut pass = frame
                .render_pass()
                .clear_color(wgpu::Color {
                    r: 0.,
                    g: 0.,
                    b: 1.,
                    a: 1.,
                })
                .begin()?;
            pass.set_scissor_rect(4, 4, 56, 56)?;
            ui.paint(&mut pass)?;
            assert_eq!(pass.scissor_rect(), [4, 4, 56, 56]);
            Ok(())
        })
        .unwrap();
    let buffer = read_pixel(graphics, &mut frame, &texture);
    pixels(graphics, &buffer, frame.finish().unwrap())
}
fn near(bytes: &[u8], x: usize, y: usize, expected: [u8; 4]) {
    let actual = &bytes[y * 256 + x * 4..y * 256 + x * 4 + 4];
    assert!(
        actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
        "pixel ({x},{y}): {actual:?}, expected {expected:?}"
    );
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn group_opacity_overlap_nesting_overflow_and_storage_reuse_match_pixels() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = target(&graphics, 1);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Page(scene(0.5, 1.))));
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut painter = UiPainter::new(&graphics);
        prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
        let stats = ui.stats();
        let bytes = draw(&mut painter, &ui, &graphics, &mut target, 1.);
        near(&bytes, 10, 10, [128, 0, 128, 255]);
        near(&bytes, 22, 22, [0, 128, 128, 255]); // overlap fades once, not per draw
        near(&bytes, 40, 40, [0, 128, 128, 255]); // child overflow expands layer
        near(&bytes, 46, 46, [0, 0, 255, 255]);
        assert_eq!(painter.layer_stats().allocations, 1);
        assert_eq!(painter.layer_stats().live_pixels, 36 * 36);
        runtime.update(|cx| root.update(cx, |s, _| s.0 = scene(0.25, 1.)));
        prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
        assert_eq!(ui.stats().layout_passes, stats.layout_passes);
        assert_eq!(ui.stats().measurements, stats.measurements);
        near(
            &draw(&mut painter, &ui, &graphics, &mut target, 1.),
            22,
            22,
            [0, 64, 191, 255],
        );
        assert_eq!(painter.layer_stats().allocations, 1);
        runtime.update(|cx| root.update(cx, |s, _| s.0 = scene(0.5, 0.5)));
        prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
        let bytes = draw(&mut painter, &ui, &graphics, &mut target, 1.);
        near(&bytes, 22, 22, [64, 64, 128, 255]);
        near(&bytes, 40, 40, [0, 64, 191, 255]);
        assert_eq!(painter.layer_stats().live_layers, 2);
        // Discarded recordings cannot make future composition sample stale output.
        {
            let mut frame = target.begin_frame().unwrap();
            painter.compose(&ui, &mut frame, 1., |_, _| Ok(())).unwrap();
        }
        near(
            &draw(&mut painter, &ui, &graphics, &mut target, 1.),
            22,
            22,
            [64, 64, 128, 255],
        );
        for alpha in [0., 1.] {
            runtime.update(|cx| root.update(cx, |s, _| s.0 = scene(alpha, 1.)));
            prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
            let passes = painter.layer_stats().render_passes;
            let bytes = draw(&mut painter, &ui, &graphics, &mut target, 1.);
            near(
                &bytes,
                22,
                22,
                if alpha == 0. {
                    [0, 0, 255, 255]
                } else {
                    [0, 255, 0, 255]
                },
            );
            assert_eq!(painter.layer_stats().live_layers, 0);
            assert_eq!(painter.layer_stats().render_passes, passes);
        }
        assert!(errors.pop().await.is_none());
    });
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn groups_include_images_text_clip_dpi_msaa_and_live_framebuffer_updates() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = target(&graphics, 4);
        let mut source = graphics
            .create_framebuffer(FramebufferOptions::new(8, 8))
            .unwrap();
        let live = Image::from_framebuffer(&source);
        let asset = Image::from_rgba8(1, 1, vec![255, 0, 0, 128]).unwrap();
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| {
            cx.new(|_| {
                Page(
                    column()
                        .size(32., 32.)
                        .clip()
                        .opacity(0.5)
                        .child(image(asset).size(8., 8.))
                        .child(image(live).size(8., 8.))
                        .child(label("Ink").font_size(8.).color([1.; 4]))
                        .child(
                            column()
                                .absolute()
                                .left(28.)
                                .top(28.)
                                .size(12., 12.)
                                .background([0., 1., 0., 1.]),
                        ),
                )
            })
        });
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut painter = UiPainter::new(&graphics);
        painter
            .fonts_mut()
            .load_font_shared(std::sync::Arc::<[u8]>::from(
                include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice(),
            ))
            .unwrap();
        ui.prepare(&mut runtime, [32., 32.], &mut painter).unwrap();
        painter.prepare(&ui, &target.render_format(), 2.).unwrap();
        assert_eq!(painter.layer_stats().live_pixels, 64 * 64);
        for color in [wgpu::Color::GREEN, wgpu::Color::RED] {
            let texture = target.color_texture().unwrap().clone();
            let mut frame = target.begin_frame().unwrap();
            {
                let _pass = frame
                    .render_to(&mut source)
                    .clear_color(color)
                    .begin()
                    .unwrap();
            }
            painter
                .compose(&ui, &mut frame, 2., |frame, ui| {
                    let mut pass = frame.render_pass().clear_color(wgpu::Color::BLUE).begin()?;
                    ui.paint(&mut pass)
                })
                .unwrap();
            let buffer = read_pixel(&graphics, &mut frame, &texture);
            let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
            near(&bytes, 4, 4, [64, 0, 191, 255]); // straight image -> premult layer -> opacity
            near(
                &bytes,
                4,
                20,
                if color == wgpu::Color::GREEN {
                    [0, 128, 128, 255]
                } else {
                    [128, 0, 128, 255]
                },
            );
            near(&bytes, 60, 60, [0, 128, 128, 255]);
            assert!(
                bytes[32 * 256..52 * 256]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] > 15 && p[1] > 15),
                "glyph ink must reach final pass"
            );
        }
        assert_eq!(painter.layer_stats().allocations, 1);
        // Reconfiguration replaces storage; stale plans fail before any layer passes.
        painter.prepare(&ui, &target.render_format(), 1.).unwrap();
        assert_eq!(painter.layer_stats().allocations, 2);
        runtime.update(|cx| {
            root.update(cx, |s, _| {
                s.0 = column().opacity(0.3).size(20., 20.).background([1.; 4])
            })
        });
        ui.prepare(&mut runtime, [32., 32.], &mut painter).unwrap();
        let passes = painter.layer_stats().render_passes;
        {
            let mut frame = target.begin_frame().unwrap();
            assert!(matches!(
                painter.compose(&ui, &mut frame, 1., |_, _| Ok(())),
                Err(UiError::InvalidGeometry)
            ));
        }
        assert_eq!(painter.layer_stats().render_passes, passes);
        painter.prepare(&ui, &target.render_format(), 1.).unwrap();
        painter.forget(&ui);
        assert_eq!(painter.layer_stats().live_layers, 0);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn fractional_srgb_groups_preserve_viewport_scissor_and_placement_ownership() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = graphics
            .create_framebuffer(
                FramebufferOptions::new(64, 64)
                    .format(wgpu::TextureFormat::Rgba8UnormSrgb)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let description = stack().size(32., 32.).clip().child(
            column()
                .absolute()
                .left(4.25)
                .top(4.25)
                .size(16., 16.)
                .opacity(0.5)
                .background([1., 0., 0., 1.])
                .child(
                    column()
                        .absolute()
                        .left(-8.)
                        .top(4.)
                        .size(8., 8.)
                        .background([0., 1., 0., 1.]),
                ),
        );
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Page(description)));
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut other = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::new(&graphics);
        for ui in [&mut ui, &mut other] {
            ui.prepare(&mut runtime, [32., 32.], &mut painter).unwrap();
            painter.prepare(ui, &target.render_format(), 1.5).unwrap();
        }
        assert_eq!(painter.layer_stats().live_layers, 2);
        painter.forget(&other);
        assert_eq!(painter.layer_stats().live_layers, 1);
        let texture = target.color_texture().unwrap().clone();
        let mut frame = target.begin_frame().unwrap();
        // Raw painting cannot silently apply incorrect per-draw opacity.
        {
            let mut pass = frame
                .render_pass()
                .clear_color(wgpu::Color::BLUE)
                .begin()
                .unwrap();
            assert!(matches!(
                painter.paint(&ui, &mut pass, 1.5),
                Err(UiError::CompositionRequired)
            ));
        }
        painter
            .compose(&ui, &mut frame, 1.5, |frame, ui| {
                let mut pass = frame.render_pass().clear_color(wgpu::Color::BLUE).begin()?;
                pass.set_viewport(6., 4., 48., 48., 0., 1.)?;
                pass.set_scissor_rect(8, 6, 40, 40)?;
                let viewport = pass.viewport();
                ui.paint(&mut pass)?;
                assert_eq!(pass.viewport(), viewport);
                assert_eq!(pass.scissor_rect(), [8, 6, 40, 40]);
                Ok(())
            })
            .unwrap();
        let buffer = read_pixel(&graphics, &mut frame, &texture);
        let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
        near(&bytes, 20, 20, [188, 0, 188, 255]); // blending is linear, storage is sRGB
        near(&bytes, 9, 22, [0, 188, 188, 255]); // negative child overflow survives cropped origin
        near(&bytes, 6, 22, [0, 0, 255, 255]); // caller scissor stays authoritative
        near(&bytes, 40, 22, [0, 0, 255, 255]);
        painter.forget(&ui);
        assert_eq!(painter.layer_stats().live_layers, 0);
        assert!(errors.pop().await.is_none());
    });
}

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn zero_opacity_preserves_unrelated_text_across_layer_to_direct_transitions() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = target(&graphics, 1);
        let description = |alpha| {
            stack()
                .size(64., 64.)
                .child(
                    label("Above")
                        .key("above")
                        .absolute()
                        .top(0.)
                        .font_size(8.)
                        .color([1.; 4]),
                )
                .child(
                    button(format!("{alpha}"))
                        .key("control")
                        .absolute()
                        .top(12.)
                        .size(64., 12.)
                        .padding(0.)
                        .border(0., [0.; 4])
                        .font_size(8.),
                )
                .child(
                    label("Outside")
                        .key("outside")
                        .absolute()
                        .top(24.)
                        .font_size(8.)
                        .color([1.; 4]),
                )
                .child(
                    column()
                        .key("fade")
                        .absolute()
                        .top(36.)
                        .size(16., 12.)
                        .opacity(alpha)
                        .background([1., 0., 0., 1.])
                        .child(label("In").font_size(8.)),
                )
                .child(
                    label("Below")
                        .key("below")
                        .absolute()
                        .top(50.)
                        .font_size(8.)
                        .color([1.; 4]),
                )
        };
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Page(description(0.5))));
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut painter = UiPainter::new(&graphics);
        painter
            .fonts_mut()
            .load_font_shared(std::sync::Arc::<[u8]>::from(
                include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice(),
            ))
            .unwrap();
        prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
        let baseline = draw(&mut painter, &ui, &graphics, &mut target, 1.);
        assert!(
            baseline[24 * 256..36 * 256]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 20 && p[1] > 20)
        );
        for alpha in [0.75, 1., 0., 0.5] {
            runtime.update(|cx| root.update(cx, |s, _| s.0 = description(alpha)));
            prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
            let bytes = draw(&mut painter, &ui, &graphics, &mut target, 1.);
            assert_eq!(
                &bytes[24 * 256..36 * 256],
                &baseline[24 * 256..36 * 256],
                "outside text with alpha {alpha}"
            );
            assert_eq!(
                &bytes[50 * 256..60 * 256],
                &baseline[50 * 256..60 * 256],
                "following text with alpha {alpha}"
            );
        }
        assert!(errors.pop().await.is_none());
    });
}
