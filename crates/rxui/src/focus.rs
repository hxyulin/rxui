//! Placement-local focus requests, remembered groups and keyboard traversal.
use super::*;
use std::{
    cell::RefCell,
    error::Error,
    fmt,
    rc::{Rc, Weak},
};

/// Keyboard traversal at a focus group boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusScope {
    /// Tab may leave the group. Re-entry restores its last eligible focus target.
    #[default]
    Group,
    /// Tab/Shift-Tab cycle within the innermost containing scope. Pointer focus
    /// remains unrestricted; this alone does not implement a modal dialog.
    Cycle,
}
#[derive(Default, Clone)]
pub(crate) struct Properties {
    pub handle: Option<FocusHandle>,
    pub scope: Option<FocusScope>,
    pub tab_stop: Option<bool>,
}
/// Failure to resolve or use a weak focus placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusError {
    /// The context has no source placement containing this handle.
    NoPlacement,
    /// The bound element/window was removed or replaced.
    Disposed,
    /// The mutation context belongs to another runtime.
    WrongRuntime,
}
impl fmt::Display for FocusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoPlacement => "focus handle has no source placement",
            Self::Disposed => "focus placement was disposed",
            Self::WrongRuntime => "focus placement belongs to another runtime",
        })
    }
}
impl Error for FocusError {}
struct Binding {
    target: ElementId,
    scopes: Rc<Vec<crate::MountId>>,
    life: Weak<()>,
    commands: Weak<RefCell<Vec<Command>>>,
}
/// Cloneable focus reference bound with [`Element::focus_handle`]. A container
/// restores its last eligible descendant, falling back to its first focus target.
/// Sharing a handle across windows preserves independent focus and memory.
#[derive(Clone, Default)]
pub struct FocusHandle(Rc<RefCell<HashMap<u64, Binding>>>);
impl fmt::Debug for FocusHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusHandle").finish_non_exhaustive()
    }
}
impl FocusHandle {
    /// Creates an unbound reference. Bind at most once in each Ui placement.
    pub fn new() -> Self {
        Self::default()
    }
    fn id(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    /// Resolves a weak placement inside a source listener/view context. Capture
    /// this capability for later async completions; initialization has no source.
    pub fn placement(&self, cx: &impl crate::ReadContext) -> Result<FocusPlacement, FocusError> {
        let scope = cx.placement_scope().ok_or(FocusError::NoPlacement)?;
        let data = self.0.borrow();
        let (&tree, binding) = data
            .iter()
            .find(|(_, b)| b.life.strong_count() != 0 && b.scopes.contains(&scope))
            .ok_or(FocusError::NoPlacement)?;
        Ok(FocusPlacement {
            handle: self.clone(),
            tree,
            target: binding.target,
            runtime: scope.runtime,
        })
    }
    /// Queues focus restoration in the listener's source window. Applied during
    /// Ui preparation; hidden, inert or disabled targets never acquire focus.
    pub fn focus(&self, cx: &mut crate::AppContext<'_>) -> Result<(), FocusError> {
        self.placement(cx)?.focus(cx)
    }
    /// Queues blur only if focus is currently inside this bound element.
    pub fn blur(&self, cx: &mut crate::AppContext<'_>) -> Result<(), FocusError> {
        self.placement(cx)?.blur(cx)
    }
}
/// Weak focus command destination. It does not retain an element or window and
/// cannot address a replacement that happens to reuse the same description key.
#[derive(Clone, Debug)]
pub struct FocusPlacement {
    handle: FocusHandle,
    tree: u64,
    target: ElementId,
    runtime: u64,
}
impl FocusPlacement {
    /// Queues remembered/first eligible focus in this explicit placement.
    pub fn focus(&self, cx: &mut crate::AppContext<'_>) -> Result<(), FocusError> {
        self.command(cx, false)
    }
    /// Queues blur if this placement currently contains focus.
    pub fn blur(&self, cx: &mut crate::AppContext<'_>) -> Result<(), FocusError> {
        self.command(cx, true)
    }
    fn command(&self, cx: &mut crate::AppContext<'_>, blur: bool) -> Result<(), FocusError> {
        if cx.runtime.id != self.runtime {
            return Err(FocusError::WrongRuntime);
        }
        let data = self.handle.0.borrow();
        let binding = data
            .get(&self.tree)
            .filter(|b| b.target == self.target && b.life.strong_count() != 0)
            .ok_or(FocusError::Disposed)?;
        binding
            .commands
            .upgrade()
            .ok_or(FocusError::Disposed)?
            .borrow_mut()
            .push(Command {
                target: self.target,
                blur,
            });
        Ok(())
    }
}
struct Command {
    target: ElementId,
    blur: bool,
}
#[derive(Default)]
pub(super) struct State {
    pub drop_focus: Option<Box<(ElementId, Key)>>,
    pub nodes: HashSet<ElementId>,
    life: Rc<()>,
    commands: Rc<RefCell<Vec<Command>>>,
    handles: HashMap<usize, (FocusHandle, ElementId)>,
    remembered: HashMap<ElementId, ElementId>,
    tree: Option<u64>,
    revision: Option<u64>,
}
impl State {
    pub(super) fn pending(&self) -> bool {
        !self.commands.borrow().is_empty() || self.drop_focus.is_some()
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(tree) = self.tree {
            for (handle, _) in self.handles.values() {
                handle.0.borrow_mut().remove(&tree);
            }
        }
    }
}
#[derive(Clone)]
pub(super) struct TabAnchor {
    root: ElementId,
    key: Key,
    panel: bool,
}
impl<T: View> Ui<T> {
    pub(super) fn register_focus_node(&mut self, id: ElementId) {
        if self.nodes[&id]
            .element
            .input
            .as_ref()
            .is_some_and(|p| p.focus.is_some() || p.tabs.is_some())
        {
            self.focus_state
                .get_or_insert_with(Default::default)
                .nodes
                .insert(id);
        }
    }
    fn focus_properties(&self, id: ElementId) -> Option<&Properties> {
        self.nodes
            .get(&id)?
            .element
            .input
            .as_ref()?
            .focus
            .as_deref()
    }
    pub(super) fn within(&self, mut id: ElementId, root: ElementId) -> bool {
        loop {
            if id == root {
                return self.nodes.contains_key(&id);
            }
            let Some(parent) = self.nodes.get(&id).and_then(|n| n.parent) else {
                return false;
            };
            id = parent;
        }
    }
    pub(super) fn tab_stop(&self, id: ElementId) -> bool {
        if !self.enabled(id) {
            return false;
        }
        if let Some(value) = self.focus_properties(id).and_then(|p| p.tab_stop) {
            return value;
        }
        if matches!(
            self.tab_properties(id),
            Some(crate::tabs::Properties::Panel { .. })
        ) {
            return !self.order.iter().any(|child| {
                *child != id
                    && self.within(*child, id)
                    && self.enabled(*child)
                    && self.focus_properties(*child).and_then(|p| p.tab_stop) != Some(false)
            });
        }
        true
    }
    pub(super) fn remember_focus(&mut self, next: Option<ElementId>) {
        let Some(mut id) = next else {
            return;
        };
        let mut groups = Vec::new();
        loop {
            if self
                .focus_properties(id)
                .is_some_and(|p| p.scope.is_some() || p.handle.is_some())
            {
                groups.push(id);
            }
            let Some(parent) = self.nodes.get(&id).and_then(|n| n.parent) else {
                break;
            };
            id = parent;
        }
        if let Some(state) = &mut self.focus_state {
            for group in groups {
                state.remembered.insert(group, next.unwrap());
            }
        }
    }
    pub(super) fn group_target(
        &self,
        root: ElementId,
        reverse: bool,
        tab_only: bool,
    ) -> Option<ElementId> {
        if let Some(last) = self
            .focus_state
            .as_ref()
            .and_then(|s| s.remembered.get(&root))
            && self.within(*last, root)
            && if tab_only {
                self.tab_stop(*last)
            } else {
                self.enabled(*last)
            }
        {
            return Some(*last);
        }
        let eligible = |id: &ElementId| {
            *id != root
                && self.within(*id, root)
                && if tab_only {
                    self.tab_stop(*id)
                } else {
                    self.enabled(*id)
                }
        };
        let child = if reverse {
            self.order.iter().rev().find(|id| eligible(id))
        } else {
            self.order.iter().find(|id| eligible(id))
        };
        child.copied().or_else(|| {
            (if tab_only {
                self.tab_stop(root)
            } else {
                self.enabled(root)
            })
            .then_some(root)
        })
    }
    /// Focuses one eligible retained element in a custom host, revealing its scroll
    /// ancestors. Foreign, hidden, removed and disabled identities return false.
    pub fn focus(&mut self, id: ElementId) -> bool {
        if !self.is_prepared() || !self.enabled(id) {
            return false;
        }
        let changed = self.focused != Some(id);
        self.change_focus(Some(id));
        changed | self.reveal(id)
    }
    pub(super) fn scoped_focus_next(&mut self, reverse: bool) -> bool {
        if !self.is_prepared() {
            return false;
        }
        let mut current = self.focused;
        let mut cycle = None;
        while let Some(id) = current {
            if self.focus_properties(id).and_then(|p| p.scope) == Some(FocusScope::Cycle) {
                cycle = Some(id);
                break;
            }
            current = self.nodes.get(&id).and_then(|n| n.parent);
        }
        let eligible =
            |id: &ElementId| self.tab_stop(*id) && cycle.is_none_or(|root| self.within(*id, root));
        let candidates: Vec<_> = self
            .order
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, id)| eligible(id))
            .collect();
        let order_index = self
            .focused
            .and_then(|id| self.order.iter().position(|other| *other == id));
        let next = if reverse {
            candidates
                .iter()
                .rev()
                .find(|(index, _)| order_index.is_some_and(|i| *index < i))
                .or_else(|| candidates.last())
        } else {
            candidates
                .iter()
                .find(|(index, _)| order_index.is_some_and(|i| *index > i))
                .or_else(|| candidates.first())
        };
        let mut next = next.map(|(_, id)| *id);
        if let Some(candidate) = next {
            let mut group = Some(candidate);
            while let Some(id) = group {
                if self.focus_properties(id).is_some_and(|p| p.scope.is_some())
                    && self.focused.is_none_or(|focus| !self.within(focus, id))
                {
                    next = self.group_target(id, reverse, true).or(next);
                    break;
                }
                group = self.nodes[&id].parent;
            }
        }
        let changed = self.focused != next;
        self.change_focus(next);
        changed | next.is_some_and(|id| self.reveal(id))
    }
    pub(super) fn publish_focus(&mut self) -> Result<(), UiError> {
        let Some(state) = &self.focus_state else {
            return Ok(());
        };
        if state.revision == Some(self.stats.component_evaluations) {
            return Ok(());
        }
        let scopes = Rc::new(self.mount_ids().collect::<Vec<_>>());
        let ids: Vec<_> = state.nodes.iter().copied().collect();
        let mut next = HashMap::new();
        for id in ids {
            if let Some(handle) = self.focus_properties(id).and_then(|p| p.handle.as_ref())
                && next.insert(handle.id(), (handle.clone(), id)).is_some()
            {
                return Err(UiError::DuplicateFocusHandle);
            }
        }
        let state = self.focus_state.as_mut().unwrap();
        state.nodes.retain(|id| self.nodes.contains_key(id));
        state
            .remembered
            .retain(|root, last| self.nodes.contains_key(root) && self.nodes.contains_key(last));
        for (key, (handle, _)) in &state.handles {
            if !next.contains_key(key) {
                handle.0.borrow_mut().remove(&self.tree);
            }
        }
        for (handle, target) in next.values() {
            handle.0.borrow_mut().insert(
                self.tree,
                Binding {
                    target: *target,
                    scopes: scopes.clone(),
                    life: Rc::downgrade(&state.life),
                    commands: Rc::downgrade(&state.commands),
                },
            );
        }
        state.handles = next;
        state.tree = Some(self.tree);
        state.revision = Some(self.stats.component_evaluations);
        Ok(())
    }
    pub(super) fn apply_focus_commands(&mut self) {
        let commands = self
            .focus_state
            .as_ref()
            .map(|s| std::mem::take(&mut *s.commands.borrow_mut()))
            .unwrap_or_default();
        for command in commands {
            if !self.nodes.contains_key(&command.target) {
                continue;
            }
            if command.blur {
                if self
                    .focused
                    .is_some_and(|id| self.within(id, command.target))
                {
                    self.change_focus(None);
                }
            } else if let Some(next) = self.group_target(command.target, false, false) {
                self.change_focus(Some(next));
                self.reveal(next);
            }
        }
    }
    pub(super) fn tab_anchor(&self) -> Option<TabAnchor> {
        let mut id = self.focused?;
        let mut source = None;
        loop {
            match self.tab_properties(id) {
                Some(crate::tabs::Properties::Panel { key }) => source = Some((key.clone(), true)),
                Some(crate::tabs::Properties::Close { key })
                | Some(crate::tabs::Properties::Header { key, .. }) => {
                    source = Some((key.clone(), false))
                }
                Some(crate::tabs::Properties::Root { .. }) => {
                    let (key, panel) = source?;
                    return Some(TabAnchor {
                        root: id,
                        key,
                        panel,
                    });
                }
                _ => {}
            }
            id = self.nodes.get(&id)?.parent?;
        }
    }
    pub(super) fn restore_tab_focus(&mut self, anchor: Option<TabAnchor>) {
        let Some(anchor) = anchor else {
            return;
        };
        let Some(crate::tabs::Properties::Root { selected, .. }) = self.tab_properties(anchor.root)
        else {
            return;
        };
        let selected = selected.clone();
        if self.focused.is_some_and(|id| self.enabled(id))
            && (!anchor.panel || selected.as_ref() == Some(&anchor.key))
        {
            return;
        }
        let target = self.order.iter().copied().find(|id| {
            self.within(*id, anchor.root)
                && match self.tab_properties(*id) {
                    Some(crate::tabs::Properties::Panel { key }) if anchor.panel => {
                        selected.as_ref() == Some(key)
                    }
                    Some(crate::tabs::Properties::Header { key, .. }) if !anchor.panel => {
                        selected.as_ref() == Some(key) && self.enabled(*id)
                    }
                    _ => false,
                }
        });
        let next = target
            .and_then(|id| {
                if anchor.panel {
                    self.group_target(id, false, false)
                } else {
                    Some(id)
                }
            })
            .or_else(|| {
                self.order.iter().copied().find(|id| {
                    self.within(*id, anchor.root)
                        && matches!(
                            self.tab_properties(*id),
                            Some(crate::tabs::Properties::Header { .. })
                        )
                        && self.enabled(*id)
                })
            })
            .or_else(|| {
                self.order
                    .iter()
                    .copied()
                    .find(|id| !self.within(*id, anchor.root) && self.tab_stop(*id))
            });
        self.change_focus(next);
        if let Some(id) = next {
            self.reveal(id);
        }
    }
}
