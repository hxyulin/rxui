//! Serializable, message-driven node graph editing.

use std::{
    any::Any,
    collections::{HashMap, HashSet},
    error::Error,
    fmt::{self, Display},
    hash::Hash,
};

use astrelis_core::geometry::{LogicalPoint, LogicalRect, LogicalSize, Point, Rect, Size};
use astrelis_paint::{Brush, CornerRadii, Painter, Path, RoundedRect, StrokeStyle};
use astrelis_platform::{DeviceId, ElementState, Key, NamedKey, PointerButton};
use astrelis_text::{FontFamily, TextLayout, TextLayoutContext, TextLayoutRequest, TextWrap};
use astrelis_ui_core::{
    EventContext, RoutedEvent, RoutedEventKind, SemanticAction, SemanticActionKind, SemanticRole,
    Theme, UiError, Widget, WidgetContainerStyle, deterministic_font_database,
};
use rxui_widgets::{ViewportNavigationBindings, ViewportNavigationIntent};
use serde::{Deserialize, Serialize};

/// Current serialized node-graph envelope version.
pub const NODE_GRAPH_FORMAT_VERSION: u32 = 1;

/// Serializable graph-space point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphPoint {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}

impl GraphPoint {
    /// Creates a graph point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Serializable graph-space size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphSize {
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl GraphSize {
    /// Creates a graph size.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Port flow direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphPortDirection {
    /// Receives an edge.
    Input,
    /// Produces an edge.
    Output,
}

/// One labeled node port.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphPort<Id> {
    /// Stable port identity within its node.
    pub id: Id,
    /// User-visible name.
    pub label: String,
    /// Input or output direction.
    pub direction: GraphPortDirection,
}

/// One positioned graph node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphNode<Id> {
    /// Stable node identity.
    pub id: Id,
    /// User-visible title.
    pub title: String,
    /// Top-left graph position.
    pub position: GraphPoint,
    /// Node dimensions.
    pub size: GraphSize,
    /// Ordered input and output ports.
    pub ports: Vec<GraphPort<Id>>,
}

/// One edge endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEndpoint<Id> {
    /// Owning node.
    pub node: Id,
    /// Port within the node.
    pub port: Id,
}

/// Directed connection between an output and input port.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge<Id> {
    /// Stable edge identity.
    pub id: Id,
    /// Output endpoint.
    pub from: GraphEndpoint<Id>,
    /// Input endpoint.
    pub to: GraphEndpoint<Id>,
}

/// Serializable graph camera.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphViewport {
    /// Screen-space translation before zoom.
    pub pan: GraphPoint,
    /// Uniform scale, clamped by the view to `0.1..=4`.
    pub zoom: f32,
}

impl Default for GraphViewport {
    fn default() -> Self {
        Self {
            pan: GraphPoint::new(32.0, 32.0),
            zoom: 1.0,
        }
    }
}

/// Versioned serializable graph model and camera.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeGraphDocument<Id> {
    /// Envelope version.
    pub format_version: u32,
    /// Nodes in paint order.
    pub nodes: Vec<GraphNode<Id>>,
    /// Routed edges.
    pub edges: Vec<GraphEdge<Id>>,
    /// Saved camera.
    pub viewport: GraphViewport,
}

impl<Id> NodeGraphDocument<Id> {
    /// Creates an empty version-1 document.
    pub fn new() -> Self {
        Self {
            format_version: NODE_GRAPH_FORMAT_VERSION,
            nodes: Vec::new(),
            edges: Vec::new(),
            viewport: GraphViewport::default(),
        }
    }
}

impl<Id> Default for NodeGraphDocument<Id> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Id: Eq + Hash + Display> NodeGraphDocument<Id> {
    /// Validates identities, geometry, port directions, and edge endpoints.
    pub fn validate(&self) -> Result<(), NodeGraphError> {
        if self.format_version != NODE_GRAPH_FORMAT_VERSION {
            return Err(NodeGraphError(format!(
                "unsupported node graph format {}",
                self.format_version
            )));
        }
        if !self.viewport.zoom.is_finite()
            || !(0.1..=4.0).contains(&self.viewport.zoom)
            || !finite(self.viewport.pan)
        {
            return Err(NodeGraphError(
                "graph viewport must be finite with zoom in 0.1..=4".into(),
            ));
        }
        let mut nodes = HashMap::new();
        for node in &self.nodes {
            if nodes.insert(&node.id, node).is_some() {
                return Err(NodeGraphError(format!("duplicate node id {}", node.id)));
            }
            if !finite(node.position)
                || !node.size.width.is_finite()
                || !node.size.height.is_finite()
                || node.size.width <= 0.0
                || node.size.height <= 0.0
            {
                return Err(NodeGraphError(format!(
                    "node {} has invalid geometry",
                    node.id
                )));
            }
            let mut ports = HashSet::new();
            for port in &node.ports {
                if !ports.insert(&port.id) {
                    return Err(NodeGraphError(format!(
                        "node {} has duplicate port {}",
                        node.id, port.id
                    )));
                }
            }
        }
        let mut edges = HashSet::new();
        for edge in &self.edges {
            if !edges.insert(&edge.id) {
                return Err(NodeGraphError(format!("duplicate edge id {}", edge.id)));
            }
            validate_endpoint(&nodes, &edge.from, GraphPortDirection::Output)?;
            validate_endpoint(&nodes, &edge.to, GraphPortDirection::Input)?;
        }
        Ok(())
    }
}

