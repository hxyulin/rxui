//! Application-owned docking surfaces, independent of regions and native windows.
use crate::{
    DockDropTarget, DockError, DockEvent, DockNode, DockNodeId, DockSide, DockTree, Key,
    SplitPosition,
};
use std::{error::Error, fmt, sync::atomic::AtomicU64};

/// Stable opaque surface identity. Removed identities are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DockSurfaceId(u64);
impl DockSurfaceId {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(crate::id::next_identity(&NEXT))
    }
}

/// A failed workspace edit leaves all surfaces unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockWorkspaceError {
    /// A surface was removed or belongs to another workspace snapshot.
    SurfaceNotFound,
    /// A panel key already belongs to a surface in this workspace.
    DuplicatePanel,
    /// A checked tree edit failed, including stale source membership.
    Tree(DockError),
}
impl fmt::Display for DockWorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SurfaceNotFound => f.write_str("dock surface not found"),
            Self::DuplicatePanel => f.write_str("panel already belongs to the dock workspace"),
            Self::Tree(error) => error.fmt(f),
        }
    }
}
impl Error for DockWorkspaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Tree(error) => Some(error),
            _ => None,
        }
    }
}
impl From<DockError> for DockWorkspaceError {
    fn from(error: DockError) -> Self {
        Self::Tree(error)
    }
}

/// Accepted transfer details for updating application surface/window placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockTransfer {
    /// Original surface; it remains in the workspace, even when empty.
    pub source_surface: DockSurfaceId,
    /// Original group; it may have collapsed during the transfer.
    pub source_group: DockNodeId,
    /// Receiving surface, newly allocated for a detach operation.
    pub destination_surface: DockSurfaceId,
    /// Receiving group, including the new group created by a split.
    pub destination_group: DockNodeId,
    /// Whether panel ordering, selection or topology changed.
    pub changed: bool,
    /// Whether the original surface is empty after the transfer.
    pub source_empty: bool,
}

