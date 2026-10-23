use std::sync::atomic::{AtomicU64, Ordering};

use crate::GraphicsContext;

/// Shader and render-state choices for an indexed mesh material.
///
/// Start with [`Self::new`] and a shader created on the context's device. The
/// shader uses [`crate::Vertex`]: location 0 is `vec3<f32>` position and location 1
/// is `vec4<f32>` color. Vertex output positions are clip-space homogeneous
/// coordinates. The fragment output at location 0 must match the target format.
/// Shaders can transform positions and use application-owned uniform buffers,
/// textures, and storage resources through explicit bind-group layouts.
///
/// Defaults use `vertex_main` / `fragment_main`, premultiplied alpha blending,
/// all color channels, counterclockwise front faces, and no face culling.
/// Premultiplied blending requires the shader to output premultiplied colors.
/// Depth/stencil settings are optional. Declare matching custom vertex layouts and topology when using custom geometry.
#[derive(Clone, Debug)]
#[must_use = "pass these options to GraphicsContext::create_material"]
pub struct MaterialOptions<'a> {
    /// Application-created shader module, on the context's device.
    pub shader: &'a wgpu::ShaderModule,
    /// Vertex entry point, consuming the declared stream layouts.
    pub vertex_entry: &'a str,
    /// Fragment entry point, writing color at location 0.
    pub fragment_entry: &'a str,
    /// Binding layouts in shader group order, following wgpu's layout descriptor.
    pub bind_group_layouts: &'a [Option<&'a wgpu::BindGroupLayout>],
    /// Color blending; `None` replaces the destination color without blending.
    pub blend: Option<wgpu::BlendState>,
    /// Color channels written by the shader.
    pub write_mask: wgpu::ColorWrites,
    /// Winding that identifies front-facing triangles.
    pub front_face: wgpu::FrontFace,
    /// Faces discarded before fragment shading; `None` draws both sides.
    pub cull_mode: Option<wgpu::Face>,
    /// Optional explicit depth/stencil tests, writes, masks, operations, and bias.
    /// The format must match the pass attachment. `None` disables all tests/writes.
    pub depth_stencil: Option<wgpu::DepthStencilState>,
    /// Custom stream layouts; empty selects the standard position/color layout.
    pub vertex_layouts: &'a [crate::VertexLayout],
    /// Primitive topology.
    pub topology: wgpu::PrimitiveTopology,
    /// Index width for strip restart; required for indexed strip topologies.
    pub strip_index_format: Option<wgpu::IndexFormat>,
}

impl<'a> MaterialOptions<'a> {
    /// Selects a shader with the default entry points and mesh render state.
    pub const fn new(shader: &'a wgpu::ShaderModule) -> Self {
        Self {
            shader,
            vertex_entry: "vertex_main",
            fragment_entry: "fragment_main",
            bind_group_layouts: &[],
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            depth_stencil: None,
            vertex_layouts: &[],
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
        }
    }

    /// Selects layouts matching the mesh's vertex and instance streams.
    pub const fn vertex_layouts(mut self, layouts: &'a [crate::VertexLayout]) -> Self {
        self.vertex_layouts = layouts;
        self
    }
    /// Selects primitive topology; indexed strips also need a strip index format.
    pub const fn topology(mut self, topology: wgpu::PrimitiveTopology) -> Self {
        self.topology = topology;
        self
    }
    /// Selects strip index width and primitive restart interpretation.
    pub const fn strip_index_format(mut self, format: wgpu::IndexFormat) -> Self {
        self.strip_index_format = Some(format);
        self
    }
    /// Selects explicit depth/stencil state, or `None` to disable tests and writes.
    ///
    /// Attachment allocation belongs to target creation. This only configures the
    /// mesh pipeline; formats must match. Dynamic stencil references belong to
    /// the pass. With `None`, meshes can still draw over depth-enabled scenes
    /// without testing or modifying the depth/stencil attachment.
    pub fn depth_stencil(mut self, state: Option<wgpu::DepthStencilState>) -> Self {
        self.depth_stencil = state;
        self
    }

    /// Selects vertex and fragment entry points in the shader module.
    pub const fn entry_points(mut self, vertex: &'a str, fragment: &'a str) -> Self {
        self.vertex_entry = vertex;
        self.fragment_entry = fragment;
        self
    }

