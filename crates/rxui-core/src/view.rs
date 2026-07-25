//! Lightweight view values, the open view protocol, and keyed reconciliation.
//!
//! A [`View`] is a description, not mounted state. Building one allocates a
//! small tree of [`ViewNode`] values; mounting it walks that tree once and
//! produces a parallel tree of [`Mounted`] nodes that own retained identity.
//! Every later pass reconciles a fresh description against the mounted tree.
//!
//! The protocol is open: the twenty view kinds this module ships are written
//! against exactly the surface a third-party crate gets. See
//! [`ViewNode`] for the contract and `docs/view-protocol.md` for the guide.

use std::{
    any::{Any, TypeId},
    cell::RefCell,
    cmp::Ordering as CmpOrdering,
    collections::HashMap,
    fmt,
    hash::{Hash, Hasher},
    ops::{ControlFlow, RangeInclusive},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use astrelis_core::{
    color::Color,
    geometry::{LogicalPoint, LogicalSize},
};
use astrelis_ui_next::{
    Align, Alignment, Axis, BoxElement, Button, ButtonIcon, Checkbox, Element, Flex, Frame,
    Invalidation, KeyListener, Label, NodeHandle, NodeId, Scroll, ScrollAxis, SemanticData, Slider,
    SplitPane, Stack, TextField, UiError, UiRoot,
};

use crate::{
    ButtonVariant, ColorRole, ComponentContext, ComponentServiceRequest, ComponentWithProps,
    DirtyHandle, Icon, IconButtonStyle, RenderScope, RoutedComponentAction, Space, Theme,
    diagnostics::ViewStats,
};

static NEXT_COMPONENT_ID: AtomicU64 = AtomicU64::new(1);

/// Stable view identity used by dynamic collections.
///
/// Keys are compared and hashed on every keyed reconciliation pass, so the
/// representation avoids allocating for the two idiomatic sources of identity: a
/// numeric domain id (`.key(item.id)`) and a `&'static str` literal
/// (`.key("header")`). Only an owned or borrowed non-static string allocates,
/// and then only once per key rather than once per frame.
///
/// String-shaped keys compare and hash by their text regardless of whether they
/// are borrowed or owned, so a collection keyed with a literal one frame and a
/// `String` the next keeps its retained identity. [`ViewKey::Index`] is a
/// distinct space: `ViewKey::from(7u64)` is *not* equal to `ViewKey::from("7")`.
#[derive(Clone, Debug)]
pub enum ViewKey {
    /// Numeric domain identity.
    Index(u64),
    /// Borrowed static identity, typically a literal.
    Name(&'static str),
    /// Shared owned identity.
    Owned(Arc<str>),
}

impl ViewKey {
    /// Creates an owned key.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self::Owned(value.into())
    }

    /// Returns the textual form of a string-shaped key.
    fn text(&self) -> Option<&str> {
        match self {
            Self::Index(_) => None,
            Self::Name(value) => Some(value),
            Self::Owned(value) => Some(value),
        }
    }
}

impl PartialEq for ViewKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Index(left), Self::Index(right)) => left == right,
            (Self::Index(_), _) | (_, Self::Index(_)) => false,
            (left, right) => left.text() == right.text(),
        }
    }
}

impl Eq for ViewKey {}

impl Hash for ViewKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // The discriminant byte keeps the numeric and textual spaces apart; both
        // textual variants hash as `str` so they collide only when equal.
        match self {
            Self::Index(value) => {
                0u8.hash(state);
                value.hash(state);
            }
            Self::Name(value) => {
                1u8.hash(state);
                (*value).hash(state);
            }
            Self::Owned(value) => {
                1u8.hash(state);
                (**value).hash(state);
            }
        }
    }
}

impl PartialOrd for ViewKey {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ViewKey {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        match (self, other) {
            (Self::Index(left), Self::Index(right)) => left.cmp(right),
            (Self::Index(_), _) => CmpOrdering::Less,
            (_, Self::Index(_)) => CmpOrdering::Greater,
            (left, right) => left.text().cmp(&right.text()),
        }
    }
}

impl From<&'static str> for ViewKey {
    fn from(value: &'static str) -> Self {
        Self::Name(value)
    }
}

impl From<String> for ViewKey {
    fn from(value: String) -> Self {
        Self::Owned(Arc::<str>::from(value))
    }
}

impl From<Arc<str>> for ViewKey {
    fn from(value: Arc<str>) -> Self {
        Self::Owned(value)
    }
}

impl From<u64> for ViewKey {
    fn from(value: u64) -> Self {
        Self::Index(value)
    }
}

impl From<NodeId> for ViewKey {
    fn from(value: NodeId) -> Self {
        Self::Index(value.to_bits())
    }
}

impl fmt::Display for ViewKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index(value) => value.fmt(formatter),
            Self::Name(value) => value.fmt(formatter),
            Self::Owned(value) => value.fmt(formatter),
        }
    }
}

/// Type identity of one view kind.
///
/// Reconciliation pairs a fresh view with a mounted node only when their kinds
/// agree; a mismatch means the mounted subtree is removed and the new view is
/// mounted from scratch. The identity is the implementing Rust type, so two
/// distinct [`ViewNode`] types never reconcile against each other even when
/// their mounted state happens to have the same shape - which is what makes
/// [`Mounted::state_mut`] a lookup rather than a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewKind(TypeId);

impl ViewKind {
    /// Returns the kind identifying `T`.
    pub fn of<T: ?Sized + 'static>() -> Self {
        Self(TypeId::of::<T>())
    }
}

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

    /// Nests a view with a local action type and maps it into its parent.
    pub fn map_action<Parent: 'static>(
        self,
        map: impl Fn(Action) -> Parent + 'static,
    ) -> AnyView<Parent> {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(MapActionView {
                child: self,
                map: Arc::new(map),
            }),
        }
    }

    /// Controls interaction for this view and its retained descendants.
    pub fn enabled(self, enabled: bool) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(EnabledView {
                child: self,
                enabled,
            }),
        }
    }

    /// Controls retained visibility without discarding subtree state.
    pub fn visible(self, visible: bool) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(VisibleView {
                child: self,
                visible,
            }),
        }
    }

    /// Wraps this view in an explicit sizing and flex-growth boundary.
    pub fn frame(self, style: FrameStyle) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(FrameView { child: self, style }),
        }
    }

    /// Expands to available space and positions this view within it.
    pub fn aligned(self, alignment: Alignment, padding: Space) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(AlignView {
                child: self,
                alignment,
                padding,
            }),
        }
    }

    /// Autofocuses this subtree while active and restores prior focus on exit.
    pub fn focus_scope(self, active: bool) -> Self {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(FocusScopeView {
                child: self,
                active,
            }),
        }
    }

    /// Handles Escape after an unhandled key event bubbles from this subtree.
    pub fn dismiss_on_escape(self, action: Action) -> Self
    where
        Action: Clone,
    {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(EscapeView {
                child: self,
                action,
            }),
        }
    }

    /// Handles list navigation and submission after focused-child bubbling.
    pub fn command_navigation(self, previous: Action, next: Action, submit: Action) -> Self
    where
        Action: Clone,
    {
        AnyView {
            key: self.key.clone(),
            inner: Box::new(CommandNavigationView {
                child: self,
                previous,
                next,
                submit,
            }),
        }
    }
}

/// Explicit sizing and flex-growth options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameStyle {
    /// Optional preferred width.
    pub width: Option<f32>,
    /// Optional preferred height.
    pub height: Option<f32>,
    /// Minimum size.
    pub min: LogicalSize,
    /// Optional maximum size.
    pub max: Option<LogicalSize>,
    /// Relative share of remaining flex space.
    pub grow: f32,
}

impl FrameStyle {
    /// Creates an unconstrained, non-growing frame.
    pub const fn new() -> Self {
        Self {
            width: None,
            height: None,
            min: LogicalSize::ZERO,
            max: None,
            grow: 0.0,
        }
    }

    /// Selects preferred width.
    pub const fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Selects preferred height.
    pub const fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Selects relative main-axis growth.
    pub const fn grow(mut self, grow: f32) -> Self {
        self.grow = grow;
        self
    }

    /// Selects minimum size.
    pub const fn min(mut self, min: LogicalSize) -> Self {
        self.min = min;
        self
    }

    /// Selects maximum size.
    pub const fn max(mut self, max: LogicalSize) -> Self {
        self.max = Some(max);
        self
    }
}

impl Default for FrameStyle {
    fn default() -> Self {
        Self::new()
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
///   [`Invalidation::ALL`], and skip the update entirely when nothing changed.
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
/// [`MountedChildren`] does the keyed reconciliation.
///
/// ```
/// use std::{any::Any, ops::ControlFlow};
///
/// use rxui_core::{
///     AnyView, Mounted, MountedChildren, MountedState, ViewContext, ViewNode,
///     core::{Axis, Flex, NodeHandle, UiError},
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

    /// Whether this node reports its own reconciliation to [`ViewStats`].
    ///
    /// The framework records [`ViewStats::record_node_rebuilt`] around every
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
macro_rules! leaf_mounted_state {
    ($state:ident $(<$param:ident>)? $(where Action: $bound:path)?) => {
        impl<Action: 'static $(+ $bound)?> MountedState<Action> for $state $(<$param>)? {
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }
    };
}

/// Implements [`MountedState`] for a wrapper that owns exactly one child view.
macro_rules! wrapper_mounted_state {
    ($state:ident $(where Action: $bound:path)?) => {
        impl<Action: 'static $(+ $bound)?> MountedState<Action> for $state<Action> {
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }

            fn visit_children(
                &mut self,
                visit: &mut dyn FnMut(&mut Mounted<Action>) -> ControlFlow<()>,
            ) {
                let _ = visit(&mut self.child);
            }
        }
    };
}

