//! The open view protocol: view values, view nodes, and mounted state.

use std::{any::Any, cell::RefCell, ops::ControlFlow, rc::Rc, sync::Arc};

use astrelis_ui_next::{NodeId, UiError, UiRoot};

use crate::{
    ComponentServiceRequest, RoutedComponentAction, Theme,
    view::{ViewContext, ViewKey, ViewKind},
};
/// One lightweight typed-action view value.
pub struct AnyView<Action: 'static> {
    pub(crate) key: Option<ViewKey>,
    pub(crate) inner: Box<dyn ViewNode<Action>>,
}

/// Application-facing lightweight view description.
///
/// The `AnyView` name remains available for low-level integrations, while
/// component APIs use this alias to avoid exposing the erasure strategy.
pub type View<Action> = AnyView<Action>;

impl<Action: 'static> AnyView<Action> {
    /// Wraps a custom [`ViewNode`] as a view value.
    ///
    /// This is the entry point for a view kind defined outside this crate. The
    /// resulting value is indistinguishable from a builtin: it can be keyed,
    /// wrapped in modifiers, and placed in any container.
    pub fn new(node: impl ViewNode<Action>) -> Self {
        Self {
            key: None,
            inner: Box::new(node),
        }
    }

    /// Assigns stable identity for reconciliation inside a dynamic sequence.
    pub fn key(self, key: impl Into<ViewKey>) -> Self {
        self.keyed(key)
    }

    /// Assigns stable identity for reconciliation inside a dynamic sequence.
    pub fn keyed(mut self, key: impl Into<ViewKey>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Returns the type identity this view reconciles against.
    pub fn kind(&self) -> ViewKind {
        self.inner.kind()
    }
}
/// Converts static or collected child views into a container's child list.
pub trait IntoChildren<Action: 'static> {
    /// Produces child views in paint and focus order.
    fn into_children(self) -> Vec<AnyView<Action>>;
}

impl<Action: 'static> IntoChildren<Action> for Vec<AnyView<Action>> {
    fn into_children(self) -> Vec<AnyView<Action>> {
        self
    }
}

impl<Action: 'static, const N: usize> IntoChildren<Action> for [AnyView<Action>; N] {
    fn into_children(self) -> Vec<AnyView<Action>> {
        Vec::from(self)
    }
}

/// Collected dynamic children created by [`views`].
pub struct DynamicViews<Action: 'static>(Vec<AnyView<Action>>);

impl<Action: 'static> IntoChildren<Action> for DynamicViews<Action> {
    fn into_children(self) -> Vec<AnyView<Action>> {
        self.0
    }
}

/// Collects an iterator of dynamic views for use in a container.
pub fn views<Action: 'static>(
    children: impl IntoIterator<Item = AnyView<Action>>,
) -> DynamicViews<Action> {
    DynamicViews(children.into_iter().collect())
}

macro_rules! impl_tuple_children {
    ($($view:ident),+ $(,)?) => {
        impl<Action: 'static> IntoChildren<Action> for (
            $(impl_tuple_children!(@type $view Action),)+
        ) {
            #[allow(non_snake_case)]
            fn into_children(self) -> Vec<AnyView<Action>> {
                let ($($view,)+) = self;
                vec![$($view),+]
            }
        }
    };
    (@type $view:ident $action:ident) => {
        AnyView<$action>
    };
}

