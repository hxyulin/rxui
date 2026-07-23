//! Native incremental component window.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::io;

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig};
use astrelis_core::geometry::Size;
use astrelis_platform::{WindowAttributes, WindowEvent, WindowId};
use astrelis_ui_host::{GraphicsContext, WindowHostOptions};
use rxui_next::{
    ButtonStyle, ButtonVariant, ColorRole, Component, ComponentContext, ComponentWindow,
    ContainerStyle, Space, Theme, View, button, button_with, checkbox, column_with, label, row,
    slider_with_step,
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

struct NativeCounter {
    graphics: GraphicsContext,
    window: Option<ComponentWindow<Counter>>,
}

impl NativeCounter {
    fn new() -> Self {
        Self {
            graphics: GraphicsContext::new(),
            window: None,
        }
    }
}

impl App for NativeCounter {
    type Error = io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.window.is_some() {
            return Ok(());
        }
        let window = ComponentWindow::open(
            context,
            &self.graphics,
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
        .map_err(io::Error::other)?;
        context.invalidate_window(window.window().id());
        self.window = Some(window);
        Ok(())
    }

    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        let Some(window) = &mut self.window else {
            return Ok(());
        };
        if window.window().id() != id {
            return Ok(());
        }
        let update = window.handle_event(&event).map_err(io::Error::other)?;
        if update.close_requested {
            self.window = None;
            context.unregister_window(id);
            context.exit();
        } else if update.redraw {
            context.invalidate_window(id);
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        _context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
    ) -> Result<(), Self::Error> {
        if let Some(window) = &mut self.window
            && window.window().id() == id
        {
            window.redraw().map_err(io::Error::other)?;
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), astrelis_app::RuntimeError<io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        NativeCounter::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
