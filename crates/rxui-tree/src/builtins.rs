//! Basic elements used by the research vertical slices.

use std::any::Any;

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
};
use astrelis_paint::{Brush, Painter};
use astrelis_text::{ParagraphStyle, TextLayout, TextLayoutRequest, TextStyle, TextWrap};

use crate::{Constraints, Element, LayoutContext, SemanticData, SemanticRole, ShapingMemo};

/// Main-axis direction for [`Flex`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    /// Children advance from left to right.
    Horizontal,
    /// Children advance from top to bottom.
    #[default]
    Vertical,
}

/// Placement of intrinsic content within all available space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    /// Leading edge on both axes.
    TopLeading,
    /// Centered horizontally at the leading vertical edge.
    Top,
    /// Trailing horizontally at the leading vertical edge.
    TopTrailing,
    /// Leading horizontally and centered vertically.
    Leading,
    /// Centered on both axes.
    #[default]
    Center,
    /// Trailing horizontally and centered vertically.
    Trailing,
    /// Leading horizontally at the trailing vertical edge.
    BottomLeading,
    /// Centered horizontally at the trailing vertical edge.
    Bottom,
    /// Trailing edge on both axes.
    BottomTrailing,
}

/// Simple flex container with deterministic retained child layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Flex {
    /// Main axis.
    pub axis: Axis,
    /// Gap between adjacent children.
    pub gap: f32,
    /// Insets inside the container.
    pub padding: f32,
    /// Optional background fill.
    pub background: Option<Color>,
}

impl Default for Flex {
    fn default() -> Self {
        Self {
            axis: Axis::Vertical,
            gap: 0.0,
            padding: 0.0,
            background: None,
        }
    }
}

impl Element for Flex {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let padding = self.padding.max(0.0);
        let inner_max = LogicalSize::new(
            (constraints.max.width - padding * 2.0).max(0.0),
            (constraints.max.height - padding * 2.0).max(0.0),
        );
        let children = context.children();
        let mut sizes = Vec::with_capacity(children.len());
        let mut total_fixed_main = 0.0f32;
        let mut total_grow = 0.0f32;
        for child in children.iter().copied() {
            let grow = context.child_flex_grow(child);
            if grow > 0.0 {
                // Growing children are laid out exactly once below, against their
                // resolved main-axis share. Measuring them here would only pollute
                // their cached constraints, since `total_fixed_main` ignores them
                // and the growth pass overwrites their placeholder size.
                total_grow += grow;
                sizes.push(LogicalSize::ZERO);
                continue;
            }
            let size = context.layout_child(child, Constraints::new(LogicalSize::ZERO, inner_max));
            total_fixed_main += match self.axis {
                Axis::Horizontal => size.width,
                Axis::Vertical => size.height,
            };
            sizes.push(size);
        }
        let gaps = self.gap.max(0.0) * children.len().saturating_sub(1) as f32;
        let available_main = match self.axis {
            Axis::Horizontal => inner_max.width,
            Axis::Vertical => inner_max.height,
        };
        let remaining = (available_main - total_fixed_main - gaps).max(0.0);
        if total_grow > 0.0 {
            for (index, child) in children.iter().copied().enumerate() {
                let grow = context.child_flex_grow(child);
                if grow <= 0.0 {
                    continue;
                }
                let target_main = remaining * grow / total_grow;
                let child_constraints = match self.axis {
                    Axis::Horizontal => Constraints::new(
                        LogicalSize::new(target_main, inner_max.height),
                        LogicalSize::new(target_main, inner_max.height),
                    ),
                    Axis::Vertical => Constraints::new(
                        LogicalSize::new(inner_max.width, target_main),
                        LogicalSize::new(inner_max.width, target_main),
                    ),
                };
                sizes[index] = context.layout_child(child, child_constraints);
            }
        }
        let mut cursor = padding;
        let mut cross = 0.0f32;
        for (child, size) in children.iter().copied().zip(sizes) {
            let origin = match self.axis {
                Axis::Horizontal => LogicalPoint::new(cursor, padding),
                Axis::Vertical => LogicalPoint::new(padding, cursor),
            };
            context.place_child(child, origin);
            match self.axis {
                Axis::Horizontal => {
                    cursor += size.width + self.gap.max(0.0);
                    cross = cross.max(size.height);
                }
                Axis::Vertical => {
                    cursor += size.height + self.gap.max(0.0);
                    cross = cross.max(size.width);
                }
            }
        }
        if !children.is_empty() {
            cursor -= self.gap.max(0.0);
        }
        let desired = match self.axis {
            Axis::Horizontal => LogicalSize::new(cursor + padding, cross + padding * 2.0),
            Axis::Vertical => LogicalSize::new(cross + padding * 2.0, cursor + padding),
        };
        constraints.constrain(desired)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        if let Some(color) = self.background {
            painter.fill_rect(
                LogicalRect::from_xywh(0.0, 0.0, size.width, size.height),
                Brush::Solid(color),
            )?;
        }
        Ok(())
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// Overlay container which places every child at the same inset origin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    /// Insets inside the container.
    pub padding: f32,
    /// Optional background fill.
    pub background: Option<Color>,
}