impl_tuple_children!(A);
impl_tuple_children!(A, B);
impl_tuple_children!(A, B, C);
impl_tuple_children!(A, B, C, D);
impl_tuple_children!(A, B, C, D, E);
impl_tuple_children!(A, B, C, D, E, F);
impl_tuple_children!(A, B, C, D, E, F, G);
impl_tuple_children!(A, B, C, D, E, F, G, H);
/// One kind of view: how a description becomes and stays retained state.
///
/// A view node is built once and reconciled thereafter. Both entry points take
/// `self: Box<Self>` so an implementation can move owned data - a `String`, a
/// `Vec`, an `Arc` - straight into the retained element instead of cloning it
/// out of a borrow.
///
/// # Contract
///
/// - [`build`](Self::build) must append exactly one retained node below
///   [`ViewContext::parent`] and return it as the [`Mounted`] node's identity,
///   or reuse a child's node and return that. A node that appends nothing and
///   has no child has no identity and cannot be reconciled.
/// - [`rebuild`](Self::rebuild) is called only when the mounted node's
///   [`ViewKind`] equals this view's, so [`Mounted::state_mut`] with this
///   view's own state type always succeeds. It must leave the retained tree
///   describing the new value, and it must not remove its own node.
/// - Invalidation is the implementation's responsibility: prefer
///   `context.ui().update(handle, exact_bits, ..)` over a blanket
///   [`astrelis_ui_next::Invalidation::ALL`], and skip the update entirely when nothing changed.
///
/// # Identity
///
/// The default [`kind`](Self::kind) is the implementing type, which is what a
/// third-party node wants: two of its own instances reconcile against each
/// other and never against a builtin or another crate's node. Override it only
/// to deliberately merge or split identity.
///
/// # Example
///
/// A container that lays its children out along the horizontal axis. It reaches
/// for nothing this crate keeps to itself: [`ViewContext::append`] gives it a
/// retained node, [`ViewContext::child`] scopes a context to that node, and
/// [`crate::MountedChildren`] does the keyed reconciliation.
///
/// ```
/// use std::{any::Any, ops::ControlFlow};
///
/// use rxui_core::{
///     AnyView, Mounted, MountedChildren, MountedState, ViewContext, ViewNode,
///     engine::{Axis, Flex, NodeHandle, UiError},
/// };
///
/// struct Strip<Action: 'static> {
///     children: Vec<AnyView<Action>>,
/// }
///
/// struct StripState<Action: 'static> {
///     handle: NodeHandle<Flex>,
///     children: MountedChildren<Action>,
/// }
///
/// impl<Action: 'static> MountedState<Action> for StripState<Action> {
///     fn as_any_mut(&mut self) -> &mut dyn Any {
///         self
///     }
///
///     fn visit_children(
///         &mut self,
///         visit: &mut dyn FnMut(&mut Mounted<Action>) -> ControlFlow<()>,
///     ) {
///         self.children.visit(visit);
///     }
/// }
///
/// impl<Action: 'static> ViewNode<Action> for Strip<Action> {
///     fn build(
///         self: Box<Self>,
///         context: &mut ViewContext<'_, Action>,
///     ) -> Result<Mounted<Action>, UiError> {
///         let handle = context.append(Flex {
///             axis: Axis::Horizontal,
///             ..Flex::default()
///         })?;
///         let mut children = MountedChildren::new();
///         children.build(self.children, &mut context.child(handle.id()))?;
///         Ok(Mounted::new(handle.id(), StripState { handle, children }))
///     }
///
///     fn rebuild(
///         self: Box<Self>,
///         mounted: &mut Mounted<Action>,
///         context: &mut ViewContext<'_, Action>,
///     ) -> Result<(), UiError> {
///         let state = mounted.state_mut::<StripState<Action>>()?;
///         let parent = state.handle.id();
///         state
///             .children
///             .reconcile(self.children, &mut context.child(parent))
///     }
/// }
///
/// fn strip<Action: 'static>(children: Vec<AnyView<Action>>) -> AnyView<Action> {
///     AnyView::new(Strip { children })
/// }
/// ```
pub trait ViewNode<Action: 'static>: 'static {
    /// Returns the type identity this node reconciles against.
    fn kind(&self) -> ViewKind {
        ViewKind::of::<Self>()
    }

    /// Whether this node reports its own reconciliation to [`crate::diagnostics::ViewStats`].
    ///
    /// The framework records [`crate::diagnostics::ViewStats::record_node_rebuilt`] around every
    /// reconciliation, because for almost every node reconciling *is* the work.
    /// A node that may legitimately decide to do nothing at all - a component
    /// boundary whose props, theme revision, and dirty flag all agree with the
    /// previous pass - must return `true` here and record itself on the paths
    /// that really reconcile. Otherwise `nodes_rebuilt` grows with the number of
    /// *untouched* nodes, which is exactly the dependence that skipping exists
    /// to remove.
    fn records_own_rebuild(&self) -> bool {
        false
    }

    /// Mounts this view, appending retained state below the context's parent.
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError>;

    /// Reconciles this view against a mounted node of the same [`ViewKind`].
    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError>;
}

/// One mounted view node: retained identity plus the state that maintains it.
///
/// The framework stamps the key and the [`ViewKind`] onto every mounted node,
/// so a [`ViewNode`] implementation constructs one with [`Mounted::new`] and
/// never has to repeat its own identity. The state is erased; recover it with
/// [`Mounted::state_mut`].
pub struct Mounted<Action: 'static> {
    pub(crate) key: Option<ViewKey>,
    pub(crate) kind: ViewKind,
    pub(crate) node: NodeId,
    pub(crate) state: Box<dyn MountedState<Action>>,
}

