//! Entity translations of the v1 controlled-control invariants.

use astrelis_core::geometry::{LogicalPoint, LogicalSize};
use astrelis_platform::{
    DeviceId, ElementState, Key, KeyLocation, KeyboardInput, Modifiers, NamedKey, PhysicalKey,
};
use rxui_core::{
    Axis, Context, Element, EntityHarness, Render, button, checkbox, column, label, list, scroll,
    slider, split_pane, text_field,
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

struct OptionalTextHandlers {
    editable: bool,
    text: String,
    inputs: usize,
    commits: usize,
}

impl Render for OptionalTextHandlers {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        let field = text_field("Optional", self.text.clone());
        if self.editable {
            field
                .on_input(cx.listener_value(|this, value: String, cx| {
                    this.inputs += 1;
                    this.text = value;
                    cx.notify();
                }))
                .on_commit(cx.listener_value(|this, _: String, cx| {
                    this.commits += 1;
                    cx.notify();
                }))
        } else {
            field
        }
    }
}

#[test]
fn removing_optional_text_handlers_clears_retained_routing() {
    let mut harness = EntityHarness::new(|cx| {
        cx.new(|_| OptionalTextHandlers {
            editable: true,
            text: String::new(),
            inputs: 0,
            commits: 0,
        })
    });
    harness.semantic_action("Optional", SemanticAction::SetText("a".into()));
    harness.semantic_action("Optional", SemanticAction::Focus);
    harness.input(named_key(NamedKey::Enter));
    assert_eq!(
        (
            harness.root().read(harness.app()).inputs,
            harness.root().read(harness.app()).commits
        ),
        (1, 1)
    );

    let root = harness.root().clone();
    root.update(harness.app_mut(), |this, cx| {
        this.editable = false;
        cx.notify();
    });
    let _ = harness.app_mut().flush();
    harness.input(UiInput::Paste("b".into()));
    harness.input(named_key(NamedKey::Enter));
    let model = harness.root().read(harness.app());
    assert_eq!((model.inputs, model.commits), (1, 1));
    assert_eq!(model.text, "a");
}

#[test]
fn pointer_drag_emits_multiple_slider_proposals_and_respects_controlled_state() {
    for accept in [false, true] {
        let mut harness = scene(accept);
        let bounds = harness.find("Amount").bounds;
        let y = bounds.origin.y + bounds.size.height * 0.5;
        let point =
            |fraction: f32| LogicalPoint::new(bounds.origin.x + bounds.size.width * fraction, y);
        harness.press_pointer_at(point(0.2));
        harness.hover_at(point(0.4));
        harness.hover_at(point(0.7));
        harness.hover_at(point(0.9));
        harness.release_pointer_at(point(0.8));

        let model = harness.root().read(harness.app());
        let proposals = model
            .proposals
            .iter()
            .filter_map(|proposal| match proposal {
                Proposal::Slide(value) => Some(*value),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            proposals.len() >= 5,
            "press, moves, and release must all propose"
        );
        assert!(proposals.windows(2).any(|pair| pair[0] != pair[1]));
        let expected = if accept {
            *proposals.last().expect("drag proposed")
        } else {
            10.0
        };
        assert_eq!(model.value, expected);
        drop(model);
        assert_eq!(
            harness.find("Amount").data.value.as_deref(),
            Some(expected.to_string().as_str())
        );
    }
}

struct SplitScene {
    ratio: f32,
    proposals: Vec<f32>,
}

impl Render for SplitScene {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        split_pane(Axis::Horizontal, self.ratio)
            .on_change(cx.listener_value(|this, ratio, cx| {
                this.ratio = ratio;
                this.proposals.push(ratio);
                cx.notify();
            }))
            .child(label("Left"))
            .child(label("Right"))
    }
}

