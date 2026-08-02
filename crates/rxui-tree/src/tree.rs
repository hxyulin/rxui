//! Retained arena, incremental passes, fragments, and hit testing.

use std::{any::Any, collections::HashSet, marker::PhantomData, sync::Arc};

use astrelis_core::{
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
    math::{Affine2, Vec2},
};
use astrelis_paint::{DisplayList, DisplayListInstance, Painter};
use astrelis_platform::{CursorIcon, ElementState, Key, NamedKey};
use astrelis_text::{FontDatabase, TextLayout, TextLayoutContext, TextLayoutRequest};

use crate::{
    AccessibilityUpdate, ClipboardOperation, Constraints, Element, Invalidation, LayoutContext,
    SemanticAction, SemanticNode, UiInput,
};

/// Stable generational retained identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

impl NodeId {
    /// Returns this identity as a single integer.
    ///
    /// Both halves are included, so the value distinguishes a node from an
    /// earlier occupant of the same slot. It exists for consumers that key their
    /// own collections by retained identity and cannot afford to format it.
    pub const fn to_bits(self) -> u64 {
        ((self.index as u64) << 32) | self.generation as u64
    }
}

/// Typed retained identity.
pub struct NodeHandle<E> {
    id: NodeId,
    marker: PhantomData<fn() -> E>,
}

impl<E> NodeHandle<E> {
    /// Returns the kind-erased retained identity.
    pub const fn id(self) -> NodeId {
        self.id
    }
}

impl<E> Clone for NodeHandle<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E> Copy for NodeHandle<E> {}

impl<E> std::fmt::Debug for NodeHandle<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("NodeHandle").field(&self.id).finish()
    }
}

/// Per-update diagnostic counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassStats {
    /// Elements whose layout method ran.
    pub layout_elements: usize,
    /// Nodes whose composition metadata changed.
    pub composed_nodes: usize,
    /// Paint fragments rebuilt.
    pub rebuilt_fragments: usize,
    /// Paint fragments reused.
    pub reused_fragments: usize,
    /// Nodes visited by the most recent hit test.
    ///
    /// Hit testing happens during event dispatch rather than during a pass, so
    /// this survives [`UiTree::update_passes`] and keeps describing the last
    /// traversal instead of resetting to zero with the per-pass counters.
    pub hit_test_nodes: usize,
    /// Accessibility nodes emitted in the delta.
    pub accessibility_nodes: usize,
    /// Text layouts shaped during the update.
    pub shaped_text: usize,
    /// Nodes whose composition was recomputed.
    pub visited_compose_nodes: usize,
    /// Subtrees composition skipped entirely because nothing could have changed.
    pub compose_skipped_subtrees: usize,
    /// Nodes the accessibility pass walked.
    ///
    /// Distinct from [`PassStats::accessibility_nodes`], which counts the nodes
    /// the delta carried: a node is walked to reach a dirty descendant, or to
    /// learn that its inherited enablement and visibility still hold, without
    /// necessarily describing itself differently.
    pub visited_accessibility_nodes: usize,
    /// Subtrees the accessibility pass skipped entirely.
    pub accessibility_skipped_subtrees: usize,
    /// Ancestor links walked while propagating invalidation, and while deciding
    /// whether focus, hover, or pointer capture sits inside a mutated subtree.
    pub invalidate_steps: usize,
}

/// Cached fragment scene for one window.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    fragments: Vec<DisplayListInstance>,
    rebuilt: HashSet<NodeId>,
}

impl Scene {
    /// Fragment instances in paint order.
    pub fn fragments(&self) -> &[DisplayListInstance] {
        &self.fragments
    }

    /// Returns whether a node's local fragment was rebuilt this update.
    pub fn rebuilt(&self, id: NodeId) -> bool {
        self.rebuilt.contains(&id)
    }

    /// Flattens the cached fragments for today's display-list renderer.
    pub fn flatten(&self) -> Result<DisplayList, astrelis_paint::PaintError> {
        DisplayList::compose(self.fragments.clone())
    }
}

struct Node {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    element: Option<Box<dyn Element>>,
    offset: LogicalPoint,
    size: LogicalSize,
    last_constraints: Option<Constraints>,
    /// Layout pass that last reached this node, against
    /// [`UiTree::layout_generation`]. Zero means never, which is what a freshly
    /// inserted node reads as until its parent measures it.
    laid_out_at: u64,
    world_transform: Affine2,
    world_inverse: Affine2,
    world_bounds: LogicalRect,
    subtree_bounds: LogicalRect,
    world_clip: Option<LogicalRect>,
    visible: bool,
    enabled: bool,
    /// Enablement and visibility through the complete ancestor path, as the last
    /// accessibility pass computed them.
    ///
    /// The pass carries both down its own walk, so nothing has to walk ancestors
    /// per node, and comparing them is how it learns that a subtree still
    /// inherits what it inherited last time. They describe the last published
    /// state rather than the current one, which is why interactive queries such
    /// as [`UiTree::set_focus`] still resolve the ancestor path themselves: a
    /// mutation is visible to them immediately, and to these only after the pass.
    inherited_enabled: bool,
    inherited_visible: bool,
    own_dirty: Invalidation,
    subtree_dirty: Invalidation,
    fragment: Option<Arc<DisplayList>>,
    semantic: Option<SemanticNode>,
    /// Stamp against [`UiTree::mark_generation`], letting [`UiTree::set_children`]
    /// reject a duplicated identity without allocating a set.
    marked_at: u64,
}

impl Node {
    /// Creates a node with every pass invalidated, which is what a node nothing
    /// has yet measured, composed, painted, or described owes its passes.
    fn new(parent: Option<NodeId>, element: Box<dyn Element>) -> Self {
        Self {
            parent,
            children: Vec::new(),
            element: Some(element),
            offset: LogicalPoint::ZERO,
            size: LogicalSize::ZERO,
            last_constraints: None,
            laid_out_at: 0,
            world_transform: Affine2::IDENTITY,
            world_inverse: Affine2::IDENTITY,
            world_bounds: LogicalRect::default(),
            subtree_bounds: LogicalRect::default(),
            world_clip: None,
            visible: true,
            enabled: true,
            inherited_enabled: true,
            inherited_visible: true,
            own_dirty: Invalidation::ALL,
            subtree_dirty: Invalidation::ALL,
            fragment: None,
            semantic: None,
            marked_at: 0,
        }
    }
}

struct Slot {
    generation: u32,
    node: Option<Node>,
}

/// Incremental retained tree for one viewport.
pub struct UiTree {
    slots: Vec<Slot>,
    free: Vec<u32>,
    root: NodeId,
    viewport: LogicalSize,
    dirty: Invalidation,
    removed_semantics: Vec<NodeId>,
    focus: Option<NodeId>,
    last_focus: Option<NodeId>,
    hover: Option<NodeId>,
    pointer_capture: Option<NodeId>,
    scene: Scene,
    accessibility: AccessibilityUpdate,
    clipboard: Vec<ClipboardOperation>,
    stats: PassStats,
    /// Ancestor links walked since the last completed update. Accumulated while
    /// events mutate the tree, then folded into `stats` by `update_passes`.
    pending_invalidate_steps: usize,
    /// Counter identifying the current layout pass, so a node an element stopped
    /// measuring can be told apart from one it measured.
    layout_generation: u64,
    /// Counter identifying the current [`UiTree::set_children`] call.
    mark_generation: u64,
    fonts: FontDatabase,
    text_context: TextLayoutContext,
}

