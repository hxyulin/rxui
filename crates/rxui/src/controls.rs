//! Bounded scroll areas and application-controlled split panes.
//!
//! Scroll offsets belong to each UI placement. Split positions belong to application
//! state and resize listeners propose constrained updates, preserving fraction/pixel
//! mode. Both controls use portable input and publish numeric accessibility ranges.
//!
//! ```
//! use rxui::prelude::*;
//! struct Editor { sidebar: SplitPosition, files: ScrollHandle }
//! impl View for Editor {
//!     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
//!         split_row(scroll_area(column().child(label("Files"))).handle(self.files.clone()),
//!                   column().child(label("Editor")))
//!             .position(self.sidebar).min_first(120.).min_second(240.)
//!             .on_resize(cx.listener(|this, event: &ResizeEvent, _| {
//!                 this.sidebar = event.position;
//!             }))
//!     }
//! }
//! ```
use crate::{
    Element, IntoElement, Listener, PaintStyle, ScrollAxes, ScrollHandle, ThemeColor, column, row,
};
use taffy::prelude::*;
/// Logical axis: Horizontal controls x/width; Vertical controls y/height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// X/width.
    Horizontal,
    /// Y/height.
    Vertical,
}
impl Axis {
    pub(crate) fn index(self) -> usize {
        usize::from(self == Self::Vertical)
    }
}
/// Desired first pane extent. Fractions apply to space after the divider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitPosition {
    /// Fraction in 0..=1.
    Fraction(f32),
    /// Logical pixel size, constrained by both pane minima.
    Pixels(f32),
}
impl Default for SplitPosition {
    fn default() -> Self {
        Self::Fraction(0.5)
    }
}
impl SplitPosition {
    pub(crate) fn valid(self) -> bool {
        match self {
            Self::Fraction(v) => v.is_finite() && (0. ..=1.).contains(&v),
            Self::Pixels(v) => v.is_finite() && v >= 0.,
        }
    }
    pub(crate) fn at(self, pixels: f32, available: f32) -> Self {
        match self {
            Self::Fraction(_) => Self::Fraction(if available > 0. {
                (pixels / available).clamp(0., 1.)
            } else {
                0.
            }),
            Self::Pixels(_) => Self::Pixels(pixels),
        }
    }
}
/// Resize lifecycle. Escape restores the configured position at press; host/removal cancel
/// ends the gesture without rollback. Position remains application-controlled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizePhase {
    /// Press began a drag.
    Begin,
    /// Drag motion.
    Drag,
    /// Button release ended the drag.
    End,
    /// Gesture cancelled.
    Cancel,
    /// Keyboard adjustment.
    Keyboard,
    /// Assistive numeric adjustment.
    Accessibility,
}
/// Controlled resize proposal. Store position in application state in on_resize.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResizeEvent {
    /// Proposed constrained position, retaining the configured fraction/pixels mode.
    pub position: SplitPosition,
    /// Actual proposed first pane size in logical pixels.
    pub first_size: f32,
    /// Total space for both panes, excluding the divider.
    pub available: f32,
    /// Gesture/action stage.
    pub phase: ResizePhase,
}
/// Numeric range and geometry snapshot for a scrollbar or split divider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeInfo {
    /// Controlled logical axis.
    pub axis: Axis,
    /// Current logical pixel value.
    pub value: f32,
    /// Minimum value.
    pub min: f32,
    /// Maximum value.
    pub max: f32,
    /// Keyboard/assistive increment in logical pixels.
    pub step: f32,
    /// Scrollbar thumb bounds, absent for split dividers.
    pub thumb_bounds: Option<crate::Bounds>,
    /// Whether changes are unavailable (zero range or no controlled resize listener).
    pub read_only: bool,
}
#[derive(Clone)]
pub(crate) struct ScrollbarProps {
    pub handle: ScrollHandle,
    pub axis: Axis,
    pub min_thumb: f32,
}
#[derive(Clone)]
pub(crate) struct SplitterProps {
    pub axis: Axis,
    pub position: SplitPosition,
    pub min_first: f32,
    pub min_second: f32,
    pub divider: f32,
    pub resize: Option<Listener<ResizeEvent>>,
}
impl SplitterProps {
    pub fn valid(&self) -> bool {
        self.position.valid()
            && [self.min_first, self.min_second, self.divider]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.)
            && self.divider > 0.
    }
}
/// Standalone scrollbar for a viewport handle. Bind the handle to a viewport in the
/// same UI. Keyboard arrows/pages/Home/End and assistive numeric actions adjust it.
pub fn scrollbar(handle: ScrollHandle, axis: Axis) -> Element {
    let mut e = Element::new(crate::element::ElementKind::Scrollbar(Box::new(
        ScrollbarProps {
            handle,
            axis,
            min_thumb: 20.,
        },
    )))
    .background(ThemeColor::Surface)
    .color(ThemeColor::Border)
    .hover_style(PaintStyle::new().color(ThemeColor::TextMuted))
    .accessibility_label("Scroll position");
    if axis == Axis::Vertical {
        e = e.width(12.).fill_height();
    } else {
        e = e.height(12.).fill_width();
    }
    e
}
/// Scroll area builder with an explicit viewport and optional persistent scrollbar gutters.
#[must_use]
pub struct ScrollArea {
    root: Element,
    content: Element,
    handle: ScrollHandle,
    axes: ScrollAxes,
    bars: bool,
    thickness: f32,
}
/// Scrollable viewport with vertical scrollbar. Give it bounded space through its
/// parent or size; bars reserve gutters and do not change layout on overflow changes.
pub fn scroll_area(content: impl IntoElement) -> ScrollArea {
    ScrollArea {
        root: column()
            .fill_width()
            .flex_grow(1.)
            .flex_basis(0.)
            .min_width(0.)
            .min_height(0.),
        content: content.into_element(),
        handle: ScrollHandle::new(),
        axes: ScrollAxes::Vertical,
        bars: true,
        thickness: 12.,
    }
}
impl ScrollArea {
    /// Uses an application-owned placement reference.
    pub fn handle(mut self, handle: ScrollHandle) -> Self {
        self.handle = handle;
        self
    }
    /// Enables these axes.
    pub fn axes(mut self, axes: ScrollAxes) -> Self {
        self.axes = axes;
        self
    }
    /// Shows/hides scrollbar gutters without disabling scrolling.
    pub fn scrollbars(mut self, show: bool) -> Self {
        self.bars = show;
        self
    }
    /// Logical gutter width/height; must be positive and finite.
    pub fn scrollbar_size(mut self, size: f32) -> Self {
        self.thickness = size;
        self
    }
}
impl IntoElement for ScrollArea {
    fn into_element(self) -> Element {
        let [x, y] = self.axes.allowed();
        let safe = if self.thickness.is_finite() && self.thickness > 0. {
            self.thickness
        } else {
            12.
        };
        let mut viewport = column()
            .fill_width()
            .fill_height()
            .min_width(0.)
            .min_height(0.)
            .child(self.content);
        viewport = match self.axes {
            ScrollAxes::Vertical => viewport.scroll_y(),
            ScrollAxes::Horizontal => viewport.scroll_x(),
            ScrollAxes::Both => viewport.scroll(),
        };
        viewport = viewport.scroll_handle(self.handle.clone()).layout(|s| {
            s.grid_column = line(1);
            s.grid_row = line(1);
        });
        let mut root = self
            .root
            .layout(|s| {
                s.display = Display::Grid;
                s.grid_template_columns = vec![minmax(length(0.), fr(1.))];
                s.grid_template_rows = vec![minmax(length(0.), fr(1.))];
                if self.bars && y {
                    s.grid_template_columns.push(length(safe));
                }
                if self.bars && x {
                    s.grid_template_rows.push(length(safe));
                }
            })
            .child(viewport);
        if self.bars && y {
            root = root.child(
                scrollbar(self.handle.clone(), Axis::Vertical)
                    .width(self.thickness)
                    .layout(|s| {
                        s.grid_column = line(2);
                        s.grid_row = line(1);
                    }),
            );
        }
        if self.bars && x {
            root = root.child(
                scrollbar(self.handle.clone(), Axis::Horizontal)
                    .height(self.thickness)
                    .layout(|s| {
                        s.grid_column = line(1);
                        s.grid_row = line(2);
                    }),
            );
        }
        root
    }
}
/// Controlled two-pane split builder. The root participates in flex sizing and uses
/// flex allocation. When available space is below the requested minima, panes
/// retain their minima and the root clips overflow; dimensions never become negative.
#[must_use]
pub struct Split {
    root: Element,
    first: Element,
    second: Element,
    props: SplitterProps,
}
/// Side-by-side panes; divider changes first pane width.
pub fn split_row(first: impl IntoElement, second: impl IntoElement) -> Split {
    split(Axis::Horizontal, first, second)
}
/// Stacked panes; divider changes first pane height.
pub fn split_column(first: impl IntoElement, second: impl IntoElement) -> Split {
    split(Axis::Vertical, first, second)
}
fn split(axis: Axis, first: impl IntoElement, second: impl IntoElement) -> Split {
    Split {
        root: row()
            .fill_width()
            .flex_grow(1.)
            .flex_basis(0.)
            .min_width(0.)
            .min_height(0.)
            .clip(),
        first: first.into_element(),
        second: second.into_element(),
        props: SplitterProps {
            axis,
            position: SplitPosition::default(),
            min_first: 0.,
            min_second: 0.,
            divider: 8.,
            resize: None,
        },
    }
}
impl Split {
    /// Desired first extent; application owns it and receives constrained proposals.
    pub fn position(mut self, position: SplitPosition) -> Self {
        self.props.position = position;
        self
    }
    /// First pane minimum in logical pixels.
    pub fn min_first(mut self, size: f32) -> Self {
        self.props.min_first = size;
        self
    }
    /// Second pane minimum in logical pixels.
    pub fn min_second(mut self, size: f32) -> Self {
        self.props.min_second = size;
        self
    }
    /// Divider hit width/height; its visible center line remains narrower.
    pub fn divider_size(mut self, size: f32) -> Self {
        self.props.divider = size;
        self
    }
    /// Controlled resize listener; no listener makes the divider read-only.
    pub fn on_resize(mut self, listener: Listener<ResizeEvent>) -> Self {
        self.props.resize = Some(listener);
        self
    }
}
impl IntoElement for Split {
    fn into_element(self) -> Element {
        let p = &self.props;
        let horizontal = p.axis == Axis::Horizontal;
        let valid = p.valid();
        let min_first = if valid { p.min_first } else { 0. };
        let min_second = if valid { p.min_second } else { 0. };
        let divider = if valid { p.divider } else { 8. };
        let position = if valid {
            p.position
        } else {
            SplitPosition::default()
        };
        let first = column()
            .key("first")
            .fill_width()
            .fill_height()
            .min_width(0.)
            .min_height(0.)
            .clip()
            .child(self.first)
            .layout(|s| {
                let minimum = LengthPercentageAuto::length(min_first);
                if horizontal {
                    s.size.width = Dimension::AUTO;
                    s.min_size.width = minimum;
                } else {
                    s.size.height = Dimension::AUTO;
                    s.min_size.height = minimum;
                }
                match position {
                    SplitPosition::Fraction(v) => {
                        s.flex_basis = Dimension::length(0.);
                        s.flex_grow = v;
                    }
                    SplitPosition::Pixels(v) => {
                        s.flex_basis = Dimension::length(v.max(min_first));
                        s.flex_grow = 0.;
                    }
                }
                s.flex_shrink = 1.;
            });
        let divider_element = Element::new(crate::element::ElementKind::Splitter(Box::new(
            self.props.clone(),
        )))
        .key("divider")
        .background(ThemeColor::Surface)
        .color(ThemeColor::Border)
        .hover_style(PaintStyle::new().color(ThemeColor::TextMuted))
        .accessibility_label("Pane size")
        .fill_width()
        .fill_height()
        .layout(|s| {
            s.flex_basis = Dimension::length(divider);
            s.flex_shrink = 0.;
            if horizontal {
                s.size.width = Dimension::length(divider);
            } else {
                s.size.height = Dimension::length(divider);
            }
        });
        let second = column()
            .key("second")
            .fill_width()
            .fill_height()
            .min_width(0.)
            .min_height(0.)
            .clip()
            .child(self.second)
            .layout(|s| {
                let minimum = LengthPercentageAuto::length(min_second);
                if horizontal {
                    s.size.width = Dimension::AUTO;
                    s.min_size.width = minimum;
                } else {
                    s.size.height = Dimension::AUTO;
                    s.min_size.height = minimum;
                }
                s.flex_basis = Dimension::length(0.);
                s.flex_grow = match position {
                    SplitPosition::Fraction(v) => 1. - v,
                    SplitPosition::Pixels(_) => 1.,
                };
                s.flex_shrink = 0.;
            });
        self.root
            .layout(|s| {
                s.display = Display::Flex;
                s.flex_direction = if horizontal {
                    FlexDirection::Row
                } else {
                    FlexDirection::Column
                };
            })
            .child(first)
            .child(divider_element)
            .child(second)
    }
}

