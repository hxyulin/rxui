//! Validated vector icons rendered through retained paint fragments.

use std::{any::Any, error::Error, fmt};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
    math::{Affine2, Vec2},
};
use astrelis_paint::{Brush, FillRule, Painter, Path, PathVerb};
use astrelis_ui_next::{Constraints, Element, LayoutContext, SemanticData, SemanticRole, UiError};

use crate::{ActionEmitter, RetainedSpec, View, retained};

/// Validated immutable monochrome vector icon.
#[derive(Clone, Debug)]
pub struct Icon {
    view_box: LogicalSize,
    path: Path,
}

impl Icon {
    /// Creates an icon from an immutable paint path.
    pub fn new(view_box: LogicalSize, path: Path) -> Result<Self, IconError> {
        if !view_box.width.is_finite()
            || !view_box.height.is_finite()
            || view_box.width <= 0.0
            || view_box.height <= 0.0
        {
            return Err(IconError::new(
                "icon view-box dimensions must be finite and positive",
            ));
        }
        if path.is_empty() {
            return Err(IconError::new("icon paths cannot be empty"));
        }
        Ok(Self { view_box, path })
    }

    /// Builds an icon from path verbs.
    pub fn from_verbs(
        view_box: LogicalSize,
        verbs: impl IntoIterator<Item = PathVerb>,
    ) -> Result<Self, IconError> {
        let mut builder = Path::builder();
        for verb in verbs {
            match verb {
                PathVerb::MoveTo(point) => builder.move_to(point),
                PathVerb::LineTo(point) => builder.line_to(point),
                PathVerb::QuadTo(control, point) => builder.quad_to(control, point),
                PathVerb::CubicTo(first, second, point) => builder.cubic_to(first, second, point),
                PathVerb::Close => builder.close(),
            }
            .map_err(IconError::new)?;
        }
        Self::new(view_box, builder.finish())
    }

    /// Returns the icon coordinate system.
    pub const fn view_box(&self) -> LogicalSize {
        self.view_box
    }

    /// Returns the immutable vector path.
    pub const fn path(&self) -> &Path {
        &self.path
    }
}

/// Invalid custom vector icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconError(String);

impl IconError {
    fn new(message: impl fmt::Display) -> Self {
        Self(message.to_string())
    }
}

impl fmt::Display for IconError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for IconError {}

/// Reconciled vector-icon presentation.
#[derive(Clone)]
pub struct IconSpec {
    /// Immutable vector source.
    pub icon: Icon,
    /// Logical square edge.
    pub size: f32,
    /// Monochrome fill.
    pub color: Color,
    /// Optional accessible image label.
    pub label: Option<String>,
}

impl IconSpec {
    /// Creates an unlabeled 16px icon.
    pub fn new(icon: Icon) -> Self {
        Self {
            icon,
            size: 16.0,
            color: Color::WHITE,
            label: None,
        }
    }

    /// Selects logical icon size.
    pub const fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Selects monochrome fill.
    pub const fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Adds an accessible image label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// Retained vector icon used by [`IconSpec`].
#[doc(hidden)]
pub struct IconElement {
    icon: Icon,
    size: f32,
    color: Color,
    label: Option<String>,
}

impl Element for IconElement {
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
    ) -> Result<LogicalSize, UiError> {
        let size = if self.size.is_finite() {
            self.size.max(1.0)
        } else {
            16.0
        };
        Ok(constraints.constrain(LogicalSize::new(size, size)))
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        let scale = (size.width / self.icon.view_box.width)
            .min(size.height / self.icon.view_box.height)
            .max(0.0);
        let offset = LogicalPoint::new(
            (size.width - self.icon.view_box.width * scale) * 0.5,
            (size.height - self.icon.view_box.height * scale) * 0.5,
        );
        painter.with_save(|painter| {
            painter.transform(
                Affine2::from_translation(Vec2::new(offset.x, offset.y))
                    * Affine2::from_scale(Vec2::splat(scale)),
            )?;
            painter.fill_path(&self.icon.path, FillRule::NonZero, Brush::Solid(self.color))
        })
    }

    fn accessibility(&self) -> Option<SemanticData> {
        self.label.as_ref().map(|label| SemanticData {
            role: SemanticRole::Image,
            label: label.clone(),
            ..SemanticData::default()
        })
    }
}

impl<Action: 'static> RetainedSpec<Action> for IconSpec {
    type Element = IconElement;

    fn create(&self, _emitter: ActionEmitter<Action>) -> Self::Element {
        IconElement {
            icon: self.icon.clone(),
            size: self.size,
            color: self.color,
            label: self.label.clone(),
        }
    }

    fn update(&self, element: &mut Self::Element, _emitter: ActionEmitter<Action>) {
        element.icon = self.icon.clone();
        element.size = self.size;
        element.color = self.color;
        element.label.clone_from(&self.label);
    }

    fn changed(&self, previous: &Self) -> bool {
        self.icon.path.cache_id() != previous.icon.path.cache_id()
            || self.icon.view_box != previous.icon.view_box
            || self.size != previous.size
            || self.color != previous.color
            || self.label != previous.label
    }
}

