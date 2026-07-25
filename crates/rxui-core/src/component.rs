//! Typed component reducer and host.

use std::{any::Any, cell::RefCell, collections::BTreeSet, rc::Rc, sync::Arc};

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{Flex, FrameUpdate, NodeId, SemanticAction, UiError, UiInput, UiRoot};

use crate::{
    BackgroundTaskRequest, Clipboard, ClipboardReadRequest, ComponentServiceRequest, ServiceAction,
    Theme, View, ViewHost, diagnostics::ViewStats,
};

/// One erased action addressed to the component instance that produced it.
///
/// This is the token the view protocol carries while unwinding: a retained
/// element emits it through an [`crate::ActionEmitter`], and
/// [`crate::MountedState::route`] passes it down the mounted tree until the
/// addressed component takes it. It is deliberately opaque - the payload's type
/// is known only to that component.
pub struct RoutedComponentAction {
    pub(crate) target: u64,
    pub(crate) payload: Box<dyn Any>,
}

/// Depth-keyed set of components whose view must be rebuilt on the next flush.
///
/// `depth` is component nesting depth, not view-node depth: the root component
/// is `0`, a component mounted by the root's view is `1`, and so on. Draining in
/// `(depth, id)` order guarantees a parent rebuilds before any of its
/// descendants, which is what stops a child from being rebuilt twice - once by
/// its parent's cascade and once from its own entry - for a single interaction.
#[derive(Debug, Default)]
struct DirtySet {
    /// Whether the root component's own state changed.
    root: bool,
    /// Whether this flush must ignore props equality when descending.
    force: bool,
    /// Dirty nested components, ordered by `(depth, id)`.
    nested: BTreeSet<(u32, u64)>,
}

/// Shared handle to one runtime's dirty set.
///
/// Cloned into every mounted nested component so that reducing an action can
/// mark exactly that component dirty without walking or rebuilding anything.
#[derive(Clone, Default)]
pub(crate) struct DirtyHandle(Rc<RefCell<DirtySet>>);

impl DirtyHandle {
    /// Marks the root component's view as stale.
    pub(crate) fn mark_root(&self) {
        self.0.borrow_mut().root = true;
    }

    /// Marks one nested component's view as stale.
    pub(crate) fn mark(&self, depth: u32, id: u64) {
        self.0.borrow_mut().nested.insert((depth, id));
    }

    /// Disables props-equality pruning for the remainder of this flush.
    pub(crate) fn force(&self) {
        self.0.borrow_mut().force = true;
    }

    /// Clears and reports one nested component's dirty flag.
    pub(crate) fn take(&self, depth: u32, id: u64) -> bool {
        self.0.borrow_mut().nested.remove(&(depth, id))
    }

    fn take_root(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().root)
    }

    fn take_force(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().force)
    }

    fn pop(&self) -> Option<(u32, u64)> {
        self.0.borrow_mut().nested.pop_first()
    }
}

/// The component whose reducer is currently running.
///
/// Carried by [`ComponentContext`] so that [`ComponentContext::request_render`]
/// can mark the right component without the reducer knowing its own identity.
pub(crate) struct RenderScope<'a> {
    dirty: &'a DirtyHandle,
    /// `None` for the root component.
    nested: Option<(u32, u64)>,
}

impl<'a> RenderScope<'a> {
    pub(crate) const fn root(dirty: &'a DirtyHandle) -> Self {
        Self {
            dirty,
            nested: None,
        }
    }

    pub(crate) const fn nested(dirty: &'a DirtyHandle, depth: u32, id: u64) -> Self {
        Self {
            dirty,
            nested: Some((depth, id)),
        }
    }

    fn request(&self) {
        match self.nested {
            Some((depth, id)) => self.dirty.mark(depth, id),
            None => self.dirty.mark_root(),
        }
        self.dirty.force();
    }
}

/// Typed component with ordinary Rust state and local actions.
pub trait Component: 'static {
    /// Local interaction type.
    type Action: 'static;
    /// Parent/application-facing effect type.
    type Effect: 'static;

    /// Applies one local action.
    fn update(&mut self, action: Self::Action, context: &mut ComponentContext<'_, Self::Effect>);

    /// Produces the current lightweight view.
    fn view(&self, theme: &Theme) -> View<Self::Action>;
}

