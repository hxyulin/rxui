//! Application-owned docking topology and checked edits, independent of UI placement.
use crate::{Axis, Key, ResizeEvent, SplitPosition};
use std::{error::Error, fmt, sync::atomic::AtomicU64};

/// Stable opaque split/group identity. Cloned tree snapshots retain identities;
/// newly created nodes never reuse removed IDs, including across independent trees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DockNodeId(u64);
impl DockNodeId {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(crate::id::next_identity(&NEXT))
    }
    pub(crate) fn key(self) -> Key {
        self.0.into()
    }
}
/// Location of a new pane relative to the target group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockSide {
    /// New pane before the target, horizontally.
    Left,
    /// New pane after the target, horizontally.
    Right,
    /// New pane before the target, vertically.
    Top,
    /// New pane after the target, vertically.
    Bottom,
}
impl DockSide {
    fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Top | Self::Bottom => Axis::Vertical,
        }
    }
    fn before(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}
/// One nonempty group, or the empty root left after the last panel is removed.
#[derive(Clone, Debug, PartialEq)]
pub struct DockTabs {
    id: DockNodeId,
    panels: Vec<Key>,
    selected: Option<Key>,
}
impl DockTabs {
    fn new(panels: Vec<Key>) -> Self {
        Self {
            id: DockNodeId::new(),
            selected: panels.first().cloned(),
            panels,
        }
    }
    /// Stable group identity used by edits and event proposals.
    pub fn id(&self) -> DockNodeId {
        self.id
    }
    /// Ordered application panel keys. Keys are unique across the entire tree.
    pub fn panels(&self) -> &[Key] {
        &self.panels
    }
    /// Active panel, absent only in the empty root group.
    pub fn selected(&self) -> Option<&Key> {
        self.selected.as_ref()
    }
}
/// Binary split. Position always describes the first child, excluding the divider.
#[derive(Clone, Debug, PartialEq)]
pub struct DockSplit {
    id: DockNodeId,
    axis: Axis,
    position: SplitPosition,
    first: Box<DockNode>,
    second: Box<DockNode>,
}
impl DockSplit {
    /// Stable split identity used by resize proposals.
    pub fn id(&self) -> DockNodeId {
        self.id
    }
    /// Horizontal (side-by-side) or vertical (stacked) allocation.
    pub fn axis(&self) -> Axis {
        self.axis
    }
    /// Application-requested first child extent.
    pub fn position(&self) -> SplitPosition {
        self.position
    }
    /// First/left/top subtree.
    pub fn first(&self) -> &DockNode {
        &self.first
    }
    /// Second/right/bottom subtree.
    pub fn second(&self) -> &DockNode {
        &self.second
    }
}
/// Read-only docking topology. Edit through DockTree to preserve invariants.
#[derive(Clone, Debug, PartialEq)]
pub enum DockNode {
    /// A tab group containing panel keys.
    Tabs(DockTabs),
    /// Two child subtrees with a controlled divider.
    Split(DockSplit),
}
impl DockNode {
    /// Stable identity of either node kind.
    pub fn id(&self) -> DockNodeId {
        match self {
            Self::Tabs(n) => n.id,
            Self::Split(n) => n.id,
        }
    }
    /// Group properties, when this node is a tab group.
    pub fn tabs(&self) -> Option<&DockTabs> {
        if let Self::Tabs(n) = self {
            Some(n)
        } else {
            None
        }
    }
    /// Split properties, when this node is a split.
    pub fn split(&self) -> Option<&DockSplit> {
        if let Self::Split(n) = self {
            Some(n)
        } else {
            None
        }
    }
    fn find(&self, id: DockNodeId) -> Option<&Self> {
        if self.id() == id {
            return Some(self);
        }
        match self {
            Self::Tabs(_) => None,
            Self::Split(n) => n.first.find(id).or_else(|| n.second.find(id)),
        }
    }
    fn find_mut(&mut self, id: DockNodeId) -> Option<&mut Self> {
        if self.id() == id {
            return Some(self);
        }
        match self {
            Self::Tabs(_) => None,
            Self::Split(n) => {
                if let Some(node) = n.first.find_mut(id) {
                    Some(node)
                } else {
                    n.second.find_mut(id)
                }
            }
        }
    }
    fn group_for(&self, key: &Key) -> Option<DockNodeId> {
        match self {
            Self::Tabs(n) => n.panels.contains(key).then_some(n.id),
            Self::Split(n) => n.first.group_for(key).or_else(|| n.second.group_for(key)),
        }
    }
    fn empty(&self) -> bool {
        matches!(self, Self::Tabs(n) if n.panels.is_empty())
    }
    fn collapse(&mut self) {
        if let Self::Split(n) = self {
            n.first.collapse();
            n.second.collapse();
            let keep = if n.first.empty() {
                Some(&mut n.second)
            } else if n.second.empty() {
                Some(&mut n.first)
            } else {
                None
            };
            if let Some(keep) = keep {
                // Move the surviving node, preserving all of its IDs. The temporary
                // placeholder is local and never observable through the public tree.
                *self = *std::mem::replace(keep, Box::new(Self::Tabs(DockTabs::new(Vec::new()))));
            }
        }
    }
}
/// Checked edit failure. Failed edits leave the tree unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockError {
    /// The node was removed, belongs to another tree, or is absent in this snapshot.
    NodeNotFound,
    /// The operation requires a tab group rather than a split.
    NotTabGroup,
    /// Resize requires a split rather than a tab group.
    NotSplit,
    /// The panel is already present somewhere in this tree.
    DuplicatePanel,
    /// The source panel is absent or a stale event names the wrong group.
    PanelNotFound,
    /// Insertion/reorder index lies outside the destination sequence.
    InvalidIndex,
    /// Nonfinite, negative or out-of-range split position.
    InvalidPosition,
    /// A group's only panel cannot be split away from that same group.
    CannotSplitSolePanel,
}
impl fmt::Display for DockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NodeNotFound => "dock node not found",
            Self::NotTabGroup => "dock operation requires a tab group",
            Self::NotSplit => "dock resize requires a split",
            Self::DuplicatePanel => "panel already belongs to the dock tree",
            Self::PanelNotFound => "panel not found in the expected dock group",
            Self::InvalidIndex => "invalid dock panel index",
            Self::InvalidPosition => "invalid dock split position",
            Self::CannotSplitSolePanel => "cannot split a group's only panel away from itself",
        })
    }
}
impl Error for DockError {}
/// Controlled UI proposal. Applications may accept with DockTree::apply, modify,
/// ignore or defer it (for example, confirming unsaved data before Close).
#[derive(Clone, Debug, PartialEq)]
pub enum DockEvent {
    /// Propose a completed header drag. No layout changes during pointer motion.
    Drop {
        /// Original group, checked before accepting a possibly deferred proposal.
        source: DockNodeId,
        /// Dragged panel key.
        panel: Key,
        /// Proposed insertion or adjacent split.
        target: DockDropTarget,
    },
    /// Propose selecting a panel in its current group.
    Select {
        /// Source group identity.
        group: DockNodeId,
        /// Proposed panel key.
        panel: Key,
    },
    /// Propose removing a panel; document/entity disposal is application-owned.
    Close {
        /// Source group identity, checked to reject stale moved-panel requests.
        group: DockNodeId,
        /// Panel whose close action was requested.
        panel: Key,
    },
    /// Propose the existing split's first child extent.
    Resize {
        /// Source split identity.
        split: DockNodeId,
        /// Ordinary split resize proposal, including gesture phase.
        event: ResizeEvent,
    },
}
/// Destination of a header drag, or an application-created docking proposal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DockDropTarget {
    /// Insert into a tab sequence. Index is the final index after source removal.
    Tab {
        /// Destination group.
        group: DockNodeId,
        /// Final destination index.
        index: usize,
    },
    /// Move into a new group beside the destination. Stock gestures propose halves.
    Split {
        /// Destination group.
        group: DockNodeId,
        /// Side of the destination for the new panel group.
        side: DockSide,
        /// Requested first/left/top extent, regardless of side.
        position: SplitPosition,
    },
}
/// Owned binary docking tree. Global panel-key uniqueness, valid selection, valid
/// split positions and collapsed empty branches are enforced by checked methods.
/// No entities, windows, callbacks or GPU resources are retained here.
///
/// ```
/// use rxui::prelude::*;
/// let mut tree = DockTree::from_panels(["editor", "preview"])?;
/// let editor = tree.root().id();
/// let output = tree.split(editor, DockSide::Bottom, "output", SplitPosition::Fraction(0.7))?;
/// tree.move_panel(&Key::from("preview"), output, 1)?;
/// # Ok::<(), DockError>(())
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct DockTree {
    root: DockNode,
}
impl Default for DockTree {
    fn default() -> Self {
        Self::new()
    }
}
impl DockTree {
    /// Creates one empty root group; insert into root().id() to add the first panel.
    pub fn new() -> Self {
        Self {
            root: DockNode::Tabs(DockTabs::new(Vec::new())),
        }
    }
    /// Creates one group, selecting its first panel. Duplicate keys are rejected.
    pub fn from_panels<K: Into<Key>>(
        panels: impl IntoIterator<Item = K>,
    ) -> Result<Self, DockError> {
        let panels: Vec<Key> = panels.into_iter().map(Into::into).collect();
        let mut seen = std::collections::HashSet::new();
        if panels.iter().any(|key| !seen.insert(key)) {
            return Err(DockError::DuplicatePanel);
        }
        Ok(Self {
            root: DockNode::Tabs(DockTabs::new(panels)),
        })
    }
    /// Read-only root. Collapsing a split can replace its ID with a surviving child.
    pub fn root(&self) -> &DockNode {
        &self.root
    }
    /// Looks up a live node; stale IDs are never reassigned to replacement nodes.
    pub fn node(&self, id: DockNodeId) -> Option<&DockNode> {
        self.root.find(id)
    }
    /// Finds the group containing the globally unique panel key.
    pub fn group_for(&self, panel: &Key) -> Option<DockNodeId> {
        self.root.group_for(panel)
    }
    fn group(&self, id: DockNodeId) -> Result<&DockTabs, DockError> {
        self.node(id)
            .ok_or(DockError::NodeNotFound)?
            .tabs()
            .ok_or(DockError::NotTabGroup)
    }
    fn group_mut(&mut self, id: DockNodeId) -> &mut DockTabs {
        match self.root.find_mut(id).expect("validated dock group") {
            DockNode::Tabs(n) => n,
            _ => unreachable!(),
        }
    }
    fn contains(&self, group: DockNodeId, panel: &Key) -> Result<(), DockError> {
        self.group(group)?
            .panels
            .contains(panel)
            .then_some(())
            .ok_or(DockError::PanelNotFound)
    }
    /// Selects a panel already in this group; returns whether selection changed.
    pub fn select(&mut self, group: DockNodeId, panel: &Key) -> Result<bool, DockError> {
        self.contains(group, panel)?;
        let n = self.group_mut(group);
        let changed = n.selected.as_ref() != Some(panel);
        n.selected = Some(panel.clone());
        Ok(changed)
    }
    /// Updates a live split without replacing any node identity.
    pub fn resize(
        &mut self,
        split: DockNodeId,
        position: SplitPosition,
    ) -> Result<bool, DockError> {
        if !position.valid() {
            return Err(DockError::InvalidPosition);
        }
        let node = self.root.find_mut(split).ok_or(DockError::NodeNotFound)?;
        let DockNode::Split(n) = node else {
            return Err(DockError::NotSplit);
        };
        let changed = n.position != position;
        n.position = position;
        Ok(changed)
    }
    /// Inserts a new globally unique panel at 0..=len. Existing selection remains;
    /// insertion into the empty root selects the inserted panel.
    pub fn insert(
        &mut self,
        group: DockNodeId,
        index: usize,
        panel: impl Into<Key>,
    ) -> Result<(), DockError> {
        let panel = panel.into();
        if index > self.group(group)?.panels.len() {
            return Err(DockError::InvalidIndex);
        }
        if self.group_for(&panel).is_some() {
            return Err(DockError::DuplicatePanel);
        }
        let n = self.group_mut(group);
        if n.selected.is_none() {
            n.selected = Some(panel.clone());
        }
        n.panels.insert(index, panel);
        Ok(())
    }
    fn take(&mut self, group: DockNodeId, panel: &Key) -> Key {
        let n = self.group_mut(group);
        let index = n.panels.iter().position(|key| key == panel).unwrap();
        let key = n.panels.remove(index);
        if n.selected.as_ref() == Some(panel) {
            n.selected = n.panels.get(index).or_else(|| n.panels.last()).cloned();
        }
        key
    }
    /// Removes a panel, selecting its following neighbor then preceding neighbor.
    /// Empty branches collapse; removing the last panel leaves one empty root group.
    /// Returns false for an absent panel. Strong document entities remain untouched.
    pub fn remove(&mut self, panel: &Key) -> bool {
        let Some(group) = self.group_for(panel) else {
            return false;
        };
        self.take(group, panel);
        self.root.collapse();
        true
    }
    /// Reorders or moves an existing panel, selecting it in the destination.
    /// Index is the final index after source removal (0..len for same-group reorder,
    /// 0..=len for a different group). Empty source branches collapse. Changing
    /// structural parent does not transfer retained caret/scroll/widget placement.
    pub fn move_panel(
        &mut self,
        panel: &Key,
        target: DockNodeId,
        index: usize,
    ) -> Result<bool, DockError> {
        let source = self.group_for(panel).ok_or(DockError::PanelNotFound)?;
        let len = self.group(target)?.panels.len() - usize::from(source == target);
        if index > len {
            return Err(DockError::InvalidIndex);
        }
        if source == target {
            let n = self.group_mut(source);
            let old = n.panels.iter().position(|k| k == panel).unwrap();
            let changed = old != index || n.selected.as_ref() != Some(panel);
            let key = n.panels.remove(old);
            n.panels.insert(index, key);
            n.selected = Some(panel.clone());
            return Ok(changed);
        }
        let panel = self.take(source, panel);
        let n = self.group_mut(target);
        n.panels.insert(index, panel.clone());
        n.selected = Some(panel);
        self.root.collapse();
        Ok(true)
    }
    fn wrap(
        &mut self,
        target: DockNodeId,
        side: DockSide,
        panel: Key,
        position: SplitPosition,
    ) -> DockNodeId {
        let new = DockTabs::new(vec![panel]);
        let id = new.id;
        let node = self.root.find_mut(target).unwrap();
        let old = std::mem::replace(node, DockNode::Tabs(new));
        let new = std::mem::replace(node, DockNode::Tabs(DockTabs::new(Vec::new())));
        let (first, second) = if side.before() {
            (new, old)
        } else {
            (old, new)
        };
        *node = DockNode::Split(DockSplit {
            id: DockNodeId::new(),
            axis: side.axis(),
            position,
            first: Box::new(first),
            second: Box::new(second),
        });
        id
    }
    fn check_split(&self, target: DockNodeId, position: SplitPosition) -> Result<(), DockError> {
        if !position.valid() {
            return Err(DockError::InvalidPosition);
        }
        self.group(target)?;
        Ok(())
    }
    /// Splits a group with a new unique panel, returning the new group's ID.
    /// Position describes the first/left/top child regardless of side. Splitting
    /// an empty root simply inserts the panel into that root without a divider.
    pub fn split(
        &mut self,
        target: DockNodeId,
        side: DockSide,
        panel: impl Into<Key>,
        position: SplitPosition,
    ) -> Result<DockNodeId, DockError> {
        self.check_split(target, position)?;
        let panel = panel.into();
        if self.group_for(&panel).is_some() {
            return Err(DockError::DuplicatePanel);
        }
        if self.group(target)?.panels.is_empty() {
            self.insert(target, 0, panel)?;
            return Ok(target);
        }
        Ok(self.wrap(target, side, panel, position))
    }
    /// Moves an existing panel into a new group alongside target. Splitting a
    /// panel away from its own group requires another panel to remain there.
    pub fn dock_panel(
        &mut self,
        panel: &Key,
        target: DockNodeId,
        side: DockSide,
        position: SplitPosition,
    ) -> Result<DockNodeId, DockError> {
        self.check_split(target, position)?;
        let source = self.group_for(panel).ok_or(DockError::PanelNotFound)?;
        if source == target && self.group(source)?.panels.len() == 1 {
            return Err(DockError::CannotSplitSolePanel);
        }
        let panel = self.take(source, panel);
        self.root.collapse();
        Ok(self.wrap(target, side, panel, position))
    }
    /// Accepts a current proposal. Close and Drop check original source membership
    /// before editing, so deferred requests cannot act on a panel moved elsewhere.
    pub fn apply(&mut self, event: &DockEvent) -> Result<bool, DockError> {
        match event {
            DockEvent::Drop {
                source,
                panel,
                target,
            } => {
                self.contains(*source, panel)?;
                match *target {
                    DockDropTarget::Tab { group, index } => self.move_panel(panel, group, index),
                    DockDropTarget::Split {
                        group,
                        side,
                        position,
                    } => {
                        self.dock_panel(panel, group, side, position)?;
                        Ok(true)
                    }
                }
            }
            DockEvent::Select { group, panel } => self.select(*group, panel),
            DockEvent::Close { group, panel } => {
                self.contains(*group, panel)?;
                Ok(self.remove(panel))
            }
            DockEvent::Resize { split, event } => self.resize(*split, event.position),
        }
    }
}