/// Builds a retained vector icon.
pub fn icon<Action: 'static>(spec: IconSpec) -> View<Action> {
    retained(spec)
}

/// Common editor glyphs.
pub mod icons {
    use astrelis_core::geometry::{LogicalPoint, LogicalSize};
    use astrelis_paint::{Path, PathVerb};

    use super::Icon;

    fn icon(verbs: impl IntoIterator<Item = PathVerb>) -> Icon {
        Icon::from_verbs(LogicalSize::new(24.0, 24.0), verbs)
            .expect("built-in icon paths are valid")
    }

    /// Check mark.
    pub fn check() -> Icon {
        icon([
            PathVerb::MoveTo(LogicalPoint::new(4.0, 12.5)),
            PathVerb::LineTo(LogicalPoint::new(9.5, 18.0)),
            PathVerb::LineTo(LogicalPoint::new(20.0, 6.0)),
            PathVerb::LineTo(LogicalPoint::new(18.2, 4.5)),
            PathVerb::LineTo(LogicalPoint::new(9.3, 14.8)),
            PathVerb::LineTo(LogicalPoint::new(5.8, 11.0)),
            PathVerb::Close,
        ])
    }

    /// Downward chevron.
    pub fn chevron_down() -> Icon {
        icon([
            PathVerb::MoveTo(LogicalPoint::new(5.0, 8.0)),
            PathVerb::LineTo(LogicalPoint::new(12.0, 15.0)),
            PathVerb::LineTo(LogicalPoint::new(19.0, 8.0)),
            PathVerb::LineTo(LogicalPoint::new(17.0, 6.0)),
            PathVerb::LineTo(LogicalPoint::new(12.0, 11.0)),
            PathVerb::LineTo(LogicalPoint::new(7.0, 6.0)),
            PathVerb::Close,
        ])
    }

    /// Close glyph.
    pub fn close() -> Icon {
        icon([
            PathVerb::MoveTo(LogicalPoint::new(5.5, 4.0)),
            PathVerb::LineTo(LogicalPoint::new(12.0, 10.5)),
            PathVerb::LineTo(LogicalPoint::new(18.5, 4.0)),
            PathVerb::LineTo(LogicalPoint::new(20.0, 5.5)),
            PathVerb::LineTo(LogicalPoint::new(13.5, 12.0)),
            PathVerb::LineTo(LogicalPoint::new(20.0, 18.5)),
            PathVerb::LineTo(LogicalPoint::new(18.5, 20.0)),
            PathVerb::LineTo(LogicalPoint::new(12.0, 13.5)),
            PathVerb::LineTo(LogicalPoint::new(5.5, 20.0)),
            PathVerb::LineTo(LogicalPoint::new(4.0, 18.5)),
            PathVerb::LineTo(LogicalPoint::new(10.5, 12.0)),
            PathVerb::LineTo(LogicalPoint::new(4.0, 5.5)),
            PathVerb::Close,
        ])
    }

    /// Search glyph.
    pub fn search() -> Icon {
        let mut path = Path::builder();
        path.move_to(LogicalPoint::new(10.0, 3.0)).unwrap();
        path.cubic_to(
            LogicalPoint::new(6.1, 3.0),
            LogicalPoint::new(3.0, 6.1),
            LogicalPoint::new(3.0, 10.0),
        )
        .unwrap();
        path.cubic_to(
            LogicalPoint::new(3.0, 13.9),
            LogicalPoint::new(6.1, 17.0),
            LogicalPoint::new(10.0, 17.0),
        )
        .unwrap();
        path.cubic_to(
            LogicalPoint::new(11.7, 17.0),
            LogicalPoint::new(13.3, 16.4),
            LogicalPoint::new(14.5, 15.5),
        )
        .unwrap();
        path.line_to(LogicalPoint::new(20.0, 21.0)).unwrap();
        path.line_to(LogicalPoint::new(21.0, 20.0)).unwrap();
        path.line_to(LogicalPoint::new(15.5, 14.5)).unwrap();
        path.cubic_to(
            LogicalPoint::new(16.4, 13.3),
            LogicalPoint::new(17.0, 11.7),
            LogicalPoint::new(17.0, 10.0),
        )
        .unwrap();
        path.cubic_to(
            LogicalPoint::new(17.0, 6.1),
            LogicalPoint::new(13.9, 3.0),
            LogicalPoint::new(10.0, 3.0),
        )
        .unwrap();
        path.close().unwrap();
        Icon::new(LogicalSize::new(24.0, 24.0), path.finish())
            .expect("built-in icon paths are valid")
    }
}
