use std::{
    ops::Range,
    sync::{Arc, atomic::AtomicBool},
};

use crate::{Error, Framebuffer, depth_stencil::DepthStencilAttachment, frame::AttachmentWrites};

#[derive(Debug)]
enum Initialization<'frame> {
    Surface(&'frame Arc<AtomicBool>),
    Framebuffer(&'frame Arc<AtomicBool>),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ManagedColorAttachment<'view> {
    pub(crate) view: &'view wgpu::TextureView,
    pub(crate) resolve_target: Option<&'view wgpu::TextureView>,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) size: [u32; 2],
    pub(crate) sample_count: u32,
    pub(crate) resolved_state: Option<&'view Arc<AtomicBool>>,
}

#[derive(Debug)]
pub(crate) struct PassOptions<'label> {
    pub(crate) label: Option<&'label str>,
    pub(crate) load: wgpu::LoadOp<wgpu::Color>,
    store: wgpu::StoreOp,
    resolve: bool,
    viewport: Option<[f32; 6]>,
    scissor: Option<[u32; 4]>,
    depth_stencil: Option<&'label DepthStencilAttachment>,
    depth_ops: Option<wgpu::Operations<f32>>,
    stencil_ops: Option<wgpu::Operations<u32>>,
    depth_requested: bool,
    stencil_requested: bool,
    stencil_reference: u32,
}

impl Default for PassOptions<'_> {
    fn default() -> Self {
        Self {
            label: None,
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
            resolve: true,
            viewport: None,
            scissor: None,
            depth_stencil: None,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0),
                store: wgpu::StoreOp::Store,
            }),
            depth_requested: false,
            stencil_requested: false,
            stencil_reference: 0,
        }
    }
}

/// Configuration for a frame's next color pass, before command recording begins.
///
/// Created by [`crate::Frame::render_pass`] or [`crate::Frame::render_to`]. Defaults clear to transparent black,
/// store the results, and use the full attachment for viewport and scissor. These
/// defaults are the same for every pass; use [`Self::load_color`] to preserve earlier work.
/// Attached depth defaults to clear 1.0 and stencil to clear 0, both stored.
/// Their load/store operations are independent of color and each other.
/// No GPU commands are recorded until [`Self::begin`]. Dropping the builder leaves
/// the frame unchanged. Configuration borrows the frame; beginning a pass records attachment metadata.
#[derive(Debug)]
#[must_use = "call begin to record a pass; dropping this builder does nothing"]
pub struct RenderPassBuilder<'frame> {
    encoder: &'frame mut wgpu::CommandEncoder,
    attachment: Result<ManagedColorAttachment<'frame>, Error>,
    graphics: &'frame crate::GraphicsContext,
    initialization: Initialization<'frame>,
    options: PassOptions<'frame>,
    writes: Option<&'frame mut AttachmentWrites>,
    uploads: Option<&'frame mut crate::uploads::DrawUploads>,
}

impl<'frame> RenderPassBuilder<'frame> {
    pub(crate) fn new(
        encoder: &'frame mut wgpu::CommandEncoder,
        graphics: &'frame crate::GraphicsContext,
        attachment: ManagedColorAttachment<'frame>,
        initialized: &'frame Arc<AtomicBool>,
    ) -> Self {
        Self {
            encoder,
            attachment: Ok(attachment),
            graphics,
            initialization: Initialization::Surface(initialized),
            options: PassOptions::default(),
            writes: None,
            uploads: None,
        }
    }

    pub(crate) fn for_framebuffer(
        encoder: &'frame mut wgpu::CommandEncoder,
        graphics: &'frame crate::GraphicsContext,
        framebuffer: &'frame Framebuffer,
        writes: &'frame mut AttachmentWrites,
    ) -> Self {
        let attachment = if !graphics.same_device(&framebuffer.graphics) {
            Err(Error::DeviceMismatch)
        } else {
            framebuffer.attachment()
        };
        Self {
            encoder,
            attachment,
            graphics,
            initialization: Initialization::Framebuffer(if framebuffer.sample_count() > 1 {
                &framebuffer.samples_initialized
            } else {
                &framebuffer.initialized
            }),
            options: PassOptions {
                depth_stencil: framebuffer.depth_stencil.as_ref(),
                ..Default::default()
            },
            writes: Some(writes),
            uploads: None,
        }
    }

    pub(crate) fn with_uploads(mut self, uploads: &'frame mut crate::uploads::DrawUploads) -> Self {
        self.uploads = Some(uploads);
        self
    }

    pub(crate) fn with_depth_stencil(
        mut self,
        attachment: Option<&'frame DepthStencilAttachment>,
        writes: &'frame mut AttachmentWrites,
    ) -> Self {
        self.options.depth_stencil = attachment;
        self.writes = Some(writes);
        self
    }

    /// Clears depth to a finite value in zero to one and stores it for later passes.
    ///
    /// Defaults to 1.0 for conventional `Less` depth testing. Reversed-Z shaders
    /// can clear to 0.0 and use `Greater`. Requires an attachment with depth.
    pub fn clear_depth(mut self, depth: f32) -> Self {
        self.options.depth_requested = true;
        self.options.depth_ops = Some(wgpu::Operations {
            load: wgpu::LoadOp::Clear(depth),
            store: wgpu::StoreOp::Store,
        });
        self
    }

