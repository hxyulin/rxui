//! Vector paint vocabulary.
//!
//! What a custom [`crate::engine::Element`] draws with, plus the image types
//! [`crate::media`] accepts. `ExternalImage` and `CompositorViewId` are named
//! here because they are `astrelis-paint` types; they are only *useful* through
//! a native window, and `rxui::native` names them again for that reason.

pub use astrelis_paint::{
    Brush, CompositorViewId, CornerRadii, ExternalImage, FillRule, GradientStop, Image,
    ImageOptions, ImageSampling, LineCap, LineJoin, LinearGradient, PaintError, Painter, Path,
    PathBuilder, PathVerb, RadialGradient, RoundedRect, ShadowStyle, StrokeStyle,
};
