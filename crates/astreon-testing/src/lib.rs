//! Deterministic retained-tree and command testing helpers.

#![warn(missing_docs)]

use astrelis_core::geometry::{LogicalSize, Size};
use astrelis_paint::DisplayList;
use astrelis_ui_core::{SemanticNode, SemanticRole, Ui, UiError};

/// Headless harness around one retained UI tree.
pub struct UiHarness<Message = ()> {
    ui: Ui<Message>,
}

impl<Message: 'static> UiHarness<Message> {
    /// Creates a harness with a conventional deterministic viewport.
    pub fn new(ui: Ui<Message>) -> Self {
        Self::with_viewport(ui, Size::new(800.0, 600.0), 1.0)
    }

    /// Creates a harness with an explicit logical viewport and scale factor.
    pub fn with_viewport(mut ui: Ui<Message>, viewport: LogicalSize, scale_factor: f32) -> Self {
        ui.set_viewport(viewport, scale_factor);
        Self { ui }
    }

    /// Returns the retained UI tree.
    pub const fn ui(&self) -> &Ui<Message> {
        &self.ui
    }

    /// Returns the retained UI tree for test setup or updates.
    pub const fn ui_mut(&mut self) -> &mut Ui<Message> {
        &mut self.ui
    }

    /// Produces the current semantic tree after ensuring layout.
    pub fn semantics(&mut self) -> Result<SemanticNode, UiError> {
        self.ui.semantic_tree()
    }

    /// Produces the backend-independent display list.
    pub fn display_list(&mut self) -> Result<DisplayList, UiError> {
        self.ui.display_list()
    }

    /// Drains typed application messages emitted by semantic actions or listeners.
    pub fn drain_messages(&mut self) -> impl Iterator<Item = Message> + '_ {
        self.ui.drain_messages()
    }

    /// Finds the first semantic node with the requested role and label.
    pub fn find(
        &mut self,
        role: SemanticRole,
        label: &str,
    ) -> Result<Option<SemanticNode>, UiError> {
        let root = self.semantics()?;
        Ok(find_node(&root, role, label).cloned())
    }
}

fn find_node<'a>(
    node: &'a SemanticNode,
    role: SemanticRole,
    label: &str,
) -> Option<&'a SemanticNode> {
    if node.role == role && node.label == label {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, role, label))
}

#[cfg(test)]
mod tests {
    use astrelis_text::FontDatabase;
    use astrelis_ui_core::Theme;

    use super::*;

    #[test]
    fn finds_nodes_by_semantic_identity() {
        let mut ui = Ui::<()>::new(FontDatabase::default(), Theme::default());
        ui.add_button(ui.root(), "Save").unwrap();
        let mut harness = UiHarness::new(ui);
        assert!(
            harness
                .find(SemanticRole::Button, "Save")
                .unwrap()
                .is_some()
        );
    }
}
