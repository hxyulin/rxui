//! Interactive Cartesian line, bar, and scatter charts.

use std::{any::Any, error::Error, fmt};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize, Point, Rect, Size},
};
use astrelis_paint::{Brush, Painter, Path, StrokeStyle};
use astrelis_platform::{DeviceId, ElementState, Key, NamedKey, PointerButton};
use astrelis_text::{FontFamily, TextLayout, TextLayoutContext, TextLayoutRequest, TextWrap};
use astrelis_ui_core::{
    EventContext, RoutedEvent, RoutedEventKind, SemanticAction, SemanticActionKind, SemanticRole,
    Theme, UiError, Widget, WidgetContainerStyle, deterministic_font_database,
};

use crate::{ViewportNavigationBindings, ViewportNavigationIntent};

/// One numeric Cartesian sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartPoint {
    /// Horizontal value.
    pub x: f64,
    /// Vertical value.
    pub y: f64,
}

impl ChartPoint {
    /// Creates a point.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Rendering style for one series.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChartSeriesKind {
    /// Connected polyline.
    Line,
    /// Independent circular marks.
    Scatter,
    /// Bars centered on each x value, using the specified data-space width.
    Bar {
        /// Width of each bar in x-axis data units.
        width: f64,
    },
}

/// One ordered chart series.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSeries {
    /// Stable identifier used by selections and actions.
    pub id: String,
    /// User-visible legend label.
    pub label: String,
    /// Series rendering style.
    pub kind: ChartSeriesKind,
    /// Ordered samples.
    pub points: Vec<ChartPoint>,
    /// Optional explicit color; themes provide defaults when absent.
    pub color: Option<Color>,
}

impl ChartSeries {
    /// Creates a series with theme-derived color.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        kind: ChartSeriesKind,
        points: Vec<ChartPoint>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind,
            points,
            color: None,
        }
    }
}

/// Presentation for one linear axis.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisOptions {
    /// Axis title included in chart semantics.
    pub label: String,
    /// Optional fixed range used instead of the controlled viewport.
    pub range: Option<(f64, f64)>,
    /// Number of major grid divisions.
    pub divisions: usize,
}

impl Default for AxisOptions {
    fn default() -> Self {
        Self {
            label: String::new(),
            range: None,
            divisions: 5,
        }
    }
}

/// Axes enabled for one chart interaction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartAxes {
    /// Disable the interaction.
    None,
    /// Affect only the horizontal axis.
    Horizontal,
    /// Affect only the vertical axis.
    Vertical,
    /// Affect both axes.
    #[default]
    Both,
}

impl ChartAxes {
    const fn horizontal(self) -> bool {
        matches!(self, Self::Horizontal | Self::Both)
    }

    const fn vertical(self) -> bool {
        matches!(self, Self::Vertical | Self::Both)
    }
}

/// Chart navigation, clamping, and live-follow policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartInteractionOptions {
    /// Axes changed by pointer and keyboard panning.
    pub pan: ChartAxes,
    /// Axes changed by wheel and keyboard zooming.
    pub zoom: ChartAxes,
    /// Optional outer limits for every viewport change.
    pub bounds: Option<ChartViewport>,
    /// Keep this many horizontal data units visible while following new data.
    pub follow_latest_x: Option<f64>,
    /// Mapping from wheels and native gestures to pan and zoom intents.
    pub navigation: ViewportNavigationBindings,
}

impl Default for ChartInteractionOptions {
    fn default() -> Self {
        Self {
            pan: ChartAxes::Both,
            zoom: ChartAxes::Both,
            bounds: None,
            follow_latest_x: None,
            navigation: ViewportNavigationBindings::default(),
        }
    }
}

/// Chart presentation and performance policy.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartOptions {
    /// Accessible chart title.
    pub title: String,
    /// Horizontal axis.
    pub x_axis: AxisOptions,
    /// Vertical axis.
    pub y_axis: AxisOptions,
    /// Paint compact legend markers.
    pub show_legend: bool,
    /// Maximum retained line samples per horizontal pixel.
    pub max_samples_per_pixel: usize,
    /// Navigation, clamping, and live-follow behavior.
    pub interaction: ChartInteractionOptions,
}

impl Default for ChartOptions {
    fn default() -> Self {
        Self {
            title: "Chart".into(),
            x_axis: AxisOptions::default(),
            y_axis: AxisOptions::default(),
            show_legend: true,
            max_samples_per_pixel: 2,
            interaction: ChartInteractionOptions::default(),
        }
    }
}

/// Visible data-space rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartViewport {
    /// Minimum x value.
    pub x_min: f64,
    /// Maximum x value.
    pub x_max: f64,
    /// Minimum y value.
    pub y_min: f64,
    /// Maximum y value.
    pub y_max: f64,
}

