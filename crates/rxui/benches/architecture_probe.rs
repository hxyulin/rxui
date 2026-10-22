//! Public-API CPU probes: mock text, no GPU, native host, or platform AX consumer.
//! Run: cargo bench -p rxui --bench architecture_probe --features accessibility --locked
use rxui::{AccessKitTree, ElementId, PointerEvent, TextMeasure, TextRequest, UiError, prelude::*};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
struct Busy;
impl View for Busy {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        spinner(cx)
    }
}
struct Page {
    count: usize,
    revision: u64,
    mode: u8,
    busy: Entity<Busy>,
    scroll: ScrollHandle,
}
impl View for Page {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        if self.mode == 3 {
            return virtual_list(self.count, 28., &self.scroll, cx, |i| {
                label(format!("Row {i}")).key(i)
            })
            .fill_width()
            .fill_height()
            .into_element();
        }
        let mut content = column();
        if self.mode == 1 {
            content = content.child(spinner(cx));
        }
        if self.mode == 2 {
            content = content.child(self.busy.clone());
        }
        if self.mode == 4 {
            content = content.child(tooltip(cx, button("Hover").key("hover"), "Help"));
        }
        let content = content.children((0..self.count).map(|i| {
            label(if i == 0 {
                format!("Row {i}: {}", self.revision)
            } else {
                format!("Row {i}")
            })
            .height(28.)
            .key(i)
        }));
        if self.mode == 5 {
            column()
                .size(800., 600.)
                .scroll_y()
                .child(content.fill_width())
        } else {
            content.width(800.).height(600.).scroll_y()
        }
    }
}
fn setup(count: usize, mode: u8) -> (Runtime, Ui<Page>, Entity<Page>) {
    let mut r = Runtime::new();
    r.begin_frame(Duration::ZERO);
    let root = r.update(|cx| {
        let busy = cx.new(|_| Busy);
        cx.new(|_| Page {
            count,
            revision: 0,
            mode,
            busy,
            scroll: ScrollHandle::new(),
        })
    });
    let mut ui = Ui::new(&mut r, root.clone()).unwrap();
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    (r, ui, root)
}
fn run(name: &str, count: usize, mut op: impl FnMut()) {
    for _ in 0..20 {
        op();
    }
    let mut samples = Vec::new();
    for _ in 0..20 {
        let at = Instant::now();
        for _ in 0..20 {
            op();
        }
        samples.push(at.elapsed().as_secs_f64() * 1e6 / 20.);
    }
    samples.sort_by(f64::total_cmp);
    println!("{name},{count},{:.3},{:.3}", samples[10], samples[18]);
}
fn main() {
    if std::env::args().any(|arg| arg == "--nested-only") {
        println!("operation,count,mean_us,built_per_op,sent_per_op");
        for count in [100, 1000, 10000] {
            let (mut r, mut ui, _) = setup(count, 5);
            let mut ax = AccessKitTree::new();
            ax.update(&ui, "Probe", 1.).unwrap();
            let before = ax.stats().built_nodes;
            let mut elapsed = Duration::ZERO;
            let mut sent = 0;
            let mut direction = 1.;
            for _ in 0..100 {
                assert!(ui.scroll([20., 40.], [0., direction]).unwrap());
                direction = -direction;
                ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
                let at = Instant::now();
                let update = ax.update(&ui, "Probe", 1.).unwrap().unwrap();
                elapsed += at.elapsed();
                sent += update.nodes.len();
            }
            println!(
                "ax_nested_scroll,{count},{:.3},{},{}",
                elapsed.as_secs_f64() * 1e6 / 100.,
                (ax.stats().built_nodes - before) / 100,
                sent / 100
            );
        }
        return;
    }
    println!("operation,count,median_us,p95_batch_us");
    for count in [100, 1000, 10000] {
        for (mode, name) in [(1, "spinner_owner_frame"), (2, "spinner_child_frame")] {
            let (mut r, mut ui, _) = setup(count, mode);
            let before = ui.stats();
            let mut ms = 0;
            run(name, count, || {
                ms += 16;
                r.begin_frame(Duration::from_millis(ms));
                ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
                black_box(ui.stats());
            });
            eprintln!("{name} {count}: {:?} -> {:?}", before, ui.stats());
        }
        let (mut r, mut ui, root) = setup(count, 0);
        let mut ax = AccessKitTree::new();
        ax.update(&ui, "Probe", 1.).unwrap();
        let mut total = Duration::ZERO;
        let mut built = 0;
        let mut sent = 0;
        for _ in 0..100 {
            r.update(|cx| root.update(cx, |s, _| s.revision += 1));
            ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
            let before = ax.stats().built_nodes;
            let at = Instant::now();
            let update = ax.update(&ui, "Probe", 1.).unwrap().unwrap();
            total += at.elapsed();
            built += ax.stats().built_nodes - before;
            sent += update.nodes.len();
        }
        println!(
            "ax_one_label_update_mean,{count},{:.3},0",
            total.as_secs_f64() * 1e6 / 100.
        );
        eprintln!(
            "ax_one_label {count}: built/op={}, sent/op={}",
            built / 100,
            sent / 100
        );
        let mut direction = 1.;
        let mut ax_time = Duration::ZERO;
        let before = ax.stats().built_nodes;
        let mut sent = 0;
        for _ in 0..100 {
            assert!(ui.scroll([20., 40.], [0., direction]).unwrap());
            direction = -direction;
            ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
            let at = Instant::now();
            let update = ax.update(&ui, "Probe", 1.).unwrap().unwrap();
            ax_time += at.elapsed();
            sent += update.nodes.len();
        }
        println!(
            "ax_scroll_update_mean,{count},{:.3},0",
            ax_time.as_secs_f64() * 1e6 / 100.
        );
        eprintln!(
            "ax_scroll {count}: built/op={}, sent/op={}",
            (ax.stats().built_nodes - before) / 100,
            sent / 100
        );
    }
    let (mut r, mut ui, _) = setup(100_000, 3);
    ui.scroll([20., 20.], [0., 10.]).unwrap();
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    let before = ui.stats();
    let mut direction = 0.25;
    run("virtual_within_row", 100_000, || {
        ui.scroll([20., 20.], [0., direction]).unwrap();
        direction = -direction;
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    });
    eprintln!("virtual_within_row: {:?} -> {:?}", before, ui.stats());
    let (mut r, mut ui, _) = setup(1000, 4);
    let b = ui
        .elements()
        .find(|e| e.key == Some(&"hover".into()))
        .unwrap()
        .bounds;
    ui.pointer(
        &mut r,
        PointerEvent::Moved([b.x + b.width / 2., b.y + b.height / 2.]),
    )
    .unwrap();
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    let before = ui.stats();
    for frame in 1..=30 {
        r.begin_frame(Duration::from_millis(frame * 16));
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    }
    eprintln!(
        "tooltip_wait_480ms: {:?} -> {:?}; frame_requested={}",
        before,
        ui.stats(),
        r.animation_frame_requested()
    );
}
