//! Native entity counter with an automation smoke path.

use astrelis_core::geometry::Size;
use rxui_core::{Context, Element, Render, button, column, label, row};
use rxui_host::{WindowHostOptions, native::WindowAttributes, run_entity};

struct Counter {
    value: i32,
}

impl Render for Counter {
    fn render(&mut self, context: &mut Context<Self>) -> Element {
        column()
            .gap(12.0)
            .child(label(format!("Value: {}", self.value)))
            .child(
                row()
                    .gap(8.0)
                    .child(
                        button("Decrement").on_click(context.listener(|this, _, context| {
                            this.value -= 1;
                            context.notify();
                        })),
                    )
                    .child(
                        button("Increment").on_click(context.listener(|this, _, context| {
                            this.value += 1;
                            context.notify();
                        })),
                    ),
            )
    }
}

fn main() -> Result<(), rxui_host::native::RuntimeError<std::io::Error>> {
    run_entity(
        |_| Counter { value: 0 },
        WindowHostOptions {
            window: WindowAttributes {
                title: "RXUI entity counter".into(),
                inner_size: Some(Size::new(420.0, 220.0)),
                ..WindowAttributes::default()
            },
            ..WindowHostOptions::default()
        },
    )
}
