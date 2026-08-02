//! Incremental retained chart surface.

use std::any::Any;

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
};
use astrelis_paint::{Brush, Painter, Path, StrokeStyle};
use astrelis_platform::CursorIcon;
use rxui_core::{CustomElementSpec, Element as CoreElement, RoutedValueHandler, Theme, custom};
use rxui_tree::{
    Constraints, Element, EventResult, Invalidation, LayoutContext, SemanticData, SemanticRole,
    UiInput,
};

use super::canvas::{self, Hover};

/// One chart-domain point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChartPoint {
    /// Horizontal domain coordinate.
    pub x: f64,
    /// Vertical domain coordinate.
    pub y: f64,
}

/// Series rendering mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartSeriesKind {
    /// Connected polyline.
    #[default]
    Line,
    /// Independent point markers.
    Scatter,
}

/// One keyed chart series.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSeries {
    /// Stable series identity.
    pub id: u64,
    /// Accessible name.
    pub name: String,
    /// Series color.
    pub color: Color,
    /// Rendering mode.
    pub kind: ChartSeriesKind,
    /// Domain points.
    pub points: Vec<ChartPoint>,
}

/// Chart presentation policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartOptions {
    /// Preferred logical size.
    pub size: LogicalSize,
    /// Surface fill.
    pub background: Color,
    /// Line and marker width.
    pub stroke_width: f32,
}

impl Default for ChartOptions {
    fn default() -> Self {
        Self {
            size: LogicalSize::new(640.0, 320.0),
            background: canvas::surface_fill(),
            stroke_width: 1.5,
        }
    }
}

/// Chart interaction emitted by the retained surface.
#[derive(Clone, Debug, PartialEq)]
pub enum ChartAction {
    /// Nearest point was selected.
    Select {
        /// Series identity.
        series: u64,
        /// Point index.
        point: usize,
    },
    /// Selection was cleared.
    Clear,
}

/// Retained chart implementation exposed for custom-element integration.
#[doc(hidden)]
pub struct ChartElement {
    series: Vec<ChartSeries>,
    options: ChartOptions,
    size: LogicalSize,
    hovered: Hover<(u64, usize)>,
    on_action: RoutedValueHandler<ChartAction>,
}

#[derive(Clone, Copy)]
struct DomainBounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

fn project_point(point: ChartPoint, bounds: DomainBounds, size: LogicalSize) -> LogicalPoint {
    let x_span = (bounds.max_x - bounds.min_x).max(f64::EPSILON);
    let y_span = (bounds.max_y - bounds.min_y).max(f64::EPSILON);
    let inset = 12.0f32.min(size.width * 0.1).min(size.height * 0.1);
    let width = (size.width - inset * 2.0).max(0.0);
    let height = (size.height - inset * 2.0).max(0.0);
    LogicalPoint::new(
        inset + ((point.x - bounds.min_x) / x_span) as f32 * width,
        inset + height - ((point.y - bounds.min_y) / y_span) as f32 * height,
    )
}

impl ChartElement {
    fn bounds(&self) -> Option<DomainBounds> {
        let mut points = self.series.iter().flat_map(|series| &series.points);
        let first = *points.next()?;
        Some(points.fold(
            DomainBounds {
                min_x: first.x,
                max_x: first.x,
                min_y: first.y,
                max_y: first.y,
            },
            |bounds, point| DomainBounds {
                min_x: bounds.min_x.min(point.x),
                max_x: bounds.max_x.max(point.x),
                min_y: bounds.min_y.min(point.y),
                max_y: bounds.max_y.max(point.y),
            },
        ))
    }

    fn project(&self, point: ChartPoint) -> LogicalPoint {
        self.bounds().map_or(LogicalPoint::ZERO, |bounds| {
            project_point(point, bounds, self.size)
        })
    }

    fn nearest(&self, position: LogicalPoint) -> Option<(u64, usize)> {
        self.series
            .iter()
            .flat_map(|series| {
                series.points.iter().enumerate().map(move |(index, point)| {
                    let projected = self.project(*point);
                    let dx = projected.x - position.x;
                    let dy = projected.y - position.y;
                    (dx * dx + dy * dy, series.id, index)
                })
            })
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .filter(|(distance, _, _)| *distance <= 144.0)
            .map(|(_, series, point)| (series, point))
    }
}

