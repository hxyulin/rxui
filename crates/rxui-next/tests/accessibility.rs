//! Component-level accessibility tree and action routing.

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{SemanticAction, SemanticActionKind, SemanticRole};
use rxui_next::{
    Component, ComponentContext, ComponentHost, Theme, View, button, checkbox, column, slider,
    text_field,
};

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

fn target(host: &ComponentHost<AccessibleControls>, label: &str) -> astrelis_ui_next::NodeId {
    host.ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == label)
        .unwrap()
        .id
}

#[test]
fn semantic_tree_advertises_roles_state_and_supported_actions() {
    let host = ComponentHost::new(
        AccessibleControls {
            activations: 0,
            checked: false,
            value: 2.0,
            text: "Astrelis".into(),
        },
        LogicalSize::new(400.0, 240.0),
        Theme::dark(),
    )
    .unwrap();
    let nodes = host.ui().semantic_snapshot();
    let run = nodes.iter().find(|node| node.data.label == "Run").unwrap();
    assert_eq!(run.data.role, SemanticRole::Button);
    assert!(run.actions.contains(&SemanticActionKind::Activate));
    assert!(run.actions.contains(&SemanticActionKind::Focus));

    let field = nodes.iter().find(|node| node.data.label == "Name").unwrap();
    assert_eq!(field.data.role, SemanticRole::TextField);
    assert_eq!(field.data.value.as_deref(), Some("Astrelis"));
    assert!(field.actions.contains(&SemanticActionKind::SetText));
    assert!(field.actions.contains(&SemanticActionKind::SetSelection));

    let unavailable = nodes
        .iter()
        .find(|node| node.data.label == "Unavailable")
        .unwrap();
    assert!(!unavailable.enabled);
}

#[test]
fn platform_semantic_actions_route_through_component_reducers() {
    let mut host = ComponentHost::new(
        AccessibleControls {
            activations: 0,
            checked: false,
            value: 2.0,
            text: "Astrelis".into(),
        },
        LogicalSize::new(400.0, 240.0),
        Theme::dark(),
    )
    .unwrap();
    let run = target(&host, "Run");
    let checkbox = target(&host, "Enabled");
    let slider = target(&host, "Gain");
    let field = target(&host, "Name");
    let unavailable = target(&host, "Unavailable");

    host.semantic_action(run, SemanticAction::Activate).unwrap();
    host.semantic_action(checkbox, SemanticAction::Activate)
        .unwrap();
    host.semantic_action(slider, SemanticAction::SetValue(8.0))
        .unwrap();
    host.semantic_action(field, SemanticAction::SetText("RXUI".into()))
        .unwrap();
    host.semantic_action(field, SemanticAction::Focus).unwrap();
    host.semantic_action(unavailable, SemanticAction::Activate)
        .unwrap();

    assert_eq!(host.component().activations, 1);
    assert!(host.component().checked);
    assert_eq!(host.component().value, 8.0);
    assert_eq!(host.component().text, "RXUI");
    assert_eq!(host.ui().focused(), Some(field));

    let field = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Name")
        .unwrap();
    assert!(field.focused);
    assert_eq!(field.data.value.as_deref(), Some("RXUI"));
}