/// Collection of docking trees with workspace-wide unique application panel keys.
///
/// A surface may represent a region in one window or a separate window. The
/// application owns that association and all document/entity lifetimes. Trees
/// are exposed read-only so checked edits cannot bypass panel-key uniqueness.
/// Empty source surfaces remain until the application explicitly removes them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DockWorkspace {
    surfaces: Vec<(DockSurfaceId, DockTree)>,
}
impl DockWorkspace {
    /// Creates a workspace without surfaces.
    pub fn new() -> Self {
        Self::default()
    }
    /// Number of surfaces, including empty surfaces.
    pub fn len(&self) -> usize {
        self.surfaces.len()
    }
    /// Whether this workspace has no surfaces.
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }
    /// Read-only surfaces in creation order. Node IDs are scoped by surface.
    pub fn surfaces(
        &self,
    ) -> impl ExactSizeIterator<Item = (DockSurfaceId, &DockTree)> + DoubleEndedIterator + '_ {
        self.surfaces.iter().map(|(id, tree)| (*id, tree))
    }
    /// Read-only topology for a live surface.
    pub fn surface(&self, surface: DockSurfaceId) -> Option<&DockTree> {
        self.surfaces
            .iter()
            .find(|(id, _)| *id == surface)
            .map(|(_, tree)| tree)
    }
    fn tree(&self, surface: DockSurfaceId) -> Result<&DockTree, DockWorkspaceError> {
        self.surface(surface)
            .ok_or(DockWorkspaceError::SurfaceNotFound)
    }
    fn tree_mut(&mut self, surface: DockSurfaceId) -> Result<&mut DockTree, DockWorkspaceError> {
        self.surfaces
            .iter_mut()
            .find(|(id, _)| *id == surface)
            .map(|(_, tree)| tree)
            .ok_or(DockWorkspaceError::SurfaceNotFound)
    }
    /// Finds the surface containing a panel key.
    pub fn surface_for(&self, panel: &Key) -> Option<DockSurfaceId> {
        self.surfaces()
            .find_map(|(id, tree)| tree.group_for(panel).map(|_| id))
    }
    fn check_new_panel(&self, panel: &Key) -> Result<(), DockWorkspaceError> {
        if self.surface_for(panel).is_some() {
            Err(DockWorkspaceError::DuplicatePanel)
        } else {
            Ok(())
        }
    }
    /// Adds an owned tree if none of its panel keys already belong to a surface.
    /// Use `DockTree::new()` to create an empty destination region/window model.
    pub fn create_surface(&mut self, tree: DockTree) -> Result<DockSurfaceId, DockWorkspaceError> {
        fn check(workspace: &DockWorkspace, node: &DockNode) -> Result<(), DockWorkspaceError> {
            match node {
                DockNode::Tabs(tabs) => {
                    for panel in tabs.panels() {
                        workspace.check_new_panel(panel)?;
                    }
                }
                DockNode::Split(split) => {
                    check(workspace, split.first())?;
                    check(workspace, split.second())?;
                }
            }
            Ok(())
        }
        check(self, tree.root())?;
        let id = DockSurfaceId::new();
        self.surfaces.push((id, tree));
        Ok(id)
    }
    /// Removes a surface and returns its owned tree, including all panel keys.
    /// This does not discard documents, destroy entities or close native windows.
    pub fn remove_surface(
        &mut self,
        surface: DockSurfaceId,
    ) -> Result<DockTree, DockWorkspaceError> {
        let index = self
            .surfaces
            .iter()
            .position(|(id, _)| *id == surface)
            .ok_or(DockWorkspaceError::SurfaceNotFound)?;
        Ok(self.surfaces.remove(index).1)
    }
    /// Accepts an existing tree proposal, retaining its stale-source checks.
    /// Drop targets refer to groups in this surface; use `transfer` across surfaces.
    pub fn apply(
        &mut self,
        surface: DockSurfaceId,
        event: &DockEvent,
    ) -> Result<bool, DockWorkspaceError> {
        Ok(self.tree_mut(surface)?.apply(event)?)
    }
    /// Selects a panel in its expected group; returns whether selection changed.
    pub fn select(
        &mut self,
        surface: DockSurfaceId,
        group: DockNodeId,
        panel: &Key,
    ) -> Result<bool, DockWorkspaceError> {
        Ok(self.tree_mut(surface)?.select(group, panel)?)
    }
    /// Inserts a new workspace-wide unique panel, preserving existing selection.
    pub fn insert(
        &mut self,
        surface: DockSurfaceId,
        group: DockNodeId,
        index: usize,
        panel: impl Into<Key>,
    ) -> Result<(), DockWorkspaceError> {
        self.tree(surface)?;
        let panel = panel.into();
        self.check_new_panel(&panel)?;
        Ok(self.tree_mut(surface)?.insert(group, index, panel)?)
    }
    /// Splits a group with a new workspace-wide unique panel.
    pub fn split(
        &mut self,
        surface: DockSurfaceId,
        group: DockNodeId,
        side: DockSide,
        panel: impl Into<Key>,
        position: SplitPosition,
    ) -> Result<DockNodeId, DockWorkspaceError> {
        self.tree(surface)?;
        let panel = panel.into();
        self.check_new_panel(&panel)?;
        Ok(self
            .tree_mut(surface)?
            .split(group, side, panel, position)?)
    }
    /// Atomically transfers an existing panel after checking its original group.
    /// Tab indices are final indices after source removal, as in `DockDropTarget`.
    /// The destination selects the moved panel. Empty source branches collapse;
    /// empty source surfaces remain available for application-owned cleanup.
    pub fn transfer(
        &mut self,
        source_surface: DockSurfaceId,
        source_group: DockNodeId,
        panel: &Key,
        destination_surface: DockSurfaceId,
        target: DockDropTarget,
    ) -> Result<DockTransfer, DockWorkspaceError> {
        self.tree(source_surface)?;
        self.tree(destination_surface)?;
        let (destination_group, changed) = if source_surface == destination_surface {
            let changed = self.tree_mut(source_surface)?.apply(&DockEvent::Drop {
                source: source_group,
                panel: panel.clone(),
                target,
            })?;
            (
                self.tree(source_surface)?
                    .group_for(panel)
                    .expect("transferred panel"),
                changed,
            )
        } else {
            let mut source = self.tree(source_surface)?.clone();
            let mut destination = self.tree(destination_surface)?.clone();
            let group =
                transfer_between(&mut source, source_group, panel, &mut destination, target)?;
            *self.tree_mut(source_surface)? = source;
            *self.tree_mut(destination_surface)? = destination;
            (group, true)
        };
        Ok(DockTransfer {
            source_surface,
            source_group,
            destination_surface,
            destination_group,
            changed,
            source_empty: tree_empty(self.tree(source_surface)?),
        })
    }
    /// Atomically moves a panel into a newly created surface, selecting it there.
    /// The application can place the new surface in another region or window.
    /// Reattach using `transfer`, then optionally `remove_surface` on the empty one.
    pub fn detach(
        &mut self,
        source_surface: DockSurfaceId,
        source_group: DockNodeId,
        panel: &Key,
    ) -> Result<DockTransfer, DockWorkspaceError> {
        let mut source = self.tree(source_surface)?.clone();
        let mut destination = DockTree::new();
        let group = destination.root().id();
        transfer_between(
            &mut source,
            source_group,
            panel,
            &mut destination,
            DockDropTarget::Tab { group, index: 0 },
        )?;
        let source_empty = tree_empty(&source);
        let destination_surface = DockSurfaceId::new();
        *self.tree_mut(source_surface)? = source;
        self.surfaces.push((destination_surface, destination));
        Ok(DockTransfer {
            source_surface,
            source_group,
            destination_surface,
            destination_group: group,
            changed: true,
            source_empty,
        })
    }
}
fn tree_empty(tree: &DockTree) -> bool {
    tree.root()
        .tabs()
        .is_some_and(|tabs| tabs.panels().is_empty())
}
// Operates only on unpublished snapshots. No generic mutable-tree escape hatch.
fn transfer_between(
    source: &mut DockTree,
    source_group: DockNodeId,
    panel: &Key,
    destination: &mut DockTree,
    target: DockDropTarget,
) -> Result<DockNodeId, DockError> {
    source.apply(&DockEvent::Close {
        group: source_group,
        panel: panel.clone(),
    })?;
    let group = match target {
        DockDropTarget::Tab { group, index } => {
            destination.insert(group, index, panel.clone())?;
            group
        }
        DockDropTarget::Split {
            group,
            side,
            position,
        } => destination.split(group, side, panel.clone(), position)?,
    };
    destination.select(group, panel)?;
    Ok(group)
}
