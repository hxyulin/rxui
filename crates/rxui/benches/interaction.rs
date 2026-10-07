//! CPU input/retained control work with deterministic sizing, no real shaping or GPU.
use rxui::{ElementId, ElementType, PointerEvent, TextMeasure, TextRequest, UiError, prelude::*};
use std::{hint::black_box, time::Instant};
struct Rows(usize);
impl View for Rows {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .fill_width()
            .children((0..self.0).map(|i| label(format!("row {i}")).key(i).height(24.)))
    }
}
struct Workspace {
    rows: Entity<Rows>,
    handle: ScrollHandle,
    split: SplitPosition,
}
impl View for Workspace {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        split_row(
            scroll_area(self.rows.clone()).handle(self.handle.clone()),
            column(),
        )
        .size(800., 300.)
        .position(self.split)
        .min_first(100.)
        .min_second(100.)
        .on_resize(cx.listener(|s, e: &ResizeEvent, _| s.split = e.position))
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
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
            start.elapsed().as_secs_f64() * 1e6 / 50.
        );
    }
}
fn main() {
    eprintln!(
        "Element size: {} bytes",
        std::mem::size_of::<rxui::Element>()
    );
    println!("case,count,batch,us_per_op");
    for count in [16, 256, 1000] {
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| {
            let rows = cx.new(|_| Rows(count));
            cx.new(|_| Workspace {
                rows,
                handle: ScrollHandle::new(),
                split: SplitPosition::Pixels(240.),
            })
        });
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        ui.prepare(&mut runtime, [800., 300.], &mut Measure)
            .unwrap();
        let stats = ui.stats();
        time("idle_with_handle", count, || {
            ui.prepare(&mut runtime, [800., 300.], &mut Measure)
                .unwrap();
            black_box(ui.stats());
        });
        assert_eq!(ui.stats(), stats);
        let mut direction = 1.;
        time("wheel_geometry_with_handle", count, || {
            assert!(ui.scroll([10., 10.], [0., direction]).unwrap());
            direction = -direction;
        });
        assert_eq!(ui.stats(), stats);
        let bar = ui
            .elements()
            .find(|e| e.kind == ElementType::Scrollbar)
            .unwrap();
        let thumb = bar.range.unwrap().thumb_bounds.unwrap();
        let point = [thumb.x + 2., thumb.y + 2.];
        ui.pointer(&mut runtime, PointerEvent::Pressed(point))
            .unwrap();
        let mut delta = 1.;
        time("scrollbar_drag_and_prepare", count, || {
            ui.pointer(
                &mut runtime,
                PointerEvent::Moved([point[0], point[1] + delta]),
            )
            .unwrap();
            delta = 1. - delta;
            ui.prepare(&mut runtime, [800., 300.], &mut Measure)
                .unwrap();
        });
        assert_eq!(ui.stats(), stats);
        ui.pointer(&mut runtime, PointerEvent::Released(point))
            .unwrap();
        let splitter = ui
            .elements()
            .find(|e| e.kind == ElementType::Splitter)
            .unwrap()
            .bounds;
        let point = [splitter.x + 2., 100.];
        ui.pointer(&mut runtime, PointerEvent::Pressed(point))
            .unwrap();
        ui.prepare(&mut runtime, [800., 300.], &mut Measure)
            .unwrap();
        let before = ui.stats();
        let mut delta = 1.;
        time("controlled_split_drag_and_layout", count, || {
            ui.pointer(
                &mut runtime,
                PointerEvent::Moved([point[0] + delta, point[1]]),
            )
            .unwrap();
            delta = 1. - delta;
            ui.prepare(&mut runtime, [800., 300.], &mut Measure)
                .unwrap();
        });
        // Only the workspace evaluates; the rows component keeps its description.
        assert_eq!(
            ui.stats().component_evaluations - before.component_evaluations,
            1020
        );
        assert_eq!(ui.stats().layout_passes - before.layout_passes, 1020);
        assert_eq!(ui.stats().created_nodes, before.created_nodes);
    }
}
