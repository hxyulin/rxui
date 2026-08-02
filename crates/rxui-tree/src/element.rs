//! Element lifecycle and pass contexts.

use std::any::Any;

use astrelis_core::{
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
    math::Affine2,
};
use astrelis_paint::Painter;
use astrelis_platform::{CursorIcon, ImeEvent, KeyboardInput, Modifiers};
use astrelis_text::{TextLayout, TextLayoutRequest};
use bitflags::bitflags;

use crate::{NodeId, SemanticAction, SemanticActionKind, SemanticData, UiTree};

bitflags! {
    /// Retained passes invalidated by a mutation.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Invalidation: u8 {
        /// Child topology or ordering changed.
        const TREE = 1 << 0;
        /// Intrinsic size or layout inputs changed.
        const LAYOUT = 1 << 1;
        /// Position, transform, clipping, or subtree bounds changed.
        const COMPOSE = 1 << 2;
        /// Local visual output changed.
        const PAINT = 1 << 3;
        /// Accessible properties or geometry changed.
        const ACCESSIBILITY = 1 << 4;
        /// Hit shape or hit-test participation changed.
        const HIT_TEST = 1 << 5;
    }
}

impl Invalidation {
    /// Work required after changing layout-affecting properties.
    pub const LAYOUT_ALL: Self = Self::from_bits_retain(
        Self::LAYOUT.bits()
            | Self::COMPOSE.bits()
            | Self::PAINT.bits()
            | Self::ACCESSIBILITY.bits()
            | Self::HIT_TEST.bits(),
    );

    /// Every retained pass.
    pub const ALL: Self = Self::from_bits_retain(
        Self::TREE.bits()
            | Self::LAYOUT.bits()
            | Self::COMPOSE.bits()
            | Self::PAINT.bits()
            | Self::ACCESSIBILITY.bits()
            | Self::HIT_TEST.bits(),
    );
}

/// Min/max constraints supplied by a parent during layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    /// Minimum accepted size.
    pub min: LogicalSize,
    /// Maximum accepted size.
    pub max: LogicalSize,
}

impl Constraints {
    /// Creates finite normalized constraints.
    pub fn new(min: LogicalSize, max: LogicalSize) -> Self {
        let max = LogicalSize::new(max.width.max(0.0), max.height.max(0.0));
        let min = LogicalSize::new(
            min.width.clamp(0.0, max.width),
            min.height.clamp(0.0, max.height),
        );
        Self { min, max }
    }

    /// Constrains an element to one exact size.
    pub fn tight(size: LogicalSize) -> Self {
        Self::new(size, size)
    }

    /// Constrains `size` to this range.
    pub fn constrain(self, size: LogicalSize) -> LogicalSize {
        LogicalSize::new(
            size.width.clamp(self.min.width, self.max.width),
            size.height.clamp(self.min.height, self.max.height),
        )
    }

    /// Removes minimum constraints while retaining the maximum.
    pub fn loosen(self) -> Self {
        Self::new(LogicalSize::ZERO, self.max)
    }
}

/// Normalized input delivered to a hit element.
#[derive(Clone, Debug, PartialEq)]
pub enum UiInput {
    /// Pointer moved in window coordinates.
    PointerMoved(LogicalPoint),
    /// Primary pointer button was pressed.
    PointerPressed(LogicalPoint),
    /// Primary pointer button was released.
    PointerReleased(LogicalPoint),
    /// Pointer entered or left this element's active hit region.
    HoverChanged(bool),
    /// Pointer left the native window.
    PointerLeft,
    /// Pointer wheel moved at a window-coordinate position.
    PointerWheel {
        /// Pointer location used to choose the routed subtree.
        position: LogicalPoint,
        /// Logical-pixel displacement.
        delta: LogicalPoint,
    },
    /// Keyboard focus changed.
    FocusChanged(bool),
    /// A pressed keyboard key with the modifier state at dispatch time.
    Keyboard {
        /// Platform-normalized key data.
        input: KeyboardInput,
        /// Active modifiers.
        modifiers: Modifiers,
    },
    /// Input method composition changed.
    Ime(ImeEvent),
    /// Plain text read from the platform clipboard.
    Paste(String),
}

/// Platform clipboard mutation requested by a retained element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardOperation {
    /// Replace plain-text clipboard contents.
    WriteText(String),
}

/// Result of delivering one input event.
#[derive(Default)]
pub struct EventResult {
    /// Optional typed payload, erased only inside the retained runtime.
    pub action: Option<Box<dyn Any>>,
    /// Additional work requested by the element.
    pub invalidation: Invalidation,
    /// Optional platform clipboard mutation.
    pub clipboard: Option<ClipboardOperation>,
    /// Whether event propagation should stop.
    pub handled: bool,
}