/// Implements [`MountedState`] for a container that owns a reconciled child list.
macro_rules! container_mounted_state {
    ($state:ident) => {
        impl<Action: 'static> MountedState<Action> for $state<Action> {
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }

            fn visit_children(
                &mut self,
                visit: &mut dyn FnMut(&mut Mounted<Action>) -> ControlFlow<()>,
            ) {
                self.children.visit(visit);
            }
        }
    };
}

/// Everything a [`ViewNode`] needs to reach the retained tree.
///
/// A context is always scoped to a parent node: [`append`](Self::append) and
/// child reconciliation attach below [`parent`](Self::parent). A view that
/// creates a container node re-scopes with [`child`](Self::child) before
/// mounting anything under it.
///
/// Descending is done through [`build_child`](Self::build_child) and
/// [`rebuild_child`](Self::rebuild_child) rather than by calling
/// [`ViewNode::build`] directly. They stamp identity, decide between
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

/// Mounts and incrementally reconciles one root view.
pub struct ViewHost<Action: 'static> {
    mounted: Mounted<Action>,
    /// Hoisted root action bridge.
    ///
    /// The root component's actions need no routing wrapper, so this is a single
    /// boxing closure allocated at mount instead of one per rebuild.
    emitter: ActionEmitter<Action>,
}

impl<Action: 'static> ViewHost<Action> {
    /// Builds the initial retained subtree.
    pub(crate) fn mount(
        ui: &mut UiRoot,
        theme: &Theme,
        view: AnyView<Action>,
        dirty: &DirtyHandle,
    ) -> Result<Self, UiError> {
        let emitter = ActionEmitter::from_sink(Arc::new(|action| Box::new(action)));
        let parent = ui.root();
        let mut context = ViewContext::new(ui, parent, theme, emitter.clone(), 0, dirty, false);
        let mounted = context.build_child(view)?;
        Ok(Self { mounted, emitter })
    }

    /// Reconciles a replacement root view into retained state.
    pub(crate) fn rebuild(
        &mut self,
        ui: &mut UiRoot,
        theme: &Theme,
        view: AnyView<Action>,
        dirty: &DirtyHandle,
        force: bool,
    ) -> Result<(), UiError> {
        let parent = ui.root();
        let mut context =
            ViewContext::new(ui, parent, theme, self.emitter.clone(), 0, dirty, force);
        context.rebuild_child(&mut self.mounted, view)
    }

    /// Root retained identity.
    pub const fn node(&self) -> NodeId {
        self.mounted.node
    }

    /// Rebuilds one dirty nested component without touching the root's view.
    ///
    /// A component that has since been unmounted is simply not found, which is
    /// why this reports nothing: a stale dirty entry is not an error.
    pub(crate) fn rebuild_scoped(
        &mut self,
        ui: &mut UiRoot,
        theme: &Theme,
        target: u64,
        force: bool,
    ) -> Result<(), UiError> {
        let mut context = RebuildContext { ui, theme, force };
        self.mounted.rebuild_component(target, &mut context)?;
        Ok(())
    }

    pub(crate) fn route(
        &mut self,
        action: RoutedComponentAction,
        services: &mut Vec<ComponentServiceRequest>,
    ) -> Result<Vec<Action>, UiError> {
        let mut context = RouteContext { services };
        self.mounted.route(&mut Some(action), &mut context)
    }
}

/// Creates a text view.
///
/// Text is carried as `Arc<str>` so that reconciling an unchanged label clones a
/// pointer instead of the string.
pub fn label<Action: 'static>(text: impl Into<Arc<str>>) -> AnyView<Action> {
    label_with_style(text, LabelStyle::default())
}

/// Creates a shaped text view with an optional preferred width.
pub fn label_with_width<Action: 'static>(
    text: impl Into<Arc<str>>,
    width: impl Into<Option<f32>>,
) -> AnyView<Action> {
    label_with_style(text, LabelStyle::default().width(width.into()))
}

/// Typed label presentation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelStyle {
    /// Logical font size.
    pub font_size: f32,
    /// Semantic text color.
    pub role: ColorRole,
    /// Optional preferred width.
    pub width: Option<f32>,
}

impl LabelStyle {
    /// Creates standard body-label presentation.
    pub const fn standard() -> Self {
        Self {
            font_size: 14.0,
            role: ColorRole::Text,
            width: None,
        }
    }

    /// Selects the logical font size.
    pub const fn font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    /// Selects a semantic text color.
    pub const fn role(mut self, role: ColorRole) -> Self {
        self.role = role;
        self
    }

    /// Selects an optional preferred width.
    pub const fn width(mut self, width: Option<f32>) -> Self {
        self.width = width;
        self
    }
}

impl Default for LabelStyle {
    fn default() -> Self {
        Self::standard()
    }
}

/// Creates a text view with typed presentation.
pub fn label_with_style<Action: 'static>(
    text: impl Into<Arc<str>>,
    style: LabelStyle,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(LabelView {
            text: text.into(),
            font_size: style.font_size.max(1.0),
            role: style.role,
            width: style.width,
        }),
    }
}

/// Creates a semantic colored box.
pub fn panel<Action: 'static>(
    size: LogicalSize,
    role: ColorRole,
    semantics: Option<SemanticData>,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(BoxView {
            size,
            role,
            semantics,
            interactive: false,
        }),
    }
}

/// Typed presentation options for a button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
    /// Semantic color treatment.
    pub variant: ButtonVariant,
    /// Preferred logical size.
    pub size: LogicalSize,
}

impl ButtonStyle {
    /// Creates standard button presentation.
    pub const fn standard() -> Self {
        Self {
            variant: ButtonVariant::Standard,
            size: LogicalSize::new(0.0, 30.0),
        }
    }

    /// Selects a semantic variant.
    pub const fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Selects the preferred logical size.
    pub const fn size(mut self, size: LogicalSize) -> Self {
        self.size = size;
        self
    }
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self::standard()
    }
}

/// Creates an activatable typed-action button.
pub fn button<Action: Clone + 'static>(text: impl Into<String>, action: Action) -> AnyView<Action> {
    button_with(text, action, ButtonStyle::standard())
}

/// Creates a button with typed presentation options.
pub fn button_with<Action: Clone + 'static>(
    text: impl Into<String>,
    action: Action,
    style: ButtonStyle,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(ButtonView {
            text: text.into(),
            action,
            variant: style.variant,
            size: style.size,
            icon: None,
            icon_size: 0.0,
            show_label: true,
        }),
    }
}

pub(crate) fn icon_button_view<Action: Clone + 'static>(
    icon: Icon,
    label: String,
    action: Action,
    style: IconButtonStyle,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(ButtonView {
            text: label,
            action,
            variant: style.button.variant,
            size: style.button.size,
            icon: Some(icon),
            icon_size: style.icon_size,
            show_label: style.show_label,
        }),
    }
}

/// Creates a controlled editable text field.
pub fn text_field<Action: 'static>(
    label: impl Into<String>,
    value: impl Into<String>,
    on_changed: impl Fn(String) -> Action + 'static,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(TextFieldView {
            label: label.into(),
            value: value.into(),
            on_changed: Arc::new(on_changed),
        }),
    }
}

/// Creates a controlled boolean checkbox.
pub fn checkbox<Action: 'static>(
    label: impl Into<String>,
    checked: bool,
    on_changed: impl Fn(bool) -> Action + 'static,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(CheckboxView {
            label: label.into(),
            checked,
            on_changed: Arc::new(on_changed),
        }),
    }
}

/// Creates a controlled horizontal numeric slider.
pub fn slider<Action: 'static>(
    label: impl Into<String>,
    value: f32,
    range: RangeInclusive<f32>,
    on_changed: impl Fn(f32) -> Action + 'static,
) -> View<Action> {
    slider_with_step(label, value, range, 1.0, on_changed)
}

/// Creates a controlled slider with an explicit keyboard step.
pub fn slider_with_step<Action: 'static>(
    label: impl Into<String>,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    on_changed: impl Fn(f32) -> Action + 'static,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(SliderView {
            label: label.into(),
            value,
            range,
            step: step.max(0.0),
            on_changed: Arc::new(on_changed),
        }),
    }
}

/// Mounts an independently state-owning child component.
pub fn component<C, Parent>(
    props: C::Props,
    map_effect: impl Fn(C::Effect) -> Parent + 'static,
) -> View<Parent>
where
    C: ComponentWithProps,
    Parent: 'static,
{
    AnyView {
        key: None,
        inner: Box::new(ComponentView::<C, Parent> {
            props,
            map_effect: Arc::new(map_effect),
        }),
    }
}

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

/// Configuration contract for a specialized retained element.
///
/// This is the escape hatch for charts, render viewports, docking surfaces,
/// and other workloads whose interaction state should remain imperative.
///
/// # Two disjoint phases
///
/// A pass over a spec does two things that never overlap: it writes the element,
/// then it reconciles children. The element write happens *inside* the engine's
/// `update`, which hands out `&mut Element` and nothing else - which is why
/// [`create`](Self::create) and [`update`](Self::update) receive an
/// [`ActionEmitter`] and a [`Theme`] rather than a context. Children are
/// reconciled afterwards, with the whole tree available, from
/// [`children`](Self::children).
pub trait RetainedSpec<Action: 'static>: Clone + 'static {
    /// Concrete retained element owned by the incremental tree.
    type Element: Element;

