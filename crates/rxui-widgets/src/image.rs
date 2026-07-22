//! Decoded raster images and retained image presentation.

use std::{any::Any, error::Error, fmt};

use astrelis_core::geometry::{LogicalRect, LogicalSize, Physical, Rect, Size};
use astrelis_paint::{Image, ImageOptions, ImageSampling, Painter};
use astrelis_ui_core::{SemanticRole, Theme, UiError, Widget, WidgetContainerStyle};

/// Error returned when encoded image bytes cannot be decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageDecodeError(String);

impl fmt::Display for ImageDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Error for ImageDecodeError {}

/// Decodes PNG, JPEG, or WebP bytes into Astrelis's immutable RGBA image.
pub fn decode_image(bytes: &[u8]) -> Result<Image, ImageDecodeError> {
    let decoded =
        image::load_from_memory(bytes).map_err(|error| ImageDecodeError(error.to_string()))?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Image::from_rgba8(Size::<Physical, u32>::new(width, height), rgba.into_raw())
        .map_err(|error| ImageDecodeError(error.to_string()))
}

/// How an image is fitted into its retained bounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// Preserve aspect ratio and show the entire image.
    #[default]
    Contain,
    /// Preserve aspect ratio and fill the bounds, clipping overflow.
    Cover,
    /// Stretch independently along both axes.
    Fill,
    /// Present one logical unit per source pixel.
    None,
}

/// Normalized placement of unused or clipped space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageAlignment {
    /// Horizontal placement from left (`0`) to right (`1`).
    pub x: f32,
    /// Vertical placement from top (`0`) to bottom (`1`).
    pub y: f32,
}

impl Default for ImageAlignment {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5 }
    }
}

impl ImageAlignment {
    /// Creates a clamped normalized alignment.
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
        }
    }
}

/// Retained, non-interactive presentation of a decoded CPU-backed image.
#[derive(Clone, Debug)]
pub struct ImageView {
    image: Image,
    label: String,
    fit: ImageFit,
    alignment: ImageAlignment,
    sampling: ImageSampling,
    opacity: f32,
}

impl ImageView {
    /// Creates a centered, contained image with an accessible label.
    pub fn new(image: Image, label: impl Into<String>) -> Self {
        Self {
            image,
            label: label.into(),
            fit: ImageFit::Contain,
            alignment: ImageAlignment::default(),
            sampling: ImageSampling::Linear,
            opacity: 1.0,
        }
    }

    /// Changes image fitting.
    #[must_use]
    pub const fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Changes image placement within the fitted bounds.
    #[must_use]
    pub const fn alignment(mut self, alignment: ImageAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Changes minification/magnification sampling.
    #[must_use]
    pub const fn sampling(mut self, sampling: ImageSampling) -> Self {
        self.sampling = sampling;
        self
    }

    /// Changes opacity, clamped to `0..=1`.
    #[must_use]
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Replaces the displayed image.
    pub fn set_image(&mut self, image: Image) {
        self.image = image;
    }

    /// Computes the destination rectangle for a given set of bounds.
    pub fn destination(&self, bounds: LogicalRect) -> LogicalRect {
        let size = self.image.size();
        let source: LogicalSize = Size::new(size.width as f32, size.height as f32);
        if self.fit == ImageFit::Fill {
            return bounds;
        }
        let scale = match self.fit {
            ImageFit::Contain => {
                (bounds.size.width / source.width).min(bounds.size.height / source.height)
            }
            ImageFit::Cover => {
                (bounds.size.width / source.width).max(bounds.size.height / source.height)
            }
            ImageFit::None => 1.0,
            ImageFit::Fill => unreachable!(),
        };
        let fitted: LogicalSize = Size::new(source.width * scale, source.height * scale);
        Rect::from_xywh(
            bounds.origin.x + (bounds.size.width - fitted.width) * self.alignment.x,
            bounds.origin.y + (bounds.size.height - fitted.height) * self.alignment.y,
            fitted.width,
            fitted.height,
        )
    }
}

impl<Message: 'static> Widget<Message> for ImageView {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn intrinsic_size(&self, _theme: &Theme) -> LogicalSize {
        let size = self.image.size();
        Size::new(size.width as f32, size.height as f32)
    }

    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle::structural()
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        _theme: &Theme,
    ) -> Result<(), UiError> {
        let destination = self.destination(bounds);
        painter.with_save(|painter| {
            painter.clip_rect(bounds)?;
            painter.draw_image(
                &self.image,
                destination,
                ImageOptions {
                    source: None,
                    sampling: self.sampling,
                    opacity: self.opacity,
                },
            )
        })?;
        Ok(())
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        let size = self.image.size();
        Some((
            SemanticRole::Group,
            self.label.clone(),
            Some(format!("{} by {} pixels", size.width, size.height)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contain_and_cover_preserve_aspect_ratio() {
        let image = Image::from_rgba8(Size::new(200, 100), vec![255; 200 * 100 * 4]).unwrap();
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        assert_eq!(
            ImageView::new(image.clone(), "test").destination(bounds),
            Rect::from_xywh(0.0, 25.0, 100.0, 50.0)
        );
        assert_eq!(
            ImageView::new(image, "test")
                .fit(ImageFit::Cover)
                .destination(bounds),
            Rect::from_xywh(-50.0, 0.0, 200.0, 100.0)
        );
    }

    #[test]
    fn rejects_invalid_encoded_bytes() {
        assert!(decode_image(b"not an image").is_err());
    }

    #[test]
    fn decodes_png_into_astrelis_rgba() {
        let source = image::RgbaImage::from_pixel(2, 1, image::Rgba([4, 8, 16, 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let decoded = decode_image(encoded.get_ref()).unwrap();
        assert_eq!(decoded.size(), Size::new(2, 1));
        assert_eq!(decoded.rgba8(), &[4, 8, 16, 255, 4, 8, 16, 255]);
    }
}
