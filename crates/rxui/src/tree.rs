//! Controlled, fixed-height trees. Domain data and accepted state belong to the app.
use crate::{
    AccessibilityHandle, AppContext, Element, IntoElement, Key, KeyboardKey, Listener, Modifiers,
    ReadContext, ScrollError, ScrollHandle, SemanticAction, SemanticRole, column, virtual_list,
};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// One domain node. Parents must precede their children in [`TreeData::new`].
#[derive(Clone, Debug)]
pub struct TreeNode {
    /// Stable identity, unique across the entire data set (including hidden nodes).
    pub key: Key,
    /// Parent identity, or `None` for a root.
    pub parent: Option<Key>,
    /// Accessible name and type-ahead text.
    pub label: String,
    /// A branch may have zero children; a leaf cannot have children.
    pub branch: bool,
}
/// Invalid domain hierarchy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeDataError {
    /// Duplicate identity anywhere in the data, even under a collapsed branch.
    DuplicateKey(Key),
    /// Parent is absent, occurs later, or is a leaf. Cycles are rejected too.
    InvalidParent(Key),
}
impl std::fmt::Display for TreeDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TreeDataError {}
struct Data {
    nodes: Vec<TreeNode>,
    indices: HashMap<Key, usize>,
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
}
/// Immutable, cheaply cloned domain hierarchy, independent of UI placements.
#[derive(Clone)]
pub struct TreeData(Rc<Data>);
impl TreeData {
    /// Validates every key and parent in O(n); input order determines sibling order.
    /// No I/O or lazy loading is owned by this type.
    pub fn new(nodes: Vec<TreeNode>) -> Result<Self, TreeDataError> {
        let mut indices: HashMap<Key, usize> = HashMap::with_capacity(nodes.len());
        let mut children = vec![Vec::new(); nodes.len()];
        let mut roots = Vec::new();
        for (i, node) in nodes.iter().enumerate() {
            if indices.contains_key(&node.key) {
                return Err(TreeDataError::DuplicateKey(node.key.clone()));
            }
            if let Some(parent) = &node.parent {
                let p = *indices
                    .get(parent)
                    .ok_or_else(|| TreeDataError::InvalidParent(node.key.clone()))?;
                if !nodes[p].branch {
                    return Err(TreeDataError::InvalidParent(node.key.clone()));
                }
                children[p].push(i);
            } else {
                roots.push(i);
            }
            indices.insert(node.key.clone(), i);
        }
        Ok(Self(Rc::new(Data {
            nodes,
            indices,
            children,
            roots,
        })))
    }
    /// Full domain membership, including collapsed descendants.
    pub fn contains(&self, key: &Key) -> bool {
        self.0.indices.contains_key(key)
    }
    /// Domain node by stable key, including hidden nodes.
    pub fn node(&self, key: &Key) -> Option<&TreeNode> {
        self.0.indices.get(key).map(|&i| &self.0.nodes[i])
    }
}
/// Selection policy. Logical activity is independent of selection in both modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeSelectionMode {
    /// At most one selected key.
    Single,
    /// Primary-click toggles; Shift selects the inclusive visible range.
    Multiple,
}
/// One row in the cached visible preorder projection.
#[derive(Clone, Debug)]
pub struct TreeRow {
    node_index: usize,
    /// Hierarchy level starting at one.
    pub level: usize,
    /// Zero-based position among all siblings, including unmounted siblings.
    pub position_in_set: usize,
    /// Total number of siblings.
    pub set_size: usize,
}
#[derive(Clone)]
struct State {
    data: TreeData,
    expanded: HashSet<Key>,
    selected: HashSet<Key>,
    active: Option<Key>,
    anchor: Option<Key>,
    mode: TreeSelectionMode,
    rows: Rc<Vec<TreeRow>>,
    visible: Rc<HashMap<Key, usize>>,
    search: String,
    search_at: Duration,
    input_origin: Instant,
}
/// Accepted application-owned tree state. Cloning is O(1), including its projection.
/// Keep separate states for independent selection/expansion, sharing [`TreeData`].
#[derive(Clone)]
pub struct TreeState(Rc<State>);
impl TreeState {
    /// Starts with collapsed branches, no selection, and the first root active.
    pub fn new(data: TreeData, mode: TreeSelectionMode) -> Self {
        let mut state = State {
            data,
            expanded: HashSet::new(),
            selected: HashSet::new(),
            active: None,
            anchor: None,
            mode,
            rows: Rc::new(Vec::new()),
            visible: Rc::new(HashMap::new()),
            search: String::new(),
            search_at: Duration::ZERO,
            input_origin: Instant::now(),
        };
        state.project();
        Self(Rc::new(state))
    }
    /// Current visible preorder. Reused across scroll-only descriptions.
    pub fn rows(&self) -> &[TreeRow] {
        &self.0.rows
    }
    /// Domain node for a visible row index.
    pub fn node(&self, index: usize) -> &TreeNode {
        &self.0.data.0.nodes[self.0.rows[index].node_index]
    }
    /// Full domain hierarchy.
    pub fn data(&self) -> &TreeData {
        &self.0.data
    }
    /// Accepted logical active key, always visible when nonempty.
    pub fn active(&self) -> Option<&Key> {
        self.0.active.as_ref()
    }
    /// Accepted selected keys, including collapsed descendants.
    pub fn selected(&self) -> &HashSet<Key> {
        &self.0.selected
    }
    /// Range anchor. Hidden anchors survive collapse; deleted anchors are cleared.
    pub fn anchor(&self) -> Option<&Key> {
        self.0.anchor.as_ref()
    }
    /// Whether a branch is expanded.
    pub fn expanded(&self, key: &Key) -> bool {
        self.0.expanded.contains(key)
    }
    /// Replaces domain data and rebuilds the projection. Invalidates outstanding proposals.
    /// Hidden selection/anchor survive; deleted keys are removed. Hidden activity moves
    /// to its closest visible ancestor, then the first root if the key was deleted.
    pub fn set_data(&mut self, data: TreeData) {
        let mut next = (*self.0).clone();
        next.data = data;
        next.project();
        self.0 = Rc::new(next);
    }
    /// Controlled expansion update. Unknown keys and leaves are ignored.
    /// Invalidates outstanding proposals, even when the expansion set is unchanged.
    pub fn set_expanded(&mut self, keys: impl IntoIterator<Item = Key>) {
        let mut next = (*self.0).clone();
        next.expanded = keys.into_iter().collect();
        next.project();
        self.0 = Rc::new(next);
    }
    /// Replaces controlled selection/activity. Deleted selected keys and anchors are
    /// discarded; collapsed selected keys and anchors survive. Hidden activity moves
    /// to its closest visible ancestor. Single mode keeps only the first valid key.
    pub fn set_selection(
        &mut self,
        selected: impl IntoIterator<Item = Key>,
        active: Option<Key>,
        anchor: Option<Key>,
    ) {
        let mut next = (*self.0).clone();
        next.selected = selected
            .into_iter()
            .filter(|k| next.data.contains(k))
            .take(if next.mode == TreeSelectionMode::Single {
                1
            } else {
                usize::MAX
            })
            .collect();
        next.active = active;
        while let Some(key) = &next.active {
            if next.visible.contains_key(key) {
                break;
            }
            next.active = next.data.node(key).and_then(|n| n.parent.clone());
        }
        if next.active.is_none() {
            next.active = next
                .rows
                .first()
                .map(|r| next.data.0.nodes[r.node_index].key.clone());
        }
        next.anchor = anchor.filter(|k| next.data.contains(k));
        next.search.clear();
        self.0 = Rc::new(next);
    }
    /// Accepts a proposal only if it was made against this exact accepted snapshot.
    /// Rejection is simply not calling this method; no optimistic state is retained.
    pub fn apply(&mut self, proposal: &TreeProposal) -> bool {
        if !Rc::ptr_eq(&self.0, &proposal.base.0) {
            return false;
        }
        self.0 = proposal.next.0.clone();
        true
    }
    /// Reveals a visible key in the event's scroll placement. Hidden/deleted keys
    /// return `Ok(false)`; this does not expand ancestors or change selection.
    pub fn reveal(
        &self,
        key: &Key,
        scroll: &ScrollHandle,
        height: f32,
        cx: &mut AppContext<'_>,
    ) -> Result<bool, ScrollError> {
        let Some(&index) = self.0.visible.get(key) else {
            return Ok(false);
        };
        scroll.reveal_row(cx, index, height)?;
        Ok(true)
    }
    /// Monotonic elapsed input time, independent of frames. Custom event adapters
    /// mixed with [`virtual_tree`] can use this same time base; deterministic
    /// tests may pass their own consistent timestamps to [`Self::propose`].
    pub fn input_time(&self) -> Duration {
        self.0.input_origin.elapsed()
    }
    /// Makes a proposal for a command using a browser-safe, caller-supplied monotonic
    /// timestamp. No wall clock, local task, or `Instant::now` is used.
    pub fn propose(
        &self,
        command: TreeCommand,
        modifiers: Modifiers,
        page_rows: usize,
        time: Duration,
    ) -> Option<TreeProposal> {
        if matches!(&command, TreeCommand::Key(_))
            && (modifiers.alt
                || if cfg!(target_os = "macos") {
                    modifiers.control
                } else {
                    modifiers.meta
                })
        {
            return None;
        }
        let character_search = matches!(&command, TreeCommand::Key(KeyboardKey::Character(_)));
        let reveal = !matches!(&command, TreeCommand::Expand(_, _));
        let activate = matches!(
            &command,
            TreeCommand::Select(_) | TreeCommand::Key(KeyboardKey::Space)
        );
        let mut next = (*self.0).clone();
        if !matches!(&command, TreeCommand::Key(KeyboardKey::Character(_))) {
            next.search.clear();
        }
        let index = self.active().and_then(|k| self.0.visible.get(k)).copied()?;
        let mut destination = None;
        match command {
            TreeCommand::Select(key) => {
                destination = Some(*self.0.visible.get(&key)?);
            }
            TreeCommand::Expand(key, expanded) => {
                let node = next.data.node(&key)?;
                if !node.branch || !next.visible.contains_key(&key) {
                    return None;
                }
                if expanded {
                    next.expanded.insert(key);
                } else {
                    next.expanded.remove(&key);
                }
                next.project();
            }
            TreeCommand::Key(key) => match key {
                KeyboardKey::ArrowDown => destination = Some((index + 1).min(next.rows.len() - 1)),
                KeyboardKey::ArrowUp => destination = Some(index.saturating_sub(1)),
                KeyboardKey::Home => destination = Some(0),
                KeyboardKey::End => destination = Some(next.rows.len() - 1),
                KeyboardKey::PageDown => {
                    destination = Some(
                        index
                            .saturating_add(page_rows.max(1))
                            .min(next.rows.len() - 1),
                    )
                }
                KeyboardKey::PageUp => destination = Some(index.saturating_sub(page_rows.max(1))),
                KeyboardKey::ArrowRight => {
                    let node = self.node(index);
                    if node.branch && !self.expanded(&node.key) {
                        next.expanded.insert(node.key.clone());
                        next.project();
                    } else if node.branch
                        && index + 1 < next.rows.len()
                        && self.node(index + 1).parent.as_ref() == Some(&node.key)
                    {
                        destination = Some(index + 1);
                    }
                }
                KeyboardKey::ArrowLeft => {
                    let node = self.node(index);
                    if self.expanded(&node.key) {
                        next.expanded.remove(&node.key);
                        next.project();
                    } else {
                        destination = node
                            .parent
                            .as_ref()
                            .and_then(|p| next.visible.get(p))
                            .copied();
                    }
                }
                KeyboardKey::Space => destination = Some(index),
                KeyboardKey::Character(text)
                    if !modifiers.control
                        && !modifiers.meta
                        && !modifiers.alt
                        && !text.chars().any(char::is_control)
                        && !text.is_empty() =>
                {
                    if time.saturating_sub(next.search_at) > Duration::from_millis(700)
                        || time < next.search_at
                    {
                        next.search.clear();
                    }
                    let text = text.to_lowercase();
                    // Repeated single letters cycle; extending a prefix keeps the
                    // current match before searching subsequent rows.
                    let repeated = next.search == text && text.chars().count() == 1;
                    let extending = !next.search.is_empty() && !repeated;
                    if !repeated {
                        next.search.push_str(&text);
                    }
                    next.search_at = time;
                    let start = usize::from(!extending);
                    destination = (start..next.rows.len() + start)
                        .map(|step| (index + step) % next.rows.len())
                        .find(|&i| self.node(i).label.to_lowercase().starts_with(&next.search));
                    if destination.is_none() && extending {
                        next.search = text;
                        destination = (1..=next.rows.len())
                            .map(|step| (index + step) % next.rows.len())
                            .find(|&i| self.node(i).label.to_lowercase().starts_with(&next.search));
                    }
                }
                _ => return None,
            },
        }
        if let Some(i) = destination {
            let key = self.node(i).key.clone();
            next.active = Some(key.clone());
            if next.mode == TreeSelectionMode::Multiple && modifiers.shift && !character_search {
                // A collapsed anchor is retained, but cannot define a visible range.
                // Fall back to the previous active row without overwriting the anchor.
                if next.anchor.is_none() {
                    next.anchor = self.0.active.clone();
                }
                let anchor = next
                    .anchor
                    .as_ref()
                    .and_then(|a| self.0.visible.get(a))
                    .copied()
                    .unwrap_or(index);
                if !modifiers.primary() {
                    next.selected.clear();
                }
                for i in anchor.min(i)..=anchor.max(i) {
                    next.selected.insert(self.node(i).key.clone());
                }
            } else {
                // Primary navigation moves activity only; primary Select/Space toggles.
                if modifiers.primary() {
                    if activate {
                        if next.mode == TreeSelectionMode::Single {
                            next.selected.clear();
                        }
                        if !next.selected.remove(&key) {
                            next.selected.insert(key.clone());
                        }
                        next.anchor = Some(key.clone());
                    }
                } else {
                    next.selected.clear();
                    next.selected.insert(key.clone());
                }
                if !modifiers.primary() {
                    next.anchor = Some(key.clone());
                }
            }
        }
        Some(TreeProposal {
            base: self.clone(),
            reveal: reveal || self.0.active != next.active,
            next: Self(Rc::new(next)),
        })
    }
}
impl State {
    fn project(&mut self) {
        self.expanded
            .retain(|key| self.data.node(key).is_some_and(|n| n.branch));
        self.selected.retain(|key| self.data.contains(key));
        if self.anchor.as_ref().is_some_and(|k| !self.data.contains(k)) {
            self.anchor = None;
        }
        let mut rows = Vec::new();
        let mut visible = HashMap::new();
        let mut stack = vec![(&self.data.0.roots[..], 0, 1)];
        while let Some((siblings, position, level)) = stack.pop() {
            if let Some(&node_index) = siblings.get(position) {
                stack.push((siblings, position + 1, level));
                let node = &self.data.0.nodes[node_index];
                visible.insert(node.key.clone(), rows.len());
                rows.push(TreeRow {
                    node_index,
                    level,
                    position_in_set: position,
                    set_size: siblings.len(),
                });
                if self.expanded.contains(&node.key) {
                    stack.push((&self.data.0.children[node_index], 0, level + 1));
                }
            }
        }
        let mut active = self.active.clone();
        while let Some(key) = &active {
            if visible.contains_key(key) {
                break;
            }
            active = self.data.node(key).and_then(|n| n.parent.clone());
        }
        self.active = active.or_else(|| {
            rows.first()
                .map(|r| self.data.0.nodes[r.node_index].key.clone())
        });
        self.rows = Rc::new(rows);
        self.visible = Rc::new(visible);
        self.search.clear();
    }
}
/// Application intent, interpreted against a particular accepted projection.
#[derive(Clone, Debug)]
pub enum TreeCommand {
    /// Pointer/assistive selection of a visible key.
    Select(Key),
    /// Explicit expansion/collapse of a visible branch.
    Expand(Key, bool),
    /// Logical keyboard input.
    Key(KeyboardKey),
}
/// Controlled proposal. Retaining or rejecting it does not change accepted state.
#[derive(Clone)]
pub struct TreeProposal {
    base: TreeState,
    next: TreeState,
    reveal: bool,
}
impl TreeProposal {
    /// Proposed state, useful for validation before acceptance.
    pub fn proposed(&self) -> &TreeState {
        &self.next
    }
    /// Accepts if current, then reveals accepted activity in the source placement.
    /// Rejected/stale proposals queue no scroll. Geometry errors are returned after
    /// acceptance; valid geometry is the same requirement as [`crate::virtual_list`].
    pub fn accept(
        &self,
        state: &mut TreeState,
        scroll: &ScrollHandle,
        height: f32,
        cx: &mut AppContext<'_>,
    ) -> Result<bool, ScrollError> {
        if !state.apply(self) {
            return Ok(false);
        }
        if self.reveal
            && let Some(key) = state.active()
        {
            state.reveal(key, scroll, height, cx)?;
        }
        Ok(true)
    }
}