/// State-owning component that can be mounted as a child with controlled props.
///
/// # Update isolation
///
/// A mounted instance's view is rebuilt only when one of the following holds:
///
/// - its [`Props`](ComponentWithProps::Props) compare unequal to the previous
///   ones,
/// - the [`Theme::revision`] it last rendered at changed,
/// - it reduced one of its own actions since the last flush, or
/// - something called [`ComponentContext::request_render`] or
///   [`ComponentRuntime::mark_dirty`] on an enclosing scope.
///
/// This makes [`Props`](ComponentWithProps::Props)`: PartialEq` load-bearing: a
/// `Props` implementation that reports equality for values that render
/// differently now yields a stale subtree rather than a redundant rebuild. The
/// same applies to a `view` that reads state reached through interior
/// mutability or a global, because nothing about that state is visible to the
/// framework. See `docs/update-isolation.md` for the migration rules.
pub trait ComponentWithProps: Component {
    /// Parent-owned configuration used to create and update the instance.
    ///
    /// `PartialEq` is consulted on every parent rebuild to decide whether this
    /// instance's view is rebuilt at all, so it must be exact with respect to
    /// everything [`Component::view`] reads.
    type Props: Clone + PartialEq + 'static;

    /// Creates local component state for a newly mounted instance.
    fn create(props: &Self::Props) -> Self;

    /// Applies changed parent props without discarding local component state.
    fn changed(&mut self, props: &Self::Props);
}

/// Services available while reducing a component action.
pub struct ComponentContext<'a, Effect> {
    effects: &'a mut Vec<Effect>,
    services: &'a mut Vec<ComponentServiceRequest>,
    route_action: Arc<dyn Fn(Box<dyn Any>) -> ServiceAction>,
    render: RenderScope<'a>,
}

impl<Effect> ComponentContext<'_, Effect> {
    /// Emits a typed effect to the application coordinator.
    pub fn emit(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    /// Forces this component and its descendants to re-render on the next flush.
    ///
    /// # When this is required
    ///
    /// Reconciliation is incremental: a nested component's view is rebuilt only
    /// when its props changed, the theme revision changed, or it reduced one of
    /// its own actions. Any other input to [`Component::view`] is invisible to
    /// the framework, so a component that renders state it does not own  -
    /// something behind an `Rc<RefCell<_>>`, an `Arc<Mutex<_>>`, a global, a
    /// clock, or a handle shared with an ancestor - will keep painting the
    /// values it last saw.
    ///
    /// Call this from the reducer that mutated such shared state. It marks the
    /// reducing component dirty *and* disables props-equality pruning for the
    /// whole flush, so descendants that read the same shared state refresh too.
    ///
    /// Prefer putting the value in props. `request_render` reinstates
    /// unconditional whole-subtree work and should be the exception.
    pub fn request_render(&mut self) {
        self.render.request();
    }

    /// Requests replacement of host clipboard text.
    pub fn write_clipboard(&mut self, text: impl Into<String>) {
        self.services
            .push(ComponentServiceRequest::ClipboardWrite(text.into()));
    }

    /// Requests clipboard text and maps it back into a local action.
    pub fn read_clipboard<Action: 'static>(
        &mut self,
        then: impl FnOnce(Option<String>) -> Action + 'static,
    ) {
        let route_action = self.route_action.clone();
        self.services.push(ComponentServiceRequest::ClipboardRead(
            ClipboardReadRequest::new(move |contents| route_action(Box::new(then(contents)))),
        ));
    }

    /// Requests background work and maps its result back on the UI thread.
    pub fn spawn<Result, Action>(
        &mut self,
        work: impl FnOnce() -> Result + Send + 'static,
        then: impl FnOnce(Result) -> Action + 'static,
    ) where
        Result: Send + 'static,
        Action: 'static,
    {
        let route_action = self.route_action.clone();
        self.services.push(ComponentServiceRequest::BackgroundTask(
            BackgroundTaskRequest::new(work, move |result| route_action(Box::new(then(result)))),
        ));
    }

    pub(crate) fn new<'a>(
        effects: &'a mut Vec<Effect>,
        services: &'a mut Vec<ComponentServiceRequest>,
        route_action: Arc<dyn Fn(Box<dyn Any>) -> ServiceAction>,
        render: RenderScope<'a>,
    ) -> ComponentContext<'a, Effect> {
        ComponentContext {
            effects,
            services,
            route_action,
            render,
        }
    }
}