impl Element for Stack {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let padding = self.padding.max(0.0);
        let inner_max = LogicalSize::new(
            (constraints.max.width - padding * 2.0).max(0.0),
            (constraints.max.height - padding * 2.0).max(0.0),
        );
        let mut content = LogicalSize::ZERO;
        for child in context.children() {
            let size = context.layout_child(
                child,
                if context.child_flex_grow(child) > 0.0 {
                    Constraints::tight(inner_max)
                } else {
                    Constraints::new(LogicalSize::ZERO, inner_max)
                },
            );
            context.place_child(child, LogicalPoint::new(padding, padding));
            content.width = content.width.max(size.width);
            content.height = content.height.max(size.height);
        }
        constraints.constrain(LogicalSize::new(
            content.width + padding * 2.0,
            content.height + padding * 2.0,
        ))
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        if let Some(color) = self.background {
            painter.fill_rect(
                LogicalRect::from_xywh(0.0, 0.0, size.width, size.height),
                Brush::Solid(color),
            )?;
        }
        Ok(())
    }
}

/// Expands to the available size and positions one intrinsic child within it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Align {
    /// Child placement.
    pub alignment: Alignment,
    /// Minimum distance from each boundary.
    pub padding: f32,
}

impl Element for Align {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let padding = self.padding.max(0.0);
        let size = constraints.max;
        let available = LogicalSize::new(
            (size.width - padding * 2.0).max(0.0),
            (size.height - padding * 2.0).max(0.0),
        );
        if let Some(child) = context.children().into_iter().next() {
            let child_size =
                context.layout_child(child, Constraints::new(LogicalSize::ZERO, available));
            let remaining_x = (available.width - child_size.width).max(0.0);
            let remaining_y = (available.height - child_size.height).max(0.0);
            let x = match self.alignment {
                Alignment::TopLeading | Alignment::Leading | Alignment::BottomLeading => 0.0,
                Alignment::Top | Alignment::Center | Alignment::Bottom => remaining_x * 0.5,
                Alignment::TopTrailing | Alignment::Trailing | Alignment::BottomTrailing => {
                    remaining_x
                }
            };
            let y = match self.alignment {
                Alignment::TopLeading | Alignment::Top | Alignment::TopTrailing => 0.0,
                Alignment::Leading | Alignment::Center | Alignment::Trailing => remaining_y * 0.5,
                Alignment::BottomLeading | Alignment::Bottom | Alignment::BottomTrailing => {
                    remaining_y
                }
            };
            context.place_child(child, LogicalPoint::new(padding + x, padding + y));
        }
        constraints.constrain(size)
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// Explicit sizing and flex-growth boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Optional preferred width.
    pub width: Option<f32>,
    /// Optional preferred height.
    pub height: Option<f32>,
    /// Minimum accepted size.
    pub min: LogicalSize,
    /// Optional maximum size.
    pub max: Option<LogicalSize>,
    /// Relative main-axis growth in a [`Flex`] parent.
    pub grow: f32,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            min: LogicalSize::ZERO,
            max: None,
            grow: 0.0,
        }
    }
}

