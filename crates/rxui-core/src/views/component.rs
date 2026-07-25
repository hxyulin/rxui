//! The component boundary: an independently state-owning child.

use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use astrelis_ui_next::{Flex, NodeHandle, UiError, UiRoot};

use crate::{
    ComponentContext, ComponentWithProps, DirtyHandle, RenderScope, RoutedComponentAction, Theme,
    View,
    diagnostics::ViewStats,
    view::{
        ActionEmitter, AnyView, Mounted, MountedState, RebuildContext, RouteContext, ViewContext,
        ViewNode,
    },
};
static NEXT_COMPONENT_ID: AtomicU64 = AtomicU64::new(1);
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
