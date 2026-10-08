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

struct RoundedClipView;
impl View for RoundedClipView {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        row()
            .child(
                column()
                    .size(40., 40.)
                    .radius(20.)
                    .clip()
                    .child(column().size(40., 40.).background([1., 0., 0., 1.])),
            )
            // Scrolled content stays inside the rounded clip.
            .child(
                column()
                    .size(40., 40.)
                    .corner_radii(0., 16., 0., 0.)
                    .scroll_y()
                    .child(column().size(40., 80.).background([0., 1., 0., 1.])),
            )
    }
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn rounded_clipping_containers_round_descendant_painting() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| RoundedClipView));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::new(&graphics);
        let mut pixels = render(&graphics, &mut runtime, &mut ui, &mut painter, [80, 40]);
        for scrolled in [false, true] {
            let at = |x, y| pixel(&pixels, 80, x, y);
            assert_eq!(at(1, 1), [0, 0, 0, 255]); // Clipped corner.
            assert_eq!(at(38, 38), [0, 0, 0, 255]);
            assert_eq!(at(20, 20), [255, 0, 0, 255]);
            assert_eq!(at(20, 1), [255, 0, 0, 255]); // Edge midpoint stays inside.
            assert_eq!(at(78, 1), [0, 0, 0, 255], "scrolled {scrolled}");
            assert_eq!(at(42, 1), [0, 255, 0, 255]);
            assert_eq!(at(78, 38), [0, 255, 0, 255]);
            ui.scroll([60., 20.], [0., 20.]).unwrap();
            pixels = render(&graphics, &mut runtime, &mut ui, &mut painter, [80, 40]);
        }
        assert!(errors.pop().await.is_none());
    });
}

struct Typography;
impl View for Typography {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .width(300.)
            .child(label("Start").key("start").fill_width())
            .child(
                label("Center")
                    .key("center")
                    .fill_width()
                    .text_align(crate::TextAlign::Center),
            )
            .child(label("Tall").key("tall").line_height(3.))
    }
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn text_alignment_and_line_height_reach_the_shaped_layout() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Typography));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::new(&graphics);
        painter
            .fonts_mut()
            .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
            .unwrap();
        render(&graphics, &mut runtime, &mut ui, &mut painter, [300, 200]);
        let ink = |key: &str| {
            let id = ui
                .elements()
                .find(|e| e.key == Some(&Key::from(key)))
                .unwrap()
                .id;
            painter.texts[&id]
                .prepared
                .as_ref()
                .unwrap()
                .ink_bounds()
                .unwrap()
        };
        assert!(ink("start").x < 5.);
        let center = ink("center");
        assert!(
            (center.x + center.width / 2. - 150.).abs() < 2.,
            "{center:?}"
        );
        let tall = ui
            .elements()
            .find(|e| e.key == Some(&Key::from("tall")))
            .unwrap();
        assert_eq!(tall.bounds.height, tall.font_size * 3.);
    });
}

/// A bar whose prepare pass counts preparations in its retained state.
#[derive(PartialEq)]
struct Bar {
    fraction: f32,
}
#[derive(Default)]
struct BarState {
    prepared: u32,
}
impl CustomElement for Bar {
    type State = BarState;
    fn measure(&self, request: CustomMeasure) -> [f32; 2] {
        [request.known[0].unwrap_or(40.), 20.]
    }
    fn prepare(
        &self,
        state: &mut BarState,
        _: &ElementInfo<'_>,
        cx: &mut CustomPrepare<'_>,
    ) -> Result<(), UiError> {
        assert_eq!(cx.raster_scale, 1.);
        state.prepared += 1;
        Ok(())
    }
    fn paint(
        &self,
        state: &BarState,
        element: &ElementInfo<'_>,
        paint: &mut astrelis::PaintSession<'_, '_>,
    ) -> Result<(), UiError> {
        assert!(state.prepared > 0);
        let b = element.content_bounds;
        paint.fill_rect(
            Rect::new(b.x, b.y, b.width * self.fraction, b.height),
            element.color,
        )?;
        Ok(())
    }
}
struct Bars;
impl View for Bars {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .padding(10.)
            .color([0., 1., 0., 1.])
            .child(custom(Bar { fraction: 0.5 }).key("bar"))
            .child(custom(Bar { fraction: 1. }).opacity(0.5))
    }
}
#[test]
#[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
fn custom_elements_prepare_retained_state_and_paint_in_logical_units() {
    pollster::block_on(async {
        let graphics = GraphicsContext::headless().await.unwrap();
        let errors = graphics
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Bars));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::new(&graphics);
        let pixels = render(&graphics, &mut runtime, &mut ui, &mut painter, [60, 60]);
        let at = |x, y| pixel(&pixels, 60, x, y);
        assert_eq!(at(15, 20), [0, 255, 0, 255]);
        assert_eq!(at(35, 20), [0, 0, 0, 255]); // Past the half-filled bar.
        let faded = at(45, 40); // Second bar, inside an opacity layer.
        assert!(faded[1] > 100 && faded[1] < 160, "{faded:?}");
        let id = ui
            .elements()
            .find(|e| e.key == Some(&Key::from("bar")))
            .unwrap()
            .id;
        render(&graphics, &mut runtime, &mut ui, &mut painter, [60, 60]);
        let state = painter.customs[&id].downcast_ref::<BarState>().unwrap();
        assert_eq!(state.prepared, 2);
        painter.forget(&ui);
        assert!(painter.customs.is_empty());
        assert!(errors.pop().await.is_none());
    });
}