    /// Creates newly mounted retained state.
    fn create(&self, emitter: &ActionEmitter<Action>, theme: &Theme) -> Self::Element;

    /// Applies changed configuration without discarding interaction state.
    ///
    /// Called on every pass, including one where [`changed`](Self::changed)
    /// reported nothing: the element still has to be pointed at this frame's
    /// callbacks. Only the invalidation is conditional.
    fn update(&self, element: &mut Self::Element, emitter: &ActionEmitter<Action>, theme: &Theme);

    /// Reports which retained passes this configuration change requires.
    ///
    /// An empty [`Invalidation`] means the element is visually and semantically
    /// identical to `previous` and no pass has to run over it. Reporting more
    /// than changed is merely slow; reporting less leaves a stale frame, so
    /// widen when in doubt - [`Invalidation::ALL`] is always correct.
    fn changed(&self, previous: &Self) -> Invalidation;

    /// Produces child views hosted inside this element.
    ///
    /// Defaults to none, which is what a leaf wants. A container element returns
    /// its children here and gets full keyed reconciliation, nested components
    /// included, without implementing [`ViewNode`] itself.
    fn children(&self) -> Vec<AnyView<Action>> {
        Vec::new()
    }
}

/// Mounts a specialized retained element behind the reconciled view boundary.
pub fn retained<Action: 'static, Spec: RetainedSpec<Action>>(spec: Spec) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(RetainedView { spec }),
    }
}

struct RetainedView<Spec> {
    spec: Spec,
}

struct RetainedState<Spec: RetainedSpec<Action>, Action: 'static> {
    handle: NodeHandle<Spec::Element>,
    spec: Spec,
    children: MountedChildren<Action>,
}

impl<Spec: RetainedSpec<Action>, Action: 'static> MountedState<Action>
    for RetainedState<Spec, Action>
{
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn visit_children(&mut self, visit: &mut dyn FnMut(&mut Mounted<Action>) -> ControlFlow<()>) {
        self.children.visit(visit);
    }
}

impl<Spec: RetainedSpec<Action>, Action: 'static> ViewNode<Action> for RetainedView<Spec> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let spec = self.spec;
        let emitter = context.emitter();
        let element = spec.create(&emitter, context.theme());
        let handle = context.append(element)?;
        let mut children = MountedChildren::new();
        let child_views = spec.children();
        if !child_views.is_empty() {
            children.build(child_views, &mut context.child(handle.id()))?;
        }
        Ok(Mounted::new(
            handle.id(),
            RetainedState::<Spec, Action> {
                handle,
                spec,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let spec = self.spec;
        let emitter = context.emitter();
        let theme = context.theme();
        let state = mounted.state_mut::<RetainedState<Spec, Action>>()?;
        let handle = state.handle;
        let invalidation = spec.changed(&state.spec);
        let child_views = spec.children();
        // The element write and the child reconciliation are deliberately
        // sequential: `update` holds `&mut Element`, so nothing that needs the
        // tree can run inside it.
        context.ui().update(handle, invalidation, |element| {
            spec.update(element, &emitter, theme);
        })?;
        state.spec = spec;
        if !(child_views.is_empty() && state.children.is_empty()) {
            let mut child_context = context.child(handle.id());
            state.children.reconcile(child_views, &mut child_context)?;
        }
        Ok(())
    }
}

struct ComponentView<C: ComponentWithProps, Parent: 'static> {
    props: C::Props,
    map_effect: Arc<dyn Fn(C::Effect) -> Parent>,
}

struct ComponentState<C: ComponentWithProps, Parent: 'static> {
    id: u64,
    /// Component nesting depth, used to order the dirty drain.
    depth: u32,
    parent: NodeHandle<Flex>,
    props: C::Props,
    component: C,
    child: Mounted<C::Action>,
    map_effect: Arc<dyn Fn(C::Effect) -> Parent>,
    /// Hoisted routing bridge handed to every retained element in this subtree.
    ///
    /// Allocated once per mounted instance. It used to be rebuilt on every
    /// rebuild of the subtree, which also meant no two frames ever agreed on
    /// sink identity.
    emitter: ActionEmitter<C::Action>,
    /// Hoisted routing sink for erased service completions.
    service_route: Arc<dyn Fn(Box<dyn Any>) -> crate::ServiceAction>,
    /// Dirty set shared with the owning [`crate::ComponentRuntime`].
    dirty: DirtyHandle,
    /// [`Theme::revision`] this instance last rendered at.
    theme_revision: u64,
}

impl<C: ComponentWithProps, Parent: 'static> ComponentState<C, Parent> {
    fn emitter(id: u64) -> ActionEmitter<C::Action> {
        ActionEmitter::from_sink(Arc::new(move |action| {
            Box::new(RoutedComponentAction {
                target: id,
                payload: Box::new(action),
            })
        }))
    }

    fn service_sink(id: u64) -> Arc<dyn Fn(Box<dyn Any>) -> crate::ServiceAction> {
        Arc::new(move |payload| {
            Box::new(RoutedComponentAction {
                target: id,
                payload,
            })
        })
    }

    fn rebuild_child(
        &mut self,
        ui: &mut UiRoot,
        theme: &Theme,
        force: bool,
    ) -> Result<(), UiError> {
        // `component_views`: one nested component re-render. Reached only when
        // this instance's props, theme revision, or own dirty flag says its
        // output can have changed.
        ViewStats::record_component_view();
        let view = self.component.view(theme);
        self.theme_revision = theme.revision;
        let mut context = ViewContext::new(
            ui,
            self.parent.id(),
            theme,
            self.emitter.clone(),
            self.depth,
            &self.dirty,
            force,
        );
        context.rebuild_child(&mut self.child, view)
    }

    fn reduce(
        &mut self,
        actions: Vec<C::Action>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Parent>, UiError> {
        if actions.is_empty() {
            return Ok(Vec::new());
        }
        let mut effects = Vec::new();
        for action in actions {
            self.component.update(
                action,
                &mut ComponentContext::new(
                    &mut effects,
                    context.services,
                    self.service_route.clone(),
                    RenderScope::nested(&self.dirty, self.depth, self.id),
                ),
            );
        }
        // Reducing does not rebuild. Anything this component's view reads may
        // have changed, so it records itself as stale and lets the runtime's
        // depth-ordered flush decide whether the parent's cascade already
        // covers it.
        self.dirty.mark(self.depth, self.id);
        Ok(effects
            .into_iter()
            .map(|effect| (self.map_effect)(effect))
            .collect())
    }
}

impl<C: ComponentWithProps, Parent: 'static> MountedState<Parent> for ComponentState<C, Parent> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Routes across the action-type boundary this component introduces.
    ///
    /// The child subtree speaks `C::Action`, so it cannot be reported through
    /// `visit_children`. An action addressed to this instance is taken here;
    /// anything else descends, and whatever the child's reducer emits is mapped
    /// into the parent's vocabulary on the way out.
    fn route(
        &mut self,
        action: &mut Option<RoutedComponentAction>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Parent>, UiError> {
        if action
            .as_ref()
            .is_some_and(|action| action.target == self.id)
        {
            let action = action.take().expect("targeted action exists");
            let action = action
                .payload
                .downcast::<C::Action>()
                .map_err(|_| UiError::new("nested component action type mismatch"))?;
            return self.reduce(vec![*action], context);
        }
        let actions = self.child.route(action, context)?;
        self.reduce(actions, context)
    }

    fn rebuild_component(
        &mut self,
        target: u64,
        context: &mut RebuildContext<'_>,
    ) -> Result<bool, UiError> {
        if self.id == target {
            self.rebuild_child(context.ui, context.theme, context.force)?;
            return Ok(true);
        }
        self.child.rebuild_component(target, context)
    }
}

impl<C: ComponentWithProps, Parent: 'static> ViewNode<Parent> for ComponentView<C, Parent> {
    /// Reports its own reconciliation, because it may do nothing at all.
    ///
    /// A boundary whose props, theme revision, and dirty flag all agree with the
    /// previous pass touches neither the retained tree nor its own state.
    /// Letting the framework count it as a rebuilt node would make
    /// `nodes_rebuilt` grow with the number of *untouched* sibling components -
    /// exactly the dependence update isolation exists to remove.
    fn records_own_rebuild(&self) -> bool {
        true
    }

    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<Mounted<Parent>, UiError> {
        let id = NEXT_COMPONENT_ID.fetch_add(1, Ordering::Relaxed);
        let depth = context.depth() + 1;
        // Read out before the tree is borrowed exclusively: `ViewContext::theme`
        // hands back a borrow of the theme itself, not of the context.
        let theme = context.theme();
        let force = context.force();
        let dirty = context.dirty().clone();
        let parent = context.append(Flex::default())?;
        let component = C::create(&self.props);
        // `component_views`: a newly mounted nested component's first view.
        ViewStats::record_component_view();
        let view = component.view(theme);
        let emitter = ComponentState::<C, Parent>::emitter(id);
        let mut child_context = ViewContext::new(
            context.ui(),
            parent.id(),
            theme,
            emitter.clone(),
            depth,
            &dirty,
            force,
        );
        let child = child_context.build_child(view)?;
        Ok(Mounted::new(
            parent.id(),
            ComponentState::<C, Parent> {
                id,
                depth,
                parent,
                props: self.props,
                component,
                child,
                map_effect: self.map_effect,
                emitter,
                service_route: ComponentState::<C, Parent>::service_sink(id),
                dirty,
                theme_revision: theme.revision,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Parent>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let force = context.force();
        let state = mounted.state_mut::<ComponentState<C, Parent>>()?;
        // The effect mapping is a fresh closure every frame and cannot be
        // compared, so it is always replaced. It is only read while routing an
        // action, never while rendering, so replacing it cannot make a skipped
        // subtree stale.
        state.map_effect = self.map_effect;
        let props_changed = state.props != self.props;
        if props_changed {
            state.component.changed(&self.props);
            state.props = self.props;
        }
        let theme_changed = state.theme_revision != theme.revision;
        // Taken unconditionally: a boundary refreshed here by its parent's
        // cascade must not be rebuilt a second time by the dirty drain.
        let self_dirty = state.dirty.take(state.depth, state.id);
        if !(props_changed || theme_changed || self_dirty || force) {
            return Ok(());
        }
        // `nodes_rebuilt`: the component boundary, counted only when it really
        // reconciles.
        ViewStats::record_node_rebuilt();
        state.rebuild_child(context.ui(), theme, force)
    }
}

struct MapActionView<Child: 'static, Parent: 'static> {
    child: AnyView<Child>,
    map: Arc<dyn Fn(Child) -> Parent>,
}

struct EnabledView<Action: 'static> {
    child: AnyView<Action>,
    enabled: bool,
}

struct FrameView<Action: 'static> {
    child: AnyView<Action>,
    style: FrameStyle,
}

