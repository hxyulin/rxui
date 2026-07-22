//! Serializable editor docking policy and retained workspace implementation.

mod model;
mod workspace;

pub use model::{
    DockAxis, DockError, DockLayout, DockNode, DockPlacement, DockSide, DockTabs, FloatingGroup,
    FloatingRect, NormalizationReport, PanelDescriptor, PanelId, PreferredPlacement,
};
pub use workspace::{
    DockAction, DockFloatFrame, DockGroup, DockOutcome, DockStyle, DockTab, DockWorkspace,
    DockWorkspaceSurface, SplitBranch,
};