/// Component reducer and reconciler independent of retained-tree ownership.
///
/// Reducing an action never rebuilds anything. It updates state and records
/// which components went stale; [`ComponentRuntime::flush`] then rebuilds the
/// stale components in depth order and runs the retained passes once. Every
/// entry point that used to reduce-and-rebuild now reduces and flushes, so a
/// batch of actions costs one flush rather than one per action.
pub struct ComponentRuntime<C: Component> {
    component: C,
    views: ViewHost<C::Action>,
    theme: Theme,
    effects: Vec<C::Effect>,
    services: Vec<ComponentServiceRequest>,
    dirty: DirtyHandle,
    /// Hoisted identity sink for root-owned service completions.
    ///
    /// The root component's actions need no routing wrapper, so this closure is
    /// the identity. It is allocated once instead of per reduced action.
    route_action: Arc<dyn Fn(Box<dyn Any>) -> ServiceAction>,
}

impl<C: Component> ComponentRuntime<C> {
    /// Mounts a component into an existing incremental retained root.
    pub fn mount(component: C, ui: &mut UiRoot, theme: Theme) -> Result<Self, UiError> {
        let dirty = DirtyHandle::default();
        // `component_views`: the root component's initial view build.
        ViewStats::record_component_view();
        let views = ViewHost::mount(ui, &theme, component.view(&theme), &dirty)?;
        ui.update_passes()?;
        Ok(Self {
            component,
            views,
            theme,
            effects: Vec::new(),
            services: Vec::new(),
            dirty,
            route_action: Arc::new(|action| action),
        })
    }

