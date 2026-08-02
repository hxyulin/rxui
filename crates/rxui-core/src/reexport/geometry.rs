//! Logical and physical geometry.
//!
//! RXUI's own API is entirely logical: a `LogicalSize` is device-independent and
//! a window's scale factor converts it. The generic [`Point`], [`Size`], and
//! [`Rect`] and the [`Logical`]/[`Physical`] markers are exported because the
//! aliases below cannot be named without them, and because a window is opened
//! with a bare `Size`.

pub use astrelis_core::{
    geometry::{
        Logical, LogicalPoint, LogicalRect, LogicalSize, Physical, PhysicalPoint, PhysicalRect,
        PhysicalSize, Point, Rect, Size,
    },
    math::Affine2,
};
