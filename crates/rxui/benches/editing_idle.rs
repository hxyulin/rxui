//! Idle ordinary text inputs: deterministic CPU sizing, no shaping or GPU.
use rxui::{ElementId, TextMeasure, TextRequest, UiError, prelude::*};
use std::{hint::black_box, time::Instant};

struct Form(usize);
impl View for Form {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().children((0..self.0).map(|i| text_input("ordinary input").key(i)))
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
fn time(case: &str, count: usize, mut f: impl FnMut()) {
    const ITERATIONS: usize = 10_000;
    for _ in 0..ITERATIONS {
        f();
    }
    for batch in 0..20 {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            f();
        }
        println!(
            "{case},{count},{batch},{:.6}",
            start.elapsed().as_secs_f64() * 1e6 / ITERATIONS as f64
        );
    }
}
fn main() {
    println!("case,count,batch,us_per_op");
    for count in [16, 256, 1000] {
        let mut runtime = Runtime::new();
        let root = runtime.update(|cx| cx.new(|_| Form(count)));
        let mut ui = Ui::new(&mut runtime, root).unwrap();
        ui.prepare(&mut runtime, [800., 600.], &mut Measure)
            .unwrap();
        assert!(!ui.needs_prepare(&runtime).unwrap());
        let stats = ui.stats();
        time("needs_prepare", count, || {
            black_box(black_box(&ui).needs_prepare(black_box(&runtime)).unwrap());
        });
        time("idle_prepare", count, || {
            black_box(&mut ui)
                .prepare(black_box(&mut runtime), [800., 600.], &mut Measure)
                .unwrap();
        });
        assert_eq!(ui.stats(), stats);
        assert!(!ui.needs_prepare(&runtime).unwrap());
    }
}
