use super::image_gpu_tests::{Page, pixels, prepare, read_pixel};
use super::*;
use crate::*;
use astrelis::{FramebufferOptions, wgpu};

#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn scroll_culling_retains_glyphs_overflow_and_independent_descendants() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut target = graphics
            .create_framebuffer(
                FramebufferOptions::new(64, 64)
                    .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            )
            .unwrap();
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| {
            cx.new(|_| {
                Page(
                    stack()
                        .size(64., 64.)
                        .clip()
                        .child(
                            column()
                                .key("viewport")
                                .absolute()
                                .size(32., 64.)
                                .scroll_y()
                                .clip()
                                .child(
                                    column()
                                        .children((0..1000).map(|i| {
                                            label("row").key(i).font_size(8.).height(16.)
                                        })),
                                ),
                        )
                        // An offscreen layout parent must not suppress its visible child.
                        .child(
                            stack()
                                .absolute()
                                .left(96.)
                                .size(8., 16.)
                                .child(label("D").absolute().left(-60.).top(8.).font_size(12.)),
                        )
                        // Ink can extend beyond a label's zero-height layout box.
                        .child(
                            label("Z")
                                .key("overflow")
                                .absolute()
                                .left(48.)
                                .top(8.)
                                .font_size(12.)
                                .height(0.),
                        )
                        .child(
                            column()
                                .absolute()
                                .left(40.25)
                                .top(40.25)
                                .size(8., 8.)
                                .background([0., 1., 0., 1.]),
                        ),
                )
            })
        });
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::new(&graphics);
        painter
            .fonts_mut()
            .load_font_shared(Arc::<[u8]>::from(
                include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice(),
            ))
            .unwrap();
        prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
        let ui_stats = ui.stats();
        let text_stats = painter.painter().text().stats();
        let overflow = ui
            .elements()
            .find(|e| e.key == Some(&Key::from("overflow")))
            .unwrap();
        assert_eq!(overflow.bounds.height, 0.);
        let texture = target.color_texture().unwrap().clone();
        for scrolled in [false, true] {
            if scrolled {
                assert!(ui.scroll([8., 8.], [0., 1600.125]).unwrap());
                painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            }
            let before = painter.painter().text().stats().draw_calls;
            let mut frame = target.begin_frame().unwrap();
            {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::BLACK)
                    .begin()
                    .unwrap();
                pass.set_scissor_rect(0, 0, 60, 64).unwrap();
                painter.paint(&ui, &mut pass, 1.).unwrap();
                assert_eq!(pass.scissor_rect(), [0, 0, 60, 64]);
            }
            let buffer = read_pixel(&graphics, &mut frame, &texture);
            let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
            let stats = painter.painter().text().stats();
            assert!(
                (6..=8).contains(&(stats.draw_calls - before)),
                "only visible text should draw"
            );
            assert_eq!(stats.geometry_bytes, text_stats.geometry_bytes);
            assert_eq!(stats.uploaded_bytes, text_stats.uploaded_bytes);
            assert_eq!(ui.stats(), ui_stats);
            let has_ink = |left: usize, right: usize| {
                (8..28).any(|y| (left..right).any(|x| bytes[y * 256 + x * 4] > 0))
            };
            assert!(has_ink(36, 46), "visible child of an offscreen parent");
            assert!(
                has_ink(48, 59),
                "glyph overflow beyond a zero-height layout box"
            );
            assert_eq!(
                &bytes[44 * 256 + 44 * 4..44 * 256 + 44 * 4 + 4],
                &[0, 255, 0, 255]
            );
            assert_eq!(
                &bytes[16 * 256 + 62 * 4..16 * 256 + 62 * 4 + 4],
                &[0, 0, 0, 255]
            );
        }
        assert!(errors.pop().await.is_none());
    });
}
