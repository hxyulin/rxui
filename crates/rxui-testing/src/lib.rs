//! Deterministic component testing and legacy/Next differential goldens.

#![warn(missing_docs)]

pub mod differential;
pub mod golden;
mod next;

pub use next::ComponentHarness;
