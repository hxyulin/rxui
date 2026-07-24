//! Deterministic component testing and old/component differential goldens.

#![warn(missing_docs)]

mod component;
pub mod differential;
pub mod golden;

pub use component::ComponentHarness;