impl ChartViewport {
    /// Creates a validated non-empty viewport.
    pub fn new(x_min: f64, x_max: f64, y_min: f64, y_max: f64) -> Result<Self, ChartError> {
        let value = Self {
            x_min,
            x_max,
            y_min,
            y_max,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(self) -> Result<(), ChartError> {
        if [self.x_min, self.x_max, self.y_min, self.y_max]
            .iter()
            .any(|value| !value.is_finite())
            || self.x_min >= self.x_max
            || self.y_min >= self.y_max
        {
            return Err(ChartError(
                "chart viewport must contain finite increasing ranges".into(),
            ));
        }
        Ok(())
    }

    /// Derives a padded viewport from finite series data.
    pub fn fit(series: &[ChartSeries]) -> Self {
        let mut xs = series
            .iter()
            .flat_map(|series| series.points.iter().map(|point| point.x));
        let Some(first_x) = xs.next() else {
            return Self {
                x_min: 0.0,
                x_max: 1.0,
                y_min: 0.0,
                y_max: 1.0,
            };
        };
        let (mut x_min, mut x_max) = (first_x, first_x);
        for x in xs {
            x_min = x_min.min(x);
            x_max = x_max.max(x);
        }
        let mut ys = series
            .iter()
            .flat_map(|series| series.points.iter().map(|point| point.y));
        let Some(first_y) = ys.next() else {
            return Self {
                x_min: 0.0,
                x_max: 1.0,
                y_min: 0.0,
                y_max: 1.0,
            };
        };
        let (mut y_min, mut y_max) = (first_y, first_y);
        for y in ys {
            y_min = y_min.min(y);
            y_max = y_max.max(y);
        }
        if x_min == x_max {
            x_min -= 0.5;
            x_max += 0.5;
        }
        if y_min == y_max {
            y_min -= 0.5;
            y_max += 0.5;
        }
        let x_pad = (x_max - x_min) * 0.04;
        let y_pad = (y_max - y_min) * 0.08;
        Self {
            x_min: x_min - x_pad,
            x_max: x_max + x_pad,
            y_min: y_min - y_pad,
            y_max: y_max + y_pad,
        }
    }

    fn zoom(self, factor: f64, center: ChartPoint) -> Self {
        let factor = factor.clamp(0.05, 20.0);
        Self {
            x_min: center.x + (self.x_min - center.x) * factor,
            x_max: center.x + (self.x_max - center.x) * factor,
            y_min: center.y + (self.y_min - center.y) * factor,
            y_max: center.y + (self.y_max - center.y) * factor,
        }
    }

    fn pan(self, x: f64, y: f64) -> Self {
        Self {
            x_min: self.x_min + x,
            x_max: self.x_max + x,
            y_min: self.y_min + y,
            y_max: self.y_max + y,
        }
    }

    fn zoom_axes(self, factor: f64, center: ChartPoint, axes: ChartAxes) -> Self {
        let zoomed = self.zoom(factor, center);
        Self {
            x_min: if axes.horizontal() {
                zoomed.x_min
            } else {
                self.x_min
            },
            x_max: if axes.horizontal() {
                zoomed.x_max
            } else {
                self.x_max
            },
            y_min: if axes.vertical() {
                zoomed.y_min
            } else {
                self.y_min
            },
            y_max: if axes.vertical() {
                zoomed.y_max
            } else {
                self.y_max
            },
        }
    }

    fn pan_axes(self, x: f64, y: f64, axes: ChartAxes) -> Self {
        self.pan(
            if axes.horizontal() { x } else { 0.0 },
            if axes.vertical() { y } else { 0.0 },
        )
    }
}

/// Selected chart datum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChartSelection {
    /// Series identifier.
    pub series: String,
    /// Index within the series.
    pub point: usize,
}

/// Typed chart interaction.
#[derive(Clone, Debug, PartialEq)]
pub enum ChartAction {
    /// Selects or clears one datum.
    Select(Option<ChartSelection>),
    /// Requests a new viewport after panning or zooming.
    SetViewport(ChartViewport),
    /// Requests the data-fitted viewport.
    ResetViewport,
}

/// Invalid chart model or options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChartError(String);

impl fmt::Display for ChartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
impl Error for ChartError {}

/// Deterministically retains first/last and per-bucket extrema for a line.
pub fn decimate_line(points: &[ChartPoint], target: usize) -> Vec<usize> {
    if points.len() <= target.max(2) {
        return (0..points.len()).collect();
    }
    let buckets = (target.max(4) / 2).max(1);
    let span = (points.len() - 2) as f64 / buckets as f64;
    let mut output = vec![0];
    for bucket in 0..buckets {
        let start = 1 + (bucket as f64 * span).floor() as usize;
        let end = (1 + ((bucket + 1) as f64 * span).ceil() as usize).min(points.len() - 1);
        if start >= end {
            continue;
        }
        let mut min = start;
        let mut max = start;
        for index in start + 1..end {
            if points[index].y < points[min].y {
                min = index;
            }
            if points[index].y > points[max].y {
                max = index;
            }
        }
        if min < max {
            output.extend([min, max]);
        } else if max < min {
            output.extend([max, min]);
        } else {
            output.push(min);
        }
    }
    output.push(points.len() - 1);
    output.sort_unstable();
    output.dedup();
    output
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    device: DeviceId,
    start: LogicalPoint,
    viewport: ChartViewport,
}

struct ChartText {
    title: TextLayout,
    x_axis: Option<TextLayout>,
    y_axis: Option<TextLayout>,
    legend: Vec<(String, TextLayout)>,
}

/// Retained interactive chart widget.
pub struct ChartView<Message> {
    series: Vec<ChartSeries>,
    options: ChartOptions,
    viewport: ChartViewport,
    selection: Option<ChartSelection>,
    drag: Option<Drag>,
    hover: Option<(DeviceId, LogicalPoint)>,
    text: ChartText,
    following_latest: bool,
    on_action: Box<dyn FnMut(ChartAction) -> Message>,
}

impl<Message> ChartView<Message> {
    /// Creates a validated chart using a data-fitted viewport.
    pub fn new(
        series: Vec<ChartSeries>,
        options: ChartOptions,
        on_action: impl FnMut(ChartAction) -> Message + 'static,
    ) -> Result<Self, ChartError> {
        validate_series(&series, &options)?;
        let mut viewport = ChartViewport::fit(&series);
        if let Some((min, max)) = options.x_axis.range {
            viewport.x_min = min;
            viewport.x_max = max;
        }
        if let Some((min, max)) = options.y_axis.range {
            viewport.y_min = min;
            viewport.y_max = max;
        }
        let following_latest = options.interaction.follow_latest_x.is_some();
        viewport = constrain_viewport(viewport, &options);
        if following_latest {
            viewport = follow_latest_viewport(viewport, &series, &options);
        }
        let text = shape_chart_text(&series, &options)?;
        Ok(Self {
            series,
            options,
            viewport,
            selection: None,
            drag: None,
            hover: None,
            text,
            following_latest,
            on_action: Box::new(on_action),
        })
    }

