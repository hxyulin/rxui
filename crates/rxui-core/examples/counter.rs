//! Headless entity counter using the normative v2 authoring surface.

use rxui_core::{
    Context, Element, EntityHarness, EventEmitter, Render, button, column, label, row,
};

struct Counter {
    value: i32,
}

#[derive(Clone, Copy, Debug)]
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

fn main() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| Counter { value: 0 }));
    harness.click("+");
    harness.click("+");
    harness.click("−");
    harness.activate("Save");
    println!("counter after headless clicks:\n{}", harness.snapshot());

    // Keep the event payload exercised in the example build as well as tests.
    let saved = Saved(harness.root().read(harness.app()).value);
    println!("saved payload: {}", saved.0);
}
