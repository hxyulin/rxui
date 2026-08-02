//! Normative counter interactions and typed-event integration gate.

use std::{cell::Cell, rc::Rc};

use rxui_core::{
    Context, Element, EntityHarness, EventEmitter, Render, Subscription, ViewStats, button, column,
    label, row,
};
use rxui_tree::PassStats;

struct Counter {
    value: i32,
}

#[derive(Clone, Copy)]
struct Saved(i32);

impl EventEmitter<Saved> for Counter {}

impl Render for Counter {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        column()
            .gap(12.0)
            .child(label(format!("Value: {}", self.value)))
            .child(
                row()
                    .gap(8.0)
                    .child(button("−").on_click(cx.listener(|this, _, cx| {
                        this.value -= 1;
                        cx.notify();
                    })))
                    .child(button("+").on_click(cx.listener(|this, _, cx| {
                        this.value += 1;
                        cx.notify();
                    })))
                    .child(button("Save").on_click(cx.listener(|this, _, cx| {
                        cx.emit(Saved(this.value));
                    }))),
            )
    }
}

struct Observer;

#[test]
fn clicks_have_exact_cost_and_save_is_observed_by_subscription() {
    let observed = Rc::new(Cell::new((0, None)));
    let mut observer = None;
    let mut subscription: Option<Subscription> = None;
    let mut harness = EntityHarness::new(|cx| {
        let counter = cx.new(|_| Counter { value: 0 });
        let sink = cx.new(|_| Observer);
        let observed = observed.clone();
        subscription = Some(cx.update(&sink, |_, cx| {
            cx.subscribe(&counter, move |_, _, saved, _| {
                observed.set((observed.get().0 + 1, Some(saved.0)));
            })
        }));
        observer = Some(sink);
        counter
    });

    harness.click("+");
    let increment = harness.stats();
    assert_eq!(harness.root().read(harness.app()).value, 1);
    assert_eq!(
        increment.views,
        ViewStats {
            component_views: 1,
            nodes_built: 0,
            nodes_rebuilt: 6,
            containers_reconciled: 2,
            set_children_calls: 0,
            memo_hits: 0,
            memo_misses: 0,
            rows_realized: 0,
            rows_recycled: 0,
        }
    );
    assert_eq!(
        increment.passes,
        PassStats {
            layout_elements: 14,
            rebuilt_fragments: 2,
            reused_fragments: 6,
            hit_test_nodes: 6,
            accessibility_nodes: 1,
            shaped_text: 2,
            visited_compose_nodes: 4,
            compose_skipped_subtrees: 1,
            visited_accessibility_nodes: 4,
            accessibility_skipped_subtrees: 1,
            invalidate_steps: 7,
            ..PassStats::default()
        }
    );

    harness.click("−");
    let decrement = harness.stats();
    assert_eq!(harness.root().read(harness.app()).value, 0);
    assert_eq!(decrement.views, increment.views);
    assert_eq!(
        decrement.passes,
        PassStats {
            hit_test_nodes: 7,
            ..increment.passes
        }
    );

    harness.click("Save");
    let first_click = harness.stats();
    harness.click("Save");
    let second_click = harness.stats();
    assert_eq!(observed.get(), (2, Some(0)));
    assert_eq!(first_click.views, ViewStats::new());
    assert_eq!(second_click.views, ViewStats::new());
    assert_eq!(first_click, second_click);
    assert_eq!(
        first_click.passes,
        PassStats {
            rebuilt_fragments: 1,
            reused_fragments: 7,
            hit_test_nodes: 5,
            invalidate_steps: 4,
            ..PassStats::default()
        }
    );

    harness.activate("Save");
    let first_semantic = harness.stats();
    harness.activate("Save");
    let second_semantic = harness.stats();
    assert_eq!(observed.get(), (4, Some(0)));
    assert_eq!(first_semantic.views, ViewStats::new());
    assert_eq!(second_semantic.views, ViewStats::new());
    assert_eq!(first_semantic, second_semantic);
    assert_eq!(
        first_semantic.passes,
        PassStats {
            reused_fragments: 8,
            ..PassStats::default()
        }
    );

    assert_eq!(
        harness
            .semantics()
            .iter()
            .filter(|node| node.data.label == "Save")
            .count(),
        1,
        "button must publish one accessible name"
    );

    drop((observer, subscription));
}