    /// Replaces the controlled viewport.
    pub fn set_viewport(&mut self, viewport: ChartViewport) -> Result<(), ChartError> {
        viewport.validate()?;
        self.viewport = constrain_viewport(viewport, &self.options);
        Ok(())
    }

    /// Replaces the controlled selection.
    pub fn set_selection(&mut self, selection: Option<ChartSelection>) {
        self.selection = selection;
    }

    /// Replaces and validates series data.
    pub fn set_series(&mut self, series: Vec<ChartSeries>) -> Result<(), ChartError> {
        validate_series(&series, &self.options)?;
        let labels_changed = self.text.legend.len() != series.len()
            || self
                .text
                .legend
                .iter()
                .zip(&series)
                .any(|((id, label), series)| id != &series.id || label.text() != series.label);
        if labels_changed {
            self.text = shape_chart_text(&series, &self.options)?;
        }
        self.series = series;
        if self.following_latest {
            self.viewport = follow_latest_viewport(self.viewport, &self.series, &self.options);
        }
        Ok(())
    }

    /// Appends one finite point without replacing the complete series.
    ///
    /// When latest-X following is active, the viewport advances by the same
    /// amount and remains inside configured bounds.
    pub fn append_point(&mut self, series_id: &str, point: ChartPoint) -> Result<(), ChartError> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(ChartError("chart points must be finite".into()));
        }
        let series = self
            .series
            .iter_mut()
            .find(|series| series.id == series_id)
            .ok_or_else(|| ChartError(format!("unknown chart series `{series_id}`")))?;
        series.points.push(point);
        if self.following_latest {
            self.viewport = follow_latest_viewport(self.viewport, &self.series, &self.options);
        }
        Ok(())
    }

    /// Resumes automatic latest-X following after manual navigation paused it.
    pub fn resume_follow_latest(&mut self) -> bool {
        if self.options.interaction.follow_latest_x.is_none() {
            return false;
        }
        self.following_latest = true;
        self.viewport = follow_latest_viewport(self.viewport, &self.series, &self.options);
        true
    }

    /// Pauses automatic latest-X following without changing the viewport.
    pub fn pause_follow_latest(&mut self) {
        self.following_latest = false;
    }

    /// Whether new samples currently advance the horizontal viewport.
    pub const fn is_following_latest(&self) -> bool {
        self.following_latest
    }

    /// Current viewport.
    pub const fn viewport(&self) -> ChartViewport {
        self.viewport
    }

    fn plot(bounds: LogicalRect) -> LogicalRect {
        Rect::from_xywh(
            bounds.origin.x + 54.0,
            bounds.origin.y + 34.0,
            (bounds.size.width - 70.0).max(1.0),
            (bounds.size.height - 66.0).max(1.0),
        )
    }

    fn screen(&self, point: ChartPoint, plot: LogicalRect) -> LogicalPoint {
        Point::new(
            plot.origin.x
                + ((point.x - self.viewport.x_min) / (self.viewport.x_max - self.viewport.x_min))
                    as f32
                    * plot.size.width,
            plot.max_y()
                - ((point.y - self.viewport.y_min) / (self.viewport.y_max - self.viewport.y_min))
                    as f32
                    * plot.size.height,
        )
    }

    fn nearest(&self, position: LogicalPoint, plot: LogicalRect) -> Option<ChartSelection> {
        self.series
            .iter()
            .flat_map(|series| {
                series
                    .points
                    .iter()
                    .enumerate()
                    .map(move |(index, point)| (series, index, point))
            })
            .map(|(series, index, point)| {
                let screen = self.screen(*point, plot);
                let distance = (screen.x - position.x).powi(2) + (screen.y - position.y).powi(2);
                (
                    distance,
                    ChartSelection {
                        series: series.id.clone(),
                        point: index,
                    },
                )
            })
            .filter(|(distance, _)| *distance <= 144.0)
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, selection)| selection)
    }

    fn data_at(&self, position: LogicalPoint, plot: LogicalRect) -> ChartPoint {
        let x = position.x.clamp(plot.origin.x, plot.max_x());
        let y = position.y.clamp(plot.origin.y, plot.max_y());
        ChartPoint::new(
            self.viewport.x_min
                + f64::from((x - plot.origin.x) / plot.size.width)
                    * (self.viewport.x_max - self.viewport.x_min),
            self.viewport.y_max
                - f64::from((y - plot.origin.y) / plot.size.height)
                    * (self.viewport.y_max - self.viewport.y_min),
        )
    }

    fn emit(&mut self, context: &mut EventContext<'_, Message>, action: ChartAction) {
        context.emit((self.on_action)(action));
    }

    fn apply_user_viewport(&mut self, viewport: ChartViewport) {
        let viewport = constrain_viewport(viewport, &self.options);
        if viewport.x_min != self.viewport.x_min || viewport.x_max != self.viewport.x_max {
            self.following_latest = false;
        }
        self.viewport = viewport;
    }
}

