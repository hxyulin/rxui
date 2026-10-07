use super::image_gpu_tests::{pixels, read_pixel};
use super::*;
use crate::*;
use astrelis::{FramebufferOptions, wgpu};

struct Controls {
    position: SplitPosition,
    handle: ScrollHandle,
}
impl View for Controls {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        split_row(
            scroll_area(
                column().children(
                    [
                        [1., 0., 0., 1.],
                        [0., 1., 0., 1.],
                        [1., 0., 0., 1.],
                        [0., 1., 0., 1.],
                    ]
                    .into_iter()
                    .map(|c| column().height(32.).fill_width().background(c)),
                ),
            )
            .handle(self.handle.clone())
            .into_element()
            .opacity(0.5),
            column()
                .fill_width()
                .fill_height()
                .background([0., 0., 1., 1.]),
        )
        .size(64., 64.)
        .position(self.position)
        .min_first(16.)
        .min_second(16.)
        .on_resize(cx.listener(|s, e: &ResizeEvent, _| s.position = e.position))
    }
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn scroll_thumbs_split_lines_and_isolated_clipping_follow_live_geometry() {
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
            cx.new(|_| Controls {
                position: SplitPosition::Pixels(24.),
                handle: ScrollHandle::new(),
            })
        });
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        ui.set_theme(Theme::dark().colors(|c| {
            c.surface = [0., 0., 0., 1.];
            c.border = [1.; 4];
        }))
        .unwrap();
        let mut painter = UiPainter::new(&graphics);
        ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
        let stats = ui.stats();
        for scrolled in [false, true] {
            if scrolled {
                assert!(ui.scroll([4., 10.], [0., 32.]).unwrap());
            }
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let texture = target.color_texture().unwrap().clone();
            let mut frame = target.begin_frame().unwrap();
            painter
                .compose(&ui, &mut frame, 1., |frame, composed| {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()?;
                    pass.set_scissor_rect(4, 4, 56, 56)?;
                    composed.paint(&mut pass)?;
                    assert_eq!(pass.scissor_rect(), [4, 4, 56, 56]);
                    Ok(())
                })
                .unwrap();
            let buffer = read_pixel(&graphics, &mut frame, &texture);
            let bytes = pixels(&graphics, &buffer, frame.finish().unwrap());
            let at = |x: usize, y: usize| &bytes[y * 256 + x * 4..y * 256 + x * 4 + 4];
            assert_eq!(
                at(4, 10),
                if scrolled {
                    [0, 128, 0, 255]
                } else {
                    [128, 0, 0, 255]
                }
            );
            assert_eq!(at(48, 10), [0, 0, 255, 255]);
            assert_eq!(at(27, 10), [255; 4]);
            assert_eq!(
                at(18, 10),
                if scrolled {
                    [0, 0, 0, 255]
                } else {
                    [128, 128, 128, 255]
                }
            );
            assert_eq!(at(2, 10), [0, 0, 0, 255]);
            assert_eq!(ui.stats(), stats);
        }
        ui.pointer(&mut runtime, PointerEvent::Pressed([27., 10.]))
            .unwrap();
        ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
        ui.pointer(&mut runtime, PointerEvent::Moved([35., 10.]))
            .unwrap();
        ui.prepare(&mut runtime, [64.; 2], &mut painter).unwrap();
        assert_eq!(
            ui.elements()
                .find(|n| n.kind == ElementType::Splitter)
                .unwrap()
                .range
                .unwrap()
                .value,
            32.
        );
        painter.prepare(&ui, &target.render_format(), 1.).unwrap();
        let mut frame = target.begin_frame().unwrap();
        painter
            .compose(&ui, &mut frame, 1., |frame, composed| {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::BLACK)
                    .begin()?;
                composed.paint(&mut pass)
            })
            .unwrap();
        frame.finish().unwrap();
        assert!(errors.pop().await.is_none());
    });
}
