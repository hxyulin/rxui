//! Owned CPU work and explicit, bounded atlas commits. No executor or device on workers.
use super::*;
use std::collections::VecDeque;

/// Hard limits on unique generated keys and retained pixel bytes per request.
/// Generator scratch for one glyph and retained layout/font storage are additional.
#[derive(Clone, Copy, Debug)]
pub struct GlyphPreparationLimits {
    /// Counts images, blank entries, fallback classifications and phase variants.
    pub max_glyphs: usize,
    /// Maximum sum of uncompressed image payloads, excluding atlas gutters.
    pub max_image_bytes: usize,
}
impl Default for GlyphPreparationLimits {
    fn default() -> Self {
        Self {
            max_glyphs: 16_384,
            max_image_bytes: 64 * 1024 * 1024,
        }
    }
}
/// Maximum work in one atlas commit call. Zero keys permits no work; zero bytes
/// still permits cache hits, blank entries and fallback classifications.
/// These are work limits, not a wall-clock deadline; page allocation can have driver cost.
#[derive(Clone, Copy, Debug)]
pub struct GlyphUploadBudget {
    /// Maximum keys processed, including cache hits, blanks and fallback classifications.
    pub max_glyphs: usize,
    /// Maximum pixel bytes passed to queue writes; no oversized-first-item exception.
    pub max_bytes: usize,
}
/// Work performed by a commit call, and remaining keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphUploadProgress {
    /// Keys committed or reused from cache this call.
    pub glyphs: usize,
    /// Actual image bytes queued this call (cache hits cost zero).
    pub uploaded_bytes: usize,
    /// Keys still waiting. A too-small byte budget may make no progress.
    pub remaining: usize,
}
/// Device-free owned work. Move to an application worker and call [`Self::generate`].
/// Keeps immutable layout/font sources alive after the text system is dropped.
/// Requests deliberately generate independently of the GPU cache; overlapping jobs
/// deduplicate at commit. Drop the request/result to cancel work you no longer need.
#[derive(Debug)]
pub struct GlyphPreparationRequest {
    layout: Arc<TextLayout>,
    preparation: TextPreparation,
    options: TextRendererOptions,
    epoch: Arc<()>,
    limits: GlyphPreparationLimits,
}
/// CPU-only generated images. Contains no GPU resources, atlas reservations or leases.
#[derive(Debug)]
pub struct GeneratedGlyphs {
    layout: Arc<TextLayout>,
    preparation: TextPreparation,
    epoch: Arc<()>,
    entries: VecDeque<(Key, CpuGlyph)>,
    image_bytes: usize,
}
impl GeneratedGlyphs {
    /// Number of unique generated cache keys, including blank/classification results.
    pub fn glyph_count(&self) -> usize {
        self.entries.len()
    }
    /// Retained image payload bytes; atlas allocation/geometry are additional.
    pub fn image_bytes(&self) -> usize {
        self.image_bytes
    }
}
/// Renderer-bound incremental upload. Committed images hold atlas leases even if
/// the lookup cache evicts them. Drop to cancel and release those leases.
#[derive(Debug)]
pub struct TextUpload {
    generated: GeneratedGlyphs,
    resolved: HashMap<Key, GlyphSource>,
}
impl TextUpload {
    /// Uncommitted keys. Finalization requires zero.
    pub fn remaining(&self) -> usize {
        self.generated.entries.len()
    }
    /// Conservative byte budget needed for the next entry; cache reuse can cost less.
    /// `None` means complete, `Some(0)` a blank or fallback classification.
    pub fn next_image_bytes(&self) -> Option<usize> {
        self.generated
            .entries
            .front()
            .map(|(_, glyph)| glyph.bytes())
    }
}
pub(super) enum CpuGlyph {
    Image(Image, Kind),
    Blank,
    RasterFallback,
}
impl std::fmt::Debug for CpuGlyph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image(image, kind) => f
                .debug_struct("Image")
                .field("kind", kind)
                .field("bytes", &image.data.len())
                .finish_non_exhaustive(),
            Self::Blank => f.write_str("Blank"),
            Self::RasterFallback => f.write_str("RasterFallback"),
        }
    }
}
impl CpuGlyph {
    fn bytes(&self) -> usize {
        match self {
            Self::Image(i, _) => i.data.len(),
            _ => 0,
        }
    }
    fn outline_mask(&self) -> bool {
        matches!(self, Self::Image(i, Kind::Mask) if matches!(i.source, Source::Outline))
    }
}
pub(super) struct GlyphGenerator {
    scale: ScaleContext,
    fonts: HashMap<FontId, swash::CacheKey>,
    page_size: u32,
}
impl GlyphGenerator {
    pub(super) fn new(page_size: u32) -> Self {
        Self {
            scale: ScaleContext::new(),
            fonts: HashMap::new(),
            page_size,
        }
    }
    pub(super) fn generate(
        &mut self,
        key: Key,
        font: &TextFont,
    ) -> Result<CpuGlyph, TextRenderError> {
        profiling::scope!("rxui_draw::rasterize_glyph");
        let mut font_ref = swash::FontRef::from_index(font.data(), font.face_index() as usize)
            .ok_or(TextRenderError::InvalidRaster)?;
        if !self.fonts.contains_key(&font.id()) && self.fonts.len() >= 64 {
            self.fonts.clear();
        }
        font_ref.key = *self.fonts.entry(font.id()).or_default();
        let weight = swash::Tag::from_be_bytes(*b"wght");
        let coords = font_ref
            .variations()
            .normalized_coords([(weight, f32::from(key.weight))]);
        let (size, hint) = match key.representation {
            Representation::Coverage { size, hint, .. } => (f32::from_bits(size), hint),
            Representation::Mtsdf { pixels_per_em, .. } => (pixels_per_em as f32, false),
        };
        let mut scaler = self
            .scale
            .builder(font_ref)
            .size(size)
            .hint(hint)
            .normalized_coords(coords)
            .build();
        let glyph = key.glyph;
        let slant = key.italic.then(|| {
            swash::zeno::Transform::skew(
                swash::zeno::Angle::from_degrees(14.),
                swash::zeno::Angle::from_degrees(0.),
            )
        });
        Ok(match key.representation {
            Representation::Mtsdf {
                pixels_per_em,
                range_em,
            } => {
                // Probe artwork before monochrome outlines: some color glyphs have both.
                // Classification is size independent. Raster fallback is generated separately
                // at the requested physical size, preserving the existing source priority.
                if scaler.scale_color_outline(glyph).is_some()
                    || scaler
                        .scale_color_bitmap(glyph, StrikeWith::BestFit)
                        .is_some()
                {
                    CpuGlyph::RasterFallback
                } else if let Some(mut outline) = scaler.scale_outline(glyph) {
                    if let Some(t) = &slant {
                        outline.transform(t);
                    }
                    match crate::text::distance_field::generate(
                        &outline,
                        pixels_per_em,
                        f32::from_bits(range_em),
                        self.page_size,
                    )? {
                        Some(image) => CpuGlyph::Image(image, Kind::Mtsdf),
                        None => CpuGlyph::Blank,
                    }
                } else if scaler.scale_bitmap(glyph, StrikeWith::BestFit).is_some() {
                    CpuGlyph::RasterFallback
                } else {
                    CpuGlyph::Blank
                }
            }
            Representation::Coverage { phase, .. } => {
                // Check outline dimensions before Swash allocates its output image.
                // Bitmap dimensions are validated after decoding; Swash owns decode scratch.
                if let Some(mut outline) = scaler
                    .scale_color_outline(glyph)
                    .or_else(|| scaler.scale_outline(glyph))
                {
                    if let Some(t) = &slant {
                        outline.transform(t);
                    }
                    let bounds = outline.bounds();
                    if [bounds.width(), bounds.height()]
                        .iter()
                        .any(|v| !v.is_finite() || v.ceil() + 4. > self.page_size as f32)
                    {
                        return Err(TextRenderError::GlyphTooLarge);
                    }
                }
                let mut render = Render::new(&[
                    Source::ColorOutline(0),
                    Source::ColorBitmap(StrikeWith::BestFit),
                    Source::Outline,
                    Source::Bitmap(StrikeWith::BestFit),
                ]);
                render.format(swash::zeno::Format::Alpha);
                render.transform(slant);
                // Nonzero phases are requested only after the zero-phase probe
                // classified a monochrome outline. Intrinsic artwork keeps phase zero.
                if phase != [0; 2] {
                    render.offset(swash::zeno::Vector::new(
                        f32::from(phase[0]) * 0.25,
                        f32::from(phase[1]) * 0.25,
                    ));
                }
                match render.render(&mut scaler, glyph) {
                    Some(image) if image.placement.width > 0 && image.placement.height > 0 => {
                        cpu_image(image)?
                    }
                    _ => CpuGlyph::Blank,
                }
            }
        })
    }
}
fn cpu_image(mut image: Image) -> Result<CpuGlyph, TextRenderError> {
    let kind = match image.content {
        Content::Mask => Kind::Mask,
        Content::Color => Kind::Color,
        Content::SubpixelMask => return Err(TextRenderError::InvalidRaster),
    };
    if kind == Kind::Color {
        linearize_color(
            &mut image.data,
            matches!(image.source, Source::ColorOutline(_)),
        );
    }
    Ok(CpuGlyph::Image(image, kind))
}
impl GlyphPreparationRequest {
    /// Generates all unique required images on the calling thread, without GPU work.
    /// Includes size-dependent artwork fallback and exact coverage phase keys.
    /// Failure discards partial CPU results and leaves the renderer untouched.
    pub fn generate(self) -> Result<GeneratedGlyphs, TextRenderError> {
        let mut generator = GlyphGenerator::new(self.options.page_size);
        let mut entries = VecDeque::new();
        let mut indexes = HashMap::new();
        let mut bytes = 0usize;
        let mut ensure = |key: Key, font: &TextFont| -> Result<(bool, bool), TextRenderError> {
            if let Some(&index) = indexes.get(&key) {
                let (_, value): &(Key, CpuGlyph) = &entries[index];
                return Ok((
                    value.outline_mask(),
                    matches!(value, CpuGlyph::RasterFallback),
                ));
            }
            if entries.len() >= self.limits.max_glyphs {
                return Err(TextRenderError::PreparationLimit);
            }
            let value = generator.generate(key, font)?;
            bytes = bytes
                .checked_add(value.bytes())
                .ok_or(TextRenderError::PreparationLimit)?;
            if bytes > self.limits.max_image_bytes {
                return Err(TextRenderError::PreparationLimit);
            }
            // Validate decoded bitmap payloads too, before they cross the worker boundary.
            if let CpuGlyph::Image(image, kind) = &value {
                let (w, h) = (image.placement.width, image.placement.height);
                if w.checked_add(2).is_none_or(|v| v > self.options.page_size)
                    || h.checked_add(2).is_none_or(|v| v > self.options.page_size)
                {
                    return Err(TextRenderError::GlyphTooLarge);
                }
                if image.data.len() != w as usize * h as usize * kind.bytes() {
                    return Err(TextRenderError::InvalidRaster);
                }
            }
            let classification = (
                value.outline_mask(),
                matches!(value, CpuGlyph::RasterFallback),
            );
            indexes.insert(key, entries.len());
            entries.push_back((key, value));
            Ok(classification)
        };
        for glyph in self.layout.glyphs() {
            let font = &self.layout.fonts()[glyph.font_index];
            let physical_size = glyph.font_size * self.preparation.raster_scale();
            let size = raster_size(physical_size, self.options.max_raster_size);
            let coverage = Representation::Coverage {
                size: size.to_bits(),
                hint: match self.preparation {
                    TextPreparation::Coverage(v) => v.hinting,
                    _ => true,
                },
                phase: [0; 2],
            };
            let key = Key {
                font: font.id(),
                glyph: glyph.glyph_id,
                weight: font.weight(),
                italic: glyph.synthetic_italic,
                representation: match self.preparation {
                    TextPreparation::Coverage(_) => coverage,
                    TextPreparation::Mtsdf(v) => Representation::Mtsdf {
                        pixels_per_em: v.pixels_per_em,
                        range_em: v.range_em.to_bits(),
                    },
                },
            };
            let (outline, fallback) = ensure(key, font)?;
            if fallback {
                if physical_size > self.options.max_raster_size {
                    return Err(TextRenderError::InvalidOptions);
                }
                ensure(
                    Key {
                        representation: coverage,
                        ..key
                    },
                    font,
                )?;
            }
            // Intrinsic artwork retains phase zero; monochrome outlines get the exact phase key.
            if let TextPreparation::Coverage(raster) = self.preparation
                && outline
                && let Some(origin) = raster.physical_origin
            {
                let physical = std::array::from_fn::<_, 2, _>(|i| {
                    glyph.position[i] * raster.raster_scale + origin[i].rem_euclid(1.)
                });
                if physical.iter().any(|v| !v.is_finite()) {
                    return Err(TextRenderError::InvalidOptions);
                }
                let phase = physical.map(|p| coverage_phase(p).1);
                ensure(
                    Key {
                        representation: Representation::Coverage {
                            size: physical_size.to_bits(),
                            hint: raster.hinting,
                            phase,
                        },
                        ..key
                    },
                    font,
                )?;
            }
        }
        Ok(GeneratedGlyphs {
            layout: self.layout,
            preparation: self.preparation,
            epoch: self.epoch,
            entries,
            image_bytes: bytes,
        })
    }
}
impl TextRenderer {
    /// Captures an immutable layout for application-controlled CPU generation.
    /// No rasterization, atlas allocation, executor, submission or waiting occurs.
    /// `clear_cache` invalidates outstanding requests/uploads; ordinary eviction does not.
    ///
    /// ```no_run
    /// # use rxui_draw::{TextRenderer, TextLayout, MtsdfOptions, GlyphPreparationLimits,
    /// #     GlyphUploadBudget, TextRenderError};
    /// # use std::sync::Arc;
    /// # fn prewarm(renderer: &mut TextRenderer, layout: Arc<TextLayout>)
    /// #     -> Result<(), TextRenderError> {
    /// let request = renderer.request_glyphs(layout, MtsdfOptions::new(),
    ///     GlyphPreparationLimits::default())?;
    /// let worker = std::thread::spawn(move || request.generate());
    /// // Keep drawing old PreparedText resources. In an event loop, check
    /// // worker.is_finished() before joining; scheduling stays with the application.
    /// let generated = worker.join().expect("worker panicked")?;
    /// let mut upload = renderer.begin_glyph_upload(generated)?;
    /// // One call per application-selected work opportunity:
    /// let progress = renderer.commit_glyphs(&mut upload,
    ///     GlyphUploadBudget { max_glyphs: 4, max_bytes: 64 * 1024 })?;
    /// if progress.remaining == 0 {
    ///     let text = renderer.finish_glyph_upload(&upload, 1024 * 1024)?;
    ///     // Store text for later draws; drop upload to release its extra leases.
    /// }
    /// # Ok(()) }
    /// ```
    pub fn request_glyphs(
        &self,
        layout: Arc<TextLayout>,
        preparation: impl Into<TextPreparation>,
        limits: GlyphPreparationLimits,
    ) -> Result<GlyphPreparationRequest, TextRenderError> {
        let preparation = preparation.into();
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
        Ok(GlyphPreparationRequest {
            layout,
            preparation,
            limits,
            options: self.options.clone(),
            epoch: self.preparation_epoch.clone(),
        })
    }
    /// Binds CPU results to this renderer. Does no uploads. Foreign/stale results fail.
    pub fn begin_glyph_upload(
        &self,
        generated: GeneratedGlyphs,
    ) -> Result<TextUpload, TextRenderError> {
        self.check_preparation(&generated)?;
        Ok(TextUpload {
            generated,
            resolved: HashMap::new(),
        })
    }
    fn check_preparation(&self, generated: &GeneratedGlyphs) -> Result<(), TextRenderError> {
        if !Arc::ptr_eq(&self.preparation_epoch, &generated.epoch) {
            return Err(TextRenderError::StalePreparation);
        }
        Ok(())
    }
    /// Inserts at most the supplied keys/bytes, with no generation or waiting.
    /// Cache hits reuse existing leases. A failed entry stays pending for retry;
    /// earlier commits remain leased and valid. `AtlasFull` may require dropping
    /// other texts/uploads, polling completed frames, trimming or raising max_pages.
    pub fn commit_glyphs(
        &mut self,
        upload: &mut TextUpload,
        budget: GlyphUploadBudget,
    ) -> Result<GlyphUploadProgress, TextRenderError> {
        self.check_preparation(&upload.generated)?;
        let mut progress = GlyphUploadProgress::default();
        while progress.glyphs < budget.max_glyphs {
            let Some((key, generated)) = upload.generated.entries.front() else {
                break;
            };
            let cached = self.cache.get(key).map(|v| v.source.clone());
            let bytes = if cached.is_some() {
                0
            } else {
                generated.bytes()
            };
            if bytes > budget.max_bytes - progress.uploaded_bytes {
                break;
            }
            self.clock += 1;
            let source = if let Some(source) = cached {
                self.stats.cache_hits += 1;
                let cached = self.cache.get_mut(key).unwrap();
                cached.last = self.clock;
                if let GlyphSource::Image(image) = &source {
                    image.page.last.store(self.clock, Ordering::Relaxed);
                }
                source
            } else {
                let source = self.upload_generated(generated)?;
                self.stats.cache_misses += 1;
                if self.cache.len() >= self.options.max_cached_glyphs {
                    self.evict_glyphs();
                }
                self.cache
                    .insert(*key, Cached::new(source.clone(), self.clock));
                source
            };
            upload.resolved.insert(*key, source);
            upload.generated.entries.pop_front();
            progress.glyphs += 1;
            progress.uploaded_bytes += bytes;
        }
        progress.remaining = upload.remaining();
        Ok(progress)
    }
    /// Builds immutable geometry from completed uploads, with an explicit geometry
    /// byte limit (48 bytes per glyph/decorative quad upper bound). No glyph generation
    /// or atlas uploads occur. Keep the returned text, then drop the upload's leases.
    /// Limit/incomplete errors leave the upload reusable. Decoration storage uses
    /// a shared 1x1 solid texture, initialized once (four extra bytes).
    pub fn finish_glyph_upload(
        &mut self,
        upload: &TextUpload,
        max_geometry_bytes: u64,
    ) -> Result<PreparedText, TextRenderError> {
        self.check_preparation(&upload.generated)?;
        if upload.remaining() != 0 {
            return Err(TextRenderError::PreparationIncomplete);
        }
        let layout = &upload.generated.layout;
        let bytes = (layout.glyphs().len() as u64 + layout.decorations().len() as u64)
            .checked_mul(48)
            .ok_or(TextRenderError::PreparationLimit)?;
        if bytes > max_geometry_bytes {
            return Err(TextRenderError::PreparationLimit);
        }
        let draft =
            self.prepare_geometry(layout, upload.generated.preparation, Some(&upload.resolved))?;
        let bytes = draft.glyphs.len() as u64 * 48;
        let geometry = (!draft.glyphs.is_empty())
            .then(|| GeometryBuffer::Dedicated(self.upload_geometry(&draft.glyphs)));
        Ok(self.finish_text(draft, geometry, 0..bytes, false))
    }
}
