//! Experimental typed component and reconciled-view layer for RXUI.
//!
//! The crate is intentionally unpublished. It exercises an ergonomic
//! component API against `astrelis-ui-next` without changing RXUI 0.1.

#![warn(missing_docs)]

mod catalog;
mod chart;
mod component;
mod editor;
mod native;
mod specialized;
mod style;
mod view;

pub use catalog::*;
pub use chart::*;
pub use component::*;
pub use editor::*;
pub use native::*;
pub use specialized::*;
pub use style::*;
pub use view::*;

/// Re-export of the retained experimental core.
pub use astrelis_ui_next as core;