fn validate_series(series: &[ChartSeries], options: &ChartOptions) -> Result<(), ChartError> {
    if options.max_samples_per_pixel == 0 {
        return Err(ChartError("max_samples_per_pixel must be non-zero".into()));
    }
    if let Some(bounds) = options.interaction.bounds {
        bounds.validate()?;
    }
    if options
        .interaction
        .follow_latest_x
        .is_some_and(|span| !span.is_finite() || span <= 0.0)
    {
        return Err(ChartError(
            "latest-X follow span must be finite and positive".into(),
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    for series in series {
        if series.id.trim().is_empty() || !ids.insert(&series.id) {
            return Err(ChartError(
                "series identifiers must be non-empty and unique".into(),
            ));
        }
        if matches!(series.kind, ChartSeriesKind::Bar { width } if !width.is_finite() || width <= 0.0)
        {
            return Err(ChartError("bar width must be finite and positive".into()));
        }
        if series
            .points
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            return Err(ChartError("chart points must be finite".into()));
        }
    }
    for range in [options.x_axis.range, options.y_axis.range]
        .into_iter()
        .flatten()
    {
        if !range.0.is_finite() || !range.1.is_finite() || range.0 >= range.1 {
            return Err(ChartError(
                "fixed axis ranges must be finite and increasing".into(),
            ));
        }
    }
    Ok(())
}

fn constrain_viewport(mut viewport: ChartViewport, options: &ChartOptions) -> ChartViewport {
    if let Some((min, max)) = options.x_axis.range {
        viewport.x_min = min;
        viewport.x_max = max;
    } else if let Some(bounds) = options.interaction.bounds {
        (viewport.x_min, viewport.x_max) =
            clamp_axis(viewport.x_min, viewport.x_max, bounds.x_min, bounds.x_max);
    }
    if let Some((min, max)) = options.y_axis.range {
        viewport.y_min = min;
        viewport.y_max = max;
    } else if let Some(bounds) = options.interaction.bounds {
        (viewport.y_min, viewport.y_max) =
            clamp_axis(viewport.y_min, viewport.y_max, bounds.y_min, bounds.y_max);
    }
    viewport
}

fn clamp_axis(min: f64, max: f64, bound_min: f64, bound_max: f64) -> (f64, f64) {
    let span = max - min;
    let bound_span = bound_max - bound_min;
    if span >= bound_span {
        return (bound_min, bound_max);
    }
    if min < bound_min {
        return (bound_min, bound_min + span);
    }
    if max > bound_max {
        return (bound_max - span, bound_max);
    }
    (min, max)
}

fn follow_latest_viewport(
    mut viewport: ChartViewport,
    series: &[ChartSeries],
    options: &ChartOptions,
) -> ChartViewport {
    let (Some(span), Some(latest)) = (
        options.interaction.follow_latest_x,
        series
            .iter()
            .flat_map(|series| series.points.iter().map(|point| point.x))
            .max_by(f64::total_cmp),
    ) else {
        return viewport;
    };
    viewport.x_min = latest - span;
    viewport.x_max = latest;
    constrain_viewport(viewport, options)
}

impl<Message: 'static> Widget<Message> for ChartView<Message> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn intrinsic_size(&self, _theme: &Theme) -> LogicalSize {
        Size::new(560.0, 300.0)
    }
    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle::structural()
    }
    fn hit_testable(&self) -> bool {
        true
    }
    fn focusable(&self) -> bool {
        true
    }

    fn event(&mut self, context: &mut EventContext<'_, Message>, event: &RoutedEvent) {
        let bounds = context.bounds();
        let plot = Self::plot(Rect::from_xywh(
            0.0,
            0.0,
            bounds.size.width,
            bounds.size.height,
        ));
        match &event.kind {
            RoutedEventKind::PointerEntered {
                device_id,
                position,
                ..
            } => {
                if let Some(local) = context.window_to_local(*position) {
                    self.hover = Some((*device_id, local));
                }
            }
            RoutedEventKind::PointerButton {
                device_id,
                position,
                button: PointerButton::Primary,
                state: ElementState::Pressed,
            } => {
                let Some(local) = context.window_to_local(*position) else {
                    return;
                };
                context.request_focus();
                context.capture_pointer(*device_id);
                self.drag = Some(Drag {
                    device: *device_id,
                    start: local,
                    viewport: self.viewport,
                });
            }
            RoutedEventKind::PointerMoved {
                device_id,
                position,
            } if self.drag.is_some_and(|drag| drag.device == *device_id) => {
                let Some(local) = context.window_to_local(*position) else {
                    return;
                };
                self.hover = Some((*device_id, local));
                let drag = self.drag.expect("checked");
                let x = -f64::from(local.x - drag.start.x) / f64::from(plot.size.width)
                    * (drag.viewport.x_max - drag.viewport.x_min);
                let y = f64::from(local.y - drag.start.y) / f64::from(plot.size.height)
                    * (drag.viewport.y_max - drag.viewport.y_min);
                if self.options.interaction.pan == ChartAxes::None {
                    return;
                }
                self.apply_user_viewport(drag.viewport.pan_axes(
                    x,
                    y,
                    self.options.interaction.pan,
                ));
                context.request_paint();
                self.emit(context, ChartAction::SetViewport(self.viewport));
            }
            RoutedEventKind::PointerMoved {
                device_id,
                position,
            } => {
                if let Some(local) = context.window_to_local(*position) {
                    self.hover = Some((*device_id, local));
                }
            }
            RoutedEventKind::PointerLeft { device_id, .. } => {
                if self.hover.is_some_and(|(hovered, _)| hovered == *device_id) {
                    self.hover = None;
                }
            }
            RoutedEventKind::PointerButton {
                device_id,
                position,
                button: PointerButton::Primary,
                state: ElementState::Released,
            } => {
                let drag = self.drag.take();
                let moved = self.options.interaction.pan != ChartAxes::None
                    && drag.is_some_and(|drag| {
                        context.window_to_local(*position).is_some_and(|local| {
                            (local.x - drag.start.x).abs() + (local.y - drag.start.y).abs() > 3.0
                        })
                    });
                context.release_pointer(*device_id);
                if !moved && let Some(local) = context.window_to_local(*position) {
                    self.selection = self.nearest(local, plot);
                    self.emit(context, ChartAction::Select(self.selection.clone()));
                    context.request_paint();
                }
            }
            RoutedEventKind::PointerCancelled { device_id } => {
                self.drag = None;
                if self.hover.is_some_and(|(hovered, _)| hovered == *device_id) {
                    self.hover = None;
                }
                context.release_pointer(*device_id);
            }
            RoutedEventKind::Scroll { .. }
            | RoutedEventKind::PinchGesture { .. }
            | RoutedEventKind::PanGesture { .. } => {
                let Some(intent) = self
                    .options
                    .interaction
                    .navigation
                    .decode(&event.kind, context.modifiers())
                else {
                    return;
                };
                match intent {
                    ViewportNavigationIntent::Pan { delta, .. } => {
                        if self.options.interaction.pan == ChartAxes::None {
                            return;
                        }
                        let x = f64::from(delta.x) / f64::from(plot.size.width)
                            * (self.viewport.x_max - self.viewport.x_min);
                        let y = f64::from(delta.y) / f64::from(plot.size.height)
                            * (self.viewport.y_max - self.viewport.y_min);
                        self.apply_user_viewport(self.viewport.pan_axes(
                            x,
                            y,
                            self.options.interaction.pan,
                        ));
                    }
                    ViewportNavigationIntent::Zoom {
                        position, factor, ..
                    } => {
                        if self.options.interaction.zoom == ChartAxes::None {
                            return;
                        }
                        let center = context.window_to_local(position).map_or_else(
                            || {
                                ChartPoint::new(
                                    (self.viewport.x_min + self.viewport.x_max) * 0.5,
                                    (self.viewport.y_min + self.viewport.y_max) * 0.5,
                                )
                            },
                            |position| self.data_at(position, plot),
                        );
                        self.apply_user_viewport(self.viewport.zoom_axes(
                            factor.recip(),
                            center,
                            self.options.interaction.zoom,
                        ));
                    }
                }
                context.prevent_default();
                self.emit(context, ChartAction::SetViewport(self.viewport));
                context.request_paint();
            }
            RoutedEventKind::Keyboard(input) if input.state == ElementState::Pressed => {
                match &input.logical_key {
                    Key::Character(value) if value == "+" || value == "=" => {
                        if self.options.interaction.zoom == ChartAxes::None {
                            return;
                        }
                        let center = ChartPoint::new(
                            (self.viewport.x_min + self.viewport.x_max) * 0.5,
                            (self.viewport.y_min + self.viewport.y_max) * 0.5,
                        );
                        self.apply_user_viewport(self.viewport.zoom_axes(
                            0.8,
                            center,
                            self.options.interaction.zoom,
                        ));
                        self.emit(context, ChartAction::SetViewport(self.viewport));
                    }
                    Key::Character(value) if value == "-" => {
                        if self.options.interaction.zoom == ChartAxes::None {
                            return;
                        }
                        let center = ChartPoint::new(
                            (self.viewport.x_min + self.viewport.x_max) * 0.5,
                            (self.viewport.y_min + self.viewport.y_max) * 0.5,
                        );
                        self.apply_user_viewport(self.viewport.zoom_axes(
                            1.25,
                            center,
                            self.options.interaction.zoom,
                        ));
                        self.emit(context, ChartAction::SetViewport(self.viewport));
                    }
                    Key::Named(NamedKey::Other(key)) if key == "Home" => {
                        self.emit(context, ChartAction::ResetViewport)
                    }
                    Key::Named(NamedKey::Other(key))
                        if key == "ArrowLeft" || key == "ArrowRight" =>
                    {
                        if context.modifiers().shift {
                            if !self.options.interaction.pan.horizontal() {
                                return;
                            }
                            let direction = if key == "ArrowLeft" { -0.1 } else { 0.1 };
                            self.apply_user_viewport(self.viewport.pan_axes(
                                (self.viewport.x_max - self.viewport.x_min) * direction,
                                0.0,
                                self.options.interaction.pan,
                            ));
                            self.emit(context, ChartAction::SetViewport(self.viewport));
                        } else if let Some(current) = &self.selection
                            && let Some(series) = self
                                .series
                                .iter()
                                .find(|series| series.id == current.series)
                        {
                            let next = if key == "ArrowLeft" {
                                current.point.saturating_sub(1)
                            } else {
                                (current.point + 1).min(series.points.len().saturating_sub(1))
                            };
                            self.selection = Some(ChartSelection {
                                series: current.series.clone(),
                                point: next,
                            });
                            self.emit(context, ChartAction::Select(self.selection.clone()));
                        }
                    }
                    Key::Named(NamedKey::Other(key)) if key == "ArrowUp" || key == "ArrowDown" => {
                        if !self.series.is_empty() {
                            let current = self
                                .selection
                                .as_ref()
                                .and_then(|selection| {
                                    self.series
                                        .iter()
                                        .position(|series| series.id == selection.series)
                                })
                                .unwrap_or(0);
                            let next = if key == "ArrowUp" {
                                current.saturating_sub(1)
                            } else {
                                (current + 1).min(self.series.len() - 1)
                            };
                            let point = self
                                .selection
                                .as_ref()
                                .map_or(0, |selection| selection.point)
                                .min(self.series[next].points.len().saturating_sub(1));
                            self.selection = Some(ChartSelection {
                                series: self.series[next].id.clone(),
                                point,
                            });
                            self.emit(context, ChartAction::Select(self.selection.clone()));
                        }
                    }
                    _ => return,
                }
                context.request_paint();
            }
            _ => {}
        }
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        painter.fill_rect(bounds, Brush::Solid(theme.surface))?;
        let plot = Self::plot(bounds);
        painter.draw_text(
            &self.text.title,
            Point::new(bounds.origin.x + 8.0, bounds.origin.y + 5.0),
            1.0,
        )?;
        let x_divisions = self.options.x_axis.divisions.max(1);
        let y_divisions = self.options.y_axis.divisions.max(1);
        for division in 0..=x_divisions {
            let x = plot.origin.x + plot.size.width * division as f32 / x_divisions as f32;
            painter.fill_rect(
                Rect::from_xywh(x, plot.origin.y, 1.0, plot.size.height),
                Brush::Solid(theme.border),
            )?;
        }
        for division in 0..=y_divisions {
            let y = plot.origin.y + plot.size.height * division as f32 / y_divisions as f32;
            painter.fill_rect(
                Rect::from_xywh(plot.origin.x, y, plot.size.width, 1.0),
                Brush::Solid(theme.border),
            )?;
        }
        let palette = [
            theme.accent,
            theme.success,
            theme.warning,
            theme.danger,
            theme.muted_foreground,
        ];
        painter.with_save(|painter| {
            painter.clip_rect(plot)?;
            for (series_index, series) in self.series.iter().enumerate() {
                let color = series
                    .color
                    .unwrap_or(palette[series_index % palette.len()]);
                match series.kind {
                    ChartSeriesKind::Line => {
                        let indices = decimate_line(
                            &series.points,
                            plot.size.width.max(1.0) as usize * self.options.max_samples_per_pixel,
                        );
                        let mut builder = Path::builder();
                        for (offset, index) in indices.into_iter().enumerate() {
                            let point = self.screen(series.points[index], plot);
                            if offset == 0 {
                                builder.move_to(point)?;
                            } else {
                                builder.line_to(point)?;
                            }
                        }
                        let path = builder.finish();
                        painter.stroke_path(
                            &path,
                            StrokeStyle {
                                width: 2.0,
                                ..Default::default()
                            },
                            Brush::Solid(color),
                        )?;
                    }
                    ChartSeriesKind::Scatter => {
                        for point in &series.points {
                            let point = self.screen(*point, plot);
                            painter.fill_ellipse(
                                Rect::from_xywh(point.x - 3.0, point.y - 3.0, 6.0, 6.0),
                                Brush::Solid(color),
                            )?;
                        }
                    }
                    ChartSeriesKind::Bar { width } => {
                        for point in &series.points {
                            let left = self
                                .screen(ChartPoint::new(point.x - width * 0.5, point.y), plot)
                                .x;
                            let right = self
                                .screen(ChartPoint::new(point.x + width * 0.5, point.y), plot)
                                .x;
                            let top = self.screen(*point, plot).y;
                            let baseline = self
                                .screen(
                                    ChartPoint::new(
                                        point.x,
                                        0.0_f64.clamp(self.viewport.y_min, self.viewport.y_max),
                                    ),
                                    plot,
                                )
                                .y;
                            painter.fill_rect(
                                Rect::from_xywh(
                                    left,
                                    top.min(baseline),
                                    (right - left).max(1.0),
                                    (baseline - top).abs().max(1.0),
                                ),
                                Brush::Solid(color),
                            )?;
                        }
                    }
                }
            }
            Ok(())
        })?;
        if self.options.show_legend {
            let mut x = bounds.origin.x + self.text.title.size().width + 24.0;
            for (index, (series, (_, label))) in
                self.series.iter().zip(&self.text.legend).enumerate()
            {
                let color = series.color.unwrap_or(palette[index % palette.len()]);
                painter.fill_rect(
                    Rect::from_xywh(x, bounds.origin.y + 11.0, 12.0, 6.0),
                    Brush::Solid(color),
                )?;
                painter.draw_text(label, Point::new(x + 16.0, bounds.origin.y + 5.0), 1.0)?;
                x += label.size().width + 30.0;
            }
        }
        if let Some(label) = &self.text.x_axis {
            painter.draw_text(
                label,
                Point::new(
                    plot.origin.x + (plot.size.width - label.size().width) * 0.5,
                    plot.max_y() + 8.0,
                ),
                1.0,
            )?;
        }
        if let Some(label) = &self.text.y_axis {
            painter.draw_text(
                label,
                Point::new(bounds.origin.x + 7.0, plot.origin.y + 2.0),
                1.0,
            )?;
        }
        if let Some(selection) = &self.selection
            && let Some(series) = self
                .series
                .iter()
                .find(|series| series.id == selection.series)
            && let Some(point) = series.points.get(selection.point)
        {
            let point = self.screen(*point, plot);
            painter.stroke_ellipse(
                Rect::from_xywh(point.x - 6.0, point.y - 6.0, 12.0, 12.0),
                StrokeStyle {
                    width: 2.0,
                    ..Default::default()
                },
                Brush::Solid(theme.foreground),
            )?;
        }
        Ok(())
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        let value = self.selection.as_ref().and_then(|selection| {
            self.series
                .iter()
                .find(|series| series.id == selection.series)
                .and_then(|series| {
                    series
                        .points
                        .get(selection.point)
                        .map(|point| format!("{}, x {}, y {}", series.label, point.x, point.y))
                })
        });
        Some((SemanticRole::Group, self.options.title.clone(), value))
    }
    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![SemanticActionKind::Focus, SemanticActionKind::ScrollBy]
    }
    fn semantic_action(
        &mut self,
        context: &mut EventContext<'_, Message>,
        action: &SemanticAction,
    ) -> bool {
        match action {
            SemanticAction::Focus => {
                context.request_focus();
                true
            }
            SemanticAction::ScrollBy(delta) => {
                if self.options.interaction.pan.horizontal() {
                    self.apply_user_viewport(self.viewport.pan_axes(
                        f64::from(*delta) * 0.02 * (self.viewport.x_max - self.viewport.x_min),
                        0.0,
                        self.options.interaction.pan,
                    ));
                } else if self.options.interaction.zoom != ChartAxes::None {
                    let center = ChartPoint::new(
                        (self.viewport.x_min + self.viewport.x_max) * 0.5,
                        (self.viewport.y_min + self.viewport.y_max) * 0.5,
                    );
                    self.apply_user_viewport(self.viewport.zoom_axes(
                        (f64::from(*delta) * 0.02).exp(),
                        center,
                        self.options.interaction.zoom,
                    ));
                } else {
                    return false;
                }
                self.emit(context, ChartAction::SetViewport(self.viewport));
                context.request_paint();
                true
            }
            _ => false,
        }
    }
}

