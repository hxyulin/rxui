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

/// Domain extent covered by every plotted series.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DomainBounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

/// Maps one domain point into inset surface coordinates.
///
/// Kept free of element state so projection can be exercised directly for
/// degenerate domains and surfaces smaller than the nominal inset.
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

impl<Action: 'static> ChartElement<Action> {
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
        let Some(bounds) = self.bounds() else {
            return LogicalPoint::ZERO;
        };
        project_point(point, bounds, self.size)
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

    fn create(&self, emitter: &ActionEmitter<Action>, _theme: &rxui_core::Theme) -> Self::Element {
        ChartElement {
            series: self.series.clone(),
            options: self.options,
            size: LogicalSize::ZERO,
            hovered: None,
            emitter: emitter.clone(),
            map_action: self.map_action.clone(),
        }
    }

    fn update(
        &self,
        element: &mut Self::Element,
        emitter: &ActionEmitter<Action>,
        _theme: &rxui_core::Theme,
    ) {
        element.series.clone_from(&self.series);
        element.options = self.options;
        element.emitter = emitter.clone();
        element.map_action = self.map_action.clone();
    }

    /// Reports the passes each field actually feeds.
    ///
    /// The chart's one layout input is `options.size`: `layout` constrains it
    /// and caches the result, and that is the whole of the pass. The domain
    /// extents are *not* a layout product, however much they look like axis
    /// geometry - `bounds` recomputes them from `series` inside `paint` on every
    /// frame - so new data is a repaint, not a re-measure. `accessibility`
    /// announces the series count and nothing finer, so it only cares when the
    /// count moves.
    ///
    /// Hover and selection do not appear here at all. They live in the element
    /// as `hovered`, are driven by `event`, and are reported through
    /// `EventResult::invalidation`, which already asks for `PAINT` alone. No
    /// controlled hover field would improve on that, and adding one would put
    /// the pointer's position through the component reducer.
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
pub fn chart<Action: 'static>(spec: ChartSpec<Action>) -> View<Action> {
    retained(spec)
}

/// Selects representative line points while preserving both endpoints.
///
/// Returns indices into `points`, ascending, never longer than `target`. An
/// empty input needs no special case: `target >= points.len()` already covers
/// it and returns the empty index list.
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

    fn points(count: usize) -> Vec<ChartPoint> {
        (0..count)
            .map(|index| ChartPoint {
                x: index as f64,
                y: index as f64,
            })
            .collect()
    }

    #[test]
    fn decimate_line_keeps_every_point_when_target_is_not_smaller() {
        let points = points(4);
        assert_eq!(decimate_line(&points, 4), vec![0, 1, 2, 3]);
        assert_eq!(decimate_line(&points, 9), vec![0, 1, 2, 3]);
    }

    #[test]
    fn decimate_line_handles_empty_input_for_every_target() {
        // The `target >= points.len()` branch absorbs the empty case, so the
        // former `points.is_empty()` guard was unreachable.
        for target in [0, 1, 2, 7] {
            assert!(decimate_line(&[], target).is_empty(), "target {target}");
        }
    }

    #[test]
    fn decimate_line_degenerates_below_two_targets() {
        let points = points(8);
        assert!(decimate_line(&points, 0).is_empty());
        assert_eq!(decimate_line(&points, 1), vec![0]);
    }

    #[test]
    fn decimate_line_preserves_both_endpoints() {
        let points = points(97);
        for target in 2..=32 {
            let selected = decimate_line(&points, target);
            assert_eq!(selected.len(), target, "target {target}");
            assert_eq!(selected.first().copied(), Some(0), "target {target}");
            assert_eq!(selected.last().copied(), Some(96), "target {target}");
            assert!(
                selected.windows(2).all(|pair| pair[0] < pair[1]),
                "target {target} produced {selected:?}"
            );
        }
    }

    #[test]
    fn decimate_line_spaces_indices_evenly() {
        assert_eq!(decimate_line(&points(10), 4), vec![0, 3, 6, 9]);
        assert_eq!(decimate_line(&points(5), 3), vec![0, 2, 4]);
    }

    #[test]
    fn decimate_line_repeats_no_index_when_target_is_one_below_the_count() {
        // Rounding is the only thing keeping this injective; a duplicate index
        // would silently drop a point from the decimated polyline.
        for count in 3..64 {
            let selected = decimate_line(&points(count), count - 1);
            assert!(
                selected.windows(2).all(|pair| pair[0] < pair[1]),
                "count {count} produced {selected:?}"
            );
        }
    }

    const UNIT_BOUNDS: DomainBounds = DomainBounds {
        min_x: 0.0,
        max_x: 10.0,
        min_y: 0.0,
        max_y: 10.0,
    };

    #[test]
    fn project_point_maps_the_domain_corners_to_the_inset_box() {
        let size = LogicalSize::new(200.0, 100.0);
        let origin = project_point(ChartPoint { x: 0.0, y: 0.0 }, UNIT_BOUNDS, size);
        let opposite = project_point(ChartPoint { x: 10.0, y: 10.0 }, UNIT_BOUNDS, size);
        assert_eq!(origin, LogicalPoint::new(10.0, 90.0));
        assert_eq!(opposite, LogicalPoint::new(190.0, 10.0));
    }

    #[test]
    fn project_point_inverts_the_vertical_axis() {
        let size = LogicalSize::new(200.0, 100.0);
        let low = project_point(ChartPoint { x: 5.0, y: 2.0 }, UNIT_BOUNDS, size);
        let high = project_point(ChartPoint { x: 5.0, y: 8.0 }, UNIT_BOUNDS, size);
        assert!(high.y < low.y);
        assert_eq!(low.x, high.x);
    }

    #[test]
    fn project_point_stays_finite_for_a_degenerate_domain() {
        let bounds = DomainBounds {
            min_x: 3.0,
            max_x: 3.0,
            min_y: -1.0,
            max_y: -1.0,
        };
        let projected = project_point(
            ChartPoint { x: 3.0, y: -1.0 },
            bounds,
            LogicalSize::new(200.0, 100.0),
        );
        assert!(projected.x.is_finite() && projected.y.is_finite());
        assert_eq!(projected, LogicalPoint::new(10.0, 90.0));
    }

    #[test]
    fn project_point_shrinks_the_inset_on_small_surfaces() {
        let size = LogicalSize::new(40.0, 20.0);
        // The inset is 10% of the smaller side, not the nominal 12 logical
        // pixels, so the plot area never collapses to zero width.
        let origin = project_point(ChartPoint { x: 0.0, y: 0.0 }, UNIT_BOUNDS, size);
        let opposite = project_point(ChartPoint { x: 10.0, y: 10.0 }, UNIT_BOUNDS, size);
        assert_eq!(origin, LogicalPoint::new(2.0, 18.0));
        assert_eq!(opposite, LogicalPoint::new(38.0, 2.0));
    }

    #[test]
    fn project_point_survives_a_zero_sized_surface() {
        let projected = project_point(
            ChartPoint { x: 4.0, y: 4.0 },
            UNIT_BOUNDS,
            LogicalSize::ZERO,
        );
        assert_eq!(projected, LogicalPoint::ZERO);
    }
}