impl<Action: 'static> Mounted<Action> {
    /// Creates a mounted node owning `state` and identified by `node`.
    ///
    /// `node` is the retained identity this view contributes to its parent's
    /// child list: either a node this view appended, or the node of the single
    /// child a transparent wrapper reuses.
    pub fn new(node: NodeId, state: impl MountedState<Action>) -> Self {
        Self {
            // Stamped by the framework immediately after `ViewNode::build`
            // returns, from the view value that produced this node.
            key: None,
            kind: ViewKind::of::<()>(),
            node,
            state: Box::new(state),
        }
    }

    /// Retained identity contributed to the parent's child list.
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Reconciliation identity assigned when this node was mounted.
    pub const fn kind(&self) -> ViewKind {
        self.kind
    }

    /// Key this node was mounted with, if it was part of a keyed sequence.
    pub fn key(&self) -> Option<&ViewKey> {
        self.key.as_ref()
    }

    /// Re-points this node at a different retained identity.
    ///
    /// Only a transparent wrapper needs this: when the single child it reuses is
    /// replaced rather than reconciled, the wrapper's own identity moves with
    /// it, and a parent that has already published the old node would otherwise
    /// keep a dangling child.
    pub fn set_node(&mut self, node: NodeId) {
        self.node = node;
    }

    /// Borrows this node's state as the concrete type that mounted it.
    ///
    /// A [`ViewNode`] reconciling its own mounted node can rely on this: the
    /// framework only calls [`ViewNode::rebuild`] when the kinds agree, and the
    /// kind is the view's own Rust type. The error is therefore a framework
    /// invariant violation rather than something a caller can provoke - it is
    /// returned instead of panicking because a view protocol should not be able
    /// to abort the host process.
    pub fn state_mut<S: MountedState<Action>>(&mut self) -> Result<&mut S, UiError> {
        self.state
            .as_any_mut()
            .downcast_mut::<S>()
            .ok_or_else(|| UiError::new("mounted view state does not match the view kind"))
    }

    /// Offers one routed action to this node's subtree.
    ///
    /// A [`MountedState`] that owns children of a *different* action type routes
    /// through this after mapping; see [`MountedState::route`].
    pub fn route(
        &mut self,
        action: &mut Option<RoutedComponentAction>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Action>, UiError> {
        self.state.route(action, context)
    }

    /// Rebuilds the nested component identified by `target`, if it lives here.
    pub fn rebuild_component(
        &mut self,
        target: u64,
        context: &mut RebuildContext<'_>,
    ) -> Result<bool, UiError> {
        self.state.rebuild_component(target, context)
    }
}

/// Host services available while a routed action unwinds the mounted tree.
///
/// Routing runs without the retained tree: an action is reduced into component
/// state, and the rebuild it implies happens afterwards. Anything a reducer
/// wants from the host therefore leaves through here.
pub struct RouteContext<'a> {
    pub(crate) services: &'a mut Vec<ComponentServiceRequest>,
}

impl RouteContext<'_> {
    /// Queues one host service request for the runtime to drain.
    pub fn request_service(&mut self, request: ComponentServiceRequest) {
        self.services.push(request);
    }
}

/// Retained-tree access for the depth-ordered dirty drain.
///
/// A dirty nested component carries everything else it needs - its retained
/// parent handle, its action sink, its depth - in its own mounted state, so a
/// scoped rebuild only needs the tree, the theme, and this flush's force flag.
pub struct RebuildContext<'a> {
    pub(crate) ui: &'a mut UiRoot,
    pub(crate) theme: &'a Theme,
    pub(crate) force: bool,
}

impl<'a> RebuildContext<'a> {
    /// Borrows the retained tree.
    pub fn ui(&mut self) -> &mut UiRoot {
        self.ui
    }

    /// Reads the theme this flush resolves styles against.
    pub const fn theme(&self) -> &'a Theme {
        self.theme
    }

    /// Whether this flush must ignore props equality at component boundaries.
    pub const fn force(&self) -> bool {
        self.force
    }
}

