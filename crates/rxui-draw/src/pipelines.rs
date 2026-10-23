use crate::{Error, GraphicsContext, Material, RenderPass};
use std::collections::{HashMap, hash_map::Entry};

pub(crate) type Pipelines = HashMap<
    (u64, wgpu::TextureFormat, u32, Option<wgpu::TextureFormat>),
    crate::pipeline_cache::CachedPipeline,
>;

pub(crate) fn validate_aspects(pass: &RenderPass<'_>, material: &Material) -> Result<(), Error> {
    if let Some(state) = &material.depth_stencil {
        if pass.depth_read_only() && !state.is_depth_read_only() {
            return Err(Error::ReadOnlyDepth);
        }
        if pass.stencil_read_only() && !state.is_stencil_read_only(material.cull_mode) {
            return Err(Error::ReadOnlyStencil);
        }
    }
    Ok(())
}

pub(crate) fn pipeline<'cache>(
    graphics: &GraphicsContext,
    pipelines: &'cache mut Pipelines,
    material: &Material,
    format: wgpu::TextureFormat,
    sample_count: u32,
    depth_stencil_format: Option<wgpu::TextureFormat>,
    default_material: bool,
) -> Result<&'cache wgpu::RenderPipeline, Error> {
    if !material.graphics.same_device(graphics) {
        return Err(Error::DeviceMismatch);
    }
    if let Some(state) = &material.depth_stencil
        && Some(state.format) != depth_stencil_format
    {
        return Err(Error::DepthStencilMismatch {
            material: state.format,
            pass: depth_stencil_format,
        });
    }
    let entry = match pipelines.entry((material.id, format, sample_count, depth_stencil_format)) {
        Entry::Occupied(entry) => return Ok(entry.into_mut()),
        Entry::Vacant(entry) => entry,
    };
    // Cache hits return above; only new variants pay for these timing scopes.
    profiling::scope!("rxui_draw::create_mesh_pipeline");
    let features = crate::target::format_features(graphics, format);
    if format.is_depth_stencil_format()
        || format.is_multi_planar_format()
        || !graphics
            .device()
            .features()
            .contains(format.required_features())
        || !features
            .allowed_usages
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
    {
        return Err(Error::UnsupportedColorFormat { format });
    }
    if default_material
        && (!matches!(
            format.sample_type(None, Some(graphics.device().features())),
            Some(wgpu::TextureSampleType::Float { .. })
        ) || (material.blend.is_some()
            && !features
                .flags
                .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)))
    {
        return Err(Error::UnsupportedMeshFormat { format });
    }
    if material.blend.is_some()
        && !features
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)
    {
        return Err(Error::UnsupportedMaterialFormat { format });
    }
    crate::target::validate_sample_count(
        format,
        sample_count,
        &crate::target::sample_counts(features),
    )?;
    if let Some(depth_format) = depth_stencil_format {
        let counts = crate::depth_stencil::supported_counts(
            graphics,
            depth_format,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )?;
        crate::target::validate_sample_count(depth_format, sample_count, &counts)?;
    }
    let depth_stencil = material.depth_stencil.clone().or_else(|| {
        depth_stencil_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: format.has_depth_aspect().then_some(false),
            depth_compare: None,
            stencil: Default::default(),
            bias: Default::default(),
        })
    });
    let vertex_layouts: Vec<_> = material
        .vertex_layouts
        .iter()
        .map(|l| Some(l.as_wgpu()))
        .collect();
    profiling::scope!("rxui_draw::wgpu::create_render_pipeline");
    Ok(entry.insert(crate::pipeline_cache::CachedPipeline::new(
        graphics
            .device()
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("RXUI drawing mesh material pipeline"),
                layout: Some(&material.layout),
                vertex: wgpu::VertexState {
                    module: &material.shader,
                    entry_point: Some(&material.vertex_entry),
                    compilation_options: Default::default(),
                    buffers: &vertex_layouts,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &material.shader,
                    entry_point: Some(&material.fragment_entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: material.blend,
                        write_mask: material.write_mask,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    front_face: material.front_face,
                    topology: material.topology,
                    strip_index_format: material.strip_index_format,
                    cull_mode: material.cull_mode,
                    ..Default::default()
                },
                depth_stencil,
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            }),
        &material.lifetime,
    )))
}
