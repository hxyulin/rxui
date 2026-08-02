//! Entity-aware harness routing smoke test.

use rxui_core::{Context, Element, Render, button, column, label};
use rxui_test::EntityHarness;

struct Counter {
    value: usize,
}

impl Render for Counter {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        column()
            .child(label(format!("Value: {}", self.value)))
            .child(button("Increment").on_click(cx.listener(|this, _, cx| {
                this.value += 1;
                cx.notify();
            })))
    }
}

#[test]
fn click_routes_action_box_through_app_and_flushes() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| Counter { value: 0 }));
    harness.click("Increment");

    assert_eq!(harness.root().read(harness.app()).value, 1);
    assert!(harness.try_find("Value: 1").is_some());
    assert_eq!(harness.stats().views.component_views, 1);
}
