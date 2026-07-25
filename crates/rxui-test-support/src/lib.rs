//! Deterministic headless harness shared by RXUI's own test suites.
//!
//! Integration tests link `rxui` as an ordinary dependency, so they see the
//! library without `cfg(test)`. Neither a `testing` cargo feature nor
//! `#[cfg(test)]` helpers can therefore be shared with them, which is why the
//! same host construction and input plumbing used to be copied into every test
//! file. This crate is that shared layer, kept unpublished and dependent only
//! on [`rxui_core`] so it stays below `rxui` in the dependency graph.

#![warn(missing_docs)]

mod golden;
mod harness;
mod scene;

pub use golden::assert_text_golden;
pub use harness::Harness;
pub use scene::{SemanticLandmark, SemanticScene};
