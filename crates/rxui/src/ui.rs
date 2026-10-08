use crate::{
    AccessError, Dispatch, Entity, Mount, ResolvedPaint, Runtime, StyleColor, TextChangeEvent,
    TextInputEvent, TextInputInfo, TextMovement, TextPosition, TextSelection, TextSubmitEvent,
    Theme, ThemeColor, ViewContext,
    editing::{
        Composition, EditAction, EditKind, Editor, Pending, Proposal, after_boundary, boundary,
        next, previous, single_line, word_left, word_right, word_selection,
    },
    element::{ClickEvent, Color, Element, ElementKind, IntoElement, Key, View},
    id::next_runtime,
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
};
use taffy::{TaffyTree, prelude::*};

/// Failure during description preparation, measurement, or input dispatch.
#[derive(Debug)]
pub enum UiError {
    /// Invalid overlay anchor coordinates, spacing or viewport margin.
    InvalidOverlay,
    /// A geometry anchor is bound more than once in one Ui.
    DuplicateAnchorHandle,
    /// A scope has duplicate command types or ambiguous shortcut chords.
    AmbiguousCommand,
    /// Dock pane minima, divider thickness or drag threshold are invalid.
    InvalidDockConfiguration,
    /// A focus handle is bound to multiple elements in one placement.
    DuplicateFocusHandle,
    /// Tab keys are duplicated within a tab group.
    DuplicateTabKey,
    /// A selected tab is absent or disabled.
    InvalidTabSelection,
    /// Entity/mount access failed.
    Access(AccessError),
    /// Siblings contain the same explicit key.
    DuplicateKey(Key),
    /// A component attempts to mount itself or an ancestor recursively.
    RecursiveComponent(crate::EntityId),
    /// A style has non-finite, out-of-range, or negative size/spacing values.
    InvalidStyle,
    /// Invalid image dimensions, byte length or placement parameters.
    InvalidImage,
    /// Encoded image could not be decoded.
    #[cfg(feature = "image-decoding")]
    ImageDecode(Box<dyn Error>),
    /// A button contains another button/input, also through a component boundary.
    NestedControl,
    /// A label/image/input/component/caption-button leaf was given children.
    LeafChildren,
    /// Viewport or input coordinates are invalid.
    InvalidGeometry,
    /// Application supplied a single-line input value containing control characters.
    InvalidTextValue,
    /// Opacity is non-finite or outside 0..=1.
    InvalidOpacity,
    /// A handle was attached to an element without scrolling enabled.
    InvalidScrollHandle,
    /// The same scroll handle was bound twice inside one UI placement.
    DuplicateScrollHandle,
    /// Metric-dependent view layout did not settle within the preparation budget.
    UnstableControlLayout,
    /// Invalid split extent/minima or scrollbar geometry.
    InvalidRangeControl,
    /// Group opacity requires the scoped frame composition API.
    #[cfg(feature = "rendering")]
    CompositionRequired,
    /// Host text measurement failed.
    Measurement(Box<dyn Error>),
    /// Internal Taffy operation failed.
    Layout(taffy::TaffyError),
    /// Astrelis pipeline/recording failure.
    #[cfg(feature = "rendering")]
    Graphics(astrelis::Error),
    /// Astrelis CPU text failure.
    #[cfg(feature = "rendering")]
    Text(astrelis::TextError),
    /// Astrelis text preparation failure.
    #[cfg(feature = "rendering")]
    TextRender(astrelis::TextRenderError),
}
impl From<AccessError> for UiError {
    fn from(value: AccessError) -> Self {
        Self::Access(value)
    }
}
impl From<taffy::TaffyError> for UiError {
    fn from(value: taffy::TaffyError) -> Self {
        Self::Layout(value)
    }
}
impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOverlay => f.write_str("invalid overlay configuration"),
            Self::DuplicateAnchorHandle => f.write_str("duplicate geometry anchor in one UI"),
            Self::AmbiguousCommand => {
                f.write_str("duplicate command type or shortcut in one scope")
            }
            Self::InvalidDockConfiguration => {
                f.write_str("invalid dock pane minima, divider thickness or drag threshold")
            }
            Self::DuplicateFocusHandle => f.write_str("duplicate focus handle in one UI placement"),
            Self::DuplicateTabKey => f.write_str("duplicate key in tab group"),
            Self::InvalidTabSelection => f.write_str("selected tab must exist and be enabled"),
            Self::Access(e) => e.fmt(f),
            Self::Layout(e) => e.fmt(f),
            Self::Measurement(e) => e.fmt(f),
            Self::DuplicateKey(key) => write!(f, "duplicate sibling key: {key:?}"),
            Self::RecursiveComponent(id) => write!(f, "recursive component placement: {id:?}"),
            #[cfg(feature = "rendering")]
            Self::Graphics(e) => e.fmt(f),
            #[cfg(feature = "rendering")]
            Self::Text(e) => e.fmt(f),
            #[cfg(feature = "rendering")]
            Self::TextRender(e) => e.fmt(f),
            Self::InvalidTextValue => {
                f.write_str("single-line input value contains control characters")
            }
            Self::InvalidStyle => f.write_str("invalid RXUI element style"),
            Self::NestedControl => f.write_str("buttons cannot contain interactive controls"),
            Self::InvalidImage => f.write_str("invalid image data or placement"),
            #[cfg(feature = "image-decoding")]
            Self::ImageDecode(e) => e.fmt(f),
            Self::LeafChildren => f.write_str(
                "leaf elements cannot contain children; compose button content with a container",
            ),
            Self::InvalidRangeControl => f.write_str("invalid split or scrollbar configuration"),
            Self::InvalidScrollHandle => f.write_str("scroll handle requires a scrolling viewport"),
            Self::DuplicateScrollHandle => {
                f.write_str("scroll handle is bound more than once in this UI")
            }
            Self::UnstableControlLayout => {
                f.write_str("metric-dependent control layout did not settle")
            }
            Self::InvalidOpacity => f.write_str("opacity must be finite and within 0..=1"),
            #[cfg(feature = "rendering")]
            Self::CompositionRequired => f.write_str("group opacity requires UiPainter::compose"),
            Self::InvalidGeometry => f.write_str("invalid RXUI viewport or input geometry"),
        }
    }
}
impl Error for UiError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Access(e) => Some(e),
            Self::Layout(e) => Some(e),
            Self::Measurement(e) => Some(e.as_ref()),
            #[cfg(feature = "image-decoding")]
            Self::ImageDecode(e) => Some(e.as_ref()),
            #[cfg(feature = "rendering")]
            Self::Graphics(e) => Some(e),
            #[cfg(feature = "rendering")]
            Self::Text(e) => Some(e),
            #[cfg(feature = "rendering")]
            Self::TextRender(e) => Some(e),
            _ => None,
        }
    }
}

/// Stable retained node identity, unique within one Ui tree and never recycled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ElementId {
    pub(crate) tree: u64,
    serial: u64,
}

/// Logical rectangle, with X right and Y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}
impl Bounds {
    /// Intersection with another axis-aligned rectangle, retaining zero-area results.
    pub fn intersection(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self {
            x,
            y,
            width: ((self.x + self.width).min(other.x + other.width) - x).max(0.),
            height: ((self.y + self.height).min(other.y + other.height) - y).max(0.),
        }
    }
    /// Half-open containment, excluding zero-area rectangles.
    pub fn contains(&self, point: [f32; 2]) -> bool {
        point[0] >= self.x
            && point[1] >= self.y
            && point[0] < self.x + self.width
            && point[1] < self.y + self.height
    }
}

/// Rounded descendant clip from the nearest clipping ancestor with corner radii.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedClip {
    /// Ancestor content box in absolute logical units.
    pub bounds: Bounds,
    /// Ancestor radii inset by its border and padding, clockwise from the top-left.
    pub radii: [f32; 4],
}
/// Width request from Taffy's leaf measurement algorithm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextWidth {
    /// Smallest intrinsic width.
    MinContent,
    /// Unconstrained intrinsic width.
    MaxContent,
    /// Available content width in logical units.
    Available(f32),
}
/// Measurement input; owned text is retained by the UI node.
#[derive(Clone, Copy, Debug)]
pub struct TextRequest<'a> {
    /// Current leaf text.
    pub text: &'a str,
    /// Leaf font size in logical units.
    pub font_size: f32,
    /// Width constraint for intrinsic sizing/wrapping.
    pub width: TextWidth,
    /// Disable wrapping for single-line editing geometry.
    pub single_line: bool,
    /// Resolved family, weight, slope, line height and alignment.
    pub style: &'a crate::TextStyle,
    /// Retained text/font revision, scoped to the element identity.
    pub revision: u64,
}
/// Host-supplied text sizing, allowing headless tests and real Astrelis shaping.
pub trait TextMeasure {
    /// Measures text content, excluding node padding/border.
    fn measure(&mut self, id: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError>;
    /// Whether missing geometry denotes a real edge rather than an unsupported adapter.
    fn supports_text_geometry(&self) -> bool {
        false
    }
    /// Optional shaped-text hit test in local text units. None uses an end-of-value fallback.
    fn text_hit_test(
        &mut self,
        _id: ElementId,
        _request: TextRequest<'_>,
        _point: [f32; 2],
    ) -> Result<Option<TextPosition>, UiError> {
        Ok(None)
    }
    /// Optional shaped-text caret geometry in local units.
    fn text_caret(
        &mut self,
        _id: ElementId,
        _request: TextRequest<'_>,
        _position: TextPosition,
    ) -> Result<Option<Bounds>, UiError> {
        Ok(None)
    }
    /// Optional visually adjacent caret, preserving bidirectional affinity.
    fn text_neighbor(
        &mut self,
        _id: ElementId,
        _request: TextRequest<'_>,
        _position: TextPosition,
        _right: bool,
    ) -> Result<Option<TextPosition>, UiError> {
        Ok(None)
    }
    /// Changes when fonts or other external measurement inputs change.
    /// A new generation invalidates all text leaves; default zero means stable inputs.
    fn generation(&self) -> u64 {
        0
    }
}

/// Retained element kind, independent of user component types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementType {
    /// Horizontal flex container.
    Row,
    /// Vertical flex container.
    Column,
    /// Overlapping single-cell container.
    Stack,
    /// Text leaf.
    Label,
    /// Shared raster/GPU image leaf.
    Image,
    /// Activatable caption or composed-content button.
    Button,
    /// Controlled single-line editable text leaf.
    TextInput,
    /// Stateful component boundary.
    Component,
    /// Scroll viewport control.
    Scrollbar,
    /// Controlled pane divider.
    Splitter,
    /// Application-defined leaf from [`crate::custom`].
    Custom,
}
/// Read-only snapshot for painting/inspection; iteration follows tree paint order.
pub struct ElementInfo<'a> {
    /// Viewport overlay boundary. Logical ownership is retained, while custom
    /// painters reset ancestor clipping/opacity here.
    pub viewport_overlay: bool,
    /// Stable identity.
    pub id: ElementId,
    /// Description-tree parent, independent of sibling paint order.
    pub parent: Option<ElementId>,
    /// Local group opacity, applied once to this complete painted subtree.
    pub opacity: f32,
    /// Explicit sibling key, if provided.
    pub key: Option<&'a Key>,
    /// Element kind.
    pub kind: ElementType,
    /// Absolute logical border bounds.
    pub bounds: Bounds,
    /// Absolute logical content bounds, excluding padding and border.
    pub content_bounds: Bounds,
    /// Effective ancestor/viewport clip for this node's own painting and hit testing.
    pub clip_bounds: Bounds,
    /// Rounded painting clip from the nearest rounded clipping ancestor, within
    /// clip_bounds. Hit testing stays rectangular.
    pub rounded_clip: Option<RoundedClip>,
    /// Retained logical scroll offset; zero for ordinary elements.
    pub scroll_offset: [f32; 2],
    /// Maximum reachable scroll offset after layout.
    pub scroll_range: [f32; 2],
    /// Owned node text, borrowed for the snapshot.
    pub text: Option<&'a str>,
    /// Sibling-scoped paint order; layout and focus retain description order.
    pub z_index: i32,
    /// Pointer targeting policy, independent of keyboard/semantic focus.
    pub pointer_events: crate::PointerEvents,
    /// Effective subtree inertness; painting and geometry are preserved.
    pub inert: bool,
    /// Image source and resolved placement, absent for other kinds.
    pub image: Option<crate::ImageInfo<'a>>,
    /// Selection/composition retained by this text input placement.
    pub editing: Option<TextInputInfo>,
    /// Leaf font size.
    pub font_size: f32,
    /// Resolved inherited font family, weight, slope, line height and alignment.
    pub text_style: &'a crate::TextStyle,
    /// Monotonic text/font revision for constant-time prepared-resource validation.
    pub text_revision: u64,
    /// Leaf text color.
    pub color: Color,
    /// Optional background fill.
    pub background: Option<Color>,
    /// Resolved paint values for the current interaction state.
    pub paint: ResolvedPaint,
    /// Resolved border widths in left, right, top, bottom order.
    pub border: [f32; 4],
    /// Whether button activation is disabled.
    pub disabled: bool,
    /// Whether this button is under the pointer.
    pub hovered: bool,
    /// Whether this button has pointer capture.
    pub pressed: bool,
    /// Whether this element has keyboard focus.
    pub focused: bool,
    /// Whether the underlying element participates in focus navigation.
    pub focusable: bool,
    /// Numeric control geometry, when this element is a scrollbar/divider.
    pub range: Option<crate::RangeInfo>,
}
/// Cumulative preparation counters, excluding host text/GPU work inside callbacks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiStats {
    /// Component descriptions evaluated.
    pub component_evaluations: u64,
    /// New retained nodes allocated.
    pub created_nodes: u64,
    /// Retained nodes reused by reconciliation.
    pub reused_nodes: u64,
    /// Removed retained nodes.
    pub removed_nodes: u64,
    /// Retained nodes whose base/state styling was resolved.
    pub style_resolutions: u64,
    /// Host text measurement requests.
    pub measurements: u64,
    /// Taffy layout computations requested.
    pub layout_passes: u64,
}
/// Logical mouse input. Legacy short variants represent an unmodified primary mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEvent {
    /// Pointer moved.
    Moved([f32; 2]),
    /// Mouse motion with a source-window modifier snapshot.
    Motion {
        /// Logical position.
        position: [f32; 2],
        /// Modifiers.
        modifiers: crate::Modifiers,
    },
    /// General mouse button press.
    Down {
        /// Logical position.
        position: [f32; 2],
        /// Changed button.
        button: crate::PointerButton,
        /// Modifiers.
        modifiers: crate::Modifiers,
    },
    /// General mouse button release.
    Up {
        /// Logical position.
        position: [f32; 2],
        /// Changed button.
        button: crate::PointerButton,
        /// Modifiers.
        modifiers: crate::Modifiers,
    },
    /// Primary button pressed.
    Pressed([f32; 2]),
    /// Primary button released.
    Released([f32; 2]),
    /// Pointer left the window; capture remains until release/cancellation.
    Left,
    /// Capture was cancelled, for example when the window lost focus.
    Cancelled,
}

