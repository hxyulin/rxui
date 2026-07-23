//! Experimental typed component and reconciled-view layer for RXUI.
//!
//! The crate is intentionally unpublished. It exercises an ergonomic
//! component API against `astrelis-ui-next` without changing RXUI 0.1.

#![warn(missing_docs)]

mod catalog;
mod chart;
mod component;
mod editor;
mod inspection;
mod native;
mod node_graph;
mod specialized;
mod style;
mod validation;
mod view;
mod workspace;

pub use catalog::*;
pub use chart::*;
pub use component::*;
pub use editor::*;
pub use inspection::*;
pub use native::*;
pub use node_graph::*;
pub use specialized::*;
pub use style::*;
pub use validation::*;
pub use view::*;
pub use workspace::*;

/// Re-export of the retained experimental core.
pub use astrelis_ui_next as core;