    /// Selects explicit layouts; applications bind matching groups before drawing.
    pub const fn bind_group_layouts(
        mut self,
        layouts: &'a [Option<&'a wgpu::BindGroupLayout>],
    ) -> Self {
        self.bind_group_layouts = layouts;
        self
    }

    /// Selects blending, or `None` for opaque replacement or integer color outputs.
    pub const fn blend(mut self, blend: Option<wgpu::BlendState>) -> Self {
        self.blend = blend;
        self
    }

    /// Selects which color channels a draw writes.
    pub const fn write_mask(mut self, mask: wgpu::ColorWrites) -> Self {
        self.write_mask = mask;
        self
    }

    /// Selects triangle winding used for face classification.
    pub const fn front_face(mut self, face: wgpu::FrontFace) -> Self {
        self.front_face = face;
        self
    }

    /// Selects face culling; `None` draws both sides of a triangle.
    pub const fn cull_mode(mut self, face: Option<wgpu::Face>) -> Self {
        self.cull_mode = face;
        self
    }
}

/// An immutable, device-bound shading configuration for indexed triangle meshes.
///
/// Create through [`GraphicsContext::create_material`] and draw with
/// the drawing renderer. A material retains its shader and
/// pipeline layout, independently of windows, targets, or a particular renderer.
/// Clones share GPU handles and the identity used by each renderer's pipeline cache.
/// Create a new material to change shader or render state. After the last clone is
/// dropped, the drawing renderer can reclaim its cached variants.
///
/// Buffers, textures, bind groups, and their updates remain application-owned.
/// Bind the groups required by this material with
/// [`crate::RenderPass::set_bind_group`]
/// before each draw that changes bindings. Dynamic offsets work the same as wgpu.
/// The renderer establishes the material's pipeline and mesh geometry, without
/// replacing application resource bindings or pass clipping settings.
#[derive(Clone, Debug)]
pub struct Material {
    pub(crate) id: u64,
    pub(crate) lifetime: std::sync::Arc<()>,
    #[cfg(test)]
    pub(crate) device: wgpu::Device,
    pub(crate) graphics: GraphicsContext,

    pub(crate) shader: wgpu::ShaderModule,
    pub(crate) layout: wgpu::PipelineLayout,
    pub(crate) vertex_entry: String,
    pub(crate) fragment_entry: String,
    pub(crate) blend: Option<wgpu::BlendState>,
    pub(crate) write_mask: wgpu::ColorWrites,
    pub(crate) front_face: wgpu::FrontFace,
    pub(crate) cull_mode: Option<wgpu::Face>,
    pub(crate) depth_stencil: Option<wgpu::DepthStencilState>,
    pub(crate) vertex_layouts: Vec<crate::VertexLayout>,
    pub(crate) topology: wgpu::PrimitiveTopology,
    pub(crate) strip_index_format: Option<wgpu::IndexFormat>,
}

impl Material {
    pub(crate) fn create(graphics: &GraphicsContext, options: MaterialOptions<'_>) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        // Deprecated in 1.99 in favor of `try_update`, which the 1.98.1 MSRV lacks.
        #[allow(deprecated)]
        let id = NEXT_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("material identity space exhausted");
        let layout = graphics
            .device()
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("RXUI drawing mesh material layout"),
                bind_group_layouts: options.bind_group_layouts,
                immediate_size: 0,
            });
        Self {
            id,
            lifetime: std::sync::Arc::new(()),
            #[cfg(test)]
            device: graphics.device().clone(),
            graphics: graphics.clone(),

            shader: options.shader.clone(),
            layout,
            vertex_entry: options.vertex_entry.to_owned(),
            fragment_entry: options.fragment_entry.to_owned(),
            blend: options.blend,
            write_mask: options.write_mask,
            front_face: options.front_face,
            cull_mode: options.cull_mode,
            depth_stencil: options.depth_stencil,
            vertex_layouts: if options.vertex_layouts.is_empty() {
                vec![crate::VertexLayout::new(&crate::Vertex::layout())]
            } else {
                options.vertex_layouts.to_vec()
            },
            topology: options.topology,
            strip_index_format: options.strip_index_format,
        }
    }

    /// Returns the material's explicit depth/stencil state, if testing is enabled.
    pub fn depth_stencil(&self) -> Option<&wgpu::DepthStencilState> {
        self.depth_stencil.as_ref()
    }

    /// Returns the retained application shader module.
    pub fn shader(&self) -> &wgpu::ShaderModule {
        &self.shader
    }

    /// Returns the explicit wgpu pipeline layout retained by this material.
    pub fn pipeline_layout(&self) -> &wgpu::PipelineLayout {
        &self.layout
    }
}
