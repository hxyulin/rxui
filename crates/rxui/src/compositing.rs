use super::*;
use astrelis::{
    Frame, Framebuffer, FramebufferOptions, TextureAlpha, TextureBinding, TextureBindingOptions,
    TextureFilter, wgpu,
};

/// Offscreen composition counters. Allocations/passes/composites are cumulative;
/// live layers/pixels describe currently cached resolved outputs (excluding MSAA storage).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerStats {
    /// Created/replaced layer storage, including resize, format or MSAA changes.
    pub allocations: u64,
    /// Transparent layer passes recorded; each visible isolated group records one.
    pub render_passes: u64,
    /// Layer image draws recorded into a parent layer or final UI pass.
    pub composites: u64,
    /// Cached layer targets across prepared placements.
    pub live_layers: usize,
    /// Pixels of cached resolved color outputs; format/MSAA determine actual memory.
    pub live_pixels: u64,
}
pub(super) struct LayerResource {
    target: Option<Framebuffer>,
    binding: TextureBinding,
}
pub(super) struct LayerSpec {
    id: ElementId,
    start: usize,
    end: usize,
    pub bounds: Option<[u32; 4]>,
    pub opacity: f32,
    pub binding: Option<TextureBinding>,
}
pub(super) struct CompositionPlan {
    key: [u64; 5],
    scale: f32,
    format: RenderFormat,
    nodes: Vec<ElementId>,
    groups: Vec<LayerSpec>,
    starts: HashMap<usize, usize>,
}
/// Borrowed UI painting capability inside UiPainter::compose. Its layer recordings
/// belong to the lent frame; use that frame for destination passes. This capability
/// cannot be retained after the callback. It owns no submission/presentation policy.
pub struct ComposedUi<'a, T: View> {
    painter: &'a mut UiPainter,
    ui: &'a Ui<T>,
    plan: Option<&'a CompositionPlan>,
    scale: f32,
}
impl<T: View> ComposedUi<'_, T> {
    /// Paints into a pass from the lent frame, preserving caller viewport/scissor.
    /// With layers, its attachment format must match UiPainter::prepare's format.
    /// Repeated calls can paint the prepared composition into several compatible
    /// passes in this recording. Opacity zero does not disable input or semantics.
    pub fn paint(&mut self, pass: &mut RenderPass<'_>) -> Result<(), UiError> {
        if self.plan.is_some_and(|p| p.format != pass.render_format()) {
            return Err(UiError::InvalidGeometry);
        }
        self.painter.paint_scope(
            self.ui,
            pass,
            self.scale,
            PaintScope {
                plan: self.plan,
                ..Default::default()
            },
        )
    }
}
#[derive(Default)]
pub(super) struct PaintScope<'a> {
    pub shift: [f32; 2],
    pub plan: Option<&'a CompositionPlan>,
    pub group: Option<usize>,
}
pub(super) struct PaintCursor<'a> {
    nodes: &'a [ElementId],
    plan: Option<&'a CompositionPlan>,
    index: usize,
    end: usize,
    own_group: Option<usize>,
}
impl<'a> PaintCursor<'a> {
    pub fn new<T: View>(
        ui: &'a Ui<T>,
        plan: Option<&'a CompositionPlan>,
        scope: Option<usize>,
    ) -> Self {
        let nodes = plan.map_or_else(|| ui.painting_ids(), |p| p.nodes.as_slice());
        let (index, end) = scope.map_or((0, nodes.len()), |group| {
            let spec = &plan.unwrap().groups[group];
            (spec.start, spec.end)
        });
        Self {
            nodes,
            plan,
            index,
            end,
            own_group: scope,
        }
    }
    pub fn next<'u, T: View>(
        &mut self,
        ui: &'u Ui<T>,
    ) -> Option<(crate::ElementInfo<'u>, Option<&'a LayerSpec>)> {
        while self.index < self.end {
            let index = self.index;
            self.index += 1;
            if let Some(element) = ui.element(self.nodes[index]) {
                if let Some(plan) = self.plan
                    && let Some(group) = plan.starts.get(&index)
                    && Some(*group) != self.own_group
                {
                    let spec = &plan.groups[*group];
                    self.index = spec.end;
                    return Some((element, Some(spec)));
                }
                return Some((element, None));
            }
        }
        None
    }
}
impl UiPainter {
    /// Current/cumulative offscreen composition work. No layers are allocated for
    /// ordinary opacity-one content, hidden/zero-opacity groups or empty clipped output.
    pub fn layer_stats(&self) -> LayerStats {
        let mut stats = self.layer_stats;
        stats.live_layers = self.layers.len();
        stats.live_pixels = self
            .layers
            .values()
            .filter_map(|r| r.target.as_ref())
            .map(|t| u64::from(t.size()[0]) * u64::from(t.size()[1]))
            .sum();
        stats
    }
    /// Records isolated opacity groups, then lends the frame and a UI painter to
    /// the callback for caller-selected destination passes. Call prepare beforehand.
    /// The callback must paint using passes from this frame. The host still chooses
    /// clears/loads, viewport/scissor and finish/presentation. Dropping the frame
    /// submits nothing; the next compose records all layers again, avoiding stale output.
    /// This is an ordinary direct painting scope when no opacity groups exist.
    ///
    /// ```no_run
    /// # use rxui::{Ui, UiPainter, UiError, View, astrelis::{Frame, wgpu}};
    /// fn draw<T: View>(painter: &mut UiPainter, ui: &Ui<T>, frame: &mut Frame<'_, '_>) -> Result<(), UiError> {
    ///     painter.compose(ui, frame, 1., |frame, ui| {
    ///         let mut pass = frame.render_pass().clear_color(wgpu::Color::BLACK).begin()?;
    ///         ui.paint(&mut pass)
    ///     })
    /// }
    /// ```
    pub fn compose<'target, 'window, T: View, R>(
        &mut self,
        ui: &Ui<T>,
        frame: &mut Frame<'target, 'window>,
        scale: f32,
        draw: impl FnOnce(&mut Frame<'target, 'window>, &mut ComposedUi<'_, T>) -> Result<R, UiError>,
    ) -> Result<R, UiError> {
        profiling::scope!("rxui::compose_layers");
        self.validate_resources(ui, scale)?;
        let plan = if ui.needs_composition() {
            let plan = self
                .compositions
                .get(&ui.tree_id())
                .cloned()
                .ok_or(UiError::InvalidGeometry)?;
            if plan.key != ui.composition_key() || plan.scale != scale {
                return Err(UiError::InvalidGeometry);
            }
            Some(plan)
        } else {
            None
        };
        if let Some(plan) = &plan {
            // Children complete before parent sampling, in one encoder/submission.
            for (group, spec) in plan.groups.iter().enumerate().rev() {
                if spec.bounds.is_none() {
                    continue;
                }
                let mut target = self
                    .layers
                    .get_mut(&spec.id)
                    .and_then(|r| r.target.take())
                    .ok_or(UiError::InvalidGeometry)?;
                let result = (|| {
                    let bounds = spec.bounds.unwrap();
                    let mut pass = frame
                        .render_to(&mut target)
                        .clear_color(wgpu::Color::TRANSPARENT)
                        .begin()?;
                    self.paint_scope(
                        ui,
                        &mut pass,
                        scale,
                        PaintScope {
                            shift: [-(bounds[0] as f32), -(bounds[1] as f32)],
                            plan: Some(plan),
                            group: Some(group),
                        },
                    )
                })();
                self.layers.get_mut(&spec.id).unwrap().target = Some(target);
                result?;
                self.layer_stats.render_passes += 1;
            }
        }
        draw(
            frame,
            &mut ComposedUi {
                painter: self,
                ui,
                plan: plan.as_deref(),
                scale,
            },
        )
    }
    pub(super) fn prepare_composition<T: View>(
        &mut self,
        ui: &Ui<T>,
        format: &RenderFormat,
        scale: f32,
    ) -> Result<(), UiError> {
        if !ui.needs_composition() {
            self.layers.retain(|id, _| !ui.owns_element(*id));
            self.compositions.remove(&ui.tree_id());
            return Ok(());
        }
        // A cached plan remains valid across blink/selection/pointer state changes;
        // reserve control boxes so those decorations cannot extend layer bounds.
        if self.compositions.get(&ui.tree_id()).is_some_and(|p| {
            p.key == ui.composition_key() && p.scale == scale && p.format == *format
        }) {
            return Ok(());
        }
        let mut plan = self.composition_plan(ui, format, scale)?;
        let layer_format = format.layer()?;
        let layer_color = layer_format.colors[0].expect("single layer color");
        let mut active = HashSet::new();
        for group in &mut plan.groups {
            let Some(bounds) = group.bounds else {
                continue;
            };
            active.insert(group.id);
            let size = [bounds[2], bounds[3]];
            let changed = self
                .layers
                .get(&group.id)
                .and_then(|r| r.target.as_ref())
                .is_none_or(|t| {
                    t.size() != size
                        || t.format() != layer_color
                        || t.sample_count() != format.sample_count
                });
            if changed {
                let target = self.graphics.create_framebuffer(
                    FramebufferOptions::new(size[0], size[1])
                        .format(layer_color)
                        .sample_count(format.sample_count),
                )?;
                let binding = self
                    .painter
                    .textures()
                    .create_sampled_binding_with_options(
                        &target.sampled_color(),
                        TextureBindingOptions::new()
                            .alpha(TextureAlpha::Premultiplied)
                            .filter(TextureFilter::Nearest),
                    )?;
                self.layers.insert(
                    group.id,
                    LayerResource {
                        target: Some(target),
                        binding,
                    },
                );
                self.layer_stats.allocations += 1;
            }
            let binding = &self.layers[&group.id].binding;
            self.painter.prepare_image(binding, format)?;
            self.painter.prepare_image(binding, &layer_format)?;
            group.binding = Some(binding.clone());
        }
        self.layers
            .retain(|id, _| !ui.owns_element(*id) || active.contains(id));
        if !active.is_empty() {
            self.painter.prepare(&layer_format)?;
            let mut seen = HashSet::new();
            for e in ui.elements() {
                if let Some(image) = e.image {
                    let key = image_key(&image);
                    if seen.insert(key)
                        && let Some(binding) = self.image_bindings.get(&key)
                    {
                        self.painter.prepare_image(binding, &layer_format)?;
                    }
                }
            }
        }
        self.compositions.insert(ui.tree_id(), Arc::new(plan));
        Ok(())
    }
    fn composition_plan<T: View>(
        &self,
        ui: &Ui<T>,
        format: &RenderFormat,
        scale: f32,
    ) -> Result<CompositionPlan, UiError> {
        let elements: Vec<_> = ui.elements().collect();
        let len = elements.len();
        let indices: HashMap<_, _> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        let parents: Vec<_> = elements
            .iter()
            .map(|e| {
                if ui.is_overlay(e.id) {
                    None
                } else {
                    e.parent.and_then(|p| indices.get(&p).copied())
                }
            })
            .collect();
        let mut ends = vec![len; len];
        let mut stack = Vec::new();
        for (i, parent) in parents.iter().enumerate() {
            while stack.last() != parent.as_ref() && !stack.is_empty() {
                ends[stack.pop().unwrap()] = i;
            }
            stack.push(i);
        }
        let mut visual: Vec<_> = elements.iter().map(|e| self.visual_bounds(e)).collect();
        for i in (0..len).rev() {
            if elements[i].opacity == 0. {
                visual[i] = None;
            }
            if let Some(parent) = parents[i] {
                visual[parent] = union(visual[parent], visual[i]);
            }
        }
        let mut groups = Vec::new();
        let mut starts = HashMap::new();
        let mut i = 0;
        while i < len {
            let e = &elements[i];
            if e.opacity < 1. {
                let bounds = visual[i]
                    .map(|b| pixel_bounds(b, scale))
                    .transpose()?
                    .flatten();
                starts.insert(i, groups.len());
                groups.push(LayerSpec {
                    id: e.id,
                    start: i,
                    end: ends[i],
                    bounds,
                    opacity: e.opacity,
                    binding: None,
                });
                if e.opacity == 0. || bounds.is_none() {
                    i = ends[i];
                    continue;
                }
            }
            i += 1;
        }
        Ok(CompositionPlan {
            key: ui.composition_key(),
            scale,
            format: format.clone(),
            nodes: elements.iter().map(|e| e.id).collect(),
            groups,
            starts,
        })
    }
    fn visual_bounds(&self, e: &crate::ElementInfo<'_>) -> Option<Bounds> {
        let mut bounds = None;
        if e.focusable
            || e.paint.background.is_some()
            || e.paint.border_color.is_some()
            || matches!(
                e.kind,
                crate::ElementType::Button | crate::ElementType::TextInput
            )
        {
            bounds = nonempty(e.bounds.intersection(e.clip_bounds));
        }
        if let Some(shadow) = e.paint.shadow
            && e.bounds.width > 0.
            && e.bounds.height > 0.
        {
            bounds = union(
                bounds,
                nonempty(shadow.extent(e.bounds).intersection(e.clip_bounds)),
            );
        }
        if let Some(image) = &e.image
            && image.source.pixel_size().is_some()
        {
            bounds = union(
                bounds,
                nonempty(
                    image
                        .destination
                        .intersection(e.content_bounds)
                        .intersection(e.clip_bounds),
                ),
            );
        }
        if e.editing.is_some() {
            bounds = union(
                bounds,
                nonempty(e.content_bounds.intersection(e.clip_bounds)),
            );
        } else if let Some(ink) = self
            .texts
            .get(&e.id)
            .and_then(|t| t.prepared.as_ref())
            .and_then(|p| p.ink_bounds())
        {
            bounds = union(
                bounds,
                nonempty(
                    Bounds {
                        x: e.content_bounds.x + ink.x,
                        y: e.content_bounds.y + ink.y,
                        width: ink.width,
                        height: ink.height,
                    }
                    .intersection(e.clip_bounds),
                ),
            );
        }
        bounds
    }
}
fn nonempty(b: Bounds) -> Option<Bounds> {
    (b.width > 0. && b.height > 0.).then_some(b)
}
fn union(a: Option<Bounds>, b: Option<Bounds>) -> Option<Bounds> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            Some(Bounds {
                x,
                y,
                width: (a.x + a.width).max(b.x + b.width) - x,
                height: (a.y + a.height).max(b.y + b.height) - y,
            })
        }
        (a, b) => a.or(b),
    }
}
fn pixel_bounds(b: Bounds, scale: f32) -> Result<Option<[u32; 4]>, UiError> {
    let edges = [b.x, b.y, b.x + b.width, b.y + b.height].map(|v| v * scale);
    if edges
        .iter()
        .any(|v| !v.is_finite() || *v >= u32::MAX as f32)
    {
        return Err(UiError::InvalidGeometry);
    }
    let [x, y, right, bottom] = edges;
    let x = x.floor().max(0.) as u32;
    let y = y.floor().max(0.) as u32;
    let right = right.ceil().max(0.) as u32;
    let bottom = bottom.ceil().max(0.) as u32;
    Ok((right > x && bottom > y).then_some([
        x,
        y,
        right.saturating_sub(x),
        bottom.saturating_sub(y),
    ]))
}
