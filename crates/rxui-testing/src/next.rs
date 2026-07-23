//! Deterministic driver for component-native RXUI trees.

use astrelis_core::geometry::LogicalSize;
use rxui_next::{
    Component, ComponentHost, Theme,
    core::{SemanticAction, SemanticNode, SemanticRole, UiError},
};

/// Headless component reducer, reconciler, input driver, and semantic query surface.
pub struct ComponentHarness<C: Component> {
    host: ComponentHost<C>,
}

impl<C: Component> ComponentHarness<C> {
    /// Mounts a component in a deterministic logical viewport.
    pub fn new(component: C, viewport: LogicalSize) -> Result<Self, UiError> {
        Ok(Self {
            host: ComponentHost::new(component, viewport, Theme::dark())?,
        })
    }

    /// Applies one typed component action.
    pub fn dispatch(&mut self, action: C::Action) -> Result<(), UiError> {
        self.host.dispatch(action).map(|_| ())
    }

    /// Activates the first matching accessible node.
    pub fn activate(&mut self, role: SemanticRole, label: &str) -> Result<(), UiError> {
        let target = self
            .semantics()
            .into_iter()
            .find(|node| node.data.role == role && node.data.label == label)
            .map(|node| node.id)
            .ok_or_else(|| UiError::new(format!("semantic node `{label}` was not found")))?;
        self.host
            .semantic_action(target, SemanticAction::Activate)
            .map(|_| ())
    }

    /// Returns the deterministic flat semantic snapshot.
    pub fn semantics(&self) -> Vec<SemanticNode> {
        self.host.ui().semantic_snapshot()
    }

    /// Reads component state.
    pub const fn component(&self) -> &C {
        self.host.component()
    }

    /// Mutably accesses the underlying component host.
    pub fn host_mut(&mut self) -> &mut ComponentHost<C> {
        &mut self.host
    }
}