struct FocusScopeView<Action: 'static> {
    child: AnyView<Action>,
    active: bool,
}

struct FocusScopeState<Action: 'static> {
    child: Mounted<Action>,
    active: bool,
    previous: Option<NodeId>,
}

wrapper_mounted_state!(FocusScopeState);

impl<Action: 'static> ViewNode<Action> for FocusScopeView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let previous = self
            .active
            .then(|| {
                context
                    .ui()
                    .focused()
                    .or_else(|| context.ui().last_focused())
            })
            .flatten();
        let child = context.build_child(self.child)?;
        if self.active {
            context.ui().focus_first_in_subtree(child.node())?;
        }
        let node = child.node();
        Ok(Mounted::new(
            node,
            FocusScopeState {
                child,
                active: self.active,
                previous,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<FocusScopeState<Action>>()?;
        let activating = !state.active && self.active;
        let deactivating = state.active && !self.active;
        let previous = activating
            .then(|| {
                context
                    .ui()
                    .focused()
                    .or_else(|| context.ui().last_focused())
            })
            .flatten();
        context.rebuild_child(&mut state.child, self.child)?;
        let child_node = state.child.node();
        if activating {
            state.previous = previous;
            context.ui().focus_first_in_subtree(child_node)?;
        } else if deactivating {
            let restored = state
                .previous
                .filter(|previous| context.ui().contains(*previous))
                .is_some_and(|previous| context.ui().set_focus(Some(previous)).is_ok());
            if !restored {
                context.ui().set_focus(None)?;
            }
            state.previous = None;
        }
        state.active = self.active;
        mounted.set_node(child_node);
        Ok(())
    }
}

struct EscapeView<Action: Clone + 'static> {
    child: AnyView<Action>,
    action: Action,
}

struct EscapeState<Action: Clone + 'static> {
    handle: NodeHandle<KeyListener>,
    child: Mounted<Action>,
    action: ActionCell<Action>,
}

wrapper_mounted_state!(EscapeState where Action: Clone);

impl<Action: Clone + 'static> ViewNode<Action> for EscapeView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let action = ActionCell::new(self.action, context.emitter());
        let emit = action.emitter();
        let handle = context.append(KeyListener::on_escape(move || emit()))?;
        let child = context.child(handle.id()).build_child(self.child)?;
        Ok(Mounted::new(
            handle.id(),
            EscapeState {
                handle,
                child,
                action,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let emitter = context.emitter();
        let state = mounted.state_mut::<EscapeState<Action>>()?;
        // Writing the cell replaces the dismissal action without reinstalling a
        // listener closure, which is why no retained update happens here at all.
        state.action.update(self.action, &emitter);
        let parent = state.handle.id();
        context
            .child(parent)
            .rebuild_child(&mut state.child, self.child)
    }
}

struct CommandNavigationView<Action: Clone + 'static> {
    child: AnyView<Action>,
    previous: Action,
    next: Action,
    submit: Action,
}

struct CommandNavigationState<Action: Clone + 'static> {
    handle: NodeHandle<KeyListener>,
    child: Mounted<Action>,
    previous: ActionCell<Action>,
    next: ActionCell<Action>,
    submit: ActionCell<Action>,
}

wrapper_mounted_state!(CommandNavigationState where Action: Clone);

impl<Action: Clone + 'static> ViewNode<Action> for CommandNavigationView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let previous = ActionCell::new(self.previous, context.emitter());
        let next = ActionCell::new(self.next, context.emitter());
        let submit = ActionCell::new(self.submit, context.emitter());
        let emit_previous = previous.emitter();
        let emit_next = next.emitter();
        let emit_submit = submit.emitter();
        let handle = context.append(KeyListener::command_navigation(
            move || emit_previous(),
            move || emit_next(),
            move || emit_submit(),
        ))?;
        let child = context.child(handle.id()).build_child(self.child)?;
        Ok(Mounted::new(
            handle.id(),
            CommandNavigationState {
                handle,
                child,
                previous,
                next,
                submit,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let emitter = context.emitter();
        let state = mounted.state_mut::<CommandNavigationState<Action>>()?;
        state.previous.update(self.previous, &emitter);
        state.next.update(self.next, &emitter);
        state.submit.update(self.submit, &emitter);
        let parent = state.handle.id();
        context
            .child(parent)
            .rebuild_child(&mut state.child, self.child)
    }
}

struct AlignView<Action: 'static> {
    child: AnyView<Action>,
    alignment: Alignment,
    padding: Space,
}

struct AlignState<Action: 'static> {
    handle: NodeHandle<Align>,
    child: Mounted<Action>,
    alignment: Alignment,
    padding: f32,
}

wrapper_mounted_state!(AlignState);

impl<Action: 'static> ViewNode<Action> for AlignView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let padding = context.theme().space(self.padding);
        let handle = context.append(Align {
            alignment: self.alignment,
            padding,
        })?;
        let child = context.child(handle.id()).build_child(self.child)?;
        Ok(Mounted::new(
            handle.id(),
            AlignState {
                handle,
                child,
                alignment: self.alignment,
                padding,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let padding = context.theme().space(self.padding);
        let state = mounted.state_mut::<AlignState<Action>>()?;
        if state.alignment != self.alignment || state.padding != padding {
            let alignment = self.alignment;
            context
                .ui()
                .update(state.handle, Invalidation::LAYOUT_ALL, |align| {
                    align.alignment = alignment;
                    align.padding = padding;
                })?;
            state.alignment = alignment;
            state.padding = padding;
        }
        let parent = state.handle.id();
        context
            .child(parent)
            .rebuild_child(&mut state.child, self.child)
    }
}

struct FrameState<Action: 'static> {
    handle: NodeHandle<Frame>,
    child: Mounted<Action>,
    style: FrameStyle,
}

wrapper_mounted_state!(FrameState);

impl<Action: 'static> ViewNode<Action> for FrameView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let handle = context.append(Frame {
            width: self.style.width,
            height: self.style.height,
            min: self.style.min,
            max: self.style.max,
            grow: self.style.grow,
        })?;
        let child = context.child(handle.id()).build_child(self.child)?;
        Ok(Mounted::new(
            handle.id(),
            FrameState {
                handle,
                child,
                style: self.style,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<FrameState<Action>>()?;
        if state.style != self.style {
            // Every field of a frame is a layout input, so `LAYOUT_ALL` is the
            // exact answer. Normalization mirrors what `set_frame` guaranteed.
            let style = self.style;
            context
                .ui()
                .update(state.handle, Invalidation::LAYOUT_ALL, |frame| {
                    frame.width = style.width.map(|value| value.max(0.0));
                    frame.height = style.height.map(|value| value.max(0.0));
                    frame.min =
                        LogicalSize::new(style.min.width.max(0.0), style.min.height.max(0.0));
                    frame.max = style.max.map(|size| {
                        LogicalSize::new(
                            size.width.max(frame.min.width),
                            size.height.max(frame.min.height),
                        )
                    });
                    frame.grow = style.grow.max(0.0);
                })?;
            state.style = style;
        }
        let parent = state.handle.id();
        context
            .child(parent)
            .rebuild_child(&mut state.child, self.child)
    }
}

struct VisibleView<Action: 'static> {
    child: AnyView<Action>,
    visible: bool,
}

struct VisibleState<Action: 'static> {
    child: Mounted<Action>,
    visible: bool,
}

wrapper_mounted_state!(VisibleState);

impl<Action: 'static> ViewNode<Action> for VisibleView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let child = context.build_child(self.child)?;
        let node = child.node();
        context.ui().set_visible(node, self.visible)?;
        Ok(Mounted::new(
            node,
            VisibleState {
                child,
                visible: self.visible,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<VisibleState<Action>>()?;
        context.rebuild_child(&mut state.child, self.child)?;
        let node = state.child.node();
        if state.visible != self.visible {
            context.ui().set_visible(node, self.visible)?;
        }
        state.visible = self.visible;
        mounted.set_node(node);
        Ok(())
    }
}

struct EnabledState<Action: 'static> {
    child: Mounted<Action>,
    enabled: bool,
}

wrapper_mounted_state!(EnabledState);

