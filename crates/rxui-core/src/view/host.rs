//! The root of one mounted view tree.

use std::sync::Arc;

use astrelis_ui_next::{NodeId, UiError, UiRoot};

use crate::{
    ComponentServiceRequest, DirtyHandle, RoutedComponentAction, Theme,
    view::{ActionEmitter, AnyView, Mounted, RebuildContext, RouteContext, ViewContext},
};
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
