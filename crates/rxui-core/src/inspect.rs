//! Retained-tree inspection data.
//!
//! The snapshot lives here rather than beside its viewer because it is the only
//! consumer of [`astrelis_ui_next::UiRoot::stats`] besides the component host,
//! and because de-duplicating [`InspectionNode`] against the engine's semantic
//! nodes is a core concern. The viewer is `rxui_widgets::inspector`, behind a
//! feature flag.

use astrelis_core::geometry::LogicalRect;
use astrelis_ui_next::{NodeId, PassStats, SemanticRole, UiRoot};

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
