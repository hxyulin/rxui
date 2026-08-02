//! Curated re-exports of the Astrelis vocabulary RXUI's own API speaks.
//!
//! These modules replace a blanket `pub use astrelis_ui_next as core`. That
//! re-export was both too much and too little: it published an entire
//! unpublished crate's unbounded surface from a 1.0-track facade, and it still
//! did not cover the types consumers actually needed - `LogicalSize` and
//! `Color` are `astrelis-core`, `Path` is `astrelis-paint`, and every keyboard
//! type is `astrelis-platform`, none of which `astrelis-ui-next` re-exports.
//! Every test and example therefore depended on the engine crates directly.
//!
//! Each module here is *closed*: it names what it exports. [`engine`] is the
//! exception in stability, not in shape - it is enumerated too, but it carries
//! the retained-element authoring vocabulary and is documented semver-exempt
//! while RXUI is pre-1.0.

pub mod color;
pub mod engine;
pub mod geometry;
pub mod input;
pub mod paint;
pub mod semantics;