    /// Loads and stores initialized depth, independently of the color load operation.
    pub fn load_depth(mut self) -> Self {
        self.options.depth_requested = true;
        self.options.depth_ops = Some(wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
        });
        self
    }

    /// Clears stencil to an integer mask and stores it. Defaults to zero.
    pub fn clear_stencil(mut self, stencil: u32) -> Self {
        self.options.stencil_requested = true;
        self.options.stencil_ops = Some(wgpu::Operations {
            load: wgpu::LoadOp::Clear(stencil),
            store: wgpu::StoreOp::Store,
        });
        self
    }

    /// Loads and stores initialized stencil, independently of color and depth.
    pub fn load_stencil(mut self) -> Self {
        self.options.stencil_requested = true;
        self.options.stencil_ops = Some(wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
        });
        self
    }

    /// Sets raw depth load/store operations; `None` makes depth read-only.
    ///
    /// Loading or read-only access requires a stored clear in an earlier submitted
    /// recording or this frame. `Discard` invalidates depth for later wrapped loads.
    /// Materials in read-only passes must disable depth writes.
    pub fn depth_ops(mut self, ops: Option<wgpu::Operations<f32>>) -> Self {
        self.options.depth_requested = true;
        self.options.depth_ops = ops;
        self
    }

    /// Sets raw stencil load/store operations; `None` makes stencil read-only.
    ///
    /// Loading/read-only access requires initialized contents. `Discard` invalidates
    /// subsequent wrapped loads. Materials must not write a read-only stencil aspect.
    pub fn stencil_ops(mut self, ops: Option<wgpu::Operations<u32>>) -> Self {
        self.options.stencil_requested = true;
        self.options.stencil_ops = ops;
        self
    }

    /// Sets the reference used by stencil comparisons and `Replace` operations.
    ///
    /// Defaults to zero. This is per-pass dynamic state, not part of a material or
    /// pipeline cache key. Use the pass setter to change it between draws.
    pub fn stencil_reference(mut self, reference: u32) -> Self {
        self.options.stencil_reference = reference;
        self
    }

    /// Sets a debug label for the pass.
    pub fn label(mut self, label: &'frame str) -> Self {
        self.options.label = Some(label);
        self
    }

    /// Selects a clear operation using linear RGBA, replacing any previous load.
    pub fn clear_color(mut self, color: wgpu::Color) -> Self {
        self.options.load = wgpu::LoadOp::Clear(color);
        self
    }

    /// Selects loading existing contents, replacing any previous clear operation.
    ///
    /// A surface needs an earlier clear in this frame. A framebuffer may also
    /// load contents initialized in a previously submitted recording.
    pub fn load_color(mut self) -> Self {
        self.options.load = wgpu::LoadOp::Load;
        self
    }

    /// Loads color, depth, and stencil aspects that are present, preserving their contents.
    pub fn load_all(mut self) -> Self {
        self.options.load = wgpu::LoadOp::Load;
        if let Some(attachment) = self.options.depth_stencil {
            let format = attachment.texture.format();
            if format.has_depth_aspect() {
                self = self.load_depth();
            }
            if format.has_stencil_aspect() {
                self = self.load_stencil();
            }
        }
        self
    }

    /// Sets independent color load and store operations.
    /// With MSAA, these operations refer to multisampled storage, not the resolve output.
    pub fn color_ops(mut self, ops: wgpu::Operations<wgpu::Color>) -> Self {
        self.options.load = ops.load;
        self.options.store = ops.store;
        self
    }

    /// Enables or skips this pass's automatic MSAA resolve. Defaults to enabled.
    /// Retain samples with `Store` when a later pass loads them; resolve the last
    /// pass before sampling or presenting the output.
    pub fn resolve(mut self, enabled: bool) -> Self {
        self.options.resolve = enabled;
        self
    }

    /// Omits the target's depth/stencil attachment from this pass.
    pub fn without_depth_stencil(mut self) -> Self {
        self.options.depth_stencil = None;
        self.options.depth_requested = false;
        self.options.stencil_requested = false;
        self
    }

    /// Sets the initial viewport in physical pixels and its depth range.
    ///
    /// Parameters follow [`wgpu::RenderPass::set_viewport`]. The default depth range
    /// is zero to one. Validation occurs at [`Self::begin`].
    pub fn viewport(
        mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) -> Self {
        self.options.viewport = Some([x, y, width, height, min_depth, max_depth]);
        self
    }

    /// Sets the initial scissor rectangle in physical pixels.
    ///
    /// The rectangle must fit the attachment. Zero dimensions clip all drawing.
    /// Validation occurs at [`Self::begin`].
    pub fn scissor_rect(mut self, x: u32, y: u32, width: u32, height: u32) -> Self {
        self.options.scissor = Some([x, y, width, height]);
        self
    }

    /// Validates configuration and starts a scoped color pass.
    ///
    /// Store/resolve choices determine retained samples and output. A clear-only pass is valid.
    /// Dropping the returned pass ends recording; the frame still owns submission.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UninitializedFrame`] for an initial surface load,
    /// [`Error::UninitializedFramebuffer`] for loading unsubmitted framebuffer contents,
    /// [`Error::TargetSuspended`] for a zero-sized framebuffer,
    /// [`Error::DeviceMismatch`] for a framebuffer on another device,
    /// [`Error::InvalidClearColor`] for nonfinite clear components,
    /// [`Error::InvalidViewport`] for invalid viewport values, or
    /// [`Error::InvalidScissorRect`] for a rectangle outside the attachment.
    /// Depth/stencil configuration can return [`Error::MissingDepthAttachment`],
    /// [`Error::MissingStencilAttachment`], [`Error::InvalidClearDepth`],
    /// [`Error::UninitializedDepth`], or [`Error::UninitializedStencil`].
    /// Rejected configuration records no commands and leaves the frame unchanged.
    pub fn begin(mut self) -> Result<RenderPass<'frame>, Error> {
        let mut attachment = self.attachment?;
        if !self.options.resolve {
            attachment.resolve_target = None;
        }
        validate_depth_stencil_options(&self.options)?;
        if matches!(self.options.load, wgpu::LoadOp::Load) {
            match &self.initialization {
                Initialization::Surface(initialized)
                    if !self.writes.as_ref().unwrap().is_initialized(initialized) =>
                {
                    return Err(Error::UninitializedFrame);
                }
                Initialization::Framebuffer(state)
                    if !self.writes.as_ref().unwrap().is_initialized(state) =>
                {
                    return Err(Error::UninitializedFramebuffer);
                }
                _ => {}
            }
        }
        if let Some(depth_stencil) = self.options.depth_stencil {
            let writes = self.writes.as_ref().unwrap();
            let format = depth_stencil.texture.format();
            if format.has_depth_aspect()
                && needs_initialized(self.options.depth_ops)
                && !writes.is_initialized(&depth_stencil.depth_initialized)
            {
                return Err(Error::UninitializedDepth);
            }
            if format.has_stencil_aspect()
                && needs_initialized(self.options.stencil_ops)
                && !writes.is_initialized(&depth_stencil.stencil_initialized)
            {
                return Err(Error::UninitializedStencil);
            }
        }
        let loading = matches!(self.options.load, wgpu::LoadOp::Load);
        let stored = self.options.store == wgpu::StoreOp::Store;
        let depth_stencil = self.options.depth_stencil;
        let depth_ops = self.options.depth_ops;
        let stencil_ops = self.options.stencil_ops;
        let pass = RenderPass::new_with_uploads(
            self.encoder,
            self.graphics,
            attachment,
            self.options,
            self.uploads,
        )?;
        match self.initialization {
            Initialization::Surface(initialized) => {
                let writes = self.writes.as_mut().unwrap();
                if loading {
                    writes.require(initialized, crate::frame::AttachmentAspect::Surface);
                }
                writes.record(initialized, stored)
            }
            Initialization::Framebuffer(state) => {
                let writes = self.writes.as_mut().unwrap();
                if loading {
                    writes.require(state, crate::frame::AttachmentAspect::Color);
                }
                writes.record(state, stored)
            }
        }
        if attachment.resolve_target.is_some()
            && let Some(state) = attachment.resolved_state
        {
            self.writes.as_mut().unwrap().record(state, true);
        }
        if let Some(attachment) = depth_stencil {
            let writes = self.writes.as_mut().unwrap();
            let format = attachment.texture.format();
            if format.has_depth_aspect() && needs_initialized(depth_ops) {
                writes.require(
                    &attachment.depth_initialized,
                    crate::frame::AttachmentAspect::Depth,
                );
            }
            if format.has_stencil_aspect() && needs_initialized(stencil_ops) {
                writes.require(
                    &attachment.stencil_initialized,
                    crate::frame::AttachmentAspect::Stencil,
                );
            }
            if format.has_depth_aspect()
                && let Some(ops) = depth_ops
            {
                writes.record(
                    &attachment.depth_initialized,
                    ops.store == wgpu::StoreOp::Store,
                );
            }
            if format.has_stencil_aspect()
                && let Some(ops) = stencil_ops
            {
                writes.record(
                    &attachment.stencil_initialized,
                    ops.store == wgpu::StoreOp::Store,
                );
            }
        }
        Ok(pass)
    }
}

