//! Deterministic test support for RXUI.

#![warn(missing_docs)]

mod golden;
mod scene;

pub use golden::assert_text_golden;
pub use scene::{SemanticLandmark, SemanticScene};
