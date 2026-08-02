//! Accessibility vocabulary.
//!
//! Everything a view publishes to a screen reader, and everything a semantic
//! snapshot reports back. This is the whole of the engine's semantics module,
//! which is already a closed set.

pub use astrelis_ui_next::{
    AccessibilityUpdate, SemanticAction, SemanticActionKind, SemanticData, SemanticNode,
    SemanticRole,
};
