//! Entity translations of the v1 controlled-control invariants.

use astrelis_platform::{
    DeviceId, ElementState, Key, KeyLocation, KeyboardInput, Modifiers, NamedKey, PhysicalKey,
};
use rxui_core::{
    Context, Element, EntityHarness, Render, button, checkbox, column, label, list, slider,
    text_field,
};
use rxui_tree::{SemanticAction, UiInput};

fn named_key(key: NamedKey) -> UiInput {
    UiInput::Keyboard {
        input: KeyboardInput {
            device_id: DeviceId(1),
            physical_key: PhysicalKey::Unidentified,
            logical_key: Key::Named(key),
            text: None,
            location: KeyLocation::Standard,
            state: ElementState::Pressed,
            repeat: false,
            synthetic: false,
        },
        modifiers: Modifiers::default(),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Proposal {
    Check(bool),
    Slide(f32),
    Text(String),
}

struct Controls {
    checked: bool,
    value: f32,
    text: String,
    accept: bool,
    proposals: Vec<Proposal>,
}

impl Render for Controls {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        column()
            .gap(6.0)
            .child(button("Run").on_click(cx.listener(|_, _, _| {})))
            .child(
                checkbox("Enabled", self.checked).on_toggle(cx.listener_value(
                    |this, value, cx| {
                        this.proposals.push(Proposal::Check(value));
                        if this.accept {
                            this.checked = value;
                        }
                        cx.notify();
                    },
                )),
            )
            .child(
                slider("Amount", self.value, 0.0..=100.0).on_change(cx.listener_value(
                    |this, value, cx| {
                        this.proposals.push(Proposal::Slide(value));
                        if this.accept {
                            this.value = value;
                        }
                        cx.notify();
                    },
                )),
            )
            .child(
                text_field("Name", self.text.clone()).on_input(cx.listener_value(
                    |this, value: String, cx| {
                        this.proposals.push(Proposal::Text(value.clone()));
                        if this.accept {
                            this.text = value;
                        }
                        cx.notify();
                    },
                )),
            )
    }
}

fn scene(accept: bool) -> EntityHarness<Controls> {
    EntityHarness::new(|cx| {
        cx.new(move |_| Controls {
            checked: false,
            value: 10.0,
            text: "Astrelis".into(),
            accept,
            proposals: Vec::new(),
        })
    })
}

#[test]
fn checkbox_and_slider_are_controlled_and_revert_refused_values() {
    let mut harness = scene(false);
    harness.activate("Enabled");
    harness.semantic_action("Amount", SemanticAction::SetValue(90.0));
    assert_eq!(
        harness.root().read(harness.app()).proposals,
        vec![Proposal::Check(true), Proposal::Slide(90.0)]
    );
    assert_eq!(
        harness.find("Enabled").data.value.as_deref(),
        Some("unchecked")
    );
    assert_eq!(harness.find("Amount").data.value.as_deref(), Some("10"));
    harness.refresh();
    assert_eq!(
        harness.find("Enabled").data.value.as_deref(),
        Some("unchecked")
    );
    assert_eq!(harness.find("Amount").data.value.as_deref(), Some("10"));
}

#[test]
fn accepted_control_values_settle_without_retained_mutations() {
    let mut harness = scene(true);
    harness.activate("Enabled");
    harness.semantic_action("Amount", SemanticAction::SetValue(25.0));
    harness.semantic_action("Name", SemanticAction::SetText("RXUI".into()));
    assert!(harness.root().read(harness.app()).checked);
    assert_eq!(harness.root().read(harness.app()).value, 25.0);
    assert_eq!(harness.root().read(harness.app()).text, "RXUI");
    harness.refresh();
    assert_eq!(harness.stats().passes.layout_elements, 0);
    assert_eq!(harness.stats().passes.rebuilt_fragments, 0);
    assert_eq!(harness.stats().passes.accessibility_nodes, 0);
}

#[test]
fn checkbox_keyboard_activation_proposes_the_next_value() {
    let mut harness = scene(true);
    harness.semantic_action("Enabled", SemanticAction::Focus);
    harness.input(named_key(NamedKey::Space));
    assert!(harness.root().read(harness.app()).checked);
}

struct KeyboardControls {
    clicks: usize,
    value: f32,
    text: String,
    committed: Option<String>,
}

impl Render for KeyboardControls {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        column()
            .child(button("Run").on_click(cx.listener(|this, _, cx| {
                this.clicks += 1;
                cx.notify();
            })))
            .child(
                slider("Gain", self.value, 0.0..=10.0)
                    .step(2.0)
                    .on_change(cx.listener_value(|this, value, cx| {
                        this.value = value;
                        cx.notify();
                    })),
            )
            .child(
                text_field("Title", self.text.clone())
                    .on_input(cx.listener_value(|this, value: String, cx| {
                        this.text = value;
                        cx.notify();
                    }))
                    .on_commit(cx.listener_value(|this, value: String, cx| {
                        this.committed = Some(value);
                        cx.notify();
                    })),
            )
    }
}

#[test]
fn button_slider_and_text_field_route_keyboard_behavior() {
    let mut harness = EntityHarness::new(|cx| {
        cx.new(|_| KeyboardControls {
            clicks: 0,
            value: 2.0,
            text: "RX".into(),
            committed: None,
        })
    });
    harness.semantic_action("Run", SemanticAction::Focus);
    harness.input(named_key(NamedKey::Enter));
    harness.semantic_action("Gain", SemanticAction::Focus);
    harness.input(named_key(NamedKey::Other("ArrowRight".into())));
    harness.semantic_action("Title", SemanticAction::SetText("RXUI".into()));
    harness.semantic_action("Title", SemanticAction::Focus);
    harness.input(named_key(NamedKey::Enter));
    let model = harness.root().read(harness.app());
    assert_eq!(model.clicks, 1);
    assert_eq!(model.value, 4.0);
    assert_eq!(model.text, "RXUI");
    assert_eq!(model.committed.as_deref(), Some("RXUI"));
}

struct KeyedList {
    rows: Vec<u64>,
}

impl Render for KeyedList {
    fn render(&mut self, _: &mut Context<Self>) -> Element {
        list().children(
            self.rows
                .iter()
                .map(|row| label(format!("Row {row}")).key(*row)),
        )
    }
}

#[test]
fn list_uses_keyed_reconciliation_inside_its_scroll_view() {
    let mut harness = EntityHarness::new(|cx| {
        cx.new(|_| KeyedList {
            rows: vec![1, 2, 3],
        })
    });
    let row_one = harness.node_id("Row 1");
    let row_three = harness.node_id("Row 3");
    let root = harness.root().clone();
    root.update(harness.app_mut(), |this, cx| {
        this.rows.reverse();
        cx.notify();
    });
    let _ = harness.app_mut().flush();
    assert_eq!(harness.node_id("Row 1"), row_one);
    assert_eq!(harness.node_id("Row 3"), row_three);
}

// Deferred from v1 to Stage 6/control expansion: radio/choice, combo box, and
// numeric-field parsing/rejection invariants. Those controls are not in Stage 4.