/// Retained state owned by one mounted view node.
///
/// Most implementations are a handle, the values last written to the retained
/// element, and any mounted children. The three trait methods exist so the
/// framework can walk *through* a node it knows nothing about: a leaf
/// implements only [`as_any_mut`](Self::as_any_mut), a container implements
/// [`visit_children`](Self::visit_children) and gets the other two for free.
pub trait MountedState<Action: 'static>: Any {
    /// Returns this state for typed recovery by [`Mounted::state_mut`].
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Visits each mounted child in paint order, stopping on `Break`.
    ///
    /// This is how the framework reaches components nested anywhere below a
    /// third-party node: both [`route`](Self::route) and
    /// [`rebuild_component`](Self::rebuild_component) are defined in terms of
    /// it. A container that does not report its children here silently
    /// swallows its descendants' actions and leaves them stale.
    ///
    /// A node whose children carry a *different* action type cannot report them
    /// here, because they are not `Mounted<Action>`. It must override
    /// [`route`](Self::route) and [`rebuild_component`](Self::rebuild_component)
    /// instead, mapping in and out as it descends.
    fn visit_children(&mut self, _visit: &mut dyn FnMut(&mut Mounted<Action>) -> ControlFlow<()>) {}

    /// Offers one routed action to this node and its subtree.
    ///
    /// The action is taken from `action` by the component that owns it, so a
    /// container stops descending as soon as `action` is `None`. Anything
    /// returned is a *parent* action produced while unwinding - a nested
    /// component's mapped effects - and is reduced by the enclosing component.
    ///
    /// The default descends through [`visit_children`](Self::visit_children).
    fn route(
        &mut self,
        action: &mut Option<RoutedComponentAction>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Action>, UiError> {
        let mut output = Vec::new();
        let mut failure = None;
        self.visit_children(&mut |child| match child.route(action, context) {
            Ok(actions) => {
                output.extend(actions);
                if action.is_none() {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            }
            Err(error) => {
                failure = Some(error);
                ControlFlow::Break(())
            }
        });
        match failure {
            Some(error) => Err(error),
            None => Ok(output),
        }
    }

    /// Rebuilds the nested component identified by `target`, if it lives here.
    ///
    /// Returns whether the target was found, which lets a container stop
    /// descending as soon as the owning subtree has been handled. Leaves cannot
    /// own a component boundary and never find anything.
    ///
    /// The default descends through [`visit_children`](Self::visit_children).
    fn rebuild_component(
        &mut self,
        target: u64,
        context: &mut RebuildContext<'_>,
    ) -> Result<bool, UiError> {
        let mut found = false;
        let mut failure = None;
        self.visit_children(
            &mut |child| match child.rebuild_component(target, context) {
                Ok(true) => {
                    found = true;
                    ControlFlow::Break(())
                }
                Ok(false) => ControlFlow::Continue(()),
                Err(error) => {
                    failure = Some(error);
                    ControlFlow::Break(())
                }
            },
        );
        match failure {
            Some(error) => Err(error),
            None => Ok(found),
        }
    }
}
/// Implements [`MountedState`] for a state that owns no child views.
///
/// Every path in the expansion is absolute, so a view kind can invoke this
/// without importing the protocol items the impl happens to name.
macro_rules! leaf_mounted_state {
    ($state:ident $(<$param:ident>)? $(where Action: $bound:path)?) => {
        impl<Action: 'static $(+ $bound)?> $crate::MountedState<Action>
            for $state $(<$param>)?
        {
            fn as_any_mut(&mut self) -> &mut dyn ::std::any::Any {
                self
            }
        }
    };
}

/// Implements [`MountedState`] for a wrapper that owns exactly one child view.
macro_rules! wrapper_mounted_state {
    ($state:ident $(where Action: $bound:path)?) => {
        impl<Action: 'static $(+ $bound)?> $crate::MountedState<Action> for $state<Action> {
            fn as_any_mut(&mut self) -> &mut dyn ::std::any::Any {
                self
            }

            fn visit_children(
                &mut self,
                visit: &mut dyn FnMut(
                    &mut $crate::Mounted<Action>,
                ) -> ::std::ops::ControlFlow<()>,
            ) {
                let _ = visit(&mut self.child);
            }
        }
    };
}

/// Implements [`MountedState`] for a container that owns a reconciled child list.
macro_rules! container_mounted_state {
    ($state:ident) => {
        impl<Action: 'static> $crate::MountedState<Action> for $state<Action> {
            fn as_any_mut(&mut self) -> &mut dyn ::std::any::Any {
                self
            }

            fn visit_children(
                &mut self,
                visit: &mut dyn FnMut(&mut $crate::Mounted<Action>) -> ::std::ops::ControlFlow<()>,
            ) {
                self.children.visit(visit);
            }
        }
    };
}

pub(crate) use {container_mounted_state, leaf_mounted_state, wrapper_mounted_state};

/// Typed action bridge handed to retained elements and custom view nodes.
///
/// An emitter erases one typed action so the runtime can route it back to the
/// component that owns it. Its identity is stable for the lifetime of a mounted
/// node, which is what lets an element install a callback once at mount instead
/// of on every pass; [`ActionEmitter::ptr_eq`] is how a cache checks that.
pub struct ActionEmitter<Action: 'static> {
    sink: Arc<dyn Fn(Action) -> Box<dyn Any>>,
}