impl Split {
    /// Configures the root size constraint.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.root = self.root.size(width, height);
        self
    }
    /// Configures the root width constraint.
    pub fn width(mut self, width: f32) -> Self {
        self.root = self.root.width(width);
        self
    }
    /// Configures the root height constraint.
    pub fn height(mut self, height: f32) -> Self {
        self.root = self.root.height(height);
        self
    }
    /// Configures the root flex grow constraint.
    pub fn flex_grow(mut self, factor: f32) -> Self {
        self.root = self.root.flex_grow(factor);
        self
    }
    /// Configures the root fill width constraint.
    pub fn fill_width(mut self) -> Self {
        self.root = self.root.fill_width();
        self
    }
    /// Configures the root fill height constraint.
    pub fn fill_height(mut self) -> Self {
        self.root = self.root.fill_height();
        self
    }
    /// Configures the root min width constraint.
    pub fn min_width(mut self, value: f32) -> Self {
        self.root = self.root.min_width(value);
        self
    }
    /// Configures the root min height constraint.
    pub fn min_height(mut self, value: f32) -> Self {
        self.root = self.root.min_height(value);
        self
    }
}

impl ScrollArea {
    /// Configures the root size constraint.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.root = self.root.size(width, height);
        self
    }
    /// Configures the root width constraint.
    pub fn width(mut self, width: f32) -> Self {
        self.root = self.root.width(width);
        self
    }
    /// Configures the root height constraint.
    pub fn height(mut self, height: f32) -> Self {
        self.root = self.root.height(height);
        self
    }
    /// Configures the root flex grow constraint.
    pub fn flex_grow(mut self, factor: f32) -> Self {
        self.root = self.root.flex_grow(factor);
        self
    }
    /// Configures the root fill width constraint.
    pub fn fill_width(mut self) -> Self {
        self.root = self.root.fill_width();
        self
    }
    /// Configures the root fill height constraint.
    pub fn fill_height(mut self) -> Self {
        self.root = self.root.fill_height();
        self
    }
    /// Configures the root min width constraint.
    pub fn min_width(mut self, value: f32) -> Self {
        self.root = self.root.min_width(value);
        self
    }
    /// Configures the root min height constraint.
    pub fn min_height(mut self, value: f32) -> Self {
        self.root = self.root.min_height(value);
        self
    }
}
