//! Headless composition benchmark. Reports CPU preparation/encoding separately from
//! submit-and-wait wall time; the latter includes driver/queue overhead, not GPU timestamps.
use rxui::{
    Element, UiPainter,
    astrelis::{FramebufferOptions, GraphicsContext, wgpu},
    prelude::*,
};
use std::{hint::black_box, time::Instant};
#[derive(Clone, Copy)]
enum Mode {
    Direct,
    One,
    Many,
    Nested,
}
struct Page {
    count: usize,
    mode: Mode,
    alpha: f32,
    text: bool,
}
impl View for Page {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let leaf = |i| {
            let mut element = column()
                .key(i)
                .absolute()
                .left((i % 40) as f32 * 20.)
                .top((i / 40) as f32 * 20.)
                .size(16., 16.)
                .background([0.2, 0.5, 0.8, 1.])
                .opacity(if matches!(self.mode, Mode::Many) {
                    self.alpha
                } else {
                    1.
                });
            if self.text {
                element = element.child(label("Rx").font_size(6.));
            }
            element
        };
        let content = stack()
            .key("content")
            .size(800., 600.)
            .children((0..self.count).map(leaf));
        match self.mode {
            Mode::Direct | Mode::Many => content,
            Mode::One => content.opacity(self.alpha),
            Mode::Nested => stack()
                .size(800., 600.)
                .opacity(self.alpha)
                .child(content.opacity(0.5)),
        }
    }
}
fn sample(case: &str, count: usize, mut f: impl FnMut()) {
    for _ in 0..10 {
        f();
    }
    for batch in 0..10 {
        let start = Instant::now();
        for _ in 0..20 {
            f();
        }
        println!(
            "{case},{count},{batch},{:.6}",
            start.elapsed().as_secs_f64() * 1e6 / 20.
        );
    }
}
fn main() {
    let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
    eprintln!("adapter: {:?}", graphics.adapter().get_info());
    println!("case,count,batch,us_per_op");
    let text = std::env::args().any(|a| a == "--text-only");
    let counts = if text {
        vec![1000]
    } else {
        vec![16, 256, 1000]
    };
    let modes = if text {
        vec![(Mode::Direct, "text_direct"), (Mode::One, "text_one_group")]
    } else {
        vec![
            (Mode::Direct, "direct"),
            (Mode::One, "one_group"),
            (Mode::Nested, "nested"),
            (Mode::Many, "many_groups"),
        ]
    };
    for count in counts {
        for &(mode, name) in &modes {
            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| {
                cx.new(|_| Page {
                    count,
                    mode,
                    alpha: 0.5,
                    text,
                })
            });
            let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
            let mut painter = UiPainter::new(&graphics);
            if text {
                painter
                    .fonts_mut()
                    .load_font_shared(std::sync::Arc::<[u8]>::from(
                        include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice(),
                    ))
                    .unwrap();
            }
            let mut target = graphics
                .create_framebuffer(
                    FramebufferOptions::new(800, 600).format(wgpu::TextureFormat::Rgba8Unorm),
                )
                .unwrap();
            ui.prepare(&mut runtime, [800., 600.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let stats = ui.stats();
            let allocations = painter.layer_stats().allocations;
            let glyph_bytes = painter.painter().text().stats().geometry_bytes;
            sample(&format!("{name}_prepare_cached"), count, || {
                painter.prepare(&ui, &target.render_format(), 1.).unwrap();
                black_box(painter.layer_stats());
            });
            sample(&format!("{name}_update_prepare"), count, || {
                runtime.update(|cx| {
                    root.update(cx, |s, _| s.alpha = if s.alpha == 0.5 { 0.6 } else { 0.5 })
                });
                ui.prepare(&mut runtime, [800., 600.], &mut painter)
                    .unwrap();
                painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            });
            assert_eq!(ui.stats().layout_passes, stats.layout_passes);
            assert_eq!(ui.stats().measurements, stats.measurements);
            assert_eq!(painter.layer_stats().allocations, allocations);
            assert_eq!(painter.painter().text().stats().geometry_bytes, glyph_bytes);
            sample(&format!("{name}_encode_discard"), count, || {
                let mut frame = target.begin_frame().unwrap();
                painter
                    .compose(&ui, &mut frame, 1., |frame, ui| {
                        let mut pass = frame
                            .render_pass()
                            .clear_color(wgpu::Color::BLACK)
                            .begin()?;
                        ui.paint(&mut pass)
                    })
                    .unwrap();
                black_box(&frame);
            });
            sample(&format!("{name}_submit_wait"), count, || {
                let mut frame = target.begin_frame().unwrap();
                painter
                    .compose(&ui, &mut frame, 1., |frame, ui| {
                        let mut pass = frame
                            .render_pass()
                            .clear_color(wgpu::Color::BLACK)
                            .begin()?;
                        ui.paint(&mut pass)
                    })
                    .unwrap();
                let submission = frame.finish().unwrap();
                graphics
                    .device()
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(std::time::Duration::from_secs(10)),
                    })
                    .unwrap();
            });
            eprintln!("{name},{count}: {:?}", painter.layer_stats());
        }
    }
    black_box(std::mem::size_of::<Element>());
}