/// A scoped color render pass that accepts drawing from multiple renderers.
///
/// Created by [`RenderPassBuilder::begin`]. Dropping the pass ends command recording
/// for that pass; submission and presentation remain the frame's responsibility.
/// The pass borrows the frame, preventing another pass or encoder access until it ends.
/// Viewport, scissor, and dynamic stencil reference belong to this pass and
/// persist across mesh draws. Depth/stencil tests and writes belong to materials.
#[derive(Debug)]
#[must_use = "keep the pass in scope while recording draws"]
pub struct RenderPass<'frame> {
    inner: wgpu::RenderPass<'frame>,
    // A built-in instanced draw not yet recorded. A following draw of the same
    // geometry whose instances continue its range extends it instead of
    // recording another command. Every other command flushes it first, so
    // recorded order and state are exactly those of the calls.
    pending: Option<PendingDraw>,
    #[cfg(test)]
    pub(crate) recorded_draws: u32,
    graphics: &'frame crate::GraphicsContext,
    format: Option<wgpu::TextureFormat>,
    size: [u32; 2],
    sample_count: u32,
    viewport: [f32; 6],
    scissor: [u32; 4],
    depth_stencil_format: Option<wgpu::TextureFormat>,
    depth_read_only: bool,
    stencil_read_only: bool,
    stencil_reference: u32,
    color_texture: Option<wgpu::Texture>,
    resolve_texture: Option<wgpu::Texture>,
    raster_dirty: bool,
    bound_pipeline: Option<wgpu::RenderPipeline>,
    vertex_zero: Option<(wgpu::Buffer, std::ops::Range<u64>)>,
    vertex_other: Vec<Option<(wgpu::Buffer, std::ops::Range<u64>)>>,
    bound_index: Option<(wgpu::Buffer, std::ops::Range<u64>, wgpu::IndexFormat)>,
    group_zero: Option<wgpu::BindGroup>,
    group_one: Option<wgpu::BindGroup>,
    uploads: Option<&'frame mut crate::uploads::DrawUploads>,
    color_formats: Vec<Option<wgpu::TextureFormat>>,
    additional_colors: Vec<wgpu::Texture>,
    rounded_clips: Vec<crate::RoundedClip>,
    clip_stack_upload: Option<wgpu::BindGroup>,
    // The active clip's record for the current viewport size, uploaded on first use.
    clip_upload: Option<(wgpu::Buffer, std::ops::Range<u64>)>,
}

impl<'frame> RenderPass<'frame> {
    #[cfg(test)]
    pub(crate) fn new(
        encoder: &'frame mut wgpu::CommandEncoder,
        graphics: &'frame crate::GraphicsContext,
        attachment: ManagedColorAttachment<'_>,
        options: PassOptions<'_>,
    ) -> Result<Self, Error> {
        Self::new_with_uploads(encoder, graphics, attachment, options, None)
    }

