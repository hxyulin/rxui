//! Native window and runtime integration for RXUI components.
//!
//! This crate is the only real dependency cliff in RXUI: it is what pulls wgpu,
//! the compositor, and - with the default `winit` feature - a winit event loop
//! and a platform clipboard. Everything in `rxui-core` runs headless.
//!
//! # Re-exports
//!
//! Opening a window means naming types from five Astrelis crates. They appear
//! in [`ComponentWindow`]'s own signatures, so a consumer had to add all five to
//! its own `Cargo.toml` just to call a method here. They are all re-exported
//! below instead.
//!
//! The engine's `Next*` codenames are aliased away: that prefix distinguished
//! the incremental host from the retained one it replaced, which is Astrelis
//! history and not something an RXUI consumer should have to know.

#![warn(missing_docs)]

mod native;

#[cfg(not(target_arch = "wasm32"))]
pub use native::ComponentApplication;
#[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
pub use native::run_component;
pub use native::{ComponentWindow, GraphicsContext, WindowHostOptions};

pub use astrelis_ui_host::{
    HostError, HostUpdate, NextAccessibilityAdapter as AccessibilityAdapter,
    NextAccessibilityRequest as AccessibilityRequest, NextWindowHost as WindowHost,
};

/// Windowing and application-loop vocabulary.
///
/// None of this is gated. `astrelis-app` has no `cfg(target_arch)` in it at all
/// and `WindowId` is unconditional, so gating the re-exports would make them
/// *less* portable than the crates they come from - and `App::window_event`
/// names `WindowId` in its signature on every target, so an `impl App` written
/// against the facade could not compile for the web.
pub use astrelis_app::{App, AppContext, Runtime, RuntimeConfig, RuntimeError};
pub use astrelis_platform::{Window, WindowAttributes, WindowEvent, WindowId};

/// Runs an application to completion on a winit event loop.
///
/// Re-exported because the native entry points are written around it, and
/// because an application that wants `ComponentWindow` inside its own `App` but
/// still wants winit to drive the loop would otherwise depend on
/// `astrelis-platform-winit` directly.
#[cfg(all(not(target_arch = "wasm32"), feature = "winit"))]
pub use astrelis_platform_winit::run_return;

/// GPU and compositor types named by [`ComponentWindow`]'s render entry points.
pub use astrelis_compositor::{CompositionStats, ViewOptions, ViewRenderTarget};
pub use astrelis_gpu::{CommandEncoder, TextureView};
pub use astrelis_paint_gpu::RenderStats;
