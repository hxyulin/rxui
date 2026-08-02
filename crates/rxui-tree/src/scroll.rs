//! Clipped scrolling container.

use std::any::Any;

use astrelis_core::geometry::{LogicalPoint, LogicalSize};

use crate::{Constraints, Element, EventResult, Invalidation, LayoutContext, UiInput};

/// Erased factory turning a settled scroll offset into an application action.
type ScrollAction = dyn Fn(LogicalPoint) -> Box<dyn Any>;

/// Maximum handed to content on a scrollable axis that has not declared its
/// extent.
///
/// Large enough that content wanting to exceed the viewport says so, and finite
/// so that arithmetic on it stays well defined.
const UNMEASURED_EXTENT: f32 = 1_000_000.0;

/// Axes along which content may exceed the viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollAxis {
    /// Vertical scrolling only.
    #[default]
    Vertical,
    /// Horizontal scrolling only.
    Horizontal,
    /// Independent horizontal and vertical scrolling.
    Both,
}

/// One clipped viewport which overlays its children at a controlled offset.
pub struct Scroll {
    /// Enabled scroll axes.
    pub axis: ScrollAxis,
    /// Current controlled logical offset.
    pub offset: LogicalPoint,
    /// Content extent on the scrollable axes, when the caller already knows it.
    ///
    /// `None` measures children against a pseudo-infinite box, which is the only
    /// way to discover an extent the content itself decides - and which hands
    /// every child an absurd maximum on the scrollable axis, so a child that
    /// grows into what it is offered expands to it. Declaring the extent replaces
    /// both: children are measured against the real content size, and
    /// [`Scroll::max_offset`] is known before anything has been laid out.
    pub content_extent: Option<LogicalSize>,
    viewport: LogicalSize,
    measured: LogicalSize,
    scrolled: Option<Box<ScrollAction>>,
}

impl Scroll {
    /// Creates a viewport at offset zero.
    pub const fn new(axis: ScrollAxis) -> Self {
        Self {
            axis,
            offset: LogicalPoint::ZERO,
            content_extent: None,
            viewport: LogicalSize::ZERO,
            measured: LogicalSize::ZERO,
            scrolled: None,
        }
    }

    /// Declares the content extent up front.
    pub const fn with_content_extent(mut self, extent: LogicalSize) -> Self {
        self.content_extent = Some(extent);
        self
    }

    /// Emits an erased action carrying the new offset whenever the wheel moves
    /// the content.
    ///
    /// Without one, wheel input is swallowed: the element scrolls itself and the
    /// application never learns the offset it now holds, which is no use to a
    /// consumer that keeps scroll position in its own state.
    pub fn on_scrolled_factory(
        mut self,
        scrolled: impl Fn(LogicalPoint) -> Box<dyn Any> + 'static,
    ) -> Self {
        self.scrolled = Some(Box::new(scrolled));
        self
    }

    /// Replaces the erased scroll-action factory without recreating the element.
    pub fn set_scrolled_factory(
        &mut self,
        scrolled: impl Fn(LogicalPoint) -> Box<dyn Any> + 'static,
    ) {
        self.scrolled = Some(Box::new(scrolled));
    }

    /// Removes the scroll-action factory.
    pub fn clear_scrolled_factory(&mut self) {
        self.scrolled = None;
    }

    /// Returns the clipped viewport resolved by the latest layout.
    pub const fn viewport(&self) -> LogicalSize {
        self.viewport
    }

    /// Returns the content extent the latest layout resolved.
    ///
    /// A scrollable axis reports [`Scroll::content_extent`] when it is declared,
    /// because that is what the children were measured against; every other axis
    /// reports what they measured.
    pub fn content(&self) -> LogicalSize {
        LogicalSize::new(
            self.declared_width().unwrap_or(self.measured.width),
            self.declared_height().unwrap_or(self.measured.height),
        )
    }

    /// Returns the largest valid offset for the resolved content and viewport.
    pub fn max_offset(&self) -> LogicalPoint {
        let content = self.content();
        LogicalPoint::new(
            (content.width - self.viewport.width).max(0.0),
            (content.height - self.viewport.height).max(0.0),
        )
    }