fn validate_endpoint<Id: Eq + Hash + Display>(
    nodes: &HashMap<&Id, &GraphNode<Id>>,
    endpoint: &GraphEndpoint<Id>,
    direction: GraphPortDirection,
) -> Result<(), NodeGraphError> {
    let node = nodes
        .get(&endpoint.node)
        .ok_or_else(|| NodeGraphError(format!("edge references missing node {}", endpoint.node)))?;
    let port = node
        .ports
        .iter()
        .find(|port| port.id == endpoint.port)
        .ok_or_else(|| {
            NodeGraphError(format!(
                "edge references missing port {} on node {}",
                endpoint.port, endpoint.node
            ))
        })?;
    if port.direction != direction {
        return Err(NodeGraphError(format!(
            "port {} on node {} has the wrong direction",
            endpoint.port, endpoint.node
        )));
    }
    Ok(())
}

fn finite(point: GraphPoint) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

/// Invalid graph document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeGraphError(String);
impl fmt::Display for NodeGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
impl Error for NodeGraphError {}

/// Controlled node and edge selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeGraphSelection<Id> {
    /// Selected node identities.
    pub nodes: Vec<Id>,
    /// Selected edge identities.
    pub edges: Vec<Id>,
}

impl<Id> Default for NodeGraphSelection<Id> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

/// Whether a continuous edit is a preview or undoable commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphInteractionPhase {
    /// A continuous gesture is still in progress.
    Preview,
    /// The interaction should become one undoable edit.
    Commit,
    /// The interaction was cancelled and its preview should be discarded.
    Cancel,
}

/// Typed graph interaction emitted to application state.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeGraphAction<Id> {
    /// Replaces controlled selection.
    SetSelection(NodeGraphSelection<Id>),
    /// Replaces positions for one drag or keyboard nudge.
    SetNodePositions {
        /// Absolute replacement positions.
        positions: Vec<(Id, GraphPoint)>,
        /// Gesture lifecycle phase.
        phase: GraphInteractionPhase,
    },
    /// Requests a new directed edge.
    Connect {
        /// Output endpoint.
        from: GraphEndpoint<Id>,
        /// Input endpoint.
        to: GraphEndpoint<Id>,
    },
    /// Requests deletion of the controlled selection.
    DeleteSelection(NodeGraphSelection<Id>),
    /// Replaces the camera.
    SetViewport {
        /// Replacement camera.
        viewport: GraphViewport,
        /// Gesture lifecycle phase.
        phase: GraphInteractionPhase,
    },
    /// Requests a camera that frames all nodes.
    FrameAll,
}

/// Node graph interaction and rendering policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeGraphOptions {
    /// Keyboard movement and background grid spacing.
    pub grid_size: f32,
    /// Whether node movement snaps to the grid.
    pub snap_to_grid: bool,
    /// Mapping from wheels and native gestures to viewport navigation.
    pub navigation: ViewportNavigationBindings,
}

impl Default for NodeGraphOptions {
    fn default() -> Self {
        Self {
            grid_size: 16.0,
            snap_to_grid: true,
            navigation: ViewportNavigationBindings::default(),
        }
    }
}

#[derive(Clone, Debug)]
enum Gesture<Id> {
    Move {
        device: DeviceId,
        start: LogicalPoint,
        originals: Vec<(Id, GraphPoint)>,
    },
    Box {
        device: DeviceId,
        start: LogicalPoint,
        current: LogicalPoint,
        original: NodeGraphSelection<Id>,
    },
    Pan {
        device: DeviceId,
        start: LogicalPoint,
        original: GraphViewport,
    },
    Connect {
        device: DeviceId,
        from: GraphEndpoint<Id>,
        current: LogicalPoint,
    },
}

