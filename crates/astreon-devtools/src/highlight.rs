//! Box-model highlight overlay painted above the inspected interface.

use astrelis_core::{
    color::Color,
    geometry::{LogicalRect, Point, Size},
};
use astrelis_paint::{Brush, Painter, StrokeStyle};
use astrelis_ui::widget_any;
use astrelis_ui_core::{
    ElementInspection, Insets, Theme, UiError, Widget, WidgetContainerStyle,
};

/// Chrome-style band tints; translucent so they read on any app surface.
fn margin_tint() -> Color {
    Color::from_hex_alpha(0xf3ad6b59)
}
fn padding_tint() -> Color {
    Color::from_hex_alpha(0x93c76359)
}
fn content_tint() -> Color {
    Color::from_hex_alpha(0x6aa9d840)
}
fn selection_stroke() -> Color {
    Color::from_hex(0x4c8dff)
}
fn hover_tint() -> Color {
    Color::from_hex_alpha(0x4c8dff26)
}
fn hover_stroke() -> Color {
    Color::from_hex(0x7fb0ff)
}

/// Margin, border-box, and content rectangles of the selected element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BandSet {
    /// Border box grown by the resolved margin.
    pub margin: LogicalRect,
    /// The element's border box.
    pub bounds: LogicalRect,
    /// Border box shrunk by resolved border and padding.
    pub content: LogicalRect,
}

impl BandSet {
    /// Derives highlight bands from one inspection node, clipped when needed.
    pub fn of(node: &ElementInspection) -> Self {
        let bounds = clipped_bounds(node);
        let inner = shrink(bounds, node.resolved_border);
        Self {
            margin: clip(grow(node.world_bounds, node.resolved_margin), node.clip),
            bounds,
            content: shrink(inner, node.resolved_padding),
        }
    }
}

/// World bounds intersected with the effective clip.
pub(crate) fn clipped_bounds(node: &ElementInspection) -> LogicalRect {
    clip(node.world_bounds, node.clip)
}

fn clip(rect: LogicalRect, clip: Option<LogicalRect>) -> LogicalRect {
    clip.and_then(|clip| intersect(rect, clip)).unwrap_or(rect)
}

fn grow(rect: LogicalRect, insets: Insets) -> LogicalRect {
    LogicalRect::new(
        Point::new(rect.origin.x - insets.left, rect.origin.y - insets.top),
        Size::new(
            rect.size.width + insets.left + insets.right,
            rect.size.height + insets.top + insets.bottom,
        ),
    )
}

fn shrink(rect: LogicalRect, insets: Insets) -> LogicalRect {
    LogicalRect::new(
        Point::new(rect.origin.x + insets.left, rect.origin.y + insets.top),
        Size::new(
            (rect.size.width - insets.left - insets.right).max(0.0),
            (rect.size.height - insets.top - insets.bottom).max(0.0),
        ),
    )
}

pub(crate) fn intersect(left: LogicalRect, right: LogicalRect) -> Option<LogicalRect> {
    let x = left.origin.x.max(right.origin.x);
    let y = left.origin.y.max(right.origin.y);
    let max_x = (left.origin.x + left.size.width).min(right.origin.x + right.size.width);
    let max_y = (left.origin.y + left.size.height).min(right.origin.y + right.size.height);
    (max_x >= x && max_y >= y)
        .then(|| LogicalRect::new(Point::new(x, y), Size::new(max_x - x, max_y - y)))
}

/// Fills the frame between an outer and an inner rectangle with four strips.
fn fill_ring(
    painter: &mut Painter,
    outer: LogicalRect,
    inner: LogicalRect,
    color: Color,
) -> Result<(), UiError> {
    let Some(inner) = intersect(outer, inner) else {
        painter.fill_rect(outer, Brush::Solid(color))?;
        return Ok(());
    };
    let outer_max_x = outer.origin.x + outer.size.width;
    let outer_max_y = outer.origin.y + outer.size.height;
    let inner_max_x = inner.origin.x + inner.size.width;
    let inner_max_y = inner.origin.y + inner.size.height;
    let strips = [
        // Top and bottom span the full outer width.
        LogicalRect::new(
            outer.origin,
            Size::new(outer.size.width, inner.origin.y - outer.origin.y),
        ),
        LogicalRect::new(
            Point::new(outer.origin.x, inner_max_y),
            Size::new(outer.size.width, outer_max_y - inner_max_y),
        ),
        // Left and right fill the remaining middle band.
        LogicalRect::new(
            Point::new(outer.origin.x, inner.origin.y),
            Size::new(inner.origin.x - outer.origin.x, inner.size.height),
        ),
        LogicalRect::new(
            Point::new(inner_max_x, inner.origin.y),
            Size::new(outer_max_x - inner_max_x, inner.size.height),
        ),
    ];
    for strip in strips {
        if strip.size.width > 0.0 && strip.size.height > 0.0 {
            painter.fill_rect(strip, Brush::Solid(color))?;
        }
    }
    Ok(())
}

/// Overlay widget painting the committed selection and the hover preview.
#[derive(Default)]
pub(crate) struct Highlight {
    pub selection: Option<BandSet>,
    pub hover: Option<LogicalRect>,
}

impl<Message: 'static> Widget<Message> for Highlight {
    widget_any!();

    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle::structural()
    }

    fn paint(
        &self,
        painter: &mut Painter,
        _bounds: LogicalRect,
        _theme: &Theme,
    ) -> Result<(), UiError> {
        if let Some(bands) = &self.selection {
            fill_ring(painter, bands.margin, bands.bounds, margin_tint())?;
            fill_ring(painter, bands.bounds, bands.content, padding_tint())?;
            painter.fill_rect(bands.content, Brush::Solid(content_tint()))?;
            painter.stroke_rect(
                bands.bounds,
                StrokeStyle {
                    width: 1.5,
                    ..StrokeStyle::default()
                },
                Brush::Solid(selection_stroke()),
            )?;
        }
        if let Some(rect) = self.hover {
            painter.fill_rect(rect, Brush::Solid(hover_tint()))?;
            painter.stroke_rect(
                rect,
                StrokeStyle {
                    width: 1.0,
                    ..StrokeStyle::default()
                },
                Brush::Solid(hover_stroke()),
            )?;
        }
        Ok(())
    }
}