#[test]
fn split_pane_builds_two_children_and_round_trips_dragged_ratio() {
    let mut harness = EntityHarness::with_viewport(LogicalSize::new(400.0, 200.0), |cx| {
        cx.new(|_| SplitScene {
            ratio: 0.25,
            proposals: Vec::new(),
        })
    });
    assert!(harness.try_find("Left").is_some());
    assert!(harness.try_find("Right").is_some());
    let divider = LogicalPoint::new(101.0, 100.0);
    harness.press_pointer_at(divider);
    harness.hover_at(LogicalPoint::new(200.0, 100.0));
    harness.release_pointer_at(LogicalPoint::new(280.0, 100.0));
    let model = harness.root().read(harness.app());
    assert_eq!(model.proposals.len(), 2);
    assert_eq!(
        model.ratio,
        *model.proposals.last().expect("release proposes ratio")
    );
    assert!(model.ratio > 0.6 && model.ratio < 0.8);
}

struct ScrollScene {
    offset: LogicalPoint,
    listening: bool,
    proposals: Vec<LogicalPoint>,
}

impl Render for ScrollScene {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        let toggle = cx.listener_value(|_: &mut Self, _: bool, _| {});
        let viewport =
            scroll()
                .offset(self.offset)
                .child(column().children((0..20).map(|row| {
                    checkbox(format!("Scroll row {row}"), false).on_toggle(toggle.clone())
                })));
        if self.listening {
            viewport.on_scroll(cx.listener_value(|this, offset, cx| {
                this.offset = offset;
                this.proposals.push(offset);
                cx.notify();
            }))
        } else {
            viewport
        }
    }
}

#[test]
fn scroll_round_trips_offsets_and_clears_removed_listener() {
    let mut harness = EntityHarness::with_viewport(LogicalSize::new(200.0, 80.0), |cx| {
        cx.new(|_| ScrollScene {
            offset: LogicalPoint::ZERO,
            listening: true,
            proposals: Vec::new(),
        })
    });
    harness.scroll_at(LogicalPoint::new(10.0, 10.0), LogicalPoint::new(0.0, 25.0));
    harness.scroll_at(LogicalPoint::new(10.0, 10.0), LogicalPoint::new(0.0, 15.0));
    assert_eq!(harness.root().read(harness.app()).offset.y, 40.0);
    assert_eq!(harness.root().read(harness.app()).proposals.len(), 2);

    let root = harness.root().clone();
    root.update(harness.app_mut(), |this, cx| {
        this.listening = false;
        cx.notify();
    });
    let _ = harness.app_mut().flush();
    harness.scroll_at(LogicalPoint::new(10.0, 10.0), LogicalPoint::new(0.0, 10.0));
    assert_eq!(harness.root().read(harness.app()).proposals.len(), 2);
    assert_eq!(harness.root().read(harness.app()).offset.y, 40.0);
}

struct StepScene {
    value: f32,
    proposals: Vec<f32>,
}

impl Render for StepScene {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        slider("Clamped step", self.value, 0.0..=10.0)
            .step(-3.0)
            .on_change(cx.listener_value(|this, value, cx| {
                this.value = value;
                this.proposals.push(value);
                cx.notify();
            }))
    }
}

#[test]
fn negative_slider_step_is_clamped_to_zero() {
    let mut harness = EntityHarness::new(|cx| {
        cx.new(|_| StepScene {
            value: 5.0,
            proposals: Vec::new(),
        })
    });
    harness.semantic_action("Clamped step", SemanticAction::Focus);
    harness.input(named_key(NamedKey::Other("ArrowRight".into())));
    assert_eq!(harness.root().read(harness.app()).proposals, vec![5.0]);
}

struct GraphemeField {
    text: String,
}

impl Render for GraphemeField {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        text_field("Unicode", self.text.clone()).on_input(cx.listener_value(
            |this, value: String, cx| {
                this.text = value;
                cx.notify();
            },
        ))
    }
}

#[test]
fn text_field_caret_and_backspace_respect_combining_and_emoji_graphemes() {
    for text in ["e\u{301}x", "👩‍💻x"] {
        let initial = text.to_owned();
        let mut harness = EntityHarness::new(|cx| cx.new(move |_| GraphemeField { text: initial }));
        harness.semantic_action("Unicode", SemanticAction::Focus);
        harness.input(named_key(NamedKey::Other("ArrowLeft".into())));
        harness.input(named_key(NamedKey::Backspace));
        assert_eq!(harness.root().read(harness.app()).text, "x");
    }
}

// Deferred from v1 to Stage 6/control expansion: radio/choice, combo box, and
// numeric-field parsing/rejection invariants. Those controls are not in Stage 4.
