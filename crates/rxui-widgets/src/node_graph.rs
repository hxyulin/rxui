//! Specialized retained node-graph surface.

use std::{any::Any, hash::Hash, sync::Arc};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
};
use astrelis_paint::{Brush, Painter, Path, StrokeStyle};
use astrelis_platform::CursorIcon;
use astrelis_text::{TextLayout, TextLayoutRequest, TextStyle, TextWrap};
use astrelis_ui_next::{
    Constraints, Element, EventResult, Invalidation, LayoutContext, SemanticData, SemanticRole,
    UiError, UiInput,
};

use rxui_core::{ActionEmitter, RetainedSpec, View, retained};

/// One positioned graph node.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphNode<Id> {
    /// Stable domain identity.
    pub id: Id,
    /// User-visible title.
    pub title: String,
    /// Canvas position.
    pub position: LogicalPoint,
    /// Logical node size.
    pub size: LogicalSize,
}

/// One directed graph edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphEdge<Id> {
    /// Source node.
    pub from: Id,
    /// Destination node.
    pub to: Id,
}

/// Controlled graph viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphViewport {
    /// Canvas translation.
    pub pan: LogicalPoint,
    /// Canvas scale.
    pub zoom: f32,
}

impl Default for GraphViewport {
    fn default() -> Self {
        Self {
            pan: LogicalPoint::ZERO,
            zoom: 1.0,
        }
    }
}

/// Node-graph interaction.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeGraphAction<Id> {
    /// A node was selected.
    Select(Id),
    /// Empty canvas was selected.
    ClearSelection,
}

/// Retained graph implementation exposed only for [`RetainedSpec`] integration.
#[doc(hidden)]
pub struct NodeGraphElement<Id, Action>
where
    Id: Clone + Eq + Hash + 'static,
    Action: 'static,
{
    nodes: Vec<GraphNode<Id>>,
    edges: Vec<GraphEdge<Id>>,
    viewport: GraphViewport,
    selected: Option<Id>,
    hovered: Option<Id>,
    size: LogicalSize,
    labels: Vec<TextLayout>,
    emitter: ActionEmitter<Action>,
    map_action: Arc<dyn Fn(NodeGraphAction<Id>) -> Action>,
}

impl<Id, Action> NodeGraphElement<Id, Action>
where
    Id: Clone + Eq + Hash + 'static,
    Action: 'static,
{
    fn rect(&self, node: &GraphNode<Id>) -> LogicalRect {
        let zoom = self.viewport.zoom.clamp(0.1, 8.0);
        LogicalRect::from_xywh(
            self.viewport.pan.x + node.position.x * zoom,
            self.viewport.pan.y + node.position.y * zoom,
            node.size.width * zoom,
            node.size.height * zoom,
        )
    }
}

impl<Id, Action> Element for NodeGraphElement<Id, Action>
where
    Id: Clone + Eq + Hash + 'static,
    Action: 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> Result<LogicalSize, UiError> {
        self.size = constraints.constrain(LogicalSize::new(640.0, 360.0));
        self.labels = self
            .nodes
            .iter()
            .map(|node| {
                let mut request = TextLayoutRequest::new(node.title.clone());
                request.style = TextStyle {
                    size: 14.0,
                    color: Color::WHITE,
                    ..TextStyle::default()
                };
                request.paragraph.wrap = TextWrap::NoWrap;
                context.shape_text(request)
            })
            .collect::<Result<Vec<_>, _>>()?;
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
                Brush::Solid(Color::from_hex(0x16181d)),
            )?;
            for edge in &self.edges {
                let Some(from) = self.nodes.iter().find(|node| node.id == edge.from) else {
                    continue;
                };
                let Some(to) = self.nodes.iter().find(|node| node.id == edge.to) else {
                    continue;
                };
                let from = self.rect(from);
                let from = LogicalPoint::new(
                    from.origin.x + from.size.width * 0.5,
                    from.origin.y + from.size.height * 0.5,
                );
                let to = self.rect(to);
                let to = LogicalPoint::new(
                    to.origin.x + to.size.width * 0.5,
                    to.origin.y + to.size.height * 0.5,
                );
                let mut path = Path::builder();
                path.move_to(from)?;
                path.line_to(to)?;
                painter.stroke_path(
                    &path.finish(),
                    StrokeStyle {
                        width: 2.0,
                        ..StrokeStyle::default()
                    },
                    Brush::Solid(Color::from_hex(0x7f8798)),
                )?;
            }
            for (node, label) in self.nodes.iter().zip(&self.labels) {
                let rect = self.rect(node);
                painter.fill_rect(
                    rect,
                    Brush::Solid(if self.selected.as_ref() == Some(&node.id) {
                        Color::from_hex(0x4c8dff)
                    } else if self.hovered.as_ref() == Some(&node.id) {
                        Color::from_hex(0x343944)
                    } else {
                        Color::from_hex(0x23262e)
                    }),
                )?;
                painter.draw_text(
                    label,
                    LogicalPoint::new(
                        rect.origin.x + (rect.size.width - label.size().width).max(0.0) * 0.5,
                        rect.origin.y + (rect.size.height - label.size().height).max(0.0) * 0.5,
                    ),
                    1.0,
                )?;
            }
            Ok(())
        })
    }

    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::Graph,
            label: "Node graph".into(),
            value: Some(format!(
                "{} nodes, {} edges",
                self.nodes.len(),
                self.edges.len()
            )),
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
            UiInput::PointerMoved(point) => {
                let hovered = self
                    .nodes
                    .iter()
                    .rev()
                    .find(|node| self.rect(node).contains(point))
                    .map(|node| node.id.clone());
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
            UiInput::PointerReleased(point) => {
                let action = self
                    .nodes
                    .iter()
                    .rev()
                    .find(|node| self.rect(node).contains(point))
                    .map(|node| NodeGraphAction::Select(node.id.clone()))
                    .unwrap_or(NodeGraphAction::ClearSelection);
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
        if self.hovered.is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Move
        }
    }
}

