//! Typed component reducer and host.

use std::any::Any;

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{Flex, FrameUpdate, UiError, UiInput, UiRoot};

use crate::{Theme, View, ViewHost};

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

/// Services available while reducing a component action.
pub struct ComponentContext<'a, Effect> {
    effects: &'a mut Vec<Effect>,
}

impl<Effect> ComponentContext<'_, Effect> {
    /// Emits a typed effect to the application coordinator.
    pub fn emit(&mut self, effect: Effect) {
        self.effects.push(effect);
    }
}

/// Runtime-owned component instance and retained view subtree.
pub struct ComponentHost<C: Component> {
    component: C,
    ui: UiRoot,
    views: ViewHost<C::Action>,
    theme: Theme,
    effects: Vec<C::Effect>,
}

impl<C: Component> ComponentHost<C> {
    /// Mounts a component into a fresh retained root.
    pub fn new(component: C, viewport: LogicalSize, theme: Theme) -> Result<Self, UiError> {
        let mut ui = UiRoot::new(Flex::default(), viewport);
        let views = ViewHost::mount(&mut ui, &theme, component.view(&theme))?;
        ui.update_passes()?;
        Ok(Self {
            component,
            ui,
            views,
            theme,
            effects: Vec::new(),
        })
    }

    /// Applies a typed local action and reconciles that component.
    pub fn dispatch(&mut self, action: C::Action) -> Result<FrameUpdate<'_>, UiError> {
        self.component.update(
            action,
            &mut ComponentContext {
                effects: &mut self.effects,
            },
        );
        self.views
            .rebuild(&mut self.ui, &self.theme, self.component.view(&self.theme))?;
        self.ui.update_passes()
    }

    /// Routes one normalized UI input and dispatches its typed action.
    pub fn input(&mut self, input: UiInput) -> Result<Option<FrameUpdate<'_>>, UiError> {
        let action = self.ui.dispatch(input)?;
        let Some(action) = action else {
            return Ok(None);
        };
        let action = action
            .downcast::<C::Action>()
            .map_err(|_| UiError::new("element emitted an action for another component"))?;
        self.dispatch(*action).map(Some)
    }

    /// Reconciles after an application-owned mutation.
    pub fn refresh(&mut self) -> Result<FrameUpdate<'_>, UiError> {
        self.views
            .rebuild(&mut self.ui, &self.theme, self.component.view(&self.theme))?;
        self.ui.update_passes()
    }

    /// Replaces typed theme tokens and reconciles resolved styles.
    pub fn set_theme(&mut self, theme: Theme) -> Result<FrameUpdate<'_>, UiError> {
        self.theme = theme;
        self.refresh()
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
        let action = action
            .downcast::<C::Action>()
            .map_err(|_| UiError::new("component action type mismatch"))?;
        self.dispatch(*action)
    }
}
