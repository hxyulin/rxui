//! Per-edit CPU work in a text area with real shaping on a headless device: the edit
//! or caret movement, `Ui::prepare` and `UiPainter::prepare`, as the native host runs
//! them for one frame, for documents of 100, 1,000 and 5,000 lines inside a
//! scrolling column. Encoding, submission and presentation are not timed. Pass
//! `--accessibility` to add an AccessKit update after each frame. Set
//! `RXUI_BENCH_LOOP=<operation>` to repeat one 5,000-line operation forever, for
//! attaching a sampling profiler.
use rxui::{
    AccessKitTree, ElementType, SemanticAction, TextInputEvent, TextMovement, TextSelection,
    UiPainter,
    drawing::{FramebufferOptions, GraphicsContext, wgpu},
    prelude::*,
};
use std::{hint::black_box, time::Instant};

struct Doc {
    text: String,
}
impl View for Doc {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().size(800., 600.).scroll_y().child(
            text_area(self.text.clone())
                .key("doc")
                .fill_width()
                .accessibility_label("Document")
                .on_change(cx.listener(|this, e: &TextChangeEvent, _| this.text = e.value.clone())),
        )
    }
}
fn sample(case: &str, lines: usize, mut f: impl FnMut()) {
    if let Ok(only) = std::env::var("RXUI_BENCH_LOOP") {
        if only == case && lines == 5000 {
            loop {
                f();
            }
        }
        return;
    }
    for _ in 0..10 {
        f();
    }
    let mut times = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        for _ in 0..10 {
            f();
        }
        times.push(start.elapsed().as_secs_f64() * 1e6 / 10.);
    }
    times.sort_by(f64::total_cmp);
    println!("{case},{lines},20,10,{:.1},{:.1}", times[10], times[18]);
}
fn main() {
    let accessibility = std::env::args().any(|a| a == "--accessibility");
    let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
    let target = graphics
        .create_framebuffer(
            FramebufferOptions::new(800, 600).format(wgpu::TextureFormat::Rgba8Unorm),
        )
        .unwrap();
    let format = target.render_format();
    println!("operation,lines,samples,iterations_per_sample,median_us,p95_us");
    for lines in [100, 1000, 5000] {
        let text: String = (0..lines)
            .map(|i| format!("Line {i:05}: the quick brown fox jumps over the lazy dog.\n"))
            .collect();
        let middle = text.len() / 2;
        let middle = middle - middle % 61;
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Doc { text }));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        let mut painter = UiPainter::with_graphics(&graphics);
        painter
            .register_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice())
            .unwrap();
        let mut tree = AccessKitTree::new();
        let mut frame = |runtime: &mut Runtime, ui: &mut Ui<Doc>, painter: &mut UiPainter| {
            ui.prepare(runtime, [800., 600.], painter).unwrap();
            painter.prepare(ui, &format, 1.).unwrap();
            if accessibility {
                black_box(tree.update(ui, "Benchmark", 1.).unwrap());
            }
        };
        frame(&mut runtime, &mut ui, &mut painter);
        let id = ui
            .elements()
            .find(|e| e.kind == ElementType::TextInput)
            .unwrap()
            .id;
        assert!(ui.focus(id));
        let caret = |runtime: &mut Runtime, ui: &mut Ui<Doc>, painter: &mut UiPainter, at| {
            let revision = ui.semantic_node(id).unwrap().text_revision;
            ui.semantic_action(
                runtime,
                SemanticAction::SetSelection {
                    target: id,
                    text_revision: revision,
                    selection: TextSelection::caret(at),
                },
                painter,
            )
            .unwrap();
        };
        caret(&mut runtime, &mut ui, &mut painter, middle);
        frame(&mut runtime, &mut ui, &mut painter);
        sample("idle", lines, || frame(&mut runtime, &mut ui, &mut painter));
        let mut down = true;
        sample("caret_vertical", lines, || {
            let movement = if down {
                TextMovement::Down
            } else {
                TextMovement::Up
            };
            down = !down;
            let event = TextInputEvent::Move {
                movement,
                extend: false,
            };
            ui.text_input(&mut runtime, event, &mut painter).unwrap();
            frame(&mut runtime, &mut ui, &mut painter);
        });
        for case in ["type_middle", "type_end"] {
            if case == "type_end" {
                let end = runtime.update(|cx| ui.entity().read(cx).text.len());
                caret(&mut runtime, &mut ui, &mut painter, end);
                frame(&mut runtime, &mut ui, &mut painter);
            }
            let mut insert = true;
            sample(case, lines, || {
                let event = if insert {
                    TextInputEvent::Insert("a".into())
                } else {
                    TextInputEvent::Backspace
                };
                insert = !insert;
                assert!(ui.text_input(&mut runtime, event, &mut painter).unwrap());
                frame(&mut runtime, &mut ui, &mut painter);
            });
        }
        ui.text_input(
            &mut runtime,
            TextInputEvent::Paste("pasted".into()),
            &mut painter,
        )
        .unwrap();
        frame(&mut runtime, &mut ui, &mut painter);
        let mut undo = true;
        sample("undo_redo", lines, || {
            let event = if undo {
                TextInputEvent::Undo
            } else {
                TextInputEvent::Redo
            };
            undo = !undo;
            assert!(ui.text_input(&mut runtime, event, &mut painter).unwrap());
            frame(&mut runtime, &mut ui, &mut painter);
        });
    }
}