    pub(crate) fn new_with_uploads(
        encoder: &'frame mut wgpu::CommandEncoder,
        graphics: &'frame crate::GraphicsContext,
        attachment: ManagedColorAttachment<'_>,
        options: PassOptions<'_>,
        uploads: Option<&'frame mut crate::uploads::DrawUploads>,
    ) -> Result<Self, Error> {
        let device = graphics.device();
        validate_depth_stencil_options(&options)?;
        let ManagedColorAttachment {
            view,
            resolve_target,
            format,
            size,
            sample_count,
            ..
        } = attachment;
        if let wgpu::LoadOp::Clear(color) = options.load
            && ![color.r, color.g, color.b, color.a]
                .iter()
                .all(|value| value.is_finite())
        {
            return Err(Error::InvalidClearColor);
        }
        let viewport =
            options
                .viewport
                .unwrap_or([0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0]);
        let scissor = options.scissor.unwrap_or([0, 0, size[0], size[1]]);
        validate_viewport(device, viewport)?;
        validate_scissor(size, scissor)?;
        let attachments = [Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target,
            ops: wgpu::Operations {
                load: options.load,
                store: options.store,
            },
        })];
        let depth_stencil_format = options
            .depth_stencil
            .map(|attachment| attachment.texture.format());
        let depth_stencil_attachment =
            options
                .depth_stencil
                .map(|attachment| wgpu::RenderPassDepthStencilAttachment {
                    view: &attachment.view,
                    depth_ops: depth_stencil_format
                        .filter(|format| format.has_depth_aspect())
                        .and(options.depth_ops),
                    stencil_ops: depth_stencil_format
                        .filter(|format| format.has_stencil_aspect())
                        .and(options.stencil_ops),
                });
        let inner = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: options.label,
            color_attachments: &attachments,
            depth_stencil_attachment,
            ..Default::default()
        });
        let mut pass = Self {
            inner,
            pending: None,
            #[cfg(test)]
            recorded_draws: 0,
            graphics,
            format: Some(format),
            size,
            sample_count,
            viewport,
            scissor,
            depth_stencil_format,
            depth_read_only: options.depth_ops.is_none(),
            stencil_read_only: options.stencil_ops.is_none(),
            stencil_reference: options.stencil_reference,
            color_texture: Some(view.texture().clone()),
            resolve_texture: resolve_target.map(|view| view.texture().clone()),
            raster_dirty: true,
            bound_pipeline: None,
            vertex_zero: None,
            vertex_other: Vec::new(),
            bound_index: None,
            group_zero: None,
            group_one: None,
            uploads,
            color_formats: vec![Some(format)],
            additional_colors: Vec::new(),
            rounded_clips: Vec::new(),
            clip_stack_upload: None,
            clip_upload: None,
        };
        pass.apply_raster_state();
        Ok(pass)
    }

    pub(crate) fn from_descriptor(
        encoder: &'frame mut wgpu::CommandEncoder,
        graphics: &'frame crate::GraphicsContext,
        writes: &mut AttachmentWrites,
        uploads: &'frame mut crate::uploads::DrawUploads,
        descriptor: &crate::RenderPassDescriptor<'_>,
    ) -> Result<Self, Error> {
        let first = descriptor.colors.iter().flatten().next();
        let size = first
            .map(|a| a.size)
            .or_else(|| descriptor.depth_stencil.map(|a| a.size))
            .ok_or(Error::InvalidAttachments)?;
        let sample_count = first
            .map(|a| a.view.texture().sample_count())
            .or_else(|| {
                descriptor
                    .depth_stencil
                    .map(|a| a.view.texture().sample_count())
            })
            .unwrap();
        if size.contains(&0)
            || descriptor.colors.len() > graphics.device().limits().max_color_attachments as usize
        {
            return Err(Error::InvalidAttachments);
        }
        for a in descriptor.colors.iter().flatten() {
            if a.owner.as_ref().is_some_and(|g| !graphics.same_device(g)) {
                return Err(Error::DeviceMismatch);
            }
            if a.size != size
                || a.view.texture().sample_count() != sample_count
                || a.format.is_depth_stencil_format()
            {
                return Err(Error::InvalidAttachments);
            }
            if let wgpu::LoadOp::Clear(c) = a.ops.load
                && ![c.r, c.g, c.b, c.a].iter().all(|v| v.is_finite())
            {
                return Err(Error::InvalidClearColor);
            }
            if matches!(a.ops.load, wgpu::LoadOp::Load)
                && a.state.as_ref().is_some_and(|s| !writes.is_initialized(s))
            {
                return Err(Error::UninitializedFramebuffer);
            }
        }
        if let Some(a) = descriptor.depth_stencil {
            if a.owner.as_ref().is_some_and(|g| !graphics.same_device(g)) {
                return Err(Error::DeviceMismatch);
            }
            if a.size != size
                || a.view.texture().sample_count() != sample_count
                || !a.format.is_depth_stencil_format()
            {
                return Err(Error::InvalidAttachments);
            }
            if !a.format.has_depth_aspect() && a.depth_ops.is_some() {
                return Err(Error::MissingDepthAttachment);
            }
            if !a.format.has_stencil_aspect() && a.stencil_ops.is_some() {
                return Err(Error::MissingStencilAttachment);
            }
            if let Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(depth),
                ..
            }) = a.depth_ops
                && (!depth.is_finite() || !(0.0..=1.0).contains(&depth))
            {
                return Err(Error::InvalidClearDepth);
            }
            if a.format.has_depth_aspect()
                && needs_initialized(a.depth_ops)
                && a.depth_state
                    .as_ref()
                    .is_some_and(|s| !writes.is_initialized(s))
            {
                return Err(Error::UninitializedDepth);
            }
            if a.format.has_stencil_aspect()
                && needs_initialized(a.stencil_ops)
                && a.stencil_state
                    .as_ref()
                    .is_some_and(|s| !writes.is_initialized(s))
            {
                return Err(Error::UninitializedStencil);
            }
        }
        let viewport =
            descriptor
                .viewport
                .unwrap_or([0., 0., size[0] as f32, size[1] as f32, 0., 1.]);
        let scissor = descriptor.scissor.unwrap_or([0, 0, size[0], size[1]]);
        validate_viewport(graphics.device(), viewport)?;
        validate_scissor(size, scissor)?;
        let colors: Vec<_> = descriptor
            .colors
            .iter()
            .map(|a| {
                a.as_ref().map(|a| wgpu::RenderPassColorAttachment {
                    view: &a.view,
                    resolve_target: a.resolve_target(),
                    depth_slice: a.depth_slice,
                    ops: a.ops,
                })
            })
            .collect();
        let depth = descriptor
            .depth_stencil
            .map(|a| wgpu::RenderPassDepthStencilAttachment {
                view: &a.view,
                depth_ops: a.depth_ops,
                stencil_ops: a.stencil_ops,
            });
        let inner = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: descriptor.label,
            color_attachments: &colors,
            depth_stencil_attachment: depth,
            timestamp_writes: descriptor.timestamp_writes.clone(),
            occlusion_query_set: descriptor.occlusion_query_set,
            ..Default::default()
        });
        for a in descriptor.colors.iter().flatten() {
            if let Some(state) = &a.state {
                if matches!(a.ops.load, wgpu::LoadOp::Load) {
                    writes.require(state, crate::frame::AttachmentAspect::Color);
                }
                writes.record(state, a.ops.store == wgpu::StoreOp::Store);
            }
            if a.resolve_target().is_some()
                && let Some(state) = &a.resolved_state
            {
                writes.record(state, true);
            }
        }
        if let Some(a) = descriptor.depth_stencil {
            if a.format.has_depth_aspect()
                && needs_initialized(a.depth_ops)
                && let Some(s) = &a.depth_state
            {
                writes.require(s, crate::frame::AttachmentAspect::Depth);
            }
            if a.format.has_stencil_aspect()
                && needs_initialized(a.stencil_ops)
                && let Some(s) = &a.stencil_state
            {
                writes.require(s, crate::frame::AttachmentAspect::Stencil);
            }
            if let (Some(state), Some(ops)) = (&a.depth_state, a.depth_ops) {
                writes.record(state, ops.store == wgpu::StoreOp::Store);
            }
            if let (Some(state), Some(ops)) = (&a.stencil_state, a.stencil_ops) {
                writes.record(state, ops.store == wgpu::StoreOp::Store);
            }
        }
        let mut pass = Self {
            inner,
            pending: None,
            #[cfg(test)]
            recorded_draws: 0,
            graphics,
            format: descriptor
                .colors
                .first()
                .and_then(|a| a.as_ref())
                .map(|a| a.format),
            size,
            sample_count,
            viewport,
            scissor,
            depth_stencil_format: descriptor.depth_stencil.map(|a| a.format),
            depth_read_only: descriptor
                .depth_stencil
                .is_some_and(|a| a.depth_ops.is_none()),
            stencil_read_only: descriptor
                .depth_stencil
                .is_some_and(|a| a.stencil_ops.is_none()),
            stencil_reference: descriptor.stencil_reference,
            color_texture: first.map(|a| a.view.texture().clone()),
            resolve_texture: first
                .and_then(|a| a.resolve_target())
                .map(|v| v.texture().clone()),
            raster_dirty: true,
            bound_pipeline: None,
            vertex_zero: None,
            vertex_other: Vec::new(),
            bound_index: None,
            group_zero: None,
            group_one: None,
            uploads: Some(uploads),
            color_formats: descriptor
                .colors
                .iter()
                .map(|a| a.as_ref().map(|a| a.format))
                .collect(),
            additional_colors: descriptor
                .colors
                .iter()
                .flatten()
                .flat_map(|a| {
                    std::iter::once(a.view.texture().clone())
                        .chain(a.resolve_target().map(|v| v.texture().clone()))
                })
                .chain(descriptor.depth_stencil.map(|a| a.view.texture().clone()))
                .collect(),
            rounded_clips: Vec::new(),
            clip_stack_upload: None,
            clip_upload: None,
        };
        pass.apply_raster_state();
        Ok(pass)
    }

    /// Returns all attachment formats and the sample count for pipeline preparation.
    pub fn render_format(&self) -> crate::RenderFormat {
        crate::RenderFormat {
            colors: self.color_formats.clone(),
            depth_stencil: self.depth_stencil_format,
            sample_count: self.sample_count,
        }
    }

    pub(crate) fn single_color_format(&self) -> Result<wgpu::TextureFormat, Error> {
        if self.color_formats.len() != 1 {
            return Err(Error::ExpectedSingleColor);
        }
        self.format.ok_or(Error::ExpectedSingleColor)
    }

    pub(crate) fn upload_instances(
        &mut self,
        bytes: &[u8],
        alignment: u64,
    ) -> (wgpu::Buffer, std::ops::Range<u64>) {
        self.uploads
            .as_mut()
            .expect("instance uploads require a frame-owned pass")
            .append(self.graphics, bytes, alignment)
    }

    /// Appends `stride`-sized instance records, binds their whole upload page at
    /// `slot`, and returns the index of the first record. Consecutive draws that
    /// land in the same page keep one binding and differ only in first instance.
    pub(crate) fn bind_instances(&mut self, slot: u32, bytes: &[u8], stride: u64) -> u32 {
        let (buffer, range) = self.upload_instances(bytes, stride);
        self.set_vertex_buffer(slot, &buffer, 0..buffer.size());
        (range.start / stride) as u32
    }

    pub(crate) fn viewport_size(&self) -> [f32; 2] {
        [self.viewport[2], self.viewport[3]]
    }
    pub(crate) fn retain_resource_ref<T: crate::uploads::ResourceLease + 'static>(
        &mut self,
        resource: &Arc<T>,
    ) {
        self.uploads
            .as_mut()
            .expect("resource leases require a frame-owned pass")
            .retain_ref(resource);
    }
    pub(crate) fn retain_resource(
        &mut self,
        resource: std::sync::Arc<dyn crate::uploads::ResourceLease>,
    ) {
        self.uploads
            .as_mut()
            .expect("resource leases require a frame-owned pass")
            .retain(resource);
    }

    /// Binds application resources without invalidating tracked raster state.
    /// Dynamic offsets follow wgpu alignment and layout validation.
    pub fn set_bind_group(
        &mut self,
        index: u32,
        group: &wgpu::BindGroup,
        offsets: &[wgpu::DynamicOffset],
    ) {
        let tracked = match index {
            0 => Some(&mut self.group_zero),
            1 => Some(&mut self.group_one),
            _ => None,
        };
        if let Some(tracked) = tracked {
            if offsets.is_empty() {
                if tracked.as_ref() == Some(group) {
                    return;
                }
                *tracked = Some(group.clone());
            } else {
                *tracked = None;
            }
        }
        self.flush();
        self.inner.set_bind_group(index, group, offsets);
    }

    pub(crate) fn same_device(&self, graphics: &crate::GraphicsContext) -> bool {
        self.graphics.same_device(graphics)
    }

    pub(crate) fn uses_color_texture(&self, texture: &wgpu::Texture) -> bool {
        self.color_texture.as_ref() == Some(texture)
            || self.resolve_texture.as_ref() == Some(texture)
            || self.additional_colors.contains(texture)
    }

    /// Sets the viewport for subsequent draws, in physical pixels.
    ///
    /// Values must be finite, dimensions nonnegative and within device limits,
    /// positions within wgpu's device viewport bounds, and `0 <= min_depth <=
    /// max_depth <= 1`. A viewport may extend beyond the attachment; scissoring
    /// limits rasterization to its bounds. Zero dimensions produce no fragments.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidViewport`] without changing state for invalid values.
    pub fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) -> Result<(), Error> {
        let viewport = [x, y, width, height, min_depth, max_depth];
        validate_viewport(self.graphics.device(), viewport)?;
        if self.viewport[..4] != viewport[..4] {
            self.clip_stack_upload = None;
            self.clip_upload = None;
        }
        self.raster_dirty |= self.viewport != viewport;
        self.viewport = viewport;
        self.apply_raster_state();
        Ok(())
    }

    /// Sets the scissor rectangle for subsequent draws, in physical pixels.
    ///
    /// The rectangle must fit the attachment; zero dimensions clip all drawing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidScissorRect`] without changing state for invalid bounds.
    pub fn set_scissor_rect(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), Error> {
        let scissor = [x, y, width, height];
        validate_scissor(self.size, scissor)?;
        self.raster_dirty |= self.scissor != scissor;
        self.scissor = scissor;
        self.apply_raster_state();
        Ok(())
    }

    /// Selects an anti-aliased rounded-rectangle clip for subsequent built-in 2D draws,
    /// replacing the entire stack, or removes it with `None`. Geometry is relative to the current viewport, like
    /// pixel-space draws. The scissor is unchanged; see [`crate::RoundedClip`] for
    /// which renderers evaluate the clip. Changing it creates no pipeline; the first
    /// draw under a new clip or viewport uploads the stack. Unclipped draws upload nothing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidClip`] without changing state for nonfinite or negative
    /// geometry/radii or a singular transform.
    pub fn set_rounded_clip(&mut self, clip: Option<crate::RoundedClip>) -> Result<(), Error> {
        if let Some(clip) = &clip {
            clip.validate()?;
        }
        self.set_rounded_clips(clip.as_slice())
    }

    /// Returns the innermost rounded clip, if any. Use [`Self::rounded_clips`]
    /// to inspect the complete intersection.
    pub fn rounded_clip(&self) -> Option<crate::RoundedClip> {
        self.rounded_clips.last().copied()
    }

    /// Returns all active rounded clips, from outermost to innermost.
    pub fn rounded_clips(&self) -> &[crate::RoundedClip] {
        &self.rounded_clips
    }

    /// Replaces the analytic intersection. Invalid clips or a stack longer than
    /// the device's [`crate::GraphicsContext::rounded_clip_capacity`] return an error without changing state.
    /// Does not alter the scissor, attachments, stencil policy, or draw order.
    pub fn set_rounded_clips(&mut self, clips: &[crate::RoundedClip]) -> Result<(), Error> {
        if clips.len() > self.graphics.rounded_clip_capacity() {
            return Err(Error::ClipStackFull);
        }
        for clip in clips {
            clip.validate()?;
        }
        if self.rounded_clips != clips {
            self.rounded_clips.clear();
            self.rounded_clips.extend_from_slice(clips);
            self.clip_upload = None;
            self.clip_stack_upload = None;
        }
        Ok(())
    }

    pub(crate) fn push_rounded_clip(&mut self, clip: crate::RoundedClip) -> Result<(), Error> {
        if self.rounded_clips.len() >= self.graphics.rounded_clip_capacity() {
            return Err(Error::ClipStackFull);
        }
        clip.validate()?;
        self.rounded_clips.push(clip);
        self.clip_upload = None;
        self.clip_stack_upload = None;
        Ok(())
    }

    /// Binds the complete analytic intersection at group one for custom shaders
    /// using [`crate::GraphicsContext::rounded_clip_shader`] and the context's rounded clip layout.
    /// Call after changing clips/viewport or interleaving other work. Custom work
    /// owns its pipeline and other bindings; raw commands do not apply this
    /// coverage automatically. No clips binds shared immutable data with no upload.
    /// At zero clip capacity this does nothing; omit group one from custom pipelines.
    pub fn bind_rounded_clips(&mut self) {
        if self.graphics.rounded_clip_capacity() == 0 {
            return;
        }
        let group = if self.rounded_clips.is_empty() {
            self.graphics.no_clip_stack.get_or_init(|| {
                use wgpu::util::DeviceExt;
                let capacity = self.graphics.rounded_clip_capacity();
                let buffer =
                    self.graphics
                        .device()
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("RXUI drawing disabled rounded stack"),
                            contents: &crate::clip::stack_bytes(&[], [0.; 6], capacity),
                            usage: wgpu::BufferUsages::UNIFORM,
                        });
                self.graphics.clip_group(Some((&buffer, 0)))
            })
        } else {
            if self.clip_stack_upload.is_none() {
                let bytes = crate::clip::stack_bytes(
                    &self.rounded_clips,
                    self.viewport,
                    self.graphics.rounded_clip_capacity(),
                );
                let alignment = self
                    .graphics
                    .device()
                    .limits()
                    .min_uniform_buffer_offset_alignment as u64;
                self.clip_stack_upload = Some(
                    self.uploads
                        .as_mut()
                        .expect("clip uploads require a frame-owned pass")
                        .append_clips(self.graphics, &bytes, alignment),
                );
            }
            self.clip_stack_upload.as_ref().unwrap()
        };
        // Compare borrowed bindings before cloning: this runs for every primitive,
        // even when the disabled/active stack is already bound. group_one is also
        // invalidated by raw access and application bindings with dynamic offsets.
        if self.group_one.as_ref() == Some(group) {
            return;
        }
        self.group_one = Some(group.clone());
        self.flush();
        self.inner
            .set_bind_group(1, self.group_one.as_ref().unwrap(), &[]);
    }

    // Binds the active clip record, or the shared disabled record, at `slot`.
    pub(crate) fn bind_clip(&mut self, slot: u32) {
        let Some(clip) = self.rounded_clip() else {
            let graphics = self.graphics;
            self.set_vertex_buffer(slot, graphics.no_clip(), 0..crate::clip::SIZE);
            return;
        };
        if self.clip_upload.is_none() {
            let record = clip.record(self.viewport_size());
            self.clip_upload = Some(self.upload_instances(bytemuck::bytes_of(&record), 4));
        }
        let (buffer, range) = self.clip_upload.clone().unwrap();
        self.set_vertex_buffer(slot, &buffer, range);
    }

    // Restores a scissor that was valid for this pass's attachment.
    pub(crate) fn restore_scissor(&mut self, scissor: [u32; 4]) {
        debug_assert!(validate_scissor(self.size, scissor).is_ok());
        self.raster_dirty |= self.scissor != scissor;
        self.scissor = scissor;
        self.apply_raster_state();
    }

    /// Physical scissor covering viewport-relative pixel `bounds`, intersected
    /// with the current scissor. Disjoint bounds give an empty rectangle.
    pub(crate) fn scissor_within(&self, bounds: crate::Rect) -> [u32; 4] {
        let [sx, sy, sw, sh] = self.scissor.map(f64::from);
        let x = f64::from(self.viewport[0]) + f64::from(bounds.x);
        let y = f64::from(self.viewport[1]) + f64::from(bounds.y);
        let left = x.floor().clamp(sx, sx + sw);
        let top = y.floor().clamp(sy, sy + sh);
        let right = (x + f64::from(bounds.width)).ceil().clamp(left, sx + sw);
        let bottom = (y + f64::from(bounds.height)).ceil().clamp(top, sy + sh);
        [left, top, right - left, bottom - top].map(|v| v as u32)
    }

    /// Changes stencil comparisons and replacement values for subsequent draws.
    ///
    /// Built-in renderers restore this wrapped state after raw access, along with viewport
    /// and scissor. Changing the reference creates no pipeline or GPU resource.
    pub fn set_stencil_reference(&mut self, reference: u32) {
        self.raster_dirty |= self.stencil_reference != reference;
        self.stencil_reference = reference;
        self.apply_raster_state();
    }

    /// Returns the optional depth/stencil format required by compatible pipelines.
    pub fn depth_stencil_format(&self) -> Option<wgpu::TextureFormat> {
        self.depth_stencil_format
    }

    /// Reports whether an attached depth aspect is read-only in this pass.
    pub fn depth_read_only(&self) -> bool {
        self.depth_stencil_format
            .is_some_and(|format| format.has_depth_aspect())
            && self.depth_read_only
    }

    /// Reports whether an attached stencil aspect is read-only in this pass.
    pub fn stencil_read_only(&self) -> bool {
        self.depth_stencil_format
            .is_some_and(|format| format.has_stencil_aspect())
            && self.stencil_read_only
    }

    /// Returns the color attachment format used to select compatible pipelines.
    pub fn format(&self) -> Option<wgpu::TextureFormat> {
        self.format
    }

    /// Returns the color attachment sample count required by compatible pipelines.
    ///
    /// Custom renderers must use this count in [`wgpu::MultisampleState`]. The
    /// built-in the drawing renderer selects it automatically.
    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    /// Returns the tracked physical scissor [x, y, width, height].
    /// Raw wgpu changes do not update this value; the next wrapped draw restores it.
    pub fn scissor_rect(&self) -> [u32; 4] {
        self.scissor
    }

    /// Returns the tracked physical viewport [x, y, width, height, min_depth, max_depth].
    /// Raw wgpu changes do not update it. Geometry uses coordinates relative to its origin.
    pub fn viewport(&self) -> [f32; 6] {
        self.viewport
    }

    /// Returns the attachment's physical pixel dimensions.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// Returns the device that all resources used in this pass must belong to.
    pub fn device(&self) -> &wgpu::Device {
        self.graphics.device()
    }

    /// Borrows the underlying wgpu pass for application-defined rendering.
    ///
    /// Use pipelines matching [`Self::format`], [`Self::sample_count`],
    /// [`Self::depth_stencil_format`], and aspect read-only flags, and
    /// resources from [`Self::device`]. Raw changes
    /// persist in wgpu's state but do not update this wrapper's viewport, scissor,
    /// or stencil reference.
    /// the drawing renderer reapplies the wrapper's chosen viewport and scissor
    /// and stencil reference when raw access invalidates tracked state. Use wrapped setters to control
    /// mesh rasterization and stencil comparisons.
    /// Custom renderers are responsible for the GPU state their draws need.
    pub fn as_wgpu(&mut self) -> &mut wgpu::RenderPass<'frame> {
        self.flush();
        self.raster_dirty = true;
        self.bound_pipeline = None;
        self.vertex_zero = None;
        self.vertex_other.clear();
        self.bound_index = None;
        self.group_zero = None;
        self.group_one = None;
        &mut self.inner
    }

    pub(crate) fn set_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: &wgpu::Buffer,
        range: std::ops::Range<u64>,
    ) {
        let current = if slot == 0 {
            &mut self.vertex_zero
        } else {
            let slot = slot as usize - 1;
            if self.vertex_other.len() <= slot {
                self.vertex_other.resize_with(slot + 1, || None);
            }
            &mut self.vertex_other[slot]
        };
        if current
            .as_ref()
            .is_some_and(|(b, r)| b == buffer && *r == range)
        {
            return;
        }
        if let Some(draw) = self.pending.take() {
            #[cfg(test)]
            {
                self.recorded_draws += 1;
            }
            draw.record(&mut self.inner);
        }
        self.inner
            .set_vertex_buffer(slot, buffer.slice(range.clone()));
        *current = Some((buffer.clone(), range));
    }
    pub(crate) fn set_index_buffer(
        &mut self,
        buffer: &wgpu::Buffer,
        range: std::ops::Range<u64>,
        format: wgpu::IndexFormat,
    ) {
        if self
            .bound_index
            .as_ref()
            .is_some_and(|(b, r, f)| b == buffer && *r == range && *f == format)
        {
            return;
        }
        self.flush();
        self.inner
            .set_index_buffer(buffer.slice(range.clone()), format);
        self.bound_index = Some((buffer.clone(), range, format));
    }

    pub(crate) fn set_pipeline(&mut self, pipeline: &wgpu::RenderPipeline) {
        if self.bound_pipeline.as_ref() != Some(pipeline) {
            self.flush();
            self.inner.set_pipeline(pipeline);
            self.bound_pipeline = Some(pipeline.clone());
        }
    }

    pub(crate) fn apply_raster_state(&mut self) {
        if !self.raster_dirty {
            return;
        }
        self.flush();
        self.raster_dirty = false;
        let [x, y, width, height, min_depth, max_depth] = self.viewport;
        self.inner
            .set_viewport(x, y, width, height, min_depth, max_depth);
        let [x, y, width, height] = self.scissor;
        self.inner.set_scissor_rect(x, y, width, height);
        if self
            .depth_stencil_format
            .is_some_and(|format| format.has_stencil_aspect())
        {
            self.inner.set_stencil_reference(self.stencil_reference);
        }
    }

    /// Records a draw immediately, after any pending draw.
    pub(crate) fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.flush();
        #[cfg(test)]
        {
            self.recorded_draws += 1;
        }
        self.inner.draw(vertices, instances);
    }

    /// Records an indexed draw immediately, after any pending draw.
    #[cfg(test)]
    pub(crate) fn draw_indexed(
        &mut self,
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    ) {
        self.flush();
        #[cfg(test)]
        {
            self.recorded_draws += 1;
        }
        self.inner.draw_indexed(indices, base_vertex, instances);
    }

    /// Records an instanced draw of bound geometry, merging it into the pending
    /// draw when nothing changed in between and its instances continue that
    /// draw's range. The merged command shades the same instances in the same order.
    pub(crate) fn draw_instances(&mut self, geometry: DrawGeometry, instances: Range<u32>) {
        if let Some(pending) = &mut self.pending
            && pending.geometry == geometry
            && pending.instances.end == instances.start
        {
            pending.instances.end = instances.end;
            return;
        }
        self.flush();
        self.pending = Some(PendingDraw {
            geometry,
            instances,
        });
    }

    pub(crate) fn flush(&mut self) {
        if let Some(draw) = self.pending.take() {
            #[cfg(test)]
            {
                self.recorded_draws += 1;
            }
            draw.record(&mut self.inner);
        }
    }
}