    /// Returns `offset` restricted to the currently valid range.
    pub(crate) fn clamped(&self, offset: LogicalPoint) -> LogicalPoint {
        let max = self.max_offset();
        LogicalPoint::new(offset.x.clamp(0.0, max.x), offset.y.clamp(0.0, max.y))
    }

    /// Returns the local origin a child sits at for the current offset.
    pub(crate) const fn content_origin(&self) -> LogicalPoint {
        LogicalPoint::new(-self.offset.x, -self.offset.y)
    }

    const fn scrolls_horizontally(&self) -> bool {
        matches!(self.axis, ScrollAxis::Horizontal | ScrollAxis::Both)
    }

    const fn scrolls_vertically(&self) -> bool {
        matches!(self.axis, ScrollAxis::Vertical | ScrollAxis::Both)
    }

    fn declared_width(&self) -> Option<f32> {
        self.content_extent
            .filter(|_| self.scrolls_horizontally())
            .map(|extent| extent.width.max(0.0))
    }

    fn declared_height(&self) -> Option<f32> {
        self.content_extent
            .filter(|_| self.scrolls_vertically())
            .map(|extent| extent.height.max(0.0))
    }

    fn clamp_offset(&mut self) {
        self.offset = self.clamped(self.offset);
    }
}

impl Default for Scroll {
    fn default() -> Self {
        Self::new(ScrollAxis::Vertical)
    }
}

/// Hand-written because an erased action cannot be derived over, and summarized
/// to the state a failing layout assertion needs.
impl std::fmt::Debug for Scroll {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Scroll")
            .field("axis", &self.axis)
            .field("offset", &self.offset)
            .field("content_extent", &self.content_extent)
            .field("viewport", &self.viewport)
            .field("measured", &self.measured)
            .finish()
    }
}

impl Element for Scroll {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        let viewport = constraints.max;
        let child_max = LogicalSize::new(
            if self.scrolls_horizontally() {
                self.declared_width().unwrap_or(UNMEASURED_EXTENT)
            } else {
                viewport.width
            },
            if self.scrolls_vertically() {
                self.declared_height().unwrap_or(UNMEASURED_EXTENT)
            } else {
                viewport.height
            },
        );
        let mut measured = LogicalSize::ZERO;
        let children = context.children();
        for child in children.iter().copied() {
            let size = context.layout_child(child, Constraints::new(LogicalSize::ZERO, child_max));
            measured.width = measured.width.max(size.width);
            measured.height = measured.height.max(size.height);
        }
        self.viewport = viewport;
        self.measured = measured;
        self.clamp_offset();
        let origin = self.content_origin();
        for child in children {
            context.place_child(child, origin);
        }
        constraints.constrain(viewport)
    }

    fn event(&mut self, input: UiInput) -> EventResult {
        let UiInput::PointerWheel { delta, .. } = input else {
            return EventResult::default();
        };
        let before = self.offset;
        if self.scrolls_horizontally() {
            self.offset.x += delta.x;
        }
        if self.scrolls_vertically() {
            self.offset.y += delta.y;
        }
        self.clamp_offset();
        if self.offset == before {
            // Still handled: a viewport already at its end absorbs the wheel
            // rather than handing the rest of the gesture to an enclosing scroll.
            return EventResult {
                handled: true,
                ..EventResult::default()
            };
        }
        EventResult {
            action: self.scrolled.as_ref().map(|scrolled| scrolled(self.offset)),
            // The offset reaches the children as their layout offset, which only
            // `place_child` assigns, and an element cannot reach its children
            // outside its own layout. `NodeMut::set_offset` places them
            // directly and moves content without a layout pass; this path cannot.
            invalidation: Invalidation::LAYOUT,
            handled: true,
            ..EventResult::default()
        }
    }

    fn hit_testable(&self) -> bool {
        true
    }

    fn clips_children(&self) -> bool {
        true
    }
}
