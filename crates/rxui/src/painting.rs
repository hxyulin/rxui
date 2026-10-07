use crate::{Bounds, ElementId, TextMeasure, TextRequest, TextWidth, Ui, UiError, View};
use astrelis::{
    GraphicsContext, Painter, PreparedText, Rect, RenderFormat, RenderPass, Stroke, TextBuffer,
    TextDraw, TextLayout, TextRasterOptions, TextStyle, TextSystem, TextWrap, Transform2D,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

impl From<astrelis::Error> for UiError {
    fn from(error: astrelis::Error) -> Self {
        Self::Graphics(error)
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
}
impl TextResource {
    fn layout(
        &mut self,
        fonts: &mut TextSystem,
        request: TextRequest<'_>,
    ) -> Result<Arc<TextLayout>, UiError> {
        self.buffer.set_text(
            request.text,
            TextStyle::new()
                .font_size(request.font_size)
                .line_height(request.font_size * 1.4),
        )?;
        self.buffer.set_wrap(if request.single_line {
            TextWrap::None
        } else {
            TextWrap::Word
        });
        self.buffer.set_width(match request.width {
            TextWidth::MinContent => Some(0.),
            TextWidth::MaxContent => None,
            TextWidth::Available(width) => Some(width),
        })?;
        Ok(self.buffer.layout(fonts)?)
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
/// Painting uses caller-selected passes/clipping. No font discovery is implicit.
pub struct UiPainter {
    fonts: TextSystem,
    generation: u64,
    painter: Painter,
    texts: HashMap<ElementId, TextResource>,
    graphics: GraphicsContext,
    images: HashMap<crate::ImageId, ImageResource>,
    image_bindings: HashMap<ImageKey, astrelis::TextureBinding>,
    image_placements: HashMap<ElementId, ImageKey>,
    image_stats: ImageStats,
    layers: HashMap<ElementId, composition::LayerResource>,
    compositions: HashMap<u64, Arc<composition::CompositionPlan>>,
    layer_stats: LayerStats,
}
impl UiPainter {
    /// Creates empty fonts and renderer caches for this graphics device.
    pub fn new(graphics: &GraphicsContext) -> Self {
        Self {
            fonts: TextSystem::new(),
            generation: 0,
            painter: Painter::new(graphics),
            texts: HashMap::new(),
            graphics: graphics.clone(),
            images: HashMap::new(),
            image_bindings: HashMap::new(),
            image_placements: HashMap::new(),
            image_stats: ImageStats::default(),
            layers: HashMap::new(),
            compositions: HashMap::new(),
            layer_stats: LayerStats::default(),
        }
    }
    /// Cumulative image upload/binding counters.
    pub fn image_stats(&self) -> ImageStats {
        self.image_stats
    }
    /// Font loading/configuration access. Each call advances measurement generation,
    /// invalidating text measurements on the next prepare. Load/discover once at startup.
    pub fn fonts_mut(&mut self) -> &mut TextSystem {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("RXUI font generation exhausted");
        &mut self.fonts
    }
    /// Read-only font information.
    pub fn fonts(&self) -> &TextSystem {
        &self.fonts
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
        self.texts
            .retain(|id, _| !ui.owns_element(*id) || ui.contains_element(*id));
        self.painter.prepare(format)?;
        self.prepare_images(ui, format)?;
        for element in ui.elements() {
            let Some(text) = element.text else {
                self.texts.remove(&element.id);
                continue;
            };
            let resource = self.texts.entry(element.id).or_default();
            if resource.prepared.is_some()
                && resource.prepared_revision == element.text_revision
                && resource.prepared_width == element.content_bounds.width
                && resource.font_generation == self.generation
                && resource.raster_scale == raster_scale
            {
                continue;
            }
            let layout = resource.layout(
                &mut self.fonts,
                TextRequest {
                    text,
                    font_size: element.font_size,
                    width: TextWidth::Available(element.content_bounds.width),
                    single_line: element.editing.is_some(),
                    revision: element.text_revision,
                },
            )?;
            if element.editing.is_some() {
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
                resource.prepared_width = element.content_bounds.width;
            }
            resource.prepared_revision = element.text_revision;
            resource.font_generation = self.generation;
        }
        self.prepare_composition(ui, format, raster_scale)?;
        Ok(())
    }
    fn prepare_images<T: View>(
        &mut self,
        ui: &Ui<T>,
        format: &RenderFormat,
    ) -> Result<(), UiError> {
        self.image_placements
            .retain(|id, _| !ui.owns_element(*id) || ui.contains_element(*id));
        let mut seen = HashSet::new();
        for element in ui.elements() {
            let Some(image) = element.image else {
                self.image_placements.remove(&element.id);
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
        for element in ui.elements() {
            if let Some(image) = &element.image {
                let key = image_key(image);
                if self.image_placements.get(&element.id) != Some(&key)
                    || !self.images.contains_key(&key.id)
                {
                    return Err(UiError::InvalidGeometry);
                }
                if image.source.pixel_size().is_some() && !self.image_bindings.contains_key(&key) {
                    return Err(UiError::InvalidGeometry);
                }
            }
            if element.text.is_some() {
                let resource = self
                    .texts
                    .get(&element.id)
                    .ok_or(UiError::InvalidGeometry)?;
                if resource.prepared.is_none()
                    || resource.prepared_revision != element.text_revision
                    || resource.font_generation != self.generation
                    || resource.prepared_width != element.content_bounds.width
                {
                    return Err(UiError::InvalidGeometry);
                }
            }
        }
        Ok(())
    }
    fn has_visible_ink(
        texts: &HashMap<ElementId, TextResource>,
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
        if (element.paint.background.is_some()
            || element.paint.border_color.is_some()
            || element.range.is_some()
            || (element.focused && element.paint.focus_width > 0.))
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
            .and_then(|t| t.prepared.as_ref())
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
            while let Some((element, isolated)) = elements.next(ui) {
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
                let bounds = element.bounds;
                let rect = Rect::new(bounds.x, bounds.y, bounds.width, bounds.height);
                let appearance = element.paint;
                if bounds.width > 0. && bounds.height > 0. {
                    if let Some(color) = appearance.background {
                        paint.fill_rounded_rect(rect, appearance.radius, color)?;
                    }
                    if let Some(color) = appearance.border_color {
                        let [left, right, top, bottom] = element.border;
                        if left == right && left == top && left == bottom {
                            if left > 0. {
                                paint.stroke_rounded_rect(
                                    rect,
                                    appearance.radius,
                                    Stroke::new(left).inside(),
                                    color,
                                )?;
                            }
                        } else {
                            // Full Taffy customization can supply unequal border widths.
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
                        let layout = self.texts[&element.id].prepared_layout.as_ref().unwrap();
                        let origin = [
                            element.content_bounds.x - editing.scroll_x,
                            element.content_bounds.y,
                        ];
                        let selected = if let Some(cursor) = &editing.preedit_cursor {
                            cursor.clone()
                        } else if editing.preedit_range.is_some() {
                            0..0
                        } else {
                            editing.selection.range()
                        };
                        for rect in layout.selection_rects(selected)? {
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
                if element.text.is_some() {
                    let resource = self
                        .texts
                        .get(&element.id)
                        .ok_or(UiError::InvalidGeometry)?;
                    let prepared = resource.prepared.as_ref().ok_or(UiError::InvalidGeometry)?;
                    let color = appearance.color;
                    // Native Metal can drop later retained glyph draws when cached
                    // pass bindings survive mixed-renderer/opacity transitions.
                    // Reapply text state; retain shaping, atlas and geometry caches.
                    let _ = paint.pass().as_wgpu();
                    paint.draw_text(
                        prepared,
                        TextDraw::new([
                            element.content_bounds.x
                                - element.editing.as_ref().map_or(0., |e| e.scroll_x),
                            element.content_bounds.y,
                        ])
                        .color(color),
                    )?;
                }
                if let Some(editing) = &element.editing {
                    let layout = self.texts[&element.id].prepared_layout.as_ref().unwrap();
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
                            editing.selection.range()
                        };
                        let prepared = self.texts[&element.id].prepared.as_ref().unwrap();
                        for rect in layout.selection_rects(selected)? {
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
                            if selected_clip[2] > 0 && selected_clip[3] > 0 {
                                paint.pass().set_scissor_rect(
                                    selected_clip[0],
                                    selected_clip[1],
                                    selected_clip[2],
                                    selected_clip[3],
                                )?;
                                paint.draw_text(
                                    prepared,
                                    TextDraw::new(origin).color(appearance.selection_text_color),
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
                        for rect in layout.selection_rects(range.clone())? {
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
                                .then_some(editing.selection.focus)
                        });
                    if editing.caret_visible
                        && (editing.preedit_range.is_some() || editing.selection.is_collapsed())
                        && let Some(caret) = cursor.and_then(|p| layout.caret(ast_position(p)))
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
                if element.focused
                    && appearance.focus_width > 0.
                    && bounds.width > 0.
                    && bounds.height > 0.
                {
                    paint.stroke_rounded_rect(
                        rect,
                        appearance.radius,
                        Stroke::new(appearance.focus_width).inside(),
                        appearance.focus_color,
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
        self.compositions.remove(&ui.tree_id());
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
impl UiPainter {
    fn interaction_layout(
        &mut self,
        id: ElementId,
        request: TextRequest<'_>,
    ) -> Result<Arc<TextLayout>, UiError> {
        let resource = self.texts.entry(id).or_default();
        if resource.prepared_revision == request.revision
            && resource.font_generation == self.generation
            && request.width == TextWidth::Available(resource.prepared_width)
            && let Some(layout) = &resource.prepared_layout
        {
            return Ok(layout.clone());
        }
        resource.layout(&mut self.fonts, request)
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
            .visual_neighbor(ast_position(position), right)
            .map(ui_position))
    }
    fn measure(&mut self, id: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok(self
            .texts
            .entry(id)
            .or_default()
            .layout(&mut self.fonts, request)?
            .size())
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