struct NodeLabel<Id> {
    id: Id,
    title: String,
    layout: TextLayout,
}

/// Retained node-graph editor.
pub struct NodeGraphView<Id, Message> {
    document: NodeGraphDocument<Id>,
    selection: NodeGraphSelection<Id>,
    options: NodeGraphOptions,
    gesture: Option<Gesture<Id>>,
    space_down: bool,
    node_labels: Vec<NodeLabel<Id>>,
    on_action: Box<dyn FnMut(NodeGraphAction<Id>) -> Message>,
}

impl<Id, Message> NodeGraphView<Id, Message>
where
    Id: Clone + Eq + Hash + Display + 'static,
{
    /// Creates a validated graph editor.
    pub fn new(
        document: NodeGraphDocument<Id>,
        options: NodeGraphOptions,
        on_action: impl FnMut(NodeGraphAction<Id>) -> Message + 'static,
    ) -> Result<Self, NodeGraphError> {
        document.validate()?;
        if !options.grid_size.is_finite() || options.grid_size <= 0.0 {
            return Err(NodeGraphError(
                "grid size must be finite and positive".into(),
            ));
        }
        let node_labels = shape_node_labels(&document)?;
        Ok(Self {
            document,
            selection: NodeGraphSelection::default(),
            options,
            gesture: None,
            space_down: false,
            node_labels,
            on_action: Box::new(on_action),
        })
    }

    /// Synchronizes the controlled document and selection.
    pub fn sync(
        &mut self,
        document: NodeGraphDocument<Id>,
        selection: NodeGraphSelection<Id>,
    ) -> Result<(), NodeGraphError> {
        document.validate()?;
        let labels_changed = self.node_labels.len() != document.nodes.len()
            || self
                .node_labels
                .iter()
                .zip(&document.nodes)
                .any(|(label, node)| label.id != node.id || label.title != node.title);
        if labels_changed {
            self.node_labels = shape_node_labels(&document)?;
        }
        self.document = document;
        self.selection = selection;
        Ok(())
    }

    /// Current document, including transient previews.
    pub const fn document(&self) -> &NodeGraphDocument<Id> {
        &self.document
    }

    fn emit(&mut self, context: &mut EventContext<'_, Message>, action: NodeGraphAction<Id>) {
        context.emit((self.on_action)(action));
    }
    fn local_bounds(bounds: LogicalRect) -> LogicalRect {
        Rect::from_xywh(0.0, 0.0, bounds.size.width, bounds.size.height)
    }
    fn screen_point(&self, graph: GraphPoint, bounds: LogicalRect) -> LogicalPoint {
        Point::new(
            bounds.origin.x + self.document.viewport.pan.x + graph.x * self.document.viewport.zoom,
            bounds.origin.y + self.document.viewport.pan.y + graph.y * self.document.viewport.zoom,
        )
    }
    fn node_rect(&self, node: &GraphNode<Id>, bounds: LogicalRect) -> LogicalRect {
        let origin = self.screen_point(node.position, bounds);
        Rect::from_xywh(
            origin.x,
            origin.y,
            node.size.width * self.document.viewport.zoom,
            node.size.height * self.document.viewport.zoom,
        )
    }
    fn node_at(&self, point: LogicalPoint, bounds: LogicalRect) -> Option<&GraphNode<Id>> {
        self.document
            .nodes
            .iter()
            .rev()
            .find(|node| self.node_rect(node, bounds).contains(point))
    }

    fn port_point(
        &self,
        node: &GraphNode<Id>,
        port: &GraphPort<Id>,
        bounds: LogicalRect,
    ) -> LogicalPoint {
        let same = node
            .ports
            .iter()
            .filter(|candidate| candidate.direction == port.direction)
            .collect::<Vec<_>>();
        let index = same
            .iter()
            .position(|candidate| candidate.id == port.id)
            .unwrap_or(0);
        let rect = self.node_rect(node, bounds);
        Point::new(
            if port.direction == GraphPortDirection::Input {
                rect.origin.x
            } else {
                rect.max_x()
            },
            rect.origin.y + rect.size.height * (index + 1) as f32 / (same.len() + 1) as f32,
        )
    }

    fn port_at(
        &self,
        point: LogicalPoint,
        bounds: LogicalRect,
    ) -> Option<(GraphEndpoint<Id>, GraphPortDirection)> {
        for node in self.document.nodes.iter().rev() {
            for port in &node.ports {
                let position = self.port_point(node, port, bounds);
                if (position.x - point.x).powi(2) + (position.y - point.y).powi(2) <= 100.0 {
                    return Some((
                        GraphEndpoint {
                            node: node.id.clone(),
                            port: port.id.clone(),
                        },
                        port.direction,
                    ));
                }
            }
        }
        None
    }

    fn endpoint_point(
        &self,
        endpoint: &GraphEndpoint<Id>,
        bounds: LogicalRect,
    ) -> Option<LogicalPoint> {
        let node = self
            .document
            .nodes
            .iter()
            .find(|node| node.id == endpoint.node)?;
        let port = node.ports.iter().find(|port| port.id == endpoint.port)?;
        Some(self.port_point(node, port, bounds))
    }

    fn selected_positions(&self) -> Vec<(Id, GraphPoint)> {
        self.document
            .nodes
            .iter()
            .filter(|node| self.selection.nodes.contains(&node.id))
            .map(|node| (node.id.clone(), node.position))
            .collect()
    }
    fn apply_positions(&mut self, positions: &[(Id, GraphPoint)]) {
        for (id, position) in positions {
            if let Some(node) = self.document.nodes.iter_mut().find(|node| &node.id == id) {
                node.position = *position;
            }
        }
    }
}