impl<Action: 'static> Clone for ActionEmitter<Action> {
    fn clone(&self) -> Self {
        Self {
            sink: self.sink.clone(),
        }
    }
}

impl<Action: 'static> ActionEmitter<Action> {
    pub(crate) fn from_sink(sink: Arc<dyn Fn(Action) -> Box<dyn Any>>) -> Self {
        Self { sink }
    }

    /// Erases one typed action for routing to its owning component.
    pub fn emit(&self, action: Action) -> Box<dyn Any> {
        (self.sink)(action)
    }

    /// Derives an emitter for a nested action vocabulary.
    ///
    /// The result composes `map` with this emitter, so a subtree can speak its
    /// own action type while its actions still reach this emitter's component.
    /// Build one per mounted node, not per pass.
    pub fn map<Child: 'static>(
        &self,
        map: impl Fn(Child) -> Action + 'static,
    ) -> ActionEmitter<Child> {
        let sink = self.sink.clone();
        ActionEmitter {
            sink: Arc::new(move |action| sink(map(action))),
        }
    }

    /// Whether two emitters are the same allocation.
    ///
    /// Action mappings are closures and cannot be compared, so identity is the
    /// only thing a cache can key on. Two emitters that compare equal here
    /// deliver to the same component through the same composition.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.sink, &other.sink)
    }
}
/// Mutable inputs of one retained control's change callback.
struct MapTarget<Input: 'static, Action: 'static> {
    map: Arc<dyn Fn(Input) -> Action>,
    emitter: ActionEmitter<Action>,
}

/// Stable indirection behind a control's change callback.
///
/// A retained control owns a boxed erased factory. Reinstalling it on every
/// rebuild allocated a closure and a box per control per frame for a value that
/// virtually never changes. Instead the element captures a clone of this cell
/// once, at mount, and reads the current mapping through it; a rebuild only
/// writes the cell.
pub(crate) struct MapCell<Input: 'static, Action: 'static>(Rc<RefCell<MapTarget<Input, Action>>>);

impl<Input: 'static, Action: 'static> MapCell<Input, Action> {
    pub(crate) fn new(map: Arc<dyn Fn(Input) -> Action>, emitter: ActionEmitter<Action>) -> Self {
        Self(Rc::new(RefCell::new(MapTarget { map, emitter })))
    }

    /// Returns the closure installed into the retained element exactly once.
    pub(crate) fn emitter(&self) -> impl Fn(Input) -> Box<dyn Any> + use<Input, Action> {
        let cell = self.0.clone();
        move |input| {
            // Cloned out before being called so a user mapping can never observe
            // this cell mid-borrow.
            let (map, emitter) = {
                let target = cell.borrow();
                (target.map.clone(), target.emitter.clone())
            };
            emitter.emit(map(input))
        }
    }

    /// Points the cell at this frame's mapping without touching the element.
    pub(crate) fn update(
        &self,
        map: Arc<dyn Fn(Input) -> Action>,
        emitter: &ActionEmitter<Action>,
    ) {
        let mut target = self.0.borrow_mut();
        target.map = map;
        if !target.emitter.ptr_eq(emitter) {
            target.emitter = emitter.clone();
        }
    }
}

/// Mutable inputs of one retained control's activation action.
struct ActionTarget<Action: 'static> {
    action: Action,
    emitter: ActionEmitter<Action>,
}

/// Stable indirection behind an activation action carried by value.
pub(crate) struct ActionCell<Action: Clone + 'static>(Rc<RefCell<ActionTarget<Action>>>);

impl<Action: Clone + 'static> ActionCell<Action> {
    pub(crate) fn new(action: Action, emitter: ActionEmitter<Action>) -> Self {
        Self(Rc::new(RefCell::new(ActionTarget { action, emitter })))
    }

    /// Returns the closure installed into the retained element exactly once.
    pub(crate) fn emitter(&self) -> impl Fn() -> Box<dyn Any> + use<Action> {
        let cell = self.0.clone();
        move || {
            let (action, emitter) = {
                let target = cell.borrow();
                (target.action.clone(), target.emitter.clone())
            };
            emitter.emit(action)
        }
    }

    /// Points the cell at this frame's action without touching the element.
    pub(crate) fn update(&self, action: Action, emitter: &ActionEmitter<Action>) {
        let mut target = self.0.borrow_mut();
        target.action = action;
        if !target.emitter.ptr_eq(emitter) {
            target.emitter = emitter.clone();
        }
    }
}
