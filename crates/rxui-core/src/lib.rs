//! The unconditional half of RXUI: components, the view protocol, styling,
//! services, and every composition an application cannot do without.
//!
//! # What belongs here
//!
//! This crate is default-on, 1.0-committed, dependency-minimal, and has **no
//! `[features]` table, ever**. That is the stated axis of the crate graph, and
//! CI asserts both halves of it: that this manifest grows no `[features]`
//! section, and that this crate's dependency closure never reaches wgpu, winit,
//! taffy, arboard, or the deprecated retained engine.
//!
//! Anything an application can do without belongs in `rxui-widgets`, one Cargo
//! feature each. Anything that opens a window belongs in `rxui-native`.
//!
//! Libraries should depend on this crate directly rather than on `rxui` with
//! narrowed features - Cargo unifies features across a dependency graph, so a
//! library that narrows them makes its own surface depend on its consumer's
//! choices.
//!
//! # Layout
//!
//! Two private modules carry the view system and are re-exported flat: `view`
//! is the open protocol, and `views` holds the kinds implemented against it.
//! Everything else composes those, and stays a module because the facade groups
//! it the same way: [`controls`] for leaf controls, [`surfaces`] for the ones
//! that also carry policy, then [`forms`], [`data`], [`media`], and
//! [`inspect`].

#![warn(missing_docs)]

pub mod controls;
pub mod data;
pub mod diagnostics;
pub mod forms;
pub mod inspect;
pub mod media;
pub mod surfaces;

mod component;
mod reexport;
#[cfg(test)]
mod semantic_probe;
mod services;
mod style;
mod view;
mod views;

pub use component::*;
pub use reexport::{color, engine, geometry, input, paint, semantics};
pub use services::*;

/// Layout axis, at the root because a `row` and a `column` are the same view.
///
/// Re-exported rather than newtyped. The deleted `DockAxis` was the proof of
/// what a newtype costs here: a two-variant shadow of a two-variant enum plus a
/// public converter, buying nothing.
pub use astrelis_ui_next::{Alignment, Axis};
pub use style::*;
pub use view::*;
pub use views::*;
