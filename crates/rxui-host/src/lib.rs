#![doc = include_str!("../README.md")]
//!
//! ## Entry points
//!
//! - Implement [`Handler`] for the application. It prepares uploads and records
//!   each window's frame; the runner submits the recording once.
//! - [`Runner`] drives the winit event loop. [`RunnerOptions`] sets the
//!   scheduling and surface-loss retry policy.
//! - [`winit`] and [`drawing`] (rxui-draw) are re-exported so applications use
//!   the same versions.

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub mod drag;
mod error;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod handler;
mod policy;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod runner;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod scheduler;
#[cfg(all(
    test,
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
mod scheduler_tests;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod trace;
mod window;

pub use error::{Callback, RunError, UnknownWindow, WindowError};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub use handler::{ErrorAction, Handler, PrepareAction};
pub use policy::{CloseResponse, InvalidWindowMetrics, RedrawMode, SurfaceSettings, WindowMetrics};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub use runner::{AppContext, Runner, RunnerOptions};
pub use rxui_draw as drawing;
pub use window::{WindowContext, WindowInfo, WindowUpdate};
pub use winit;
