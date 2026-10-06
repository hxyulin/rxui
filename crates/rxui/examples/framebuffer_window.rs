//! Standalone app-owned offscreen chart. Preparation owns allocation, resize and
//! point uploads; recording uses the host's encoder, followed by the UI image pass.
//! The same Image identity follows the resolved framebuffer output across resize/MSAA.
use rxui::{
    astrelis::{
        Framebuffer, FramebufferOptions, Painter, Point2D, PointBuffer, PointBufferOptions,
        PolylineDraw, Rect, Transform2D, wgpu,
    },
    prelude::*,
};
use std::{cell::RefCell, rc::Rc};

struct Chart {
    image: Option<Image>,
    revision: u32,
    msaa: bool,
}
impl View for Chart {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut content = column()
            .fill_width()
            .fill_height()
            .padding(24.)
            .gap(16.)
            .scroll_y()
            .child(
                label("Application-owned GPU chart")
                    .font_size(26.)
                    .accessibility_role(SemanticRole::Heading),
            )
            .child(
                label("4096 retained points · GPU output displayed as an ordinary image")
                    .color(ThemeColor::TextMuted),
            );
        if let Some(source) = &self.image {
            content = content.child(
                image(source.clone())
                    .key("chart")
                    .width(640.)
                    .height(260.)
                    .fit(ImageFit::Stretch)
                    .accessibility_label(format!("Sine chart, dataset {}", self.revision)),
            );
        }
        content.child(row().gap(12.)
            .child(button("Next dataset").variant(ButtonVariant::Primary).on_click(cx.listener(|s, _, _| s.revision += 1)))
            .child(button(if self.msaa { "Disable MSAA" } else { "Enable MSAA if supported" })
                .on_click(cx.listener(|s, _, _| s.msaa = !s.msaa))))
            .child(label("Resize the window or move it between displays; the chart backing follows DPI without changing its logical size."))
    }
}
struct GpuChart {
    framebuffer: Framebuffer,
    painter: Painter,
    points: PointBuffer,
    revision: Option<u32>,
}
fn main() -> Result<(), ApplicationError> {
    let model: Rc<RefCell<Option<Entity<Chart>>>> = Rc::new(RefCell::new(None));
    let gpu: Rc<RefCell<Option<GpuChart>>> = Rc::new(RefCell::new(None));
    let prepare_model = model.clone();
    let prepare_gpu = gpu.clone();
    let render_gpu = gpu.clone();
    Application::new()
        .prepare_graphics(move |graphics, cx| {
            let root = prepare_model.borrow().as_ref().unwrap().clone();
            let (revision, msaa) = {
                let state = root.read(cx);
                (state.revision, state.msaa)
            };
            let scale = graphics.metrics.scale_factor() as f32;
            let size = [(640. * scale).round() as u32, (260. * scale).round() as u32];
            let mut slot = prepare_gpu.borrow_mut();
            if slot.is_none() {
                let framebuffer = graphics
                    .graphics
                    .create_framebuffer(FramebufferOptions::new(size[0], size[1]))?;
                let image = Image::from_framebuffer(&framebuffer);
                *slot = Some(GpuChart {
                    framebuffer,
                    painter: Painter::new(graphics.graphics),
                    points: graphics
                        .graphics
                        .create_point_buffer(PointBufferOptions::new(4096))?,
                    revision: None,
                });
                root.update(cx, |state, _| state.image = Some(image));
            }
            let state = slot.as_mut().unwrap();
            state.framebuffer.resize(size[0], size[1])?;
            let samples = if msaa && state.framebuffer.supported_sample_counts().contains(&4) {
                4
            } else {
                1
            };
            state.framebuffer.set_sample_count(samples)?;
            if state.revision != Some(revision) {
                let values: Vec<_> = (0..4096)
                    .map(|i| {
                        let x = i as f32 / 4095.;
                        Point2D::new([
                            x,
                            (x * 24. + revision as f32 * 0.7).sin() * 0.7 + (x * 100.).sin() * 0.1,
                        ])
                    })
                    .collect();
                state.points.replace(&values)?;
                state.revision = Some(revision);
            }
            state.painter.prepare(&state.framebuffer.render_format())?;
            state
                .painter
                .prepare_points(&state.framebuffer.render_format())?;
            Ok(())
        })
        .render_graphics(move |_, _, frame| {
            let mut slot = render_gpu.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return Ok(());
            };
            let [width, height] = state.framebuffer.size().map(|v| v as f32);
            let mut pass = frame
                .render_to(&mut state.framebuffer)
                .clear_color(wgpu::Color {
                    r: 0.015,
                    g: 0.02,
                    b: 0.025,
                    a: 1.,
                })
                .begin()?;
            let mut paint = state.painter.begin(&mut pass)?;
            for i in 1..5 {
                paint.fill_rect(
                    Rect::new(0., height * i as f32 / 5., width, 1.),
                    [0.12, 0.14, 0.16, 1.],
                )?;
            }
            paint.draw_polyline(
                &state.points,
                PolylineDraw::new([0.2, 0.8, 1., 1.])
                    .width_pixels(2.)
                    .transform(Transform2D::from([
                        width - 16.,
                        0.,
                        0.,
                        -height * 0.4,
                        8.,
                        height * 0.5,
                    ])),
            )?;
            Ok(())
        })
        .run(move |cx| {
            let root = cx.new(|_| Chart {
                image: None,
                revision: 0,
                msaa: false,
            });
            *model.borrow_mut() = Some(root.clone());
            cx.open_window(
                WindowOptions::new()
                    .title("RXUI — framebuffer chart")
                    .size(700., 560.),
                root,
            )?;
            Ok(())
        })
}
