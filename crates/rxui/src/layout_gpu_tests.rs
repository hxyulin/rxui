use super::image_gpu_tests::{Page, pixels, prepare, read_pixel};
use super::*;
use crate::*;
use astrelis::{FramebufferOptions, wgpu};
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn stacked_subtrees_paint_in_scoped_z_order_and_restore_caller_clipping() {
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
        let description = |z| {
            stack()
                .size(32., 32.)
                .clip()
                .child(
                    column()
                        .key("red")
                        .size(40., 40.)
                        .background([1., 0., 0., 1.])
                        .child(
                            column()
                                .absolute()
                                .left(20.)
                                .top(20.)
                                .size(12., 12.)
                                .z_index(999)
                                .background([0., 0., 1., 1.]),
                        ),
                )
                .child(
                    column()
                        .key("green")
                        .absolute()
                        .left(16.)
                        .top(16.)
                        .size(40., 40.)
                        .z_index(z)
                        .background([0., 1., 0., 1.]),
                )
        };
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Page(description(-1))));
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        let mut painter = UiPainter::new(&graphics);
        prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
        let layouts = ui.stats().layout_passes;
        for (z, expected) in [(-1, [0, 0, 255, 255]), (1, [0, 255, 0, 255])] {
            runtime.update(|cx| root.update(cx, |s, _| s.0 = description(z)));
            prepare(&mut runtime, &mut ui, &mut painter, &target.render_format());
            assert_eq!(ui.stats().layout_passes, layouts);
            let texture = target.color_texture().unwrap().clone();
            let mut frame = target.begin_frame().unwrap();
            {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::BLACK)
                    .begin()
                    .unwrap();
                pass.set_scissor_rect(4, 4, 56, 56).unwrap();
                painter.paint(&ui, &mut pass, 1.).unwrap();
                assert_eq!(pass.scissor_rect(), [4, 4, 56, 56]);
            }
            let buffer = read_pixel(&graphics, &mut frame, &texture);
            let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
            let at = |x: usize, y: usize| &bytes[y * 256 + x * 4..y * 256 + x * 4 + 4];
            assert_eq!(at(22, 22), expected);
            assert_eq!(at(10, 10), [255, 0, 0, 255]);
            assert_eq!(at(2, 10), [0, 0, 0, 255]);
            assert_eq!(at(40, 40), [0, 0, 0, 255]);
        }
        assert!(errors.pop().await.is_none());
    });
}