impl Element for Frame {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let configured_max = self.max.unwrap_or(constraints.max);
        let maximum = LogicalSize::new(
            configured_max.width.min(constraints.max.width),
            configured_max.height.min(constraints.max.height),
        );
        let minimum = LogicalSize::new(
            self.min.width.max(constraints.min.width).min(maximum.width),
            self.min
                .height
                .max(constraints.min.height)
                .min(maximum.height),
        );
        let children = context.children();
        // Measured content only feeds an axis without an explicit extent, and a
        // pinned axis clamps to one value regardless of the measurement. So when
        // both axes are pinned - by `width`/`height` or by a tight incoming
        // constraint, which is what a growing flex parent supplies - the
        // measuring pass cannot affect `size` and is pure duplicate work.
        let pinned_width = self.width.is_some() || minimum.width == maximum.width;
        let pinned_height = self.height.is_some() || minimum.height == maximum.height;
        let mut content = LogicalSize::ZERO;
        if !pinned_width || !pinned_height {
            for child in children.iter().copied() {
                let size =
                    context.layout_child(child, Constraints::new(LogicalSize::ZERO, maximum));
                content.width = content.width.max(size.width);
                content.height = content.height.max(size.height);
            }
        }
        let size = Constraints::new(minimum, maximum).constrain(LogicalSize::new(
            self.width.unwrap_or(content.width),
            self.height.unwrap_or(content.height),
        ));
        for child in children {
            context.layout_child(child, Constraints::tight(size));
            context.place_child(child, LogicalPoint::ZERO);
        }
        size
    }

    fn flex_grow(&self) -> f32 {
        self.grow.max(0.0)
    }
}

/// Fixed or constraint-filling visual box.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxElement {
    /// Preferred size.
    pub size: LogicalSize,
    /// Background color.
    pub color: Color,
    /// Accessible properties.
    pub semantics: Option<SemanticData>,
    /// Whether pointer input targets this box.
    pub interactive: bool,
}

impl BoxElement {
    /// Creates a colored fixed-size box.
    pub fn new(size: LogicalSize, color: Color) -> Self {
        Self {
            size,
            color,
            semantics: None,
            interactive: false,
        }
    }
}

impl Element for BoxElement {
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
        painter.fill_rect(
            LogicalRect::from_xywh(0.0, 0.0, size.width, size.height),
            Brush::Solid(self.color),
        )?;
        Ok(())
    }

    fn accessibility(&self) -> Option<SemanticData> {
        self.semantics.clone()
    }

    fn hit_testable(&self) -> bool {
        self.interactive
    }
}

/// Retained shaped text label.
#[derive(Clone, Debug)]
pub struct Label {
    /// Display and accessible text.
    pub text: String,
    /// Nominal font size used for intrinsic measurement.
    pub font_size: f32,
    /// Optional paint color for a deterministic placeholder glyph bar.
    pub color: Option<Color>,
    preferred_width: Option<f32>,
    layout: Option<TextLayout>,
    shaped: ShapingMemo,
}

impl Label {
    /// Creates a label.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: 14.0,
            color: Some(Color::WHITE),
            preferred_width: None,
            layout: None,
            shaped: ShapingMemo::default(),
        }
    }

    /// Sets the label's font size.
    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    /// Sets the label's glyph color.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Constrains wrapping and reserves a preferred width.
    pub fn with_width(mut self, width: f32) -> Self {
        self.preferred_width = Some(width.max(0.0));
        self
    }

    /// Changes the optional preferred width.
    pub fn set_width(&mut self, width: Option<f32>) {
        self.preferred_width = width.map(|width| width.max(0.0));
    }

    /// Returns the normalized preferred width.
    pub const fn width(&self) -> Option<f32> {
        self.preferred_width
    }
}

impl Element for Label {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let mut request = TextLayoutRequest::new(self.text.clone());
        request.style = TextStyle {
            size: self.font_size.max(1.0),
            color: self.color.unwrap_or(Color::WHITE),
            ..TextStyle::default()
        };
        let max_width = self
            .preferred_width
            .unwrap_or(constraints.max.width)
            .min(constraints.max.width);
        request.paragraph = ParagraphStyle {
            max_width: Some(max_width),
            wrap: TextWrap::Wrap,
            ..ParagraphStyle::default()
        };
        let layout = self.shaped.shape(context, request);
        let mut desired = layout.size();
        if let Some(width) = self.preferred_width {
            desired.width = width;
        }
        let size = constraints.constrain(desired);
        self.layout = Some(layout);
        size
    }

    fn paint(
        &self,
        painter: &mut Painter,
        _size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        if let Some(layout) = &self.layout {
            painter.draw_text(layout, LogicalPoint::ZERO, 1.0)?;
        }
        Ok(())
    }

    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::Label,
            label: self.text.clone(),
            ..SemanticData::default()
        })
    }
}
