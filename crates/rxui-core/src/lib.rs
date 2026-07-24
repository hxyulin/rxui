//! Component, reconciliation, styling, and service foundations for RXUI.

#![warn(missing_docs)]

mod component;
mod icon;
mod services;
mod specialized;
mod style;
mod view;

pub use component::*;
pub use icon::*;
pub use services::*;
pub use specialized::*;
pub use style::*;
pub use view::*;

/// Re-export of the retained experimental core.
pub use astrelis_ui_next as core;