impl Drop for RenderPass<'_> {
    fn drop(&mut self) {
        self.flush();
    }
}

/// Geometry of a mergeable instanced draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DrawGeometry {
    Vertices(Range<u32>),
    Indexed(Range<u32>),
}

#[derive(Debug)]
struct PendingDraw {
    geometry: DrawGeometry,
    instances: Range<u32>,
}
impl PendingDraw {
    fn record(self, pass: &mut wgpu::RenderPass<'_>) {
        match self.geometry {
            DrawGeometry::Vertices(vertices) => pass.draw(vertices, self.instances),
            DrawGeometry::Indexed(indices) => pass.draw_indexed(indices, 0, self.instances),
        }
    }
}

fn validate_viewport(device: &wgpu::Device, viewport: [f32; 6]) -> Result<(), Error> {
    let [x, y, width, height, min_depth, max_depth] = viewport;
    let max = device.limits().max_texture_dimension_2d as f32;
    let bound = max * 2.0;
    if !viewport.iter().all(|value| value.is_finite())
        || width < 0.0
        || height < 0.0
        || width > max
        || height > max
        || x < -bound
        || y < -bound
        || x + width > bound - 1.0
        || y + height > bound - 1.0
        || !(0.0..=1.0).contains(&min_depth)
        || !(0.0..=1.0).contains(&max_depth)
        || min_depth > max_depth
    {
        return Err(Error::InvalidViewport);
    }
    Ok(())
}

