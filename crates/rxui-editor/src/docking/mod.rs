//! Serializable editor docking policy and retained workspace implementation.

mod model;
mod multiviewport;
mod viewport_drag;
mod workspace;

pub use model::{
    DockAxis, DockError, DockLayout, DockNode, DockPlacement, DockSide, DockTabs, FloatingGroup,
    FloatingRect, NormalizationReport, PanelDescriptor, PanelId, PreferredPlacement,
};
pub use multiviewport::{DockViewport, DockViewportId, MultiViewportDockLayout};
pub use viewport_drag::{DockViewportDrag, DockViewportDragEvent};
pub use workspace::{
    DockAction, DockFloatFrame, DockFloatingMode, DockGroup, DockOutcome, DockStyle, DockTab,
    DockWorkspace, DockWorkspaceSurface, NativeViewportRequest, SplitBranch,
};
