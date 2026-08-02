//! Entity-level accessibility tree and action routing.

use rxui_core::{
    Context, Element, EntityHarness, Render, button, checkbox, column, slider, text_field,
};
use rxui_tree::{SemanticAction, SemanticActionKind, SemanticRole};

struct Accessible {
    activations: usize,
    checked: bool,
    value: f32,
    text: String,
}

impl Render for Accessible {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        column()
            .child(button("Run").on_click(cx.listener(|this, _, cx| {
                this.activations += 1;
                cx.notify();
            })))
            .child(
                checkbox("Enabled", self.checked).on_toggle(cx.listener_value(
                    |this, value, cx| {
                        this.checked = value;
                        cx.notify();
                    },
                )),
            )
            .child(
                slider("Gain", self.value, 0.0..=10.0).on_change(cx.listener_value(
                    |this, value, cx| {
                        this.value = value;
                        cx.notify();
                    },
                )),
            )
            .child(
                text_field("Name", self.text.clone()).on_input(cx.listener_value(
                    |this, value: String, cx| {
                        this.text = value;
                        cx.notify();
                    },
                )),
            )
            .child(
                button("Unavailable")
                    .enabled(false)
                    .on_click(cx.listener(|this, _, cx| {
                        this.activations += 1;
                        cx.notify();
                    })),
            )
    }
}

fn controls() -> EntityHarness<Accessible> {
    EntityHarness::new(|cx| {
        cx.new(|_| Accessible {
            activations: 0,
            checked: false,
            value: 2.0,
            text: "Astrelis".into(),
        })
    })
}

#[test]
fn semantic_tree_advertises_roles_state_and_supported_actions() {
    let harness = controls();
    let run = harness.find("Run");
    assert_eq!(run.data.role, SemanticRole::Button);
    assert!(run.actions.contains(&SemanticActionKind::Activate));
    assert!(run.actions.contains(&SemanticActionKind::Focus));
    let checkbox = harness.find("Enabled");
    assert_eq!(checkbox.data.role, SemanticRole::Checkbox);
    assert_eq!(checkbox.data.value.as_deref(), Some("unchecked"));
    assert!(checkbox.actions.contains(&SemanticActionKind::Activate));
    let slider = harness.find("Gain");
    assert_eq!(slider.data.role, SemanticRole::Slider);
    assert_eq!(slider.data.value.as_deref(), Some("2"));
    assert!(slider.actions.contains(&SemanticActionKind::SetValue));
    let field = harness.find("Name");
    assert_eq!(field.data.role, SemanticRole::TextField);
    assert_eq!(field.data.value.as_deref(), Some("Astrelis"));
    assert!(field.actions.contains(&SemanticActionKind::SetText));
    assert!(field.actions.contains(&SemanticActionKind::SetSelection));
    assert!(!harness.find("Unavailable").enabled);
}

#[test]
fn platform_semantic_actions_route_through_entity_updates() {
    let mut harness = controls();
    let field_id = harness.find("Name").id;
    harness.activate("Run");
    harness.activate("Enabled");
    harness.semantic_action("Gain", SemanticAction::SetValue(8.0));
    harness.semantic_action("Name", SemanticAction::SetText("RXUI".into()));
    harness.semantic_action("Name", SemanticAction::Focus);
    harness.activate("Unavailable");
    let model = harness.root().read(harness.app());
    assert_eq!(model.activations, 1);
    assert!(model.checked);
    assert_eq!(model.value, 8.0);
    assert_eq!(model.text, "RXUI");
    drop(model);
    let field = harness.find("Name");
    assert_eq!(field.id, field_id);
    assert!(field.focused);
}
