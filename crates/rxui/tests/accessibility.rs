//! Component-level accessibility tree and action routing.

use rxui::{
    Component, ComponentContext, Theme, View, button, checkbox, column,
    geometry::LogicalSize,
    semantics::{SemanticAction, SemanticActionKind, SemanticRole},
    slider, text_field,
};
use rxui_test_support::Harness;

#[derive(Clone)]
enum Action {
    Activate,
    Checked(bool),
    Value(f32),
    Text(String),
}

struct AccessibleControls {
    activations: usize,
    checked: bool,
    value: f32,
    text: String,
}

impl Component for AccessibleControls {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Activate => self.activations += 1,
            Action::Checked(checked) => self.checked = checked,
            Action::Value(value) => self.value = value,
            Action::Text(text) => self.text = text,
        }
    }

    fn view(&self, _theme: &Theme) -> View<Action> {
        column((
            button("Run", Action::Activate),
            checkbox("Enabled", self.checked, Action::Checked),
            slider("Gain", self.value, 0.0..=10.0, Action::Value),
            text_field("Name", self.text.clone(), Action::Text),
            button("Unavailable", Action::Activate).enabled(false),
        ))
    }
}

fn controls() -> Harness<AccessibleControls> {
    Harness::new(
        AccessibleControls {
            activations: 0,
            checked: false,
            value: 2.0,
            text: "Astrelis".into(),
        },
        LogicalSize::new(400.0, 240.0),
    )
    .unwrap()
}

#[test]
fn semantic_tree_advertises_roles_state_and_supported_actions() {
    let harness = controls();

    let run = harness.find("Run");
    assert_eq!(run.data.role, SemanticRole::Button);
    assert!(run.actions.contains(&SemanticActionKind::Activate));
    assert!(run.actions.contains(&SemanticActionKind::Focus));

    let field = harness.find("Name");
    assert_eq!(field.data.role, SemanticRole::TextField);
    assert_eq!(field.data.value.as_deref(), Some("Astrelis"));
    assert!(field.actions.contains(&SemanticActionKind::SetText));
    assert!(field.actions.contains(&SemanticActionKind::SetSelection));

    assert!(!harness.find("Unavailable").enabled);
}

#[test]
fn platform_semantic_actions_route_through_component_reducers() {
    let mut harness = controls();
    let field_id = harness.find("Name").id;

    harness.activate("Run");
    harness.activate("Enabled");
    harness.semantic_action("Gain", SemanticAction::SetValue(8.0));
    harness.semantic_action("Name", SemanticAction::SetText("RXUI".into()));
    harness.semantic_action("Name", SemanticAction::Focus);
    harness.activate("Unavailable");

    assert_eq!(harness.component().activations, 1);
    assert!(harness.component().checked);
    assert_eq!(harness.component().value, 8.0);
    assert_eq!(harness.component().text, "RXUI");
    assert_eq!(harness.focused(), Some(field_id));

    let field = harness.find("Name");
    assert!(field.focused);
    assert_eq!(field.data.value.as_deref(), Some("RXUI"));
}