impl Element for ChartElement {
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
        self.size = constraints.constrain(self.options.size);
        self.size
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        canvas::draw(painter, size, self.options.background, |painter| {
            for series in &self.series {
                match series.kind {
                    ChartSeriesKind::Line if series.points.len() > 1 => {
                        let mut builder = Path::builder();
                        builder.move_to(self.project(series.points[0]))?;
                        for point in &series.points[1..] {
                            builder.line_to(self.project(*point))?;
                        }
                        painter.stroke_path(
                            &builder.finish(),
                            StrokeStyle {
                                width: self.options.stroke_width.max(0.5),
                                ..StrokeStyle::default()
                            },
                            Brush::Solid(series.color),
                        )?;
                    }
                    _ => {
                        for point in &series.points {
                            let point = self.project(*point);
                            painter.fill_rect(
                                LogicalRect::from_xywh(point.x - 2.0, point.y - 2.0, 4.0, 4.0),
                                Brush::Solid(series.color),
                            )?;
                        }
                    }
                }
            }
            if let Some(&(series_id, point_index)) = self.hovered.current()
                && let Some(point) = self
                    .series
                    .iter()
                    .find(|series| series.id == series_id)
                    .and_then(|series| series.points.get(point_index))
            {
                let point = self.project(*point);
                painter.fill_rect(
                    LogicalRect::from_xywh(point.x - 4.0, point.y - 4.0, 8.0, 8.0),
                    Brush::Solid(Color::WHITE),
                )?;
            }
            Ok(())
        })
    }

    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::Chart,
            label: "Chart".into(),
            value: Some(format!("{} series", self.series.len())),
            ..SemanticData::default()
        })
    }

    fn event(&mut self, input: UiInput) -> EventResult {
        match input {
            UiInput::HoverChanged(false) => EventResult {
                invalidation: self.hovered.set(None),
                handled: true,
                ..EventResult::default()
            },
            UiInput::PointerMoved(position) => EventResult {
                invalidation: self.hovered.set(self.nearest(position)),
                handled: true,
                ..EventResult::default()
            },
            UiInput::PointerReleased(position) => {
                let action =
                    self.nearest(position)
                        .map_or(ChartAction::Clear, |(series, point)| ChartAction::Select {
                            series,
                            point,
                        });
                EventResult::action(self.on_action.with(action))
            }
            _ => EventResult::default(),
        }
    }

    fn hit_testable(&self) -> bool {
        true
    }
    fn cursor_icon(&self) -> CursorIcon {
        CursorIcon::Crosshair
    }
}

/// Reconciled chart configuration.
#[derive(Clone)]
pub struct ChartSpec {
    /// Keyed series.
    pub series: Vec<ChartSeries>,
    /// Presentation policy.
    pub options: ChartOptions,
    on_action: RoutedValueHandler<ChartAction>,
}

impl ChartSpec {
    /// Creates a chart specification routed to its owning entity.
    pub fn new(series: Vec<ChartSeries>, on_action: RoutedValueHandler<ChartAction>) -> Self {
        Self {
            series,
            options: ChartOptions::default(),
            on_action,
        }
    }

    /// Selects presentation policy.
    pub const fn options(mut self, options: ChartOptions) -> Self {
        self.options = options;
        self
    }
}

impl CustomElementSpec for ChartSpec {
    type Element = ChartElement;

    fn create(&self, _theme: &Theme) -> Self::Element {
        ChartElement {
            series: self.series.clone(),
            options: self.options,
            size: LogicalSize::ZERO,
            hovered: Hover::default(),
            on_action: self.on_action.clone(),
        }
    }

    fn update(&self, element: &mut Self::Element, _theme: &Theme) {
        element.series.clone_from(&self.series);
        element.options = self.options;
        element.on_action = self.on_action.clone();
    }

    fn changed(&self, previous: &Self) -> Invalidation {
        let mut invalidation = Invalidation::empty();
        if self.options.size != previous.options.size {
            invalidation |= Invalidation::LAYOUT;
        }
        if self.series.len() != previous.series.len() {
            invalidation |= Invalidation::ACCESSIBILITY;
        }
        if self.series != previous.series || self.options != previous.options {
            invalidation |= Invalidation::PAINT;
        }
        invalidation
    }
}

/// Builds an incremental retained chart.
pub fn chart(spec: ChartSpec) -> CoreElement {
    custom(spec)
}

/// Selects representative line points while preserving both endpoints.
pub fn decimate_line(points: &[ChartPoint], target: usize) -> Vec<usize> {
    if target >= points.len() {
        return (0..points.len()).collect();
    }
    if target < 2 {
        return (0..target).collect();
    }
    let step = (points.len() - 1) as f64 / (target - 1) as f64;
    (0..target)
        .map(|index| ((index as f64 * step).round() as usize).min(points.len() - 1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimation_keeps_endpoints() {
        let points = (0..10)
            .map(|x| ChartPoint {
                x: x as f64,
                y: 0.0,
            })
            .collect::<Vec<_>>();
        assert_eq!(decimate_line(&points, 4), vec![0, 3, 6, 9]);
    }
}
