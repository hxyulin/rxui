use std::sync::{Arc, atomic::AtomicBool};

use crate::{Error, GraphicsContext, target};

/// Target-owned, persistent depth/stencil storage, never resolved with MSAA.
#[derive(Debug)]
pub(crate) struct DepthStencilAttachment {
    pub(crate) view: wgpu::TextureView,
    pub(crate) texture: wgpu::Texture,
    pub(crate) depth_initialized: Arc<AtomicBool>,
    pub(crate) stencil_initialized: Arc<AtomicBool>,
}

pub(crate) fn supported_counts(
    graphics: &GraphicsContext,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> Result<Vec<u32>, Error> {
    let features = target::format_features(graphics, format);
    if !format.is_depth_stencil_format()
        || !graphics
            .device()
            .features()
            .contains(format.required_features())
        || !features
            .allowed_usages
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
    {
        return Err(Error::UnsupportedDepthStencilFormat { format });
    }
    if !usage.contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        || usage.contains(wgpu::TextureUsages::TRANSIENT_ATTACHMENT)
        || !features.allowed_usages.contains(usage)
    {
        return Err(Error::InvalidDepthStencilUsage { format, usage });
    }
    // Depth/stencil samples are stored directly. Color resolve support is irrelevant.
    Ok(features.flags.supported_sample_counts())
}

pub(crate) fn restrict_counts(
    graphics: &GraphicsContext,
    counts: &mut Vec<u32>,
    format: Option<wgpu::TextureFormat>,
    usage: wgpu::TextureUsages,
) -> Result<(), Error> {
    if let Some(format) = format {
        let depth_counts = supported_counts(graphics, format, usage)?;
        counts.retain(|count| depth_counts.contains(count));
    }
    Ok(())
}

pub(crate) fn allocate(
    graphics: &GraphicsContext,
    size: [u32; 2],
    sample_count: u32,
    format: Option<wgpu::TextureFormat>,
    usage: wgpu::TextureUsages,
) -> Option<DepthStencilAttachment> {
    let format = format?;
    if size.contains(&0) {
        return None;
    }
    let texture = graphics.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("RXUI drawing depth/stencil attachment"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    Some(DepthStencilAttachment {
        view: texture.create_view(&Default::default()),
        texture,
        depth_initialized: Arc::new(AtomicBool::new(false)),
        stencil_initialized: Arc::new(AtomicBool::new(false)),
    })
}
