//! Specialized retained node-graph surface.

use std::{any::Any, hash::Hash};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
};
use astrelis_paint::{Brush, Painter, Path, StrokeStyle};
use astrelis_platform::CursorIcon;
use astrelis_text::{TextLayout, TextLayoutRequest, TextStyle, TextWrap};
use rxui_core::{CustomElementSpec, Element as CoreElement, RoutedValueHandler, Theme, custom};
use rxui_tree::{
    Constraints, Element, EventResult, Invalidation, KeyedShapingMemo, LayoutContext, SemanticData,
    SemanticRole, UiInput,
};

use super::canvas::{self, Hover};

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

fn title_request(title: &str) -> TextLayoutRequest {
    let mut request = TextLayoutRequest::new(title.to_owned());
    request.style = TextStyle {
        size: 14.0,
        color: Color::WHITE,
        ..TextStyle::default()
    };
    request.paragraph.wrap = TextWrap::NoWrap;
    request
}

/// Retained graph implementation exposed for custom-element integration.
#[doc(hidden)]
pub struct NodeGraphElement<Id>
where
    Id: Clone + Eq + Hash + 'static,
{
    nodes: Vec<GraphNode<Id>>,
    edges: Vec<GraphEdge<Id>>,
    viewport: GraphViewport,
    selected: Option<Id>,
    hovered: Hover<Id>,
    size: LogicalSize,
    labels: Vec<TextLayout>,
    titles: KeyedShapingMemo<Id>,
    on_action: RoutedValueHandler<NodeGraphAction<Id>>,
}

impl<Id> NodeGraphElement<Id>
where
    Id: Clone + Eq + Hash + 'static,
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

impl<Id> Element for NodeGraphElement<Id>
where
    Id: Clone + Eq + Hash + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        self.size = constraints.constrain(LogicalSize::new(640.0, 360.0));
        let requests = self
            .nodes
            .iter()
            .map(|node| (node.id.clone(), title_request(&node.title)));
        self.labels = self.titles.shape_all(context, requests);
        self.size
    }

    fn paint(
        &self,
        painter: &mut Painter,
        size: LogicalSize,
    ) -> Result<(), astrelis_paint::PaintError> {
        canvas::draw(painter, size, canvas::surface_fill(), |painter| {
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
                    } else if self.hovered.current() == Some(&node.id) {
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
            UiInput::HoverChanged(false) => EventResult {
                invalidation: self.hovered.set(None),
                handled: true,
                ..EventResult::default()
            },
            UiInput::PointerMoved(point) => {
                let topmost = self
                    .nodes
                    .iter()
                    .rev()
                    .find(|node| self.rect(node).contains(point))
                    .map(|node| node.id.clone());
                EventResult {
                    invalidation: self.hovered.set(topmost),
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
                EventResult::action(self.on_action.with(action))
            }
            _ => EventResult::default(),
        }
    }

    fn hit_testable(&self) -> bool {
        true
    }
    fn cursor_icon(&self) -> CursorIcon {
        if self.hovered.current().is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Move
        }
    }
}

/// Reconciled node-graph configuration.
pub struct NodeGraphSpec<Id>
where
    Id: Clone + Eq + Hash + 'static,
{
    /// Nodes.
    pub nodes: Vec<GraphNode<Id>>,
    /// Edges.
    pub edges: Vec<GraphEdge<Id>>,
    /// Controlled viewport.
    pub viewport: GraphViewport,
    /// Controlled selection.
    pub selected: Option<Id>,
    on_action: RoutedValueHandler<NodeGraphAction<Id>>,
}

impl<Id> Clone for NodeGraphSpec<Id>
where
    Id: Clone + Eq + Hash + 'static,
{
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            viewport: self.viewport,
            selected: self.selected.clone(),
            on_action: self.on_action.clone(),
        }
    }
}

impl<Id> NodeGraphSpec<Id>
where
    Id: Clone + Eq + Hash + 'static,
{
    /// Creates a node-graph specification routed to its owning entity.
    pub fn new(
        nodes: Vec<GraphNode<Id>>,
        edges: Vec<GraphEdge<Id>>,
        on_action: RoutedValueHandler<NodeGraphAction<Id>>,
    ) -> Self {
        Self {
            nodes,
            edges,
            viewport: GraphViewport::default(),
            selected: None,
            on_action,
        }
    }
}

impl<Id> CustomElementSpec for NodeGraphSpec<Id>
where
    Id: Clone + Eq + Hash + 'static,
{
    type Element = NodeGraphElement<Id>;

    fn create(&self, _theme: &Theme) -> Self::Element {
        NodeGraphElement {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            viewport: self.viewport,
            selected: self.selected.clone(),
            hovered: Hover::default(),
            size: LogicalSize::ZERO,
            labels: Vec::new(),
            titles: KeyedShapingMemo::default(),
            on_action: self.on_action.clone(),
        }
    }

    fn update(&self, element: &mut Self::Element, _theme: &Theme) {
        element.nodes.clone_from(&self.nodes);
        element.edges.clone_from(&self.edges);
        element.viewport = self.viewport;
        element.selected.clone_from(&self.selected);
        element.on_action = self.on_action.clone();
    }

    fn changed(&self, previous: &Self) -> Invalidation {
        let mut invalidation = Invalidation::empty();
        if self.nodes.len() != previous.nodes.len()
            || std::iter::zip(&self.nodes, &previous.nodes).any(|(new, old)| new.title != old.title)
        {
            invalidation |= Invalidation::LAYOUT;
        }
        if self.nodes.len() != previous.nodes.len() || self.edges.len() != previous.edges.len() {
            invalidation |= Invalidation::ACCESSIBILITY;
        }
        if self.nodes != previous.nodes
            || self.edges != previous.edges
            || self.viewport != previous.viewport
            || self.selected != previous.selected
        {
            invalidation |= Invalidation::PAINT;
        }
        invalidation
    }
}

/// Builds an incremental retained node graph.
pub fn node_graph<Id>(spec: NodeGraphSpec<Id>) -> CoreElement
where
    Id: Clone + Eq + Hash + 'static,
{
    custom(spec)
}
