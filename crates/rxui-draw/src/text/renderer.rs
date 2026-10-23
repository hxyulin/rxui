#[path = "preparation.rs"]
mod preparation;
use super::{FontId, TextFont, TextLayout};
use crate::{
    Error, GraphicsContext, Material, MaterialOptions, Rect, RenderFormat, RenderPass, Transform2D,
    VertexLayout,
    geometry_pool::{Geometry, GeometryPool},
};
use bytemuck::{Pod, Zeroable};
use preparation::{CpuGlyph, GlyphGenerator};
pub use preparation::{
    GeneratedGlyphs, GlyphPreparationLimits, GlyphPreparationRequest, GlyphUploadBudget,
    GlyphUploadProgress, TextUpload,
};
use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
use swash::scale::{
    Render, ScaleContext, Source, StrikeWith,
    image::{Content, Image},
};

/// GPU text resource preparation failure. Drawing uses [`crate::Error`].
#[derive(Debug)]
#[non_exhaustive]
pub enum TextRenderError {
    /// A device, attachment format, or pipeline error from the rendering layer.
    Graphics(Error),
    /// Invalid atlas limits, raster density/size, or prepared geometry.
    InvalidOptions,
    /// A padded glyph image exceeds the configured atlas page size.
    GlyphTooLarge,
    /// Internal atlas allocation exhausted its safe page budget.
    /// Preparation handles this by omitting affected glyphs, reporting their indices
    /// through [`PreparedText::skipped_glyphs`], and retrying on later preparation.
    /// Retained for source compatibility; ordinary preparation no longer returns it.
    AtlasFull,
    /// The backend could not parse a retained font or returned inconsistent image data.
    InvalidRaster,
    /// A worker request/result belongs to another renderer or predates `clear_cache`.
    StalePreparation,
    /// Generated entries, retained image bytes, or final geometry exceed an explicit limit.
    PreparationLimit,
    /// Commit still has pending entries; finalization performs no implicit generation/upload.
    PreparationIncomplete,
}
impl From<Error> for TextRenderError {
    fn from(value: Error) -> Self {
        Self::Graphics(value)
    }
}
impl std::fmt::Display for TextRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Graphics(e) => e.fmt(f),
            Self::InvalidOptions => {
                f.write_str("invalid text renderer, raster density, or prepared geometry")
            }
            Self::GlyphTooLarge => f.write_str("glyph image exceeds atlas page size"),
            Self::AtlasFull => f.write_str("text atlas budget is occupied by leased pages"),
            Self::StalePreparation => f.write_str("stale or foreign glyph preparation"),
            Self::PreparationLimit => f.write_str("glyph preparation limit exceeded"),
            Self::PreparationIncomplete => f.write_str("glyph preparation has pending entries"),
            Self::InvalidRaster => f.write_str("invalid retained font or raster image"),
        }
    }
}
impl std::error::Error for TextRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Graphics(e) => Some(e),
            _ => None,
        }
    }
}
/// Bounded glyph atlas/cache configuration, selected at renderer creation.
#[derive(Clone, Debug)]
pub struct TextRendererOptions {
    /// Immutable blend/color-write/depth/stencil settings for coverage, color and MTSDF.
    pub pipeline: crate::PipelineOptions,
    /// Square page dimension, default 1024. Each glyph has a transparent one-texel gutter.
    pub page_size: u32,
    /// Maximum live pages, including prepared/recording/completion leases; default 8.
    /// R8 coverage pages cost size² bytes; linear RGBA8 color/field pages cost size²×4.
    pub max_pages: usize,
    /// Maximum cached glyph keys, including blank results; default 16,384.
    pub max_cached_glyphs: usize,
    /// Maximum physical font size passed to the rasterizer, default 512 pixels.
    pub max_raster_size: f32,
}
impl Default for TextRendererOptions {
    fn default() -> Self {
        Self {
            pipeline: crate::PipelineOptions::default(),
            page_size: 1024,
            max_pages: 8,
            max_cached_glyphs: 16_384,
            max_raster_size: 512.,
        }
    }
}
/// Explicit raster scale, hinting and optional physical placement, independent of shaping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextRasterOptions {
    /// Positive finite raster texels per layout unit, default 1.
    /// Controls image density only; geometry and measurements retain layout units.
    pub raster_scale: f32,
    /// Rasterizer hinting at the chosen physical size, enabled by default.
    pub hinting: bool,
    /// Final viewport-relative physical origin for phase-aware outline coverage.
    /// `None` (default) retains freely transformed, filtered image placement.
    /// See [`Self::physical_origin`] for the draw/reuse contract.
    pub physical_origin: Option<[f32; 2]>,
}
impl Default for TextRasterOptions {
    fn default() -> Self {
        Self::new()
    }
}
impl TextRasterOptions {
    /// Creates 1:1 coverage/color preparation with hinting.
    pub const fn new() -> Self {
        Self {
            raster_scale: 1.,
            hinting: true,
            physical_origin: None,
        }
    }
    /// Selects image density without scaling geometry. Match the intended draw scale
    /// for sharp hinted coverage; a Painter/session transform applies geometry scaling.
    /// With default filtered placement, physical glyph sizes round to a quarter pixel,
    /// so continuously changing scales reuse cached rasters; quads still cover the
    /// exact requested size.
    pub const fn raster_scale(mut self, value: f32) -> Self {
        self.raster_scale = value;
        self
    }
    /// Prepares crisp axis-aligned outline masks for a known physical draw origin.
    ///
    /// Pass the final origin **after** the draw and Painter/session transforms,
    /// relative to the viewport. The final transform must be a positive uniform
    /// scale equal to `raster_scale`. Glyph baselines are rounded to quarter-pixel
    /// phases (at most 1/8 physical pixel error); masks use integer pixel quads and
    /// the exact physical font size, without rescaling a size-quantized bitmap.
    /// Layout advances, measurement and hit testing are unchanged.
    ///
    /// Reuse at the same origin or an integer physical-pixel translation is sharp.
    /// Reprepare when the origin's fractional phase or scale changes, including
    /// during smooth scrolling; quarter-pixel phases avoid whole-pixel stepping.
    /// Other draws remain valid but filter the prepared images and do not guarantee
    /// pixel alignment. Use the default preparation for arbitrary animation,
    /// rotation, shear or nonuniform scale. Color and bitmap artwork is unchanged.
    /// Phase variants share the renderer's existing bounded cache/page budgets.
    pub const fn physical_origin(mut self, origin: [f32; 2]) -> Self {
        self.physical_origin = Some(origin);
        self
    }
    /// Enables or disables hinting.
    pub const fn hinting(mut self, value: bool) -> Self {
        self.hinting = value;
        self
    }
}
/// Outline distance-field preparation, independent of font size and drawing transforms.
///
/// Generates an unhinted MTSDF: RGB encodes sharp-corner signed distance and alpha
/// encodes true signed distance. Fill rendering uses RGB; effects are not implemented.
/// Color/bitmap glyphs retain their intrinsic artwork, rasterized at the physical
/// font size with hinting. Keep coverage preparation for small text that needs hinting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MtsdfOptions {
    /// Generation density in texels per EM, default 64. Valid range is `16..=256`.
    pub pixels_per_em: u32,
    /// Full signed distance range in EM, default 0.25 (distances -0.125 through +0.125).
    /// Must be finite, positive, at most 1 EM, and span at least two generation texels.
    /// The image includes half this range outside the outline plus a filtering guard.
    pub range_em: f32,
    /// Positive finite raster texels per layout unit for color/bitmap fallback, default 1.
    /// Outline density comes from pixels_per_em; geometry always retains layout units.
    pub raster_scale: f32,
}
impl Default for MtsdfOptions {
    fn default() -> Self {
        Self::new()
    }
}
impl MtsdfOptions {
    /// Creates 64-texel/EM fields with a 0.25-EM full distance range and 1:1 geometry.
    pub const fn new() -> Self {
        Self {
            pixels_per_em: 64,
            range_em: 0.25,
            raster_scale: 1.,
        }
    }
    /// Selects generation density. Higher values cost more preparation time and atlas space.
    pub const fn pixels_per_em(mut self, value: u32) -> Self {
        self.pixels_per_em = value;
        self
    }
    /// Selects the full signed range, split symmetrically around the outline boundary.
    pub const fn range_em(mut self, value: f32) -> Self {
        self.range_em = value;
        self
    }
    /// Selects fallback image density without changing geometry or outline cache keys.
    pub const fn raster_scale(mut self, value: f32) -> Self {
        self.raster_scale = value;
        self
    }
}
/// Explicit glyph representation selected before recording draws.
///
/// `TextRasterOptions` and `MtsdfOptions` convert into this enum, so callers can
/// pass either directly to `TextRenderer::prepare_text` or `Painter::prepare_text`.
/// Default preparation is hinted coverage; no automatic representation switching occurs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextPreparation {
    /// Size-dependent coverage images and intrinsic color artwork.
    Coverage(TextRasterOptions),
    /// Size-independent outline fields, with size-dependent color/bitmap fallback.
    Mtsdf(MtsdfOptions),
}
impl Default for TextPreparation {
    fn default() -> Self {
        TextRasterOptions::new().into()
    }
}
impl From<TextRasterOptions> for TextPreparation {
    fn from(value: TextRasterOptions) -> Self {
        Self::Coverage(value)
    }
}
impl From<MtsdfOptions> for TextPreparation {
    fn from(value: MtsdfOptions) -> Self {
        Self::Mtsdf(value)
    }
}
impl TextPreparation {
    /// Raster texels per layout unit for coverage/color images; geometry is unchanged.
    pub const fn raster_scale(self) -> f32 {
        match self {
            Self::Coverage(v) => v.raster_scale,
            Self::Mtsdf(v) => v.raster_scale,
        }
    }
}
/// Placement and mask color for text geometry in its original layout units.
///
/// Identity draws interpret these units as viewport-relative pixels. An explicit
/// draw or Painter transform can convert application logical units to pixels.
/// Raster density never applies another geometry scale.
///
/// `color` is the default for glyphs without a span color. Span colors replace its RGB
/// unless [`Self::span_colors`] is disabled; `color`'s alpha and `opacity` multiply
/// every glyph and decoration, so fading a rich text needs no new preparation.
#[derive(Clone, Copy, Debug)]
pub struct TextDraw {
    /// Origin offset before the affine transform.
    pub origin: [f32; 2],
    /// Linear straight RGBA for coverage glyphs; alpha also affects intrinsic color glyphs.
    pub color: [f32; 4],
    /// Additional opacity in `0..=1`, applied to monochrome and intrinsic color glyphs.
    pub opacity: f32,
    /// Transform of layout-unit geometry and origin; X right/Y down.
    pub transform: Transform2D,
    /// Whether [`super::TextSpan`] colors replace `color`'s RGB for their glyphs and
    /// decorations, the default. Disable to draw everything in `color`, for example
    /// to recolor selected text with a second clipped draw.
    pub span_colors: bool,
}
impl Default for TextDraw {
    fn default() -> Self {
        Self::new([0., 0.])
    }
}
impl TextDraw {
    /// Creates white text at the given layout-unit origin with opacity one.
    pub const fn new(origin: [f32; 2]) -> Self {
        Self {
            origin,
            color: [1.; 4],
            opacity: 1.,
            transform: Transform2D::IDENTITY,
            span_colors: true,
        }
    }
    /// Selects whether span colors apply; disabled draws use `color` for every mask glyph.
    pub const fn span_colors(mut self, value: bool) -> Self {
        self.span_colors = value;
        self
    }
    /// Selects linear straight RGBA; RGB affects masks, alpha affects all glyphs.
    pub const fn color(mut self, value: [f32; 4]) -> Self {
        self.color = value;
        self
    }
    /// Selects additional opacity.
    pub const fn opacity(mut self, value: f32) -> Self {
        self.opacity = value;
        self
    }
    /// Selects an affine geometry transform. Coverage images filter; fields reconstruct fill.
    pub fn transform(mut self, value: impl Into<Transform2D>) -> Self {
        self.transform = value.into();
        self
    }
}
/// Cumulative renderer work counters and current atlas/cache occupancy.
#[derive(Clone, Copy, Debug, Default)]
pub struct TextRendererStats {
    /// Glyph preparation cache hits, including blank entries.
    pub cache_hits: u64,
    /// Glyph preparation cache misses.
    pub cache_misses: u64,
    /// Atlas upload payload bytes, excluding implicit texture initialization.
    pub uploaded_bytes: u64,
    /// Glyph geometry payload bytes uploaded during preparation, excluding allocation padding.
    pub geometry_bytes: u64,
    /// New glyph geometry buffer allocations. Empty preparations allocate nothing.
    pub geometry_buffer_allocations: u64,
    /// Geometry uploads that reused a buffer released by its last text/recording/completion owner.
    pub geometry_buffer_reuses: u64,
    /// Recording-owned placement/color payload bytes, 48 per nonempty text draw.
    pub parameter_bytes: u64,
    /// GPU draws recorded, one per consecutive atlas-page batch.
    pub draw_calls: u64,
    /// Live atlas page allocations, including recording and GPU completion leases.
    pub live_pages: usize,
    /// Logical atlas texture bytes for those live allocations, excluding backend overhead.
    pub atlas_bytes: usize,
    /// Current bounded cache key count.
    pub cached_glyphs: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Mask,
    Color,
    Mtsdf,
}
impl Kind {
    fn bytes(self) -> usize {
        if self == Self::Mask { 1 } else { 4 }
    }
}
#[derive(Debug)]
struct Budget {
    pages: AtomicUsize,
    bytes: AtomicUsize,
}
#[derive(Debug)]
struct Page {
    kind: Kind,
    texture: wgpu::Texture,
    group: wgpu::BindGroup,
    allocation: Arc<Allocation>,
    // Renderer clock of the last cache hit or allocation, for LRU page reclaim.
    last: AtomicU64,
    // Cache entries referencing this page, so leases are detected without a cache scan.
    cache_refs: AtomicUsize,
}
impl Page {
    /// No prepared text, recording or GPU completion holds the page; only the renderer's
    /// shelf and cache entries do.
    fn unleased(self: &Arc<Self>) -> bool {
        Arc::strong_count(self) == 1 + self.cache_refs.load(Ordering::Relaxed)
            && Arc::strong_count(&self.allocation) == 1
    }
}
#[derive(Debug)]
struct Allocation {
    bytes: usize,
    budget: Arc<Budget>,
}
impl Drop for Allocation {
    fn drop(&mut self) {
        self.budget.pages.fetch_sub(1, Ordering::Relaxed);
        self.budget.bytes.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}
#[derive(Debug)]
struct Shelf {
    page: Arc<Page>,
    x: u32,
    y: u32,
    row: u32,
}
impl Shelf {
    fn allocate(&mut self, w: u32, h: u32, size: u32) -> Option<[u32; 2]> {
        let (mut x, mut y, mut row) = (self.x, self.y, self.row);
        if x + w > size {
            x = 0;
            y += row;
            row = 0;
        }
        if y + h > size {
            return None;
        }
        self.x = x + w;
        self.y = y;
        self.row = row.max(h);
        Some([x + 1, y + 1])
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    font: FontId,
    glyph: u16,
    weight: u16,
    representation: Representation,
    italic: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Representation {
    Coverage {
        size: u32,
        hint: bool,
        phase: [u8; 2],
    },
    Mtsdf {
        pixels_per_em: u32,
        range_em: u32,
    },
}
#[derive(Clone, Debug)]
enum GlyphSource {
    Image(GlyphImage),
    Blank,
    // Temporary atlas pressure; never cached.
    Unavailable,
    // Cached outline-source classification; image lookup uses a size-dependent coverage key.
    RasterFallback,
}
#[derive(Clone, Debug)]
struct GlyphImage {
    page: Arc<Page>,
    uv: [f32; 4],
    outline_mask: bool,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}
#[derive(Debug)]
struct Cached {
    source: GlyphSource,
    last: u64,
}
impl Cached {
    fn new(source: GlyphSource, last: u64) -> Self {
        if let GlyphSource::Image(image) = &source {
            image.page.cache_refs.fetch_add(1, Ordering::Relaxed);
        }
        Self { source, last }
    }
}
impl Drop for Cached {
    fn drop(&mut self) {
        if let GlyphSource::Image(image) = &self.source {
            image.page.cache_refs.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
/// What [`TextRenderer::trim`] released.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextTrim {
    /// Atlas pages released from the budget.
    pub pages: usize,
    /// Logical atlas texture bytes of those pages.
    pub atlas_bytes: usize,
    /// Cached glyph keys dropped with them; they rasterize again on next use.
    pub cached_glyphs: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GlyphData {
    rect: [f32; 4],
    uv: [f32; 4],
    // [kind, red|green f16, blue|alpha f16, has color]. Kind 0 masks, 1 color images,
    // 2 distance fields, 3 solid decoration rectangles with UVs over the true rectangle.
    kind: [u32; 4],
}
impl GlyphData {
    fn style(kind: u32, color: Option<[f32; 4]>) -> [u32; 4] {
        let [r, g, b, a] = color.unwrap_or([0.; 4]).map(f16_bits);
        [
            kind,
            u32::from(r) | u32::from(g) << 16,
            u32::from(b) | u32::from(a) << 16,
            u32::from(color.is_some()),
        ]
    }
}
/// IEEE binary16 bits of `value`, rounding to nearest even; out-of-range values
/// become infinity. Matches WGSL `unpack2x16float`.
fn f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let magnitude = bits & 0x7fff_ffff;
    if magnitude >= 0x4780_0000 {
        // 65536 and above, infinity or NaN.
        return sign
            | if magnitude > 0x7f80_0000 {
                0x7e00
            } else {
                0x7c00
            };
    }
    if magnitude < 0x3880_0000 {
        // Below the smallest normal half: a subnormal multiple of 2^-24.
        return sign | (f32::from_bits(magnitude) * 16_777_216.).round_ties_even() as u16;
    }
    let rounded = magnitude + 0x0fff + ((magnitude >> 13) & 1);
    sign | ((rounded - 0x3800_0000) >> 13) as u16
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct DrawData {
    origin_axis_x: [f32; 4],
    axis_y: [f32; 4],
    color: [f32; 4],
}
#[derive(Debug)]
struct TextDraft {
    glyphs: Vec<GlyphData>,
    batches: Vec<Batch>,
    size: [f32; 2],
    bounds: Option<Rect>,
    skipped: Vec<usize>,
    atlas_full: bool,
    preparation: TextPreparation,
}

#[derive(Debug)]
struct Batch {
    page: Arc<Page>,
    range: Range<u32>,
}
#[derive(Debug)]
enum GeometryBuffer {
    Dedicated(Geometry),
    Shared(Arc<Geometry>),
}
impl std::ops::Deref for GeometryBuffer {
    type Target = Geometry;
    fn deref(&self) -> &Geometry {
        match self {
            Self::Dedicated(geometry) => geometry,
            Self::Shared(geometry) => geometry,
        }
    }
}

#[derive(Debug)]
struct PreparedData {
    buffer: Option<GeometryBuffer>,
    buffer_range: Range<u64>,
    batches: Vec<Batch>,
    raster_pipeline: bool,
    field_pipeline: bool,
    lease: Option<Arc<TextLease>>,
}
// GPU completion callbacks require Send even on WebGPU, whose buffer/texture
// handles are not Send. Retain only ownership tokens in the callback; wgpu owns
// the actual resources referenced by recorded commands.
#[derive(Debug)]
struct TextLease {
    _geometry: Option<Arc<()>>,
    _pages: Vec<Arc<Allocation>>,
}
/// Immutable glyph geometry and atlas leases, independent of later preparation/cache clearing.
/// Clones share GPU storage. Draws upload only a 48-byte placement/color record, not glyph geometry.
#[derive(Clone, Debug)]
pub struct PreparedText {
    graphics: GraphicsContext,
    data: Arc<PreparedData>,
    size: [f32; 2],
    bounds: Option<Rect>,
    glyph_count: usize,
    skipped: Arc<[usize]>,
    atlas_full: bool,
    preparation: TextPreparation,
}
impl PreparedText {
    /// Advance/line-box measurement in the original layout units, independent of raster density.
    pub fn size(&self) -> [f32; 2] {
        self.size
    }
    /// Image-quad bounds in layout units, distinct from advance/line-box measurement.
    /// Coverage rounding/hinting can change these bounds with raster density.
    /// Distance-field quads include distance-range padding and filtering guards.
    pub fn ink_bounds(&self) -> Option<Rect> {
        self.bounds
    }
    /// Number of drawable quads: glyph images plus decoration rectangles.
    pub fn glyph_count(&self) -> usize {
        self.glyph_count
    }
    /// Whether atlas pressure omitted any glyphs during this preparation.
    /// Prepare again after releasing obsolete text and driving device progress.
    /// Blank/unsupported glyphs alone do not set this flag.
    pub fn atlas_full(&self) -> bool {
        self.atlas_full
    }
    /// Original layout glyph indexes with no raster image, including blank spaces,
    /// unsupported sources, and glyphs omitted under atlas pressure.
    pub fn skipped_glyphs(&self) -> &[usize] {
        &self.skipped
    }
    /// Representation and geometry scale selected during preparation. Color glyphs may
    /// use raster fallback even when this returns `TextPreparation::Mtsdf`.
    pub fn preparation(&self) -> TextPreparation {
        self.preparation
    }
    /// Raster image density selected during preparation, independent of geometry scale.
    pub fn raster_scale(&self) -> f32 {
        self.preparation.raster_scale()
    }
}
/// Independent coverage/color and outline-distance-field renderer. Preparation generates and uploads before drawing.
///
/// Uses R8 coverage, linear premultiplied RGBA8 color, and linear RGBA8 MTSDF pages.
/// One-texel gutters separate images; fields include additional distance-range padding.
/// Coverage/color draws use a separate fragment shader without field reconstruction.
/// Default placement filters fractional movement; explicit physical origins select phased coverage.
/// Only consecutive glyphs on the same page are instanced together, preserving layout order.
/// Leased pages are never evicted or overwritten. A full cache evicts its least recently used
/// quarter at once, and a full page budget reclaims the least recently used unleased page, so
/// neither scans the cache per glyph. When every page is leased, preparation omits
/// glyphs that need new space and records them in [`PreparedText::skipped_glyphs`].
/// Later preparation retries them without invalidating earlier text or waiting.
/// Callers control dropping resources, device polling, [`Self::trim`] and
/// [`Self::set_max_pages`]. Font-size/raster limits bound
/// image work, while Swash keeps a fixed eight-entry scaler cache and no CPU glyph-image cache.
/// Changed text can reuse released geometry buffers. Recycling is bounded to 64
/// buffers / 1 MiB (each at most 64 KiB), including buffers awaiting completion.
/// Prepared texts, clones, recordings and GPU work prevent their storage being overwritten.
/// Larger geometry remains exact-sized and is released rather than recycled.
pub struct TextRenderer {
    graphics: GraphicsContext,
    options: TextRendererOptions,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    material: Material,
    field_material: Material,
    pipelines: crate::pipelines::Pipelines,
    generator: GlyphGenerator,
    preparation_epoch: Arc<()>,
    cache: HashMap<Key, Cached>,
    pages: Vec<Shelf>,
    budget: Arc<Budget>,
    clock: u64,
    atlas_full: bool,
    stats: TextRendererStats,
    geometry_pool: Arc<Mutex<GeometryPool>>,
    solid: Option<Arc<Page>>,
}
impl std::fmt::Debug for TextRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextRenderer")
            .field("options", &self.options)
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}
impl TextRenderer {
    /// Creates a renderer with default bounded atlas settings. Never fails: the default
    /// page size is clamped to the device's texture dimension limit.
    pub fn new(graphics: &GraphicsContext) -> Self {
        Self::with_pipeline(graphics, crate::PipelineOptions::default())
    }
    /// Default atlas settings with `pipeline`, valid for any device.
    pub(crate) fn with_pipeline(g: &GraphicsContext, pipeline: crate::PipelineOptions) -> Self {
        let defaults = TextRendererOptions::default();
        Self::build(
            g,
            TextRendererOptions {
                pipeline,
                page_size: defaults
                    .page_size
                    .min(g.device().limits().max_texture_dimension_2d),
                ..defaults
            },
        )
    }
    /// Validates atlas/raster limits and creates device resources, without allocating atlas pages.
    pub fn with_options(
        g: &GraphicsContext,
        options: TextRendererOptions,
    ) -> Result<Self, TextRenderError> {
        if options.page_size < 4
            || options.page_size > g.device().limits().max_texture_dimension_2d
            || options.max_pages == 0
            || options.max_cached_glyphs == 0
            || !options.max_raster_size.is_finite()
            || options.max_raster_size <= 0.
            || options.max_raster_size > 4096.
        {
            return Err(TextRenderError::InvalidOptions);
        }
        Ok(Self::build(g, options))
    }
    fn build(g: &GraphicsContext, options: TextRendererOptions) -> Self {
        let layout = g
            .device()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("RXUI drawing text atlas"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let sampler = g.device().create_sampler(&wgpu::SamplerDescriptor {
            label: Some("RXUI drawing text filtering"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = g
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("RXUI drawing text"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::clip::shader(g, include_str!("text.wgsl")).into(),
                ),
            });
        let layouts = [
            VertexLayout {
                stride: 48,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Uint32x4]
                    .to_vec(),
            },
            VertexLayout {
                stride: 0,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: wgpu::vertex_attr_array![3=>Float32x4,4=>Float32x4,5=>Float32x4]
                    .to_vec(),
            },
            crate::clip::layout(6),
        ];
        let material = g.create_material(
            options.pipeline.mesh(
                MaterialOptions::new(&shader)
                    .vertex_layouts(&layouts)
                    .bind_group_layouts(&crate::clip::layouts(
                        g,
                        Some(&layout),
                        &g.rounded_clip_layout(),
                    ))
                    .entry_points(
                        "vertex_main",
                        if options.pipeline.writes_attachment() {
                            "fragment_covered"
                        } else {
                            "fragment_main"
                        },
                    ),
            ),
        );
        let field_material = g.create_material(
            options.pipeline.mesh(
                MaterialOptions::new(&shader)
                    .vertex_layouts(&layouts)
                    .bind_group_layouts(&crate::clip::layouts(
                        g,
                        Some(&layout),
                        &g.rounded_clip_layout(),
                    ))
                    .entry_points(
                        "vertex_main",
                        if options.pipeline.writes_attachment() {
                            "fragment_mtsdf_covered"
                        } else {
                            "fragment_mtsdf"
                        },
                    ),
            ),
        );
        Self {
            graphics: g.clone(),
            options: options.clone(),
            layout,
            sampler,
            material,
            field_material,
            pipelines: Default::default(),
            generator: GlyphGenerator::new(options.page_size),
            preparation_epoch: Arc::new(()),
            cache: HashMap::new(),
            pages: Vec::new(),
            geometry_pool: Arc::default(),
            solid: None,
            budget: Arc::new(Budget {
                pages: AtomicUsize::new(0),
                bytes: AtomicUsize::new(0),
            }),
            clock: 0,
            atlas_full: false,
            stats: Default::default(),
        }
    }
    /// Returns cumulative work and current live allocation counters.
    pub fn stats(&self) -> TextRendererStats {
        TextRendererStats {
            live_pages: self.budget.pages.load(Ordering::Relaxed),
            atlas_bytes: self.budget.bytes.load(Ordering::Relaxed),
            cached_glyphs: self.cache.len(),
            ..self.stats
        }
    }
    /// Current options, including any budget changed by [`Self::set_max_pages`].
    pub fn options(&self) -> &TextRendererOptions {
        &self.options
    }
    /// Changes the live page budget. Raising it lets omitted glyphs fit on later preparation.
    /// Lowering it below the live count releases nothing immediately; new pages wait
    /// until reclaim, [`Self::trim`] or lease release makes room.
    pub fn set_max_pages(&mut self, max_pages: usize) -> Result<(), TextRenderError> {
        if max_pages == 0 {
            return Err(TextRenderError::InvalidOptions);
        }
        self.options.max_pages = max_pages;
        Ok(())
    }
    /// Releases every atlas page that no prepared text, recording or pending GPU work holds,
    /// with the cache entries that point into them. Leased pages and blank/fallback entries
    /// stay cached. Use it after atlas pressure, or to return memory when text goes idle.
    /// Costs one pass over the pages plus one over the cache when anything is released.
    pub fn trim(&mut self) -> TextTrim {
        let mut trim = TextTrim::default();
        let mut released = Vec::new();
        self.pages.retain(|shelf| {
            if !shelf.page.unleased() {
                return true;
            }
            trim.pages += 1;
            trim.atlas_bytes += shelf.page.allocation.bytes;
            released.push(shelf.page.clone());
            false
        });
        if !released.is_empty() {
            let before = self.cache.len();
            self.cache.retain(|_, v| {
                !matches!(&v.source, GlyphSource::Image(i)
                    if released.iter().any(|p| Arc::ptr_eq(p, &i.page)))
            });
            trim.cached_glyphs = before - self.cache.len();
        }
        trim
    }
    /// Releases lookup caches, page ownership, and recycled geometry storage.
    /// Existing prepared texts/recordings remain usable. Outstanding worker requests
    /// and incremental uploads become stale and cannot repopulate the cleared cache.
    /// Live leased pages continue counting against this renderer's budget until dropped/completed.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.pages.clear();
        self.generator = GlyphGenerator::new(self.options.page_size);
        self.preparation_epoch = Arc::new(());
        self.geometry_pool = Arc::default();
    }
    fn pipeline(
        &mut self,
        color: wgpu::TextureFormat,
        sample_count: u32,
        depth_stencil: Option<wgpu::TextureFormat>,
        field: bool,
    ) -> Result<wgpu::RenderPipeline, Error> {
        Ok(crate::pipelines::pipeline(
            &self.graphics,
            &mut self.pipelines,
            if field {
                &self.field_material
            } else {
                &self.material
            },
            color,
            sample_count,
            depth_stencil,
            true,
        )
        .map_err(|error| match error {
            Error::UnsupportedMeshFormat { format } => Error::UnsupportedTextFormat { format },
            other => other,
        })?
        .clone())
    }
    /// Prepares coverage and MTSDF pipelines without shaping or uploading glyphs.
    pub fn prepare(&mut self, format: &RenderFormat) -> Result<(), Error> {
        let color = format.single_color()?;
        {
            profiling::scope!("rxui_draw::prepare_coverage_text_pipeline");
            self.pipeline(color, format.sample_count, format.depth_stencil, false)?;
        }
        {
            profiling::scope!("rxui_draw::prepare_mtsdf_text_pipeline");
            self.pipeline(color, format.sample_count, format.depth_stencil, true)?;
        }
        Ok(())
    }
    /// Prepares a surface target's attachment variants, checking device identity.
    pub fn prepare_for_target(&mut self, target: &crate::RenderTarget<'_>) -> Result<(), Error> {
        if !self.graphics.same_device(target.graphics()) {
            return Err(Error::DeviceMismatch);
        }
        self.prepare(&target.render_format())
    }
    /// Generates missing glyph images and creates immutable geometry. Coverage is size-dependent;
    /// MTSDF outlines reuse images across font sizes/DPI while color/bitmap fallback remains
    /// size-dependent. Pass either `TextRasterOptions` or `MtsdfOptions` directly.
    /// Cold field generation can be expensive: use [`Self::request_glyphs`] for application-worker
    /// generation and explicitly bounded commits, or prepare ahead of drawing.
    /// No TextSystem borrow is
    /// needed: layouts retain font sources. Preparation can populate/evict caches even on failure;
    /// previous prepared texts remain valid. Repeated preparation reuses atlas images but creates
    /// a new geometry payload, reusing released buffers when available; keep PreparedText
    /// for unchanged content. Unsupported/blank
    /// sources are skipped and reported by original glyph index. Physical coverage/color sizes
    /// must fit `max_raster_size`; pure outline fields instead use bounded generation density.
    pub fn prepare_text(
        &mut self,
        layout: &TextLayout,
        preparation: impl Into<TextPreparation>,
    ) -> Result<PreparedText, TextRenderError> {
        profiling::scope!("rxui_draw::prepare_text");
        let draft = self.prepare_geometry(layout, preparation.into(), None)?;
        let bytes = draft.glyphs.len() as u64 * 48;
        let geometry = (!draft.glyphs.is_empty())
            .then(|| GeometryBuffer::Dedicated(self.upload_geometry(&draft.glyphs)));
        Ok(self.finish_text(draft, geometry, 0..bytes, false))
    }
    /// Prepares layouts in input order, sharing geometry uploads for consecutive small texts.
    ///
    /// Accepts owned/borrowed layouts and `Arc<TextLayout>` collections. Every result
    /// remains independently drawable, with its own measurement, skipped glyphs and
    /// atlas-page ownership. Coverage/MTSDF settings and scale apply to all inputs.
    /// No shaping, command recording, submission, or completion wait occurs here.
    ///
    /// Small texts share buffers containing at most 64 KiB of glyph payload; a text
    /// is never split between buffers. Larger texts use a dedicated exact-sized buffer.
    /// Retaining one result or recording keeps its shared geometry buffer alive, but
    /// does not keep other results' atlas pages leased. Empty/blank layouts produce
    /// corresponding empty results without geometry uploads or storage leases.
    ///
    /// An error returns no partial result vector. Earlier work may populate caches
    /// and upload geometry, as with repeated [`Self::prepare_text`] calls; existing
    /// resources stay valid. Retain the results until layout/settings change.
    ///
    /// ```no_run
    /// # use rxui_draw::{TextRenderer, TextLayout, TextRasterOptions, TextRenderError};
    /// # use std::sync::Arc;
    /// fn prepare(renderer: &mut TextRenderer, layouts: &[Arc<TextLayout>], dpi: f32)
    ///     -> Result<(), TextRenderError> {
    ///     let texts = renderer.prepare_texts(layouts,
    ///         TextRasterOptions::new().raster_scale(dpi))?;
    ///     assert_eq!(texts.len(), layouts.len());
    ///     // Store these resources; each may be drawn with its own placement/style.
    ///     Ok(())
    /// }
    /// ```
    pub fn prepare_texts<L: AsRef<TextLayout>>(
        &mut self,
        layouts: impl IntoIterator<Item = L>,
        preparation: impl Into<TextPreparation>,
    ) -> Result<Vec<PreparedText>, TextRenderError> {
        profiling::scope!("rxui_draw::prepare_texts");
        let preparation = preparation.into();
        // Validate settings even for an empty collection.
        validate_preparation(preparation)?;
        let mut result = Vec::new();
        let mut pending = Vec::new();
        let mut glyphs = Vec::new();
        let chunk_bytes = (64 * 1024).min(self.graphics.device().limits().max_buffer_size);
        for layout in layouts {
            let mut draft = self.prepare_geometry(layout.as_ref(), preparation, None)?;
            let bytes = draft.glyphs.len() as u64 * 48;
            if bytes > chunk_bytes {
                self.flush_texts(&mut result, &mut pending, &mut glyphs);
                let geometry = self.upload_geometry(&draft.glyphs);
                result.push(self.finish_text(
                    draft,
                    Some(GeometryBuffer::Dedicated(geometry)),
                    0..bytes,
                    true,
                ));
                continue;
            }
            if (glyphs.len() as u64 * 48) + bytes > chunk_bytes {
                self.flush_texts(&mut result, &mut pending, &mut glyphs);
            }
            let start = glyphs.len() as u64 * 48;
            glyphs.extend(std::mem::take(&mut draft.glyphs));
            pending.push((draft, start..start + bytes));
        }
        self.flush_texts(&mut result, &mut pending, &mut glyphs);
        Ok(result)
    }
    fn flush_texts(
        &mut self,
        result: &mut Vec<PreparedText>,
        pending: &mut Vec<(TextDraft, Range<u64>)>,
        glyphs: &mut Vec<GlyphData>,
    ) {
        let geometry = (!glyphs.is_empty()).then(|| Arc::new(self.upload_geometry(glyphs)));
        for (draft, range) in pending.drain(..) {
            let buffer = (!range.is_empty())
                .then(|| GeometryBuffer::Shared(geometry.as_ref().unwrap().clone()));
            result.push(self.finish_text(draft, buffer, range, true));
        }
        glyphs.clear();
    }
    fn prepare_geometry(
        &mut self,
        layout: &TextLayout,
        preparation: TextPreparation,
        resolved: Option<&HashMap<Key, GlyphSource>>,
    ) -> Result<TextDraft, TextRenderError> {
        validate_preparation(preparation)?;
        let scale = preparation.raster_scale();
        if !scale.is_finite()
            || scale <= 0.
            || layout.glyphs().iter().any(|g| {
                let size = g.font_size * scale;
                !size.is_finite()
                    || size <= 0.
                    || (matches!(preparation, TextPreparation::Coverage(_))
                        && size > self.options.max_raster_size)
            })
        {
            return Err(TextRenderError::InvalidOptions);
        }
        let inverse_scale = 1. / scale;
        let size = layout.size();
        let quads = layout.glyphs().len() + layout.decorations().len();
        if size.iter().any(|v| !v.is_finite())
            || (quads as u64) * 48 > self.graphics.device().limits().max_buffer_size
            || quads > u32::MAX as usize
        {
            return Err(TextRenderError::InvalidOptions);
        }
        let mut data = Vec::with_capacity(layout.glyphs().len());
        let mut batches: Vec<Batch> = Vec::new();
        let mut skipped = Vec::new();
        let mut atlas_full = false;
        let mut bounds: Option<[f32; 4]> = None;
        for (index, glyph) in layout.glyphs().iter().enumerate() {
            let font = &layout.fonts()[glyph.font_index];
            let physical_size = glyph.font_size * scale;
            let raster_size = raster_size(physical_size, self.options.max_raster_size);
            let coverage = match preparation {
                TextPreparation::Coverage(raster) => Representation::Coverage {
                    size: raster_size.to_bits(),
                    hint: raster.hinting,
                    phase: [0; 2],
                },
                TextPreparation::Mtsdf(_) => Representation::Coverage {
                    size: raster_size.to_bits(),
                    hint: true,
                    phase: [0; 2],
                },
            };
            let key = Key {
                font: font.id(),
                glyph: glyph.glyph_id,
                weight: font.weight(),
                representation: match preparation {
                    TextPreparation::Coverage(_) => coverage,
                    TextPreparation::Mtsdf(v) => Representation::Mtsdf {
                        pixels_per_em: v.pixels_per_em,
                        range_em: v.range_em.to_bits(),
                    },
                },
                italic: glyph.synthetic_italic,
            };
            let source = self.resolve_image(key, font, resolved)?;
            let source = if matches!(source, GlyphSource::RasterFallback) {
                if physical_size > self.options.max_raster_size {
                    return Err(TextRenderError::InvalidOptions);
                }
                self.resolve_image(
                    Key {
                        representation: coverage,
                        ..key
                    },
                    font,
                    resolved,
                )?
            } else {
                source
            };
            let GlyphSource::Image(mut image) = source else {
                atlas_full |= matches!(source, GlyphSource::Unavailable);
                skipped.push(index);
                continue;
            };
            let mut position = glyph.position;
            let mut phased = false;
            if image.outline_mask
                && let TextPreparation::Coverage(raster) = preparation
                && let Some(origin) = raster.physical_origin
            {
                // Integer translation cannot affect the phase. Reduce it first to
                // avoid cancellation when placing reusable text far from the origin.
                let origin = origin.map(|v| v.rem_euclid(1.));
                let physical =
                    std::array::from_fn::<_, 2, _>(|i| glyph.position[i] * scale + origin[i]);
                if physical.iter().any(|v| !v.is_finite()) {
                    return Err(TextRenderError::InvalidOptions);
                }
                let (x, px) = coverage_phase(physical[0]);
                let (y, py) = coverage_phase(physical[1]);
                let source = self.resolve_image(
                    Key {
                        representation: Representation::Coverage {
                            size: physical_size.to_bits(),
                            hint: raster.hinting,
                            phase: [px, py],
                        },
                        ..key
                    },
                    font,
                    resolved,
                )?;
                let GlyphSource::Image(phased_image) = source else {
                    atlas_full |= matches!(source, GlyphSource::Unavailable);
                    skipped.push(index);
                    continue;
                };
                image = phased_image;
                position = [
                    (x - origin[0]) * inverse_scale,
                    (y - origin[1]) * inverse_scale,
                ];
                phased = true;
            }
            let factor = if phased {
                inverse_scale
            } else if let Representation::Mtsdf { pixels_per_em, .. } = key.representation
                && image.page.kind == Kind::Mtsdf
            {
                glyph.font_size / pixels_per_em as f32
            } else if raster_size == physical_size {
                inverse_scale
            } else {
                // Map the rounded raster back to the requested size.
                glyph.font_size / raster_size
            };
            let x = position[0] + image.left as f32 * factor;
            let y = position[1] - image.top as f32 * factor;
            let w = image.width as f32 * factor;
            let h = image.height as f32 * factor;
            if [x, y, x + w, y + h].iter().any(|v| !v.is_finite()) {
                return Err(TextRenderError::InvalidOptions);
            }
            bounds = Some(match bounds {
                None => [x, y, x + w, y + h],
                Some(b) => [b[0].min(x), b[1].min(y), b[2].max(x + w), b[3].max(y + h)],
            });
            let start = data.len() as u32;
            data.push(GlyphData {
                rect: [x, y, w, h],
                uv: image.uv,
                kind: GlyphData::style(
                    match image.page.kind {
                        Kind::Mask => 0,
                        Kind::Color => 1,
                        Kind::Mtsdf => 2,
                    },
                    glyph.color,
                ),
            });
            if let Some(batch) = batches
                .last_mut()
                .filter(|b| Arc::ptr_eq(&b.page, &image.page))
            {
                batch.range.end += 1;
            } else {
                batches.push(Batch {
                    page: image.page,
                    range: start..start + 1,
                });
            }
        }
        if !layout.decorations().is_empty() {
            // Decorations follow every glyph in one batch on a 1x1 page. Quads extend
            // by one raster texel so the shader can filter their edges.
            let pad = inverse_scale;
            let start = data.len() as u32;
            for decoration in layout.decorations() {
                let r = decoration.rect;
                let quad = [
                    r.x - pad,
                    r.y - pad,
                    r.width + 2. * pad,
                    r.height + 2. * pad,
                ];
                if quad
                    .iter()
                    .chain(&[quad[0] + quad[2], quad[1] + quad[3]])
                    .any(|v| !v.is_finite())
                {
                    return Err(TextRenderError::InvalidOptions);
                }
                bounds = Some(match bounds {
                    None => [quad[0], quad[1], quad[0] + quad[2], quad[1] + quad[3]],
                    Some(b) => [
                        b[0].min(quad[0]),
                        b[1].min(quad[1]),
                        b[2].max(quad[0] + quad[2]),
                        b[3].max(quad[1] + quad[3]),
                    ],
                });
                data.push(GlyphData {
                    rect: quad,
                    uv: [
                        -pad / r.width,
                        -pad / r.height,
                        quad[2] / r.width,
                        quad[3] / r.height,
                    ],
                    kind: GlyphData::style(3, decoration.color),
                });
            }
            batches.push(Batch {
                page: self.solid_page(),
                range: start..data.len() as u32,
            });
        }
        if bounds.is_some_and(|b| !(b[2] - b[0]).is_finite() || !(b[3] - b[1]).is_finite()) {
            return Err(TextRenderError::InvalidOptions);
        }
        Ok(TextDraft {
            glyphs: data,
            batches,
            size,
            bounds: bounds.map(|b| Rect::new(b[0], b[1], b[2] - b[0], b[3] - b[1])),
            skipped,
            atlas_full,
            preparation,
        })
    }
    // A blank 1x1 page bound for decoration batches, which never sample it.
    // Its allocation is outside the atlas budget.
    fn solid_page(&mut self) -> Arc<Page> {
        if let Some(page) = &self.solid {
            return page.clone();
        }
        let texture = self
            .graphics
            .device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("RXUI drawing text decorations"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
        let group = self.bind_page(&texture);
        let page = Arc::new(Page {
            kind: Kind::Mask,
            texture,
            group,
            allocation: Arc::new(Allocation {
                bytes: 0,
                budget: Arc::new(Budget {
                    pages: AtomicUsize::new(1),
                    bytes: AtomicUsize::new(0),
                }),
            }),
            last: AtomicU64::new(0),
            cache_refs: AtomicUsize::new(0),
        });
        self.solid = Some(page.clone());
        page
    }
    fn bind_page(&self, texture: &wgpu::Texture) -> wgpu::BindGroup {
        let view = texture.create_view(&Default::default());
        self.graphics
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("RXUI drawing glyph atlas binding"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
    }
    fn upload_geometry(&mut self, glyphs: &[GlyphData]) -> Geometry {
        let (geometry, reused) = GeometryPool::upload(
            &self.geometry_pool,
            &self.graphics,
            "RXUI drawing prepared glyphs",
            wgpu::BufferUsages::VERTEX,
            &[bytemuck::cast_slice(glyphs)],
        );
        if reused {
            self.stats.geometry_buffer_reuses += 1;
        } else {
            self.stats.geometry_buffer_allocations += 1;
        }
        self.stats.geometry_bytes += glyphs.len() as u64 * 48;
        geometry
    }
    fn finish_text(
        &self,
        draft: TextDraft,
        buffer: Option<GeometryBuffer>,
        buffer_range: Range<u64>,
        shared: bool,
    ) -> PreparedText {
        let glyph_count = (buffer_range.end - buffer_range.start) as usize / 48;
        let lease = (!shared).then(|| {
            Arc::new(TextLease {
                _geometry: buffer.as_ref().map(|geometry| geometry.lease.clone()),
                _pages: draft
                    .batches
                    .iter()
                    .map(|batch| batch.page.allocation.clone())
                    .collect(),
            })
        });
        PreparedText {
            graphics: self.graphics.clone(),
            data: Arc::new(PreparedData {
                raster_pipeline: draft.batches.iter().any(|b| b.page.kind != Kind::Mtsdf),
                field_pipeline: draft.batches.iter().any(|b| b.page.kind == Kind::Mtsdf),
                buffer,
                buffer_range,
                batches: draft.batches,
                lease,
            }),
            size: draft.size,
            bounds: draft.bounds,
            glyph_count,
            skipped: draft.skipped.into(),
            atlas_full: draft.atlas_full,
            preparation: draft.preparation,
        }
    }
    /// Records ordered prepared batches into a frame-owned pass, honoring its viewport/scissor.
    /// Restores owned pipeline/bindings/raster state each call; never shapes or rasterizes.
    /// Default shading ignores depth/stencil; explicit pipeline options select tests/writes.
    /// Attachment compatibility and read-only restrictions are checked before drawing.
    pub fn draw(
        &mut self,
        pass: &mut RenderPass<'_>,
        text: &PreparedText,
        draw: TextDraw,
    ) -> Result<(), Error> {
        if !pass.same_device(&self.graphics) || !self.graphics.same_device(&text.graphics) {
            return Err(Error::DeviceMismatch);
        }
        let parameters = draw_parameters(text.bounds, draw, pass.viewport_size())?;
        crate::pipelines::validate_aspects(pass, &self.material)?;
        let Some(buffer) = &text.data.buffer else {
            return Ok(());
        };
        let color = pass.single_color_format()?;
        // Validate/create every required variant before recording commands.
        let raster_pipeline = if text.data.raster_pipeline {
            Some(self.pipeline(
                color,
                pass.sample_count(),
                pass.depth_stencil_format(),
                false,
            )?)
        } else {
            None
        };
        let field_pipeline = if text.data.field_pipeline {
            Some(self.pipeline(
                color,
                pass.sample_count(),
                pass.depth_stencil_format(),
                true,
            )?)
        } else {
            None
        };
        let (draw_buffer, range) = pass.upload_instances(bytemuck::bytes_of(&parameters), 4);
        self.stats.parameter_bytes += 48;
        // Keep both atlas and geometry immutable through recording and GPU completion.
        if let Some(lease) = &text.data.lease {
            pass.retain_resource(lease.clone());
        } else {
            // Shared geometry needs one token per buffer, not one per label.
            // Atlas ownership stays specific to the text's actual page batches.
            pass.retain_resource(buffer.lease.clone());
            for batch in &text.data.batches {
                pass.retain_resource(batch.page.allocation.clone());
            }
        }
        pass.apply_raster_state();
        // Bind the whole geometry buffer so labels sharing it keep one binding;
        // the text's offset into it moves to the first instance.
        pass.set_vertex_buffer(0, &buffer.buffer, 0..buffer.buffer.size());
        pass.set_vertex_buffer(1, &draw_buffer, range);
        let base = (text.data.buffer_range.start / 48) as u32;
        pass.bind_clip(2);
        pass.bind_rounded_clips();
        let mut previous = None;
        for batch in &text.data.batches {
            let field = batch.page.kind == Kind::Mtsdf;
            if previous != Some(field) {
                pass.set_pipeline(if field {
                    field_pipeline.as_ref().unwrap()
                } else {
                    raster_pipeline.as_ref().unwrap()
                });
                previous = Some(field);
            }
            pass.set_bind_group(0, &batch.page.group, &[]);
            pass.draw(0..6, base + batch.range.start..base + batch.range.end);
            self.stats.draw_calls += 1;
        }
        Ok(())
    }
    fn resolve_image(
        &mut self,
        key: Key,
        font: &TextFont,
        resolved: Option<&HashMap<Key, GlyphSource>>,
    ) -> Result<GlyphSource, TextRenderError> {
        match resolved {
            Some(images) => images
                .get(&key)
                .cloned()
                .ok_or(TextRenderError::PreparationIncomplete),
            None => self.image(key, font),
        }
    }
    fn image(&mut self, key: Key, font: &TextFont) -> Result<GlyphSource, TextRenderError> {
        match self.try_image(key, font) {
            Err(TextRenderError::AtlasFull) => {
                if !self.atlas_full {
                    log::warn!(
                        "text atlas is fully leased; omitting glyphs until space is available"
                    );
                    self.atlas_full = true;
                }
                // Do not cache this as blank: later preparation must retry the glyph.
                Ok(GlyphSource::Unavailable)
            }
            result => result,
        }
    }
    fn try_image(&mut self, key: Key, font: &TextFont) -> Result<GlyphSource, TextRenderError> {
        self.clock += 1;
        if let Some(cached) = self.cache.get_mut(&key) {
            self.stats.cache_hits += 1;
            cached.last = self.clock;
            if let GlyphSource::Image(image) = &cached.source {
                image.page.last.store(self.clock, Ordering::Relaxed);
            }
            return Ok(cached.source.clone());
        }
        self.stats.cache_misses += 1;
        let generated = self.generator.generate(key, font)?;
        let source = self.upload_generated(&generated)?;
        if self.cache.len() >= self.options.max_cached_glyphs {
            self.evict_glyphs();
        }
        self.cache
            .insert(key, Cached::new(source.clone(), self.clock));
        Ok(source)
    }
    /// Drops the least recently used quarter of the cache in one pass, so a full cache
    /// costs amortized constant time per miss instead of a scan for each.
    fn evict_glyphs(&mut self) {
        let len = self.cache.len();
        let count = (len / 4).max(len + 1 - self.options.max_cached_glyphs.min(len + 1));
        let mut ages: Vec<u64> = self.cache.values().map(|v| v.last).collect();
        // Clock values are unique per entry, so this keeps exactly `len - count` entries.
        let (_, &mut threshold, _) = ages.select_nth_unstable(count - 1);
        self.cache.retain(|_, v| v.last > threshold);
    }
    fn upload_generated(&mut self, generated: &CpuGlyph) -> Result<GlyphSource, TextRenderError> {
        Ok(match generated {
            CpuGlyph::Image(image, kind) => GlyphSource::Image(self.upload(image, *kind)?),
            CpuGlyph::Blank => GlyphSource::Blank,
            CpuGlyph::RasterFallback => GlyphSource::RasterFallback,
        })
    }
    fn upload(&mut self, image: &Image, kind: Kind) -> Result<GlyphImage, TextRenderError> {
        let (w, h) = (image.placement.width, image.placement.height);
        let size = self.options.page_size;
        if w.checked_add(2).is_none_or(|v| v > size) || h.checked_add(2).is_none_or(|v| v > size) {
            return Err(TextRenderError::GlyphTooLarge);
        }
        if image.data.len() != (w as usize) * (h as usize) * kind.bytes() {
            return Err(TextRenderError::InvalidRaster);
        }
        let mut allocation = None;
        for shelf in &mut self.pages {
            if shelf.page.kind == kind
                && let Some(pos) = shelf.allocate(w + 2, h + 2, size)
            {
                shelf.page.last.store(self.clock, Ordering::Relaxed);
                allocation = Some((shelf.page.clone(), pos));
                break;
            }
        }
        let (page, pos) = match allocation {
            Some(a) => a,
            None => {
                self.reclaim_page();
                if self.budget.pages.load(Ordering::Relaxed) >= self.options.max_pages {
                    return Err(TextRenderError::AtlasFull);
                }
                let texture = self
                    .graphics
                    .device()
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("RXUI drawing glyph atlas page"),
                        size: wgpu::Extent3d {
                            width: size,
                            height: size,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: if kind == Kind::Mask {
                            wgpu::TextureFormat::R8Unorm
                        } else {
                            wgpu::TextureFormat::Rgba8Unorm
                        },
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                let group = self.bind_page(&texture);
                let bytes = (size as usize) * (size as usize) * kind.bytes();
                self.budget.pages.fetch_add(1, Ordering::Relaxed);
                self.budget.bytes.fetch_add(bytes, Ordering::Relaxed);
                let page = Arc::new(Page {
                    kind,
                    texture,
                    group,
                    allocation: Arc::new(Allocation {
                        bytes,
                        budget: self.budget.clone(),
                    }),
                    last: AtomicU64::new(self.clock),
                    cache_refs: AtomicUsize::new(0),
                });
                let mut shelf = Shelf {
                    page: page.clone(),
                    x: 0,
                    y: 0,
                    row: 0,
                };
                let pos = shelf.allocate(w + 2, h + 2, size).unwrap();
                self.pages.push(shelf);
                (page, pos)
            }
        };
        self.graphics.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &page.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: pos[0],
                    y: pos[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &image.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * kind.bytes() as u32),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.stats.uploaded_bytes += image.data.len() as u64;
        let s = size as f32;
        self.atlas_full = false;
        Ok(GlyphImage {
            outline_mask: kind == Kind::Mask && matches!(image.source, Source::Outline),
            page,
            uv: [
                pos[0] as f32 / s,
                pos[1] as f32 / s,
                w as f32 / s,
                h as f32 / s,
            ],
            left: image.placement.left,
            top: image.placement.top,
            width: w,
            height: h,
        })
    }
    fn reclaim_page(&mut self) {
        if self.budget.pages.load(Ordering::Relaxed) < self.options.max_pages {
            return;
        }
        // O(pages) to choose; the cache pass runs once per reclaimed page, not per glyph.
        let victim = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, s)| s.page.unleased())
            .min_by_key(|(_, s)| s.page.last.load(Ordering::Relaxed))
            .map(|(i, _)| i);
        if let Some(index) = victim {
            let page = self.pages.swap_remove(index).page;
            self.cache.retain(
                |_, v| !matches!(&v.source, GlyphSource::Image(i) if Arc::ptr_eq(&i.page, &page)),
            );
        }
    }
}

fn linearize_color(data: &mut [u8], premultiplied: bool) {
    for p in data.as_chunks_mut::<4>().0 {
        let alpha = f32::from(p[3]) / 255.;
        for channel in &mut p[..3] {
            let mut s = f32::from(*channel) / 255.;
            if premultiplied {
                s = if alpha > 0. { (s / alpha).min(1.) } else { 0. };
            }
            let linear = if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            };
            *channel = (linear * alpha * 255.).round() as u8;
        }
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;

/// Steps per physical pixel at which coverage and color glyphs are rasterized.
/// Continuous zoom then reuses each raster across a range of nearby sizes
/// instead of creating a cache entry per frame; quads scale the image by less
/// than 1/8 px to the exact size.
const RASTER_STEPS_PER_PIXEL: f32 = 4.;

fn raster_size(physical: f32, max: f32) -> f32 {
    let rounded = (physical * RASTER_STEPS_PER_PIXEL).round() / RASTER_STEPS_PER_PIXEL;
    if rounded > 0. && rounded <= max {
        rounded
    } else {
        physical
    }
}

// Ties round toward positive infinity, so integer translations preserve phases
// even at negative half-bin boundaries. Euclidean floor handles phase rollover.
fn coverage_phase(position: f32) -> (f32, u8) {
    let quantized = (f64::from(position) * 4. + 0.5).floor() * 0.25;
    let pixel = quantized.floor();
    (pixel as f32, ((quantized - pixel) * 4.) as u8)
}

fn validate_preparation(preparation: TextPreparation) -> Result<(), TextRenderError> {
    if let TextPreparation::Mtsdf(options) = preparation
        && (!(16..=256).contains(&options.pixels_per_em)
            || !options.range_em.is_finite()
            || options.range_em <= 0.
            || options.range_em > 1.
            || options.range_em * (options.pixels_per_em as f32) < 2.)
    {
        return Err(TextRenderError::InvalidOptions);
    }
    if let TextPreparation::Coverage(options) = preparation
        && options
            .physical_origin
            .is_some_and(|p| p.iter().any(|v| !v.is_finite()))
    {
        return Err(TextRenderError::InvalidOptions);
    }
    let scale = preparation.raster_scale();
    if !scale.is_finite() || scale <= 0. {
        return Err(TextRenderError::InvalidOptions);
    }
    Ok(())
}

fn draw_parameters(
    bounds: Option<Rect>,
    draw: TextDraw,
    viewport: [f32; 2],
) -> Result<DrawData, Error> {
    if draw
        .origin
        .iter()
        .chain(&draw.color)
        .chain([&draw.opacity])
        .any(|v| !v.is_finite())
        || !(0. ..=1.).contains(&draw.color[3])
        || !(0. ..=1.).contains(&draw.opacity)
        || !draw.transform.valid()
    {
        return Err(Error::InvalidTextDraw);
    }
    let [a, b, c, d, tx, ty] = draw.transform.0.map(f64::from);
    let [ox, oy] = draw.origin.map(f64::from);
    let [vw, vh] = viewport.map(|v| if v > 0. { f64::from(v) } else { 1. });
    let origin = [(a * ox + c * oy + tx) / vw, (b * ox + d * oy + ty) / vh];
    let p = DrawData {
        origin_axis_x: [
            origin[0] as f32,
            origin[1] as f32,
            (a / vw) as f32,
            (b / vh) as f32,
        ],
        axis_y: [
            (c / vw) as f32,
            (d / vh) as f32,
            if draw.span_colors { 0. } else { 1. },
            0.,
        ],
        color: [
            draw.color[0],
            draw.color[1],
            draw.color[2],
            draw.color[3] * draw.opacity,
        ],
    };
    if bytemuck::cast_slice::<DrawData, f32>(&[p])
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err(Error::InvalidTextDraw);
    }
    if let Some(r) = bounds {
        for x in [r.x, r.x + r.width] {
            for y in [r.y, r.y + r.height] {
                let px = (origin[0] + (a * f64::from(x) + c * f64::from(y)) / vw) as f32;
                let py = (origin[1] + (b * f64::from(x) + d * f64::from(y)) / vh) as f32;
                if !(px * 2.).is_finite() || !(py * 2.).is_finite() {
                    return Err(Error::InvalidTextDraw);
                }
            }
        }
    }
    Ok(p)
}
