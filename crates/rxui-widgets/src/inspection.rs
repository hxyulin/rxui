//! Component-native retained-tree inspection.

use astrelis_core::geometry::LogicalRect;
use astrelis_ui_next::{NodeId, PassStats, SemanticRole, UiRoot};

use rxui_core::{ColorRole, ContainerStyle, Space, View, button, column_with, label, views};

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
                views(snapshot.nodes.iter().map(|node| {
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
                    // The inspected identity is the row's domain identity.
                    // `NodeId` exposes no scalar, so its debug form is the
                    // stable spelling available here.
                    .key(format!("{id:?}"))
                })),
            ),
        ),
    )
}

#[cfg(test)]
mod tests {
    use astrelis_core::geometry::LogicalSize;
    use astrelis_ui_next::SemanticNode;
    use rxui_core::{Component, ComponentContext, ComponentHost, Theme, button, column, views};

    use super::*;

    /// Supplies real retained identities: `NodeId` cannot be built by hand.
    struct Inspected;

    impl Component for Inspected {
        type Action = ();
        type Effect = ();

        fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

        fn view(&self, _theme: &Theme) -> View<()> {
            column(views(
                ["Alpha", "Beta", "Gamma"]
                    .into_iter()
                    .map(|label| button(label, ()).key(label)),
            ))
        }
    }

    struct Inspector {
        snapshot: InspectionSnapshot,
    }

    impl Component for Inspector {
        type Action = NodeId;
        type Effect = ();

        fn update(&mut self, _action: NodeId, _context: &mut ComponentContext<'_, ()>) {}

        fn view(&self, _theme: &Theme) -> View<NodeId> {
            inspection_view(&self.snapshot, None, |id| id)
        }
    }

    fn inspected_buttons() -> Vec<InspectionNode> {
        let host =
            ComponentHost::new(Inspected, LogicalSize::new(320.0, 240.0), Theme::dark()).unwrap();
        let nodes = InspectionSnapshot::capture(host.ui()).nodes;
        let buttons = nodes
            .into_iter()
            .filter(|node| node.role == SemanticRole::Button)
            .collect::<Vec<_>>();
        assert_eq!(buttons.len(), 3);
        buttons
    }

    fn focused(host: &ComponentHost<Inspector>) -> SemanticNode {
        host.ui()
            .semantic_snapshot()
            .into_iter()
            .find(|node| node.focused)
            .expect("a focused node")
    }

    fn row_labeled(host: &ComponentHost<Inspector>, suffix: &str) -> NodeId {
        host.ui()
            .semantic_snapshot()
            .into_iter()
            .find(|node| node.data.label.ends_with(suffix))
            .map(|node| node.id)
            .unwrap_or_else(|| panic!("no row labeled {suffix}"))
    }

    #[test]
    fn inspector_rows_are_keyed_by_inspected_identity() {
        let buttons = inspected_buttons();
        let snapshot = InspectionSnapshot {
            stats: PassStats::default(),
            nodes: buttons[1..].to_vec(),
        };
        let mut host = ComponentHost::new(
            Inspector { snapshot },
            LogicalSize::new(480.0, 240.0),
            Theme::dark(),
        )
        .unwrap();

        let beta = row_labeled(&host, "Beta");
        host.ui_mut().set_focus(Some(beta)).unwrap();
        host.refresh().unwrap();
        assert_eq!(focused(&host).id, beta);

        // The inspected tree grew a node above Beta, which is exactly what a
        // position-keyed row list mistakes for "Beta became Alpha".
        host.component_mut()
            .snapshot
            .nodes
            .insert(0, buttons[0].clone());
        host.refresh().unwrap();

        let focused = focused(&host);
        assert_eq!(focused.id, beta);
        assert!(focused.data.label.ends_with("Beta"), "{:?}", focused.data);
        assert_eq!(row_labeled(&host, "Beta"), beta);
    }

    #[test]
    fn every_captured_node_gets_a_row() {
        let buttons = inspected_buttons();
        let snapshot = InspectionSnapshot {
            stats: PassStats::default(),
            nodes: buttons,
        };
        let host = ComponentHost::new(
            Inspector { snapshot },
            LogicalSize::new(480.0, 240.0),
            Theme::dark(),
        )
        .unwrap();
        let rows = host
            .ui()
            .semantic_snapshot()
            .into_iter()
            .filter(|node| node.data.label.starts_with("  Button: "))
            .count();
        assert_eq!(rows, 3);
    }
}
