//! Component, reconciliation, styling, and service foundations for RXUI.

#![warn(missing_docs)]

pub mod diagnostics;

mod component;
mod icon;
mod services;
mod style;
mod view;
mod views;

pub use component::*;
pub use icon::*;
pub use services::*;
pub use style::*;
pub use view::*;
pub use views::*;

/// Re-export of the retained experimental core.
pub use astrelis_ui_next as core;
