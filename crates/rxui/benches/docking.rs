//! Deterministic CPU docking composition and controlled update costs, without GPU.
//! `panels` counts tabs in the main group; each additional group has one extra tab.
//! Each case warms 20 operations, then emits 15 batches of 50 operations.
//! Selection/resize change the application model then prepare; input dispatch,
//! native windows, real text shaping, and GPU rendering are outside this benchmark.
use rxui::{ElementId, TextMeasure, TextRequest, UiError, prelude::*};
use std::{hint::black_box, time::Instant};

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
fn heavy_content() -> rxui::Element {
    column().children((0..32).map(|i| label(format!("row {i}")).height(20.)))
}
struct Workspace {
    tree: DockTree,
    policy: TabContentPolicy,
}
impl View for Workspace {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        dock(&self.tree, |_| dock_panel("Panel", label("Content")))
            .content_policy(self.policy)
            .min_pane_size(0., 0.)
    }
}
fn time(case: &str, panels: usize, groups: usize, mut f: impl FnMut()) {
    for _ in 0..20 {
        f();
    }
    for batch in 0..15 {
        let start = Instant::now();
        for _ in 0..50 {
            f();
        }
        println!(
            "{case},{panels},{groups},{batch},{:.6}",
            start.elapsed().as_secs_f64() * 1e6 / 50.
        );
    }
}
fn main() {
    println!("case,panels,groups,batch,us_per_op");
    for (panels, groups) in [(16, 1), (256, 1), (256, 16)] {
        let mut tree = DockTree::from_panels((0..panels).map(Key::from)).unwrap();
        let main = tree.root().id();
        for group in 1..groups {
            tree.split(
                main,
                DockSide::Right,
                panels + group,
                SplitPosition::Fraction(0.5),
            )
            .unwrap();
        }
        time("describe_keep", panels, groups, || {
            let _ = black_box(
                dock(black_box(&tree), |_| dock_panel("Panel", label("Content"))).into_element(),
            );
        });
        time("describe_selected", panels, groups, || {
            let _ = black_box(
                dock(black_box(&tree), |_| dock_panel("Panel", label("Content")))
                    .content_policy(TabContentPolicy::MountSelected)
                    .into_element(),
            );
        });
        time("describe_heavy_eager_selected", panels, groups, || {
            let _ = black_box(
                dock(black_box(&tree), |_| dock_panel("Panel", heavy_content()))
                    .content_policy(TabContentPolicy::MountSelected)
                    .into_element(),
            );
        });
        time("describe_heavy_lazy_selected", panels, groups, || {
            let _ = black_box(
                dock(black_box(&tree), |_| {
                    dock_panel_lazy("Panel", heavy_content)
                })
                .content_policy(TabContentPolicy::MountSelected)
                .into_element(),
            );
        });
        for policy in [
            TabContentPolicy::KeepMounted,
            TabContentPolicy::MountSelected,
        ] {
            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| {
                cx.new(|_| Workspace {
                    tree: tree.clone(),
                    policy,
                })
            });
            let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
            ui.prepare(&mut runtime, [1600., 900.], &mut Measure)
                .unwrap();
            let mut next = 0usize;
            time(
                if policy == TabContentPolicy::KeepMounted {
                    "select_keep_prepare"
                } else {
                    "select_selected_prepare"
                },
                panels,
                groups,
                || {
                    next = 1 - next;
                    runtime.update(|cx| {
                        root.update(cx, |s, _| {
                            s.tree.select(main, &Key::from(next)).unwrap();
                        })
                    });
                    ui.prepare(&mut runtime, [1600., 900.], &mut Measure)
                        .unwrap();
                    black_box(ui.stats());
                },
            );
            if groups > 1 {
                let split = tree.root().id();
                time(
                    if policy == TabContentPolicy::KeepMounted {
                        "resize_keep_prepare"
                    } else {
                        "resize_selected_prepare"
                    },
                    panels,
                    groups,
                    || {
                        next = 1 - next;
                        runtime.update(|cx| {
                            root.update(cx, |s, _| {
                                s.tree
                                    .resize(
                                        split,
                                        SplitPosition::Fraction(0.45 + next as f32 * 0.05),
                                    )
                                    .unwrap();
                            })
                        });
                        ui.prepare(&mut runtime, [1600., 900.], &mut Measure)
                            .unwrap();
                        black_box(ui.stats());
                    },
                );
            }
        }
    }
}
