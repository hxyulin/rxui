//! Native incremental component window.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use astrelis_core::geometry::Size;
use astrelis_platform::WindowAttributes;
use rxui_next::{
    ButtonStyle, ButtonVariant, ColorRole, Component, ComponentContext, ContainerStyle, Space,
    Theme, View, WindowHostOptions, button, button_with, checkbox, column_with, label, row,
    run_component, slider_with_step,
};

#[derive(Clone)]
enum Action {
    Decrement,
    Increment,
    SetEnabled(bool),
    SetScale(f32),
}

struct Counter {
    value: i32,
    enabled: bool,
    scale: f32,
}

impl Component for Counter {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Decrement => self.value -= 1,
            Action::Increment => self.value += 1,
            Action::SetEnabled(enabled) => self.enabled = enabled,
            Action::SetScale(scale) => self.scale = scale,
        }
    }

    fn view(&self, _theme: &Theme) -> View<Action> {
        column_with(
            ContainerStyle::new()
                .gap(Space::Md)
                .padding(Space::Lg)
                .background(ColorRole::Background),
            (
                label(format!("Value: {}", self.value)).key("value"),
                row((
                    button("−", Action::Decrement).key("decrement"),
                    button_with(
                        "+",
                        Action::Increment,
                        ButtonStyle::standard().variant(ButtonVariant::Primary),
                    )
                    .key("increment"),
                ))
                .enabled(self.enabled)
                .key("buttons"),
                checkbox("Counter enabled", self.enabled, Action::SetEnabled).key("enabled"),
                slider_with_step("Scale", self.scale, 0.5..=2.0, 0.1, Action::SetScale)
                    .key("scale"),
            ),
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), astrelis_app::RuntimeError<std::io::Error>> {
    run_component(
        Counter {
            value: 0,
            enabled: true,
            scale: 1.0,
        },
        Theme::dark(),
        WindowHostOptions {
            window: WindowAttributes {
                title: "RXUI Next counter".into(),
                inner_size: Some(Size::new(420.0, 180.0)),
                ..WindowAttributes::default()
            },
            ..WindowHostOptions::default()
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {}