impl EventResult {
    /// Emits a typed application payload.
    pub fn action(action: impl Any) -> Self {
        Self {
            action: Some(Box::new(action)),
            handled: true,
            ..Self::default()
        }
    }
}

/// Lifecycle implemented by retained elements.
pub trait Element: Any {
    /// Returns this element for typed retained access.
    fn as_any(&self) -> &dyn Any;

    /// Returns this element for typed retained mutation.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Computes size and lays out children.
    ///
    /// A child this method does not lay out - one past the arity the container
    /// documents, or one it skips conditionally - has its size and offset reset
    /// to zero, along with its whole subtree. Retaining the geometry it was last
    /// given would leave it painting and hit-testing somewhere that no longer
    /// describes anything; zero is what the same tree built from scratch gives
    /// it. Laying the child out again restores it.
    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize;

    /// Produces this element's local-coordinate paint fragment.
    fn paint(
        &self,
        _painter: &mut Painter,
        _size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        Ok(())
    }

    /// Supplies accessible properties for this element.
    fn accessibility(&self) -> Option<SemanticData> {
        None
    }

    /// Handles normalized input targeted at this element.
    fn event(&mut self, _input: UiInput) -> EventResult {
        EventResult::default()
    }

    /// Accessibility operations accepted by this element.
    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        Vec::new()
    }

    /// Applies an accessibility operation.
    fn semantic_action(&mut self, _action: SemanticAction) -> EventResult {
        EventResult::default()
    }

    /// Tests a point expressed in this element's local coordinates.
    fn hit_test(&self, point: LogicalPoint, size: LogicalSize) -> bool {
        LogicalRect::from_xywh(0.0, 0.0, size.width, size.height).contains(point)
    }

    /// Whether this element can become a pointer target.
    fn hit_testable(&self) -> bool {
        false
    }

    /// Whether this element participates in keyboard focus traversal.
    fn focusable(&self) -> bool {
        false
    }

    /// Optional element-local transform applied after layout translation.
    ///
    /// Composition is incremental and treats this as stable between updates. An
    /// implementation whose transform depends on mutable state must therefore
    /// report [`Invalidation::COMPOSE`] when that state changes, or the stale
    /// transform will be reused.
    fn transform(&self) -> Affine2 {
        Affine2::IDENTITY
    }

    /// Whether descendants are clipped to the local layout rectangle.
    ///
    /// Subject to the same incremental-composition contract as
    /// [`Element::transform`]: a value that varies with mutable state must be
    /// accompanied by [`Invalidation::COMPOSE`].
    fn clips_children(&self) -> bool {
        false
    }

    /// Preferred native cursor while this element is hovered or dragging.
    fn cursor_icon(&self) -> CursorIcon {
        CursorIcon::Default
    }

    /// Relative share of remaining main-axis space requested from a flex parent.
    fn flex_grow(&self) -> f32 {
        0.0
    }
}

/// Restricted access to retained children during layout.
pub struct LayoutContext<'a> {
    pub(crate) ui: &'a mut UiTree,
    pub(crate) current: NodeId,
}

impl LayoutContext<'_> {
    /// Returns retained children in paint order.
    ///
    /// This allocates. Prefer [`LayoutContext::child_count`] with
    /// [`LayoutContext::child_at`] when simply walking the children, which is
    /// the common case and stays allocation-free.
    pub fn children(&self) -> Vec<NodeId> {
        self.ui.children_ids(self.current)
    }

    /// Returns how many retained children this element has.
    pub fn child_count(&self) -> usize {
        self.ui.child_count(self.current)
    }

    /// Returns the child at `index` in paint order, if it exists.
    pub fn child_at(&self, index: usize) -> Option<NodeId> {
        self.ui.child_at(self.current, index)
    }

    /// Lays out one direct child.
    pub fn layout_child(&mut self, child: NodeId, constraints: Constraints) -> LogicalSize {
        self.ui.layout_child(self.current, child, constraints)
    }

    /// Assigns one direct child's local origin.
    pub fn place_child(&mut self, child: NodeId, origin: LogicalPoint) {
        self.ui.place_child(self.current, child, origin)
    }

    /// Returns a child's most recently computed size.
    pub fn child_size(&self, child: NodeId) -> LogicalSize {
        self.ui.child_size(self.current, child)
    }

    /// Returns a direct child's requested flex growth.
    pub fn child_flex_grow(&self, child: NodeId) -> f32 {
        self.ui.child_flex_grow(self.current, child)
    }

    /// Shapes and freezes text using the root's shared font and scratch state.
    pub fn shape_text(&mut self, request: TextLayoutRequest) -> TextLayout {
        self.ui.shape_text(request)
    }
}
