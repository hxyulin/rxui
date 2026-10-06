//! Description reconciliation/layout CPU work with deterministic mock text sizing.
//! No actual shaping, rasterization, GPU work, native event processing or painting.
use rxui::{ElementId, TextMeasure, TextRequest, UiError, prelude::*};
use std::{hint::black_box, time::Instant};

struct List {
    items: Vec<u32>,
    revision: u32,
    scrolling: bool,
}
impl View for List {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let content = column().children(self.items.iter().map(|id| {
            label(if *id == 0 {
                format!("Item {id}: {}", self.revision)
            } else {
                format!("Item {id}")
            })
            .key(*id)
        }));
        if self.scrolling {
            content.width(300.).height(100.).scroll_y()
        } else {
            content
        }
    }
}
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, text: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([
            text.text.len() as f32 * text.font_size / 2.,
            text.font_size * 1.25,
        ])
    }
}
fn measure(name: &str, items: u32, mut operation: impl FnMut()) {
    for _ in 0..20 {
        operation();
    }
    let mut times = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        for _ in 0..100 {
            operation();
        }
        times.push(start.elapsed().as_secs_f64() * 1e6 / 100.);
    }
    times.sort_by(f64::total_cmp);
    println!("{name},{items},20,100,{:.3},{:.3}", times[10], times[18]);
}
fn main() {
    println!("operation,items,samples,iterations_per_sample,median_us,p95_us");
    for count in [16, 256, 1000] {
        let mut runtime = Runtime::new();
        let list = runtime.update(|cx| {
            cx.new(|_| List {
                items: (0..count).collect(),
                revision: 0,
                scrolling: false,
            })
        });
        let mut ui = Ui::new(&mut runtime, list.clone()).unwrap();
        let mut text = Measure;
        ui.prepare(&mut runtime, [800., 600.], &mut text).unwrap();
        let initial = ui.stats();
        measure("idle_prepare", count, || {
            ui.prepare(&mut runtime, [800., 600.], &mut text).unwrap();
            black_box(ui.stats());
        });
        assert_eq!(ui.stats(), initial);
        measure("unchanged_description", count, || {
            runtime.update(|cx| list.update(cx, |_, _| ()));
            ui.prepare(&mut runtime, [800., 600.], &mut text).unwrap();
            black_box(ui.stats());
        });
        assert_eq!(ui.stats().measurements, initial.measurements);
        assert_eq!(ui.stats().layout_passes, initial.layout_passes);
        measure("keyed_reorder", count, || {
            runtime.update(|cx| list.update(cx, |this, _| this.items.rotate_left(1)));
            ui.prepare(&mut runtime, [800., 600.], &mut text).unwrap();
            black_box(ui.stats());
        });
        assert_eq!(ui.stats().created_nodes, initial.created_nodes);
        let scroll_list = runtime.update(|cx| {
            cx.new(|_| List {
                items: (0..count).collect(),
                revision: 0,
                scrolling: true,
            })
        });
        let mut scrolling = Ui::new(&mut runtime, scroll_list).unwrap();
        scrolling
            .prepare(&mut runtime, [800., 600.], &mut text)
            .unwrap();
        let scroll_initial = scrolling.stats();
        let mut direction = 1.;
        measure("scroll_geometry", count, || {
            assert!(scrolling.scroll([20., 20.], [0., direction]).unwrap());
            direction = -direction;
            black_box(scrolling.elements().next().unwrap().scroll_offset);
        });
        assert_eq!(scrolling.stats(), scroll_initial);
        assert_eq!(ui.stats().removed_nodes, 0);
        let before = ui.stats();
        measure("single_label_update", count, || {
            runtime.update(|cx| {
                list.update(cx, |this, _| this.revision = this.revision.wrapping_add(1))
            });
            ui.prepare(&mut runtime, [800., 600.], &mut text).unwrap();
            black_box(ui.stats());
        });
        assert!(ui.stats().measurements > before.measurements);
        assert_eq!(ui.stats().created_nodes, initial.created_nodes);
        let before = ui.stats();
        let dark = Theme::dark();
        let light = Theme::light();
        let mut light_next = true;
        measure("theme_palette_switch", count, || {
            ui.set_theme(if light_next {
                light.clone()
            } else {
                dark.clone()
            })
            .unwrap();
            light_next = !light_next;
            ui.prepare(&mut runtime, [800., 600.], &mut text).unwrap();
            black_box(ui.elements().next().unwrap().color);
        });
        assert_eq!(
            ui.stats().component_evaluations,
            before.component_evaluations
        );
        assert_eq!(ui.stats().measurements, before.measurements);
        assert_eq!(ui.stats().layout_passes, before.layout_passes);
        let larger = dark.clone().metrics(|m| m.font_size = 18.);
        let before = ui.stats();
        let mut larger_next = true;
        measure("theme_font_switch", count, || {
            ui.set_theme(if larger_next {
                larger.clone()
            } else {
                dark.clone()
            })
            .unwrap();
            larger_next = !larger_next;
            ui.prepare(&mut runtime, [800., 600.], &mut text).unwrap();
            black_box(ui.stats());
        });
        assert_eq!(
            ui.stats().component_evaluations,
            before.component_evaluations
        );
        assert!(ui.stats().measurements > before.measurements);
        assert!(ui.stats().layout_passes > before.layout_passes);
        #[cfg(feature = "accessibility")]
        {
            let mut cache = rxui::AccessKitTree::new();
            cache.update(&ui, "Benchmark", 1.).unwrap().unwrap();
            let stats = cache.stats();
            measure("semantic_warm_update", count, || {
                assert!(cache.update(&ui, "Benchmark", 1.).unwrap().is_none());
                black_box(cache.stats());
            });
            assert_eq!(cache.stats(), stats);
            measure("semantic_initial_tree", count, || {
                let mut tree = rxui::AccessKitTree::new();
                black_box(tree.update(&ui, "Benchmark", 1.).unwrap().unwrap());
            });
        }
    }
}
