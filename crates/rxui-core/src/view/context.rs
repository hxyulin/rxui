//! The handle a view node reaches the retained tree through.

use astrelis_ui_next::{Element, NodeHandle, NodeId, UiError, UiRoot};

use crate::{
    DirtyHandle, Theme,
    diagnostics::ViewStats,
    view::{ActionEmitter, AnyView, Mounted},
};
/// Everything a [`crate::ViewNode`] needs to reach the retained tree.
///
/// A context is always scoped to a parent node: [`append`](Self::append) and
/// child reconciliation attach below [`parent`](Self::parent). A view that
/// creates a container node re-scopes with [`child`](Self::child) before
/// mounting anything under it.
///
/// Descending is done through [`build_child`](Self::build_child) and
/// [`rebuild_child`](Self::rebuild_child) rather than by calling
/// [`crate::ViewNode::build`] directly. They stamp identity, decide between
/// reconciling and replacing, and keep the [`ViewStats`] counters honest.
pub struct ViewContext<'a, Action: 'static> {
    ui: &'a mut UiRoot,
    parent: NodeId,
    theme: &'a Theme,
    emitter: ActionEmitter<Action>,
    /// Component nesting depth of the component whose view is being built.
    depth: u32,
    /// Dirty set shared with the owning [`crate::ComponentRuntime`].
    dirty: &'a DirtyHandle,
    /// Whether this pass must ignore props equality at component boundaries.
    force: bool,
}

impl<'a, Action: 'static> ViewContext<'a, Action> {
    pub(crate) fn new(
        ui: &'a mut UiRoot,
        parent: NodeId,
        theme: &'a Theme,
        emitter: ActionEmitter<Action>,
        depth: u32,
        dirty: &'a DirtyHandle,
        force: bool,
    ) -> Self {
        Self {
            ui,
            parent,
            theme,
            emitter,
            depth,
            dirty,
            force,
        }
    }

    /// Borrows the retained tree.
    pub fn ui(&mut self) -> &mut UiRoot {
        self.ui
    }

    /// Reads the theme this pass resolves styles against.
    ///
    /// The borrow outlives the context, so resolved colors and spacing can be
    /// computed before the tree is borrowed mutably.
    pub const fn theme(&self) -> &'a Theme {
        self.theme
    }

    /// Retained node new children are attached below.
    pub const fn parent(&self) -> NodeId {
        self.parent
    }

    /// Returns the bridge that erases this subtree's actions for routing.
    ///
    /// Hand it to a retained element once, at mount, and store it: a fresh
    /// emitter allocates, and reinstalling a callback closure on every pass is
    /// what makes action-sink identity unstable across frames.
    pub fn emitter(&self) -> ActionEmitter<Action> {
        self.emitter.clone()
    }

    /// Appends a retained element below [`parent`](Self::parent).
    pub fn append<E: Element>(&mut self, element: E) -> Result<NodeHandle<E>, UiError> {
        let parent = self.parent;
        self.ui.append(parent, element)
    }

    /// Re-scopes this context so children attach below `parent`.
    pub fn child(&mut self, parent: NodeId) -> ViewContext<'_, Action> {
        ViewContext {
            ui: self.ui,
            parent,
            theme: self.theme,
            emitter: self.emitter.clone(),
            depth: self.depth,
            dirty: self.dirty,
            force: self.force,
        }
    }

    /// Re-scopes this context for a subtree with its own action type.
    ///
    /// `emitter` is what the subtree's retained elements will emit through, so a
    /// wrapper that maps actions builds one at mount - typically with
    /// [`ActionEmitter::map`] - stores it, and passes the same one every pass.
    pub fn scoped<Child: 'static>(
        &mut self,
        parent: NodeId,
        emitter: &ActionEmitter<Child>,
    ) -> ViewContext<'_, Child> {
        ViewContext {
            ui: self.ui,
            parent,
            theme: self.theme,
            emitter: emitter.clone(),
            depth: self.depth,
            dirty: self.dirty,
            force: self.force,
        }
    }

    /// Mounts one child view from scratch.
    pub fn build_child(&mut self, view: AnyView<Action>) -> Result<Mounted<Action>, UiError> {
        // `nodes_built`: one view node mounted from scratch. Every mount goes
        // through here, so the counter is independent of how many wrapper layers
        // a retained node sits behind.
        ViewStats::record_node_built();
        let AnyView { key, inner } = view;
        let kind = inner.kind();
        let mut mounted = inner.build(self)?;
        mounted.key = key;
        mounted.kind = kind;
        Ok(mounted)
    }

    /// Reconciles one child view, replacing the mounted node on a kind change.
    ///
    /// This is the single place the kind-mismatch decision is made. A container
    /// that pairs a fresh view with a mounted node calls this and is done: on a
    /// mismatch the old retained subtree is removed, the new view is mounted,
    /// and `mounted` is overwritten - including its identity, which a
    /// transparent wrapper should read back with [`Mounted::node`].
    pub fn rebuild_child(
        &mut self,
        mounted: &mut Mounted<Action>,
        view: AnyView<Action>,
    ) -> Result<(), UiError> {
        let AnyView { key, inner } = view;
        if mounted.kind == inner.kind() {
            if !inner.records_own_rebuild() {
                // `nodes_rebuilt`: one view node reconciled against its previous
                // mounted instance rather than replaced.
                ViewStats::record_node_rebuilt();
            }
            return inner.rebuild(mounted, self);
        }
        self.ui.remove(mounted.node)?;
        ViewStats::record_node_built();
        let kind = inner.kind();
        let mut built = inner.build(self)?;
        built.key = key;
        built.kind = kind;
        *mounted = built;
        Ok(())
    }

    pub(crate) const fn depth(&self) -> u32 {
        self.depth
    }

    pub(crate) const fn dirty(&self) -> &'a DirtyHandle {
        self.dirty
    }

    pub(crate) const fn force(&self) -> bool {
        self.force
    }
}