impl<Action: 'static> ViewNode<Action> for EnabledView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let child = context.build_child(self.child)?;
        let node = child.node();
        context.ui().set_enabled(node, self.enabled)?;
        Ok(Mounted::new(
            node,
            EnabledState {
                child,
                enabled: self.enabled,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<EnabledState<Action>>()?;
        context.rebuild_child(&mut state.child, self.child)?;
        let node = state.child.node();
        if state.enabled != self.enabled {
            context.ui().set_enabled(node, self.enabled)?;
        }
        state.enabled = self.enabled;
        mounted.set_node(node);
        Ok(())
    }
}

/// Mutable inputs of a composed action emitter.
///
/// [`AnyView::map_action`] takes a fresh closure every frame, so the mapping's
/// identity can never be compared and a naive rebuild has to allocate a new
/// composed emitter each pass. Every retained element below the wrapper then
/// sees a different sink identity on every frame, which is invisible today but
/// makes any future memo keyed on sink identity miss unconditionally. Routing
/// through this cell keeps the composed emitter allocated exactly once at mount
/// while still calling the newest mapping.
struct MapActionCell<Child: 'static, Parent: 'static> {
    map: Arc<dyn Fn(Child) -> Parent>,
    parent: ActionEmitter<Parent>,
}

impl<Child: 'static, Parent: 'static> MapActionCell<Child, Parent> {
    /// Builds the single composed emitter that reads through `cell` forever.
    fn compose(cell: &Rc<RefCell<Self>>) -> ActionEmitter<Child> {
        let cell = cell.clone();
        ActionEmitter::from_sink(Arc::new(move |action| {
            // The mapping and the parent emitter are cloned out before being
            // called so that a user mapping can never observe this cell
            // mid-borrow.
            let (map, parent) = {
                let cell = cell.borrow();
                (cell.map.clone(), cell.parent.clone())
            };
            parent.emit(map(action))
        }))
    }
}

struct MapActionState<Child: 'static, Parent: 'static> {
    child: Mounted<Child>,
    cell: Rc<RefCell<MapActionCell<Child, Parent>>>,
    /// Emitter handed to the child subtree. Allocated at mount, never replaced.
    composed: ActionEmitter<Child>,
}

impl<Child: 'static, Parent: 'static> MountedState<Parent> for MapActionState<Child, Parent> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Maps the child vocabulary out on the way back up.
    ///
    /// The child is a `Mounted<Child>`, so it cannot be reported through
    /// `visit_children`; this is the mapping half of that boundary.
    fn route(
        &mut self,
        action: &mut Option<RoutedComponentAction>,
        context: &mut RouteContext<'_>,
    ) -> Result<Vec<Parent>, UiError> {
        let actions = self.child.route(action, context)?;
        let map = self.cell.borrow().map.clone();
        Ok(actions.into_iter().map(|action| map(action)).collect())
    }

    fn rebuild_component(
        &mut self,
        target: u64,
        context: &mut RebuildContext<'_>,
    ) -> Result<bool, UiError> {
        self.child.rebuild_component(target, context)
    }
}

impl<Child: 'static, Parent: 'static> ViewNode<Parent> for MapActionView<Child, Parent> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<Mounted<Parent>, UiError> {
        let cell = Rc::new(RefCell::new(MapActionCell {
            map: self.map,
            parent: context.emitter(),
        }));
        let composed = MapActionCell::compose(&cell);
        let parent = context.parent();
        let child = context.scoped(parent, &composed).build_child(self.child)?;
        let node = child.node();
        Ok(Mounted::new(
            node,
            MapActionState {
                child,
                cell,
                composed,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Parent>,
        context: &mut ViewContext<'_, Parent>,
    ) -> Result<(), UiError> {
        let emitter = context.emitter();
        let parent = context.parent();
        let state = mounted.state_mut::<MapActionState<Child, Parent>>()?;
        {
            let mut cell = state.cell.borrow_mut();
            cell.map = self.map;
            if !cell.parent.ptr_eq(&emitter) {
                cell.parent = emitter;
            }
        }
        let composed = state.composed.clone();
        context
            .scoped(parent, &composed)
            .rebuild_child(&mut state.child, self.child)?;
        let node = state.child.node();
        mounted.set_node(node);
        Ok(())
    }
}

/// Creates a vertical flex view.
pub fn column<Action: 'static>(children: impl IntoChildren<Action>) -> AnyView<Action> {
    column_with(ContainerStyle::default(), children)
}

/// Creates a horizontal flex view.
pub fn row<Action: 'static>(children: impl IntoChildren<Action>) -> AnyView<Action> {
    row_with(ContainerStyle::default(), children)
}

/// Typed presentation options for row and column containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerStyle {
    /// Space between children.
    pub gap: Space,
    /// Uniform inset around children.
    pub padding: Space,
    /// Optional semantic background fill.
    pub background: Option<ColorRole>,
}

impl Default for ContainerStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerStyle {
    /// Creates the default compact container style.
    pub const fn new() -> Self {
        Self {
            gap: Space::Sm,
            padding: Space::None,
            background: None,
        }
    }

    /// Selects semantic child spacing.
    pub const fn gap(mut self, gap: Space) -> Self {
        self.gap = gap;
        self
    }

    /// Selects uniform semantic padding.
    pub const fn padding(mut self, padding: Space) -> Self {
        self.padding = padding;
        self
    }

    /// Selects a semantic background role.
    pub const fn background(mut self, background: ColorRole) -> Self {
        self.background = Some(background);
        self
    }
}

/// Creates a styled vertical container.
pub fn column_with<Action: 'static>(
    style: ContainerStyle,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    container(Axis::Vertical, style, children)
}

/// Creates a styled horizontal container.
pub fn row_with<Action: 'static>(
    style: ContainerStyle,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    container(Axis::Horizontal, style, children)
}

/// Creates a fixed empty layout region.
pub fn spacer<Action: 'static>(size: LogicalSize) -> AnyView<Action> {
    panel(size, ColorRole::Transparent, None)
}

/// Typed presentation options for an overlay stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackStyle {
    /// Uniform inset around overlaid children.
    pub padding: Space,
    /// Optional semantic background fill.
    pub background: Option<ColorRole>,
}

impl Default for StackStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl StackStyle {
    /// Creates an undecorated stack.
    pub const fn new() -> Self {
        Self {
            padding: Space::None,
            background: None,
        }
    }

    /// Selects uniform semantic padding.
    pub const fn padding(mut self, padding: Space) -> Self {
        self.padding = padding;
        self
    }

    /// Selects a semantic background role.
    pub const fn background(mut self, background: ColorRole) -> Self {
        self.background = Some(background);
        self
    }
}

/// Overlays children in paint order at one shared origin.
pub fn stack<Action: 'static>(children: impl IntoChildren<Action>) -> AnyView<Action> {
    stack_with(StackStyle::new(), children)
}

/// Creates a styled overlay stack.
pub fn stack_with<Action: 'static>(
    style: StackStyle,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(StackView {
            padding: style.padding,
            background: style.background,
            children: children.into_children(),
        }),
    }
}

/// Creates a clipped viewport with retained wheel offset.
pub fn scroll<Action: 'static>(
    axis: ScrollAxis,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    scroll_at(axis, LogicalPoint::ZERO, children)
}

/// Creates a clipped viewport with an explicit initial or controlled offset.
pub fn scroll_at<Action: 'static>(
    axis: ScrollAxis,
    offset: LogicalPoint,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(ScrollView {
            axis,
            offset,
            children: children.into_children(),
        }),
    }
}

fn container<Action: 'static>(
    axis: Axis,
    style: ContainerStyle,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    AnyView {
        key: None,
        inner: Box::new(FlexView {
            axis,
            gap: style.gap,
            padding: style.padding,
            background: style.background,
            children: children.into_children(),
        }),
    }
}

/// Creates a flex view with semantic spacing.
pub fn flex<Action: 'static>(
    axis: Axis,
    gap: Space,
    children: impl IntoChildren<Action>,
) -> AnyView<Action> {
    container(axis, ContainerStyle::new().gap(gap), children)
}

