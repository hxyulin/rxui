//! CPU flex/stack reconciliation and placement work. No text/GPU/native work is timed.
use rxui::{ElementId, TextMeasure, TextRequest, UiError, prelude::*};
use std::{hint::black_box, time::Instant};
struct Panel {
    count: usize,
    raised: bool,
    offset: f32,
}
impl View for Panel {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        row()
            .size(800., 600.)
            .gap(16.)
            .child(column().width(160.).fill_height())
            .child(
                stack()
                    .flex_grow(1.)
                    .flex_basis(0.)
                    .min_width(0.)
                    .fill_height()
                    .children((0..self.count).map(|i| {
                        column()
                            .key(i)
                            .size(20. + (i % 7) as f32, 10. + (i % 5) as f32)
                            .z_index(if i == 0 && self.raised { 3 } else { 0 })
                    }))
                    .child(
                        column()
                            .key("overlay")
                            .absolute()
                            .left(self.offset)
                            .top(12.)
                            .size(200., 100.)
                            .z_index(2)
                            .pointer_events(PointerEvents::Block),
                    ),
            )
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, _: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        panic!("fixture has no text")
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
    eprintln!(
        "Element size: {} bytes; Taffy Style size: {} bytes",
        std::mem::size_of::<rxui::Element>(),
        std::mem::size_of::<rxui::taffy::Style>()
    );
    println!("case,count,batch,us_per_op");
    for count in [16, 256, 1000] {
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| {
            cx.new(|_| Panel {
                count,
                raised: false,
                offset: 12.,
            })
        });
        let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
        ui.prepare(&mut runtime, [800., 600.], &mut Measure)
            .unwrap();
        let initial = ui.stats();
        time("stack_idle", count, || {
            ui.prepare(&mut runtime, [800., 600.], &mut Measure)
                .unwrap();
            black_box(ui.stats());
        });
        assert_eq!(ui.stats(), initial);
        time("stack_description", count, || {
            runtime.update(|cx| root.update(cx, |_, _| ()));
            ui.prepare(&mut runtime, [800., 600.], &mut Measure)
                .unwrap();
            black_box(ui.stats());
        });
        assert_eq!(ui.stats().layout_passes, initial.layout_passes);
        time("stack_z_order", count, || {
            runtime.update(|cx| root.update(cx, |s, _| s.raised = !s.raised));
            ui.prepare(&mut runtime, [800., 600.], &mut Measure)
                .unwrap();
            black_box(ui.elements().last().unwrap().id);
        });
        assert_eq!(ui.stats().layout_passes, initial.layout_passes);
        assert_eq!(ui.stats().style_resolutions, initial.style_resolutions);
        time("stack_position", count, || {
            runtime.update(|cx| {
                root.update(cx, |s, _| {
                    s.offset = if s.offset == 12. { 24. } else { 12. }
                })
            });
            ui.prepare(&mut runtime, [800., 600.], &mut Measure)
                .unwrap();
            black_box(ui.stats());
        });
        assert_eq!(ui.stats().created_nodes, initial.created_nodes);
        assert_eq!(ui.stats().measurements, 0);
        assert!(ui.stats().layout_passes > initial.layout_passes);
    }
}