impl UiTree {
    /// Creates a retained tree with `root` as its root element.
    pub fn new(root: impl Element, viewport: LogicalSize) -> Self {
        Self::with_fonts(root, viewport, FontDatabase::default())
    }

    /// Creates a retained tree using an application-provided font database.
    pub fn with_fonts(root: impl Element, viewport: LogicalSize, fonts: FontDatabase) -> Self {
        let root_id = NodeId {
            index: 0,
            generation: 1,
        };
        Self {
            slots: vec![Slot {
                generation: 1,
                node: Some(Node::new(None, Box::new(root))),
            }],
            free: Vec::new(),
            root: root_id,
            viewport,
            dirty: Invalidation::ALL,
            removed_semantics: Vec::new(),
            focus: None,
            last_focus: None,
            hover: None,
            pointer_capture: None,
            scene: Scene::default(),
            accessibility: AccessibilityUpdate::default(),
            clipboard: Vec::new(),
            stats: PassStats::default(),
            pending_invalidate_steps: 0,
            layout_generation: 0,
            mark_generation: 0,
            fonts,
            text_context: TextLayoutContext::new(),
        }
    }

    /// Returns the root identity.
    pub const fn root(&self) -> NodeId {
        self.root
    }

    /// Returns the logical viewport.
    pub const fn viewport(&self) -> LogicalSize {
        self.viewport
    }

    /// Changes the viewport.
    pub fn set_viewport(&mut self, viewport: LogicalSize) {
        if self.viewport != viewport {
            self.viewport = viewport;
            self.invalidate(self.root, Invalidation::LAYOUT_ALL);
        }
    }

    /// Appends one retained child.
    pub fn append<E: Element>(&mut self, parent: NodeId, element: E) -> NodeHandle<E> {
        let index = self.node(parent).children.len();
        self.insert_child_at(parent, index, element)
    }

    /// Inserts one retained child at `index`, shifting later siblings along.
    ///
    /// `index` is clamped to the current child count, so passing one past the
    /// end appends. The siblings that shift keep their fragments, semantics,
    /// cached constraints, and identities: only the parent is invalidated, since
    /// only its child list changed.
    pub fn insert_child_at<E: Element>(
        &mut self,
        parent: NodeId,
        index: usize,
        element: E,
    ) -> NodeHandle<E> {
        let count = self.node(parent).children.len();
        let id = self.allocate(Node::new(Some(parent), Box::new(element)));
        self.node_mut(parent).children.insert(index.min(count), id);
        self.invalidate(parent, Invalidation::TREE);
        NodeHandle {
            id,
            marker: PhantomData,
        }
    }

    /// Moves one existing child to `index` among its siblings.
    ///
    /// `index` counts positions in the resulting list and is clamped to its last
    /// slot. The moved subtree is untouched: nothing is rebuilt, re-described, or
    /// re-measured for it, and focus, which is an identity rather than a path,
    /// stays where it was.
    pub fn move_child(&mut self, parent: NodeId, child: NodeId, index: usize) {
        let from = self
            .node(parent)
            .children
            .iter()
            .position(|current| *current == child)
            .expect("move_child requires a direct child");
        let node = self.node_mut(parent);
        let to = index.min(node.children.len() - 1);
        if from == to {
            return;
        }
        node.children.remove(from);
        node.children.insert(to, child);
        self.invalidate(parent, Invalidation::TREE);
    }

    /// Moves one subtree under a new parent at `index`.
    ///
    /// The subtree keeps its identities, cached fragments, published semantics,
    /// and cached constraints, so a reparent costs the two parents' relayout
    /// plus recomposition of the moved subtree, not a rebuild of it. Focus
    /// survives for the same reason it survives a reorder, unless the new
    /// ancestor path leaves the focused node hidden or disabled, in which case
    /// focus is cleared as though the node had been hidden.
    ///
    /// `index` is clamped to the new parent's child count, counted after the
    /// subtree is detached, so reparenting within one parent behaves like
    /// [`UiTree::move_child`].
    pub fn reparent(&mut self, id: NodeId, parent: NodeId, index: usize) {
        if id == self.root {
            panic!("the retained root cannot be reparented");
        }
        self.node(parent);
        if self.is_descendant_or_self(parent, id) {
            panic!("reparent would make a node its own ancestor");
        }
        let old_parent = self.node(id).parent;
        if let Some(old_parent) = old_parent {
            self.node_mut(old_parent)
                .children
                .retain(|child| *child != id);
        }
        let count = self.node(parent).children.len();
        self.node_mut(parent).children.insert(index.min(count), id);
        self.node_mut(id).parent = Some(parent);
        if let Some(old_parent) = old_parent.filter(|old| *old != parent) {
            self.invalidate(old_parent, Invalidation::TREE);
        }
        self.invalidate(parent, Invalidation::TREE);
        // The subtree can inherit a different world transform and clip, and the
        // node reports its parent as part of its own semantics. Neither follows
        // from the parents' relayout: a move between two containers that overlay
        // their children at the same origin changes no geometry at all, and the
        // reported parent would stay stale.
        self.invalidate(
            id,
            Invalidation::COMPOSE | Invalidation::ACCESSIBILITY | Invalidation::HIT_TEST,
        );
        if let Some(focus) = self.focus
            && self.is_descendant_or_self(focus, id)
            && (!self.effective_visible(focus) || !self.effective_enabled(focus))
        {
            self.set_focus(None);
        }
    }

    /// Removes one subtree.
    pub fn remove(&mut self, id: NodeId) {
        if id == self.root {
            panic!("the retained root cannot be removed");
        }
        let parent = self.node(id).parent;
        if let Some(parent) = parent {
            self.node_mut(parent).children.retain(|child| *child != id);
        }
        self.release_subtree(id);
        if let Some(parent) = parent {
            self.invalidate(parent, Invalidation::TREE);
        }
    }