/// Creates a two-child pane with a dedicated draggable divider.
pub fn split_pane<Action: 'static>(
    axis: Axis,
    ratio: f32,
    first: View<Action>,
    second: View<Action>,
    on_resize: impl Fn(f32) -> Action + 'static,
) -> View<Action> {
    AnyView {
        key: None,
        inner: Box::new(SplitPaneView {
            axis,
            ratio: ratio.clamp(0.05, 0.95),
            on_resize: Arc::new(on_resize),
            children: vec![first, second],
        }),
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
struct MapCell<Input: 'static, Action: 'static>(Rc<RefCell<MapTarget<Input, Action>>>);

impl<Input: 'static, Action: 'static> MapCell<Input, Action> {
    fn new(map: Arc<dyn Fn(Input) -> Action>, emitter: ActionEmitter<Action>) -> Self {
        Self(Rc::new(RefCell::new(MapTarget { map, emitter })))
    }

    /// Returns the closure installed into the retained element exactly once.
    fn emitter(&self) -> impl Fn(Input) -> Box<dyn Any> + use<Input, Action> {
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
    fn update(&self, map: Arc<dyn Fn(Input) -> Action>, emitter: &ActionEmitter<Action>) {
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
struct ActionCell<Action: Clone + 'static>(Rc<RefCell<ActionTarget<Action>>>);

impl<Action: Clone + 'static> ActionCell<Action> {
    fn new(action: Action, emitter: ActionEmitter<Action>) -> Self {
        Self(Rc::new(RefCell::new(ActionTarget { action, emitter })))
    }

    /// Returns the closure installed into the retained element exactly once.
    fn emitter(&self) -> impl Fn() -> Box<dyn Any> + use<Action> {
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
    fn update(&self, action: Action, emitter: &ActionEmitter<Action>) {
        let mut target = self.0.borrow_mut();
        target.action = action;
        if !target.emitter.ptr_eq(emitter) {
            target.emitter = emitter.clone();
        }
    }
}

struct LabelView {
    text: Arc<str>,
    font_size: f32,
    role: ColorRole,
    width: Option<f32>,
}

struct LabelState {
    handle: NodeHandle<Label>,
    text: Arc<str>,
    font_size: f32,
    color: Color,
    width: Option<f32>,
}

leaf_mounted_state!(LabelState);

impl<Action: 'static> ViewNode<Action> for LabelView {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let color = context.theme().color(self.role);
        let mut label = Label::new(&*self.text)
            .with_font_size(self.font_size)
            .with_color(color);
        label.set_width(self.width);
        let handle = context.append(label)?;
        Ok(Mounted::new(
            handle.id(),
            LabelState {
                handle,
                text: self.text,
                font_size: self.font_size,
                color,
                width: self.width,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let color = context.theme().color(self.role);
        let state = mounted.state_mut::<LabelState>()?;
        if state.text == self.text
            && state.font_size == self.font_size
            && state.width == self.width
            && state.color == color
        {
            return Ok(());
        }
        // `LAYOUT_ALL` is exact for a label rather than a fallback: the shaper
        // bakes the brush into every glyph run, so even a color-only change has
        // to re-shape, and re-shaping happens in layout. The one thing that is
        // *not* exact is the comparison - `Label::preferred_width` is private,
        // so the retained width cannot be read back and is mirrored here
        // instead.
        //
        // The engine's own per-element shaping memo keeps a redundant re-shape
        // cheap; this cannot become narrower until paint takes a text brush.
        let text = String::from(&*self.text);
        let font_size = self.font_size;
        let width = self.width;
        context
            .ui()
            .update(state.handle, Invalidation::LAYOUT_ALL, |label| {
                label.text = text;
                label.font_size = font_size;
                label.color = Some(color);
                label.set_width(width);
            })?;
        state.text = self.text;
        state.font_size = font_size;
        state.color = color;
        state.width = width;
        Ok(())
    }
}

struct BoxView {
    size: LogicalSize,
    role: ColorRole,
    semantics: Option<SemanticData>,
    interactive: bool,
}

struct BoxState {
    handle: NodeHandle<BoxElement>,
    size: LogicalSize,
    color: Color,
    semantics: Option<SemanticData>,
    interactive: bool,
}

leaf_mounted_state!(BoxState);

impl<Action: 'static> ViewNode<Action> for BoxView {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let color = context.theme().color(self.role);
        let handle = context.append(BoxElement {
            size: self.size,
            color,
            semantics: self.semantics.clone(),
            interactive: self.interactive,
        })?;
        Ok(Mounted::new(
            handle.id(),
            BoxState {
                handle,
                size: self.size,
                color,
                semantics: self.semantics,
                interactive: self.interactive,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let color = context.theme().color(self.role);
        let state = mounted.state_mut::<BoxState>()?;
        let mut invalidation = Invalidation::empty();
        if state.size != self.size {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.color != color {
            invalidation |= Invalidation::PAINT;
        }
        if state.semantics != self.semantics {
            invalidation |= Invalidation::ACCESSIBILITY;
        }
        if state.interactive != self.interactive {
            invalidation |= Invalidation::HIT_TEST;
        }
        if invalidation.is_empty() {
            return Ok(());
        }
        // These bits were already computed correctly and then discarded by
        // `set_box`, which asks for `LAYOUT_ALL` unconditionally. A selection
        // mark that only changes color now repaints instead of relaying out.
        let semantics = self.semantics.clone();
        let interactive = self.interactive;
        let size = self.size;
        context.ui().update(state.handle, invalidation, |element| {
            element.size = size;
            element.color = color;
            element.semantics = semantics;
            element.interactive = interactive;
        })?;
        state.size = size;
        state.color = color;
        state.semantics = self.semantics;
        state.interactive = interactive;
        Ok(())
    }
}

struct ButtonView<Action> {
    text: String,
    action: Action,
    variant: ButtonVariant,
    size: LogicalSize,
    icon: Option<Icon>,
    icon_size: f32,
    show_label: bool,
}

struct ButtonState<Action: Clone + 'static> {
    handle: NodeHandle<Button>,
    text: String,
    variant: ButtonVariant,
    size: LogicalSize,
    colors: (Color, Color),
    icon: Option<(u64, LogicalSize, f32, astrelis_paint::FillRule)>,
    show_label: bool,
    action: ActionCell<Action>,
}

leaf_mounted_state!(ButtonState<Action> where Action: Clone);

/// Normalizes an icon's comparable geometry for one button pass.
fn button_icon_state(
    icon: Option<&Icon>,
    icon_size: f32,
) -> Option<(u64, LogicalSize, f32, astrelis_paint::FillRule)> {
    icon.map(|icon| {
        (
            icon.path.cache_id(),
            icon.view_box,
            if icon_size.is_finite() {
                icon_size.max(1.0)
            } else {
                16.0
            },
            icon.fill_rule,
        )
    })
}

impl<Action: Clone + 'static> ViewNode<Action> for ButtonView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let colors = context.theme().button(self.variant);
        let action = ActionCell::new(self.action, context.emitter());
        let emit = action.emitter();
        let icon_state = button_icon_state(self.icon.as_ref(), self.icon_size);
        let mut button = Button::with_action_factory(
            self.text.clone(),
            self.size,
            colors.0,
            colors.1,
            move || emit(),
        )
        .with_label_visible(self.show_label);
        if let (Some(icon), Some((_, _, icon_size, fill_rule))) = (&self.icon, icon_state) {
            button = button.with_icon(
                ButtonIcon::new(icon.path.clone(), icon.view_box, icon_size)
                    .with_fill_rule(fill_rule),
            );
        }
        let handle = context.append(button)?;
        Ok(Mounted::new(
            handle.id(),
            ButtonState {
                handle,
                text: self.text,
                variant: self.variant,
                size: self.size,
                colors,
                icon: icon_state,
                show_label: self.show_label,
                action,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let colors = context.theme().button(self.variant);
        let emitter = context.emitter();
        let icon_state = button_icon_state(self.icon.as_ref(), self.icon_size);
        let state = mounted.state_mut::<ButtonState<Action>>()?;
        // The activation action is written into the cell the retained element
        // already reads through, so a changed action no longer reinstalls a
        // boxed closure.
        state.action.update(self.action, &emitter);
        // Label, size, and icon geometry feed layout; the two fills only repaint.
        let mut invalidation = Invalidation::empty();
        if state.size != self.size
            || state.text != self.text
            || state.icon != icon_state
            || state.show_label != self.show_label
        {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.colors != colors {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let text = self.text.clone();
            let size = self.size;
            let show_label = self.show_label;
            let icon = (state.icon != icon_state).then(|| {
                self.icon.as_ref().map(|icon| {
                    ButtonIcon::new(
                        icon.path.clone(),
                        icon.view_box,
                        icon_state.map(|(_, _, size, _)| size).unwrap_or(16.0),
                    )
                    .with_fill_rule(icon.fill_rule)
                })
            });
            context.ui().update(state.handle, invalidation, |button| {
                button.label = text;
                button.size = size;
                button.color = colors.0;
                button.pressed_color = colors.1;
                button.show_label = show_label;
                if let Some(icon) = icon {
                    button.icon = icon;
                }
            })?;
        }
        state.text = self.text;
        state.variant = self.variant;
        state.size = self.size;
        state.colors = colors;
        state.icon = icon_state;
        state.show_label = self.show_label;
        Ok(())
    }
}

struct TextFieldView<Action: 'static> {
    label: String,
    value: String,
    on_changed: Arc<dyn Fn(String) -> Action>,
}

struct TextFieldState<Action: 'static> {
    handle: NodeHandle<TextField>,
    label: String,
    value: String,
    colors: (Color, Color),
    changed: MapCell<String, Action>,
}

leaf_mounted_state!(TextFieldState<Action>);

impl<Action: 'static> ViewNode<Action> for TextFieldView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Text),
            theme.color(ColorRole::Surface),
        );
        let changed = MapCell::new(self.on_changed, context.emitter());
        let emit = changed.emitter();
        let mut field = TextField::new(self.label.clone(), self.value.clone())
            .on_changed_factory(move |value| emit(value));
        field.text_color = colors.0;
        field.background = colors.1;
        let handle = context.append(field)?;
        Ok(Mounted::new(
            handle.id(),
            TextFieldState {
                handle,
                label: self.label,
                value: self.value,
                colors,
                changed,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Text),
            theme.color(ColorRole::Surface),
        );
        let emitter = context.emitter();
        let state = mounted.state_mut::<TextFieldState<Action>>()?;
        state.changed.update(self.on_changed, &emitter);
        // Value, placeholder label, and glyph color all reach the shaper, which
        // runs in layout. Only the field's background is paint-only.
        let mut invalidation = Invalidation::empty();
        if state.value != self.value || state.label != self.label || state.colors.0 != colors.0 {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.colors.1 != colors.1 {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let label = self.label.clone();
            let value = self.value.clone();
            context.ui().update(state.handle, invalidation, |field| {
                field.label = label;
                field.set_text(value);
                field.text_color = colors.0;
                field.background = colors.1;
            })?;
        }
        state.label = self.label;
        state.value = self.value;
        state.colors = colors;
        Ok(())
    }
}

struct CheckboxView<Action: 'static> {
    label: String,
    checked: bool,
    on_changed: Arc<dyn Fn(bool) -> Action>,
}

struct CheckboxState<Action: 'static> {
    handle: NodeHandle<Checkbox>,
    label: String,
    checked: bool,
    colors: (Color, Color, Color),
    changed: MapCell<bool, Action>,
}

leaf_mounted_state!(CheckboxState<Action>);

impl<Action: 'static> ViewNode<Action> for CheckboxView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Text),
            theme.color(ColorRole::Muted),
            theme.color(ColorRole::Accent),
        );
        let changed = MapCell::new(self.on_changed, context.emitter());
        let emit = changed.emitter();
        let mut checkbox = Checkbox::new(self.label.clone(), self.checked, move |checked| {
            emit(checked)
        });
        checkbox.text_color = colors.0;
        checkbox.outline_color = colors.1;
        checkbox.accent_color = colors.2;
        let handle = context.append(checkbox)?;
        Ok(Mounted::new(
            handle.id(),
            CheckboxState {
                handle,
                label: self.label,
                checked: self.checked,
                colors,
                changed,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Text),
            theme.color(ColorRole::Muted),
            theme.color(ColorRole::Accent),
        );
        let emitter = context.emitter();
        let state = mounted.state_mut::<CheckboxState<Action>>()?;
        state.changed.update(self.on_changed, &emitter);
        // The label and its glyph color reach the shaper, which runs in layout.
        // The checked state changes the indicator fill and the accessible value;
        // the outline and accent fills are paint-only.
        let mut invalidation = Invalidation::empty();
        if state.label != self.label || state.colors.0 != colors.0 {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.checked != self.checked {
            invalidation |= Invalidation::PAINT | Invalidation::ACCESSIBILITY;
        }
        if state.colors.1 != colors.1 || state.colors.2 != colors.2 {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let label = self.label.clone();
            let checked = self.checked;
            context
                .ui()
                .update(state.handle, invalidation, |checkbox| {
                    checkbox.label = label;
                    checkbox.checked = checked;
                    checkbox.text_color = colors.0;
                    checkbox.outline_color = colors.1;
                    checkbox.accent_color = colors.2;
                })?;
        }
        state.label = self.label;
        state.checked = self.checked;
        state.colors = colors;
        Ok(())
    }
}

struct SliderView<Action: 'static> {
    label: String,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    on_changed: Arc<dyn Fn(f32) -> Action>,
}

struct SliderState<Action: 'static> {
    handle: NodeHandle<Slider>,
    label: String,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    colors: (Color, Color),
    changed: MapCell<f32, Action>,
}

leaf_mounted_state!(SliderState<Action>);

impl<Action: 'static> ViewNode<Action> for SliderView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Muted),
            theme.color(ColorRole::Accent),
        );
        let changed = MapCell::new(self.on_changed, context.emitter());
        let emit = changed.emitter();
        let mut slider = Slider::new(
            self.label.clone(),
            self.value,
            self.range.clone(),
            move |value| emit(value),
        );
        slider.step = self.step;
        slider.track_color = colors.0;
        slider.accent_color = colors.1;
        let handle = context.append(slider)?;
        Ok(Mounted::new(
            handle.id(),
            SliderState {
                handle,
                label: self.label,
                value: self.value,
                range: self.range,
                step: self.step,
                colors,
                changed,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let colors = (
            theme.color(ColorRole::Muted),
            theme.color(ColorRole::Accent),
        );
        let emitter = context.emitter();
        let state = mounted.state_mut::<SliderState<Action>>()?;
        state.changed.update(self.on_changed, &emitter);
        // A slider's size is fixed and its label is never painted, so nothing
        // here relayouts. The label is accessibility-only; value and range move
        // the thumb and the reported value; the step is neither painted nor
        // exposed, so changing it invalidates nothing.
        let mut invalidation = Invalidation::empty();
        if state.label != self.label {
            invalidation |= Invalidation::ACCESSIBILITY;
        }
        if state.value != self.value || state.range != self.range {
            invalidation |= Invalidation::PAINT | Invalidation::ACCESSIBILITY;
        }
        if state.colors != colors {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() || state.step != self.step {
            let label = self.label.clone();
            let range = self.range.clone();
            let value = self.value;
            let step = self.step;
            context.ui().update(state.handle, invalidation, |slider| {
                // Normalization and clamping mirror `Slider::new`, which is what
                // the retained element guarantees about these fields.
                let start = (*range.start()).min(*range.end());
                let end = (*range.start()).max(*range.end());
                slider.label = label;
                slider.range = start..=end;
                slider.value = value.clamp(start, end);
                slider.step = step.max(0.0);
                slider.track_color = colors.0;
                slider.accent_color = colors.1;
            })?;
        }
        state.label = self.label;
        state.value = self.value;
        state.range = self.range;
        state.step = self.step;
        state.colors = colors;
        Ok(())
    }
}

struct FlexView<Action: 'static> {
    axis: Axis,
    gap: Space,
    padding: Space,
    background: Option<ColorRole>,
    children: Vec<AnyView<Action>>,
}

struct SplitPaneView<Action: 'static> {
    axis: Axis,
    ratio: f32,
    on_resize: Arc<dyn Fn(f32) -> Action>,
    children: Vec<AnyView<Action>>,
}