    /// Applies a typed local action and reconciles the components it affected.
    pub fn dispatch<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        action: C::Action,
    ) -> Result<FrameUpdate<'a>, UiError> {
        self.reduce_root(action);
        self.flush(ui)
    }

    /// Applies a batch of typed local actions with a single reconciliation.
    ///
    /// Reducing `n` actions and flushing once is what separates a coalesced
    /// frame from `n` full rebuilds. Prefer this over looping [`Self::dispatch`].
    pub fn dispatch_all<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        actions: impl IntoIterator<Item = C::Action>,
    ) -> Result<FrameUpdate<'a>, UiError> {
        for action in actions {
            self.reduce_root(action);
        }
        self.flush(ui)
    }

    /// Routes one normalized UI input and dispatches its typed action.
    pub fn input<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        input: UiInput,
    ) -> Result<Option<FrameUpdate<'a>>, UiError> {
        let action = ui.dispatch(input)?;
        let Some(action) = action else {
            return ui.update_passes().map(Some);
        };
        self.dispatch_erased(ui, action).map(Some)
    }

    /// Downcasts an externally supplied action and reconciles its owner.
    pub fn dispatch_erased<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        action: Box<dyn Any>,
    ) -> Result<FrameUpdate<'a>, UiError> {
        self.reduce_erased(action)?;
        self.flush(ui)
    }

    /// Applies a batch of erased actions with a single reconciliation.
    ///
    /// This is the coalesced path a window host uses after draining a frame's
    /// worth of retained actions.
    pub fn dispatch_all_erased<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        actions: impl IntoIterator<Item = Box<dyn Any>>,
    ) -> Result<FrameUpdate<'a>, UiError> {
        for action in actions {
            self.reduce_erased(action)?;
        }
        self.flush(ui)
    }

    /// Rebuilds every stale component and runs the retained passes once.
    ///
    /// The root's view is built only when the root's own state changed. Nested
    /// components are then drained in `(depth, id)` order, so a component
    /// already refreshed by its parent's cascade is not rebuilt again.
    pub fn flush<'a>(&mut self, ui: &'a mut UiRoot) -> Result<FrameUpdate<'a>, UiError> {
        let force = self.dirty.take_force();
        if self.dirty.take_root() {
            // `component_views`: the root re-render, reached only when the root
            // component's own state changed.
            ViewStats::record_component_view();
            let view = self.component.view(&self.theme);
            self.views
                .rebuild(ui, &self.theme, view, &self.dirty, force)?;
        }
        // Anything still listed is a component the root's cascade did not
        // reach: either the root was clean, or the component sits behind a
        // boundary whose props did not change.
        while let Some((_, id)) = self.dirty.pop() {
            self.views.rebuild_scoped(ui, &self.theme, id, force)?;
        }
        ui.update_passes()
    }

    /// Reconciles after an application-owned mutation.
    ///
    /// Application-owned state is invisible to reconciliation, so this marks
    /// the root dirty *and* forces the pass, reproducing the unconditional
    /// whole-tree rebuild that callers of [`Self::component_mut`] rely on.
    pub fn refresh<'a>(&mut self, ui: &'a mut UiRoot) -> Result<FrameUpdate<'a>, UiError> {
        self.mark_dirty();
        self.flush(ui)
    }

    /// Marks the root component's view stale without reconciling yet.
    ///
    /// Use this when application-owned state changed and the resulting frame
    /// will be flushed later - typically alongside other pending actions, so
    /// the mutation and the actions share one rebuild. It also disables
    /// props-equality pruning for that flush, because a nested component may
    /// read the mutated state without its props changing.
    ///
    /// [`Self::refresh`] is this plus an immediate [`Self::flush`].
    pub fn mark_dirty(&mut self) {
        self.dirty.mark_root();
        self.dirty.force();
    }

    /// Replaces typed theme tokens and reconciles resolved styles.
    ///
    /// Theme changes cascade through [`Theme::revision`]: each mounted
    /// component records the revision it last rendered at and rebuilds when it
    /// differs. A caller that mutates tokens without bumping `revision` still
    /// gets a correct frame - the mismatch is detected here and forces the
    /// pass - but only because this comparison exists, so bumping `revision`
    /// remains the documented contract.
    pub fn set_theme<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        theme: Theme,
    ) -> Result<FrameUpdate<'a>, UiError> {
        if self.theme != theme {
            let bumped = self.theme.revision != theme.revision;
            self.theme = theme;
            self.dirty.mark_root();
            if !bumped {
                self.dirty.force();
            }
        }
        self.flush(ui)
    }

    /// Drains parent/application effects.
    pub fn drain_effects(&mut self) -> impl Iterator<Item = C::Effect> + '_ {
        self.effects.drain(..)
    }

    /// Drains host services requested by root or nested components.
    pub fn drain_service_requests(&mut self) -> impl Iterator<Item = ComponentServiceRequest> + '_ {
        self.services.drain(..)
    }

    /// Reads component state.
    pub const fn component(&self) -> &C {
        &self.component
    }

    /// Mutates component state before [`Self::refresh`].
    pub fn component_mut(&mut self) -> &mut C {
        &mut self.component
    }

    fn reduce_root(&mut self, action: C::Action) {
        self.component.update(
            action,
            &mut ComponentContext::new(
                &mut self.effects,
                &mut self.services,
                self.route_action.clone(),
                RenderScope::root(&self.dirty),
            ),
        );
        self.dirty.mark_root();
    }

    fn reduce_erased(&mut self, action: Box<dyn Any>) -> Result<(), UiError> {
        if action.is::<RoutedComponentAction>() {
            let action = *action
                .downcast::<RoutedComponentAction>()
                .expect("type was checked");
            let parent_actions = self.views.route(action, &mut self.services)?;
            for action in parent_actions {
                self.reduce_root(action);
            }
        } else {
            let action = action
                .downcast::<C::Action>()
                .map_err(|_| UiError::new("component action type mismatch"))?;
            self.reduce_root(*action);
        }
        Ok(())
    }
}

/// Headless component runtime owning its incremental retained tree.
pub struct ComponentHost<C: Component> {
    runtime: ComponentRuntime<C>,
    ui: UiRoot,
}

impl<C: Component> ComponentHost<C> {
    /// Mounts a component into a fresh retained root.
    pub fn new(component: C, viewport: LogicalSize, theme: Theme) -> Result<Self, UiError> {
        let mut ui = UiRoot::new(Flex::default(), viewport);
        let runtime = ComponentRuntime::mount(component, &mut ui, theme)?;
        Ok(Self { runtime, ui })
    }

