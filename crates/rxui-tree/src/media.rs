//! Image and application-rendered viewport elements.

use std::any::Any;

use astrelis_core::{
    color::Color,
    geometry::{LogicalRect, LogicalSize},
};
use astrelis_paint::{
    Brush, CompositorViewId, ExternalImage, Image, ImageOptions, ImageSampling, Painter,
};

use crate::{
    Constraints, Element, EventResult, LayoutContext, SemanticData, SemanticRole, UiInput,
};

/// How a raster image fits its retained bounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// Preserve aspect ratio and show the entire image.
    #[default]
    Contain,
    /// Preserve aspect ratio and cover the bounds.
    Cover,
    /// Stretch to the complete bounds.
    Fill,
    /// Present one logical unit per source pixel.
    None,
}

/// Normalized image placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageAlignment {
    /// Horizontal placement from left to right.
    pub x: f32,
    /// Vertical placement from top to bottom.
    pub y: f32,
}

impl ImageAlignment {
    /// Creates clamped normalized placement.
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
        }
    }
}

impl Default for ImageAlignment {
    fn default() -> Self {
        Self::new(0.5, 0.5)
    }
}

/// CPU-backed raster image presentation.
#[derive(Clone, Debug)]
pub struct ImageElement {
    /// Immutable source image.
    pub image: Image,
    /// Accessible label.
    pub label: String,
    /// Preferred logical size.
    pub size: LogicalSize,
    /// Fitting policy.
    pub fit: ImageFit,
    /// Placement within fitted bounds.
    pub alignment: ImageAlignment,
    /// Sampling policy.
    pub sampling: ImageSampling,
    /// Draw opacity.
    pub opacity: f32,
}

impl ImageElement {
    /// Creates a centered, contained image at its source dimensions.
    pub fn new(image: Image, label: impl Into<String>) -> Self {
        let source = image.size();
        Self {
            image,
            label: label.into(),
            size: LogicalSize::new(source.width as f32, source.height as f32),
            fit: ImageFit::Contain,
            alignment: ImageAlignment::default(),
            sampling: ImageSampling::Linear,
            opacity: 1.0,
        }
    }

    fn destination(&self, bounds: LogicalRect) -> LogicalRect {
        let source = self.image.size();
        let source = LogicalSize::new(source.width as f32, source.height as f32);
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
        let fitted = LogicalSize::new(source.width * scale, source.height * scale);
        LogicalRect::from_xywh(
            bounds.origin.x + (bounds.size.width - fitted.width) * self.alignment.x,
            bounds.origin.y + (bounds.size.height - fitted.height) * self.alignment.y,
            fitted.width,
            fitted.height,
        )
    }
}

impl Element for ImageElement {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(
        &mut self,
        _context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LogicalSize {
        constraints.constrain(self.size)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        let bounds = LogicalRect::from_xywh(0.0, 0.0, size.width, size.height);
        painter.with_save(|painter| {
            painter.clip_rect(bounds)?;
            painter.draw_image(
                &self.image,
                self.destination(bounds),
                ImageOptions {
                    source: None,
                    sampling: self.sampling,
                    opacity: self.opacity.clamp(0.0, 1.0),
                },
            )
        })
    }

    fn accessibility(&self) -> Option<SemanticData> {
        let size = self.image.size();
        Some(SemanticData {
            role: SemanticRole::Image,
            label: self.label.clone(),
            value: Some(format!("{} by {} pixels", size.width, size.height)),
            ..SemanticData::default()
        })
    }
}

/// Content presented by an application render viewport.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderViewContent {
    /// No allocation is available.
    Unavailable,
    /// Registered application-owned texture.
    Image(ExternalImage),
    /// Compositor-managed scene slot.
    Composited {
        /// Stable compositor identity.
        id: CompositorViewId,
        /// Prefer direct frame composition when possible.
        prefer_direct: bool,
    },
    /// Scene rendering failed.
    Error(String),
}

type InputFactory = dyn Fn(UiInput) -> Box<dyn Any>;

/// Interactive application-rendered viewport.
pub struct RenderView {
    /// Accessible label.
    pub label: String,
    /// Preferred size.
    pub size: LogicalSize,
    /// Current render content.
    pub content: RenderViewContent,
    input: Option<Box<InputFactory>>,
}

impl RenderView {
    /// Creates an unavailable viewport.
    pub fn new(label: impl Into<String>, size: LogicalSize) -> Self {
        Self {
            label: label.into(),
            size,
            content: RenderViewContent::Unavailable,
            input: None,
        }
    }

    /// Replaces typed-erased input routing.
    pub fn set_input(&mut self, input: impl Fn(UiInput) -> Box<dyn Any> + 'static) {
        self.input = Some(Box::new(input));
    }
}

impl Element for RenderView {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(
        &mut self,
        _context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LogicalSize {
        constraints.constrain(self.size)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        let bounds = LogicalRect::from_xywh(0.0, 0.0, size.width, size.height);
        match &self.content {
            RenderViewContent::Unavailable => painter.fill_rect(bounds, Brush::Solid(Color::BLACK)),
            RenderViewContent::Image(image) => painter.draw_external_image(
                image,
                bounds,
                ImageOptions {
                    source: None,
                    sampling: ImageSampling::Linear,
                    opacity: 1.0,
                },
            ),
            RenderViewContent::Composited { id, prefer_direct } => {
                painter.compositor_view(*id, bounds, *prefer_direct)
            }
            RenderViewContent::Error(_) => painter.fill_rect(bounds, Brush::Solid(Color::RED)),
        }
    }

    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::RenderView,
            label: self.label.clone(),
            value: match &self.content {
                RenderViewContent::Error(error) => Some(error.clone()),
                _ => None,
            },
            ..SemanticData::default()
        })
    }

    fn event(&mut self, input: UiInput) -> EventResult {
        EventResult {
            action: self.input.as_ref().map(|callback| callback(input)),
            handled: self.input.is_some(),
            ..EventResult::default()
        }
    }

    fn hit_testable(&self) -> bool {
        self.input.is_some()
    }

    fn focusable(&self) -> bool {
        self.input.is_some()
    }
}