struct SplitPaneState<Action: 'static> {
    handle: NodeHandle<SplitPane>,
    axis: Axis,
    ratio: f32,
    resize: MapCell<f32, Action>,
    children: MountedChildren<Action>,
}

container_mounted_state!(SplitPaneState);

impl<Action: 'static> ViewNode<Action> for SplitPaneView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let resize = MapCell::new(self.on_resize, context.emitter());
        let emit = resize.emitter();
        let handle = context.append(SplitPane::new(self.axis, self.ratio, move |ratio| {
            emit(ratio)
        }))?;
        let mut children = MountedChildren::new();
        children.build(self.children, &mut context.child(handle.id()))?;
        Ok(Mounted::new(
            handle.id(),
            SplitPaneState {
                handle,
                axis: self.axis,
                ratio: self.ratio,
                resize,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let emitter = context.emitter();
        let state = mounted.state_mut::<SplitPaneState<Action>>()?;
        state.resize.update(self.on_resize, &emitter);
        // Guarded like every other container. This was the one view that asked
        // for `LAYOUT_ALL` on every pass whether or not the divider moved.
        if state.axis != self.axis || state.ratio != self.ratio {
            let axis = self.axis;
            let ratio = self.ratio;
            context
                .ui()
                .update(state.handle, Invalidation::LAYOUT_ALL, |split| {
                    split.axis = axis;
                    split.ratio = ratio;
                })?;
            state.axis = axis;
            state.ratio = ratio;
        }
        let parent = state.handle.id();
        state
            .children
            .reconcile(self.children, &mut context.child(parent))
    }
}

struct StackView<Action: 'static> {
    padding: Space,
    background: Option<ColorRole>,
    children: Vec<AnyView<Action>>,
}

struct ScrollView<Action: 'static> {
    axis: ScrollAxis,
    offset: LogicalPoint,
    children: Vec<AnyView<Action>>,
}

struct ScrollState<Action: 'static> {
    handle: NodeHandle<Scroll>,
    axis: ScrollAxis,
    requested_offset: LogicalPoint,
    children: MountedChildren<Action>,
}

container_mounted_state!(ScrollState);

impl<Action: 'static> ViewNode<Action> for ScrollView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let handle = context.append(Scroll::new(self.axis))?;
        let axis = self.axis;
        let offset = self.offset;
        // Both fields feed `Scroll::layout`, which measures children against the
        // axis and places them at the offset.
        context
            .ui()
            .update(handle, Invalidation::LAYOUT_ALL, |scroll| {
                scroll.axis = axis;
                scroll.offset = offset;
            })?;
        let mut children = MountedChildren::new();
        children.build(self.children, &mut context.child(handle.id()))?;
        Ok(Mounted::new(
            handle.id(),
            ScrollState {
                handle,
                axis,
                requested_offset: offset,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<ScrollState<Action>>()?;
        if state.axis != self.axis || state.requested_offset != self.offset {
            let axis = self.axis;
            let offset = self.offset;
            context
                .ui()
                .update(state.handle, Invalidation::LAYOUT_ALL, |scroll| {
                    scroll.axis = axis;
                    scroll.offset = offset;
                })?;
            state.axis = axis;
            state.requested_offset = offset;
        }
        let parent = state.handle.id();
        state
            .children
            .reconcile(self.children, &mut context.child(parent))
    }
}

struct StackState<Action: 'static> {
    handle: NodeHandle<Stack>,
    padding: f32,
    background: Option<Color>,
    children: MountedChildren<Action>,
}

container_mounted_state!(StackState);

