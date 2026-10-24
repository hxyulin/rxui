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
    /// A roving group, such as a toolbar: Tab enters it once and arrow keys
    /// along this axis, Home and End move between its Tab stops.
    pub roving: Option<crate::Axis>,
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
    /// Queues focus in this handle's only live binding, without a source mount.
    /// Stock controls create a fresh handle per description, so it has one
    /// placement; their internal listeners run outside any component scope.
    pub(crate) fn focus_bound(&self, cx: &mut crate::AppContext<'_>) -> bool {
        let data = self.0.borrow();
        let mut live = data.values().filter(|b| {
            b.life.strong_count() != 0
                && b.scopes.first().is_some_and(|s| s.runtime == cx.runtime.id)
        });
        let (Some(binding), None) = (live.next(), live.next()) else {
            return false;
        };
        let Some(commands) = binding.commands.upgrade() else {
            return false;
        };
        commands.borrow_mut().push(Command {
            target: binding.target,
            blur: false,
        });
        true
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
    notifications: Vec<(crate::Listener<crate::FocusEvent>, crate::FocusEvent)>,
    path: Vec<(ElementId, crate::Listener<crate::FocusEvent>)>,
    keyboard: bool,
    pub drop_focus: Option<Box<(ElementId, Key)>>,
    pub nodes: IdSet<ElementId>,
    life: Rc<()>,
    commands: Rc<RefCell<Vec<Command>>>,
    handles: HashMap<usize, (FocusHandle, ElementId)>,
    remembered: IdMap<ElementId, ElementId>,
    tree: Option<u64>,
    revision: Option<u64>,
}
impl State {
    pub(super) fn pending(&self) -> bool {
        !self.commands.borrow().is_empty()
            || self.drop_focus.is_some()
            || !self.notifications.is_empty()
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
    pub(super) fn queue_focus_notifications(&mut self) {
        let mut path = Vec::new();
        let mut current = self.focused.filter(|_| self.active);
        while let Some(id) = current {
            let Some(node) = self.nodes.get(&id) else {
                break;
            };
            if let Some(listener) = node
                .element
                .input
                .as_ref()
                .and_then(|p| p.focus_change.clone())
            {
                path.push((id, listener));
            }
            current = node.parent;
        }
        path.reverse();
        if path.is_empty() && self.focus_state.is_none() {
            return;
        }
        let state = self.focus_state.get_or_insert_with(Default::default);
        for (id, listener) in state.path.iter().rev() {
            if !path.iter().any(|(next, _)| id == next) {
                state.notifications.push((
                    listener.clone(),
                    crate::FocusEvent {
                        target: *id,
                        focused: false,
                        keyboard: self.keyboard_modality,
                    },
                ));
            }
        }
        for (id, listener) in &path {
            if state.keyboard != self.keyboard_modality
                || !state.path.iter().any(|(old, _)| id == old)
            {
                state.notifications.push((
                    listener.clone(),
                    crate::FocusEvent {
                        target: *id,
                        focused: true,
                        keyboard: self.keyboard_modality,
                    },
                ));
            }
        }
        state.path = path;
        state.keyboard = self.keyboard_modality;
    }
    pub(super) fn flush_focus_notifications(
        &mut self,
        runtime: &mut Runtime,
    ) -> Result<bool, UiError> {
        let pending = self
            .focus_state
            .as_mut()
            .map(|s| std::mem::take(&mut s.notifications))
            .unwrap_or_default();
        let mut changed = false;
        for (listener, event) in pending {
            changed |= runtime.update(|cx| listener.dispatch(&event, cx))? == Dispatch::Handled;
        }
        Ok(changed)
    }
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
        // Each link of focused rich text is a stop before traversal leaves it.
        if let Some(changed) = self.step_link_focus(reverse) {
            return changed;
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
        // Tab leaves a roving group from its focused item in one step.
        let roving = self
            .focused
            .and_then(|id| self.roving_root(id))
            .map(|(root, _)| root);
        let eligible = |id: &ElementId| {
            self.tab_stop(*id)
                && cycle.is_none_or(|root| self.within(*id, root))
                && roving.is_none_or(|root| Some(*id) == self.focused || !self.within(*id, root))
        };
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
        // Entering rich text focuses and reveals its first link, or its last
        // moving backward.
        if next.is_some_and(|id| self.rich_links(id).is_some()) {
            return self.enter_links(reverse) | changed;
        }
        changed | next.is_some_and(|id| self.reveal(id))
    }
    /// The nearest roving group containing `id`, and its arrow-key axis. A
    /// popover's content belongs to no group outside the popover.
    fn roving_root(&self, mut id: ElementId) -> Option<(ElementId, crate::Axis)> {
        loop {
            if let Some(axis) = self.focus_properties(id).and_then(|p| p.roving) {
                return Some((id, axis));
            }
            if self.is_overlay(id) {
                return None;
            }
            id = self.nodes.get(&id)?.parent?;
        }
    }
    /// Arrow-key orientation published for a roving group's root.
    pub(super) fn roving_axis(&self, id: ElementId) -> Option<crate::Axis> {
        self.focus_properties(id).and_then(|p| p.roving)
    }
    /// Moves focus between the Tab stops of the roving group around `target`:
    /// the previous or next one along its axis, wrapping, or the first or last
    /// with Home and End. Text inputs keep their arrow keys.
    pub(super) fn roving_key(
        &mut self,
        target: ElementId,
        event: &crate::KeyEvent,
    ) -> Option<bool> {
        use crate::KeyboardKey;
        if event.modifiers != crate::Modifiers::default()
            || matches!(
                self.nodes[&target].element.kind,
                ElementKind::TextInput { .. }
            )
        {
            return None;
        }
        let (root, axis) = self.roving_root(target)?;
        let horizontal = axis == crate::Axis::Horizontal;
        let forward = match event.key {
            KeyboardKey::Home => None,
            KeyboardKey::End => Some(None),
            KeyboardKey::ArrowLeft if horizontal => Some(Some(false)),
            KeyboardKey::ArrowRight if horizontal => Some(Some(true)),
            KeyboardKey::ArrowUp if !horizontal => Some(Some(false)),
            KeyboardKey::ArrowDown if !horizontal => Some(Some(true)),
            _ => return None,
        };
        let stops: Vec<_> = self
            .order
            .iter()
            .enumerate()
            .filter(|(_, id)| **id != root && self.within(**id, root) && self.tab_stop(**id))
            .map(|(index, id)| (index, *id))
            .collect();
        let (first, last) = (stops.first()?.1, stops.last()?.1);
        let at = self.order.iter().position(|id| *id == target)?;
        let next = match forward {
            None => first,
            Some(None) => last,
            Some(Some(true)) => stops.iter().find(|(i, _)| *i > at).map_or(first, |s| s.1),
            Some(Some(false)) => stops
                .iter()
                .rev()
                .find(|(i, _)| *i < at)
                .map_or(last, |s| s.1),
        };
        let changed = self.focused != Some(next);
        self.change_focus(Some(next));
        Some(changed | self.reveal(next))
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
