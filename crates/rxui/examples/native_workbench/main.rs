//! Native entry point for the editor-style workbench.
//!
//! Everything this binary knows about the workbench lives in [`model`], which
//! `crates/rxui/tests/workbench.rs` compiles a second time and drives headlessly.
//! Keeping the window, the theme, and the event loop out of that module is what
//! makes the same component testable on three operating systems with no GPU.

use astrelis_core::geometry::Size;
use astrelis_platform::WindowAttributes;
use rxui::{Theme, WindowHostOptions, run_component};

mod model;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), astrelis_app::RuntimeError<std::io::Error>> {
    run_component(
        model::Workbench::new(),
        Theme::dark(),
        WindowHostOptions {
            window: WindowAttributes {
                title: "RXUI workbench".into(),
                inner_size: Some(Size::new(1100.0, 720.0)),
                ..WindowAttributes::default()
            },
            ..WindowHostOptions::default()
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {}