pub(crate) trait Component {
    fn entity_id(&self) -> crate::EntityId;
    fn mount(&self, runtime: &mut Runtime) -> Result<Box<dyn MountedView>, AccessError>;
}
pub(crate) struct ViewEntity<T: View>(pub Entity<T>);
impl<T: View> Component for ViewEntity<T> {
    fn entity_id(&self) -> crate::EntityId {
        self.0.id()
    }
    fn mount(&self, runtime: &mut Runtime) -> Result<Box<dyn MountedView>, AccessError> {
        Ok(Box::new(runtime.update(|cx| cx.mount(&self.0))?))
    }
}
pub(crate) trait MountedView {
    fn mount_id(&self) -> crate::MountId;
    fn dirty(&self, runtime: &Runtime) -> Result<bool, AccessError>;
    fn evaluate(&self, runtime: &mut Runtime) -> Result<Element, UiError>;
}
fn describe<T: View>(value: &T, cx: &mut ViewContext<'_, T>) -> Result<Element, UiError> {
    let element = value.view(cx).into_element();
    element.validate()?;
    Ok(element)
}
impl<T: View> MountedView for Mount<T> {
    fn mount_id(&self) -> crate::MountId {
        self.id()
    }
    fn dirty(&self, runtime: &Runtime) -> Result<bool, AccessError> {
        runtime.is_dirty(self)
    }
    fn evaluate(&self, runtime: &mut Runtime) -> Result<Element, UiError> {
        runtime.evaluate_checked(self, describe)
    }
}
fn kind(element: &Element) -> ElementType {
    match element.kind {
        ElementKind::Row => ElementType::Row,
        ElementKind::Column => ElementType::Column,
        ElementKind::Stack => ElementType::Stack,
        ElementKind::Scrollbar(_) => ElementType::Scrollbar,
        ElementKind::Splitter(_) => ElementType::Splitter,
        ElementKind::Label(_) => ElementType::Label,
        ElementKind::Image(_) => ElementType::Image,
        ElementKind::TextInput { .. } => ElementType::TextInput,
        ElementKind::Button { .. } => ElementType::Button,
        ElementKind::Component(_) => ElementType::Component,
        ElementKind::Custom(_) => ElementType::Custom,
    }
}
fn text(element: &Element) -> Option<&str> {
    match &element.kind {
        ElementKind::Label(text) | ElementKind::TextInput { value: text, .. } => Some(text),
        ElementKind::Button { text, .. } => text.as_deref(),
        _ => None,
    }
}
fn image_properties(element: &Element) -> Option<&crate::image::Properties> {
    if let ElementKind::Image(props) = &element.kind {
        Some(props)
    } else {
        None
    }
}
fn compatible(old: &Element, new: &Element) -> bool {
    match (&old.kind, &new.kind) {
        (ElementKind::Component(a), ElementKind::Component(b)) => a.entity_id() == b.entity_id(),
        (ElementKind::Custom(a), ElementKind::Custom(b)) => {
            std::any::Any::type_id(a.as_any()) == std::any::Any::type_id(b.as_any())
        }
        _ => kind(old) == kind(new),
    }
}
pub(crate) struct Node {
    pub(crate) element: Element,
    layout: NodeId,
    children: Vec<ElementId>,
    mounted: Option<Box<dyn MountedView>>,
    pub(crate) bounds: Bounds,
    pub(crate) content_bounds: Bounds,
    pub(crate) visible: bool,
    needs_evaluation: bool,
    ancestors: Vec<crate::EntityId>,
    text_revision: u64,
    parent: Option<ElementId>,
    clip_bounds: Bounds,
    rounded_clip: Option<RoundedClip>,
    pub(crate) scroll_offset: [f32; 2],
    pub(crate) scroll_range: [f32; 2],
    editor: Option<Box<Editor>>,
    resolved_theme: Theme,
    color_binding: StyleColor,
    font_binding: Option<f32>,
    font_size: f32,
    text_binding: crate::typography::Overrides,
    text_style: crate::TextStyle,
    paint: ResolvedPaint,
    state_paints: Option<Box<[ResolvedPaint; 3]>>,
    border: [f32; 4],
    layout_dirty: bool,
    image_size: [f32; 2],
    image_tint: Color,
    button_owner: Option<ElementId>,
    control_color: bool,
    inert: bool,
    pointer_allowed: bool,
    button_name: String,
}

impl Node {
    fn displayed_text(&self) -> Option<&str> {
        self.editor
            .as_ref()
            .map(|e| e.display.as_str())
            .or_else(|| text(&self.element))
    }
    fn text_request(&self) -> TextRequest<'_> {
        TextRequest {
            text: self.displayed_text().unwrap_or(""),
            font_size: self.font_size,
            width: TextWidth::Available(self.content_bounds.width),
            single_line: self.editor.is_some(),
            style: &self.text_style,
            revision: self.text_revision,
        }
    }
}

