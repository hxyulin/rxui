//! Widgets composed entirely from `rxui-core` view kinds.
//!
//! Nothing here implements a retained element; the split from
//! [`crate::element`] is what keeps the heterogeneity of this crate explicit.

#[cfg(feature = "docking")]
pub mod docking;
