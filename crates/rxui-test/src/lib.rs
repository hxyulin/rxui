//! Deterministic test support for RXUI.

#![warn(missing_docs)]

mod golden;
mod harness;
pub mod probe;
mod scene;

pub use golden::assert_text_golden;
pub use harness::{Clipboard, Harness, MemoryClipboard};
pub use scene::{SemanticLandmark, SemanticScene};
