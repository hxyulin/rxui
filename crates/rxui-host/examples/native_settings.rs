//! Native controlled settings form with an automation smoke path.

use astrelis_core::geometry::Size;
use rxui_core::{Context, Element, Render, button, checkbox, column, label, slider, text_field};
use rxui_host::{WindowHostOptions, native::WindowAttributes, run_entity};

struct Settings {
    profile: String,
    notifications: bool,
    scale: f32,
    status: String,
}

impl Render for Settings {
    fn render(&mut self, context: &mut Context<Self>) -> Element {
        column()
            .gap(12.0)
            .child(label("RXUI settings"))
            .child(
                text_field("Profile", self.profile.clone()).on_input(context.listener_value(
                    |this, value: String, context| {
                        this.profile = value;
                        context.notify();
                    },
                )),
            )
            .child(
                checkbox("Notifications", self.notifications).on_toggle(context.listener_value(
                    |this, value, context| {
                        this.notifications = value;
                        context.notify();
                    },
                )),
            )
            .child(
                slider("Interface scale", self.scale, 0.75..=2.0)
                    .step(0.05)
                    .on_change(context.listener_value(|this, value, context| {
                        this.scale = value;
                        context.notify();
                    })),
            )
            .child(
                button("Apply settings").on_click(context.listener(|this, _, context| {
                    this.status = format!("Applied settings for {}", this.profile);
                    context.notify();
                })),
            )
            .child(label(self.status.clone()))
    }
}

fn main() -> Result<(), rxui_host::native::RuntimeError<std::io::Error>> {
    run_entity(
        |_| Settings {
            profile: "Astrelis".into(),
            notifications: true,
            scale: 1.0,
            status: "No changes applied".into(),
        },
        WindowHostOptions {
            window: WindowAttributes {
                title: "RXUI entity settings".into(),
                inner_size: Some(Size::new(520.0, 420.0)),
                ..WindowAttributes::default()
            },
            ..WindowHostOptions::default()
        },
    )
}