/// Reconciled node-graph configuration.
pub struct NodeGraphSpec<Id, Action>
where
    Id: Clone + Eq + Hash + 'static,
    Action: 'static,
{
    /// Nodes.
    pub nodes: Vec<GraphNode<Id>>,
    /// Edges.
    pub edges: Vec<GraphEdge<Id>>,
    /// Controlled viewport.
    pub viewport: GraphViewport,
    /// Controlled selection.
    pub selected: Option<Id>,
    map_action: Arc<dyn Fn(NodeGraphAction<Id>) -> Action>,
}

impl<Id, Action> Clone for NodeGraphSpec<Id, Action>
where
    Id: Clone + Eq + Hash + 'static,
    Action: 'static,
{
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            viewport: self.viewport,
            selected: self.selected.clone(),
            map_action: self.map_action.clone(),
        }
    }
}

impl<Id, Action> NodeGraphSpec<Id, Action>
where
    Id: Clone + Eq + Hash + 'static,
    Action: 'static,
{
    /// Creates a node-graph specification.
    pub fn new(
        nodes: Vec<GraphNode<Id>>,
        edges: Vec<GraphEdge<Id>>,
        map_action: impl Fn(NodeGraphAction<Id>) -> Action + 'static,
    ) -> Self {
        Self {
            nodes,
            edges,
            viewport: GraphViewport::default(),
            selected: None,
            map_action: Arc::new(map_action),
        }
    }
}

impl<Id, Action> RetainedSpec<Action> for NodeGraphSpec<Id, Action>
where
    Id: Clone + Eq + Hash + 'static,
    Action: 'static,
{
    type Element = NodeGraphElement<Id, Action>;

    fn create(&self, emitter: &ActionEmitter<Action>, _theme: &rxui_core::Theme) -> Self::Element {
        NodeGraphElement {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            viewport: self.viewport,
            selected: self.selected.clone(),
            hovered: None,
            size: LogicalSize::ZERO,
            labels: Vec::new(),
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
        element.nodes.clone_from(&self.nodes);
        element.edges.clone_from(&self.edges);
        element.viewport = self.viewport;
        element.selected.clone_from(&self.selected);
        element.emitter = emitter.clone();
        element.map_action = self.map_action.clone();
    }

    fn changed(&self, previous: &Self) -> Invalidation {
        // Narrowing this is a per-widget judgement about which passes each field
        // feeds, and it moves the engine's `PassStats`; the protocol change only
        // makes it expressible.
        if self.nodes != previous.nodes
            || self.edges != previous.edges
            || self.viewport != previous.viewport
            || self.selected != previous.selected
        {
            Invalidation::ALL
        } else {
            Invalidation::empty()
        }
    }
}

/// Builds an incremental retained node graph.
pub fn node_graph<Id, Action>(spec: NodeGraphSpec<Id, Action>) -> View<Action>
where
    Id: Clone + Eq + Hash + 'static,
    Action: 'static,
{
    retained(spec)
}