/// One retained UI placement, with keyed reconciliation and Taffy flex layout.
/// It owns no window, fonts, device, event loop, or executor. Prepare before routing
/// geometry-based input, and flush Runtime effects at the host's update boundary.
pub struct Ui<T: View> {
    owner: Mount<T>,
    pub(crate) tree: u64,
    next_node: u64,
    root: Option<ElementId>,
    pub(crate) nodes: HashMap<ElementId, Node>,
    order: Vec<ElementId>,
    paint_order: Option<Vec<ElementId>>,
    opacity_count: usize,
    taffy: TaffyTree<ElementId>,
    viewport: Option<[f32; 2]>,
    measurement_generation: u64,
    geometry_ready: bool,
    geometry_revision: u64,
    needs_evaluation: bool,
    input: input_dispatch::State,
    pub(crate) scrolling: Option<Box<crate::scrolling::Scrolling>>,
    runtime: std::rc::Weak<crate::runtime::RuntimeInner>,
    hovered: Option<ElementId>,
    pressed: Option<ElementId>,
    focused: Option<ElementId>,
    focus_state: Option<Box<focus::State>>,
    dock_drag: Option<Box<dock_dispatch::Drag>>,
    overlays: Option<Box<overlay_dispatch::State>>,
    stats: UiStats,
    active_views: Vec<crate::EntityId>,
    component_nodes: Vec<ElementId>,
    editor_nodes: Vec<ElementId>,
    image_nodes: Vec<ElementId>,
    live_image_nodes: Vec<ElementId>,
    pub(crate) scroll_handle_nodes: Vec<ElementId>,
    composed_buttons: Vec<ElementId>,
    order_dirty: bool,
    active: bool,
    caret_visible: bool,
    theme: Theme,
    style_roots: HashSet<ElementId>,
}
impl<T: View> Ui<T> {
    /// Mounts a root view. Preparing evaluates it for the first time.
    pub fn new(runtime: &mut Runtime, root: Entity<T>) -> Result<Self, UiError> {
        let owner = runtime.update(|cx| cx.mount(&root))?;
        let mut taffy = TaffyTree::new();
        taffy.disable_rounding();
        Ok(Self {
            owner,
            tree: next_runtime(),
            next_node: 1,
            root: None,
            nodes: HashMap::new(),
            order: Vec::new(),
            paint_order: None,
            opacity_count: 0,
            taffy,
            viewport: None,
            measurement_generation: 0,
            geometry_ready: false,
            geometry_revision: 0,
            needs_evaluation: true,
            input: input_dispatch::State::default(),
            scrolling: None,
            runtime: std::rc::Rc::downgrade(&runtime.inner),
            hovered: None,
            pressed: None,
            focused: None,
            focus_state: None,
            dock_drag: None,
            overlays: None,
            stats: UiStats::default(),
            active_views: Vec::new(),
            component_nodes: Vec::new(),
            editor_nodes: Vec::new(),
            image_nodes: Vec::new(),
            live_image_nodes: Vec::new(),
            scroll_handle_nodes: Vec::new(),
            composed_buttons: Vec::new(),
            order_dirty: true,
            active: true,
            caret_visible: true,
            theme: Theme::default(),
            style_roots: HashSet::new(),
        })
    }
    /// Root theme for this placement; subtree overrides remain independent.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }
    /// Installs a validated root theme without rebuilding views or resetting input.
    /// Prepare before painting/geometry input. Returns false for an identical theme.
    pub fn set_theme(&mut self, theme: Theme) -> Result<bool, UiError> {
        theme.validate()?;
        if self.theme == theme {
            return Ok(false);
        }
        self.theme = theme;
        if let Some(root) = self.root {
            self.style_roots.insert(root);
        }
        Ok(true)
    }
    /// Whether retained descriptions contain opacity below one (including hidden nodes).
    /// Custom GPU hosts use compose when this is true; CPU/custom painters can inspect
    /// ElementInfo.opacity and parent to implement their own subtree composition.
    pub fn needs_composition(&self) -> bool {
        self.opacity_count != 0
    }
    #[cfg(feature = "rendering")]
    pub(crate) fn tree_id(&self) -> u64 {
        self.tree
    }
    #[cfg(feature = "rendering")]
    pub(crate) fn composition_key(&self) -> [u64; 5] {
        [
            self.stats.component_evaluations,
            self.stats.layout_passes,
            self.stats.style_resolutions,
            self.geometry_revision,
            self.measurement_generation,
        ]
    }
    /// Root's persistent state, retained by this UI placement.
    pub fn entity(&self) -> &Entity<T> {
        self.owner.entity()
    }
    /// Current component placement identities, including the root. Native hosts use
    /// these to resolve the source window for a bound listener's update context.
    pub fn mount_ids(&self) -> impl Iterator<Item = crate::MountId> + '_ {
        std::iter::once(self.owner.id()).chain(self.component_nodes.iter().filter_map(|id| {
            self.nodes
                .get(id)?
                .mounted
                .as_ref()
                .map(|mount| mount.mount_id())
        }))
    }
    /// Cumulative counters, suitable for before/after comparisons.
    pub fn stats(&self) -> UiStats {
        self.stats
    }
    /// External text measurement generation used by the prepared geometry snapshot.
    pub fn measurement_generation(&self) -> u64 {
        self.measurement_generation
    }
    /// Whether this placement has unevaluated component work or unavailable geometry.
    /// Hosts also account for viewport, font-generation and visual-input changes.
    pub fn needs_prepare(&self, runtime: &Runtime) -> Result<bool, AccessError> {
        if self.scroll_commands_pending()
            || self.overlay_pending()
            || self.input.pending()
            || self.focus_state.as_ref().is_some_and(|s| s.pending())
        {
            return Ok(true);
        }
        if !self.geometry_ready
            || self.needs_evaluation
            || !self.style_roots.is_empty()
            || runtime.is_dirty(&self.owner)?
            || self
                .root
                .is_some_and(|id| self.taffy.dirty(self.nodes[&id].layout).unwrap_or(true))
        {
            return Ok(true);
        }
        for id in &self.live_image_nodes {
            if let Some(node) = self.nodes.get(id)
                && let Some(props) = image_properties(&node.element)
                && let Some(size) = props.source.pixel_size()
                && node.image_size != [size[0] as f32, size[1] as f32]
            {
                return Ok(true);
            }
        }
        for id in &self.component_nodes {
            if let Some(node) = self.nodes.get(id)
                && let Some(mounted) = &node.mounted
                && (node.needs_evaluation || mounted.dirty(runtime)?)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// Invalidates geometry after a viewport/DPI lifecycle change. State and retained
    /// identities survive, but geometry-based input waits for successful preparation.
    pub fn invalidate_geometry(&mut self) {
        self.geometry_ready = false;
    }
    /// Whether an identity belongs to this placement, also after node removal.
    pub fn owns_element(&self, id: ElementId) -> bool {
        id.tree == self.tree
    }
    /// Evaluates dirty components, reconciles descriptions, and computes affected layout.
    /// Invalid descriptions are rejected before that component's dependencies commit.
    /// A later child/measurement failure can follow earlier committed components;
    /// preparation is not a whole-tree rollback transaction. Retry after correction.
    pub fn prepare(
        &mut self,
        runtime: &mut Runtime,
        viewport: [f32; 2],
        measurer: &mut impl TextMeasure,
    ) -> Result<(), UiError> {
        profiling::scope!("rxui::Ui::prepare");
        if !self.geometry_ready {
            self.clean_partial_tree();
        }
        self.geometry_ready = false;
        let force_evaluation = self.needs_evaluation;
        // Retain retry intent even if a user view or measurer unwinds.
        self.needs_evaluation = true;
        for attempt in 0..4 {
            let result = self.prepare_inner(
                runtime,
                viewport,
                measurer,
                force_evaluation && attempt == 0,
            );
            if result.is_err() {
                self.needs_evaluation = true;
                self.clean_partial_tree();
                return result;
            }
            self.geometry_ready = false;
            self.needs_evaluation = true;
            let post = (|| {
                self.publish_focus()?;
                self.sync_overlay_focus();
                self.restore_dock_drop_focus();
                self.apply_focus_commands();
                self.publish_scroll(runtime)?;
                self.apply_scroll_commands()?;
                self.clear_invalid_interaction();
                self.cancel_invalid_capture();
                self.refresh_dock_drag();
                self.flush_input_cancellations(runtime)?;
                Ok::<_, UiError>(())
            })();
            if let Err(error) = post {
                self.clean_partial_tree();
                return Err(error);
            }
            self.geometry_ready = true;
            self.needs_evaluation = false;
            if !self.needs_prepare(runtime)? {
                return Ok(());
            }
            self.geometry_ready = false;
        }
        self.needs_evaluation = true;
        Err(UiError::UnstableControlLayout)
    }
    fn clean_partial_tree(&mut self) {
        let ids: HashSet<_> = self.nodes.keys().copied().collect();
        for node in self.nodes.values_mut() {
            node.children.retain(|id| ids.contains(id));
        }
        if self.root.is_some_and(|id| !ids.contains(&id)) {
            self.root = None;
        }
    }
    fn prepare_inner(
        &mut self,
        runtime: &mut Runtime,
        viewport: [f32; 2],
        measurer: &mut impl TextMeasure,
        force_evaluation: bool,
    ) -> Result<(), UiError> {
        if viewport.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err(UiError::InvalidGeometry);
        }
        let before_focus = self.focused;
        self.snapshot_overlay_focus();
        let focus_anchor = self.tab_anchor();
        let evaluated = self.stats.component_evaluations;
        self.refresh_descriptions(runtime, force_evaluation)?;
        self.resolve_styles()?;
        self.cancel_invalid_capture();
        if self.flush_input_cancellations(runtime)? {
            self.refresh_descriptions(runtime, false)?;
            self.resolve_styles()?;
        }
        self.refresh_images(evaluated != self.stats.component_evaluations)?;
        let root = self.root.expect("initially dirty root");
        if self.order_dirty {
            self.rebuild_order(root);
        }
        self.collect_overlays(before_focus, viewport)?;
        let generation = measurer.generation();
        if generation != self.measurement_generation {
            for node in self.nodes.values() {
                if text(&node.element).is_some() {
                    self.taffy.mark_dirty(node.layout)?;
                }
            }
        }
        let layout_root = self.nodes[&root].layout;
        if self.viewport != Some(viewport) || self.taffy.dirty(layout_root)? {
            self.layout_subtree(layout_root, viewport, measurer)?;
            self.update_bounds(
                root,
                [0., 0.],
                true,
                Bounds {
                    x: 0.,
                    y: 0.,
                    width: viewport[0],
                    height: viewport[1],
                },
            )?;
        }
        let portals = self
            .overlays
            .as_ref()
            .map(|s| s.roots.clone())
            .unwrap_or_default();
        for id in portals {
            let layout = self.nodes[&id].layout;
            if self.viewport != Some(viewport) || self.taffy.dirty(layout)? {
                self.layout_subtree(layout, viewport, measurer)?;
            }
        }
        self.refresh_overlay_bounds(viewport)?;
        self.dismiss_unavailable_overlays(runtime)?;
        self.viewport = Some(viewport);
        self.measurement_generation = generation;
        if self.order_dirty {
            self.rebuild_order(root);
        }
        self.restore_tab_focus(focus_anchor);
        self.clear_invalid_interaction();
        self.editor_nodes.retain(|id| self.nodes.contains_key(id));
        for index in 0..self.editor_nodes.len() {
            self.update_text_scroll(self.editor_nodes[index], measurer)?;
        }
        self.geometry_ready = true;
        self.needs_evaluation = false;
        Ok(())
    }
    fn layout_subtree(
        &mut self,
        layout_root: NodeId,
        viewport: [f32; 2],
        measurer: &mut impl TextMeasure,
    ) -> Result<(), UiError> {
        profiling::scope!("rxui::layout");
        let nodes = &self.nodes;
        let mut error = None;
        let measurements = &mut self.stats.measurements;
        self.taffy.compute_layout_with_measure(
            layout_root,
            Size {
                width: AvailableSpace::Definite(viewport[0]),
                height: AvailableSpace::Definite(viewport[1]),
            },
            |inputs, _, context, style| {
                taffy::compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.,
                    |known, available| {
                        let Some(id) = context.as_deref() else {
                            return Size::ZERO;
                        };
                        let node = &nodes[id];
                        if let ElementKind::Image(props) = &node.element.kind {
                            let [w, h] = [
                                node.image_size[0] * props.uv[2],
                                node.image_size[1] * props.uv[3],
                            ];
                            return match (known.width, known.height) {
                                (Some(width), Some(height)) => Size { width, height },
                                (Some(width), None) => Size {
                                    width,
                                    height: if w > 0. { width * h / w } else { 0. },
                                },
                                (None, Some(height)) => Size {
                                    width: if h > 0. { height * w / h } else { 0. },
                                    height,
                                },
                                _ => Size {
                                    width: w,
                                    height: h,
                                },
                            };
                        }
                        if let ElementKind::Custom(custom) = &node.element.kind {
                            let size = custom.measure(crate::CustomMeasure {
                                known: [known.width, known.height],
                                available: [available.width, available.height],
                                font_size: node.font_size,
                            });
                            if size.iter().any(|v| !v.is_finite() || *v < 0.) {
                                error.get_or_insert(UiError::InvalidGeometry);
                                return Size::ZERO;
                            }
                            return Size {
                                width: known.width.unwrap_or(size[0]),
                                height: known.height.unwrap_or(size[1]),
                            };
                        }
                        let Some(text) = node.displayed_text() else {
                            return Size::ZERO;
                        };
                        if let Size {
                            width: Some(width),
                            height: Some(height),
                        } = known
                        {
                            return Size { width, height };
                        }
                        // Intrinsic grid probes can supply a known width below
                        // zero after ancestor padding. Text's content constraint
                        // is zero in that case, matching definite available space.
                        let width = known
                            .width
                            .map(|width| TextWidth::Available(width.max(0.)))
                            .unwrap_or(match available.width {
                                AvailableSpace::MinContent => TextWidth::MinContent,
                                AvailableSpace::MaxContent => TextWidth::MaxContent,
                                AvailableSpace::Definite(value) => {
                                    TextWidth::Available(value.max(0.))
                                }
                            });
                        *measurements += 1;
                        match measurer.measure(
                            *id,
                            TextRequest {
                                text,
                                font_size: node.font_size,
                                width,
                                single_line: node.editor.is_some(),
                                style: &node.text_style,
                                revision: node.text_revision,
                            },
                        ) {
                            Ok(size) if size.iter().all(|v| v.is_finite() && *v >= 0.) => Size {
                                width: known.width.unwrap_or(size[0]),
                                height: known.height.unwrap_or(size[1]),
                            },
                            Ok(_) => {
                                error.get_or_insert(UiError::InvalidGeometry);
                                Size::ZERO
                            }
                            Err(e) => {
                                error.get_or_insert(e);
                                Size::ZERO
                            }
                        }
                    },
                )
            },
        )?;
        self.stats.layout_passes += 1;
        if let Some(error) = error {
            self.taffy.mark_dirty(layout_root)?;
            return Err(error);
        }
        Ok(())
    }
    fn refresh_descriptions(
        &mut self,
        runtime: &mut Runtime,
        force_evaluation: bool,
    ) -> Result<(), UiError> {
        profiling::scope!("rxui::evaluate_views");
        let before = self.stats.component_evaluations;
        self.active_views.clear();
        self.active_views.push(self.owner.entity().id());
        if runtime.is_dirty(&self.owner)? || force_evaluation {
            let description = runtime.evaluate_checked(&self.owner, describe)?;
            self.stats.component_evaluations += 1;
            self.reconcile(runtime, self.root, description, None)?;
        }
        self.component_nodes
            .retain(|id| self.nodes.contains_key(id));
        let mut index = 0;
        while index < self.component_nodes.len() {
            let id = self.component_nodes[index];
            if self.nodes.contains_key(&id) {
                self.refresh_component(runtime, id)?;
            }
            index += 1;
        }
        if before != self.stats.component_evaluations {
            self.composed_buttons.retain(|id| {
                self.nodes.get(id).is_some_and(|n| {
                    matches!(n.element.kind, ElementKind::Button { text: None, .. })
                })
            });
            self.composed_buttons.sort_unstable_by_key(|id| id.serial);
            self.composed_buttons.dedup();
            for id in self.composed_buttons.clone() {
                let mut labels = Vec::new();
                self.button_labels(id, &mut labels);
                let name = labels.join(" ");
                self.nodes.get_mut(&id).unwrap().button_name = name;
            }
        }
        Ok(())
    }
    fn button_labels(&self, id: ElementId, labels: &mut Vec<String>) {
        let node = &self.nodes[&id];
        if node.element.style.display == Display::None
            || node.element.semantics.as_ref().is_some_and(|s| s.hidden)
        {
            return;
        }
        if let ElementKind::Label(text) = &node.element.kind {
            labels.push(text.clone());
        }
        for child in &node.children {
            self.button_labels(*child, labels);
        }
    }
    fn reconcile(
        &mut self,
        runtime: &mut Runtime,
        old: Option<ElementId>,
        mut element: Element,
        parent: Option<ElementId>,
    ) -> Result<ElementId, UiError> {
        if parent.is_some_and(|id| matches!(self.nodes[&id].element.kind, ElementKind::Stack))
            && element.style.position != taffy::style::Position::Absolute
        {
            element.style.grid_row = taffy::prelude::line(1);
            element.style.grid_column = taffy::prelude::line(1);
        }
        if matches!(
            element.kind,
            ElementKind::Button { .. }
                | ElementKind::TextInput { .. }
                | ElementKind::Scrollbar(_)
                | ElementKind::Splitter(_)
        ) {
            let mut ancestor = parent;
            while let Some(id) = ancestor {
                let node = &self.nodes[&id];
                if matches!(node.element.kind, ElementKind::Button { .. }) {
                    return Err(UiError::NestedControl);
                }
                ancestor = node.parent;
            }
        }
        if let ElementKind::Component(component) = &element.kind
            && self.active_views.contains(&component.entity_id())
        {
            return Err(UiError::RecursiveComponent(component.entity_id()));
        }
        let descriptions = std::mem::take(&mut element.children);
        let id = if let Some(id) = old
            && compatible(&self.nodes[&id].element, &element)
        {
            let node = self.nodes.get_mut(&id).unwrap();
            self.order_dirty |= node.element.z_index != element.z_index
                || node
                    .element
                    .input
                    .as_ref()
                    .and_then(|p| p.overlay.as_ref())
                    .map(|p| p.modal)
                    != element
                        .input
                        .as_ref()
                        .and_then(|p| p.overlay.as_ref())
                        .map(|p| p.modal);
            node.layout_dirty |= node.element.style != element.style
                || node.element.layout_overrides != element.layout_overrides;
            if node.element.style != element.style
                || node.element.paint != element.paint
                || node.element.font_size != element.font_size
                || node.element.text_style != element.text_style
                || node.element.theme != element.theme
                || node.element.inert != element.inert
                || node.element.pointer_events != element.pointer_events
                || node.element.button_variant != element.button_variant
                || node.element.states != element.states
                || image_properties(&node.element) != image_properties(&element)
                || node.element.layout_overrides != element.layout_overrides
            {
                self.style_roots.insert(id);
            }
            if let (Some(old), Some(new)) =
                (image_properties(&node.element), image_properties(&element))
                && (old.uv[2..] != new.uv[2..])
                && (element.style.size.width.is_auto() || element.style.size.height.is_auto())
            {
                self.taffy.mark_dirty(node.layout)?;
            }
            if let (ElementKind::Custom(old), ElementKind::Custom(new)) =
                (&node.element.kind, &element.kind)
                && !old.same(new.as_ref())
            {
                self.taffy.mark_dirty(node.layout)?;
            }
            if text(&node.element) != text(&element) {
                self.taffy.mark_dirty(node.layout)?;
                node.text_revision = node
                    .text_revision
                    .checked_add(1)
                    .expect("RXUI text revision exhausted");
            }
            if let (
                Some(editor),
                ElementKind::TextInput { value: old, .. },
                ElementKind::TextInput {
                    value: new,
                    read_only,
                    disabled,
                    change,
                    ..
                },
            ) = (&mut node.editor, &node.element.kind, &element.kind)
            {
                if (*read_only || *disabled || change.is_none()) && editor.cancel_composition() {
                    editor.rebuild(new);
                    node.text_revision = node
                        .text_revision
                        .checked_add(1)
                        .expect("RXUI text revision exhausted");
                    self.taffy.mark_dirty(node.layout)?;
                }
                editor.reconcile(old, new);
            }
            if matches!(element.kind, ElementKind::Button { text: None, .. })
                && !matches!(node.element.kind, ElementKind::Button { text: None, .. })
            {
                self.composed_buttons.push(id);
            }
            self.opacity_count += usize::from(element.opacity < 1.);
            self.opacity_count -= usize::from(node.element.opacity < 1.);
            if node.element.scroll_handle.is_none() && element.scroll_handle.is_some() {
                self.scroll_handle_nodes.push(id);
            }
            node.element = element;
            self.stats.reused_nodes += 1;
            id
        } else {
            if let Some(old) = old {
                self.remove(old)?;
            }
            let id = ElementId {
                tree: self.tree,
                serial: self.next_node,
            };
            self.next_node = self
                .next_node
                .checked_add(1)
                .expect("RXUI element identity exhausted");
            let mounted = if let ElementKind::Component(component) = &element.kind {
                Some(component.mount(runtime)?)
            } else {
                None
            };
            let ancestors = if mounted.is_some() {
                self.component_nodes.push(id);
                self.active_views.clone()
            } else {
                Vec::new()
            };
            let layout = self
                .taffy
                .new_leaf_with_context(element.style.clone(), id)?;
            let editor = if let ElementKind::TextInput { value, .. } = &element.kind {
                self.editor_nodes.push(id);
                Some(Box::new(Editor::new(value)))
            } else {
                None
            };
            if matches!(element.kind, ElementKind::Button { text: None, .. }) {
                self.composed_buttons.push(id);
            }
            if matches!(element.kind, ElementKind::Image(_)) {
                self.image_nodes.push(id);
            }
            if element.scroll_handle.is_some() {
                self.scroll_handle_nodes.push(id);
            }
            self.style_roots.insert(id);
            self.opacity_count += usize::from(element.opacity < 1.);
            self.nodes.insert(
                id,
                Node {
                    element,
                    layout,
                    children: Vec::new(),
                    mounted,
                    bounds: Bounds::default(),
                    content_bounds: Bounds::default(),
                    visible: false,
                    needs_evaluation: true,
                    ancestors,
                    text_revision: 1,
                    parent,
                    clip_bounds: Bounds::default(),
                    rounded_clip: None,
                    scroll_offset: [0.; 2],
                    scroll_range: [0.; 2],
                    editor,
                    resolved_theme: Theme::default(),
                    color_binding: ThemeColor::Text.into(),
                    font_binding: None,
                    font_size: 16.,
                    text_binding: Default::default(),
                    text_style: Default::default(),
                    paint: ResolvedPaint::new(&Theme::default(), Theme::default().palette().text),
                    state_paints: None,
                    border: [0.; 4],
                    layout_dirty: true,
                    image_size: [0.; 2],
                    image_tint: [1.; 4],
                    button_owner: None,
                    control_color: false,
                    inert: false,
                    pointer_allowed: true,
                    button_name: String::new(),
                },
            );
            if let Some(parent) = parent {
                self.nodes.get_mut(&parent).unwrap().children.push(id);
            }
            self.stats.created_nodes += 1;
            self.order_dirty = true;
            id
        };
        if parent.is_none() {
            self.root = Some(id);
        }
        self.register_focus_node(id);
        self.register_overlay_node(id);
        if self.nodes[&id].mounted.is_some() {
            self.refresh_component(runtime, id)?;
        } else {
            self.reconcile_children(runtime, id, descriptions)?;
        }
        Ok(id)
    }
    fn reconcile_children(
        &mut self,
        runtime: &mut Runtime,
        parent: ElementId,
        descriptions: Vec<Element>,
    ) -> Result<(), UiError> {
        let old = self.nodes[&parent].children.clone();
        let keyed: HashMap<_, _> = old
            .iter()
            .filter_map(|id| {
                self.nodes[id]
                    .element
                    .key
                    .as_ref()
                    .map(|key| (key.clone(), *id))
            })
            .collect();
        // Unkeyed children match by position among unkeyed siblings only, so inserting or
        // removing a keyed sibling does not shift them onto the wrong node.
        let mut unkeyed = old
            .iter()
            .copied()
            .filter(|id| self.nodes[id].element.key.is_none())
            .collect::<Vec<_>>()
            .into_iter();
        let mut next = Vec::with_capacity(descriptions.len());
        let mut retained = HashSet::with_capacity(descriptions.len());
        for description in descriptions {
            let candidate = match &description.key {
                Some(key) => keyed.get(key).copied(),
                None => unkeyed.next(),
            };
            let node = self.reconcile(runtime, candidate, description, Some(parent))?;
            next.push(node);
            retained.insert(node);
        }
        for id in &old {
            if !retained.contains(id) && self.nodes.contains_key(id) {
                self.remove(*id)?;
            }
        }
        let layout = self.nodes[&parent].layout;
        let children: Vec<_> = next
            .iter()
            .filter(|id| !self.is_overlay(**id))
            .map(|id| self.nodes[id].layout)
            .collect();
        if self.taffy.children(layout)? != children {
            self.taffy.set_children(layout, &children)?;
            self.order_dirty = true;
        }
        self.nodes.get_mut(&parent).unwrap().children = next;
        Ok(())
    }
    fn refresh_component(&mut self, runtime: &mut Runtime, id: ElementId) -> Result<(), UiError> {
        let node = &self.nodes[&id];
        let mounted = node.mounted.as_ref().unwrap();
        if mounted.dirty(runtime)? || node.needs_evaluation {
            let ElementKind::Component(component) = &node.element.kind else {
                unreachable!()
            };
            let entity_id = component.entity_id();
            let previous = std::mem::replace(&mut self.active_views, node.ancestors.clone());
            self.active_views.push(entity_id);
            self.nodes.get_mut(&id).unwrap().needs_evaluation = true;
            let result = (|| {
                let description = self.nodes[&id]
                    .mounted
                    .as_ref()
                    .unwrap()
                    .evaluate(runtime)?;
                self.stats.component_evaluations += 1;
                self.reconcile_children(runtime, id, vec![description])
            })();
            self.active_views = previous;
            result?;
            self.nodes.get_mut(&id).unwrap().needs_evaluation = false;
        }
        Ok(())
    }
    fn resolve_styles(&mut self) -> Result<(), UiError> {
        profiling::scope!("rxui::resolve_styles");
        if self.style_roots.is_empty() {
            return Ok(());
        }
        let roots: Vec<_> = self
            .style_roots
            .iter()
            .copied()
            .filter(|id| {
                let Some(node) = self.nodes.get(id) else {
                    return false;
                };
                let mut parent = node.parent;
                while let Some(id) = parent {
                    if self.style_roots.contains(&id) {
                        return false;
                    }
                    parent = self.nodes.get(&id).and_then(|n| n.parent);
                }
                true
            })
            .collect();
        for id in roots {
            let (theme, color, font, text) = self.nodes[&id]
                .parent
                .and_then(|p| self.nodes.get(&p))
                .map_or_else(
                    || {
                        (
                            self.theme.clone(),
                            ThemeColor::Text.into(),
                            None,
                            Default::default(),
                        )
                    },
                    |p| {
                        (
                            p.resolved_theme.clone(),
                            p.color_binding,
                            p.font_binding,
                            p.text_binding.clone(),
                        )
                    },
                );
            self.resolve_subtree(id, &theme, color, font, &text)?;
        }
        self.style_roots.clear();
        Ok(())
    }
    fn resolve_subtree(
        &mut self,
        id: ElementId,
        parent_theme: &Theme,
        parent_color: StyleColor,
        parent_font: Option<f32>,
        parent_text: &crate::typography::Overrides,
    ) -> Result<(), UiError> {
        let inert = self.nodes[&id].element.inert
            || self.nodes[&id]
                .parent
                .is_some_and(|parent| self.nodes[&parent].inert);
        // Cancel preedit before layout rather than while clearing focus afterward.
        if inert {
            self.cancel_composition(id);
        }
        let node = &self.nodes[&id];
        let element = &node.element;
        let theme = element.theme.as_ref().unwrap_or(parent_theme).clone();
        let color = element.paint.color.unwrap_or(parent_color);
        let font = element.font_size.or(parent_font);
        let font_size = font.unwrap_or(theme.sizes().font_size);
        let text_binding = element
            .text_style
            .as_deref()
            .map_or_else(|| parent_text.clone(), |own| own.over(parent_text));
        let text_style = text_binding.resolve(&theme);
        let control = matches!(
            element.kind,
            ElementKind::Button { .. } | ElementKind::TextInput { .. }
        );
        let input = matches!(element.kind, ElementKind::TextInput { .. });
        let metrics = theme.sizes();
        let layout = if node.layout_dirty || (control && metrics != node.resolved_theme.sizes()) {
            let mut layout = element.style.clone();
            if control {
                use taffy::geometry::Rect;
                let all = |x, y| Rect {
                    left: length(x),
                    right: length(x),
                    top: length(y),
                    bottom: length(y),
                };
                if element.layout_overrides & 1 == 0 {
                    layout.padding = if input {
                        all(metrics.input_padding_x, metrics.input_padding_y)
                    } else {
                        all(metrics.button_padding_x, metrics.button_padding_y)
                    };
                }
                if element.layout_overrides & 8 == 0 {
                    layout.border = all(metrics.border_width, metrics.border_width);
                }
                if input {
                    if element.layout_overrides & 2 == 0 {
                        layout.size.width = length(metrics.input_width);
                    }
                    if element.layout_overrides & 4 == 0 {
                        layout.size.height = length(metrics.input_height);
                    }
                }
            }
            Some(layout)
        } else {
            None
        };
        let mut paint = ResolvedPaint::new(&theme, color.resolve(&theme));
        if control {
            let c = theme.palette();
            let (fill, border) = if input {
                (c.input, c.input_border)
            } else {
                (c.control, c.border)
            };
            paint.background = Some(fill);
            paint.border_color = Some(border);
            paint.radii = [metrics.radius; 4];
        }
        if matches!(element.kind, ElementKind::Button { .. }) {
            match element.button_variant {
                crate::ButtonVariant::Primary => {
                    paint.background = Some(theme.palette().accent);
                    paint.color = theme.palette().accent_text;
                    paint.border_color = Some(theme.palette().accent);
                    paint.focus_color = theme.palette().accent_text;
                }
                crate::ButtonVariant::Quiet => {
                    paint.background = None;
                    paint.border_color = None;
                }
                crate::ButtonVariant::Default => {}
            }
        }
        let states = if control || element.states.is_some() {
            let mut states = [paint; 3];
            if control {
                states[0].background = Some(theme.palette().control_hover);
                states[1].background = Some(theme.palette().control_pressed);
                states[2].background = Some(theme.palette().control_disabled);
                states[2].color = theme.palette().text_disabled;
                states[2].border_color = Some(theme.palette().border_disabled);
            }
            if matches!(element.kind, ElementKind::Button { .. }) {
                match element.button_variant {
                    crate::ButtonVariant::Primary => {
                        states[0].background = Some(theme.palette().accent_hover);
                        states[0].border_color = Some(theme.palette().accent_hover);
                        states[1].background = Some(theme.palette().accent_pressed);
                        states[1].border_color = Some(theme.palette().accent_pressed);
                        states[2].focus_color = theme.palette().focus;
                    }
                    crate::ButtonVariant::Quiet => {
                        states[2].background = None;
                        states[2].border_color = None;
                    }
                    crate::ButtonVariant::Default => {}
                }
            }
            for state in &mut states {
                element.paint.apply(&theme, state);
            }
            if let Some(patches) = &element.states {
                patches.hover.apply(&theme, &mut states[0]);
                patches.pressed.apply(&theme, &mut states[1]);
                patches.disabled.apply(&theme, &mut states[2]);
            }
            Some(Box::new(states))
        } else {
            None
        };
        element.paint.apply(&theme, &mut paint);
        let children = node.children.clone();
        let layout_id = node.layout;
        if let Some(layout) = layout
            && self.taffy.style(layout_id)? != &layout
        {
            self.taffy.set_style(layout_id, layout)?;
        }
        let (button_owner, control_color) = self.nodes[&id]
            .parent
            .and_then(|p| self.nodes.get(&p).map(|n| (p, n)))
            .map_or((None, false), |(parent, n)| {
                if matches!(n.element.kind, ElementKind::Button { .. }) {
                    (Some(parent), true)
                } else {
                    (n.button_owner, n.control_color)
                }
            });
        let inherited = self.nodes[&id].parent.map(|parent| &self.nodes[&parent]);
        let inert = self.nodes[&id].element.inert || inherited.is_some_and(|n| n.inert);
        let pointer_allowed = !inert
            && self.nodes[&id].element.pointer_events != crate::PointerEvents::None
            && inherited.is_none_or(|n| n.pointer_allowed);
        let control_color = control_color && self.nodes[&id].element.paint.color.is_none();
        let node = self.nodes.get_mut(&id).unwrap();
        node.inert = inert;
        node.pointer_allowed = pointer_allowed;
        node.button_owner = button_owner;
        node.control_color = control_color;
        if (node.font_size != font_size || node.text_style != text_style)
            && (node.displayed_text().is_some()
                || matches!(node.element.kind, ElementKind::Custom(_)))
        {
            self.taffy.mark_dirty(layout_id)?;
            node.text_revision = node
                .text_revision
                .checked_add(1)
                .expect("RXUI text revision exhausted");
        }
        node.resolved_theme = theme.clone();
        node.color_binding = color;
        node.font_binding = font;
        node.layout_dirty = false;
        node.font_size = font_size;
        node.text_style = text_style;
        node.image_tint = if let ElementKind::Image(props) = &node.element.kind {
            props.tint.resolve(&theme)
        } else {
            [1.; 4]
        };
        node.paint = paint;
        node.state_paints = states;
        self.stats.style_resolutions += 1;
        node.text_binding = text_binding.clone();
        for child in children {
            self.resolve_subtree(child, &theme, color, font, &text_binding)?;
        }
        Ok(())
    }
    fn refresh_images(&mut self, refresh_all: bool) -> Result<(), UiError> {
        if refresh_all {
            self.image_nodes.retain(|id| self.nodes.contains_key(id));
            self.live_image_nodes.clear();
            self.live_image_nodes
                .extend(self.image_nodes.iter().copied().filter(|id| {
                    image_properties(&self.nodes[id].element)
                        .is_some_and(|props| props.source.is_live())
                }));
        }
        let ids = if refresh_all {
            &self.image_nodes
        } else {
            &self.live_image_nodes
        };
        for id in ids {
            let node = self.nodes.get_mut(id).unwrap();
            let ElementKind::Image(props) = &node.element.kind else {
                continue;
            };
            if let Some(size) = props.source.pixel_size() {
                let size = [size[0] as f32, size[1] as f32];
                if size != node.image_size {
                    node.image_size = size;
                    let style = self.taffy.style(node.layout)?;
                    if style.size.width.is_auto() || style.size.height.is_auto() {
                        self.taffy.mark_dirty(node.layout)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn remove(&mut self, id: ElementId) -> Result<(), UiError> {
        let mut capture = self.captured_pointer();
        while let Some(current) = capture {
            if current == id {
                self.cancel_capture(crate::PointerCancelReason::TargetUnavailable);
                break;
            }
            capture = self.nodes.get(&current).and_then(|n| n.parent);
        }
        let node = self.nodes.remove(&id).unwrap();
        if let Some(overlays) = &mut self.overlays {
            overlays.forget(id);
        }
        self.opacity_count -= usize::from(node.element.opacity < 1.);
        for child in node.children {
            self.remove(child)?;
        }
        self.taffy.remove(node.layout)?;
        self.stats.removed_nodes += 1;
        self.order_dirty = true;
        Ok(())
    }
    fn update_bounds(
        &mut self,
        id: ElementId,
        origin: [f32; 2],
        visible: bool,
        clip: Bounds,
    ) -> Result<(), UiError> {
        let rounded_clip = if self.is_overlay(id) {
            None
        } else {
            self.nodes[&id]
                .parent
                .and_then(|p| self.descendant_rounded_clip(p))
        };
        let node = self.nodes.get_mut(&id).unwrap();
        node.rounded_clip = rounded_clip;
        let layout = self.taffy.layout(node.layout)?;
        let visible = visible && node.element.style.display != Display::None;
        let bounds = Bounds {
            x: origin[0] + layout.location.x,
            y: origin[1] + layout.location.y,
            width: layout.size.width,
            height: layout.size.height,
        };
        if [bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(UiError::InvalidGeometry);
        }
        node.border = [
            layout.border.left,
            layout.border.right,
            layout.border.top,
            layout.border.bottom,
        ];
        node.bounds = bounds;
        node.visible = visible;
        node.content_bounds = Bounds {
            x: bounds.x + layout.border.left + layout.padding.left,
            y: bounds.y + layout.border.top + layout.padding.top,
            width: (bounds.width
                - layout.padding.left
                - layout.padding.right
                - layout.border.left
                - layout.border.right)
                .max(0.),
            height: (bounds.height
                - layout.padding.top
                - layout.padding.bottom
                - layout.border.top
                - layout.border.bottom)
                .max(0.),
        };
        let allowed = node
            .element
            .scroll
            .map_or([false; 2], |axes| axes.allowed());
        // Display:none panels retain their last valid scroll state. Hidden Taffy
        // layouts have zero extents; clamp against fresh content when shown again.
        if visible {
            node.scroll_range = [
                if allowed[0] {
                    layout.scroll_width()
                } else {
                    0.
                },
                if allowed[1] {
                    layout.scroll_height()
                } else {
                    0.
                },
            ];
            if node.scroll_range.iter().any(|v| !v.is_finite()) {
                return Err(UiError::InvalidGeometry);
            }
            for axis in 0..2 {
                node.scroll_offset[axis] =
                    node.scroll_offset[axis].clamp(0., node.scroll_range[axis]);
            }
        }
        node.clip_bounds = if node.element.clip {
            clip.intersection(bounds)
        } else {
            clip
        };
        let child_clip = if node.element.clip {
            clip.intersection(node.content_bounds)
        } else {
            clip
        };
        let child_origin = [
            bounds.x - node.scroll_offset[0],
            bounds.y - node.scroll_offset[1],
        ];
        for child in node.children.clone() {
            if self.is_overlay(child) {
                continue;
            }
            self.update_bounds(child, child_origin, visible, child_clip)?;
        }
        Ok(())
    }
    /// The rounded clip a clipping node applies to its descendants: its own content
    /// box and inset radii when it has corner radii, otherwise its inherited clip.
    fn descendant_rounded_clip(&self, id: ElementId) -> Option<RoundedClip> {
        let node = &self.nodes[&id];
        if !node.element.clip || node.paint.radii.iter().all(|r| *r <= 0.) {
            return node.rounded_clip;
        }
        let (b, c) = (node.bounds, node.content_bounds);
        let [left, top] = [c.x - b.x, c.y - b.y];
        let [right, bottom] = [
            b.x + b.width - c.x - c.width,
            b.y + b.height - c.y - c.height,
        ];
        let [tl, tr, br, bl] = node.paint.radii;
        Some(RoundedClip {
            bounds: c,
            radii: [
                (tl - left.max(top)).max(0.),
                (tr - right.max(top)).max(0.),
                (br - right.max(bottom)).max(0.),
                (bl - left.max(bottom)).max(0.),
            ],
        })
    }
    fn collect_order(&mut self, id: ElementId) {
        self.order.push(id);
        for child in self.nodes[&id].children.clone() {
            self.collect_order(child);
        }
    }
    fn rebuild_order(&mut self, root: ElementId) {
        self.order.clear();
        self.collect_order(root);
        if self.nodes.values().any(|n| {
            n.element.z_index != 0
                || n.element
                    .input
                    .as_ref()
                    .is_some_and(|p| p.overlay.is_some())
        }) {
            let mut order = self.paint_order.take().unwrap_or_default();
            order.clear();
            self.collect_paint_order(root, &mut order);
            let mut portals: Vec<_> = self
                .order
                .iter()
                .copied()
                .filter(|id| self.is_overlay(*id))
                .collect();
            portals.sort_by_key(|id| self.in_modal_layer(*id));
            for id in portals {
                self.collect_paint_order(id, &mut order);
            }
            self.paint_order = Some(order);
        } else {
            self.paint_order = None;
        }
        self.order_dirty = false;
    }
    fn collect_paint_order(&self, id: ElementId, order: &mut Vec<ElementId>) {
        order.push(id);
        let mut children = self.nodes[&id].children.clone();
        children.sort_by_key(|child| self.nodes[child].element.z_index);
        for child in children {
            if self.is_overlay(child) {
                continue;
            }
            self.collect_paint_order(child, order);
        }
    }
    #[cfg(feature = "rendering")]
    pub(crate) fn custom_element(&self, id: ElementId) -> Option<&dyn crate::custom::AnyCustom> {
        match &self.nodes.get(&id)?.element.kind {
            ElementKind::Custom(custom) => Some(custom.as_ref()),
            _ => None,
        }
    }
    #[cfg(feature = "rendering")]
    pub(crate) fn painting_ids(&self) -> &[ElementId] {
        self.painting_order()
    }
    fn painting_order(&self) -> &[ElementId] {
        self.paint_order.as_deref().unwrap_or(&self.order)
    }
    fn pointer_allowed(&self, id: ElementId) -> bool {
        self.nodes[&id].pointer_allowed && self.modal_allows(id)
    }
    /// Whether a visible node's clipped box, and a custom element's own hit test,
    /// contain the point.
    fn node_contains(n: &Node, point: [f32; 2]) -> bool {
        n.visible
            && n.bounds.contains(point)
            && n.clip_bounds.contains(point)
            && match &n.element.kind {
                ElementKind::Custom(custom) => custom.hit_test(
                    [point[0] - n.bounds.x, point[1] - n.bounds.y],
                    [n.bounds.width, n.bounds.height],
                ),
                _ => true,
            }
    }
    fn pointer_target(&self, point: [f32; 2]) -> Option<ElementId> {
        self.painting_order().iter().rev().copied().find(|id| {
            let n = &self.nodes[id];
            Self::node_contains(n, point)
                && (matches!(
                    n.element.kind,
                    ElementKind::Button { .. }
                        | ElementKind::TextInput { .. }
                        | ElementKind::Scrollbar(_)
                        | ElementKind::Splitter(_)
                ) || n.element.pointer_events == crate::PointerEvents::Block
                    || n.element.input.as_ref().is_some_and(|p| p.pointer_target()))
                && self.pointer_allowed(*id)
        })
    }
    fn enabled(&self, id: ElementId) -> bool {
        if !self.modal_allows(id) {
            return false;
        }
        self.enabled_unconfined(id)
    }
    fn enabled_unconfined(&self, id: ElementId) -> bool {
        let Some(node) = self.nodes.get(&id) else {
            return false;
        };
        if self.range_unavailable(id) {
            return false;
        }
        let control = matches!(
            node.element.kind,
            ElementKind::Button { .. }
                | ElementKind::TextInput { .. }
                | ElementKind::Scrollbar(_)
                | ElementKind::Splitter(_)
        );
        if !node.visible
            || node.inert
            || matches!(
                node.element.kind,
                ElementKind::Button { disabled: true, .. }
                    | ElementKind::TextInput { disabled: true, .. }
            )
            || !node
                .element
                .input
                .as_ref()
                .and_then(|p| p.focusable)
                .unwrap_or(control)
        {
            return false;
        }
        let mut current = Some(id);
        while let Some(id) = current {
            let node = &self.nodes[&id];
            if node.element.style.display == Display::None {
                return false;
            }
            current = node.parent;
        }
        true
    }
    fn clear_invalid_interaction(&mut self) {
        if self
            .hovered
            .is_some_and(|id| !self.input_available(id) || !self.pointer_allowed(id))
        {
            self.hovered = None;
        }
        if self
            .pressed
            .is_some_and(|id| !self.input_available(id) || !self.pointer_allowed(id))
        {
            self.pressed = None;
        }
        if self.focused.is_some_and(|id| !self.enabled(id)) {
            self.change_focus(None);
        }
    }
    /// Iterates visible retained elements in paint order after successful preparation.
    pub fn elements(&self) -> impl Iterator<Item = ElementInfo<'_>> {
        self.painting_order()
            .iter()
            .filter_map(|id| self.element(*id))
    }
    /// Whether a retained identity still exists, including hidden/unprepared nodes.
    /// Hosts can use this to dispose per-node measurement and GPU caches.
    pub fn contains_element(&self, id: ElementId) -> bool {
        self.nodes.contains_key(&id)
    }
    /// Whether geometry is ready for inspection, painting and input routing.
    pub fn is_prepared(&self) -> bool {
        self.geometry_ready && self.style_roots.is_empty()
    }
    /// Looks up one visible element from the current prepared snapshot.
    pub fn element(&self, id: ElementId) -> Option<ElementInfo<'_>> {
        if !self.is_prepared() {
            return None;
        }
        let node = self.nodes.get(&id)?;
        if !node.visible {
            return None;
        }
        let disabled = self.range_unavailable(id)
            || matches!(
                node.element.kind,
                ElementKind::Button { disabled: true, .. }
                    | ElementKind::TextInput { disabled: true, .. }
            );
        let mut paint = node.state_paints.as_ref().map_or(node.paint, |states| {
            if disabled {
                states[2]
            } else if self.pressed == Some(id) || self.captured_pointer() == Some(id) {
                states[1]
            } else if self.hovered == Some(id) {
                states[0]
            } else {
                node.paint
            }
        });
        if let Some(owner) = node.button_owner
            && node.control_color
        {
            let owner = &self.nodes[&owner];
            let owner_id = node.button_owner.unwrap();
            let is_disabled = matches!(
                owner.element.kind,
                ElementKind::Button { disabled: true, .. }
            );
            if let Some(states) = &owner.state_paints {
                paint.color = if is_disabled {
                    states[2].color
                } else if self.pressed == Some(owner_id) {
                    states[1].color
                } else if self.hovered == Some(owner_id) {
                    states[0].color
                } else {
                    owner.paint.color
                };
            }
        }
        Some(ElementInfo {
            viewport_overlay: self.is_overlay(id),
            id,
            parent: node.parent,
            opacity: node.element.opacity,
            key: node.element.key.as_ref(),
            kind: kind(&node.element),
            bounds: node.bounds,
            content_bounds: node.content_bounds,
            clip_bounds: node.clip_bounds,
            rounded_clip: node.rounded_clip,
            scroll_offset: node.scroll_offset,
            scroll_range: node.scroll_range,
            z_index: node.element.z_index,
            pointer_events: node.element.pointer_events,
            inert: node.inert,
            text: node.displayed_text(),
            image: if let ElementKind::Image(props) = &node.element.kind {
                let (destination, uv) =
                    crate::image::placement(node.content_bounds, node.image_size, props);
                Some(crate::ImageInfo {
                    source: &props.source,
                    destination,
                    uv,
                    tint: node.image_tint,
                    filter: props.filter,
                    alpha: props.alpha,
                })
            } else {
                None
            },
            editing: self.text_input_info(id),
            font_size: node.font_size,
            text_style: &node.text_style,
            text_revision: node.text_revision,
            color: paint.color,
            background: paint.background,
            paint,
            border: node.border,
            disabled,
            hovered: self.hovered == Some(id),
            pressed: self.pressed == Some(id) || self.captured_pointer() == Some(id),
            focused: self.focused == Some(id),
            focusable: self.enabled(id),
            range: self.range_info(id),
        })
    }
    /// Topmost enabled focusable target within the logical viewport and ancestor clips,
    /// using the current retained scroll offsets.
    pub fn hit_test(&self, point: [f32; 2]) -> Option<ElementId> {
        if !self.is_prepared() {
            return None;
        }
        let [width, height] = self.viewport?;
        if !(Bounds {
            x: 0.,
            y: 0.,
            width,
            height,
        })
        .contains(point)
        {
            return None;
        }
        self.pointer_target(point).filter(|id| self.enabled(*id))
    }
    /// Scrolls the innermost hit container, chaining unconsumed motion to ancestors.
    /// Delta is logical content motion: positive values move toward later content.
    /// Retained geometry changes without description evaluation or Taffy layout.
    pub fn scroll(&mut self, point: [f32; 2], mut delta: [f32; 2]) -> Result<bool, UiError> {
        if point.iter().chain(delta.iter()).any(|v| !v.is_finite()) {
            return Err(UiError::InvalidGeometry);
        }
        if !self.geometry_ready {
            return Ok(false);
        }
        let mut current =
            self.painting_order().iter().rev().copied().find(|id| {
                Self::node_contains(&self.nodes[id], point) && self.pointer_allowed(*id)
            });
        if let Some(id) = current
            && let ElementKind::Scrollbar(p) = &self.nodes[&id].element.kind
        {
            current = p.handle.element_in(self.tree);
        }
        let mut changed = false;
        while let Some(id) = current {
            let node = self.nodes.get_mut(&id).unwrap();
            if node.element.scroll.is_some() {
                for (axis, motion) in delta.iter_mut().enumerate() {
                    let old = node.scroll_offset[axis];
                    let next = (old + *motion).clamp(0., node.scroll_range[axis]);
                    node.scroll_offset[axis] = next;
                    *motion -= next - old;
                    changed |= next != old;
                }
            }
            current = node.parent;
            if self.is_overlay(id) {
                break;
            }
        }
        if changed {
            self.refresh_geometry()?;
            self.hovered = self
                .pointer_target(point)
                .filter(|id| self.input_available(*id));
        }
        Ok(changed)
    }
    pub(crate) fn scroll_revision(&self) -> (u64, u64, u64) {
        (
            self.geometry_revision,
            self.stats.layout_passes,
            self.stats.component_evaluations,
        )
    }
    pub(crate) fn refresh_geometry(&mut self) -> Result<(), UiError> {
        self.geometry_revision = self
            .geometry_revision
            .checked_add(1)
            .expect("RXUI geometry revision exhausted");
        let [width, height] = self.viewport.expect("prepared viewport");
        self.update_bounds(
            self.root.expect("prepared root"),
            [0., 0.],
            true,
            Bounds {
                x: 0.,
                y: 0.,
                width,
                height,
            },
        )?;
        self.refresh_overlay_bounds([width, height])?;
        if let Some(inner) = self.runtime.upgrade() {
            self.publish_scroll(&Runtime { inner })?;
        }
        Ok(())
    }
    fn reveal(&mut self, id: ElementId) -> bool {
        let mut parent = self.nodes[&id].parent;
        let mut changed = false;
        while let Some(ancestor) = parent {
            if self.is_overlay(ancestor) {
                break;
            }
            let target = self.nodes[&id].bounds;
            let node = self.nodes.get_mut(&ancestor).unwrap();
            if let Some(axes) = node.element.scroll {
                let allowed = axes.allowed();
                let content = node.content_bounds;
                let starts = [target.x, target.y];
                let ends = [target.x + target.width, target.y + target.height];
                let near = [content.x, content.y];
                let far = [content.x + content.width, content.y + content.height];
                let mut moved = false;
                for axis in 0..2 {
                    if allowed[axis] {
                        let delta = if starts[axis] < near[axis]
                            || ends[axis] - starts[axis] > far[axis] - near[axis]
                        {
                            starts[axis] - near[axis]
                        } else {
                            (ends[axis] - far[axis]).max(0.)
                        };
                        let old = node.scroll_offset[axis];
                        node.scroll_offset[axis] = (old + delta).clamp(0., node.scroll_range[axis]);
                        moved |= old != node.scroll_offset[axis];
                    }
                }
                if moved {
                    changed = true;
                    self.refresh_geometry()
                        .expect("prepared layout remains valid");
                }
            }
            parent = self.nodes[&ancestor].parent;
        }
        changed
    }
    /// Routes mouse listeners and control defaults through this placement. Returns
    /// whether visual/application state changed. Button activation requires press
    /// and release on the same surviving identity; explicit capture routes gestures
    /// independently of hit_target. Hosts prepare geometry before sending input.
    pub fn pointer(&mut self, runtime: &mut Runtime, event: PointerEvent) -> Result<bool, UiError> {
        self.pointer_general(runtime, event)
    }
    /// Moves focus in tree order, wrapping at the end. Hidden/disabled controls are skipped.
    pub fn focus_next(&mut self, reverse: bool) -> bool {
        self.scoped_focus_next(reverse)
    }
    /// Activates the focused button through the same handler as pointer input.
    pub fn activate_focused(&mut self, runtime: &mut Runtime) -> Result<bool, UiError> {
        runtime.is_dirty(&self.owner)?;
        match self.focused {
            Some(id) if self.geometry_ready => self.activate(runtime, id),
            _ => Ok(false),
        }
    }
    fn change_focus(&mut self, next: Option<ElementId>) {
        if self.focused != next {
            if let Some(old) = self.focused {
                if let Some(editor) = self.nodes.get_mut(&old).and_then(|n| n.editor.as_mut()) {
                    editor.break_group();
                    editor.drag_selection = None;
                }
                self.cancel_composition(old);
            }
            self.focused = next;
            self.remember_focus(next);
            self.caret_visible = true;
        }
    }
    fn rebuild_editor(&mut self, id: ElementId) {
        let Some(node) = self.nodes.get_mut(&id) else {
            return;
        };
        let Some(editor) = &mut node.editor else {
            return;
        };
        let ElementKind::TextInput { value, .. } = &node.element.kind else {
            return;
        };
        let old = editor.display.clone();
        editor.rebuild(value);
        if old != editor.display {
            node.text_revision = node
                .text_revision
                .checked_add(1)
                .expect("RXUI text revision exhausted");
            self.taffy
                .mark_dirty(node.layout)
                .expect("retained layout node");
            self.geometry_ready = false;
        }
    }
    fn cancel_composition(&mut self, id: ElementId) -> bool {
        let changed = self
            .nodes
            .get_mut(&id)
            .and_then(|n| n.editor.as_mut())
            .is_some_and(|e| e.cancel_composition());
        if changed {
            self.rebuild_editor(id);
        }
        changed
    }
    fn sync_edit_views(&mut self, runtime: &mut Runtime) -> Result<(), UiError> {
        if let Err(error) = self.refresh_descriptions(runtime, self.needs_evaluation) {
            self.geometry_ready = false;
            self.needs_evaluation = true;
            return Err(error);
        }
        self.resolve_styles()?;
        if let Some(root) = self.root {
            if self.order_dirty {
                self.rebuild_order(root);
            }
            if self.taffy.dirty(self.nodes[&root].layout)? {
                self.geometry_ready = false;
            }
        }
        self.needs_evaluation = false;
        self.clear_invalid_interaction();
        Ok(())
    }
    /// Current selection, composition and history availability for a retained input.
    /// This needs no prepared geometry; controlled values reflect the most recent
    /// reconciliation. Returns None for foreign/removed identities or other kinds.
    pub fn text_input_info(&self, id: ElementId) -> Option<TextInputInfo> {
        let node = self.nodes.get(&id)?;
        let ElementKind::TextInput {
            read_only,
            disabled,
            change,
            ..
        } = &node.element.kind
        else {
            return None;
        };
        let mut info = node.editor.as_ref()?.info(
            *read_only || change.is_none(),
            self.focused == Some(id) && self.active && self.caret_visible,
        );
        info.can_undo &= !*disabled;
        info.can_redo &= !*disabled;
        Some(info)
    }
    /// Current retained focus identity, independent of native activation.
    pub fn focused_element(&self) -> Option<ElementId> {
        self.focused
    }
    /// Changes when active composition is cancelled, including an external value
    /// replacement. Native hosts use it to reset the platform IME session even when
    /// the focused element remains the same. A normal IME commit does not reset it.
    pub fn ime_reset_revision(&self) -> u64 {
        self.focused
            .and_then(|id| self.nodes.get(&id))
            .and_then(|n| n.editor.as_ref())
            .map_or(0, |e| e.ime_reset_revision)
    }
    /// Whether a live, focused single-line input is accepting committed text/IME.
    pub fn accepts_text_input(&self) -> bool {
        self.active
            && self.focused.is_some_and(|id| {
                self.enabled(id)
                    && matches!(
                        self.nodes[&id].element.kind,
                        ElementKind::TextInput {
                            disabled: false,
                            read_only: false,
                            change: Some(_),
                            ..
                        }
                    )
            })
    }
    /// Whether focus is on a selectable text input, including read-only inputs.
    pub fn has_text_focus(&self) -> bool {
        self.active
            && self
                .focused
                .is_some_and(|id| self.enabled(id) && self.nodes[&id].editor.is_some())
    }
    /// Changes native activation. Losing activation cancels composition/capture while
    /// retaining the focused identity and committed selection for later reactivation.
    /// Cancellation callbacks run during the next preparation; hosts can forward
    /// PointerEvent::Cancelled first to dispatch them immediately.
    pub fn set_active(&mut self, active: bool) -> bool {
        let changed = self.active != active;
        self.active = active;
        self.caret_visible = true;
        if !active {
            self.cancel_capture(crate::PointerCancelReason::Host);
            self.input.buttons = crate::PointerButtons::default();
            self.hovered = None;
            self.pressed = None;
            if let Some(id) = self.focused {
                if let Some(editor) = self.nodes.get_mut(&id).and_then(|n| n.editor.as_mut()) {
                    editor.break_group();
                    editor.drag_selection = None;
                }
                self.cancel_composition(id);
            }
        }
        changed
    }
    /// Paint-only caret visibility for a host's on-demand blink deadline.
    /// Returns whether a focused input's appearance changed; it does no text work.
    pub fn set_caret_visible(&mut self, visible: bool) -> bool {
        let changed = self.caret_visible != visible;
        self.caret_visible = visible;
        changed && self.has_text_focus()
    }
    /// Current retained selection text for clipboard integration; prepare/synchronize
    /// after external model updates before relying on this snapshot.
    pub fn selected_text(&self) -> Option<&str> {
        let node = self.nodes.get(&self.focused?)?;
        let ElementKind::TextInput { value, .. } = &node.element.kind else {
            return None;
        };
        let range = node.editor.as_ref()?.selection.range();
        (!range.is_empty()).then(|| &value[range])
    }
    fn update_text_scroll(
        &mut self,
        id: ElementId,
        measure: &mut impl TextMeasure,
    ) -> Result<(), UiError> {
        if !self.nodes.get(&id).is_some_and(|node| node.visible) {
            return Ok(());
        }

        let node = &self.nodes[&id];
        let editor = node.editor.as_ref().unwrap();
        let position = editor.ime_position();
        let Some(caret) = measure.text_caret(id, node.text_request(), position)? else {
            return Ok(());
        };
        let node = self.nodes.get_mut(&id).unwrap();
        let width = node.content_bounds.width;
        let editor = node.editor.as_mut().unwrap();
        if caret.x < editor.scroll_x {
            editor.scroll_x = caret.x.max(0.);
        } else if caret.x + 1. > editor.scroll_x + width {
            editor.scroll_x = (caret.x + 1. - width).max(0.);
        }
        // A shorter externally supplied value should not retain an obsolete offset.
        editor.scroll_x = editor.scroll_x.min(caret.x.max(0.));
        Ok(())
    }
    /// Logical native IME candidate rectangle for the active editor, using the same
    /// shaping snapshot as painting. A custom host applies DPI/platform conversion.
    pub fn ime_cursor_area(
        &mut self,
        measure: &mut impl TextMeasure,
    ) -> Result<Option<Bounds>, UiError> {
        if !self.accepts_text_input() {
            return Ok(None);
        }
        let id = self.focused.unwrap();
        self.update_text_scroll(id, measure)?;
        let node = &self.nodes[&id];
        let editor = node.editor.as_ref().unwrap();
        let position = editor.ime_position();
        let Some(caret) = measure.text_caret(id, node.text_request(), position)? else {
            return Ok(None);
        };
        Ok(Some(Bounds {
            x: node.content_bounds.x + caret.x - editor.scroll_x,
            y: node.content_bounds.y + caret.y,
            width: 1.,
            height: caret.height,
        }))
    }
    /// Routes controlled single-line editing without requiring a frame. Dirty
    /// descriptions reconcile before input and after each committed proposal, so
    /// application acceptance/normalization/rejection is authoritative for the next
    /// event. Selection/preedit do not dispatch model changes or rerun Taffy here.
    pub fn text_input(
        &mut self,
        runtime: &mut Runtime,
        event: TextInputEvent,
        measure: &mut impl TextMeasure,
    ) -> Result<bool, UiError> {
        self.sync_edit_views(runtime)?;
        let Some(id) = self
            .focused
            .filter(|id| self.enabled(*id) && self.nodes[id].editor.is_some())
        else {
            return Ok(false);
        };
        if !self.active {
            return Ok(false);
        }
        let caret_was_hidden = !self.caret_visible;
        self.caret_visible = true;
        let node = &self.nodes[&id];
        let ElementKind::TextInput {
            value,
            change,
            submit,
            read_only,
            ..
        } = &node.element.kind
        else {
            unreachable!()
        };
        let value = value.as_str();
        let change = change.clone();
        let submit = submit.clone();
        let readonly = *read_only || change.is_none();
        let selection = node.editor.as_ref().unwrap().selection;
        let composing = node.editor.as_ref().unwrap().composition.is_some();
        let edit_kind = match &event {
            TextInputEvent::Insert(text)
                if selection.is_collapsed()
                    && unicode_segmentation::UnicodeSegmentation::graphemes(
                        text.as_str(),
                        true,
                    )
                    .count()
                        == 1
                    && !text.chars().any(char::is_whitespace) =>
            {
                EditKind::Typing
            }
            TextInputEvent::Backspace if selection.is_collapsed() => EditKind::Backspace,
            TextInputEvent::Delete if selection.is_collapsed() => EditKind::Delete,
            _ => EditKind::Separate,
        };
        match event {
            TextInputEvent::Undo | TextInputEvent::Redo if !readonly && !composing => {
                let redo = event == TextInputEvent::Redo;
                let Some(snapshot) = node.editor.as_ref().unwrap().replay(redo) else {
                    return Ok(false);
                };
                let proposed = snapshot.value.clone();
                let restored = snapshot.selection;
                let value = value.to_owned();
                self.propose_edit(
                    runtime,
                    id,
                    &value,
                    Proposal {
                        range: 0..value.len(),
                        insert: &proposed,
                        action: if redo {
                            EditAction::Redo
                        } else {
                            EditAction::Undo
                        },
                        selection: Some(restored),
                    },
                    change.unwrap(),
                )?;
            }
            TextInputEvent::CancelComposition => {
                return Ok(self.cancel_composition(id) || caret_was_hidden);
            }
            TextInputEvent::Preedit { text, cursor } if !readonly => {
                if text.chars().any(char::is_control)
                    || cursor.is_some_and(|(a, b)| {
                        a > b
                            || b > text.len()
                            || !text.is_char_boundary(a)
                            || !text.is_char_boundary(b)
                    })
                {
                    return Err(UiError::InvalidTextValue);
                }
                let editor = self.nodes.get_mut(&id).unwrap().editor.as_mut().unwrap();
                editor.break_group();
                let replacement = editor
                    .composition
                    .as_ref()
                    .map_or(selection, |c| c.replacement);
                editor.composition = Some(Composition {
                    text,
                    cursor,
                    replacement,
                });
                self.rebuild_editor(id);
                self.update_text_scroll(id, measure)?;
                return Ok(true);
            }
            TextInputEvent::Move { movement, extend } if !composing => {
                let right = matches!(
                    movement,
                    TextMovement::Right | TextMovement::WordRight | TextMovement::End
                );
                let focus = selection.focus;
                let position = if !extend
                    && !selection.is_collapsed()
                    && matches!(movement, TextMovement::Left | TextMovement::Right)
                {
                    let a =
                        measure.text_caret(id, self.nodes[&id].text_request(), selection.anchor)?;
                    let b =
                        measure.text_caret(id, self.nodes[&id].text_request(), selection.focus)?;
                    if let (Some(a), Some(b)) = (a, b) {
                        if (a.x >= b.x) == right {
                            selection.anchor
                        } else {
                            selection.focus
                        }
                    } else {
                        TextPosition::new(if right {
                            selection.range().end
                        } else {
                            selection.range().start
                        })
                    }
                } else {
                    match movement {
                        TextMovement::Start => TextPosition::new(0),
                        TextMovement::End => TextPosition::new(value.len()),
                        TextMovement::WordLeft => {
                            TextPosition::new(word_left(value, focus.byte_offset))
                        }
                        TextMovement::WordRight => {
                            TextPosition::new(word_right(value, focus.byte_offset))
                        }
                        TextMovement::Left | TextMovement::Right => measure
                            .text_neighbor(id, self.nodes[&id].text_request(), focus, right)?
                            .unwrap_or_else(|| {
                                if measure.supports_text_geometry() {
                                    return focus;
                                }
                                TextPosition::new(if right {
                                    next(value, focus.byte_offset)
                                } else {
                                    previous(value, focus.byte_offset)
                                })
                            }),
                    }
                };
                let valid = if measure.supports_text_geometry() {
                    measure
                        .text_caret(id, self.nodes[&id].text_request(), position)?
                        .is_some()
                } else {
                    position.byte_offset <= value.len()
                        && boundary(value, position.byte_offset) == position.byte_offset
                };
                if !valid {
                    return Err(UiError::InvalidGeometry);
                }
                let editor = self.nodes.get_mut(&id).unwrap().editor.as_mut().unwrap();
                editor.break_group();
                editor.drag_selection = None;
                editor.selection = TextSelection {
                    anchor: if extend { selection.anchor } else { position },
                    focus: position,
                };
                let changed = selection != editor.selection;
                self.update_text_scroll(id, measure)?;
                return Ok(changed || caret_was_hidden);
            }
            TextInputEvent::SelectAll if !composing => {
                let length = value.len();
                let editor = self.nodes.get_mut(&id).unwrap().editor.as_mut().unwrap();
                editor.break_group();
                editor.drag_selection = None;
                editor.selection = TextSelection {
                    anchor: TextPosition::new(0),
                    focus: TextPosition::new(length),
                };
                let changed = selection != editor.selection;
                self.update_text_scroll(id, measure)?;
                return Ok(changed || caret_was_hidden);
            }
            TextInputEvent::Submit if !composing => {
                return match submit {
                    Some(listener) => Ok(runtime.update(|cx| {
                        listener.dispatch(
                            &TextSubmitEvent {
                                value: value.to_owned(),
                            },
                            cx,
                        )
                    })? == Dispatch::Handled),
                    None => Ok(false),
                };
            }
            TextInputEvent::Insert(_)
            | TextInputEvent::Paste(_)
            | TextInputEvent::Backspace
            | TextInputEvent::Delete
            | TextInputEvent::BackspaceWord
            | TextInputEvent::DeleteWord
            | TextInputEvent::BackspaceToStart
            | TextInputEvent::DeleteToEnd
                if composing =>
            {
                return Ok(false);
            }
            TextInputEvent::Insert(_)
            | TextInputEvent::Commit(_)
            | TextInputEvent::Backspace
            | TextInputEvent::Delete
            | TextInputEvent::Paste(_)
            | TextInputEvent::BackspaceWord
            | TextInputEvent::DeleteWord
            | TextInputEvent::BackspaceToStart
            | TextInputEvent::DeleteToEnd
                if readonly =>
            {
                return Ok(false);
            }
            TextInputEvent::Insert(insert)
            | TextInputEvent::Commit(insert)
            | TextInputEvent::Paste(insert) => {
                let range = self.nodes[&id]
                    .editor
                    .as_ref()
                    .unwrap()
                    .composition
                    .as_ref()
                    .map_or(selection.range(), |c| c.replacement.range());
                let insert = single_line(&insert);
                let value = value.to_owned();
                self.propose_edit(
                    runtime,
                    id,
                    &value,
                    Proposal::new(range, &insert, edit_kind),
                    change.unwrap(),
                )?;
            }
            TextInputEvent::Backspace
            | TextInputEvent::Delete
            | TextInputEvent::BackspaceWord
            | TextInputEvent::DeleteWord
            | TextInputEvent::BackspaceToStart
            | TextInputEvent::DeleteToEnd => {
                let mut range = selection.range();
                if range.is_empty() {
                    match event {
                        TextInputEvent::Backspace => range.start = previous(value, range.start),
                        TextInputEvent::BackspaceWord => {
                            range.start = word_left(value, range.start)
                        }
                        TextInputEvent::BackspaceToStart => range.start = 0,
                        TextInputEvent::DeleteWord => range.end = word_right(value, range.end),
                        TextInputEvent::DeleteToEnd => range.end = value.len(),
                        _ => range.end = next(value, range.end),
                    }
                }
                let value = value.to_owned();
                self.propose_edit(
                    runtime,
                    id,
                    &value,
                    Proposal::new(range, "", edit_kind),
                    change.unwrap(),
                )?;
            }
            _ => return Ok(false),
        }
        if self.nodes.get(&id).is_some_and(|n| n.editor.is_some()) {
            self.update_text_scroll(id, measure)?;
        }
        Ok(true)
    }
    fn propose_edit(
        &mut self,
        runtime: &mut Runtime,
        id: ElementId,
        value: &str,
        proposal: Proposal<'_>,
        change: crate::Listener<TextChangeEvent>,
    ) -> Result<(), UiError> {
        let Proposal {
            range,
            insert,
            action,
            selection: restored,
        } = proposal;
        let mut proposed = String::with_capacity(value.len() - range.len() + insert.len());
        proposed.push_str(&value[..range.start]);
        proposed.push_str(insert);
        proposed.push_str(&value[range.end..]);
        let selection = restored.unwrap_or_else(|| {
            TextSelection::caret(after_boundary(&proposed, range.start + insert.len()))
        });
        let editor = self.nodes.get_mut(&id).unwrap().editor.as_mut().unwrap();
        let previous = editor.selection;
        editor.composition = None;
        editor.selection = selection;
        editor.pending = Some(Pending {
            proposed: proposed.clone(),
            previous,
            action,
        });
        // Rebuild composition display before reconciliation even if application rejects.
        self.rebuild_editor(id);
        runtime.update(|cx| {
            change.dispatch(
                &TextChangeEvent {
                    value: proposed,
                    selection,
                },
                cx,
            )
        })?;
        self.sync_edit_views(runtime)?;
        if let Some(node) = self.nodes.get_mut(&id)
            && let (Some(editor), ElementKind::TextInput { value: actual, .. }) =
                (&mut node.editor, &node.element.kind)
            && editor.pending.is_some()
        {
            editor.reconcile(value, actual);
        }
        Ok(())
    }
    /// Pointer routing with shaped caret placement and drag selection. The plain
    /// pointer method still supports focus/activation; native hosts use this method.
    pub fn pointer_with_text(
        &mut self,
        runtime: &mut Runtime,
        event: PointerEvent,
        measure: &mut impl TextMeasure,
        extend: bool,
    ) -> Result<bool, UiError> {
        self.pointer_with_text_clicks(runtime, event, measure, extend, 1)
    }
    /// Pointer editing with a host-supplied click count: two selects a Unicode
    /// word-boundary segment (including punctuation/whitespace), three selects the
    /// single line. Word drags extend by whole segments. Shift retains ordinary
    /// caret extension. Hosts determine timing/distance/target continuity.
    pub fn pointer_with_text_clicks(
        &mut self,
        runtime: &mut Runtime,
        event: PointerEvent,
        measure: &mut impl TextMeasure,
        extend: bool,
        click_count: u8,
    ) -> Result<bool, UiError> {
        let previous_pressed = self.pressed;
        let mut changed = self.pointer(runtime, event)?;
        let (kind, point, button, _) = event.parts();
        if (kind == 3 || (kind == 2 && button == Some(crate::PointerButton::Primary)))
            && let Some(editor) = previous_pressed
                .and_then(|id| self.nodes.get_mut(&id))
                .and_then(|n| n.editor.as_mut())
        {
            editor.drag_selection = None;
        }
        if self.input.prevented {
            return Ok(changed);
        }
        let id = if kind == 0 && button == Some(crate::PointerButton::Primary) {
            self.focused
        } else if kind == 1 {
            self.pressed
        } else {
            None
        };
        let Some(point) = point else {
            return Ok(changed);
        };
        let Some(id) = id.filter(|id| self.nodes[id].editor.is_some()) else {
            return Ok(changed);
        };
        changed |= self.cancel_composition(id);
        let node = &self.nodes[&id];
        let editor = node.editor.as_ref().unwrap();
        let local = [
            point[0] - node.content_bounds.x + editor.scroll_x,
            point[1] - node.content_bounds.y,
        ];
        let position = measure
            .text_hit_test(id, node.text_request(), local)?
            .unwrap_or(TextPosition::new(editor.display.len()));
        let position = if measure.supports_text_geometry() {
            if measure
                .text_caret(id, node.text_request(), position)?
                .is_none()
            {
                return Err(UiError::InvalidGeometry);
            }
            position
        } else {
            TextPosition {
                byte_offset: boundary(&editor.display, position.byte_offset),
                ..position
            }
        };
        let segment = if (kind == 0 && click_count >= 3 && !extend)
            || (kind == 1 && editor.drag_line && editor.drag_selection.is_some())
        {
            Some(TextSelection {
                anchor: TextPosition::new(0),
                focus: TextPosition::new(editor.display.len()),
            })
        } else if (kind == 0 && click_count == 2 && !extend)
            || (kind == 1 && editor.drag_selection.is_some())
        {
            Some(word_selection(&editor.display, position))
        } else {
            None
        };
        let editor = self.nodes.get_mut(&id).unwrap().editor.as_mut().unwrap();
        let old = editor.selection;
        editor.break_group();
        if kind == 0 {
            editor.drag_selection = segment;
            editor.drag_line = click_count >= 3 && !extend;
            if let Some(segment) = segment {
                editor.selection = segment;
            } else {
                if !extend {
                    editor.selection.anchor = position;
                }
                editor.selection.focus = position;
            }
        } else if let (Some(origin), Some(segment)) = (editor.drag_selection, segment) {
            editor.selection = if segment.range().start < origin.range().start {
                TextSelection {
                    anchor: origin.focus,
                    focus: segment.anchor,
                }
            } else {
                TextSelection {
                    anchor: origin.anchor,
                    focus: segment.focus,
                }
            };
        } else {
            editor.selection.focus = position;
        }
        changed |= old != editor.selection;
        self.caret_visible = true;
        self.update_text_scroll(id, measure)?;
        Ok(changed)
    }
    fn semantic_visible(&self, id: ElementId) -> bool {
        if !self.overlay_semantic_allows(id) {
            return false;
        }
        let Some(node) = self.nodes.get(&id) else {
            return false;
        };
        if !node.visible || node.inert {
            return false;
        }
        let mut current = Some(id);
        while let Some(id) = current {
            let node = &self.nodes[&id];
            if node.element.style.display == Display::None
                || node.element.semantics.as_ref().is_some_and(|p| p.hidden)
            {
                return false;
            }
            current = node.parent;
        }
        true
    }
    /// Iterates borrowed semantics after successful preparation. Offscreen controls
    /// remain available for assistive navigation/reveal; hidden subtrees are excluded.
    pub fn semantics(&self) -> impl Iterator<Item = crate::SemanticNode<'_>> {
        self.order.iter().filter_map(|id| self.semantic_node(*id))
    }
    /// One prepared semantic element. Returns None for unavailable geometry,
    /// hidden/removed nodes and identities belonging to another placement.
    pub fn semantic_node(&self, id: ElementId) -> Option<crate::SemanticNode<'_>> {
        if !self.is_prepared() || !self.semantic_visible(id) {
            return None;
        }
        let node = &self.nodes[&id];
        let properties = node.element.semantics.as_deref();
        let inferred = match node.element.kind {
            ElementKind::Row
            | ElementKind::Column
            | ElementKind::Stack
            | ElementKind::Component(_)
            | ElementKind::Custom(_) => crate::SemanticRole::Container,
            ElementKind::Label(_) => crate::SemanticRole::Label,
            ElementKind::Image(_) => crate::SemanticRole::Image,
            ElementKind::Scrollbar(_) => crate::SemanticRole::Scrollbar,
            ElementKind::Splitter(_) => crate::SemanticRole::Splitter,
            ElementKind::Button { .. } => crate::SemanticRole::Button,
            ElementKind::TextInput { .. } => crate::SemanticRole::TextInput,
        };
        let role = properties.and_then(|p| p.role).unwrap_or(inferred);
        let label =
            properties
                .and_then(|p| p.label.as_deref())
                .or_else(|| match &node.element.kind {
                    ElementKind::Button {
                        text: Some(text), ..
                    } => Some(text.as_str()),
                    ElementKind::Button { text: None, .. } if !node.button_name.is_empty() => {
                        Some(node.button_name.as_str())
                    }
                    ElementKind::Label(text) if role == crate::SemanticRole::Heading => {
                        Some(text.as_str())
                    }
                    _ => None,
                });
        let disabled = self.range_unavailable(id)
            || matches!(
                node.element.kind,
                ElementKind::Button { disabled: true, .. }
                    | ElementKind::TextInput { disabled: true, .. }
            );
        let read_only = matches!(
            node.element.kind,
            ElementKind::TextInput {
                read_only: true,
                ..
            } | ElementKind::TextInput { change: None, .. }
        );
        let focusable = self.enabled(id);
        let activatable = !disabled
            && matches!(
                node.element.kind,
                ElementKind::Button {
                    listener: Some(_),
                    ..
                }
            );
        let editable = !disabled && !read_only && node.editor.is_some();
        let selection = node
            .editor
            .as_ref()
            .filter(|e| e.composition.is_none())
            .map(|e| e.selection);
        Some(crate::SemanticNode {
            set_size: properties.and_then(|p| p.set_size),
            position_in_set: properties.and_then(|p| p.position_in_set),
            viewport_overlay: self.is_overlay(id),
            modal: node
                .parent
                .and_then(|p| self.nodes[&p].element.input.as_ref())
                .and_then(|p| p.overlay.as_ref())
                .is_some_and(|p| p.modal),
            id,
            parent: node.parent,
            children: &node.children,
            role,
            label,
            description: properties.and_then(|p| p.description.as_deref()),
            bounds: node.bounds,
            content_bounds: node.content_bounds,
            clip_bounds: node.clip_bounds,
            clips_children: node.element.clip,
            value: match node.element.kind {
                ElementKind::Label(_) | ElementKind::TextInput { .. } => node.displayed_text(),
                _ => None,
            },
            text_revision: node.text_revision,
            selection,
            disabled,
            read_only,
            focusable,
            activatable,
            editable,
            scroll_offset: node.scroll_offset,
            scroll_range: node.scroll_range,
            text_scroll_x: node.editor.as_ref().map_or(0., |e| e.scroll_x),
            selected: properties.and_then(|p| p.selected),
            labelled_by: self.tab_relations(id).0,
            controls: self.tab_relations(id).1,
            orientation: match self.tab_properties(id) {
                Some(crate::tabs::Properties::List { axis, .. }) => Some(*axis),
                _ => None,
            },
            range: self.range_info(id),
        })
    }
    /// Logical focus in the semantic tree, retained across native activation changes.
    /// Platform adapters separately track whether the native window has focus.
    pub fn semantic_focus(&self) -> Option<ElementId> {
        self.focused.filter(|id| self.semantic_visible(*id))
    }
    #[cfg(feature = "accessibility")]
    pub(crate) fn semantic_origin(&self, id: ElementId) -> Result<[f64; 2], UiError> {
        let node = &self.nodes[&id];
        if self.is_overlay(id) || node.parent.is_some_and(|p| self.is_overlay(p)) {
            return Ok([f64::from(node.bounds.x), f64::from(node.bounds.y)]);
        }
        let node = &self.nodes[&id];
        let layout = self.taffy.layout(node.layout)?;
        let scroll = node
            .parent
            .map_or([0.; 2], |parent| self.nodes[&parent].scroll_offset);
        // Derive parent-relative coordinates from stable layout, not by subtracting
        // accumulated window bounds. Fractional scrolling must not introduce tiny
        // position changes in every descendant through floating-point cancellation.
        Ok([
            f64::from(layout.location.x) - f64::from(scroll[0]),
            f64::from(layout.location.y) - f64::from(scroll[1]),
        ])
    }
    #[cfg(feature = "accessibility")]
    pub(crate) fn semantic_key(&self) -> crate::semantics::Key {
        let focused = self.focused.and_then(|id| self.nodes.get(&id));
        let editor = focused.and_then(|n| n.editor.as_ref());
        crate::semantics::Key {
            tree: self.tree,
            viewport: self.viewport,
            evaluations: self.stats.component_evaluations,
            layouts: self.stats.layout_passes,
            geometry: self.geometry_revision,
            focus: self.semantic_focus(),
            text_revision: focused.map(|n| n.text_revision),
            selection: editor.map(|e| e.selection),
            composing: editor.is_some_and(|e| e.composition.is_some()),
        }
    }
    /// Applies an assistive action through existing focus/activation/controlled-edit
    /// behavior. Prepare coherent geometry before spatial actions. Value/selection
    /// requests reconcile live properties first; disabled/hidden/stale targets are
    /// ignored. Effects retain their normal host flush boundary.
    pub fn semantic_action(
        &mut self,
        runtime: &mut Runtime,
        action: crate::SemanticAction,
        measure: &mut impl TextMeasure,
    ) -> Result<bool, UiError> {
        self.sync_edit_views(runtime)?;
        let id = action.target();
        if !self.semantic_visible(id) {
            return Ok(false);
        }
        match action {
            crate::SemanticAction::Focus(_) => {
                if !self.geometry_ready || !self.enabled(id) {
                    return Ok(false);
                }
                let changed = self.focused != Some(id);
                self.change_focus(Some(id));
                let revealed = self.reveal(id);
                if self.nodes[&id].editor.is_some() {
                    self.update_text_scroll(id, measure)?;
                }
                Ok(changed || revealed)
            }
            crate::SemanticAction::Activate(_) => self.activate(runtime, id),
            crate::SemanticAction::SetValue {
                value: proposed, ..
            } => {
                let ElementKind::TextInput {
                    value,
                    change: Some(change),
                    read_only: false,
                    disabled: false,
                    ..
                } = &self.nodes[&id].element.kind
                else {
                    return Ok(false);
                };
                let old = value.clone();
                let change = change.clone();
                self.cancel_composition(id);
                self.propose_edit(
                    runtime,
                    id,
                    &old,
                    Proposal::new(0..old.len(), &single_line(&proposed), EditKind::Separate),
                    change,
                )?;
                if self.nodes.get(&id).is_some_and(|n| n.editor.is_some()) {
                    self.update_text_scroll(id, measure)?;
                }
                Ok(true)
            }
            crate::SemanticAction::SetSelection {
                text_revision,
                selection,
                ..
            } => {
                let node = &self.nodes[&id];
                let ElementKind::TextInput {
                    value,
                    disabled: false,
                    ..
                } = &node.element.kind
                else {
                    return Ok(false);
                };
                if node.text_revision != text_revision
                    || node.editor.as_ref().unwrap().composition.is_some()
                {
                    return Ok(false);
                }
                for endpoint in [selection.anchor, selection.focus] {
                    if endpoint.byte_offset > value.len()
                        || boundary(value, endpoint.byte_offset) != endpoint.byte_offset
                    {
                        return Ok(false);
                    }
                }
                let changed = node.editor.as_ref().unwrap().selection != selection
                    || self.focused != Some(id);
                self.change_focus(Some(id));
                self.nodes
                    .get_mut(&id)
                    .unwrap()
                    .editor
                    .as_mut()
                    .unwrap()
                    .break_group();
                self.nodes
                    .get_mut(&id)
                    .unwrap()
                    .editor
                    .as_mut()
                    .unwrap()
                    .selection = selection;
                self.caret_visible = true;
                self.update_text_scroll(id, measure)?;
                let revealed = self.geometry_ready && self.reveal(id);
                Ok(changed || revealed)
            }
            crate::SemanticAction::Scroll { offset, .. } => {
                if !self.geometry_ready || offset.iter().any(|v| !v.is_finite()) {
                    return Ok(false);
                }
                let node = self.nodes.get_mut(&id).unwrap();
                if node.element.scroll.is_none() {
                    return Ok(false);
                }
                let next = [
                    offset[0].clamp(0., node.scroll_range[0]),
                    offset[1].clamp(0., node.scroll_range[1]),
                ];
                if node.scroll_offset == next {
                    return Ok(false);
                }
                node.scroll_offset = next;
                self.refresh_geometry()?;
                Ok(true)
            }
            crate::SemanticAction::SetNumericValue { value, .. } => {
                if !value.is_finite() {
                    return Ok(false);
                }
                self.range_value(runtime, id, value, crate::ResizePhase::Accessibility)
            }
            crate::SemanticAction::ScrollIntoView(_) => Ok(self.geometry_ready && self.reveal(id)),
        }
    }
    fn activate(&self, runtime: &mut Runtime, id: ElementId) -> Result<bool, UiError> {
        if !self.geometry_ready || !self.enabled(id) {
            return Ok(false);
        }
        let ElementKind::Button {
            listener: Some(listener),
            ..
        } = &self.nodes[&id].element.kind
        else {
            return Ok(false);
        };
        Ok(runtime.update(|cx| listener.dispatch(&ClickEvent, cx))? == Dispatch::Handled)
    }
}

#[path = "command_dispatch.rs"]
mod command_dispatch;
#[path = "input_dispatch.rs"]
mod input_dispatch;
#[path = "overlay_dispatch.rs"]
mod overlay_dispatch;

#[path = "range_dispatch.rs"]
mod range_dispatch;

#[path = "dock_dispatch.rs"]
pub(crate) mod dock_dispatch;
#[path = "focus.rs"]
pub(crate) mod focus;
#[path = "tab_dispatch.rs"]
mod tab_dispatch;
