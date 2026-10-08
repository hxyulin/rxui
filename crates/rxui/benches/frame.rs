//! Per-redraw CPU work with real shaping on a headless device: the input or state
//! change, `Ui::prepare` and `UiPainter::prepare`, as the native host runs them for
//! one frame. Encoding, submission and presentation are not timed, except in
//! `idle_encode`, which adds CPU encoding of the composed frame.
//! Pass `--accessibility` to add an AccessKit update after each frame. Set
//! `RXUI_BENCH_LOOP=<operation>` to repeat one 1,000-row operation forever, for
//! attaching a sampling profiler.
use rxui::{
    AccessKitTree, ElementType, PointerEvent, PointerInput, TextInputEvent, UiPainter,
    astrelis::{FramebufferOptions, GraphicsContext, wgpu},
    prelude::*,
};
use std::{hint::black_box, time::Duration, time::Instant};

struct Rows {
    count: usize,
    /// Adds a pointer-move listener that leaves the state unchanged.
    track: bool,
}
impl View for Rows {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let rows = column()
            .fill_width()
            .children((0..self.count).map(|i| button(format!("Row {i}")).key(i).height(24.)));
        if self.track {
            rows.on_pointer_move(cx.listener(|_, _: &PointerInput, cx| cx.unchanged()))
        } else {
            rows
        }
    }
}
struct App {
    rows: Entity<Rows>,
    value: String,
    animate: bool,
}
impl View for App {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let alpha = if self.animate {
            cx.request_animation_frame();
            0.5 + 0.4 * (cx.frame_time().as_secs_f32() * 10.).sin()
        } else {
            1.
        };
        column()
            .size(800., 600.)
            .child(
                text_input(self.value.clone())
                    .key("input")
                    .fill_width()
                    .on_change(
                        cx.listener(|this, e: &TextChangeEvent, _| this.value = e.value.clone()),
                    ),
            )
            .child(label("Status").key("status").opacity(alpha))
            .child(scroll_area(self.rows.clone()))
    }
}
fn sample(case: &str, count: usize, mut f: impl FnMut()) {
    if let Ok(only) = std::env::var("RXUI_BENCH_LOOP") {
        if only == case && count == 1000 {
            loop {
                f();
            }
        }
        return;
    }
    for _ in 0..20 {
        f();
    }
    let mut times = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        for _ in 0..20 {
            f();
        }
        times.push(start.elapsed().as_secs_f64() * 1e6 / 20.);
    }
    times.sort_by(f64::total_cmp);
    println!("{case},{count},20,20,{:.3},{:.3}", times[10], times[18]);
}
fn main() {
    let accessibility = std::env::args().any(|a| a == "--accessibility");
    let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
    let mut target = graphics
        .create_framebuffer(
            FramebufferOptions::new(800, 600).format(wgpu::TextureFormat::Rgba8Unorm),
        )
        .unwrap();
    let format = target.render_format();
    println!("operation,rows,samples,iterations_per_sample,median_us,p95_us");
    for count in [100, 1000] {
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| {
            let rows = cx.new(|_| Rows {
                count,
                track: false,
            });
            cx.new(|_| App {
                rows,
                value: "Hello".into(),
                animate: false,
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
        let mut tree = AccessKitTree::new();
        let mut time = Duration::ZERO;
        let mut frame = |runtime: &mut Runtime, ui: &mut Ui<App>, painter: &mut UiPainter| {
            time += Duration::from_millis(16);
            runtime.begin_frame(time);
            ui.prepare(runtime, [800., 600.], painter).unwrap();
            painter.prepare(ui, &format, 1.).unwrap();
            if accessibility {
                black_box(tree.update(ui, "Benchmark", 1.).unwrap());
            }
        };
        frame(&mut runtime, &mut ui, &mut painter);
        sample("idle", count, || frame(&mut runtime, &mut ui, &mut painter));
        sample("idle_encode", count, || {
            frame(&mut runtime, &mut ui, &mut painter);
            let mut encoder = target.begin_frame().unwrap();
            painter
                .compose(&ui, &mut encoder, 1., |frame, ui| {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()?;
                    ui.paint(&mut pass)
                })
                .unwrap();
            black_box(&encoder);
        });
        let rows: Vec<_> = ui
            .elements()
            .filter(|e| e.kind == ElementType::Button)
            .take(2)
            .map(|e| [e.bounds.x + 4., e.bounds.y + 4.])
            .collect();
        let mut next = 0;
        sample("hover_move", count, || {
            ui.pointer(&mut runtime, PointerEvent::Moved(rows[next]))
                .unwrap();
            next = 1 - next;
            frame(&mut runtime, &mut ui, &mut painter);
        });
        let list = runtime.update(|cx| root.read(cx).rows.clone());
        runtime.update(|cx| list.update(cx, |rows, _| rows.track = true));
        frame(&mut runtime, &mut ui, &mut painter);
        sample("pointer_listener_move", count, || {
            let [x, y] = rows[0];
            ui.pointer(&mut runtime, PointerEvent::Moved([x + next as f32, y]))
                .unwrap();
            next = 1 - next;
            frame(&mut runtime, &mut ui, &mut painter);
        });
        runtime.update(|cx| list.update(cx, |rows, _| rows.track = false));
        ui.pointer(&mut runtime, PointerEvent::Left).unwrap();
        frame(&mut runtime, &mut ui, &mut painter);
        let mut direction = 1.;
        sample("wheel_scroll", count, || {
            ui.wheel(
                &mut runtime,
                rows[0],
                [0., direction * 24.],
                Modifiers::default(),
            )
            .unwrap();
            direction = -direction;
            frame(&mut runtime, &mut ui, &mut painter);
        });
        let input = ui
            .elements()
            .find(|e| e.kind == ElementType::TextInput)
            .unwrap()
            .id;
        assert!(ui.focus(input));
        frame(&mut runtime, &mut ui, &mut painter);
        let mut insert = true;
        sample("type_character", count, || {
            let event = if insert {
                TextInputEvent::Insert("a".into())
            } else {
                TextInputEvent::Backspace
            };
            insert = !insert;
            assert!(ui.text_input(&mut runtime, event, &mut painter).unwrap());
            frame(&mut runtime, &mut ui, &mut painter);
        });
        let (dark, light) = (Theme::dark(), Theme::light());
        let mut light_next = true;
        sample("theme_switch", count, || {
            ui.set_theme(if light_next {
                light.clone()
            } else {
                dark.clone()
            })
            .unwrap();
            light_next = !light_next;
            frame(&mut runtime, &mut ui, &mut painter);
        });
        runtime.update(|cx| root.update(cx, |app, _| app.animate = true));
        sample("opacity_animation", count, || {
            frame(&mut runtime, &mut ui, &mut painter);
        });
        runtime.update(|cx| root.update(cx, |app, _| app.animate = false));
        frame(&mut runtime, &mut ui, &mut painter);
    }
}
