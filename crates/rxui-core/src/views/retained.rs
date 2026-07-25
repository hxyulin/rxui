//! The escape hatch: a specialized retained element behind a view boundary.

use std::any::Any;

use astrelis_ui_next::{Element, Invalidation, NodeHandle, UiError};

use crate::{
    Theme, View,
    view::{ActionEmitter, AnyView, Mounted, MountedChildren, MountedState, ViewContext, ViewNode},
};
use std::ops::ControlFlow;
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
