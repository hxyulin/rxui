//! Optional RXUI surfaces, one Cargo feature each.
//!
//! This crate holds every widget an application can do without. `rxui-core`
//! carries the component model, the view protocol, and everything unconditional;
//! anything here is opt-out. That is the whole axis, and it is why this crate
//! has a `[features]` table and `rxui-core` deliberately has none.

#![warn(missing_docs)]

pub mod compose;
pub mod element;

#[cfg(feature = "devtools")]
pub mod inspector;
