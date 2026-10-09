use crate::{
    Bounds, ElementId, TextMeasure, TextRequest, TextWidth, Ui, UiError, View, id::IdMap,
    ui::VisibleContent,
};
use astrelis::{
    CornerRadii, FontFamily, FontSlant, GraphicsContext, Painter, PreparedText, Rect, RenderFormat,
    RenderPass, ShapeDraw, Stroke, TextAlign, TextBuffer, TextDraw, TextLayout, TextRasterOptions,
    TextStyle, TextSystem, TextWrap, Transform2D,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

impl From<astrelis::Error> for UiError {
    /// Text failures that astrelis wrapped into its graphics error keep their text variant,
    /// so one failure maps to one `UiError` variant however it reached rxui.
    fn from(error: astrelis::Error) -> Self {
        match error {
            astrelis::Error::Text(error) => Self::Text(error),
            astrelis::Error::TextRender(error) => Self::TextRender(*error),
            other => Self::Graphics(other),
        }
    }
}
impl From<astrelis::TextError> for UiError {
    fn from(error: astrelis::TextError) -> Self {
        Self::Text(error)
    }
}
impl From<astrelis::TextRenderError> for UiError {
    fn from(error: astrelis::TextRenderError) -> Self {
        Self::TextRender(error)
    }
}

#[derive(Default)]
struct TextResource {
    buffer: TextBuffer,
    prepared: Option<PreparedText>,
    prepared_layout: Option<Arc<TextLayout>>,
    raster_scale: f32,
    prepared_width: f32,
    prepared_revision: u64,
    font_generation: u64,
    /// Long wrapped text in separately prepared paragraph chunks; when present, the
    /// buffer and prepared fields above are unused.
    chunks: Option<Box<chunks::Chunks>>,
    /// Text revision the chunking decision was made for.
    chunked_revision: Option<u64>,
}
/// Families resolved to other loaded families before shaping; see
/// [`UiPainter::map_family`] and [`UiPainter::set_sans_serif_family`].
#[derive(Default)]
struct FamilyMap {
    mapped: HashMap<crate::FontFamily, Arc<str>>,
    /// Family used for an unmapped generic sans-serif default, such as the system UI font.
    sans_serif: Option<Arc<str>>,
}
impl FamilyMap {
    fn new() -> Self {
        Self::default()
    }
    /// Loaded family that `family` shapes with; explicit mappings win over the
    /// sans-serif substitute.
    fn get(&self, family: &crate::FontFamily) -> Option<&Arc<str>> {
        self.mapped.get(family).or(match family {
            crate::FontFamily::SansSerif => self.sans_serif.as_ref(),
            _ => None,
        })
    }
}
impl TextResource {
    /// Applies text, style and wrapping; the configured width is left unchanged.
    fn configure(&mut self, request: TextRequest<'_>, families: &FamilyMap) -> Result<(), UiError> {
        let style = request.style;
        let family = match families.get(&style.family) {
            Some(name) => FontFamily::Named(name.to_string()),
            None => match &style.family {
                crate::FontFamily::Named(name) => FontFamily::Named(name.to_string()),
                crate::FontFamily::SansSerif => FontFamily::SansSerif,
                crate::FontFamily::Serif => FontFamily::Serif,
                crate::FontFamily::Monospace => FontFamily::Monospace,
            },
        };
        self.buffer.set_text(
            request.text,
            TextStyle::new()
                .family(family)
                .font_size(request.font_size)
                .line_height(request.font_size * style.line_height)
                .weight(style.weight.0)
                .slant(match style.style {
                    crate::FontStyle::Normal => FontSlant::Normal,
                    crate::FontStyle::Italic => FontSlant::Italic,
                    crate::FontStyle::Oblique => FontSlant::Oblique,
                }),
        )?;
        // Editors scroll horizontally from the start edge, so they ignore alignment.
        self.buffer.set_align(match style.align {
            _ if request.single_line => TextAlign::Start,
            crate::TextAlign::Start => TextAlign::Start,
            crate::TextAlign::End => TextAlign::End,
            crate::TextAlign::Left => TextAlign::Left,
            crate::TextAlign::Right => TextAlign::Right,
            crate::TextAlign::Center => TextAlign::Center,
            crate::TextAlign::Justified => TextAlign::Justified,
        });
        self.buffer.set_wrap(if request.single_line {
            TextWrap::None
        } else {
            TextWrap::Word
        });
        Ok(())
    }
    fn layout(
        &mut self,
        fonts: &mut TextSystem,
        families: &FamilyMap,
        request: TextRequest<'_>,
    ) -> Result<Arc<TextLayout>, UiError> {
        self.configure(request, families)?;
        self.buffer.set_width(text_width(request.width))?;
        Ok(self.buffer.layout(fonts)?)
    }
    /// Whether this text uses paragraph chunks, deciding again when its revision
    /// changes. Single-line text never does.
    fn chunked(&mut self, request: &TextRequest<'_>) -> bool {
        if request.single_line {
            self.chunks = None;
            return false;
        }
        if self.chunked_revision != Some(request.revision) {
            self.chunked_revision = Some(request.revision);
            let wanted = chunks::wanted(request.text, self.chunks.is_some());
            if wanted && self.chunks.is_none() {
                self.chunks = Some(Box::default());
                self.buffer = TextBuffer::new();
                self.prepared = None;
                self.prepared_layout = None;
            } else if !wanted {
                self.chunks = None;
            }
        }
        self.chunks.is_some()
    }
    /// Prepared glyph ink in text-local units.
    fn ink_bounds(&self) -> Option<Rect> {
        match &self.prepared {
            Some(prepared) => prepared.ink_bounds(),
            None => self.chunks.as_ref()?.ink,
        }
    }
    /// Whether drawable geometry exists for the current layout.
    fn ready(&self) -> bool {
        self.prepared.is_some() || self.chunks.as_ref().is_some_and(|c| c.prepared())
    }
    /// Selection rectangles with the index of the drawable part each lies in.
    fn selection_rects(
        &self,
        range: std::ops::Range<usize>,
    ) -> Result<Vec<(Rect, usize)>, UiError> {
        match &self.chunks {
            Some(chunks) => chunks.selection_rects(range),
            None => Ok(self
                .prepared_layout
                .as_ref()
                .ok_or(UiError::InvalidGeometry)?
                .selection_rects(range)?
                .map(|r| (r, 0))
                .collect()),
        }
    }
    fn caret(&self, position: astrelis::TextPosition) -> Option<astrelis::TextCaret> {
        match &self.chunks {
            Some(chunks) => chunks.caret(position),
            None => self.prepared_layout.as_ref()?.caret(position),
        }
    }
    /// One drawable part and its vertical offset: a chunk, or the whole text.
    fn part(&self, index: usize) -> Option<(&PreparedText, f32)> {
        match &self.chunks {
            Some(chunks) => chunks.chunk(index),
            None => self.prepared.as_ref().map(|p| (p, 0.)),
        }
    }
    /// Size at the requested width. Probe widths reuse the configured snapshot's
    /// shaped runs without building glyph snapshots or changing its width.
    fn measure(
        &mut self,
        fonts: &mut TextSystem,
        families: &FamilyMap,
        request: TextRequest<'_>,
    ) -> Result<[f32; 2], UiError> {
        self.configure(request, families)?;
        Ok(self.buffer.measure(fonts, text_width(request.width))?)
    }
    /// Size of whole or chunked text at the requested width.
    fn measure_text(
        &mut self,
        fonts: &mut TextSystem,
        families: &FamilyMap,
        request: TextRequest<'_>,
        generation: u64,
    ) -> Result<[f32; 2], UiError> {
        if self.chunked(&request) {
            return self
                .chunks
                .as_mut()
                .unwrap()
                .measure(fonts, families, request, generation);
        }
        self.measure(fonts, families, request)
    }
}
/// Prepared placeholder of an empty input, retained separately from its value text.
#[derive(Default)]
struct PlaceholderResource {
    text: String,
    resource: TextResource,
}
impl PlaceholderResource {
    fn current(&self, request: &TextRequest<'_>, generation: u64, raster_scale: f32) -> bool {
        self.resource.prepared.is_some()
            && self.text == request.text
            && self.resource.prepared_revision == request.revision
            && Some(self.resource.prepared_width) == text_width(request.width)
            && self.resource.font_generation == generation
            && self.resource.raster_scale == raster_scale
    }
}
fn text_width(width: TextWidth) -> Option<f32> {
    match width {
        TextWidth::MinContent => Some(0.),
        TextWidth::MaxContent => None,
        TextWidth::Available(width) => Some(width),
    }
}

/// Cumulative image preparation work. Placements share uploaded pixels and bindings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageStats {
    /// CPU images uploaded to this painter's device.
    pub uploads: u64,
    /// Uploaded CPU RGBA byte count.
    pub uploaded_bytes: u64,
    /// Created source/sampler bindings, including storage replacement.
    pub bindings: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ImageKey {
    id: crate::ImageId,
    filter: crate::ImageFilter,
    alpha: crate::ImageAlpha,
}
struct ImageResource {
    texture: Option<astrelis::Texture>,
    view: Option<astrelis::wgpu::TextureView>,
}
fn image_key(image: &crate::ImageInfo<'_>) -> ImageKey {
    ImageKey {
        id: image.source.id(),
        filter: image.filter,
        alpha: image.alpha,
    }
}
/// Explicit Astrelis text measurement and painting adapter for a retained Ui.
/// It owns fonts and caches, but no window or application lifecycle. Call Ui::prepare
/// using this as TextMeasure, then prepare GPU resources before frame acquisition.
/// Painting uses caller-selected passes/clipping. No font discovery is implicit:
/// register application fonts with [`Self::register_font`] or discover installed
/// ones through [`Self::fonts_mut`]. The repository's `docs/fonts.md` describes
/// family matching and fallback.
pub struct UiPainter {
    fonts: TextSystem,
    families: FamilyMap,
    generation: u64,
    painter: Painter,
    texts: IdMap<ElementId, TextResource>,
    graphics: GraphicsContext,
    images: HashMap<crate::ImageId, ImageResource>,
    image_bindings: HashMap<ImageKey, astrelis::TextureBinding>,
    image_placements: IdMap<ElementId, ImageKey>,
    image_stats: ImageStats,
    layers: IdMap<ElementId, composition::LayerResource>,
    compositions: HashMap<u64, Arc<composition::CompositionPlan>>,
    layer_stats: LayerStats,
    customs: IdMap<ElementId, Box<dyn std::any::Any>>,
    placeholders: IdMap<ElementId, PlaceholderResource>,
    /// Last complete text pass per placement, by tree.
    passes: HashMap<u64, TextPass>,
}
/// What a text pass over one placement saw. While the placement's composition key
/// and the raster scale are unchanged, its visible text is unchanged too, and the
/// pass is skipped; image and custom elements are still prepared every time.
struct TextPass {
    key: [u64; 5],
    raster_scale: f32,
    images: Vec<ElementId>,
    customs: Vec<ElementId>,
}
impl UiPainter {
    /// Creates empty fonts and renderer caches for this graphics device.
    pub fn new(graphics: &GraphicsContext) -> Self {
        Self {
            fonts: TextSystem::new(),
            families: FamilyMap::new(),
            generation: 0,
            painter: Painter::new(graphics),
            texts: IdMap::default(),
            graphics: graphics.clone(),
            images: HashMap::new(),
            image_bindings: HashMap::new(),
            image_placements: IdMap::default(),
            image_stats: ImageStats::default(),
            layers: IdMap::default(),
            compositions: HashMap::new(),
            layer_stats: LayerStats::default(),
            customs: IdMap::default(),
            placeholders: IdMap::default(),
            passes: HashMap::new(),
        }
    }
    /// Cumulative image upload/binding counters.
    pub fn image_stats(&self) -> ImageStats {
        self.image_stats
    }
    /// Font loading/configuration access. Each call advances measurement generation,
    /// invalidating text measurements on the next prepare. Load/discover once at startup.
    pub fn fonts_mut(&mut self) -> &mut TextSystem {
        self.invalidate_fonts();
        &mut self.fonts
    }
    fn invalidate_fonts(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("RXUI font generation exhausted");
    }
    /// Loads TrueType/OpenType data, including collections, retaining the bytes
    /// without copying them. Returns the faces it added; select one with
    /// `FontFamily::Named` and a family name it reports, or make it a generic
    /// family's choice with [`Self::map_family`]. Invalid data returns
    /// `UiError::Text` and changes nothing. Success re-measures every placement
    /// using this painter on its next prepare. Registering is not expected per frame.
    pub fn register_font(
        &mut self,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<Vec<crate::FontFace>, UiError> {
        let faces = self.fonts.load_font_shared(bytes.into())?;
        self.invalidate_fonts();
        Ok(faces.into_iter().map(font_face).collect())
    }
    /// Resolves `family` to the loaded family `target` before shaping: map
    /// `FontFamily::SansSerif` to choose the default UI face, or a named alias
    /// such as "Brand" to a concrete family. Mapping is one step, not recursive.
    /// Missing glyphs still fall back to other loaded faces. A changed mapping
    /// re-measures text on the next prepare.
    pub fn map_family(
        &mut self,
        family: impl Into<crate::FontFamily>,
        target: impl Into<Arc<str>>,
    ) {
        let target = target.into();
        let family = family.into();
        if self.families.mapped.get(&family) != Some(&target) {
            self.families.mapped.insert(family, target);
            self.invalidate_fonts();
        }
    }
    /// Removes a mapping from [`Self::map_family`], restoring the family's own
    /// matching. Returns whether a mapping existed.
    pub fn unmap_family(&mut self, family: &crate::FontFamily) -> bool {
        let removed = self.families.mapped.remove(family).is_some();
        if removed {
            self.invalidate_fonts();
        }
        removed
    }
    /// Loaded family `family` currently resolves to, if mapped.
    pub fn family_mapping(&self, family: &crate::FontFamily) -> Option<&str> {
        self.families.mapped.get(family).map(|name| &**name)
    }
    /// Read-only font information.
    pub fn fonts(&self) -> &TextSystem {
        &self.fonts
    }
    /// Shapes text whose family is the generic sans-serif default with this loaded
    /// family instead; None restores the generic family. The native host sets the
    /// platform interface font here. Advances the measurement generation on change.
    pub fn set_sans_serif_family(&mut self, family: Option<Arc<str>>) {
        if self.families.sans_serif != family {
            self.families.sans_serif = family;
            self.fonts_mut();
        }
    }
    /// Family substituted for the generic sans-serif default, if any.
    pub fn sans_serif_family(&self) -> Option<&str> {
        self.families.sans_serif.as_deref()
    }
    /// Astrelis renderer access for cache statistics and explicit customization.
    pub fn painter(&mut self) -> &mut Painter {
        &mut self.painter
    }
    /// Prepares pipelines and changed text resources for this layout/device/DPI.
    /// Placement, color, hover and focus changes retain existing prepared text.
    /// A changed layout or raster density prepares a replacement explicitly.
    /// Also prepares cropped opacity layers for this destination format/scale.
    /// Layer storage is reused; contents are recorded by [`Self::compose`].
    pub fn prepare<T: View>(
        &mut self,
        ui: &Ui<T>,
        format: &RenderFormat,
        raster_scale: f32,
    ) -> Result<(), UiError> {
        profiling::scope!("rxui::UiPainter::prepare");
        if !ui.is_prepared()
            || ui.measurement_generation() != self.generation
            || !raster_scale.is_finite()
            || raster_scale <= 0.
        {
            return Err(UiError::InvalidGeometry);
        }
        self.painter.prepare(format)?;
        let key = ui.composition_key();
        let pass = match self
            .passes
            .remove(&ui.tree_id())
            .filter(|p| p.key == key && p.raster_scale == raster_scale)
        {
            Some(pass) => pass,
            None => self.prepare_texts(ui, raster_scale)?,
        };
        self.prepare_images(ui, format, &pass.images)?;
        self.prepare_customs(ui, format, raster_scale, &pass.customs)?;
        self.prepare_composition(ui, format, raster_scale)?;
        self.passes.insert(ui.tree_id(), pass);
        Ok(())
    }
    fn prepare_texts<T: View>(
        &mut self,
        ui: &Ui<T>,
        raster_scale: f32,
    ) -> Result<TextPass, UiError> {
        self.texts
            .retain(|id, _| !ui.owns_element(*id) || ui.contains_element(*id));
        self.placeholders
            .retain(|id, _| !ui.owns_element(*id) || ui.placeholder_request(*id).is_some());
        // One pass over visible nodes, without building ElementInfo for each:
        // images and custom elements are rare and prepared from their full info.
        let mut images = Vec::new();
        let mut customs = Vec::new();
        for &id in ui.painting_ids() {
            let request = match ui.visible_content(id) {
                None => continue,
                Some(VisibleContent::Text(request)) => request,
                Some(content) => {
                    match content {
                        VisibleContent::Image => images.push(id),
                        VisibleContent::Custom => customs.push(id),
                        _ => {}
                    }
                    self.texts.remove(&id);
                    continue;
                }
            };
            let width = text_width(request.width).unwrap_or_default();
            // Only an empty input shows a placeholder.
            if request.text.is_empty()
                && let Some(placeholder) = ui.placeholder_request(id)
            {
                let entry = self.placeholders.entry(id).or_default();
                if !entry.current(&placeholder, self.generation, raster_scale) {
                    let layout =
                        entry
                            .resource
                            .layout(&mut self.fonts, &self.families, placeholder)?;
                    entry.resource.prepared = None;
                    entry.resource.prepared = Some(self.painter.prepare_text(
                        &layout,
                        TextRasterOptions::new().raster_scale(raster_scale),
                    )?);
                    entry.resource.prepared_layout = Some(layout);
                    entry.resource.raster_scale = raster_scale;
                    entry.resource.prepared_width = width;
                    entry.resource.prepared_revision = placeholder.revision;
                    entry.resource.font_generation = self.generation;
                    entry.text.clear();
                    entry.text.push_str(placeholder.text);
                }
            }
            let resource = self.texts.entry(id).or_default();
            let current = resource.prepared_revision == request.revision
                && resource.prepared_width == width
                && resource.font_generation == self.generation
                && resource.raster_scale == raster_scale;
            // The common unchanged case reads only these fields; chunked text never
            // holds whole-text geometry.
            if current && resource.prepared.is_some() {
                continue;
            }
            if resource.chunked(&request) {
                if current && resource.ready() {
                    continue;
                }
                let chunks = resource.chunks.as_mut().unwrap();
                chunks.layout(&mut self.fonts, &self.families, request, self.generation)?;
                chunks.prepare(&mut self.painter, raster_scale)?;
                resource.prepared_revision = request.revision;
                resource.prepared_width = width;
                resource.font_generation = self.generation;
                resource.raster_scale = raster_scale;
                continue;
            }
            let layout = resource.layout(&mut self.fonts, &self.families, request)?;
            if request.single_line {
                layout.prepare_interaction()?;
            }
            if resource
                .prepared_layout
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, &layout))
                || resource.raster_scale != raster_scale
                || resource.prepared.is_none()
            {
                resource.prepared = None;
                resource.prepared_layout = None;
                let prepared = self
                    .painter
                    .prepare_text(&layout, TextRasterOptions::new().raster_scale(raster_scale))?;
                resource.prepared = Some(prepared);
                resource.prepared_layout = Some(layout);
                resource.raster_scale = raster_scale;
                resource.prepared_width = width;
            }
            resource.prepared_revision = request.revision;
            resource.font_generation = self.generation;
        }
        Ok(TextPass {
            key: ui.composition_key(),
            raster_scale,
            images,
            customs,
        })
    }
    fn prepare_customs<T: View>(
        &mut self,
        ui: &Ui<T>,
        format: &RenderFormat,
        raster_scale: f32,
        ids: &[ElementId],
    ) -> Result<(), UiError> {
        self.customs
            .retain(|id, _| !ui.owns_element(*id) || ui.custom_element(*id).is_some());
        for element in ids.iter().filter_map(|id| ui.element(*id)) {
            let Some(custom) = ui.custom_element(element.id) else {
                continue;
            };
            let state = self
                .customs
                .entry(element.id)
                .or_insert_with(|| custom.new_state());
            // A replacement element of another type at the same identity starts fresh.
            if (**state).type_id() != custom.state_type() {
                *state = custom.new_state();
            }
            custom.prepare(
                state.as_mut(),
                &element,
                &mut crate::CustomPrepare {
                    painter: &mut self.painter,
                    fonts: &mut self.fonts,
                    format,
                    raster_scale,
                },
            )?;
        }
        Ok(())
    }
    fn prepare_images<T: View>(
        &mut self,
        ui: &Ui<T>,
        format: &RenderFormat,
        ids: &[ElementId],
    ) -> Result<(), UiError> {
        self.image_placements
            .retain(|id, _| !ui.owns_element(*id) || ui.contains_element(*id));
        let mut seen = HashSet::new();
        for element in ids.iter().filter_map(|id| ui.element(*id)) {
            let Some(image) = element.image else {
                continue;
            };
            let key = image_key(&image);
            self.image_placements.insert(element.id, key);
            if !seen.insert(key) {
                continue;
            }
            let resource = self.images.entry(key.id).or_insert(ImageResource {
                texture: None,
                view: None,
            });
            let current = match image.source.source() {
                crate::image::Source::Rgba { size, bytes } => {
                    if resource.texture.is_none() {
                        let texture = self
                            .graphics
                            .create_texture(astrelis::TextureOptions::new(size[0], size[1]))?;
                        texture.write(bytes)?;
                        self.image_stats.uploads += 1;
                        self.image_stats.uploaded_bytes += bytes.len() as u64;
                        resource.texture = Some(texture);
                    }
                    Some(resource.texture.as_ref().unwrap().view().clone())
                }
                crate::image::Source::Texture(texture) => Some(texture.view().clone()),
                crate::image::Source::View { view, .. } => Some(view.clone()),
                crate::image::Source::Framebuffer(source) => match source.view() {
                    Ok(view) => Some(view),
                    Err(astrelis::Error::TargetSuspended) => None,
                    Err(e) => return Err(e.into()),
                },
            };
            if resource.view != current {
                for filter in [crate::ImageFilter::Linear, crate::ImageFilter::Nearest] {
                    for alpha in [
                        crate::ImageAlpha::Straight,
                        crate::ImageAlpha::Premultiplied,
                    ] {
                        self.image_bindings.remove(&ImageKey {
                            id: key.id,
                            filter,
                            alpha,
                        });
                    }
                }
                resource.view = current;
            }
            let Some(view) = &resource.view else {
                continue;
            };
            if !self.image_bindings.contains_key(&key) {
                let options = astrelis::TextureBindingOptions::new()
                    .filter(match key.filter {
                        crate::ImageFilter::Nearest => astrelis::TextureFilter::Nearest,
                        crate::ImageFilter::Linear => astrelis::TextureFilter::Linear,
                    })
                    .alpha(match key.alpha {
                        crate::ImageAlpha::Straight => astrelis::TextureAlpha::Straight,
                        crate::ImageAlpha::Premultiplied => astrelis::TextureAlpha::Premultiplied,
                    });
                let binding =
                    if let crate::image::Source::Framebuffer(source) = image.source.source() {
                        self.painter
                            .textures()
                            .create_sampled_binding_with_options(source, options)?
                    } else {
                        self.painter.create_image_binding(view, options)?
                    };
                self.image_bindings.insert(key, binding);
                self.image_stats.bindings += 1;
            }
            self.painter
                .prepare_image(&self.image_bindings[&key], format)?;
        }
        self.prune_images();
        Ok(())
    }
    fn prune_images(&mut self) {
        let keys: HashSet<_> = self.image_placements.values().copied().collect();
        let ids: HashSet<_> = keys.iter().map(|key| key.id).collect();
        self.images.retain(|id, _| ids.contains(id));
        self.image_bindings.retain(|key, _| keys.contains(key));
    }
    /// Draws the prepared snapshot into an existing pass using one logical-to-physical
    /// transform. The host supplies clipping and controls submission/presentation.
    /// Elements whose own ink cannot reach the destination clip record no draws;
    /// their prepared resources and independently visible descendants are retained.
    /// Returns [`UiError::CompositionRequired`] for opacity below one; use
    /// [`Self::compose`] to record isolated groups before opening the destination pass.
    pub fn paint<T: View>(
        &mut self,
        ui: &Ui<T>,
        pass: &mut RenderPass<'_>,
        scale: f32,
    ) -> Result<(), UiError> {
        profiling::scope!("rxui::UiPainter::paint");
        if ui.needs_composition() {
            return Err(UiError::CompositionRequired);
        }
        self.validate_resources(ui, scale)?;
        self.paint_scope(ui, pass, scale, composition::PaintScope::default())
    }
    fn validate_resources<T: View>(&self, ui: &Ui<T>, scale: f32) -> Result<(), UiError> {
        if !ui.is_prepared() || !scale.is_finite() || scale <= 0. {
            return Err(UiError::InvalidGeometry);
        }
        for &id in ui.painting_ids() {
            match ui.visible_content(id) {
                None | Some(VisibleContent::Other) => {}
                Some(VisibleContent::Text(request)) => {
                    let resource = self.texts.get(&id).ok_or(UiError::InvalidGeometry)?;
                    if !resource.ready()
                        || resource.prepared_revision != request.revision
                        || resource.font_generation != self.generation
                        || Some(resource.prepared_width) != text_width(request.width)
                    {
                        return Err(UiError::InvalidGeometry);
                    }
                    if request.text.is_empty()
                        && let Some(placeholder) = ui.placeholder_request(id)
                        && self.placeholders.get(&id).is_none_or(|p| {
                            !p.current(&placeholder, self.generation, p.resource.raster_scale)
                        })
                    {
                        return Err(UiError::InvalidGeometry);
                    }
                }
                Some(VisibleContent::Image) => {
                    let Some(image) = ui.element(id).and_then(|e| e.image) else {
                        continue;
                    };
                    let key = image_key(&image);
                    if self.image_placements.get(&id) != Some(&key)
                        || !self.images.contains_key(&key.id)
                    {
                        return Err(UiError::InvalidGeometry);
                    }
                    if image.source.pixel_size().is_some()
                        && !self.image_bindings.contains_key(&key)
                    {
                        return Err(UiError::InvalidGeometry);
                    }
                }
                Some(VisibleContent::Custom) => {
                    if let Some(custom) = ui.custom_element(id)
                        && self
                            .customs
                            .get(&id)
                            .is_none_or(|state| (**state).type_id() != custom.state_type())
                    {
                        return Err(UiError::InvalidGeometry);
                    }
                }
            }
        }
        Ok(())
    }
    fn has_visible_ink(
        texts: &IdMap<ElementId, TextResource>,
        element: &crate::ElementInfo<'_>,
        scale: f32,
        origin: [f32; 2],
        scissor: [u32; 4],
    ) -> bool {
        let visible = |bounds: Bounds| {
            let clip = physical_clip(
                bounds.intersection(element.clip_bounds),
                scale,
                origin,
                scissor,
            );
            clip[2] > 0 && clip[3] > 0
        };
        let b = element.bounds;
        if let Some(shadow) = element.paint.shadow
            && b.width > 0.
            && b.height > 0.
            && visible(shadow.extent(b))
        {
            return true;
        }
        if (element.paint.background.is_some()
            || element.paint.border_color.is_some()
            || element.range.is_some()
            || element.kind == crate::ElementType::Custom
            || (element.focus_visible && element.paint.focus_width > 0.))
            && b.width > 0.
            && b.height > 0.
        {
            // UI shapes use an axis-aligned scale. One destination pixel bounds
            // the analytic coverage fringe at every supported raster density.
            let fringe = 1. / scale;
            if visible(Bounds {
                x: b.x - fringe,
                y: b.y - fringe,
                width: b.width + 2. * fringe,
                height: b.height + 2. * fringe,
            }) {
                return true;
            }
        }
        if let Some(image) = &element.image
            && image.source.pixel_size().is_some()
            && visible(image.destination.intersection(element.content_bounds))
        {
            return true;
        }
        if element.editing.is_some() {
            // Selection, preedit and caret are all clipped to the editor's content.
            return visible(element.content_bounds);
        }
        texts
            .get(&element.id)
            .and_then(|t| t.ink_bounds())
            .is_some_and(|ink| {
                visible(Bounds {
                    x: element.content_bounds.x + ink.x,
                    y: element.content_bounds.y + ink.y,
                    width: ink.width,
                    height: ink.height,
                })
            })
    }
    fn paint_scope<T: View>(
        &mut self,
        ui: &Ui<T>,
        pass: &mut RenderPass<'_>,
        scale: f32,
        scope: composition::PaintScope<'_>,
    ) -> Result<(), UiError> {
        let composition::PaintScope { shift, plan, group } = scope;
        let original_scissor = pass.scissor_rect();
        let viewport = pass.viewport();
        let result = (|| -> Result<(), UiError> {
            let mut paint = self.painter.begin(pass)?;
            let mut paint = paint.transformed(
                Transform2D::scale(scale, scale).then(Transform2D::translation(shift[0], shift[1])),
            )?;
            let mut elements = composition::PaintCursor::new(ui, plan, group);
            // A conservative form of `has_visible_ink` on retained node data skips
            // elements such as rows scrolled out of view before building their
            // ElementInfo: the border box with the shapes' fringe, any box shadow,
            // and prepared text ink.
            let texts = &self.texts;
            let culled = |id| {
                let Some(ink) = ui.ink_extent(id) else {
                    return false;
                };
                let visible = |bounds: Bounds| {
                    let clip = physical_clip(
                        bounds.intersection(ink.clip),
                        scale,
                        [viewport[0] + shift[0], viewport[1] + shift[1]],
                        original_scissor,
                    );
                    clip[2] > 0 && clip[3] > 0
                };
                let fringe = 1. / scale;
                let b = ink.bounds;
                !(ink.shadow
                    || visible(Bounds {
                        x: b.x - fringe,
                        y: b.y - fringe,
                        width: b.width + 2. * fringe,
                        height: b.height + 2. * fringe,
                    })
                    || texts
                        .get(&id)
                        .and_then(|t| t.ink_bounds())
                        .is_some_and(|i| {
                            visible(Bounds {
                                x: ink.content.x + i.x,
                                y: ink.content.y + i.y,
                                width: i.width,
                                height: i.height,
                            })
                        }))
            };
            while let Some((element, isolated)) = elements.next(ui, culled) {
                if let Some(layer) = isolated {
                    if let Some(bounds) = layer.bounds {
                        let destination = Bounds {
                            x: bounds[0] as f32 / scale,
                            y: bounds[1] as f32 / scale,
                            width: bounds[2] as f32 / scale,
                            height: bounds[3] as f32 / scale,
                        };
                        let clip = physical_clip(
                            destination,
                            scale,
                            [viewport[0] + shift[0], viewport[1] + shift[1]],
                            original_scissor,
                        );
                        if clip[2] > 0 && clip[3] > 0 {
                            paint
                                .pass()
                                .set_scissor_rect(clip[0], clip[1], clip[2], clip[3])?;
                            let mut paint = element_session(&mut paint, &element)?;
                            paint.draw_image(
                                layer.binding.as_ref().ok_or(UiError::InvalidGeometry)?,
                                astrelis::TextureDraw::new(Rect::new(
                                    destination.x,
                                    destination.y,
                                    destination.width,
                                    destination.height,
                                ))
                                .tint([1., 1., 1., layer.opacity]),
                            )?;
                            self.layer_stats.composites += 1;
                        }
                    }
                    continue;
                }
                // Cull this element's own ink, never its descendants: visible
                // overflow and absolute children can escape a parent's layout box.
                // Keep the original clip for actual draws and conservatively include
                // the shape shader's antialiasing fringe and prepared glyph quads.
                if !Self::has_visible_ink(
                    &self.texts,
                    &element,
                    scale,
                    [viewport[0] + shift[0], viewport[1] + shift[1]],
                    original_scissor,
                ) {
                    continue;
                }
                let clip = physical_clip(
                    element.clip_bounds,
                    scale,
                    [viewport[0] + shift[0], viewport[1] + shift[1]],
                    original_scissor,
                );
                if clip[2] == 0 || clip[3] == 0 {
                    continue;
                }
                paint
                    .pass()
                    .set_scissor_rect(clip[0], clip[1], clip[2], clip[3])?;
                // A rounded ancestor clip narrows this element's own painting; the
                // child session restores the rectangular scissor state when dropped.
                let mut paint = element_session(&mut paint, &element)?;
                let bounds = element.bounds;
                let rect = Rect::new(bounds.x, bounds.y, bounds.width, bounds.height);
                let appearance = element.paint;
                let radii = corner_radii(appearance.radii);
                if bounds.width > 0. && bounds.height > 0. {
                    if let Some(shadow) = appearance.shadow {
                        paint.draw_box_shadow(
                            rect,
                            radii,
                            astrelis::BoxShadow::new(shadow.color)
                                .offset(shadow.offset[0], shadow.offset[1])
                                .blur(shadow.blur)
                                .spread(shadow.spread),
                        )?;
                    }
                    if let Some(color) = appearance.background {
                        paint.draw_shape(ShapeDraw::rounded_rect_corners(rect, radii, color))?;
                    }
                    if let Some(color) = appearance.border_color {
                        let [left, right, top, bottom] = element.border;
                        if left == right && left == top && left == bottom {
                            if left > 0. {
                                paint.draw_shape(
                                    ShapeDraw::rounded_rect_corners(rect, radii, color)
                                        .stroke(Stroke::new(left).inside()),
                                )?;
                            }
                        } else {
                            // Unequal sides paint as edge strips. Rounded corners clip
                            // the strips to the outer contour; the inner contour is square.
                            let mut paint = if radii != CornerRadii::ZERO {
                                paint.clipped_rounded(rect, radii)?
                            } else {
                                paint.transformed(Transform2D::IDENTITY)?
                            };
                            for edge in [
                                Rect::new(
                                    bounds.x,
                                    bounds.y,
                                    left.min(bounds.width),
                                    bounds.height,
                                ),
                                Rect::new(
                                    bounds.x + (bounds.width - right).max(0.),
                                    bounds.y,
                                    right.min(bounds.width),
                                    bounds.height,
                                ),
                                Rect::new(bounds.x, bounds.y, bounds.width, top.min(bounds.height)),
                                Rect::new(
                                    bounds.x,
                                    bounds.y + (bounds.height - bottom).max(0.),
                                    bounds.width,
                                    bottom.min(bounds.height),
                                ),
                            ] {
                                if edge.width > 0. && edge.height > 0. {
                                    paint.fill_rect(edge, color)?;
                                }
                            }
                        }
                    }
                }
                if let Some(range) = element.range {
                    if let Some(thumb) = range.thumb_bounds {
                        paint.fill_rounded_rect(
                            Rect::new(thumb.x, thumb.y, thumb.width, thumb.height),
                            4.,
                            appearance.color,
                        )?;
                    } else {
                        let line = if range.axis == crate::Axis::Horizontal {
                            Rect::new(
                                bounds.x + (bounds.width - 2.) / 2.,
                                bounds.y,
                                2.,
                                bounds.height,
                            )
                        } else {
                            Rect::new(
                                bounds.x,
                                bounds.y + (bounds.height - 2.) / 2.,
                                bounds.width,
                                2.,
                            )
                        };
                        paint.fill_rect(line, appearance.color)?;
                    }
                }
                if let Some(image) = &element.image
                    && image.source.pixel_size().is_some()
                    && image.destination.width > 0.
                    && image.destination.height > 0.
                {
                    let key = image_key(image);
                    let d = image.destination;
                    let image_clip = physical_clip(
                        element.clip_bounds.intersection(element.content_bounds),
                        scale,
                        [viewport[0] + shift[0], viewport[1] + shift[1]],
                        original_scissor,
                    );
                    if image_clip[2] > 0 && image_clip[3] > 0 {
                        paint.pass().set_scissor_rect(
                            image_clip[0],
                            image_clip[1],
                            image_clip[2],
                            image_clip[3],
                        )?;
                        paint.draw_image(
                            &self.image_bindings[&key],
                            astrelis::TextureDraw::new(Rect::new(d.x, d.y, d.width, d.height))
                                .uv(astrelis::UvRect::new(
                                    image.uv[0],
                                    image.uv[1],
                                    image.uv[2],
                                    image.uv[3],
                                ))
                                .tint(image.tint),
                        )?;
                        paint
                            .pass()
                            .set_scissor_rect(clip[0], clip[1], clip[2], clip[3])?;
                    }
                }
                if let Some(editing) = &element.editing {
                    let content_clip = physical_clip(
                        element.clip_bounds.intersection(element.content_bounds),
                        scale,
                        [viewport[0] + shift[0], viewport[1] + shift[1]],
                        original_scissor,
                    );
                    if content_clip[2] == 0 || content_clip[3] == 0 {
                        continue;
                    }
                    {
                        paint.pass().set_scissor_rect(
                            content_clip[0],
                            content_clip[1],
                            content_clip[2],
                            content_clip[3],
                        )?;
                        let resource = &self.texts[&element.id];
                        let origin = [
                            element.content_bounds.x - editing.scroll_x,
                            element.content_bounds.y,
                        ];
                        let selected = if let Some(cursor) = &editing.preedit_cursor {
                            cursor.clone()
                        } else if editing.preedit_range.is_some() {
                            0..0
                        } else {
                            editing.display_selection.range()
                        };
                        for (rect, _) in resource.selection_rects(selected)? {
                            paint.fill_rect(
                                Rect::new(
                                    origin[0] + rect.x,
                                    origin[1] + rect.y,
                                    rect.width,
                                    rect.height,
                                ),
                                appearance.selection_color,
                            )?;
                        }
                    }
                }
                if let Some(custom) = ui.custom_element(element.id) {
                    let state = self
                        .customs
                        .get(&element.id)
                        .ok_or(UiError::InvalidGeometry)?;
                    custom.paint(state.as_ref(), &element, &mut paint)?;
                }
                if element.text.is_some() {
                    let resource = self
                        .texts
                        .get(&element.id)
                        .ok_or(UiError::InvalidGeometry)?;
                    let color = appearance.color;
                    // Native Metal can drop later retained glyph draws when cached
                    // pass bindings survive mixed-renderer/opacity transitions.
                    // Reapply text state; retain shaping, atlas and geometry caches.
                    let _ = paint.pass().as_wgpu();
                    let origin = [
                        element.content_bounds.x
                            - element.editing.as_ref().map_or(0., |e| e.scroll_x),
                        element.content_bounds.y,
                    ];
                    if let Some(prepared) = &resource.prepared {
                        paint.draw_text(prepared, TextDraw::new(origin).color(color))?;
                    } else {
                        // Only chunks overlapping the clip record draws.
                        let chunks = resource.chunks.as_ref().ok_or(UiError::InvalidGeometry)?;
                        let top = element.clip_bounds.y - origin[1];
                        for (prepared, offset) in
                            chunks.visible(top, top + element.clip_bounds.height)
                        {
                            paint.draw_text(
                                prepared,
                                TextDraw::new([origin[0], origin[1] + offset]).color(color),
                            )?;
                        }
                    }
                    // An empty input's placeholder, inside the same content clip.
                    if element.placeholder.is_some()
                        && let Some(editing) = &element.editing
                        && let Some(placeholder) = self
                            .placeholders
                            .get(&element.id)
                            .and_then(|p| p.resource.prepared.as_ref())
                    {
                        paint.draw_text(
                            placeholder,
                            TextDraw::new([element.content_bounds.x, element.content_bounds.y])
                                .color(editing.placeholder_color),
                        )?;
                    }
                }
                if let Some(editing) = &element.editing {
                    let resource = &self.texts[&element.id];
                    let origin = [
                        element.content_bounds.x - editing.scroll_x,
                        element.content_bounds.y,
                    ];
                    // Selection foreground reuses the same prepared text. Clip an
                    // additional draw per visual selection rectangle; no reshaping/upload.
                    if appearance.selection_text_color != appearance.color {
                        let selected = if let Some(cursor) = &editing.preedit_cursor {
                            cursor.clone()
                        } else if editing.preedit_range.is_some() {
                            0..0
                        } else {
                            editing.display_selection.range()
                        };
                        for (rect, part) in resource.selection_rects(selected)? {
                            let selected_bounds = Bounds {
                                x: origin[0] + rect.x,
                                y: origin[1] + rect.y,
                                width: rect.width,
                                height: rect.height,
                            };
                            let selected_clip = physical_clip(
                                selected_bounds
                                    .intersection(element.clip_bounds)
                                    .intersection(element.content_bounds),
                                scale,
                                [viewport[0] + shift[0], viewport[1] + shift[1]],
                                original_scissor,
                            );
                            if selected_clip[2] > 0
                                && selected_clip[3] > 0
                                && let Some((prepared, offset)) = resource.part(part)
                            {
                                paint.pass().set_scissor_rect(
                                    selected_clip[0],
                                    selected_clip[1],
                                    selected_clip[2],
                                    selected_clip[3],
                                )?;
                                paint.draw_text(
                                    prepared,
                                    TextDraw::new([origin[0], origin[1] + offset])
                                        .color(appearance.selection_text_color),
                                )?;
                            }
                        }
                        let content_clip = physical_clip(
                            element.clip_bounds.intersection(element.content_bounds),
                            scale,
                            [viewport[0] + shift[0], viewport[1] + shift[1]],
                            original_scissor,
                        );
                        paint.pass().set_scissor_rect(
                            content_clip[0],
                            content_clip[1],
                            content_clip[2],
                            content_clip[3],
                        )?;
                    }
                    if let Some(range) = &editing.preedit_range {
                        for (rect, _) in resource.selection_rects(range.clone())? {
                            paint.fill_rect(
                                Rect::new(
                                    origin[0] + rect.x,
                                    origin[1] + rect.y + rect.height - 1.,
                                    rect.width,
                                    1.,
                                ),
                                appearance.preedit_color,
                            )?;
                        }
                    }
                    let cursor = editing
                        .preedit_cursor
                        .as_ref()
                        .map(|r| crate::TextPosition::new(r.end))
                        .or_else(|| {
                            editing
                                .preedit_range
                                .is_none()
                                .then_some(editing.display_selection.focus)
                        });
                    if editing.caret_visible
                        && (editing.preedit_range.is_some()
                            || editing.display_selection.is_collapsed())
                        && let Some(caret) = cursor.and_then(|p| resource.caret(ast_position(p)))
                    {
                        paint.fill_rect(
                            Rect::new(
                                origin[0] + caret.origin[0],
                                origin[1] + caret.origin[1],
                                1.,
                                caret.height,
                            ),
                            appearance.caret_color,
                        )?;
                    }
                    paint
                        .pass()
                        .set_scissor_rect(clip[0], clip[1], clip[2], clip[3])?;
                }
                if element.focus_visible
                    && appearance.focus_width > 0.
                    && bounds.width > 0.
                    && bounds.height > 0.
                {
                    paint.draw_shape(
                        ShapeDraw::rounded_rect_corners(rect, radii, appearance.focus_color)
                            .stroke(Stroke::new(appearance.focus_width).inside()),
                    )?;
                }
            }
            // Drag feedback is a destination overlay, recorded once after all
            // ordinary/isolated content. It does not participate in hit testing.
            if group.is_none()
                && let Some((preview, bounds, color)) = ui.dock_preview_paint()
            {
                let clip = physical_clip(
                    bounds,
                    scale,
                    [viewport[0] + shift[0], viewport[1] + shift[1]],
                    original_scissor,
                );
                if clip[2] > 0 && clip[3] > 0 {
                    paint
                        .pass()
                        .set_scissor_rect(clip[0], clip[1], clip[2], clip[3])?;
                    let b = preview.bounds;
                    let rect = Rect::new(b.x, b.y, b.width, b.height);
                    if preview.insertion {
                        paint.fill_rect(rect, color)?;
                    } else {
                        let mut fill = color;
                        fill[3] *= 0.16;
                        paint.fill_rect(rect, fill)?;
                        paint.stroke_rounded_rect(rect, 0., Stroke::new(2.).inside(), color)?;
                    }
                }
            }
            Ok(())
        })();
        pass.set_scissor_rect(
            original_scissor[0],
            original_scissor[1],
            original_scissor[2],
            original_scissor[3],
        )?;
        result
    }
    /// Releases this placement's cached text, images and layers without disturbing other windows using
    /// the same painter. Previously recorded resources retain GPU completion leases.
    pub fn forget<T: View>(&mut self, ui: &Ui<T>) {
        self.texts.retain(|id, _| !ui.owns_element(*id));
        self.image_placements.retain(|id, _| !ui.owns_element(*id));
        self.prune_images();
        self.layers.retain(|id, _| !ui.owns_element(*id));
        self.customs.retain(|id, _| !ui.owns_element(*id));
        self.placeholders.retain(|id, _| !ui.owns_element(*id));
        self.compositions.remove(&ui.tree_id());
        self.passes.remove(&ui.tree_id());
    }
}
pub(crate) fn font_face(info: astrelis::FontInfo) -> crate::FontFace {
    crate::FontFace {
        families: info.families,
        postscript_name: info.postscript_name,
        weight: crate::FontWeight(info.weight),
        style: match info.slant {
            FontSlant::Normal => crate::FontStyle::Normal,
            FontSlant::Italic => crate::FontStyle::Italic,
            FontSlant::Oblique => crate::FontStyle::Oblique,
        },
        monospaced: info.monospaced,
    }
}
fn ast_position(p: crate::TextPosition) -> astrelis::TextPosition {
    astrelis::TextPosition::new(p.byte_offset).affinity(match p.affinity {
        crate::TextAffinity::Upstream => astrelis::TextAffinity::Upstream,
        crate::TextAffinity::Downstream => astrelis::TextAffinity::Downstream,
    })
}
fn ui_position(p: astrelis::TextPosition) -> crate::TextPosition {
    crate::TextPosition {
        byte_offset: p.byte_offset,
        affinity: match p.affinity {
            astrelis::TextAffinity::Upstream => crate::TextAffinity::Upstream,
            astrelis::TextAffinity::Downstream => crate::TextAffinity::Downstream,
        },
    }
}
/// A child session for one element's own painting, clipped to its rounded ancestor
/// clip when it has one. Dropping it restores the parent's scissor and rounded clip.
fn element_session<'a, 'frame>(
    paint: &'a mut astrelis::PaintSession<'_, 'frame>,
    element: &crate::ElementInfo<'_>,
) -> Result<astrelis::PaintSession<'a, 'frame>, UiError> {
    Ok(match element.rounded_clip {
        Some(c) => paint.clipped_rounded(
            Rect::new(c.bounds.x, c.bounds.y, c.bounds.width, c.bounds.height),
            corner_radii(c.radii),
        )?,
        None => paint.transformed(Transform2D::IDENTITY)?,
    })
}
fn corner_radii([top_left, top_right, bottom_right, bottom_left]: [f32; 4]) -> CornerRadii {
    CornerRadii::new(top_left, top_right, bottom_right, bottom_left)
}
fn physical_clip(
    bounds: crate::Bounds,
    scale: f32,
    origin: [f32; 2],
    parent: [u32; 4],
) -> [u32; 4] {
    if bounds.width <= 0. || bounds.height <= 0. {
        return [parent[0], parent[1], 0, 0];
    }
    let left = ((origin[0] + bounds.x * scale).floor().max(0.) as u32)
        .max(parent[0])
        .min(parent[0] + parent[2]);
    let top = ((origin[1] + bounds.y * scale).floor().max(0.) as u32)
        .max(parent[1])
        .min(parent[1] + parent[3]);
    let right = ((origin[0] + (bounds.x + bounds.width) * scale)
        .ceil()
        .max(0.) as u32)
        .min(parent[0] + parent[2])
        .max(left);
    let bottom = ((origin[1] + (bounds.y + bounds.height) * scale)
        .ceil()
        .max(0.) as u32)
        .min(parent[1] + parent[3])
        .max(top);
    [left, top, right - left, bottom - top]
}
/// Geometry for interaction queries: one layout, or stacked paragraph chunks.
enum Query<'a> {
    Single(Arc<TextLayout>),
    Chunked(&'a chunks::Chunks),
}
impl Query<'_> {
    fn hit_test(&self, point: [f32; 2]) -> Option<astrelis::TextPosition> {
        match self {
            Self::Single(layout) => layout.hit_test(point),
            Self::Chunked(chunks) => chunks.hit_test(point),
        }
    }
    fn caret(&self, position: astrelis::TextPosition) -> Option<astrelis::TextCaret> {
        match self {
            Self::Single(layout) => layout.caret(position),
            Self::Chunked(chunks) => chunks.caret(position),
        }
    }
    fn neighbor(
        &self,
        position: astrelis::TextPosition,
        right: bool,
    ) -> Option<astrelis::TextPosition> {
        match self {
            Self::Single(layout) => layout.visual_neighbor(position, right),
            Self::Chunked(chunks) => chunks.neighbor(position, right),
        }
    }
}
impl UiPainter {
    fn interaction_layout(
        &mut self,
        id: ElementId,
        request: TextRequest<'_>,
    ) -> Result<Query<'_>, UiError> {
        let resource = self.texts.entry(id).or_default();
        if resource.chunked(&request) {
            let chunks = resource.chunks.as_mut().unwrap();
            chunks.layout(&mut self.fonts, &self.families, request, self.generation)?;
            return Ok(Query::Chunked(chunks));
        }
        if resource.prepared_revision == request.revision
            && resource.font_generation == self.generation
            && request.width == TextWidth::Available(resource.prepared_width)
            && let Some(layout) = &resource.prepared_layout
        {
            return Ok(Query::Single(layout.clone()));
        }
        Ok(Query::Single(resource.layout(
            &mut self.fonts,
            &self.families,
            request,
        )?))
    }
}
impl TextMeasure for UiPainter {
    fn supports_text_geometry(&self) -> bool {
        true
    }
    fn text_hit_test(
        &mut self,
        id: ElementId,
        request: TextRequest<'_>,
        point: [f32; 2],
    ) -> Result<Option<crate::TextPosition>, UiError> {
        Ok(self
            .interaction_layout(id, request)?
            .hit_test(point)
            .map(ui_position))
    }
    fn text_caret(
        &mut self,
        id: ElementId,
        request: TextRequest<'_>,
        position: crate::TextPosition,
    ) -> Result<Option<crate::Bounds>, UiError> {
        Ok(self
            .interaction_layout(id, request)?
            .caret(ast_position(position))
            .map(|c| crate::Bounds {
                x: c.origin[0],
                y: c.origin[1],
                width: 1.,
                height: c.height,
            }))
    }
    fn text_neighbor(
        &mut self,
        id: ElementId,
        request: TextRequest<'_>,
        position: crate::TextPosition,
        right: bool,
    ) -> Result<Option<crate::TextPosition>, UiError> {
        Ok(self
            .interaction_layout(id, request)?
            .neighbor(ast_position(position), right)
            .map(ui_position))
    }
    fn measure(&mut self, id: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        self.texts.entry(id).or_default().measure_text(
            &mut self.fonts,
            &self.families,
            request,
            self.generation,
        )
    }
    fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntoElement, Runtime, ViewContext, column, label};
    use astrelis::{FramebufferOptions, wgpu};

    struct TextView {
        text: String,
        color: crate::Color,
    }
    impl View for TextView {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            label(self.text.clone()).color(self.color).padding(10.)
        }
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn editing_selection_reuses_prepared_text_and_ime_draws_same_layout() {
        use crate::{TextChangeEvent, TextInputEvent, TextMovement, text_input};
        struct Input(String);
        impl View for Input {
            fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
                text_input(self.0.clone()).width(160.).on_change(
                    cx.listener(|this, event: &TextChangeEvent, _| this.0 = event.value.clone()),
                )
            }
        }
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let errors = graphics
                .device()
                .push_error_scope(wgpu::ErrorFilter::Validation);
            let mut target = graphics
                .create_framebuffer(FramebufferOptions::new(320, 120))
                .unwrap();
            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| cx.new(|_| Input("abc שלום def".into())));
            let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
            let mut painter = UiPainter::new(&graphics);
            painter
                .fonts_mut()
                .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
                .unwrap();
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            ui.focus_next(false);
            let id = ui.focused_element().unwrap();
            let layout = painter.texts[&id].prepared_layout.as_ref().unwrap().clone();
            let interaction_bytes = layout.interaction_bytes();
            assert!(interaction_bytes > 0);
            let first = painter.painter().text().stats();
            let stats = ui.stats();
            for _ in 0..30 {
                ui.text_input(&mut runtime, TextInputEvent::SelectAll, &mut painter)
                    .unwrap();
                ui.text_input(
                    &mut runtime,
                    TextInputEvent::Move {
                        movement: TextMovement::Left,
                        extend: false,
                    },
                    &mut painter,
                )
                .unwrap();
                for _ in 0..20 {
                    ui.text_input(
                        &mut runtime,
                        TextInputEvent::Move {
                            movement: TextMovement::Right,
                            extend: true,
                        },
                        &mut painter,
                    )
                    .unwrap();
                }
                ui.set_caret_visible(false);
                painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            }
            assert_eq!(ui.stats(), stats);
            assert!(Arc::ptr_eq(
                &layout,
                painter.texts[&id].prepared_layout.as_ref().unwrap()
            ));
            assert_eq!(layout.interaction_bytes(), interaction_bytes);
            let unchanged = painter.painter().text().stats();
            assert_eq!(first.geometry_bytes, unchanged.geometry_bytes);
            assert_eq!(first.uploaded_bytes, unchanged.uploaded_bytes);
            assert_eq!(first.cache_misses, unchanged.cache_misses);
            ui.set_theme(crate::Theme::light()).unwrap();
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            assert_eq!(
                ui.stats().component_evaluations,
                stats.component_evaluations
            );
            assert_eq!(ui.stats().measurements, stats.measurements);
            assert_eq!(ui.stats().layout_passes, stats.layout_passes);
            assert!(Arc::ptr_eq(
                &layout,
                painter.texts[&id].prepared_layout.as_ref().unwrap()
            ));
            let themed = painter.painter().text().stats();
            assert_eq!(themed.geometry_bytes, first.geometry_bytes);
            assert_eq!(themed.uploaded_bytes, first.uploaded_bytes);
            assert_eq!(themed.cache_misses, first.cache_misses);
            ui.set_theme(crate::Theme::light().metrics(|m| m.font_size = 18.))
                .unwrap();
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            {
                let mut frame = target.begin_frame().unwrap();
                {
                    let mut pass = frame.render_pass().begin().unwrap();
                    assert!(matches!(
                        painter.paint(&ui, &mut pass, 1.),
                        Err(UiError::InvalidGeometry)
                    ));
                }
                frame.finish().unwrap();
            }
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            assert!(!Arc::ptr_eq(
                &layout,
                painter.texts[&id].prepared_layout.as_ref().unwrap()
            ));
            ui.text_input(&mut runtime, TextInputEvent::SelectAll, &mut painter)
                .unwrap();
            ui.text_input(
                &mut runtime,
                TextInputEvent::Preedit {
                    text: "e\u{301}".into(),
                    cursor: Some((1, 3)),
                },
                &mut painter,
            )
            .unwrap();
            assert_eq!(runtime.update(|cx| root.read(cx).0.clone()), "abc שלום def");
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            {
                let mut frame = target.begin_frame().unwrap();
                {
                    let mut pass = frame.render_pass().begin().unwrap();
                    painter.paint(&ui, &mut pass, 1.).unwrap();
                }
                frame.finish().unwrap();
            }
            // No preedit caret still yields a valid native candidate rectangle.
            ui.text_input(
                &mut runtime,
                TextInputEvent::Preedit {
                    text: "".into(),
                    cursor: None,
                },
                &mut painter,
            )
            .unwrap();
            assert!(ui.ime_cursor_area(&mut painter).unwrap().is_some());
            ui.text_input(
                &mut runtime,
                TextInputEvent::Commit("".into()),
                &mut painter,
            )
            .unwrap();
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            assert!(ui.ime_cursor_area(&mut painter).unwrap().is_some());
            assert!(errors.pop().await.is_none());
        });
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn text_areas_wrap_and_move_by_visual_lines_and_placeholders_paint_only_when_set() {
        use crate::{TextChangeEvent, TextInputEvent, TextMovement, text_area, text_input};
        struct Fields {
            notes: String,
            hint: bool,
        }
        impl View for Fields {
            fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
                let empty = text_input("").key("empty").width(160.);
                column()
                    .child(
                        text_area(self.notes.clone())
                            .key("notes")
                            .width(140.)
                            .on_change(cx.listener(|this, e: &TextChangeEvent, _| {
                                this.notes = e.value.clone()
                            })),
                    )
                    .child(
                        text_input("secret")
                            .key("secret")
                            .width(160.)
                            .password(true),
                    )
                    .child(if self.hint {
                        empty.placeholder("Search")
                    } else {
                        empty
                    })
            }
        }
        fn ink(
            graphics: &GraphicsContext,
            target: &mut astrelis::Framebuffer,
            painter: &mut UiPainter,
            ui: &Ui<Fields>,
            area: Bounds,
        ) -> usize {
            let texture = target.color_texture().unwrap().clone();
            let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("RXUI placeholder readback"),
                size: 256 * 4 * 320,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut frame = target.begin_frame().unwrap();
            {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::BLACK)
                    .begin()
                    .unwrap();
                painter.paint(ui, &mut pass, 1.).unwrap();
            }
            frame.encoder().copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: Default::default(),
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256 * 4),
                        rows_per_image: Some(320),
                    },
                },
                texture.size(),
            );
            let submission = frame.finish().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
            graphics
                .device()
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(std::time::Duration::from_secs(10)),
                })
                .unwrap();
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .unwrap()
                .unwrap();
            let pixels = buffer.slice(..).get_mapped_range().unwrap();
            let pixel = |x: usize, y: usize| &pixels[y * 1024 + x * 4..y * 1024 + x * 4 + 4];
            let background = pixel(area.x as usize + 1, area.y as usize + 1).to_vec();
            let mut count = 0;
            for y in area.y as usize..(area.y + area.height) as usize {
                for x in area.x as usize..(area.x + area.width) as usize {
                    count += usize::from(pixel(x, y) != background.as_slice());
                }
            }
            count
        }
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let errors = graphics
                .device()
                .push_error_scope(wgpu::ErrorFilter::Validation);
            let mut target =
                graphics
                    .create_framebuffer(FramebufferOptions::new(256, 320).usage(
                        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    ))
                    .unwrap();
            let format = target.render_format();
            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| {
                cx.new(|_| Fields {
                    notes: "alpha beta gamma delta epsilon zeta eta theta".into(),
                    hint: true,
                })
            });
            let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
            let mut painter = UiPainter::new(&graphics);
            painter
                .fonts_mut()
                .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
                .unwrap();
            ui.prepare(&mut runtime, [256., 320.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &format, 1.).unwrap();
            let id = |ui: &Ui<Fields>, key: &str| {
                let key = crate::Key::from(key);
                ui.elements().find(|e| e.key == Some(&key)).unwrap().id
            };
            let notes = id(&ui, "notes");
            let lines = painter.texts[&notes]
                .prepared_layout
                .as_ref()
                .unwrap()
                .lines()
                .len();
            assert!(lines >= 3, "the area wraps: {lines} lines");
            assert!(ui.focus(notes));
            let mut send = |ui: &mut Ui<Fields>, movement| {
                ui.text_input(
                    &mut runtime,
                    TextInputEvent::Move {
                        movement,
                        extend: false,
                    },
                    &mut painter,
                )
                .unwrap();
                let caret = ui.ime_cursor_area(&mut painter).unwrap().unwrap();
                let offset = ui.text_input_info(notes).unwrap().selection.focus;
                (caret, offset)
            };
            let (start, _) = send(&mut ui, TextMovement::Start);
            for _ in 0..8 {
                send(&mut ui, TextMovement::Right);
            }
            let (first, at) = send(&mut ui, TextMovement::Right);
            let (second, below) = send(&mut ui, TextMovement::Down);
            assert!(second.y > first.y && below.byte_offset > at.byte_offset);
            // The goal X survives a move to a line whose caret stops differ.
            assert!((second.x - first.x).abs() < 8.);
            let (back, above) = send(&mut ui, TextMovement::Up);
            assert_eq!((back.x, back.y, above), (first.x, first.y, at));
            // Line end stays on the soft-wrapped visual line instead of the next one.
            let (end, wrap) = send(&mut ui, TextMovement::LineEnd);
            assert_eq!(end.y, start.y);
            assert!(end.x > first.x);
            assert_eq!(wrap.affinity, crate::TextAffinity::Upstream);
            let (line_start, _) = send(&mut ui, TextMovement::LineStart);
            assert_eq!((line_start.x, line_start.y), (start.x, start.y));
            // The whole area is visible: a page reaches the last line, and paging
            // past it reaches the end.
            let (paged, on_last) = send(&mut ui, TextMovement::PageDown);
            assert!(paged.y >= second.y + second.height);
            let (end_caret, last) = send(&mut ui, TextMovement::PageDown);
            assert_eq!(end_caret.y, paged.y);
            assert!(last.byte_offset > on_last.byte_offset);
            assert_eq!(
                last.byte_offset,
                runtime.update(|cx| root.read(cx).notes.len())
            );
            let height = ui.element(notes).unwrap().bounds.height;
            ui.text_input(
                &mut runtime,
                TextInputEvent::Insert("\n\n".into()),
                &mut painter,
            )
            .unwrap();
            ui.prepare(&mut runtime, [256., 320.], &mut painter)
                .unwrap();
            assert!(ui.element(notes).unwrap().bounds.height > height);
            let secret = id(&ui, "secret");
            assert_eq!(ui.element(secret).unwrap().text, Some("••••••"));
            painter.prepare(&ui, &format, 1.).unwrap();
            let empty = ui.element(id(&ui, "empty")).unwrap().content_bounds;
            let hinted = ink(&graphics, &mut target, &mut painter, &ui, empty);
            assert!(hinted > 0, "placeholder ink is painted");
            runtime.update(|cx| root.update(cx, |f, _| f.hint = false));
            ui.prepare(&mut runtime, [256., 320.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &format, 1.).unwrap();
            assert!(painter.placeholders.is_empty());
            assert_eq!(ink(&graphics, &mut target, &mut painter, &ui, empty), 0);
            assert!(errors.pop().await.is_none());
        });
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn registered_fonts_report_faces_and_family_maps_re_measure_text() {
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let mut painter = UiPainter::new(&graphics);
            let generation = painter.generation();
            assert!(matches!(
                painter.register_font(vec![0u8; 16]),
                Err(UiError::Text(_))
            ));
            assert_eq!(painter.generation(), generation);
            let faces = painter
                .register_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice())
                .unwrap();
            assert_eq!(faces.len(), 1);
            assert!(faces[0].families.iter().any(|f| f == "Source Sans 3"));
            assert_eq!(faces[0].weight, crate::FontWeight::NORMAL);
            assert_eq!(faces[0].style, crate::FontStyle::Normal);
            assert!(!faces[0].monospaced);
            assert!(painter.generation() > generation);

            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| {
                cx.new(|_| TextView {
                    text: "Mapped".into(),
                    color: [1.; 4],
                })
            });
            let mut ui = Ui::new(&mut runtime, root).unwrap();
            ui.prepare(&mut runtime, [200., 80.], &mut painter).unwrap();
            let id = ui.elements().find(|e| e.text.is_some()).unwrap().id;
            let family = |painter: &UiPainter| painter.texts[&id].buffer.style().family.clone();
            assert_eq!(family(&painter), FontFamily::SansSerif);
            let measured = ui.stats().measurements;
            painter.map_family(crate::FontFamily::SansSerif, "Source Sans 3");
            assert_eq!(
                painter.family_mapping(&crate::FontFamily::SansSerif),
                Some("Source Sans 3")
            );
            let mapped = painter.generation();
            // An unchanged mapping keeps measurements.
            painter.map_family(crate::FontFamily::SansSerif, "Source Sans 3");
            assert_eq!(painter.generation(), mapped);
            ui.prepare(&mut runtime, [200., 80.], &mut painter).unwrap();
            assert!(ui.stats().measurements > measured);
            assert_eq!(family(&painter), FontFamily::Named("Source Sans 3".into()));
            assert!(painter.unmap_family(&crate::FontFamily::SansSerif));
            assert!(!painter.unmap_family(&crate::FontFamily::SansSerif));
            ui.prepare(&mut runtime, [200., 80.], &mut painter).unwrap();
            assert_eq!(family(&painter), FontFamily::SansSerif);
        });
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn chunked_long_text_matches_single_buffer_geometry_through_edits() {
        use crate::{TextChangeEvent, TextInputEvent, TextSelection, text_area};
        struct Doc(String);
        impl View for Doc {
            fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
                column().size(320., 400.).scroll_y().child(
                    text_area(self.0.clone()).key("doc").width(300.).on_change(
                        cx.listener(|this, e: &TextChangeEvent, _| this.0 = e.value.clone()),
                    ),
                )
            }
        }
        let text: String = (0..300)
            .map(|i| match i % 7 {
                0 => String::new(),
                1 => "שלום עולם, mixed direction text".into(),
                2 => "a long paragraph that wraps across several visual lines because it \
                      keeps going well past the width of the area"
                    .into(),
                _ => format!("paragraph {i} e\u{301}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let target = graphics
                .create_framebuffer(FramebufferOptions::new(320, 400))
                .unwrap();
            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| cx.new(|_| Doc(text)));
            let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
            let mut painter = UiPainter::new(&graphics);
            painter
                .register_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf").as_slice())
                .unwrap();
            ui.prepare(&mut runtime, [320., 400.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let id = ui
                .elements()
                .find(|e| e.kind == crate::ElementType::TextInput)
                .unwrap()
                .id;
            assert!(ui.focus(id));
            // Compares every query against one unchunked buffer of the same text.
            let compare = |ui: &Ui<Doc>, painter: &mut UiPainter| {
                let info = ui.element(id).unwrap();
                let value = info.text.unwrap().to_owned();
                let request = TextRequest {
                    text: &value,
                    font_size: info.font_size,
                    width: TextWidth::Available(info.content_bounds.width),
                    single_line: false,
                    style: info.text_style,
                    revision: info.text_revision,
                };
                assert!(painter.texts[&id].chunks.is_some());
                let mut reference = TextResource::default();
                let single = reference
                    .layout(&mut painter.fonts, &painter.families, request)
                    .unwrap();
                let size = painter.measure(id, request).unwrap();
                // Stacked chunk heights and per-line accumulation round differently
                // in f32: about 0.03 units after 7,500.
                let near = |a: f32, b: f32| (a - b).abs() <= 0.1;
                assert!(
                    near(size[1], single.size()[1]),
                    "{size:?} {:?}",
                    single.size()
                );
                assert!(near(info.content_bounds.height, single.size()[1]));
                let boundaries: Vec<_> = (0..=value.len())
                    .filter(|i| single.is_text_boundary(*i))
                    .collect();
                for &offset in boundaries.iter().step_by(7) {
                    for affinity in [
                        crate::TextAffinity::Downstream,
                        crate::TextAffinity::Upstream,
                    ] {
                        let position = crate::TextPosition {
                            byte_offset: offset,
                            affinity,
                        };
                        let expected = single.caret(ast_position(position));
                        let actual = painter.text_caret(id, request, position).unwrap();
                        match (expected, actual) {
                            (Some(e), Some(a)) => {
                                assert!(near(e.origin[0], a.x), "x at {offset}");
                                assert!(near(e.origin[1], a.y), "y at {offset}");
                            }
                            (None, None) => {}
                            other => panic!("caret at {offset}: {other:?}"),
                        }
                        for right in [false, true] {
                            let expected = single
                                .visual_neighbor(ast_position(position), right)
                                .map(ui_position)
                                .map(|p| p.byte_offset);
                            let actual = painter
                                .text_neighbor(id, request, position, right)
                                .unwrap()
                                .map(|p| p.byte_offset);
                            assert_eq!(expected, actual, "neighbor of {offset} right {right}");
                        }
                    }
                }
                for y in (0..(single.size()[1] as usize)).step_by(9) {
                    // Avoid points within rounding distance of a line edge.
                    let y = y as f32 + 0.5;
                    if single.lines().iter().any(|l| (l.top - y).abs() < 0.2) {
                        continue;
                    }
                    for x in [-5., 3., 47., 151., 290., 500.] {
                        let point = [x, y];
                        assert_eq!(
                            single.hit_test(point).map(ui_position),
                            painter.text_hit_test(id, request, point).unwrap(),
                            "hit {point:?}"
                        );
                    }
                }
                let resource = &painter.texts[&id];
                for range in [
                    0..value.len(),
                    40..value.len() / 2,
                    value.len() / 3..value.len() - 9,
                ] {
                    let start = boundaries[boundaries.partition_point(|b| *b < range.start)];
                    let end = boundaries[boundaries.partition_point(|b| *b < range.end)];
                    let expected: Vec<_> = single
                        .selection_rects(start..end)
                        .unwrap()
                        .map(|r| (r.x, r.y, r.width, r.height))
                        .collect();
                    let actual: Vec<_> = resource
                        .selection_rects(start..end)
                        .unwrap()
                        .into_iter()
                        .map(|(r, _)| (r.x, r.y, r.width, r.height))
                        .collect();
                    assert_eq!(expected.len(), actual.len());
                    for (e, a) in expected.iter().zip(&actual) {
                        assert!(near(e.0, a.0) && near(e.1, a.1), "{e:?} {a:?}");
                        assert!(near(e.2, a.2) && near(e.3, a.3), "{e:?} {a:?}");
                    }
                }
            };
            compare(&ui, &mut painter);
            let chunks = painter.texts[&id].chunks.as_ref().unwrap().list.len();
            assert!(chunks >= 4, "{chunks} chunks");
            // Edits at a chunk boundary, joining chunks and inserting many lines.
            let boundary = painter.texts[&id].chunks.as_ref().unwrap().list[1].start;
            let mut select =
                |ui: &mut Ui<Doc>, painter: &mut UiPainter, range: std::ops::Range<usize>| {
                    let revision = ui.semantic_node(id).unwrap().text_revision;
                    ui.semantic_action(
                        &mut runtime,
                        crate::SemanticAction::SetSelection {
                            target: id,
                            text_revision: revision,
                            selection: TextSelection {
                                anchor: crate::TextPosition::new(range.start),
                                focus: crate::TextPosition::new(range.end),
                            },
                        },
                        painter,
                    )
                    .unwrap();
                };
            select(&mut ui, &mut painter, boundary..boundary);
            for event in [
                TextInputEvent::Insert("x".into()),
                TextInputEvent::Backspace,
                TextInputEvent::Backspace,
                TextInputEvent::Paste("one\ntwo\nthree\n".repeat(80)),
            ] {
                ui.text_input(&mut runtime, event, &mut painter).unwrap();
                ui.prepare(&mut runtime, [320., 400.], &mut painter)
                    .unwrap();
                painter.prepare(&ui, &target.render_format(), 1.).unwrap();
                compare(&ui, &mut painter);
            }
            // Painting records only visible chunks and passes validation.
            let mut target = target;
            let errors = graphics
                .device()
                .push_error_scope(wgpu::ErrorFilter::Validation);
            {
                let mut frame = target.begin_frame().unwrap();
                {
                    let mut pass = frame.render_pass().begin().unwrap();
                    painter.paint(&ui, &mut pass, 1.).unwrap();
                }
                frame.finish().unwrap();
            }
            assert!(errors.pop().await.is_none());
        });
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn intrinsic_probes_measure_without_replacing_the_prepared_layout() {
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let target = graphics
                .create_framebuffer(FramebufferOptions::new(320, 120))
                .unwrap();
            let mut runtime = Runtime::new();
            let entity = runtime.update(|cx| {
                cx.new(|_| TextView {
                    text: "Probe widths without reshaping".into(),
                    color: [1.; 4],
                })
            });
            let mut ui = Ui::new(&mut runtime, entity).unwrap();
            let mut painter = UiPainter::new(&graphics);
            painter
                .fonts_mut()
                .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
                .unwrap();
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let element = ui.elements().find(|e| e.text.is_some()).unwrap();
            let id = element.id;
            let layout = painter.texts[&id].prepared_layout.clone().unwrap();
            let request = |width| TextRequest {
                text: "Probe widths without reshaping",
                font_size: element.font_size,
                width,
                single_line: false,
                style: element.text_style,
                revision: element.text_revision,
            };
            let [min, height] = painter.measure(id, request(TextWidth::MinContent)).unwrap();
            let [max, line] = painter.measure(id, request(TextWidth::MaxContent)).unwrap();
            assert!(min < max && height > line);
            assert_eq!(
                painter
                    .measure(id, request(TextWidth::Available(max)))
                    .unwrap(),
                [max, line]
            );
            let stats = painter.painter().text().stats();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            assert!(Arc::ptr_eq(
                &layout,
                painter.texts[&id].prepared_layout.as_ref().unwrap()
            ));
            let after = painter.painter().text().stats();
            assert_eq!(after.geometry_bytes, stats.geometry_bytes);
            assert_eq!(after.uploaded_bytes, stats.uploaded_bytes);
            assert_eq!(after.cache_misses, stats.cache_misses);
        });
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn scroll_clipping_matches_pixels_and_restores_caller_scissor() {
        struct Blocks;
        impl View for Blocks {
            fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
                column()
                    .width(40.)
                    .height(20.)
                    .scroll_y()
                    .child(column().height(20.).background([1., 0., 0., 1.]))
                    .child(column().height(20.).background([0., 1., 0., 1.]))
            }
        }
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let errors = graphics
                .device()
                .push_error_scope(wgpu::ErrorFilter::Validation);
            let mut target =
                graphics
                    .create_framebuffer(FramebufferOptions::new(64, 64).usage(
                        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    ))
                    .unwrap();
            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| cx.new(|_| Blocks));
            let mut ui = Ui::new(&mut runtime, root).unwrap();
            let mut painter = UiPainter::new(&graphics);
            ui.prepare(&mut runtime, [64., 64.], &mut painter).unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            for expected in [[255, 0, 0, 255], [0, 255, 0, 255]] {
                let texture = target.color_texture().unwrap().clone();
                let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some("RXUI clip readback"),
                    size: 64 * 256,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut frame = target.begin_frame().unwrap();
                {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()
                        .unwrap();
                    pass.set_scissor_rect(5, 5, 50, 50).unwrap();
                    painter.paint(&ui, &mut pass, 1.).unwrap();
                    assert_eq!(pass.scissor_rect(), [5, 5, 50, 50]);
                }
                frame.encoder().copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: Default::default(),
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(256),
                            rows_per_image: Some(64),
                        },
                    },
                    texture.size(),
                );
                let submission = frame.finish().unwrap();
                let (tx, rx) = std::sync::mpsc::channel();
                buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
                graphics
                    .device()
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(std::time::Duration::from_secs(10)),
                    })
                    .unwrap();
                rx.recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                {
                    let pixels = buffer.slice(..).get_mapped_range().unwrap();
                    let pixel = |x: usize, y: usize| &pixels[y * 256 + x * 4..y * 256 + x * 4 + 4];
                    assert_eq!(pixel(10, 10), expected);
                    assert_eq!(pixel(10, 30), [0, 0, 0, 255]); // Ancestor clip.
                    assert_eq!(pixel(2, 10), [0, 0, 0, 255]); // Caller scissor.
                }
                buffer.unmap();
                let stats = ui.stats();
                ui.scroll([10., 10.], [0., 20.]).unwrap();
                assert_eq!(ui.stats(), stats);
            }
            assert!(errors.pop().await.is_none());
        });
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn themed_rounded_boxes_and_borders_match_pixels_after_palette_switch() {
        struct BoxView;
        impl View for BoxView {
            fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
                column()
                    .width(40.)
                    .height(40.)
                    .radius(12.)
                    .background(crate::ThemeColor::Surface)
                    .border(4., crate::ThemeColor::Border)
            }
        }
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let errors = graphics
                .device()
                .push_error_scope(wgpu::ErrorFilter::Validation);
            let mut target = graphics
                .create_framebuffer(
                    FramebufferOptions::new(64, 64)
                        .format(wgpu::TextureFormat::Rgba8UnormSrgb)
                        .usage(
                            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                        ),
                )
                .unwrap();
            let mut runtime = Runtime::new();
            let root = runtime.update(|cx| cx.new(|_| BoxView));
            let mut ui = Ui::new(&mut runtime, root).unwrap();
            let mut painter = UiPainter::new(&graphics);
            for (theme, surface, border) in [
                (crate::Theme::dark(), [27_u8, 28, 33], [62_u8, 65, 74]),
                (crate::Theme::light(), [255, 255, 255], [195, 199, 206]),
            ] {
                ui.set_theme(theme).unwrap();
                ui.prepare(&mut runtime, [64., 64.], &mut painter).unwrap();
                painter.prepare(&ui, &target.render_format(), 1.).unwrap();
                let texture = target.color_texture().unwrap().clone();
                let buffer = graphics.device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some("RXUI theme readback"),
                    size: 64 * 256,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut frame = target.begin_frame().unwrap();
                {
                    let mut pass = frame
                        .render_pass()
                        .clear_color(wgpu::Color::BLACK)
                        .begin()
                        .unwrap();
                    pass.set_scissor_rect(0, 0, 60, 60).unwrap();
                    painter.paint(&ui, &mut pass, 1.).unwrap();
                    assert_eq!(pass.scissor_rect(), [0, 0, 60, 60]);
                }
                frame.encoder().copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: Default::default(),
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(256),
                            rows_per_image: Some(64),
                        },
                    },
                    texture.size(),
                );
                let submission = frame.finish().unwrap();
                let (tx, rx) = std::sync::mpsc::channel();
                buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
                graphics
                    .device()
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(std::time::Duration::from_secs(10)),
                    })
                    .unwrap();
                rx.recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                {
                    let pixels = buffer.slice(..).get_mapped_range().unwrap();
                    let pixel = |x: usize, y: usize| &pixels[y * 256 + x * 4..y * 256 + x * 4 + 4];
                    assert_eq!(pixel(1, 1), [0, 0, 0, 255]); // Rounded corner, outside contour.
                    assert_eq!(pixel(20, 20), [surface[0], surface[1], surface[2], 255]);
                    assert_eq!(pixel(20, 2), [border[0], border[1], border[2], 255]);
                    assert_eq!(pixel(50, 20), [0, 0, 0, 255]);
                }
                buffer.unmap();
            }
            assert!(errors.pop().await.is_none());
        });
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn unchanged_placement_skips_the_text_pass_until_its_key_changes() {
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let target = graphics
                .create_framebuffer(FramebufferOptions::new(320, 120))
                .unwrap();
            let format = target.render_format();
            let mut runtime = Runtime::new();
            let entity = runtime.update(|cx| {
                cx.new(|_| TextView {
                    text: "Retained text".into(),
                    color: [1.; 4],
                })
            });
            let mut ui = Ui::new(&mut runtime, entity.clone()).unwrap();
            let mut painter = UiPainter::new(&graphics);
            painter
                .fonts_mut()
                .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
                .unwrap();
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &format, 1.).unwrap();
            let id = ui.elements().find(|e| e.text.is_some()).unwrap().id;
            // A dropped resource shows whether the next prepare walked the text.
            painter.texts.remove(&id);
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &format, 1.).unwrap();
            assert!(!painter.texts.contains_key(&id), "unchanged placement");
            painter.prepare(&ui, &format, 2.).unwrap();
            assert!(painter.texts.contains_key(&id), "raster scale change");
            painter.texts.remove(&id);
            runtime.update(|cx| entity.update(cx, |this, _| this.color = [0.5, 0.6, 0.7, 1.]));
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &format, 2.).unwrap();
            assert!(painter.texts.contains_key(&id), "reevaluated view");
            painter.forget(&ui);
            assert!(painter.passes.is_empty());
        });
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn retained_text_reuses_preparation_and_rejects_stale_resources_before_draws() {
        pollster::block_on(async {
            let graphics = GraphicsContext::headless().await.unwrap();
            let errors = graphics
                .device()
                .push_error_scope(wgpu::ErrorFilter::Validation);
            let mut target = graphics
                .create_framebuffer(FramebufferOptions::new(320, 120))
                .unwrap();
            let mut runtime = Runtime::new();
            let entity = runtime.update(|cx| {
                cx.new(|_| TextView {
                    text: "Retained text".into(),
                    color: [1.; 4],
                })
            });
            let mut ui = Ui::new(&mut runtime, entity.clone()).unwrap();
            let mut painter = UiPainter::new(&graphics);
            painter
                .fonts_mut()
                .load_font(include_bytes!("../tests/fonts/SourceSans3-Regular.otf"))
                .unwrap();
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let first = painter.painter().text().stats();
            assert!(first.geometry_bytes > 0);
            runtime.update(|cx| entity.update(cx, |this, _| this.color = [0.5, 0.6, 0.7, 1.]));
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            let unchanged = painter.painter().text().stats();
            assert_eq!(first.geometry_bytes, unchanged.geometry_bytes);
            assert_eq!(first.uploaded_bytes, unchanged.uploaded_bytes);
            assert_eq!(first.cache_misses, unchanged.cache_misses);
            let mut other = Ui::new(&mut runtime, entity.clone()).unwrap();
            other
                .prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            painter
                .prepare(&other, &target.render_format(), 1.)
                .unwrap();
            assert!(painter.texts.keys().any(|id| ui.owns_element(*id)));
            assert!(painter.texts.keys().any(|id| other.owns_element(*id)));
            {
                let mut frame = target.begin_frame().unwrap();
                {
                    let mut pass = frame.render_pass().begin().unwrap();
                    pass.set_scissor_rect(8, 6, 290, 100).unwrap();
                    let original = pass.scissor_rect();
                    painter.paint(&ui, &mut pass, 1.).unwrap();
                    assert_eq!(pass.scissor_rect(), original);
                    painter.paint(&other, &mut pass, 1.).unwrap();
                    assert_eq!(pass.scissor_rect(), original);
                }
                frame.finish().unwrap();
            }
            painter.forget(&other);
            assert!(painter.texts.keys().all(|id| ui.owns_element(*id)));
            let draws = painter.painter().text().stats().draw_calls;
            runtime.update(|cx| entity.update(cx, |this, _| this.text = "Changed text".into()));
            ui.prepare(&mut runtime, [320., 120.], &mut painter)
                .unwrap();
            {
                let mut frame = target.begin_frame().unwrap();
                let mut pass = frame.render_pass().begin().unwrap();
                assert!(matches!(
                    painter.paint(&ui, &mut pass, 1.),
                    Err(UiError::InvalidGeometry)
                ));
            }
            assert_eq!(painter.painter().text().stats().draw_calls, draws);
            painter.prepare(&ui, &target.render_format(), 1.).unwrap();
            {
                let mut frame = target.begin_frame().unwrap();
                {
                    let mut pass = frame.render_pass().begin().unwrap();
                    painter.paint(&ui, &mut pass, 1.).unwrap();
                }
                frame.finish().unwrap();
            }
            assert!(painter.painter().text().stats().geometry_bytes > first.geometry_bytes);
            assert!(errors.pop().await.is_none());
        });
    }
}

#[cfg(test)]
#[path = "image_gpu_tests.rs"]
mod image_gpu_tests;

#[cfg(test)]
#[path = "layout_gpu_tests.rs"]
mod layout_gpu_tests;

#[path = "compositing.rs"]
mod composition;

#[path = "text_chunks.rs"]
mod chunks;
pub use composition::{ComposedUi, LayerStats};

#[cfg(test)]
#[path = "compositing_gpu_tests.rs"]
mod compositing_gpu_tests;

#[cfg(test)]
#[path = "controls_gpu_tests.rs"]
mod controls_gpu_tests;

#[cfg(test)]
#[path = "culling_gpu_tests.rs"]
mod culling_gpu_tests;

#[cfg(test)]
#[path = "style_gpu_tests.rs"]
mod style_gpu_tests;
