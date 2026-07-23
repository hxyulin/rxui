//! Typed component reducer and host.

use std::any::Any;

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{Flex, FrameUpdate, UiError, UiInput, UiRoot};

use crate::{Theme, View, ViewHost};

pub(crate) struct RoutedComponentAction {
    pub(crate) target: u64,
    pub(crate) payload: Box<dyn Any>,
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
pub trait ComponentWithProps: Component {
    /// Parent-owned configuration used to create and update the instance.
    type Props: Clone + PartialEq + 'static;

    /// Creates local component state for a newly mounted instance.
    fn create(props: &Self::Props) -> Self;

    /// Applies changed parent props without discarding local component state.
    fn changed(&mut self, props: &Self::Props);
}

/// Services available while reducing a component action.
pub struct ComponentContext<'a, Effect> {
    effects: &'a mut Vec<Effect>,
}

impl<Effect> ComponentContext<'_, Effect> {
    /// Emits a typed effect to the application coordinator.
    pub fn emit(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    pub(crate) fn new(effects: &mut Vec<Effect>) -> ComponentContext<'_, Effect> {
        ComponentContext { effects }
    }
}

/// Component reducer and reconciler independent of retained-tree ownership.
pub struct ComponentRuntime<C: Component> {
    component: C,
    views: ViewHost<C::Action>,
    theme: Theme,
    effects: Vec<C::Effect>,
}

impl<C: Component> ComponentRuntime<C> {
    /// Mounts a component into an existing incremental retained root.
    pub fn mount(component: C, ui: &mut UiRoot, theme: Theme) -> Result<Self, UiError> {
        let views = ViewHost::mount(ui, &theme, component.view(&theme))?;
        ui.update_passes()?;
        Ok(Self {
            component,
            views,
            theme,
            effects: Vec::new(),
        })
    }

    /// Applies a typed local action and reconciles that component.
    pub fn dispatch<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        action: C::Action,
    ) -> Result<FrameUpdate<'a>, UiError> {
        self.component.update(
            action,
            &mut ComponentContext {
                effects: &mut self.effects,
            },
        );
        self.views
            .rebuild(ui, &self.theme, self.component.view(&self.theme))?;
        ui.update_passes()
    }

    /// Routes one normalized UI input and dispatches its typed action.
    pub fn input<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        input: UiInput,
    ) -> Result<Option<FrameUpdate<'a>>, UiError> {
        let action = ui.dispatch(input)?;
        let Some(action) = action else {
            return Ok(None);
        };
        self.dispatch_erased(ui, action).map(Some)
    }

    /// Downcasts an externally supplied action and reconciles its owner.
    pub fn dispatch_erased<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        action: Box<dyn Any>,
    ) -> Result<FrameUpdate<'a>, UiError> {
        if action.is::<RoutedComponentAction>() {
            let action = *action
                .downcast::<RoutedComponentAction>()
                .expect("type was checked");
            let parent_actions = self.views.route(ui, &self.theme, action)?;
            for action in parent_actions {
                self.component.update(
                    action,
                    &mut ComponentContext {
                        effects: &mut self.effects,
                    },
                );
            }
        } else {
            let action = action
                .downcast::<C::Action>()
                .map_err(|_| UiError::new("component action type mismatch"))?;
            self.component.update(
                *action,
                &mut ComponentContext {
                    effects: &mut self.effects,
                },
            );
        }
        self.views
            .rebuild(ui, &self.theme, self.component.view(&self.theme))?;
        ui.update_passes()
    }

    /// Reconciles after an application-owned mutation.
    pub fn refresh<'a>(&mut self, ui: &'a mut UiRoot) -> Result<FrameUpdate<'a>, UiError> {
        self.views
            .rebuild(ui, &self.theme, self.component.view(&self.theme))?;
        ui.update_passes()
    }

    /// Replaces typed theme tokens and reconciles resolved styles.
    pub fn set_theme<'a>(
        &mut self,
        ui: &'a mut UiRoot,
        theme: Theme,
    ) -> Result<FrameUpdate<'a>, UiError> {
        self.theme = theme;
        self.refresh(ui)
    }

    /// Drains parent/application effects.
    pub fn drain_effects(&mut self) -> impl Iterator<Item = C::Effect> + '_ {
        self.effects.drain(..)
    }

    /// Reads component state.
    pub const fn component(&self) -> &C {
        &self.component
    }

    /// Mutates component state before [`Self::refresh`].
    pub fn component_mut(&mut self) -> &mut C {
        &mut self.component
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

    /// Applies a typed local action and reconciles that component.
    pub fn dispatch(&mut self, action: C::Action) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.dispatch(&mut self.ui, action)
    }

    /// Routes one normalized UI input and dispatches its typed action.
    pub fn input(&mut self, input: UiInput) -> Result<Option<FrameUpdate<'_>>, UiError> {
        self.runtime.input(&mut self.ui, input)
    }

    /// Reconciles after an application-owned mutation.
    pub fn refresh(&mut self) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.refresh(&mut self.ui)
    }

    /// Replaces typed theme tokens and reconciles resolved styles.
    pub fn set_theme(&mut self, theme: Theme) -> Result<FrameUpdate<'_>, UiError> {
        self.runtime.set_theme(&mut self.ui, theme)
    }

    /// Drains parent/application effects.
    pub fn drain_effects(&mut self) -> impl Iterator<Item = C::Effect> + '_ {
        self.runtime.drain_effects()
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
}
