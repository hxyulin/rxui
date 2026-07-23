//! Experimental typed component and reconciled-view layer for RXUI.
//!
//! The crate is intentionally unpublished. It exercises an ergonomic
//! component API against `astrelis-ui-next` without changing RXUI 0.1.

#![warn(missing_docs)]

mod component;
mod editor;
mod native;
mod style;
mod view;

pub use component::*;
pub use editor::*;
pub use native::*;
pub use style::*;
pub use view::*;

/// Re-export of the retained experimental core.
pub use astrelis_ui_next as core;