fn shape_chart_text(
    series: &[ChartSeries],
    options: &ChartOptions,
) -> Result<ChartText, ChartError> {
    let mut fonts = deterministic_font_database();
    let mut context = TextLayoutContext::new();
    let mut shape = |text: &str, size: f32, weight: f32| {
        let mut request = TextLayoutRequest::new(text);
        request.style.families = vec![FontFamily::Named("Noto Sans".into())];
        request.style.size = size;
        request.style.weight = weight;
        request.paragraph.wrap = TextWrap::NoWrap;
        context
            .layout(&mut fonts, request)
            .map_err(|error| ChartError(format!("could not shape chart text: {error}")))
    };
    let title = shape(&options.title, 13.0, 600.0)?;
    let x_axis = (!options.x_axis.label.is_empty())
        .then(|| shape(&options.x_axis.label, 11.0, 400.0))
        .transpose()?;
    let y_axis = (!options.y_axis.label.is_empty())
        .then(|| shape(&options.y_axis.label, 11.0, 400.0))
        .transpose()?;
    let legend = series
        .iter()
        .map(|series| Ok((series.id.clone(), shape(&series.label, 11.0, 400.0)?)))
        .collect::<Result<_, ChartError>>()?;
    Ok(ChartText {
        title,
        x_axis,
        y_axis,
        legend,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimation_preserves_endpoints_and_extrema() {
        let points = (0..1000)
            .map(|index| ChartPoint::new(index as f64, ((index % 17) as f64 - 8.0).powi(3)))
            .collect::<Vec<_>>();
        let retained = decimate_line(&points, 100);
        assert_eq!(retained.first(), Some(&0));
        assert_eq!(retained.last(), Some(&999));
        assert!(retained.len() <= 102);
    }

    #[test]
    fn validation_rejects_duplicate_ids_and_nonfinite_points() {
        let first = ChartSeries::new(
            "same",
            "A",
            ChartSeriesKind::Line,
            vec![ChartPoint::new(0.0, 1.0)],
        );
        let mut second = first.clone();
        second.points[0].x = f64::NAN;
        assert!(
            ChartView::<()>::new(vec![first.clone(), first], ChartOptions::default(), |_| ())
                .is_err()
        );
        assert!(ChartView::<()>::new(vec![second], ChartOptions::default(), |_| ()).is_err());
    }

    #[test]
    fn chart_labels_shape_visible_glyphs() {
        let series = vec![ChartSeries::new(
            "line",
            "Signal",
            ChartSeriesKind::Line,
            vec![ChartPoint::new(0.0, 1.0)],
        )];
        let text = shape_chart_text(&series, &ChartOptions::default()).unwrap();
        assert!(!text.title.glyph_runs().is_empty());
        assert!(!text.legend[0].1.glyph_runs().is_empty());
    }

    #[test]
    fn viewport_constraints_and_live_follow_are_deterministic() {
        let series = vec![ChartSeries::new(
            "line",
            "Signal",
            ChartSeriesKind::Line,
            vec![ChartPoint::new(0.0, 10.0), ChartPoint::new(10.0, 20.0)],
        )];
        let options = ChartOptions {
            y_axis: AxisOptions {
                range: Some((0.0, 100.0)),
                ..Default::default()
            },
            interaction: ChartInteractionOptions {
                pan: ChartAxes::Horizontal,
                zoom: ChartAxes::Horizontal,
                bounds: Some(ChartViewport::new(0.0, 100.0, 0.0, 100.0).unwrap()),
                follow_latest_x: Some(5.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut view = ChartView::<()>::new(series, options, |_| ()).unwrap();
        assert_eq!(
            view.viewport(),
            ChartViewport::new(5.0, 10.0, 0.0, 100.0).unwrap()
        );

        view.append_point("line", ChartPoint::new(12.0, 30.0))
            .unwrap();
        assert_eq!(view.viewport().x_min, 7.0);
        view.apply_user_viewport(ChartViewport::new(1.0, 6.0, 0.0, 100.0).unwrap());
        assert!(!view.is_following_latest());
        view.append_point("line", ChartPoint::new(20.0, 40.0))
            .unwrap();
        assert_eq!(view.viewport().x_max, 6.0);
        assert!(view.resume_follow_latest());
        assert_eq!(view.viewport().x_min, 15.0);
        assert_eq!(view.viewport().x_max, 20.0);

        view.set_viewport(ChartViewport::new(-10.0, 10.0, -50.0, 50.0).unwrap())
            .unwrap();
        assert_eq!(
            view.viewport(),
            ChartViewport::new(0.0, 20.0, 0.0, 100.0).unwrap()
        );
    }

    #[test]
    fn cursor_anchored_zoom_preserves_the_hovered_datum() {
        let series = vec![ChartSeries::new(
            "line",
            "Signal",
            ChartSeriesKind::Line,
            vec![ChartPoint::new(0.0, 0.0), ChartPoint::new(100.0, 100.0)],
        )];
        let mut view = ChartView::<()>::new(series, ChartOptions::default(), |_| ()).unwrap();
        view.set_viewport(ChartViewport::new(0.0, 100.0, 0.0, 100.0).unwrap())
            .unwrap();
        let plot = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let cursor = Point::new(75.0, 40.0);

        let anchor = view.data_at(cursor, plot);
        view.apply_user_viewport(view.viewport.zoom_axes(0.5, anchor, ChartAxes::Both));
        let after_zoom_in = view.data_at(cursor, plot);
        assert!((after_zoom_in.x - anchor.x).abs() < 1.0e-9);
        assert!((after_zoom_in.y - anchor.y).abs() < 1.0e-9);

        view.apply_user_viewport(view.viewport.zoom_axes(2.0, after_zoom_in, ChartAxes::Both));
        assert_eq!(
            view.viewport(),
            ChartViewport::new(0.0, 100.0, 0.0, 100.0).unwrap()
        );
    }
}
