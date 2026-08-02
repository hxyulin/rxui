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
    let observed = Rc::new(Cell::new(None));
    let mut observer = None;
    let mut subscription: Option<Subscription> = None;
    let mut harness = EntityHarness::new(|cx| {
        let counter = cx.new(|_| Counter { value: 0 });
        let sink = cx.new(|_| Observer);
        let observed = observed.clone();
        subscription = Some(cx.update(&sink, |_, cx| {
            cx.subscribe(&counter, move |_, _, saved, _| observed.set(Some(saved.0)))
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
            layout_elements: 38,
            rebuilt_fragments: 4,
            reused_fragments: 10,
            hit_test_nodes: 8,
            accessibility_nodes: 1,
            shaped_text: 14,
            visited_compose_nodes: 11,
            compose_skipped_subtrees: 3,
            visited_accessibility_nodes: 11,
            accessibility_skipped_subtrees: 3,
            invalidate_steps: 3,
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
            hit_test_nodes: 9,
            ..increment.passes
        }
    );

    harness.activate("Save");
    let save = harness.stats();
    assert_eq!(observed.get(), Some(0));
    assert_eq!(save.views, ViewStats::new());
    assert_eq!(
        save.passes,
        PassStats {
            reused_fragments: 14,
            invalidate_steps: 4,
            ..PassStats::default()
        }
    );

    drop((observer, subscription));
}