    /// Replaces a parent's child order with `children`.
    ///
    /// Every identity must already be a direct child of `parent`; a child left
    /// out of the list is removed, along with its subtree. The list is diffed
    /// rather than trusted: an identical order costs nothing at all and requests
    /// no pass, and a pure reorder permutes the existing list in place and asks
    /// only for the parent's relayout, leaving every child's fragments,
    /// semantics, and cached constraints alone.
    pub fn set_children(&mut self, parent: NodeId, children: &[NodeId]) {
        let count = self.node(parent).children.len();
        // Every identity must be a distinct direct child, so a longer list can
        // only be a duplicate or a foreign node.
        if children.len() > count {
            panic!("set_children received a duplicate or non-child identity");
        }
        self.mark_generation += 1;
        let mark = self.mark_generation;
        for child in children {
            let node = self.node_mut(*child);
            if node.marked_at == mark {
                panic!("duplicate retained child");
            }
            node.marked_at = mark;
            if node.parent != Some(parent) {
                panic!("set_children received a non-child identity");
            }
        }
        let identical = children.len() == count
            && self
                .node(parent)
                .children
                .iter()
                .zip(children)
                .all(|(current, requested)| current == requested);
        if identical {
            return;
        }
        let removed = children.len() != count;
        if removed {
            let mut index = 0;
            while index < self.node(parent).children.len() {
                let child = self.node(parent).children[index];
                index += 1;
                if self.node(child).marked_at != mark {
                    self.release_subtree(child);
                }
            }
        }
        {
            // `clear` then `extend_from_slice` reuses the list's allocation,
            // where assigning a fresh `Vec` would hand the parent a new one on
            // every reorder.
            let node = self.node_mut(parent);
            node.children.clear();
            node.children.extend_from_slice(children);
        }
        self.invalidate(parent, Invalidation::TREE);
    }

    /// Mutates a typed element and requests the declared passes.
    pub(crate) fn update<E: Element>(
        &mut self,
        handle: NodeHandle<E>,
        invalidation: Invalidation,
        update: impl FnOnce(&mut E),
    ) {
        let element = self
            .node_mut(handle.id)
            .element
            .as_deref_mut()
            .and_then(|element| element.as_any_mut().downcast_mut::<E>())
            .expect("retained handle has the wrong element type");
        update(element);
        self.invalidate(handle.id, invalidation);
    }

    /// Reads a typed retained element.
    pub fn element<E: Element>(&self, handle: NodeHandle<E>) -> &E {
        self.node(handle.id)
            .element
            .as_deref()
            .and_then(|element| element.as_any().downcast_ref::<E>())
            .expect("retained handle has the wrong element type")
    }

    /// Returns whether an identity is still live.
    pub fn contains(&self, id: NodeId) -> bool {
        self.slots
            .get(id.index as usize)
            .is_some_and(|slot| slot.generation == id.generation && slot.node.is_some())
    }

    /// Sets retained visibility.
    ///
    /// A hidden node is excluded from paint, composed bounds, hit testing, focus
    /// order, and semantics, while still occupying the layout space it measured.
    /// Descendants are not flagged: every pass that cares carries the inherited
    /// value down its own walk, so this costs one field write and one
    /// early-exiting invalidation rather than a walk of the subtree.
    pub fn set_visible(&mut self, id: NodeId, visible: bool) {
        if self.node(id).visible == visible {
            return;
        }
        if !visible {
            self.release_interaction(id);
        }
        self.node_mut(id).visible = visible;
        // No `LAYOUT`: a hidden subtree keeps the space it measured, so nothing
        // above or below it reflows.
        self.invalidate(
            id,
            Invalidation::COMPOSE
                | Invalidation::PAINT
                | Invalidation::ACCESSIBILITY
                | Invalidation::HIT_TEST,
        );
    }

    /// Sets effective interaction enablement.
    ///
    /// Inherited by descendants through the same mechanism as
    /// [`UiTree::set_visible`], and so equally independent of subtree size.
    pub fn set_enabled(&mut self, id: NodeId, enabled: bool) {
        if self.node(id).enabled == enabled {
            return;
        }
        if !enabled {
            self.release_interaction(id);
        }
        self.node_mut(id).enabled = enabled;
        self.invalidate(
            id,
            Invalidation::PAINT | Invalidation::ACCESSIBILITY | Invalidation::HIT_TEST,
        );
    }

    /// Drops focus, pointer capture, and hover held inside one subtree.
    ///
    /// Each check resolves ancestors once from the held identity, rather than
    /// once per node in the subtree.
    fn release_interaction(&mut self, id: NodeId) {
        if let Some(focus) = self.focus
            && self.is_descendant_or_self(focus, id)
        {
            self.set_focus(None);
        }
        if let Some(capture) = self.pointer_capture
            && self.is_descendant_or_self(capture, id)
        {
            self.pointer_capture = None;
        }
        if let Some(hover) = self.hover
            && self.is_descendant_or_self(hover, id)
        {
            self.set_hover(None);
        }
    }