impl<Id, Message> Widget<Message> for NodeGraphView<Id, Message>
where
    Id: Clone + Eq + Hash + Display + 'static,
    Message: 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn intrinsic_size(&self, _theme: &Theme) -> LogicalSize {
        Size::new(720.0, 480.0)
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
        let bounds = Self::local_bounds(context.bounds());
        match &event.kind {
            RoutedEventKind::Keyboard(input)
                if matches!(input.logical_key, Key::Named(NamedKey::Space)) =>
            {
                self.space_down = input.state == ElementState::Pressed;
            }
            RoutedEventKind::PointerButton {
                device_id,
                position,
                button,
                state: ElementState::Pressed,
            } if matches!(button, PointerButton::Primary | PointerButton::Middle) => {
                let Some(local) = context.window_to_local(*position) else {
                    return;
                };
                context.request_focus();
                context.capture_pointer(*device_id);
                if *button == PointerButton::Middle || self.space_down {
                    self.gesture = Some(Gesture::Pan {
                        device: *device_id,
                        start: local,
                        original: self.document.viewport,
                    });
                } else if let Some((from, GraphPortDirection::Output)) = self.port_at(local, bounds)
                {
                    self.gesture = Some(Gesture::Connect {
                        device: *device_id,
                        from,
                        current: local,
                    });
                } else if let Some(node) = self.node_at(local, bounds) {
                    let id = node.id.clone();
                    if !self.selection.nodes.contains(&id) {
                        self.selection = NodeGraphSelection {
                            nodes: vec![id],
                            edges: Vec::new(),
                        };
                        self.emit(
                            context,
                            NodeGraphAction::SetSelection(self.selection.clone()),
                        );
                    }
                    self.gesture = Some(Gesture::Move {
                        device: *device_id,
                        start: local,
                        originals: self.selected_positions(),
                    });
                } else {
                    self.gesture = Some(Gesture::Box {
                        device: *device_id,
                        start: local,
                        current: local,
                        original: self.selection.clone(),
                    });
                }
            }
            RoutedEventKind::PointerMoved {
                device_id,
                position,
            } => {
                let Some(local) = context.window_to_local(*position) else {
                    return;
                };
                let Some(gesture) = self.gesture.clone() else {
                    return;
                };
                match gesture {
                    Gesture::Move {
                        device,
                        start,
                        originals,
                    } if device == *device_id => {
                        let mut positions = originals
                            .into_iter()
                            .map(|(id, original)| {
                                let mut next = GraphPoint::new(
                                    original.x + (local.x - start.x) / self.document.viewport.zoom,
                                    original.y + (local.y - start.y) / self.document.viewport.zoom,
                                );
                                if self.options.snap_to_grid {
                                    next.x = (next.x / self.options.grid_size).round()
                                        * self.options.grid_size;
                                    next.y = (next.y / self.options.grid_size).round()
                                        * self.options.grid_size;
                                }
                                (id, next)
                            })
                            .collect::<Vec<_>>();
                        self.apply_positions(&positions);
                        self.emit(
                            context,
                            NodeGraphAction::SetNodePositions {
                                positions: std::mem::take(&mut positions),
                                phase: GraphInteractionPhase::Preview,
                            },
                        );
                    }
                    Gesture::Box {
                        device,
                        start,
                        original,
                        ..
                    } if device == *device_id => {
                        self.gesture = Some(Gesture::Box {
                            device,
                            start,
                            current: local,
                            original,
                        });
                        let rect = normalized_rect(start, local);
                        self.selection.nodes = self
                            .document
                            .nodes
                            .iter()
                            .filter(|node| intersects(rect, self.node_rect(node, bounds)))
                            .map(|node| node.id.clone())
                            .collect();
                    }
                    Gesture::Pan {
                        device,
                        start,
                        original,
                    } if device == *device_id => {
                        self.document.viewport = GraphViewport {
                            pan: GraphPoint::new(
                                original.pan.x + local.x - start.x,
                                original.pan.y + local.y - start.y,
                            ),
                            zoom: original.zoom,
                        };
                        self.emit(
                            context,
                            NodeGraphAction::SetViewport {
                                viewport: self.document.viewport,
                                phase: GraphInteractionPhase::Preview,
                            },
                        );
                    }
                    Gesture::Connect { device, from, .. } if device == *device_id => {
                        self.gesture = Some(Gesture::Connect {
                            device,
                            from,
                            current: local,
                        });
                    }
                    _ => return,
                }
                context.request_paint();
            }
            RoutedEventKind::PointerButton {
                device_id,
                position,
                button: PointerButton::Primary | PointerButton::Middle,
                state: ElementState::Released,
            } => {
                let local = context.window_to_local(*position);
                let gesture = self.gesture.take();
                context.release_pointer(*device_id);
                match gesture {
                    Some(Gesture::Move { .. }) => self.emit(
                        context,
                        NodeGraphAction::SetNodePositions {
                            positions: self.selected_positions(),
                            phase: GraphInteractionPhase::Commit,
                        },
                    ),
                    Some(Gesture::Box { .. }) => self.emit(
                        context,
                        NodeGraphAction::SetSelection(self.selection.clone()),
                    ),
                    Some(Gesture::Pan { .. }) => self.emit(
                        context,
                        NodeGraphAction::SetViewport {
                            viewport: self.document.viewport,
                            phase: GraphInteractionPhase::Commit,
                        },
                    ),
                    Some(Gesture::Connect { from, .. }) => {
                        if let Some(local) = local
                            && let Some((to, GraphPortDirection::Input)) =
                                self.port_at(local, bounds)
                        {
                            self.emit(context, NodeGraphAction::Connect { from, to });
                        }
                    }
                    None => {}
                }
                context.request_paint();
            }
            RoutedEventKind::PointerCancelled { device_id } => {
                match self.gesture.take() {
                    Some(Gesture::Move { originals, .. }) => {
                        self.apply_positions(&originals);
                        self.emit(
                            context,
                            NodeGraphAction::SetNodePositions {
                                positions: originals,
                                phase: GraphInteractionPhase::Cancel,
                            },
                        );
                    }
                    Some(Gesture::Box { original, .. }) => {
                        self.selection = original;
                        self.emit(
                            context,
                            NodeGraphAction::SetSelection(self.selection.clone()),
                        );
                    }
                    Some(Gesture::Pan { original, .. }) => {
                        self.document.viewport = original;
                        self.emit(
                            context,
                            NodeGraphAction::SetViewport {
                                viewport: original,
                                phase: GraphInteractionPhase::Cancel,
                            },
                        );
                    }
                    Some(Gesture::Connect { .. }) | None => {}
                }
                context.release_pointer(*device_id);
                context.request_paint();
            }
            RoutedEventKind::Scroll { .. }
            | RoutedEventKind::PinchGesture { .. }
            | RoutedEventKind::PanGesture { .. } => {
                let Some(intent) = self
                    .options
                    .navigation
                    .decode(&event.kind, context.modifiers())
                else {
                    return;
                };
                let old = self.document.viewport;
                match intent {
                    ViewportNavigationIntent::Pan { delta, .. } => {
                        self.document.viewport.pan =
                            GraphPoint::new(old.pan.x - delta.x, old.pan.y - delta.y);
                    }
                    ViewportNavigationIntent::Zoom {
                        position, factor, ..
                    } => {
                        let Some(local) = context.window_to_local(position) else {
                            return;
                        };
                        self.document.viewport = zoom_viewport_around(old, factor, local);
                    }
                }
                context.prevent_default();
                self.emit(
                    context,
                    NodeGraphAction::SetViewport {
                        viewport: self.document.viewport,
                        phase: GraphInteractionPhase::Commit,
                    },
                );
                context.request_paint();
            }
            RoutedEventKind::Keyboard(input) if input.state == ElementState::Pressed => {
                let primary = context.modifiers().control || context.modifiers().super_key;
                match &input.logical_key {
                    Key::Character(value) if primary && value.eq_ignore_ascii_case("a") => {
                        self.selection.nodes = self
                            .document
                            .nodes
                            .iter()
                            .map(|node| node.id.clone())
                            .collect();
                        self.selection.edges = self
                            .document
                            .edges
                            .iter()
                            .map(|edge| edge.id.clone())
                            .collect();
                        self.emit(
                            context,
                            NodeGraphAction::SetSelection(self.selection.clone()),
                        );
                    }
                    Key::Named(NamedKey::Backspace) => self.emit(
                        context,
                        NodeGraphAction::DeleteSelection(self.selection.clone()),
                    ),
                    Key::Named(NamedKey::Other(value)) if value == "Delete" => self.emit(
                        context,
                        NodeGraphAction::DeleteSelection(self.selection.clone()),
                    ),
                    Key::Named(NamedKey::Tab) => {
                        if !self.document.nodes.is_empty() {
                            let current = self
                                .selection
                                .nodes
                                .first()
                                .and_then(|id| {
                                    self.document.nodes.iter().position(|node| &node.id == id)
                                })
                                .unwrap_or(self.document.nodes.len() - 1);
                            let next = (current + 1) % self.document.nodes.len();
                            self.selection = NodeGraphSelection {
                                nodes: vec![self.document.nodes[next].id.clone()],
                                edges: Vec::new(),
                            };
                            self.emit(
                                context,
                                NodeGraphAction::SetSelection(self.selection.clone()),
                            );
                        }
                    }
                    Key::Named(NamedKey::Other(key))
                        if matches!(
                            key.as_str(),
                            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown"
                        ) =>
                    {
                        let amount = self.options.grid_size
                            * if context.modifiers().shift { 10.0 } else { 1.0 };
                        let (x, y) = match key.as_str() {
                            "ArrowLeft" => (-amount, 0.0),
                            "ArrowRight" => (amount, 0.0),
                            "ArrowUp" => (0.0, -amount),
                            _ => (0.0, amount),
                        };
                        let positions = self
                            .selected_positions()
                            .into_iter()
                            .map(|(id, point)| (id, GraphPoint::new(point.x + x, point.y + y)))
                            .collect::<Vec<_>>();
                        self.apply_positions(&positions);
                        self.emit(
                            context,
                            NodeGraphAction::SetNodePositions {
                                positions,
                                phase: GraphInteractionPhase::Commit,
                            },
                        );
                    }
                    Key::Character(value) if value == "+" || value == "=" || value == "-" => {
                        let factor = if value == "-" { 0.8 } else { 1.25 };
                        self.document.viewport.zoom =
                            (self.document.viewport.zoom * factor).clamp(0.1, 4.0);
                        self.emit(
                            context,
                            NodeGraphAction::SetViewport {
                                viewport: self.document.viewport,
                                phase: GraphInteractionPhase::Commit,
                            },
                        );
                    }
                    Key::Named(NamedKey::Other(value)) if value == "Home" => {
                        self.emit(context, NodeGraphAction::FrameAll)
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
        painter.fill_rect(bounds, Brush::Solid(theme.background))?;
        let spacing = self.options.grid_size * self.document.viewport.zoom;
        if spacing >= 6.0 {
            let start_x = bounds.origin.x + self.document.viewport.pan.x.rem_euclid(spacing);
            let start_y = bounds.origin.y + self.document.viewport.pan.y.rem_euclid(spacing);
            let mut x = start_x;
            while x < bounds.max_x() {
                painter.fill_rect(
                    Rect::from_xywh(x, bounds.origin.y, 1.0, bounds.size.height),
                    Brush::Solid(theme.border.with_alpha(0.35)),
                )?;
                x += spacing;
            }
            let mut y = start_y;
            while y < bounds.max_y() {
                painter.fill_rect(
                    Rect::from_xywh(bounds.origin.x, y, bounds.size.width, 1.0),
                    Brush::Solid(theme.border.with_alpha(0.35)),
                )?;
                y += spacing;
            }
        }
        for edge in &self.document.edges {
            let (Some(from), Some(to)) = (
                self.endpoint_point(&edge.from, bounds),
                self.endpoint_point(&edge.to, bounds),
            ) else {
                continue;
            };
            paint_edge(
                painter,
                from,
                to,
                if self.selection.edges.contains(&edge.id) {
                    theme.accent
                } else {
                    theme.muted_foreground
                },
            )?;
        }
        if let Some(Gesture::Connect { from, current, .. }) = &self.gesture
            && let Some(start) = self.endpoint_point(from, bounds)
        {
            paint_edge(
                painter,
                start,
                Point::new(bounds.origin.x + current.x, bounds.origin.y + current.y),
                theme.accent,
            )?;
        }
        for node in &self.document.nodes {
            let rect = self.node_rect(node, bounds);
            let rounded = RoundedRect::new(rect, CornerRadii::uniform(theme.radii.md))?;
            painter.fill_rounded_rect(
                rounded,
                Brush::Solid(if self.selection.nodes.contains(&node.id) {
                    theme.selection
                } else {
                    theme.surface
                }),
            )?;
            painter.stroke_rounded_rect(
                rounded,
                StrokeStyle {
                    width: if self.selection.nodes.contains(&node.id) {
                        2.0
                    } else {
                        1.0
                    },
                    ..Default::default()
                },
                Brush::Solid(if self.selection.nodes.contains(&node.id) {
                    theme.accent
                } else {
                    theme.border
                }),
            )?;
            let header = Rect::from_xywh(
                rect.origin.x,
                rect.origin.y,
                rect.size.width,
                (28.0 * self.document.viewport.zoom).min(rect.size.height),
            );
            painter.fill_rect(header, Brush::Solid(theme.accent))?;
            if let Some(label) = self.node_labels.iter().find(|label| label.id == node.id) {
                painter.with_save(|painter| {
                    painter.clip_rect(header)?;
                    painter.draw_text(
                        &label.layout,
                        Point::new(header.origin.x + 9.0, header.origin.y + 5.0),
                        1.0,
                    )
                })?;
            }
            for port in &node.ports {
                let point = self.port_point(node, port, bounds);
                painter.fill_ellipse(
                    Rect::from_xywh(point.x - 5.0, point.y - 5.0, 10.0, 10.0),
                    Brush::Solid(if port.direction == GraphPortDirection::Input {
                        theme.success
                    } else {
                        theme.accent
                    }),
                )?;
            }
        }
        if let Some(Gesture::Box { start, current, .. }) = self.gesture {
            let rect = normalized_rect(
                Point::new(bounds.origin.x + start.x, bounds.origin.y + start.y),
                Point::new(bounds.origin.x + current.x, bounds.origin.y + current.y),
            );
            painter.fill_rect(rect, Brush::Solid(theme.selection))?;
            painter.stroke_rect(rect, StrokeStyle::default(), Brush::Solid(theme.accent))?;
        }
        Ok(())
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((
            SemanticRole::Group,
            "Node graph".into(),
            Some(format!(
                "{} nodes, {} edges, {} selected",
                self.document.nodes.len(),
                self.document.edges.len(),
                self.selection.nodes.len() + self.selection.edges.len()
            )),
        ))
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
                self.document.viewport.zoom =
                    (self.document.viewport.zoom * (1.0 + *delta * 0.02)).clamp(0.1, 4.0);
                self.emit(
                    context,
                    NodeGraphAction::SetViewport {
                        viewport: self.document.viewport,
                        phase: GraphInteractionPhase::Commit,
                    },
                );
                context.request_paint();
                true
            }
            _ => false,
        }
    }
}

fn zoom_viewport_around(
    viewport: GraphViewport,
    factor: f64,
    position: LogicalPoint,
) -> GraphViewport {
    let zoom = (viewport.zoom * factor as f32).clamp(0.1, 4.0);
    let graph_x = (position.x - viewport.pan.x) / viewport.zoom;
    let graph_y = (position.y - viewport.pan.y) / viewport.zoom;
    GraphViewport {
        pan: GraphPoint::new(position.x - graph_x * zoom, position.y - graph_y * zoom),
        zoom,
    }
}

fn shape_node_labels<Id>(
    document: &NodeGraphDocument<Id>,
) -> Result<Vec<NodeLabel<Id>>, NodeGraphError>
where
    Id: Clone,
{
    let mut fonts = deterministic_font_database();
    let mut context = TextLayoutContext::new();
    document
        .nodes
        .iter()
        .map(|node| {
            let mut request = TextLayoutRequest::new(&node.title);
            request.style.families = vec![FontFamily::Named("Noto Sans".into())];
            request.style.size = 13.0;
            request.style.weight = 600.0;
            request.paragraph.wrap = TextWrap::NoWrap;
            let layout = context
                .layout(&mut fonts, request)
                .map_err(|error| NodeGraphError(format!("could not shape node title: {error}")))?;
            Ok(NodeLabel {
                id: node.id.clone(),
                title: node.title.clone(),
                layout,
            })
        })
        .collect()
}

fn normalized_rect(first: LogicalPoint, second: LogicalPoint) -> LogicalRect {
    Rect::from_xywh(
        first.x.min(second.x),
        first.y.min(second.y),
        (first.x - second.x).abs(),
        (first.y - second.y).abs(),
    )
}
fn intersects(a: LogicalRect, b: LogicalRect) -> bool {
    a.origin.x <= b.max_x()
        && a.max_x() >= b.origin.x
        && a.origin.y <= b.max_y()
        && a.max_y() >= b.origin.y
}
fn paint_edge(
    painter: &mut Painter,
    from: LogicalPoint,
    to: LogicalPoint,
    color: astrelis_core::color::Color,
) -> Result<(), UiError> {
    let lead = ((to.x - from.x).abs() * 0.15).clamp(24.0, 72.0);
    let from_lead = Point::new(from.x + lead, from.y);
    let to_lead = Point::new(to.x - lead, to.y);
    let control = ((to_lead.x - from_lead.x).abs() * 0.5).max(40.0);
    let mut path = Path::builder();
    path.move_to(from)?;
    path.line_to(from_lead)?;
    path.cubic_to(
        Point::new(from_lead.x + control, from_lead.y),
        Point::new(to_lead.x - control, to_lead.y),
        to_lead,
    )?;
    path.line_to(to)?;
    painter.stroke_path(
        &path.finish(),
        StrokeStyle {
            width: 2.0,
            ..Default::default()
        },
        Brush::Solid(color),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document() -> NodeGraphDocument<u8> {
        NodeGraphDocument {
            format_version: 1,
            viewport: GraphViewport::default(),
            nodes: vec![
                GraphNode {
                    id: 1,
                    title: "Source".into(),
                    position: GraphPoint::new(0.0, 0.0),
                    size: GraphSize::new(120.0, 80.0),
                    ports: vec![GraphPort {
                        id: 1,
                        label: "out".into(),
                        direction: GraphPortDirection::Output,
                    }],
                },
                GraphNode {
                    id: 2,
                    title: "Sink".into(),
                    position: GraphPoint::new(200.0, 0.0),
                    size: GraphSize::new(120.0, 80.0),
                    ports: vec![GraphPort {
                        id: 2,
                        label: "in".into(),
                        direction: GraphPortDirection::Input,
                    }],
                },
            ],
            edges: vec![GraphEdge {
                id: 3,
                from: GraphEndpoint { node: 1, port: 1 },
                to: GraphEndpoint { node: 2, port: 2 },
            }],
        }
    }
    #[test]
    fn validates_and_round_trips() {
        let document = document();
        document.validate().unwrap();
        let json = serde_json::to_string(&document).unwrap();
        assert_eq!(
            serde_json::from_str::<NodeGraphDocument<u8>>(&json).unwrap(),
            document
        );
    }

    #[test]
    fn node_titles_shape_visible_glyphs() {
        let labels = shape_node_labels(&document()).unwrap();
        assert_eq!(labels[0].layout.text(), "Source");
        assert!(!labels[0].layout.glyph_runs().is_empty());
    }

    #[test]
    fn viewport_zoom_preserves_the_graph_point_under_the_cursor() {
        let viewport = GraphViewport {
            pan: GraphPoint::new(25.0, -10.0),
            zoom: 1.5,
        };
        let cursor = Point::new(220.0, 130.0);
        let before = GraphPoint::new(
            (cursor.x - viewport.pan.x) / viewport.zoom,
            (cursor.y - viewport.pan.y) / viewport.zoom,
        );
        let zoomed = zoom_viewport_around(viewport, 1.8, cursor);
        let after = GraphPoint::new(
            (cursor.x - zoomed.pan.x) / zoomed.zoom,
            (cursor.y - zoomed.pan.y) / zoomed.zoom,
        );
        assert!((before.x - after.x).abs() < 1.0e-4);
        assert!((before.y - after.y).abs() < 1.0e-4);
    }
    #[test]
    fn rejects_dangling_and_wrong_direction_edges() {
        let mut missing = document();
        missing.edges[0].to.node = 9;
        assert!(missing.validate().is_err());
        let mut wrong = document();
        wrong.edges[0].from = wrong.edges[0].to.clone();
        assert!(wrong.validate().is_err());
    }
}
