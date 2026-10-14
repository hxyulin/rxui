//! CPU description/layout scaling only: mock text, no GPU or native frame timing.
use rxui::{ElementId, TextMeasure, TextRequest, UiError, prelude::*};
use std::{hint::black_box, time::Instant};
struct Rows {
    count: usize,
    scroll: ScrollHandle,
    /// Variable heights averaging 28 px; `None` measures the fixed-height list.
    variable: Option<VirtualRows>,
}
impl View for Rows {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        if let Some(rows) = &self.variable {
            return variable_list(rows, &self.scroll, cx, |i| label(format!("Row {i}")))
                .fill_width()
                .fill_height()
                .into_element();
        }
        virtual_list(self.count, 28., &self.scroll, cx, |i| {
            label(format!("Row {i}")).key(i)
        })
        .fill_width()
        .fill_height()
        .into_element()
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
/// Heights 20, 28 and 36 px in turn.
fn height(i: usize) -> f32 {
    20. + 8. * (i % 3) as f32
}
fn setup(count: usize, variable: bool) -> (Runtime, Ui<Rows>) {
    let mut r = Runtime::new();
    let root = r.update(|cx| {
        cx.new(|_| Rows {
            count,
            scroll: ScrollHandle::new(),
            variable: variable.then(|| (0..count).map(|i| (i, height(i))).collect()),
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
/// Initial, idle, row-crossing and within-row preparation, prefixed for variable rows.
fn scrolling(prefix: &str, count: usize, variable: bool) -> (Runtime, Ui<Rows>, usize) {
    let (mut r, mut ui) = setup(count, variable);
    let nodes = ui.elements().count();
    assert!(nodes < 60);
    run(&format!("{prefix}initial_prepare"), count, nodes, || {
        black_box(setup(count, variable));
    });
    run(&format!("{prefix}idle_prepare"), count, nodes, || {
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        black_box(ui.stats());
    });
    let mut forward = true;
    run(&format!("{prefix}row_crossing"), count, nodes, || {
        ui.scroll([20., 20.], [0., if forward { 28. } else { -28. }])
            .unwrap();
        forward = !forward;
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        black_box(ui.stats());
    });
    ui.scroll([20., 20.], [0., 10.]).unwrap();
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    let stats = ui.stats();
    run(&format!("{prefix}within_row"), count, nodes, || {
        ui.scroll([20., 20.], [0., if forward { 0.25 } else { -0.25 }])
            .unwrap();
        forward = !forward;
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        black_box(ui.stats());
    });
    assert_eq!(ui.stats().measurements, stats.measurements);
    assert_eq!(ui.stats().layout_passes, stats.layout_passes);
    (r, ui, nodes)
}
fn main() {
    println!("operation,total_rows,live_elements,median_us,p95_us");
    for count in [100, 10_000, 100_000] {
        scrolling("", count, false);
    }
    for count in [100, 10_000, 100_000] {
        let (mut r, mut ui, nodes) = scrolling("variable_", count, true);
        let root = ui.entity().clone();
        ui.scroll([20., 20.], [0., count as f32 * 14.]).unwrap();
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        let viewport = |ui: &Ui<Rows>| {
            ui.semantics()
                .find(|n| n.scroll_range[1] > 0.)
                .unwrap()
                .scroll_offset[1]
        };
        // One row height above the viewport changes; anchoring compensates.
        let mut grow = true;
        run("variable_resize_above", count, nodes, || {
            r.update(|cx| {
                root.update(cx, |s, _| {
                    let rows = s.variable.as_mut().unwrap();
                    rows.set_height(0, if grow { 60. } else { 20. });
                })
            });
            grow = !grow;
            ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
            black_box(ui.stats());
        });
        // A row is inserted at the top and removed again: O(n) index rebuilds.
        let before = viewport(&ui);
        let mut insert = true;
        run("variable_insert_above", count, nodes, || {
            r.update(|cx| {
                root.update(cx, |s, _| {
                    let rows = s.variable.as_mut().unwrap();
                    if insert {
                        rows.insert(0, "new", 40.);
                    } else {
                        rows.remove(0);
                    }
                })
            });
            insert = !insert;
            ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
            black_box(ui.stats());
        });
        assert_eq!(viewport(&ui), before);
    }
}
