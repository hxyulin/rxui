//! Color values.
//!
//! Prefer a [`crate::ColorRole`]: a role resolves through the [`crate::Theme`],
//! so a themed application keeps working when the theme changes. A literal
//! [`Color`] is for the cases a role cannot express - a chart series, a custom
//! element's own palette.

pub use astrelis_core::color::{Color, Rgba8};
