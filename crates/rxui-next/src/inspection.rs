//! Component-native retained-tree inspection.

use astrelis_core::geometry::LogicalRect;
use astrelis_ui_next::{NodeId, PassStats, SemanticRole, UiRoot};

use crate::{ColorRole, ContainerStyle, Space, View, button, column_with, label, views};

/// One flattened retained semantic node.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectionNode {
    /// Stable retained identity.
    pub id: NodeId,
    /// Semantic role.
    pub role: SemanticRole,
    /// Accessible label.
    pub label: String,
    /// Accessible value.
    pub value: Option<String>,
    /// Window-space bounds.
    pub bounds: LogicalRect,
    /// Effective interaction enablement.
    pub enabled: bool,
    /// Keyboard focus state.
    pub focused: bool,
}

/// Deterministic retained runtime snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectionSnapshot {
    /// Most recent incremental work counters.
    pub stats: PassStats,
    /// Semantic nodes in retained order.
    pub nodes: Vec<InspectionNode>,
}

impl InspectionSnapshot {
    /// Captures one incremental UI root.
    pub fn capture(ui: &UiRoot) -> Self {
        Self {
            stats: ui.stats(),
            nodes: ui
                .semantic_snapshot()
                .into_iter()
                .map(|node| InspectionNode {
                    id: node.id,
                    role: node.data.role,
                    label: node.data.label,
                    value: node.data.value,
                    bounds: node.bounds,
                    enabled: node.enabled,
                    focused: node.focused,
                })
                .collect(),
        }
    }
}

/// Builds an interactive semantic-tree inspector.
pub fn inspection_view<Action: Clone + 'static>(
    snapshot: &InspectionSnapshot,
    selected: Option<NodeId>,
    on_select: impl Fn(NodeId) -> Action + Clone + 'static,
) -> View<Action> {
    column_with(
        ContainerStyle::new()
            .gap(Space::Xs)
            .padding(Space::Sm)
            .background(ColorRole::Surface),
        (
            label(format!(
                "layout={} paint={} shape={}",
                snapshot.stats.layout_elements,
                snapshot.stats.rebuilt_fragments,
                snapshot.stats.shaped_text
            )),
            column_with(
                ContainerStyle::new().gap(Space::Xs),
                views(snapshot.nodes.iter().enumerate().map(|(index, node)| {
                    let marker = if selected == Some(node.id) {
                        "› "
                    } else {
                        "  "
                    };
                    let id = node.id;
                    let on_select = on_select.clone();
                    button(
                        format!("{marker}{:?}: {}", node.role, node.label),
                        on_select(id),
                    )
                    .enabled(node.enabled)
                    .key(index as u64)
                })),
            ),
        ),
    )
}