    fn is_descendant_or_self(&mut self, mut node: NodeId, ancestor: NodeId) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            if !self.contains(node) {
                return false;
            }
            let current = self.node(node);
            let Some(parent) = current.parent else {
                return false;
            };
            node = parent;
            self.pending_invalidate_steps += 1;
        }
    }

    /// Returns the retained passes currently invalidated.
    ///
    /// Observational only: reading never clears work. Read it *before*
    /// [`UiTree::update_passes`], which clears every pending bit once the
    /// corresponding passes have run.
    pub const fn invalidation(&self) -> Invalidation {
        self.dirty
    }

    /// Returns whether any retained pass is pending.
    ///
    /// Read before [`UiTree::update_passes`], which clears the pending bits.
    pub const fn needs_update(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Returns whether pending work can change on-screen output.
    ///
    /// True when layout, composition, or painting is pending. Deliberately
    /// excludes [`Invalidation::ACCESSIBILITY`] and [`Invalidation::HIT_TEST`]:
    /// both still require a pass, but neither changes rendered pixels, so
    /// neither justifies presenting a new frame.
    ///
    /// Read before [`UiTree::update_passes`], which clears the pending bits.
    pub const fn needs_redraw(&self) -> bool {
        const REDRAW: Invalidation = Invalidation::from_bits_retain(
            Invalidation::LAYOUT.bits() | Invalidation::COMPOSE.bits() | Invalidation::PAINT.bits(),
        );
        self.dirty.intersects(REDRAW)
    }

    /// Runs invalidated passes and returns cached output.
    pub fn update_passes(&mut self) -> FrameUpdate<'_> {
        // Hit testing runs while events are dispatched, which is before this
        // pass begins, so both counters would read zero to anyone inspecting
        // stats after a frame if they were reset along with the rest.
        let hit_test_nodes = self.stats.hit_test_nodes;
        self.stats = PassStats::default();
        self.stats.hit_test_nodes = hit_test_nodes;
        self.stats.invalidate_steps = std::mem::take(&mut self.pending_invalidate_steps);
        self.accessibility = AccessibilityUpdate {
            changed: Vec::new(),
            removed: std::mem::take(&mut self.removed_semantics),
        };
        if self.dirty.contains(Invalidation::LAYOUT) {
            self.layout_generation += 1;
            self.layout_node(self.root, Constraints::tight(self.viewport));
            self.dirty.insert(
                Invalidation::COMPOSE
                    | Invalidation::PAINT
                    | Invalidation::ACCESSIBILITY
                    | Invalidation::HIT_TEST,
            );
        }
        if self
            .dirty
            .intersects(Invalidation::COMPOSE | Invalidation::HIT_TEST)
        {
            self.compose_node(self.root, Affine2::IDENTITY, None, false);
        }
        if self.dirty.contains(Invalidation::PAINT) {
            self.rebuild_scene();
        } else {
            self.stats.reused_fragments = self.scene.fragments.len();
        }
        if self.dirty.contains(Invalidation::ACCESSIBILITY) {
            self.update_accessibility();
        }
        self.dirty = Invalidation::empty();
        FrameUpdate {
            scene: &self.scene,
            accessibility: &self.accessibility,
            stats: self.stats,
        }
    }

    /// Returns the most recently produced scene.
    pub const fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Returns counters from the most recently completed retained update.
    pub const fn stats(&self) -> PassStats {
        self.stats
    }

    /// Returns a full deterministic semantic snapshot.
    pub fn semantic_snapshot(&self) -> Vec<SemanticNode> {
        self.live_ids()
            .filter_map(|id| self.node(id).semantic.clone())
            .collect()
    }

    /// Drains platform clipboard mutations requested by retained elements.
    pub fn drain_clipboard(&mut self) -> impl Iterator<Item = ClipboardOperation> + '_ {
        self.clipboard.drain(..)
    }

    /// Hit-tests a window-space point and records traversal work.
    pub fn hit_test(&mut self, point: LogicalPoint) -> Option<NodeId> {
        let mut visited = 0;
        let hit = self.hit_test_node(self.root, point, true, &mut visited);
        self.stats.hit_test_nodes = visited;
        hit
    }

    /// Returns the cursor requested by the captured or hovered element.
    pub fn cursor_icon(&self) -> CursorIcon {
        self.pointer_capture
            .filter(|id| self.contains(*id))
            .or_else(|| self.hover.filter(|id| self.contains(*id)))
            .and_then(|id| self.node(id).element.as_deref())
            .map(Element::cursor_icon)
            .unwrap_or_default()
    }

    /// Delivers pointer input and returns a typed erased payload.
    pub fn dispatch(&mut self, input: UiInput) -> Option<Box<dyn Any>> {
        if matches!(input, UiInput::PointerLeft) {
            self.set_hover(None);
            return None;
        }
        if matches!(input, UiInput::HoverChanged(_)) {
            return None;
        }
        if let UiInput::Keyboard { input, modifiers } = &input
            && input.state == ElementState::Pressed
            && matches!(input.logical_key, Key::Named(NamedKey::Tab))
        {
            self.focus_next(modifiers.shift);
            return None;
        }
        // Hover and dispatch share one traversal: `set_hover` only routes
        // `HoverChanged` to elements, which cannot move retained geometry
        // before the next update, so the hit result stays valid below.
        let mut hovered_hit = None;
        if let UiInput::PointerMoved(point)
        | UiInput::PointerPressed(point)
        | UiInput::PointerReleased(point) = &input
        {
            let target = self.hit_test(*point);
            hovered_hit = Some(target);
            self.set_hover(target);
        }
        let release = matches!(input, UiInput::PointerReleased(_));
        if matches!(
            input,
            UiInput::PointerMoved(_) | UiInput::PointerReleased(_)
        ) && let Some(capture) = self.pointer_capture.filter(|id| self.contains(*id))
        {
            let action = self.dispatch_bubbling(capture, input);
            if release {
                self.pointer_capture = None;
            }
            return action;
        }
        let point = match &input {
            UiInput::PointerMoved(point)
            | UiInput::PointerPressed(point)
            | UiInput::PointerReleased(point) => *point,
            UiInput::PointerWheel { position, .. } => *position,
            UiInput::Keyboard { .. } => {
                let focus = self.focus?;
                return self.dispatch_bubbling(focus, input);
            }
            UiInput::FocusChanged(_)
            | UiInput::HoverChanged(_)
            | UiInput::PointerLeft
            | UiInput::Ime(_)
            | UiInput::Paste(_) => {
                let focus = self.focus?;
                return self.dispatch_to(focus, input);
            }
        };
        // `PointerWheel` never reaches the hover traversal above, so it still
        // needs its own hit test here.
        let hit = match hovered_hit {
            Some(target) => target,
            None => self.hit_test(point),
        };
        let Some(target) = hit else {
            if matches!(input, UiInput::PointerPressed(_)) {
                self.set_focus(None);
            }
            if release {
                self.pointer_capture = None;
            }
            return None;
        };
        if matches!(input, UiInput::PointerPressed(_)) {
            let focus = self
                .node(target)
                .element
                .as_deref()
                .is_some_and(Element::focusable)
                .then_some(target);
            self.set_focus(focus);
        }
        self.dispatch_bubbling(target, input)
    }

    fn set_hover(&mut self, target: Option<NodeId>) {
        if target == self.hover {
            return;
        }
        if let Some(old) = self.hover.filter(|id| self.contains(*id)) {
            let _ = self.dispatch_to(old, UiInput::HoverChanged(false));
        }
        self.hover = target.filter(|id| {
            self.contains(*id) && self.effective_visible(*id) && self.effective_enabled(*id)
        });
        if let Some(target) = self.hover {
            let _ = self.dispatch_to(target, UiInput::HoverChanged(true));
        }
    }

    /// Moves focus to one retained target or clears it.
    pub fn set_focus(&mut self, target: Option<NodeId>) {
        if target == self.focus {
            return;
        }
        if let Some(target) = target {
            let node = self.node(target);
            if !self.effective_visible(target)
                || !self.effective_enabled(target)
                || !node.element.as_deref().is_some_and(Element::focusable)
            {
                panic!("focus target is not focusable");
            }
        }
        if let Some(old) = self.focus {
            self.last_focus = Some(old);
            let _ = self.dispatch_to(old, UiInput::FocusChanged(false));
            self.invalidate(old, Invalidation::ACCESSIBILITY);
        }
        self.focus = target;
        if let Some(target) = target {
            let _ = self.dispatch_to(target, UiInput::FocusChanged(true));
            self.invalidate(target, Invalidation::ACCESSIBILITY);
        }
    }

    /// Returns the currently focused retained identity.
    pub const fn focused(&self) -> Option<NodeId> {
        self.focus
    }

    /// Returns the most recently blurred retained identity.
    pub const fn last_focused(&self) -> Option<NodeId> {
        self.last_focus
    }

    /// Focuses the first enabled, visible focus target in one subtree.
    pub fn focus_first_in_subtree(&mut self, root: NodeId) -> Option<NodeId> {
        self.node(root);
        if !self.effective_visible(root) || !self.effective_enabled(root) {
            return None;
        }
        let mut order = Vec::new();
        self.collect_focus_order(root, true, &mut order);
        let target = order.first().copied();
        self.set_focus(target);
        target
    }

    /// Advances focus in retained tree order, optionally in reverse.
    pub fn focus_next(&mut self, reverse: bool) {
        let mut order = Vec::new();
        self.collect_focus_order(self.root, true, &mut order);
        if order.is_empty() {
            return self.set_focus(None);
        }
        let current = self
            .focus
            .and_then(|focus| order.iter().position(|id| *id == focus));
        let next = if reverse {
            current
                .map(|index| (index + order.len() - 1) % order.len())
                .unwrap_or(order.len() - 1)
        } else {
            current.map(|index| (index + 1) % order.len()).unwrap_or(0)
        };
        self.set_focus(Some(order[next]))
    }

    /// Applies a platform accessibility operation to one retained element.
    pub fn perform_semantic_action(
        &mut self,
        target: NodeId,
        action: SemanticAction,
    ) -> Option<Box<dyn Any>> {
        if !self.effective_enabled(target) || !self.effective_visible(target) {
            return None;
        }
        if matches!(&action, SemanticAction::Focus) {
            if !self
                .node(target)
                .element
                .as_deref()
                .is_some_and(Element::focusable)
            {
                return None;
            }
            self.set_focus(Some(target));
            return None;
        }
        let element = self
            .node_mut(target)
            .element
            .as_deref_mut()
            .expect("element is temporarily unavailable");
        let result = element.semantic_action(action);
        if !result.invalidation.is_empty() {
            self.invalidate(target, result.invalidation);
        }
        if let Some(operation) = result.clipboard {
            self.clipboard.push(operation);
        }
        result.action
    }

    fn dispatch_to(&mut self, target: NodeId, input: UiInput) -> Option<Box<dyn Any>> {
        let input = self.localize_input(target, input);
        let element = self
            .node_mut(target)
            .element
            .as_deref_mut()
            .expect("element is temporarily unavailable");
        let result = element.event(input);
        if !result.invalidation.is_empty() {
            self.invalidate(target, result.invalidation);
        }
        if let Some(operation) = result.clipboard {
            self.clipboard.push(operation);
        }
        result.action
    }

    fn dispatch_bubbling(&mut self, mut target: NodeId, input: UiInput) -> Option<Box<dyn Any>> {
        loop {
            let localized = self.localize_input(target, input.clone());
            let result = {
                let element = self
                    .node_mut(target)
                    .element
                    .as_deref_mut()
                    .expect("element is temporarily unavailable");
                element.event(localized)
            };
            if !result.invalidation.is_empty() {
                self.invalidate(target, result.invalidation);
            }
            if let Some(operation) = result.clipboard {
                self.clipboard.push(operation);
            }
            if result.action.is_some() || result.handled {
                if matches!(input, UiInput::PointerPressed(_)) {
                    self.pointer_capture = Some(target);
                }
                return result.action;
            }
            let parent = self.node(target).parent?;
            target = parent;
        }
    }

    fn localize_input(&self, target: NodeId, input: UiInput) -> UiInput {
        let inverse = self.node(target).world_inverse;
        let local = |point: LogicalPoint| {
            let point = inverse.transform_point2(Vec2::new(point.x, point.y));
            LogicalPoint::new(point.x, point.y)
        };
        match input {
            UiInput::PointerMoved(point) => UiInput::PointerMoved(local(point)),
            UiInput::PointerPressed(point) => UiInput::PointerPressed(local(point)),
            UiInput::PointerReleased(point) => UiInput::PointerReleased(local(point)),
            UiInput::PointerWheel { position, delta } => UiInput::PointerWheel {
                position: local(position),
                delta,
            },
            other => other,
        }
    }

    pub(crate) fn shape_text(&mut self, request: TextLayoutRequest) -> TextLayout {
        let layout = self
            .text_context
            .layout(&mut self.fonts, request)
            .unwrap_or_else(|error| panic!("text shaping failed: {error}"));
        self.stats.shaped_text += 1;
        layout
    }

    pub(crate) fn children_ids(&self, id: NodeId) -> Vec<NodeId> {
        self.node(id).children.clone()
    }

    pub(crate) fn child_count(&self, id: NodeId) -> usize {
        self.node(id).children.len()
    }

    pub(crate) fn child_at(&self, id: NodeId, index: usize) -> Option<NodeId> {
        self.node(id).children.get(index).copied()
    }

    pub(crate) fn layout_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        constraints: Constraints,
    ) -> LogicalSize {
        if self.node(child).parent != Some(parent) {
            panic!("layout_child requires a direct child");
        }
        self.layout_node(child, constraints)
    }

    pub(crate) fn place_child(&mut self, parent: NodeId, child: NodeId, origin: LogicalPoint) {
        if self.node(child).parent != Some(parent) {
            panic!("place_child requires a direct child");
        }
        if self.node(child).offset != origin {
            self.node_mut(child).offset = origin;
            self.invalidate(child, Invalidation::COMPOSE | Invalidation::ACCESSIBILITY);
        }
    }

    pub(crate) fn child_size(&self, parent: NodeId, child: NodeId) -> LogicalSize {
        if self.node(child).parent != Some(parent) {
            panic!("child_size requires a direct child");
        }
        self.node(child).size
    }

    pub(crate) fn child_flex_grow(&self, parent: NodeId, child: NodeId) -> f32 {
        if self.node(child).parent != Some(parent) {
            panic!("layout may only inspect direct children");
        }
        self.node(child)
            .element
            .as_deref()
            .map(Element::flex_grow)
            .unwrap_or(0.0)
            .max(0.0)
    }

    fn layout_node(&mut self, id: NodeId, constraints: Constraints) -> LogicalSize {
        // Stamped before the early return, because "the pass reached this node"
        // is what the stamp records, not "the element ran". A node whose cached
        // size was reused is still measured as far as its parent is concerned.
        self.node_mut(id).laid_out_at = self.layout_generation;
        let needs_layout = {
            let node = self.node(id);
            node.own_dirty.contains(Invalidation::LAYOUT)
                || node.subtree_dirty.contains(Invalidation::LAYOUT)
                || node.last_constraints != Some(constraints)
        };
        if !needs_layout {
            return self.node(id).size;
        }
        let mut element = self
            .node_mut(id)
            .element
            .take()
            .expect("recursive layout of the same element");
        let size = element.layout(
            &mut LayoutContext {
                ui: self,
                current: id,
            },
            constraints,
        );
        let size = constraints.constrain(size);
        self.stats.layout_elements += 1;
        let node = self.node_mut(id);
        let size_changed = node.size != size;
        node.size = size;
        node.last_constraints = Some(constraints);
        node.element = Some(element);
        node.own_dirty.remove(Invalidation::LAYOUT);
        node.subtree_dirty.remove(Invalidation::LAYOUT);
        if size_changed {
            // Ancestors must learn that this subtree changed, so route the
            // post-layout work through `invalidate` instead of touching this
            // node alone. The requested bits exclude `LAYOUT`, which was just
            // cleared above and must not come back this pass.
            self.invalidate(
                id,
                Invalidation::COMPOSE
                    | Invalidation::PAINT
                    | Invalidation::ACCESSIBILITY
                    | Invalidation::HIT_TEST,
            );
        }
        let generation = self.layout_generation;
        let mut index = 0;
        while index < self.node(id).children.len() {
            let child = self.node(id).children[index];
            index += 1;
            if self.node(child).laid_out_at != generation {
                self.discard_layout(child);
            }
        }
        size
    }

    /// Zeroes cached geometry for a subtree this layout pass never reached.
    ///
    /// `layout_node` runs only where a parent's `layout` called `layout_child`,
    /// so a child an element stops measuring - one past a container's arity, or
    /// one it skips conditionally - would otherwise keep the size and offset it
    /// was last given and go on painting, hit-testing, and reporting accessible
    /// bounds at geometry that describes nothing. The same tree built fresh
    /// leaves an unmeasured subtree at zero, so match that.
    ///
    /// Descendants are zeroed too, not just the child itself: their offsets are
    /// parent-relative and so still self-consistent, which is exactly why a
    /// non-zero size below a zeroed parent would keep drawing.
    fn discard_layout(&mut self, id: NodeId) {
        if !self.zero_layout(id) {
            return;
        }
        // Every zeroed node needs its own recompose. Propagating from the top
        // alone is not enough, because a discard that leaves the parent's world
        // transform untouched makes `inherited_changed` false and composition
        // would prune the descent that has to recompute the world bounds.
        self.invalidate_subtree(
            id,
            Invalidation::COMPOSE
                | Invalidation::PAINT
                | Invalidation::ACCESSIBILITY
                | Invalidation::HIT_TEST,
        );
    }

    /// Zeroes `id` and its descendants, reporting whether anything changed.
    ///
    /// The walk reaches every descendant, including those below a node that is
    /// already zeroed. Stopping there would assume that a zeroed node has a
    /// zeroed subtree, and [`UiTree::reparent`] can graft a measured subtree
    /// under an unmeasured one, at which point the assumption is false and the
    /// grafted geometry survives as exactly the stale rectangle this method
    /// exists to remove. So a permanently unmeasured child costs a walk of its
    /// subtree each time its parent relayouts, which is what buys the invariant
    /// holding for every way a child can arrive rather than only for the ones
    /// that go through layout.
    fn zero_layout(&mut self, id: NodeId) -> bool {
        let node = self.node_mut(id);
        let mut changed = node.size != LogicalSize::ZERO
            || node.offset != LogicalPoint::ZERO
            || node.last_constraints.is_some();
        node.size = LogicalSize::ZERO;
        node.offset = LogicalPoint::ZERO;
        // Clearing this is what makes a discard recoverable: `layout_node`
        // returns the cached size whenever the incoming constraints match the
        // last ones, so a surviving entry would hand back zero once the element
        // started measuring this child again.
        node.last_constraints = None;
        let mut index = 0;
        while index < self.node(id).children.len() {
            let child = self.node(id).children[index];
            index += 1;
            changed |= self.zero_layout(child);
        }
        changed
    }

    /// Recomposes world geometry for `id`, descending only where something can
    /// have changed.
    ///
    /// `inherited_changed` reports whether this node's parent produced a
    /// different world transform or clip than last time. Those are the only two
    /// values a parent contributes to its children, so when it is `false` and
    /// the subtree carries no compose work, the cached geometry is already
    /// correct and the whole subtree is skipped.
    ///
    /// Returns the subtree bounds and whether anything in the subtree still owes
    /// the accessibility pass a visit.
    ///
    /// Composed geometry is reported verbatim as accessible bounds, so a node
    /// whose composition changed needs re-describing. This is the one producer of
    /// accessibility work that does not route through [`UiTree::invalidate`], and
    /// the accessibility pass prunes on `subtree_dirty`, so the bit has to reach
    /// the ancestors: reporting it back up the descent this walk already made
    /// costs nothing, where an ancestor walk per changed node would cost
    /// `O(depth)` each.
    fn compose_node(
        &mut self,
        id: NodeId,
        parent_transform: Affine2,
        parent_clip: Option<LogicalRect>,
        inherited_changed: bool,
    ) -> (LogicalRect, bool) {
        const COMPOSE_WORK: Invalidation = Invalidation::from_bits_retain(
            Invalidation::COMPOSE.bits() | Invalidation::HIT_TEST.bits(),
        );
        let cached = {
            let node = self.node(id);
            (!inherited_changed
                && !node.own_dirty.intersects(COMPOSE_WORK)
                && !node.subtree_dirty.intersects(COMPOSE_WORK))
            .then_some(node.subtree_bounds)
        };
        if let Some(subtree_bounds) = cached {
            self.stats.compose_skipped_subtrees += 1;
            return (subtree_bounds, false);
        }
        self.stats.visited_compose_nodes += 1;
        let (offset, size, transform, clips, visible) = {
            let node = self.node(id);
            let element = node
                .element
                .as_deref()
                .expect("element is temporarily unavailable");
            (
                node.offset,
                node.size,
                element.transform(),
                element.clips_children(),
                node.visible,
            )
        };
        if !visible {
            let node = self.node_mut(id);
            let changed = node.world_bounds != LogicalRect::default()
                || node.subtree_bounds != LogicalRect::default();
            node.world_bounds = LogicalRect::default();
            node.subtree_bounds = LogicalRect::default();
            // Clearing is load-bearing rather than tidiness: `invalidate` stops
            // walking as soon as an ancestor already carries the bits, so a
            // hidden node that kept them would swallow the propagation from a
            // later `set_visible` and its subtree would never recompose.
            node.own_dirty.remove(COMPOSE_WORK);
            node.subtree_dirty.remove(COMPOSE_WORK);
            if changed {
                node.own_dirty.insert(Invalidation::ACCESSIBILITY);
                node.subtree_dirty.insert(Invalidation::ACCESSIBILITY);
                self.dirty.insert(Invalidation::ACCESSIBILITY);
            }
            return (LogicalRect::default(), changed);
        }
        let world =
            parent_transform * Affine2::from_translation(Vec2::new(offset.x, offset.y)) * transform;
        let local_bounds = LogicalRect::from_xywh(0.0, 0.0, size.width, size.height);
        let bounds = transform_rect(world, local_bounds);
        let clip = if clips {
            Some(intersect_rect(parent_clip, Some(bounds)).unwrap_or_default())
        } else {
            parent_clip
        };
        // Children inherit exactly the world transform and the clip, so compare
        // those before overwriting them. A node that only moved its own bounds
        // does not force its children to recompose.
        let (self_changed, children_inherit) = {
            let node = self.node(id);
            let inherited = node.world_transform != world || node.world_clip != clip;
            (inherited || node.world_bounds != bounds, inherited)
        };
        {
            let node = self.node_mut(id);
            node.world_transform = world;
            node.world_inverse = world.inverse();
            node.world_bounds = bounds;
            node.world_clip = clip;
        }
        let mut subtree = bounds;
        let mut describe = false;
        let mut index = 0;
        while index < self.node(id).children.len() {
            let child = self.node(id).children[index];
            let (child_bounds, child_describe) =
                self.compose_node(child, world, clip, children_inherit);
            subtree = union_rect(subtree, child_bounds);
            describe |= child_describe;
            index += 1;
        }
        let node = self.node_mut(id);
        let changed = self_changed || node.subtree_bounds != subtree;
        node.subtree_bounds = subtree;
        node.own_dirty.remove(COMPOSE_WORK);
        node.subtree_dirty.remove(COMPOSE_WORK);
        if changed {
            node.own_dirty.insert(Invalidation::ACCESSIBILITY);
            self.stats.composed_nodes += 1;
        }
        let describe = describe || changed;
        if describe {
            self.node_mut(id)
                .subtree_dirty
                .insert(Invalidation::ACCESSIBILITY);
            self.dirty.insert(Invalidation::ACCESSIBILITY);
        }
        (subtree, describe)
    }

    fn rebuild_scene(&mut self) {
        let mut scene = Scene::default();
        self.collect_fragments(self.root, &mut scene);
        self.scene = scene;
    }

    fn collect_fragments(&mut self, id: NodeId, scene: &mut Scene) {
        let (visible, dirty, size, world, clip) = {
            let node = self.node(id);
            (
                node.visible,
                node.own_dirty.contains(Invalidation::PAINT),
                node.size,
                node.world_transform,
                node.world_clip,
            )
        };
        if !visible {
            // A hidden subtree keeps its `PAINT` bits. That is safe only because
            // this walk visits every node: nothing prunes on `subtree_dirty`
            // here, so the leftover bits cost a fragment rebuild when the node
            // is shown again and nothing else. Pruning this walk - the obvious
            // next optimization - would have to clear them, the way
            // `compose_node` does, or a `set_visible` could no longer propagate
            // past this node and the subtree would never repaint.
            return;
        }
        if dirty || self.node(id).fragment.is_none() {
            let mut painter = Painter::new();
            self.node(id)
                .element
                .as_deref()
                .expect("element is temporarily unavailable")
                .paint(&mut painter, size)
                .unwrap_or_else(|error| panic!("element painting failed: {error}"));
            let fragment = Arc::new(
                painter
                    .finish()
                    .unwrap_or_else(|error| panic!("paint fragment finalization failed: {error}")),
            );
            let node = self.node_mut(id);
            node.fragment = Some(fragment);
            node.own_dirty.remove(Invalidation::PAINT);
            scene.rebuilt.insert(id);
            self.stats.rebuilt_fragments += 1;
        } else {
            self.stats.reused_fragments += 1;
        }
        let fragment = self
            .node(id)
            .fragment
            .as_deref()
            .expect("fragment was ensured")
            .clone();
        scene.fragments.push(DisplayListInstance {
            list: fragment,
            transform: world,
            clip,
            opacity: 1.0,
        });
        let mut index = 0;
        while index < self.node(id).children.len() {
            let child = self.node(id).children[index];
            self.collect_fragments(child, scene);
            index += 1;
        }
        self.node_mut(id).subtree_dirty.remove(Invalidation::PAINT);
    }

    fn update_accessibility(&mut self) {
        self.describe_node(self.root, true, true, false)
    }

    /// Republishes semantics wherever they can have changed, descending only
    /// there.
    ///
    /// `inherited_enabled` and `inherited_visible` are the effective state of the
    /// ancestor path, carried down so that no node has to walk its own ancestors
    /// to learn it, and stored per node so that a later pass can tell whether it
    /// changed. Those two values are all a parent contributes, so
    /// `inherited_changed` plus the node's own bits decide the descent: a subtree
    /// inheriting what it inherited last time, with no accessibility work of its
    /// own, already holds the semantics it would recompute.
    fn describe_node(
        &mut self,
        id: NodeId,
        inherited_enabled: bool,
        inherited_visible: bool,
        inherited_changed: bool,
    ) {
        let (enabled, visible, own_dirty) = {
            let node = self.node(id);
            if !inherited_changed
                && !node.own_dirty.contains(Invalidation::ACCESSIBILITY)
                && !node.subtree_dirty.contains(Invalidation::ACCESSIBILITY)
            {
                self.stats.accessibility_skipped_subtrees += 1;
                return;
            }
            (
                inherited_enabled && node.enabled,
                inherited_visible && node.visible,
                node.own_dirty.contains(Invalidation::ACCESSIBILITY),
            )
        };
        self.stats.visited_accessibility_nodes += 1;
        let changed = {
            let node = self.node_mut(id);
            let changed = node.inherited_enabled != enabled || node.inherited_visible != visible;
            node.inherited_enabled = enabled;
            node.inherited_visible = visible;
            // Cleared here rather than after the descent, and unconditionally:
            // `invalidate` stops at the first ancestor already carrying the bits
            // it would add, so a node that kept them would swallow the next
            // mutation below it and that subtree would never be described again.
            node.own_dirty.remove(Invalidation::ACCESSIBILITY);
            node.subtree_dirty.remove(Invalidation::ACCESSIBILITY);
            changed
        };
        if own_dirty || changed {
            self.publish_semantics(id, enabled, visible);
        }
        let mut index = 0;
        while index < self.node(id).children.len() {
            let child = self.node(id).children[index];
            index += 1;
            self.describe_node(child, enabled, visible, changed);
        }
    }

    /// Recomputes one node's semantics and records what the delta owes.
    fn publish_semantics(&mut self, id: NodeId, enabled: bool, visible: bool) {
        let (parent, bounds, data, focusable, actions) = {
            let node = self.node(id);
            let element = node
                .element
                .as_deref()
                .expect("element is temporarily unavailable");
            (
                node.parent,
                node.world_bounds,
                element.accessibility(),
                element.focusable(),
                element.semantic_actions(),
            )
        };
        let semantic = data.filter(|_| visible).map(|data| SemanticNode {
            id,
            parent,
            bounds,
            data,
            focusable,
            focused: self.focus == Some(id),
            enabled,
            actions,
        });
        let node = self.node_mut(id);
        // Compared in place. The previous value is wanted only to decide what the
        // delta carries, and cloning every visited node's semantics to answer
        // that costs more than the delta it feeds.
        if node.semantic == semantic {
            return;
        }
        let withdrawn = semantic.is_none() && node.semantic.is_some();
        node.semantic = semantic;
        if withdrawn {
            self.accessibility.removed.push(id);
        } else if let Some(semantic) = self.node(id).semantic.clone() {
            self.accessibility.changed.push(semantic);
            self.stats.accessibility_nodes += 1;
        }
    }

    fn hit_test_node(
        &self,
        id: NodeId,
        point: LogicalPoint,
        ancestors_enabled: bool,
        visited: &mut usize,
    ) -> Option<NodeId> {
        *visited += 1;
        let node = self.node(id);
        if !node.visible
            || !ancestors_enabled
            || !node.subtree_bounds.contains(point)
            || node.world_clip.is_some_and(|clip| !clip.contains(point))
        {
            return None;
        }
        for child in node.children.iter().rev() {
            if let Some(hit) = self.hit_test_node(*child, point, node.enabled, visited) {
                return Some(hit);
            }
        }
        let local = node
            .world_inverse
            .transform_point2(Vec2::new(point.x, point.y));
        let point = LogicalPoint::new(local.x, local.y);
        let element = node.element.as_deref()?;
        (node.enabled && element.hit_testable() && element.hit_test(point, node.size)).then_some(id)
    }

    fn collect_focus_order(&self, id: NodeId, ancestors_enabled: bool, output: &mut Vec<NodeId>) {
        let node = self.node(id);
        let enabled = ancestors_enabled && node.enabled;
        if !node.visible || !enabled {
            return;
        }
        if node.element.as_deref().is_some_and(Element::focusable) {
            output.push(id);
        }
        for child in &node.children {
            self.collect_focus_order(*child, enabled, output);
        }
    }

    fn invalidate(&mut self, id: NodeId, invalidation: Invalidation) {
        if !self.contains(id) || invalidation.is_empty() {
            return;
        }
        let expanded = if invalidation.contains(Invalidation::TREE) {
            invalidation | Invalidation::LAYOUT_ALL
        } else if invalidation.contains(Invalidation::LAYOUT) {
            invalidation
                | Invalidation::COMPOSE
                | Invalidation::PAINT
                | Invalidation::ACCESSIBILITY
                | Invalidation::HIT_TEST
        } else {
            invalidation
        };
        let node = self.node_mut(id);
        node.own_dirty.insert(expanded);
        node.subtree_dirty.insert(expanded);
        let relayouts = expanded.contains(Invalidation::LAYOUT);
        let mut current = self.node(id).parent;
        while let Some(parent) = current {
            let node = self.node_mut(parent);
            // Propagation is monotone, so an ancestor that already carries
            // everything this walk would add proves every node above it
            // does too. Both effects must already hold: stopping on
            // `subtree_dirty` alone would strand a `LAYOUT` bit that has
            // not yet reached `own_dirty`.
            if node.subtree_dirty.contains(expanded)
                && (!relayouts || node.own_dirty.contains(Invalidation::LAYOUT))
            {
                break;
            }
            node.subtree_dirty.insert(expanded);
            if relayouts {
                node.own_dirty.insert(Invalidation::LAYOUT);
            }
            current = node.parent;
            self.pending_invalidate_steps += 1;
        }
        self.dirty.insert(expanded);
    }

    fn invalidate_subtree(&mut self, id: NodeId, invalidation: Invalidation) {
        self.invalidate(id, invalidation);
        let mut index = 0;
        while index < self.node(id).children.len() {
            let child = self.node(id).children[index];
            self.invalidate_subtree(child, invalidation);
            index += 1;
        }
    }

    fn effective_enabled(&self, mut id: NodeId) -> bool {
        loop {
            let node = self.node(id);
            if !node.enabled {
                return false;
            }
            let Some(parent) = node.parent else {
                return true;
            };
            id = parent;
        }
    }

    fn effective_visible(&self, mut id: NodeId) -> bool {
        loop {
            let node = self.node(id);
            if !node.visible {
                return false;
            }
            let Some(parent) = node.parent else {
                return true;
            };
            id = parent;
        }
    }

    fn allocate(&mut self, node: Node) -> NodeId {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1).max(1);
            slot.node = Some(node);
            NodeId {
                index,
                generation: slot.generation,
            }
        } else {
            let id = NodeId {
                index: self.slots.len() as u32,
                generation: 1,
            };
            self.slots.push(Slot {
                generation: 1,
                node: Some(node),
            });
            id
        }
    }

    /// Frees one subtree's slots and reports the semantics it published.
    ///
    /// The interaction state each held identity sits in is resolved once here,
    /// from that identity's own ancestor path. Asking the same question inside
    /// the recursion instead made closing a deep panel cost the subtree size
    /// times the held node's depth, since a hover held anywhere else in the tree
    /// walks its whole path to answer "no" for every node freed.
    fn release_subtree(&mut self, id: NodeId) {
        if let Some(capture) = self.pointer_capture
            && self.is_descendant_or_self(capture, id)
        {
            self.pointer_capture = None;
        }
        if let Some(hover) = self.hover
            && self.is_descendant_or_self(hover, id)
        {
            self.hover = None;
        }
        if let Some(focus) = self.focus
            && self.is_descendant_or_self(focus, id)
        {
            self.focus = None;
        }
        self.free_subtree(id);
    }

    fn free_subtree(&mut self, id: NodeId) {
        // Recursion frees descendants' slots but never edits this node's own
        // child list, so indexing it while descending is stable.
        let mut index = 0;
        while index < self.node(id).children.len() {
            let child = self.node(id).children[index];
            self.free_subtree(child);
            index += 1;
        }
        if self.node(id).semantic.is_some() {
            self.removed_semantics.push(id);
        }
        if let Some(slot) = self.slots.get_mut(id.index as usize)
            && slot.generation == id.generation
        {
            slot.node = None;
            self.free.push(id.index);
        }
    }

    fn node(&self, id: NodeId) -> &Node {
        self.slots
            .get(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.node.as_ref())
            .expect("stale retained identity")
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.slots
            .get_mut(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.node.as_mut())
            .expect("stale retained identity")
    }

    fn live_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.node.as_ref().map(|_| NodeId {
                index: index as u32,
                generation: slot.generation,
            })
        })
    }
}

