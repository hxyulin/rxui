//! Application policy for stock docking destinations.
use crate::{DockDropTarget, DockNodeId, DockSurfaceId, Key};
use std::rc::Rc;

/// A candidate destination examined before showing a preview and again at release.
///
/// Return false from `Dock::drop_policy` to suppress a destination. The callback
/// reads application snapshots captured when the dock is described; accepting a
/// final event remains the application's responsibility. It should be inexpensive
/// and free of side effects because pointer motion may evaluate it repeatedly.
#[derive(Clone, Debug, PartialEq)]
pub struct DockDropQuery {
    /// Source surface, when explicitly bound to a workspace surface.
    pub source_surface: Option<DockSurfaceId>,
    /// Destination surface, when explicitly bound to a workspace surface.
    pub target_surface: Option<DockSurfaceId>,
    /// Original group containing the dragged panel.
    pub source: DockNodeId,
    /// Application panel identity.
    pub panel: Key,
    /// Proposed tab insertion or adjacent split.
    pub target: DockDropTarget,
}

pub(crate) type DockDropPolicy = Rc<dyn Fn(&DockDropQuery) -> bool>;
