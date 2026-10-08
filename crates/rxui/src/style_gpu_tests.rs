use super::*;
use crate::*;
use astrelis::{FramebufferOptions, wgpu};

/// Prepares and paints one frame into a linear RGBA8 target, returning its pixels.
pub(super) fn render<T: View>(
    graphics: &GraphicsContext,
    runtime: &mut Runtime,
    ui: &mut Ui<T>,
    painter: &mut UiPainter,
    size: [u32; 2],
) -> Vec<u8> {
    let mut target = graphics
        .create_framebuffer(
            FramebufferOptions::new(size[0], size[1])
                .format(wgpu::TextureFormat::Rgba8Unorm)
                .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
        )
        .unwrap();
    ui.prepare(runtime, [size[0] as f32, size[1] as f32], painter)
        .unwrap();
    painter.prepare(ui, &target.render_format(), 1.).unwrap();
    let mut frame = target.begin_frame().unwrap();
    painter
        .compose(ui, &mut frame, 1., |frame, composed| {
            let mut pass = frame
                .render_pass()
                .clear_color(wgpu::Color::BLACK)
                .begin()?;
            composed.paint(&mut pass)
        })
        .unwrap();
    frame.finish().unwrap();
    target.read_rgba8().unwrap()
}
pub(super) fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    pixels[i..i + 4].try_into().unwrap()
}

struct Corners;
impl View for Corners {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        row()
            .child(
                column()
                    .size(40., 40.)
                    .background([1., 0., 0., 1.])
                    .corner_radii(20., 0., 0., 0.),
            )
            .child(
                column()
                    .size(40., 40.)
                    .background([0., 0., 1., 1.])
                    .border_bottom(6.)
                    .border_color([0., 1., 0., 1.])
                    .corner_radii(0., 0., 12., 0.),
            )
    }
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn per_corner_radii_and_single_side_borders_match_pixels() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Corners));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::new(&graphics);
        let pixels = render(&graphics, &mut runtime, &mut ui, &mut painter, [80, 40]);
        let at = |x, y| pixel(&pixels, 80, x, y);
        assert_eq!(at(1, 1), [0, 0, 0, 255]); // Rounded top-left corner.
        assert_eq!(at(38, 1), [255, 0, 0, 255]); // Square top-right corner.
        assert_eq!(at(1, 38), [255, 0, 0, 255]);
        assert_eq!(at(60, 10), [0, 0, 255, 255]);
        assert_eq!(at(60, 37), [0, 255, 0, 255]); // Bottom border only.
        assert_eq!(at(41, 2), [0, 0, 255, 255]); // No left/top border.
        assert_eq!(at(79, 39), [0, 0, 0, 255]); // Rounded bottom-right corner.
        assert!(errors.pop().await.is_none());
    });
}

struct Shadows;
impl View for Shadows {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        stack()
            .size(80., 80.)
            .child(
                column()
                    .absolute()
                    .left(10.)
                    .top(10.)
                    .size(20., 20.)
                    .background([1., 0., 0., 1.])
                    .shadow(BoxShadow::new([1.; 4]).offset(10., 10.)),
            )
            // Own box is outside the target; only its shadow is visible.
            .child(
                column()
                    .absolute()
                    .left(-40.)
                    .top(50.)
                    .size(20., 20.)
                    .background([1., 0., 0., 1.])
                    .shadow(BoxShadow::new([0., 1., 0., 1.]).offset(50., 0.).blur(4.)),
            )
    }
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn box_shadows_paint_behind_backgrounds_and_survive_own_box_culling() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Shadows));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::new(&graphics);
        let pixels = render(&graphics, &mut runtime, &mut ui, &mut painter, [80, 80]);
        let at = |x, y| pixel(&pixels, 80, x, y);
        assert_eq!(at(15, 15), [255, 0, 0, 255]); // Background over its shadow.
        assert_eq!(at(35, 35), [255, 255, 255, 255]); // Offset shadow.
        assert_eq!(at(5, 5), [0, 0, 0, 255]);
        assert_eq!(at(20, 60), [0, 255, 0, 255]); // Shadow center of a culled box.
        let edge = at(20, 50);
        assert!(edge[1] > 0 && edge[1] < 255, "blurred edge {edge:?}");
        assert!(errors.pop().await.is_none());
    });
}