/// Borrowed outputs from one completed retained update.
pub struct FrameUpdate<'a> {
    /// Cached paint-fragment scene.
    pub scene: &'a Scene,
    /// Accessibility delta.
    pub accessibility: &'a AccessibilityUpdate,
    /// Diagnostic pass counters.
    pub stats: PassStats,
}

fn transform_rect(transform: Affine2, rect: LogicalRect) -> LogicalRect {
    let points = [
        transform.transform_point2(Vec2::new(rect.origin.x, rect.origin.y)),
        transform.transform_point2(Vec2::new(rect.max_x(), rect.origin.y)),
        transform.transform_point2(Vec2::new(rect.origin.x, rect.max_y())),
        transform.transform_point2(Vec2::new(rect.max_x(), rect.max_y())),
    ];
    let min_x = points
        .iter()
        .map(|point| point.x)
        .fold(f32::INFINITY, f32::min);
    let min_y = points
        .iter()
        .map(|point| point.y)
        .fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|point| point.x)
        .fold(f32::NEG_INFINITY, f32::max);
    let max_y = points
        .iter()
        .map(|point| point.y)
        .fold(f32::NEG_INFINITY, f32::max);
    LogicalRect::from_xywh(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn union_rect(a: LogicalRect, b: LogicalRect) -> LogicalRect {
    if a.size.width == 0.0 && a.size.height == 0.0 {
        return b;
    }
    if b.size.width == 0.0 && b.size.height == 0.0 {
        return a;
    }
    let min_x = a.origin.x.min(b.origin.x);
    let min_y = a.origin.y.min(b.origin.y);
    let max_x = a.max_x().max(b.max_x());
    let max_y = a.max_y().max(b.max_y());
    LogicalRect::from_xywh(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn intersect_rect(a: Option<LogicalRect>, b: Option<LogicalRect>) -> Option<LogicalRect> {
    match (a, b) {
        (None, value) | (value, None) => value,
        (Some(a), Some(b)) => {
            let min_x = a.origin.x.max(b.origin.x);
            let min_y = a.origin.y.max(b.origin.y);
            let max_x = a.max_x().min(b.max_x());
            let max_y = a.max_y().min(b.max_y());
            Some(LogicalRect::from_xywh(
                min_x,
                min_y,
                (max_x - min_x).max(0.0),
                (max_y - min_y).max(0.0),
            ))
        }
    }
}
