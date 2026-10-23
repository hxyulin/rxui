use crate::{Error, GraphicsContext};

/// Creation options for a single-layer, single-mip, single-sampled 2D color texture.
///
/// Defaults use sRGB RGBA8 with sampling and upload usages. Pixel bytes use the
/// selected format's encoding; uploads perform no color or alpha conversion.
/// Compressed, depth/stencil, and multi-planar formats are outside this helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "pass these options to GraphicsContext::create_texture"]
pub struct TextureOptions {
    /// Nonzero physical width and height.
    pub size: [u32; 2],
    /// Uncompressed color format supported by the device.
    pub format: wgpu::TextureFormat,
    /// Resource usages. Include `COPY_DST` for uploads and `TEXTURE_BINDING` for sampling.
    pub usage: wgpu::TextureUsages,
}

impl TextureOptions {
    /// Selects sRGB RGBA8, with sampling and upload usages.
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            size: [width, height],
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING.union(wgpu::TextureUsages::COPY_DST),
        }
    }

    /// Selects the byte encoding and sampling color space.
    pub const fn format(mut self, format: wgpu::TextureFormat) -> Self {
        self.format = format;
        self
    }

    /// Replaces usages with those required by the application.
    pub const fn usage(mut self, usage: wgpu::TextureUsages) -> Self {
        self.usage = usage;
        self
    }
}

/// An owned 2D color texture and its default sampling view.
///
/// Create through [`GraphicsContext::create_texture`], then upload tightly packed
/// texels with [`Self::write`] or [`Self::write_region`]. Retain the texture across
/// frames. Cloning shares storage; it does not copy pixels. wgpu zero-initializes
/// storage before use when no upload has established its contents.
///
/// This resource has no automatic resizing. Create a replacement and refresh
/// renderer bindings when dimensions change. Raw access supports custom sampling,
/// copies, and render passes when the selected usages allow them.
#[derive(Clone, Debug)]
pub struct Texture {
    graphics: GraphicsContext,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Texture {
    pub(crate) fn create(
        graphics: &GraphicsContext,
        options: TextureOptions,
    ) -> Result<Self, Error> {
        let [width, height] = options.size;
        let max = graphics.device().limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > max || height > max {
            return Err(Error::InvalidTextureSize { width, height, max });
        }
        let format = options.format;
        let features = crate::target::format_features(graphics, format);
        if format.is_depth_stencil_format()
            || format.is_multi_planar_format()
            || format.block_dimensions() != (1, 1)
            || format.block_copy_size(None).is_none()
            || !graphics
                .device()
                .features()
                .contains(format.required_features())
        {
            return Err(Error::UnsupportedTextureFormat { format });
        }
        if options.usage.is_empty()
            || options
                .usage
                .contains(wgpu::TextureUsages::TRANSIENT_ATTACHMENT)
            || !features.allowed_usages.contains(options.usage)
        {
            return Err(Error::InvalidTextureUsage {
                format,
                usage: options.usage,
            });
        }
        let texture = graphics.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("RXUI drawing texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: options.usage,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Ok(Self {
            graphics: graphics.clone(),
            texture,
            view,
        })
    }

    /// Uploads the complete texture using tightly packed rows in the texture's format.
    ///
    /// Rejects an incorrect byte count or missing `COPY_DST` usage before queuing
    /// work. This schedules a queue upload, without submitting or waiting. Writes
    /// execute before commands in the next queue submission, including commands
    /// recorded earlier. Update between submissions when preserving older draws
    /// matters; an upload is not a snapshot attached to an individual draw.
    pub fn write(&self, bytes: &[u8]) -> Result<(), Error> {
        self.write_region([0, 0], self.size(), bytes)
    }

    /// Uploads a nonempty in-bounds rectangle, with no row padding or conversions.
    ///
    /// Byte count must equal width × height × bytes per texel. Queue uploads do
    /// not require 256-byte row alignment. Errors leave the upload queue unchanged.
    /// Submission ordering follows [`Self::write`].
    pub fn write_region(
        &self,
        origin: [u32; 2],
        size: [u32; 2],
        bytes: &[u8],
    ) -> Result<(), Error> {
        if !self.texture.usage().contains(wgpu::TextureUsages::COPY_DST) {
            return Err(Error::InvalidTextureUsage {
                format: self.format(),
                usage: self.texture.usage(),
            });
        }
        let valid = size[0] != 0
            && size[1] != 0
            && origin[0]
                .checked_add(size[0])
                .is_some_and(|end| end <= self.size()[0])
            && origin[1]
                .checked_add(size[1])
                .is_some_and(|end| end <= self.size()[1]);
        if !valid {
            return Err(Error::InvalidTextureRegion);
        }
        let texel_size = self
            .format()
            .block_copy_size(None)
            .expect("validated texture format");
        let row_bytes = u64::from(size[0]) * u64::from(texel_size);
        let expected = row_bytes * u64::from(size[1]);
        if expected != bytes.len() as u64 || row_bytes > u64::from(u32::MAX) {
            return Err(Error::InvalidTextureData {
                expected,
                actual: bytes.len(),
            });
        }
        self.graphics.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes as u32),
                rows_per_image: Some(size[1]),
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        Ok(())
    }

    /// Returns physical dimensions.
    pub fn size(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }
    /// Returns the pixel encoding.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.texture.format()
    }
    /// Returns resource usages selected at creation.
    pub fn usage(&self) -> wgpu::TextureUsages {
        self.texture.usage()
    }
    /// Returns the default 2D sampling view.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
    /// Returns the underlying resource for custom wgpu operations.
    pub fn as_wgpu(&self) -> &wgpu::Texture {
        &self.texture
    }
}
