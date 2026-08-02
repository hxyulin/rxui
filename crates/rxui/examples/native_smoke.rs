//! Headed smoke test: opens a real window, renders a fixed number of frames,
//! and exits successfully.
//!
//! This is the CI gate that the native path - window creation, GPU adapter
//! selection, surface configuration, compositing, and presentation - still
//! works on every supported platform. It asserts nothing about appearance; it
//! only proves that a frame reaches the screen without an error.
//!
//! Set `RXUI_SMOKE_FRAMES` to change the frame count (default 3).

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::io;

use rxui::{
    ColorRole, Component, ComponentContext, ContainerStyle, Space, Theme, View, column_with,
    geometry::Size,
    label,
    native::{
        App, AppContext, ComponentWindow, GraphicsContext, Runtime, RuntimeConfig, RuntimeError,
        WindowAttributes, WindowEvent, WindowHostOptions, WindowId,
    },
};
// Driving the loop ourselves is the one genuinely winit-only thing here; the
// rest of the windowing vocabulary is portable, and `main` is stubbed on wasm.
#[cfg(not(target_arch = "wasm32"))]
use rxui::native::run_return;

const DEFAULT_FRAMES: u32 = 3;

fn requested_frames() -> u32 {
    std::env::var("RXUI_SMOKE_FRAMES")
        .ok()
        .and_then(|frames| frames.parse().ok())
        .filter(|frames| *frames > 0)
        .unwrap_or(DEFAULT_FRAMES)
}

struct Smoke;

impl Component for Smoke {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<()> {
        column_with(
            ContainerStyle::new()
                .gap(Space::Md)
                .padding(Space::Lg)
                .background(ColorRole::Background),
            (
                label("RXUI native smoke").key("title"),
                label("Rendering, then exiting.").key("detail"),
            ),
        )
    }
}

struct SmokeApp {
    graphics: GraphicsContext,
    window: Option<ComponentWindow<Smoke>>,
    remaining: u32,
}

impl SmokeApp {
    fn new() -> Self {
        Self {
            graphics: GraphicsContext::new(),
            window: None,
            remaining: requested_frames(),
        }
    }
}

impl App for SmokeApp {
    type Error = io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.window.is_none() {
            let window = ComponentWindow::open(
                context,
                &self.graphics,
                Smoke,
                Theme::dark(),
                WindowHostOptions {
                    window: WindowAttributes {
                        title: "RXUI smoke".into(),
                        inner_size: Some(Size::new(480.0, 240.0)),
                        ..WindowAttributes::default()
                    },
                    ..WindowHostOptions::default()
                },
            )
            .map_err(io::Error::other)?;
            context.invalidate_window(window.window().id());
            self.window = Some(window);
        }
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
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
    ) -> Result<(), Self::Error> {
        let Some(window) = &mut self.window else {
            return Ok(());
        };
        if window.window().id() != id {
            return Ok(());
        }
        window.redraw().map_err(io::Error::other)?;
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining == 0 {
            println!("native smoke rendered {} frame(s)", requested_frames());
            self.window = None;
            context.unregister_window(id);
            context.exit();
        } else {
            // The tree is clean after a redraw, so nothing would ask for
            // another frame. Drive the remaining frames explicitly.
            context.invalidate_window(id);
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), RuntimeError<io::Error>> {
    Runtime::finish(run_return(Runtime::new(
        SmokeApp::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