    /// Applies a typed local action and reconciles the components it affected.
    pub fn dispatch(&mut self, action: C::Action) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.dispatch(&mut self.ui, action)
    }

    /// Applies a batch of typed local actions with a single reconciliation.
    pub fn dispatch_all(
        &mut self,
        actions: impl IntoIterator<Item = C::Action>,
    ) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.dispatch_all(&mut self.ui, actions)
    }

    /// Routes one normalized UI input and dispatches its typed action.
    pub fn input(&mut self, input: UiInput) -> Result<Option<FrameUpdate<'_>>, UiError> {
        self.runtime.input(&mut self.ui, input)
    }

    /// Reconciles after an application-owned mutation.
    pub fn refresh(&mut self) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.refresh(&mut self.ui)
    }

    /// Marks the root component's view stale without reconciling yet.
    pub fn mark_dirty(&mut self) {
        self.runtime.mark_dirty();
    }

    /// Rebuilds every stale component and runs the retained passes once.
    pub fn flush(&mut self) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.flush(&mut self.ui)
    }

    /// Replaces typed theme tokens and reconciles resolved styles.
    pub fn set_theme(&mut self, theme: Theme) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.set_theme(&mut self.ui, theme)
    }

    /// Drains parent/application effects.
    pub fn drain_effects(&mut self) -> impl Iterator<Item = C::Effect> + '_ {
        self.runtime.drain_effects()
    }

    /// Drains host services requested by root or nested components.
    pub fn drain_service_requests(&mut self) -> impl Iterator<Item = ComponentServiceRequest> + '_ {
        self.runtime.drain_service_requests()
    }

    /// Executes pending services synchronously for deterministic headless use.
    ///
    /// Each round of completions is dispatched as one batch, so a component
    /// that requested three services pays one reconciliation rather than three.
    pub fn run_pending_services(
        &mut self,
        clipboard: &mut impl Clipboard,
    ) -> Result<usize, UiError> {
        let mut completed = 0;
        loop {
            let requests = self.runtime.drain_service_requests().collect::<Vec<_>>();
            if requests.is_empty() {
                return Ok(completed);
            }
            let mut actions = Vec::new();
            for request in requests {
                match request {
                    ComponentServiceRequest::ClipboardWrite(text) => {
                        clipboard.write_text(text);
                    }
                    ComponentServiceRequest::ClipboardRead(request) => {
                        actions.push(request.complete(clipboard.read_text()));
                    }
                    ComponentServiceRequest::BackgroundTask(request) => {
                        actions.push(request.run());
                    }
                }
                completed += 1;
            }
            if !actions.is_empty() {
                self.runtime.dispatch_all_erased(&mut self.ui, actions)?;
            }
        }
    }

    /// Reads component state.
    pub const fn component(&self) -> &C {
        self.runtime.component()
    }

    /// Mutates component state before [`Self::refresh`].
    pub fn component_mut(&mut self) -> &mut C {
        self.runtime.component_mut()
    }

    /// Reads the retained core.
    pub const fn ui(&self) -> &UiRoot {
        &self.ui
    }

    /// Mutably accesses the retained core escape hatch.
    pub fn ui_mut(&mut self) -> &mut UiRoot {
        &mut self.ui
    }

    /// Downcasts an externally supplied erased action and dispatches it.
    pub fn dispatch_erased(&mut self, action: Box<dyn Any>) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.dispatch_erased(&mut self.ui, action)
    }

    /// Applies a batch of erased actions with a single reconciliation.
    pub fn dispatch_all_erased(
        &mut self,
        actions: impl IntoIterator<Item = Box<dyn Any>>,
    ) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.dispatch_all_erased(&mut self.ui, actions)
    }

    /// Applies one semantic operation and routes any resulting component action.
    pub fn semantic_action(
        &mut self,
        target: NodeId,
        action: SemanticAction,
    ) -> Result<FrameUpdate<'_>, UiError> {
        let action = self.ui.perform_semantic_action(target, action)?;
        if let Some(action) = action {
            self.runtime.dispatch_erased(&mut self.ui, action)
        } else {
            self.ui.update_passes()
        }
    }
}
