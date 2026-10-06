use crate::{Bounds, Color, StyleColor, UiError, id::next_runtime};
use std::sync::Arc;

/// Stable shared-image identity. Cloning a handle keeps this identity and its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(u64);
#[derive(Debug)]
pub(crate) enum Source {
    Rgba {
        size: [u32; 2],
        bytes: Arc<[u8]>,
    },
    #[cfg(feature = "rendering")]
    Texture(astrelis::Texture),
    #[cfg(feature = "rendering")]
    View {
        view: astrelis::wgpu::TextureView,
        size: [u32; 2],
    },
    #[cfg(feature = "rendering")]
    Framebuffer(astrelis::SampledColor),
}
#[derive(Debug)]
struct Data {
    id: ImageId,
    source: Source,
    alpha: ImageAlpha,
}
/// Immutable, cheaply cloned image source. RGBA pixels are uploaded once per painter
/// cache; GPU sources remain application-owned. No decoding/I/O/upload occurs in a view.
/// One source pixel is one intrinsic logical unit; explicit element sizing controls DPI.
#[derive(Clone)]
pub struct Image(Arc<Data>);
impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("id", &self.id())
            .field("pixel_size", &self.pixel_size())
            .finish_non_exhaustive()
    }
}
impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}
impl Image {
    /// Copies/moves tightly packed straight-alpha sRGB RGBA8 pixels into a shared asset.
    /// Dimensions must be nonzero and bytes must contain exactly width*height*4 bytes.
    pub fn from_rgba8(
        width: u32,
        height: u32,
        pixels: impl Into<Arc<[u8]>>,
    ) -> Result<Self, UiError> {
        let bytes = pixels.into();
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4));
        if width == 0 || height == 0 || expected != Some(bytes.len()) {
            return Err(UiError::InvalidImage);
        }
        Ok(Self::new(
            Source::Rgba {
                size: [width, height],
                bytes,
            },
            ImageAlpha::Straight,
        ))
    }
    /// Decodes PNG/JPEG bytes on the calling thread with the decoder's default limits.
    /// Run this in spawn_blocking for large assets; no filesystem/network I/O is implicit.
    #[cfg(feature = "image-decoding")]
    pub fn decode(bytes: &[u8]) -> Result<Self, UiError> {
        let decoded = image_codec::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| UiError::ImageDecode(Box::new(e)))?
            .decode()
            .map_err(|e| UiError::ImageDecode(Box::new(e)))?
            .into_rgba8();
        Self::from_rgba8(decoded.width(), decoded.height(), decoded.into_raw())
    }
    /// Shares a texture's default full 2D view. Pixel writes need redraw, not new identity.
    /// The texture must belong to the painter's graphics device and support sampling.
    #[cfg(feature = "rendering")]
    pub fn from_texture(texture: astrelis::Texture) -> Self {
        Self::new(Source::Texture(texture), ImageAlpha::Straight)
    }
    /// Shares a raw view with explicit intrinsic pixel dimensions (for custom mip/layer views).
    /// Use a single-sampled 2D color view from the painter's graphics device.
    /// Astrelis validates sampling support; wgpu validates raw-view device ownership.
    #[cfg(feature = "rendering")]
    pub fn from_view(view: astrelis::wgpu::TextureView, size: [u32; 2]) -> Result<Self, UiError> {
        if size.contains(&0) {
            return Err(UiError::InvalidImage);
        }
        Ok(Self::new(Source::View { view, size }, ImageAlpha::Straight))
    }
    /// Retains the live resolved color output; resize/MSAA replacement follows the source.
    /// Creation requires TEXTURE_BINDING usage. Render before the UI samples this output.
    #[cfg(feature = "rendering")]
    pub fn from_framebuffer(framebuffer: &astrelis::Framebuffer) -> Self {
        Self::from_sampled(framebuffer.sampled_color())
    }
    /// Shares a live framebuffer output without borrowing its rendering owner.
    #[cfg(feature = "rendering")]
    pub fn from_sampled(source: astrelis::SampledColor) -> Self {
        Self::new(Source::Framebuffer(source), ImageAlpha::Premultiplied)
    }
    fn new(source: Source, alpha: ImageAlpha) -> Self {
        Self(Arc::new(Data {
            id: ImageId(next_runtime()),
            source,
            alpha,
        }))
    }
    /// Stable source identity, independent of placement and framebuffer storage generations.
    pub fn id(&self) -> ImageId {
        self.0.id
    }
    /// Current physical pixel dimensions; a suspended framebuffer has no current output.
    pub fn pixel_size(&self) -> Option<[u32; 2]> {
        match &self.0.source {
            Source::Rgba { size, .. } => Some(*size),
            #[cfg(feature = "rendering")]
            Source::Texture(texture) => Some(texture.size()),
            #[cfg(feature = "rendering")]
            Source::View { size, .. } => Some(*size),
            #[cfg(feature = "rendering")]
            Source::Framebuffer(source) => source
                .view()
                .ok()
                .map(|v| [v.texture().width(), v.texture().height()]),
        }
    }
    /// Borrowed CPU RGBA pixels, absent for GPU-backed sources.
    pub fn rgba8_pixels(&self) -> Option<&[u8]> {
        match self.source() {
            Source::Rgba { bytes, .. } => Some(bytes),
            #[cfg(feature = "rendering")]
            _ => None,
        }
    }
    pub(crate) fn is_live(&self) -> bool {
        #[cfg(feature = "rendering")]
        {
            matches!(self.source(), Source::Framebuffer(_))
        }
        #[cfg(not(feature = "rendering"))]
        {
            false
        }
    }
    pub(crate) fn source(&self) -> &Source {
        &self.0.source
    }
    pub(crate) fn alpha(&self) -> ImageAlpha {
        self.0.alpha
    }
}
/// Mapping from source aspect ratio into the element's content box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// Preserve aspect, leaving unused space around the image.
    #[default]
    Contain,
    /// Preserve aspect, cropping source pixels to fill the box.
    Cover,
    /// Fill both dimensions independently.
    Stretch,
}
/// Sampling requirement, independent of image placement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageFilter {
    /// Linear interpolation; source storage must support filtering.
    #[default]
    Linear,
    /// Nearest texel; useful for pixel art and unfilterable float storage.
    Nearest,
}
/// Source RGB alpha encoding. Framebuffer output defaults to premultiplied;
/// decoded/uploaded images default to straight. Custom shaders can override it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageAlpha {
    /// RGB is independent of alpha.
    Straight,
    /// RGB is multiplied by alpha.
    Premultiplied,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Properties {
    pub source: Image,
    pub fit: ImageFit,
    pub align: [f32; 2],
    pub uv: [f32; 4],
    pub tint: StyleColor,
    pub filter: ImageFilter,
    pub alpha: ImageAlpha,
}
impl Properties {
    pub fn new(source: Image) -> Self {
        let alpha = source.alpha();
        Self {
            source,
            fit: ImageFit::Contain,
            align: [0.5; 2],
            uv: [0., 0., 1., 1.],
            tint: StyleColor::Literal([1.; 4]),
            filter: ImageFilter::Linear,
            alpha,
        }
    }
    pub fn validate(&self) -> Result<(), UiError> {
        if self
            .align
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || !self.tint.valid()
            || self
                .uv
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || self.uv[2] <= 0.
            || self.uv[3] <= 0.
            || self.uv[0] + self.uv[2] > 1.
            || self.uv[1] + self.uv[3] > 1.
        {
            return Err(UiError::InvalidImage);
        }
        Ok(())
    }
}
/// Prepared image placement for custom painters. UVs are normalized source coordinates.
pub struct ImageInfo<'a> {
    /// Shared source, independent of placement.
    pub source: &'a Image,
    /// Destination in logical coordinates, inside the content box.
    pub destination: Bounds,
    /// Normalized sampled source rectangle, including Cover crop.
    pub uv: [f32; 4],
    /// Resolved linear RGBA tint.
    pub tint: Color,
    /// Sampling requirement.
    pub filter: ImageFilter,
    /// Alpha encoding.
    pub alpha: ImageAlpha,
}
pub(crate) fn placement(content: Bounds, size: [f32; 2], props: &Properties) -> (Bounds, [f32; 4]) {
    let mut destination = content;
    let mut uv = props.uv;
    let w = size[0] * uv[2];
    let h = size[1] * uv[3];
    if w <= 0. || h <= 0. || content.width <= 0. || content.height <= 0. {
        destination.width = 0.;
        destination.height = 0.;
        return (destination, uv);
    }
    match props.fit {
        ImageFit::Stretch => {}
        ImageFit::Contain => {
            let scale = (content.width / w).min(content.height / h);
            destination.width = w * scale;
            destination.height = h * scale;
            destination.x += (content.width - destination.width) * props.align[0];
            destination.y += (content.height - destination.height) * props.align[1];
        }
        ImageFit::Cover => {
            let scale = (content.width / w).max(content.height / h);
            let new_w = content.width / (scale * size[0]);
            let new_h = content.height / (scale * size[1]);
            uv[0] += (uv[2] - new_w) * props.align[0];
            uv[1] += (uv[3] - new_h) * props.align[1];
            uv[2] = new_w;
            uv[3] = new_h;
        }
    }
    (destination, uv)
}