/// Fixed-height single-column tree builder. Rendered content must be passive:
/// labels and decoration only, with no focusable descendants, controls or input
/// listeners. The tree owns row input and keeps focus on its persistent root.
/// Only mounted TreeItems exist in semantics; this is not a virtual accessibility
/// provider. Unmounted keys cannot receive assistive actions.
/// Type-ahead uses elapsed monotonic input time from a stable state-owned origin
/// (`web_time` on wasm), so expiration works while the host's frame loop is idle.
/// Accept proposals with [`TreeProposal::accept`] in `on_change` to reveal offscreen
/// keyboard destinations synchronously. Use a bounded height or `fill_height()`.
/// The renderer runs only for viewport rows plus two overscan rows on each side.
/// It receives the visible index, domain node and projection metadata. The tree
/// supplies 16px indentation per level and a 20px disclosure region before content.
/// A branch disclosure press proposes expansion without selecting the row; the
/// remainder selects. Built-in scrollbars support pointer scrolling, but the tree
/// root owns keyboard focus and scrolling. Rows paint as [`Element::list_row`]s:
/// hovered rows and selected rows use the theme's row colors, and selection dims
/// to RowSelectedInactive while focus is outside the tree.
pub fn virtual_tree<F, E>(
    state: &TreeState,
    row_height: f32,
    scroll: &ScrollHandle,
    cx: &impl ReadContext,
    on_change: Listener<TreeProposal>,
    mut render: F,
) -> Element
where
    F: FnMut(usize, &TreeNode, &TreeRow) -> E,
    E: IntoElement,
{
    let metrics = scroll.state(cx);
    let page = metrics.map_or(1, |m| (m.viewport[1] / row_height).floor().max(1.) as usize);
    let root_focus = crate::FocusHandle::new();
    let active_handle = AccessibilityHandle::new();
    let key_state = state.clone();
    let key_change = on_change.clone();
    let key_listener = Listener::from_callback(move |event: &crate::KeyInput, cx| {
        let time = key_state.input_time();
        if let Some(proposal) = key_state.propose(
            TreeCommand::Key(event.event.key.clone()),
            event.event.modifiers,
            page,
            time,
        ) {
            event.prevent_default();
            event.stop_propagation();
            key_change.dispatch(&proposal, cx)
        } else {
            Ok(crate::Dispatch::Unchanged)
        }
    });
    let mut rows = virtual_list(state.rows().len(), row_height, scroll, cx, |index| {
        let node = state.node(index);
        let info = &state.rows()[index];
        let branch = node.branch;
        let expanded = state.expanded(&node.key);
        let indent = (info.level - 1) as f32 * 16.;
        // The row selects; a branch's disclosure, which stops propagation, toggles.
        let pointer = |disclosure: bool| {
            let pointer_state = state.clone();
            let pointer_key = node.key.clone();
            let pointer_change = on_change.clone();
            let focus = root_focus.clone();
            Listener::from_callback(move |event: &crate::PointerInput, cx| {
                if event.button != Some(crate::PointerButton::Primary) {
                    return Ok(crate::Dispatch::Unchanged);
                }
                event.prevent_default();
                event.stop_propagation();
                focus.focus_bound(cx);
                let command = if disclosure {
                    TreeCommand::Expand(pointer_key.clone(), !expanded)
                } else {
                    TreeCommand::Select(pointer_key.clone())
                };
                if let Some(proposal) =
                    pointer_state.propose(command, event.modifiers, page, Duration::ZERO)
                {
                    pointer_change.dispatch(&proposal, cx)
                } else {
                    Ok(crate::Dispatch::Unchanged)
                }
            })
        };
        let semantic_state = state.clone();
        let semantic_key = node.key.clone();
        let semantic_change = on_change.clone();
        let semantic = Listener::from_callback(move |event: &SemanticAction, cx| {
            let command = match event {
                SemanticAction::Activate(_) | SemanticAction::Select(_) => {
                    TreeCommand::Select(semantic_key.clone())
                }
                SemanticAction::Expand(_) => TreeCommand::Expand(semantic_key.clone(), true),
                SemanticAction::Collapse(_) => TreeCommand::Expand(semantic_key.clone(), false),
                _ => return Ok(crate::Dispatch::Unchanged),
            };
            if let Some(proposal) =
                semantic_state.propose(command, Modifiers::default(), page, Duration::ZERO)
            {
                semantic_change.dispatch(&proposal, cx)
            } else {
                Ok(crate::Dispatch::Unchanged)
            }
        });
        let mut row = column()
            .key(node.key.clone())
            .accessibility_role(SemanticRole::TreeItem)
            .accessibility_label(node.label.clone())
            .accessibility_level(info.level)
            .accessibility_selected(state.selected().contains(&node.key))
            .list_row(state.selected().contains(&node.key))
            .on_pointer_down(pointer(false))
            .on_semantic_action(semantic)
            .child(
                crate::row()
                    .fill_width()
                    .fill_height()
                    .padding_left(indent)
                    // Center text lines in rows taller than them.
                    .align_items(taffy::AlignItems::CENTER)
                    .child({
                        // The chevron centers in the disclosure region.
                        let disclosure = if node.branch {
                            crate::icon(if expanded {
                                crate::Icon::CHEVRON_DOWN
                            } else {
                                crate::Icon::CHEVRON_RIGHT
                            })
                        } else {
                            crate::row()
                        }
                        .width(20.);
                        if branch {
                            disclosure.on_pointer_down(pointer(true))
                        } else {
                            disclosure
                        }
                    })
                    .child(
                        render(index, node, info)
                            .into_element()
                            .flex_grow(1.)
                            .min_width(0.),
                    ),
            );
        if node.branch {
            row = row.accessibility_expanded(state.expanded(&node.key));
        }
        if state.active() == Some(&node.key) {
            row = row.accessibility_handle(active_handle.clone());
        }
        let properties = row.semantics.get_or_insert_with(Default::default);
        properties.position_in_set = Some(info.position_in_set);
        properties.set_size = Some(info.set_size);
        row
    })
    .plain()
    .fill_width()
    .fill_height()
    .into_element();
    // The tree handles keyboard scrolling. Keep the built-in bars out of Tab
    // traversal and group focus restoration, while preserving pointer/range input.
    fn unfocus_bars(element: &mut Element) {
        if matches!(element.kind, crate::element::ElementKind::Scrollbar(_)) {
            element.input.get_or_insert_with(Default::default).focusable = Some(false);
        }
        for child in &mut element.children {
            unfocus_bars(child);
        }
    }
    unfocus_bars(&mut rows);
    let focus = root_focus.clone();
    let root_pointer = Listener::from_callback(move |_: &crate::PointerInput, cx| {
        focus.focus_bound(cx);
        Ok(crate::Dispatch::Unchanged)
    });
    let mut root = column()
        .focusable(true)
        .focus_handle(root_focus)
        .accessibility_role(SemanticRole::Tree)
        .accessibility_active_descendant(active_handle)
        .on_key_down(key_listener)
        .on_pointer_down(root_pointer)
        .child(rows);
    root.semantics
        .get_or_insert_with(Default::default)
        .multiselectable = state.0.mode == TreeSelectionMode::Multiple;
    root
}
