//! Editor workspace policy and Astrelis docking integrations.

#![warn(missing_docs)]

use astrelis_ui_docking::DockLayout;
use serde::{Deserialize, Serialize};

mod node_graph;
mod property;

pub use node_graph::{
    GraphEdge, GraphEndpoint, GraphInteractionPhase, GraphNode, GraphPoint, GraphPort,
    GraphPortDirection, GraphSize, GraphViewport, NODE_GRAPH_FORMAT_VERSION, NodeGraphAction,
    NodeGraphDocument, NodeGraphError, NodeGraphOptions, NodeGraphSelection, NodeGraphView,
};
pub use property::{PropertyAction, PropertyField, PropertyGrid, PropertySection, PropertyValue};

/// Re-exports of Astrelis's retained docking implementation.
pub mod docking {
    pub use astrelis_ui_docking::*;
}

/// Re-exports of scene-view and virtualization foundations used by editors.
pub mod widgets {
    pub use astrelis_ui_widgets::{
        RenderView, RenderViewContent, RenderViewEvent, RenderViewResizePolicy, SplitAxis,
        SplitPane, SplitPaneOptions, VirtualList, VirtualListItem, VirtualListOptions,
    };
}

/// Current serialized workspace envelope version.
pub const WORKSPACE_FORMAT_VERSION: u32 = 1;

/// Versioned, application-serializable editor workspace state.
///
/// Applications may embed this value in JSON, RON, or another serde format.
/// Panel contents remain application-owned and are not serialized here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceState {
    /// Persistence format version.
    pub format_version: u32,
    /// Serializable docking tree and floating geometry.
    pub layout: DockLayout,
    /// User-named layout snapshots in deterministic alphabetical order.
    #[serde(default)]
    pub saved_layouts: Vec<SavedLayout>,
}

impl WorkspaceState {
    /// Wraps a docking layout in the current persistence format.
    pub const fn new(layout: DockLayout) -> Self {
        Self {
            format_version: WORKSPACE_FORMAT_VERSION,
            layout,
            saved_layouts: Vec::new(),
        }
    }

    /// Validates that the envelope can be read by this RXUI version.
    pub fn validate(&self) -> Result<(), WorkspaceVersionError> {
        if self.format_version == WORKSPACE_FORMAT_VERSION {
            Ok(())
        } else {
            Err(WorkspaceVersionError {
                found: self.format_version,
                supported: WORKSPACE_FORMAT_VERSION,
            })
        }
    }

    /// Saves the current layout under a trimmed name, replacing an exact match.
    pub fn save_named(&mut self, name: impl Into<String>) -> Result<bool, SavedLayoutError> {
        let name = name.into().trim().to_owned();
        if name.is_empty() {
            return Err(SavedLayoutError);
        }
        let mut replaced = false;
        if let Some(saved) = self
            .saved_layouts
            .iter_mut()
            .find(|saved| saved.name == name)
        {
            saved.layout = self.layout.clone();
            replaced = true;
        } else {
            self.saved_layouts.push(SavedLayout {
                name,
                layout: self.layout.clone(),
            });
        }
        self.saved_layouts
            .sort_by(|left, right| left.name.cmp(&right.name));
        Ok(replaced)
    }

    /// Loads a named layout into the active layout.
    pub fn load_named(&mut self, name: &str) -> bool {
        let Some(layout) = self
            .saved_layouts
            .iter()
            .find(|saved| saved.name == name)
            .map(|saved| saved.layout.clone())
        else {
            return false;
        };
        self.layout = layout;
        true
    }

    /// Deletes an exact named layout.
    pub fn delete_named(&mut self, name: &str) -> bool {
        let before = self.saved_layouts.len();
        self.saved_layouts.retain(|saved| saved.name != name);
        self.saved_layouts.len() != before
    }
}

/// One user-named docking layout snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedLayout {
    /// Trimmed display name.
    pub name: String,
    /// Serializable docking layout.
    pub layout: DockLayout,
}

/// A saved layout name was empty after trimming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedLayoutError;

impl std::fmt::Display for SavedLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("saved layout names cannot be empty")
    }
}

impl std::error::Error for SavedLayoutError {}

/// Unsupported persisted workspace format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceVersionError {
    /// Version found in persisted data.
    pub found: u32,
    /// Version understood by this library.
    pub supported: u32,
}

impl std::fmt::Display for WorkspaceVersionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "workspace format {} is unsupported; expected {}",
            self.found, self.supported
        )
    }
}

impl std::error::Error for WorkspaceVersionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_workspace_versions() {
        let mut state = WorkspaceState::new(DockLayout::default());
        assert!(state.validate().is_ok());
        state.format_version += 1;
        assert_eq!(
            state.validate(),
            Err(WorkspaceVersionError {
                found: WORKSPACE_FORMAT_VERSION + 1,
                supported: WORKSPACE_FORMAT_VERSION,
            })
        );
    }

    #[test]
    fn named_layouts_replace_sort_load_and_delete() {
        let mut state = WorkspaceState::new(DockLayout::default());
        assert!(!state.save_named(" Zebra ").unwrap());
        assert!(!state.save_named("Alpha").unwrap());
        assert!(state.save_named("Alpha").unwrap());
        assert_eq!(
            state
                .saved_layouts
                .iter()
                .map(|layout| layout.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Alpha", "Zebra"]
        );
        assert!(state.load_named("Alpha"));
        assert!(state.delete_named("Alpha"));
        assert!(!state.delete_named("Missing"));
    }

    #[test]
    fn old_workspace_json_defaults_named_layouts() {
        let json = r#"{"format_version":1,"layout":{"root":null,"floating":[]}}"#;
        let state: WorkspaceState = serde_json::from_str(json).unwrap();
        assert!(state.saved_layouts.is_empty());
    }
}
