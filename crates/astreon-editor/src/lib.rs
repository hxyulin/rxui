//! Editor workspace policy and Astrelis docking integrations.

#![warn(missing_docs)]

use astrelis_ui_docking::DockLayout;
use serde::{Deserialize, Serialize};

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
}

impl WorkspaceState {
    /// Wraps a docking layout in the current persistence format.
    pub const fn new(layout: DockLayout) -> Self {
        Self {
            format_version: WORKSPACE_FORMAT_VERSION,
            layout,
        }
    }

    /// Validates that the envelope can be read by this Astreon version.
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
}

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
}