fn validate_scissor(size: [u32; 2], scissor: [u32; 4]) -> Result<(), Error> {
    let [x, y, width, height] = scissor;
    if x.checked_add(width).is_none_or(|right| right > size[0])
        || y.checked_add(height).is_none_or(|bottom| bottom > size[1])
    {
        return Err(Error::InvalidScissorRect);
    }
    Ok(())
}

fn needs_initialized<T>(ops: Option<wgpu::Operations<T>>) -> bool {
    ops.is_none_or(|ops| matches!(ops.load, wgpu::LoadOp::Load))
}

fn validate_depth_stencil_options(options: &PassOptions<'_>) -> Result<(), Error> {
    let format = options
        .depth_stencil
        .map(|attachment| attachment.texture.format());
    if options.depth_requested && !format.is_some_and(|format| format.has_depth_aspect()) {
        return Err(Error::MissingDepthAttachment);
    }
    if options.stencil_requested && !format.is_some_and(|format| format.has_stencil_aspect()) {
        return Err(Error::MissingStencilAttachment);
    }
    if format.is_some_and(|format| format.has_depth_aspect())
        && let Some(wgpu::Operations {
            load: wgpu::LoadOp::Clear(depth),
            ..
        }) = options.depth_ops
        && (!depth.is_finite() || !(0.0..=1.0).contains(&depth))
    {
        return Err(Error::InvalidClearDepth);
    }
    Ok(())
}
