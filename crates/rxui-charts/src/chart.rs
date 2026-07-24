//! Incremental retained chart surface.

use std::{any::Any, sync::Arc};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
};
use astrelis_paint::{Brush, Painter, Path, StrokeStyle};
use astrelis_platform::CursorIcon;
use astrelis_ui_next::{
    Constraints, Element, EventResult, Invalidation, LayoutContext, SemanticData, SemanticRole,
    UiError, UiInput,
};

use rxui_core::{ActionEmitter, RetainedSpec, View, retained};

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
            background: Color::from_hex(0x16181d),
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

/// Retained chart implementation exposed only for [`RetainedSpec`] integration.
#[doc(hidden)]
pub struct ChartElement<Action: 'static> {
    series: Vec<ChartSeries>,
    options: ChartOptions,
    size: LogicalSize,
    hovered: Option<(u64, usize)>,
    emitter: ActionEmitter<Action>,
    map_action: Arc<dyn Fn(ChartAction) -> Action>,
}

impl<Action: 'static> ChartElement<Action> {
    fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut points = self.series.iter().flat_map(|series| &series.points);
        let first = *points.next()?;
        Some(points.fold(
            (first.x, first.x, first.y, first.y),
            |(min_x, max_x, min_y, max_y), point| {
                (
                    min_x.min(point.x),
                    max_x.max(point.x),
                    min_y.min(point.y),
                    max_y.max(point.y),
                )
            },
        ))
    }

    fn project(&self, point: ChartPoint) -> LogicalPoint {
        let Some((min_x, max_x, min_y, max_y)) = self.bounds() else {
            return LogicalPoint::ZERO;
        };
        let x_span = (max_x - min_x).max(f64::EPSILON);
        let y_span = (max_y - min_y).max(f64::EPSILON);
        let inset = 12.0f32
            .min(self.size.width * 0.1)
            .min(self.size.height * 0.1);
        let width = (self.size.width - inset * 2.0).max(0.0);
        let height = (self.size.height - inset * 2.0).max(0.0);
        LogicalPoint::new(
            inset + ((point.x - min_x) / x_span) as f32 * width,
            inset + height - ((point.y - min_y) / y_span) as f32 * height,
        )
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

impl<Action: 'static> Element for ChartElement<Action> {
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
        self.size = constraints.constrain(self.options.size);
        Ok(self.size)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        painter.with_save(|painter| {
            painter.clip_rect(LogicalRect::from_xywh(0.0, 0.0, size.width, size.height))?;
            painter.fill_rect(
                LogicalRect::from_xywh(0.0, 0.0, size.width, size.height),
                Brush::Solid(self.options.background),
            )?;
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
            if let Some((series_id, point_index)) = self.hovered
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
            UiInput::HoverChanged(false) => {
                let changed = self.hovered.take().is_some();
                EventResult {
                    invalidation: if changed {
                        Invalidation::PAINT
                    } else {
                        Invalidation::empty()
                    },
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::PointerMoved(position) => {
                let hovered = self.nearest(position);
                let changed = hovered != self.hovered;
                self.hovered = hovered;
                EventResult {
                    invalidation: if changed {
                        Invalidation::PAINT
                    } else {
                        Invalidation::empty()
                    },
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::PointerReleased(position) => {
                let action =
                    self.nearest(position)
                        .map_or(ChartAction::Clear, |(series, point)| ChartAction::Select {
                            series,
                            point,
                        });
                EventResult {
                    action: Some(self.emitter.emit((self.map_action)(action))),
                    handled: true,
                    ..EventResult::default()
                }
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
pub struct ChartSpec<Action: 'static> {
    /// Keyed series.
    pub series: Vec<ChartSeries>,
    /// Presentation policy.
    pub options: ChartOptions,
    map_action: Arc<dyn Fn(ChartAction) -> Action>,
}

impl<Action: 'static> Clone for ChartSpec<Action> {
    fn clone(&self) -> Self {
        Self {
            series: self.series.clone(),
            options: self.options,
            map_action: self.map_action.clone(),
        }
    }
}

impl<Action: 'static> ChartSpec<Action> {
    /// Creates a chart specification.
    pub fn new(
        series: Vec<ChartSeries>,
        map_action: impl Fn(ChartAction) -> Action + 'static,
    ) -> Self {
        Self {
            series,
            options: ChartOptions::default(),
            map_action: Arc::new(map_action),
        }
    }

    /// Selects presentation policy.
    pub const fn options(mut self, options: ChartOptions) -> Self {
        self.options = options;
        self
    }
}

impl<Action: 'static> RetainedSpec<Action> for ChartSpec<Action> {
    type Element = ChartElement<Action>;

    fn create(&self, emitter: ActionEmitter<Action>) -> Self::Element {
        ChartElement {
            series: self.series.clone(),
            options: self.options,
            size: LogicalSize::ZERO,
            hovered: None,
            emitter,
            map_action: self.map_action.clone(),
        }
    }

    fn update(&self, element: &mut Self::Element, emitter: ActionEmitter<Action>) {
        element.series.clone_from(&self.series);
        element.options = self.options;
        element.emitter = emitter;
        element.map_action = self.map_action.clone();
    }

    fn changed(&self, previous: &Self) -> bool {
        self.series != previous.series || self.options != previous.options
    }
}

/// Builds an incremental retained chart.
pub fn chart<Action: 'static>(spec: ChartSpec<Action>) -> View<Action> {
    retained(spec)
}

/// Selects representative line points while preserving both endpoints.
pub fn decimate_line(points: &[ChartPoint], target: usize) -> Vec<usize> {
    if target >= points.len() {
        return (0..points.len()).collect();
    }
    if target < 2 || points.is_empty() {
        return (0..target.min(points.len())).collect();
    }
    let step = (points.len() - 1) as f64 / (target - 1) as f64;
    (0..target)
        .map(|index| ((index as f64 * step).round() as usize).min(points.len() - 1))
        .collect()
}