impl<Action: 'static> ViewNode<Action> for StackView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let padding = theme.space(self.padding);
        let background = self.background.map(|role| theme.color(role));
        let handle = context.append(Stack {
            padding,
            background,
        })?;
        let mut children = MountedChildren::new();
        children.build(self.children, &mut context.child(handle.id()))?;
        Ok(Mounted::new(
            handle.id(),
            StackState {
                handle,
                padding,
                background,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let padding = theme.space(self.padding);
        let background = self.background.map(|role| theme.color(role));
        let state = mounted.state_mut::<StackState<Action>>()?;
        // Padding insets children, so it relayouts. A background is a local fill
        // and only repaints.
        let mut invalidation = Invalidation::empty();
        if state.padding != padding {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.background != background {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            context.ui().update(state.handle, invalidation, |stack| {
                stack.padding = padding;
                stack.background = background;
            })?;
            state.padding = padding;
            state.background = background;
        }
        let parent = state.handle.id();
        state
            .children
            .reconcile(self.children, &mut context.child(parent))
    }
}

struct FlexState<Action: 'static> {
    handle: NodeHandle<Flex>,
    axis: Axis,
    gap: f32,
    padding: f32,
    background: Option<Color>,
    children: MountedChildren<Action>,
}

container_mounted_state!(FlexState);

impl<Action: 'static> ViewNode<Action> for FlexView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let theme = context.theme();
        let gap = theme.space(self.gap);
        let padding = theme.space(self.padding);
        let background = self.background.map(|role| theme.color(role));
        let handle = context.append(Flex {
            axis: self.axis,
            gap,
            padding,
            background,
        })?;
        let mut children = MountedChildren::new();
        children.build(self.children, &mut context.child(handle.id()))?;
        Ok(Mounted::new(
            handle.id(),
            FlexState {
                handle,
                axis: self.axis,
                gap,
                padding,
                background,
                children,
            },
        ))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let theme = context.theme();
        let gap = theme.space(self.gap);
        let padding = theme.space(self.padding);
        let background = self.background.map(|role| theme.color(role));
        let state = mounted.state_mut::<FlexState<Action>>()?;
        // Axis, gap, and padding all place children; a background is only a
        // local fill behind them.
        let mut invalidation = Invalidation::empty();
        if state.axis != self.axis || state.gap != gap || state.padding != padding {
            invalidation |= Invalidation::LAYOUT_ALL;
        }
        if state.background != background {
            invalidation |= Invalidation::PAINT;
        }
        if !invalidation.is_empty() {
            let axis = self.axis;
            context.ui().update(state.handle, invalidation, |flex| {
                flex.axis = axis;
                flex.gap = gap;
                flex.padding = padding;
                flex.background = background;
            })?;
            state.axis = axis;
            state.gap = gap;
            state.padding = padding;
            state.background = background;
        }
        let parent = state.handle.id();
        state
            .children
            .reconcile(self.children, &mut context.child(parent))
    }
}

/// Strategy chosen for one child-list reconciliation pass.
enum ChildStrategy {
    /// Pair new views with retained ones by position.
    ///
    /// Used for unkeyed lists, and for keyed lists whose length and key order
    /// are both unchanged - which is the overwhelmingly common case, and the one
    /// that used to build and throw away a `HashMap` per container per frame.
    Positional,
    /// Match retained children by key through the reusable index.
    Remap,
}

/// One container's reconciled child list plus the scratch space it reuses.
///
/// This is the whole of keyed reconciliation, and a custom container gets it by
/// owning one of these: [`build`](Self::build) at mount,
/// [`reconcile`](Self::reconcile) on every pass, and
/// [`visit`](Self::visit) from [`MountedState::visit_children`]. Nothing else is
/// required for a third-party container to preserve retained identity across
/// reorders, host nested components, and publish a minimal child list.
///
/// Everything here exists to keep a steady-state frame allocation-free: the
/// mounted list is edited in place rather than rebuilt, the key index and the
/// remap buffer are cleared instead of dropped, and the published child order is
/// remembered so an unchanged order is never handed to the engine again.
pub struct MountedChildren<Action: 'static> {
    mounted: Vec<Mounted<Action>>,
    /// Child order most recently published to the engine.
    published: Vec<NodeId>,
    /// Previous children held during a keyed remap.
    scratch: Vec<Option<Mounted<Action>>>,
    /// Key index reused by validation and by the keyed remap.
    index: HashMap<ViewKey, usize>,
}

impl<Action: 'static> Default for MountedChildren<Action> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Action: 'static> MountedChildren<Action> {
    /// Creates an empty child list.
    pub fn new() -> Self {
        Self {
            mounted: Vec::new(),
            published: Vec::new(),
            scratch: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Whether no children are mounted.
    pub fn is_empty(&self) -> bool {
        self.mounted.is_empty()
    }

    /// Number of mounted children.
    pub fn len(&self) -> usize {
        self.mounted.len()
    }

    /// Iterates the mounted children in paint order.
    pub fn mounted_mut(&mut self) -> impl Iterator<Item = &mut Mounted<Action>> {
        self.mounted.iter_mut()
    }

    /// Visits each mounted child in paint order, stopping on `Break`.
    ///
    /// Forward [`MountedState::visit_children`] straight to this.
    pub fn visit(&mut self, visit: &mut dyn FnMut(&mut Mounted<Action>) -> ControlFlow<()>) {
        for child in &mut self.mounted {
            if visit(child).is_break() {
                break;
            }
        }
    }

    /// Mounts an initial child list below the context's parent.
    ///
    /// The engine appends children in order, so the published order is recorded
    /// rather than set: there is nothing to reorder yet.
    pub fn build(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        self.validate(&views)?;
        self.mounted.reserve(views.len());
        self.published.reserve(views.len());
        for view in views {
            let built = context.build_child(view)?;
            self.published.push(built.node());
            self.mounted.push(built);
        }
        Ok(())
    }

    /// Reconciles a replacement child list into retained state.
    ///
    /// Errors when the sequence is partially keyed or carries a duplicate key,
    /// which are authoring mistakes rather than recoverable conditions: both
    /// silently lose retained identity on the next insertion.
    pub fn reconcile(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        // `containers_reconciled`: one child-list reconciliation pass. Counted
        // once for both strategies, so every container is covered exactly once.
        ViewStats::record_container_reconciled();
        match self.validate(&views)? {
            ChildStrategy::Positional => self.reconcile_positional(views, context)?,
            ChildStrategy::Remap => self.reconcile_keyed(views, context)?,
        }
        self.publish(context)
    }

    /// Checks keying and picks a strategy in a single scan.
    ///
    /// One pass rejects partially keyed sequences, rejects duplicate keys, and
    /// decides whether the new keys already line up with the retained ones. The
    /// duplicate check reuses `index` instead of allocating a `HashSet` per
    /// container per frame.
    fn validate(&mut self, views: &[AnyView<Action>]) -> Result<ChildStrategy, UiError> {
        let keyed = views.first().is_some_and(|view| view.key.is_some());
        if !keyed {
            if views.iter().any(|view| view.key.is_some()) {
                return Err(UiError::new(
                    "dynamic view sequences must key every child or no children",
                ));
            }
            // An unkeyed list is always paired by position, including when its
            // length changed: trailing children are appended or removed.
            return Ok(ChildStrategy::Positional);
        }
        self.index.clear();
        self.index.reserve(views.len());
        let mut aligned = self.mounted.len() == views.len();
        for (position, view) in views.iter().enumerate() {
            let Some(key) = view.key.as_ref() else {
                return Err(UiError::new(
                    "dynamic view sequences must key every child or no children",
                ));
            };
            if self.index.insert(key.clone(), position).is_some() {
                return Err(UiError::new(format!("duplicate view key `{key}`")));
            }
            if aligned && self.mounted[position].key() != Some(key) {
                aligned = false;
            }
        }
        Ok(if aligned {
            ChildStrategy::Positional
        } else {
            ChildStrategy::Remap
        })
    }

    fn reconcile_positional(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let kept = views.len();
        for (position, view) in views.into_iter().enumerate() {
            if let Some(retained) = self.mounted.get_mut(position) {
                context.rebuild_child(retained, view)?;
            } else {
                let built = context.build_child(view)?;
                self.mounted.push(built);
            }
        }
        while self.mounted.len() > kept {
            let extra = self.mounted.pop().expect("length was checked");
            context.ui().remove(extra.node())?;
        }
        Ok(())
    }

    fn reconcile_keyed(
        &mut self,
        views: Vec<AnyView<Action>>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        // Re-purpose the index built by `validate` as retained key -> slot.
        self.index.clear();
        self.scratch.clear();
        for retained in self.mounted.drain(..) {
            let key = retained.key.clone().expect("validated keyed sequence");
            self.index.insert(key, self.scratch.len());
            self.scratch.push(Some(retained));
        }
        for view in views {
            let key = view.key.as_ref().expect("validated keyed sequence");
            let retained = self
                .index
                .get(key)
                .copied()
                .and_then(|slot| self.scratch[slot].take());
            match retained {
                Some(mut retained) => {
                    context.rebuild_child(&mut retained, view)?;
                    self.mounted.push(retained);
                }
                None => {
                    let built = context.build_child(view)?;
                    self.mounted.push(built);
                }
            }
        }
        for slot in 0..self.scratch.len() {
            if let Some(extra) = self.scratch[slot].take() {
                context.ui().remove(extra.node())?;
            }
        }
        self.scratch.clear();
        Ok(())
    }

    /// Publishes the child order, but only when it actually moved.
    ///
    /// `UiRoot::set_children` invalidates the parent with `Invalidation::ALL`,
    /// so republishing an unchanged order costs a full pass over a container
    /// that did not change at all. Comparing against the last published order
    /// makes that cost proportional to real structural change.
    fn publish(&mut self, context: &mut ViewContext<'_, Action>) -> Result<(), UiError> {
        if self.published.len() == self.mounted.len()
            && self
                .published
                .iter()
                .zip(&self.mounted)
                .all(|(node, view)| *node == view.node())
        {
            return Ok(());
        }
        self.published.clear();
        self.published
            .extend(self.mounted.iter().map(|view| view.node()));
        // `set_children_calls`: the only place the framework hands the engine a
        // whole child list.
        ViewStats::record_set_children();
        let parent = context.parent();
        context.ui().set_children(parent, &self.published)
    }
}
