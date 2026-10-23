//! Application-controlled proposals between explicitly bound dock surfaces.
use crate::{DockDropTarget, DockNodeId, DockSurfaceId, Key};

/// Release-time proposal to move a panel between dock surfaces. No model or
/// native window changes until the application accepts the proposal.
#[derive(Clone, Debug, PartialEq)]
pub struct DockTransferEvent {
    /// Surface containing the original header.
    pub source_surface: DockSurfaceId,
    /// Original group; applications must revalidate membership when accepting.
    pub source_group: DockNodeId,
    /// Application panel identity.
    pub panel: Key,
    /// Explicitly bound destination surface.
    pub target_surface: DockSurfaceId,
    /// Proposed insertion or split in the destination tree.
    pub target: DockDropTarget,
}

/// Opt-in release outside dock surfaces. The application decides whether to
/// create a native window and how to retain its document entities.
#[derive(Clone, Debug, PartialEq)]
pub struct DockDetachEvent {
    /// Surface containing the original header.
    pub source_surface: DockSurfaceId,
    /// Original group; applications must revalidate membership when accepting.
    pub source_group: DockNodeId,
    /// Application panel identity.
    pub panel: Key,
    /// Release position in logical source-window coordinates, not desktop units.
    pub position: [f32; 2],
}
