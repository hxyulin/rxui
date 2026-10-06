//! Warm retained image/content CPU work; optional GPU resource preparation with --gpu.
//! Text metrics are deterministic mocks. No decoding, GPU drawing or presentation is timed.
use rxui::{ElementId, TextMeasure, TextRequest, UiError, prelude::*};
use std::{hint::black_box, time::Instant};
#[derive(Clone, Copy)]
enum Content {
    Image,
    Caption,
    Composed,
}
struct Gallery {
    count: usize,
    image: Image,
    content: Content,
    tinted: bool,
}
impl View for Gallery {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().children((0..self.count).map(|i| {
            let icon = || {
                image(self.image.clone())
                    .width(20.)
                    .height(20.)
                    .tint(if self.tinted {
                        rxui::StyleColor::from(ThemeColor::TextMuted)
                    } else {
                        rxui::StyleColor::from([1.; 4])
                    })
            };
            match self.content {
                Content::Image => icon().key(i),
                Content::Caption => button("Save").key(i),
                Content::Composed => button(
                    row()
                        .gap(8.)
                        .child(icon().accessibility_hidden(true))
                        .child(label("Save")),
                )
                .key(i),
            }
        }))
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([request.text.len() as f32 * 8., 20.])
    }
}
fn time(case: &str, count: usize, mut f: impl FnMut()) {
    for _ in 0..20 {
        f();
    }
    for batch in 0..20 {
        let start = Instant::now();
        for _ in 0..50 {
            f();
        }
        println!(
            "{case},{count},{batch},{:.6}",
            start.elapsed().as_secs_f64() * 1_000_000. / 50.
        );
    }
}
fn main() {
    println!("case,count,batch,us_per_op");
    let asset = Image::from_rgba8(32, 16, vec![255; 32 * 16 * 4]).unwrap();
    for count in [16, 256, 1000] {
        for (content, name) in [
            (Content::Image, "images"),
            (Content::Caption, "captions"),
            (Content::Composed, "composed"),
        ] {
            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| {
                cx.new(|_| Gallery {
                    count,
                    image: asset.clone(),
                    content,
                    tinted: false,
                })
            });
            let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
            ui.prepare(&mut runtime, [800., 600.], &mut Measure)
                .unwrap();
            let stats = ui.stats();
            time(&format!("{name}_idle"), count, || {
                ui.prepare(&mut runtime, [800., 600.], &mut Measure)
                    .unwrap();
                black_box(ui.stats());
            });
            assert_eq!(ui.stats(), stats);
            time(&format!("{name}_description"), count, || {
                runtime.update(|cx| root.update(cx, |_, _| ()));
                ui.prepare(&mut runtime, [800., 600.], &mut Measure)
                    .unwrap();
                black_box(ui.stats());
            });
            assert_eq!(ui.stats().measurements, stats.measurements);
            assert_eq!(ui.stats().layout_passes, stats.layout_passes);
            if matches!(content, Content::Image) {
                time("images_tint", count, || {
                    runtime.update(|cx| root.update(cx, |s, _| s.tinted = !s.tinted));
                    ui.prepare(&mut runtime, [800., 600.], &mut Measure)
                        .unwrap();
                    black_box(ui.elements().next().unwrap().color);
                });
                assert_eq!(ui.stats().measurements, 0);
                assert_eq!(ui.stats().layout_passes, stats.layout_passes);
            }
        }
    }
    #[cfg(feature = "rendering")]
    if std::env::args().any(|a| a == "--gpu") {
        use rxui::{
            UiPainter,
            astrelis::{FramebufferOptions, GraphicsContext},
        };
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        let target = graphics
            .create_framebuffer(FramebufferOptions::new(800, 600))
            .unwrap();
        for count in [16, 256, 1000] {
            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| {
                cx.new(|_| Gallery {
                    count,
                    image: asset.clone(),
                    content: Content::Image,
                    tinted: false,
                })
            });
            let mut ui = Ui::new(&mut runtime, root).unwrap();
            let mut painter = UiPainter::new(&graphics);
            ui.prepare(&mut runtime, [800., 600.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let stats = painter.image_stats();
            assert_eq!(stats.uploads, 1);
            assert_eq!(stats.bindings, 1);
            assert_eq!(stats.uploaded_bytes, 2048);
            time("images_gpu_prepare", count, || {
                painter.prepare(&ui, &target.render_format(), 1.).unwrap();
                black_box(painter.image_stats());
            });
            assert_eq!(painter.image_stats(), stats);
        }
    }
}
