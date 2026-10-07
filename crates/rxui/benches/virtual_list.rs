//! CPU description/layout scaling only: mock text, no GPU or native frame timing.
use rxui::{ElementId, TextMeasure, TextRequest, UiError, prelude::*};
use std::{hint::black_box, time::Instant};
struct Rows {
    count: usize,
    scroll: ScrollHandle,
}
impl View for Rows {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        virtual_list(self.count, 28., &self.scroll, cx, |i| {
            label(format!("Row {i}")).key(i)
        })
        .fill_width()
        .fill_height()
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
fn setup(count: usize) -> (Runtime, Ui<Rows>) {
    let mut r = Runtime::new();
    let root = r.update(|cx| {
        cx.new(|_| Rows {
            count,
            scroll: ScrollHandle::new(),
        })
    });
    let mut ui = Ui::new(&mut r, root).unwrap();
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    (r, ui)
}
fn run(name: &str, count: usize, nodes: usize, mut operation: impl FnMut()) {
    for _ in 0..20 {
        operation();
    }
    let mut times = Vec::new();
    for _ in 0..50 {
        let start = Instant::now();
        for _ in 0..20 {
            operation();
        }
        times.push(start.elapsed().as_secs_f64() * 1e6 / 20.);
    }
    times.sort_by(f64::total_cmp);
    println!("{name},{count},{nodes},{:.3},{:.3}", times[25], times[47]);
}
fn main() {
    println!("operation,total_rows,live_elements,median_us,p95_us");
    for count in [100, 10_000, 100_000] {
        let (mut r, mut ui) = setup(count);
        let nodes = ui.elements().count();
        assert!(nodes < 60);
        run("initial_prepare", count, nodes, || {
            black_box(setup(count));
        });
        run("idle_prepare", count, nodes, || {
            ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
            black_box(ui.stats());
        });
        let mut forward = true;
        run("row_crossing", count, nodes, || {
            ui.scroll([20., 20.], [0., if forward { 28. } else { -28. }])
                .unwrap();
            forward = !forward;
            ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
            black_box(ui.stats());
        });
        ui.scroll([20., 20.], [0., 10.]).unwrap();
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        let stats = ui.stats();
        run("within_row", count, nodes, || {
            ui.scroll([20., 20.], [0., if forward { 0.25 } else { -0.25 }])
                .unwrap();
            forward = !forward;
            ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
            black_box(ui.stats());
        });
        assert_eq!(ui.stats().measurements, stats.measurements);
        assert_eq!(ui.stats().layout_passes, stats.layout_passes);
    }
}
