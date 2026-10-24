#![doc = include_str!("../README.md")]
//!
//! ## Entry points
//!
//! - [`GraphicsContext::from_device_queue`] wraps the application's wgpu device
//!   and queue.
//! - [`Painter`] owns the shape, line, path, image and text renderers.
//!   [`Painter::begin`] opens a [`PaintSession`] on one [`RenderPass`].
//! - Text is prepared before painting: [`Painter::prepare_text`] turns a
//!   [`TextLayout`] into a retained [`PreparedText`] that sessions draw.
//! - Paths are built with [`Path::builder`] and tessellated once with
//!   [`Painter::prepare_path`].
//! - [`wgpu`] is re-exported so applications use the same version.

// Browser wgpu handles are !Send/!Sync. Keep the same shared ownership types
// as native builds; Arc does not imply crossing threads on this target.
#![cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]

mod attachments;
mod brush;
mod clip;
mod context;
mod depth_stencil;
mod drawing;
mod error;
mod frame;
mod framebuffer;
mod geometry_pool;
mod material;
#[cfg(test)]
mod mesh;
#[cfg(test)]
mod mesh_renderer;
mod painter;
mod pass;
mod path;
mod pipeline_cache;
mod pipeline_options;
mod pipelines;
mod point_renderer;
mod points;
mod primitive;
mod recording;
mod target;
mod texture;
mod texture_renderer;
mod uploads;
mod vertex;
pub use recording::{Recording, RecordingResources};

pub use attachments::{
    RenderColorAttachment, RenderDepthStencilAttachment, RenderFormat, RenderPassDescriptor,
};
pub use brush::{Brush, BrushKind, BrushOptions, GradientSpread, GradientStop};
pub use clip::{MAX_ROUNDED_CLIPS, RoundedClip};
pub use context::{GpuEvent, GraphicsContext};

pub mod text;
pub use drawing::{
    BoxShadow, CornerRadii, DrawSpace, EdgeAntialiasing, LineCap, LineDraw, Rect, Shape, ShapeDraw,
    Stroke, StrokePlacement, Transform2D,
};
pub use error::Error;
pub use frame::{Frame, FrameError};
pub use framebuffer::{Framebuffer, FramebufferOptions};
pub use material::{Material, MaterialOptions};
#[cfg(test)]
pub use mesh::{
    Mesh, MeshDraw, MeshIndexBuffer, MeshIndices, MeshOptions, MeshVertexBuffer, VertexStream,
};
#[cfg(test)]
pub use mesh_renderer::{MeshDrawSession, MeshRenderer};
pub use painter::{PaintSession, Painter};
pub use pass::{RenderPass, RenderPassBuilder};
pub use path::{
    FillRule, LineJoin, Path, PathBuilder, PathDraw, PathDrawSession, PathOptions, PathRenderer,
    PathStroke, PreparedPath,
};
pub use pipeline_cache::PipelineCacheStats;
pub use pipeline_options::PipelineOptions;
pub use point_renderer::{
    MarkerDraw, MarkerDrawSession, MarkerRenderer, MarkerShape, PolylineDraw, PolylineDrawSession,
    PolylineRenderer,
};
pub use points::{Point2D, PointAppend, PointBuffer, PointBufferOptions};
pub use primitive::{LineDrawSession, LineRenderer, ShapeDrawSession, ShapeRenderer};
pub use target::{RenderTarget, SurfaceOptions, SurfaceTarget};
pub use text::{
    FontFamily, FontId, FontInfo, FontSlant, FontStretch, GeneratedGlyphs, GlyphPreparationLimits,
    GlyphPreparationRequest, GlyphUploadBudget, GlyphUploadProgress, MtsdfOptions, PreparedText,
    SelectionRects, TextAffinity, TextAlign, TextBuffer, TextCaret, TextDecoration,
    TextDecorationKind, TextDraw, TextError, TextFont, TextGlyph, TextLayout, TextLine,
    TextLineMetrics, TextPosition, TextPreparation, TextRasterOptions, TextRenderError,
    TextRenderer, TextRendererOptions, TextRendererStats, TextShaping, TextSpan, TextStyle,
    TextSystem, TextTrim, TextUpload, TextWrap,
};
pub use texture::{Texture, TextureOptions};
pub use texture_renderer::{
    PreparedTextureDraw, PreparedTextureDrawSession, SampledColor, TextureAlpha, TextureBinding,
    TextureBindingOptions, TextureBlend, TextureDraw, TextureDrawSession, TextureFilter,
    TextureMaterial, TextureMaterialOptions, TextureRenderer, UvRect,
};
pub use vertex::{Vertex, VertexLayout};

pub use uploads::UploadStats;

/// The exact wgpu version used by RXUI drawing, available for GPU interoperability.
pub use wgpu;

#[cfg(test)]
mod api_tests;
