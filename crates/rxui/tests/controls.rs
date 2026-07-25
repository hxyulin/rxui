//! Composite leaf controls driven through the accessibility layer.
//!
//! Every control here is *controlled*: it renders the caller's value and
//! reports an intent, and none of them owns the value it displays. These tests
//! therefore assert on the pair that matters - what the tree publishes for a
//! given value, and what intent an interaction reports back.

use astrelis_core::geometry::{LogicalPoint, LogicalSize};
use rxui::core::SemanticRole;
use rxui::{
    Choice, ComboOption, Component, ComponentContext, Theme, View, column, combo_box,
    numeric_field, radio_group,
};
use rxui_test_support::Harness;

const VIEWPORT: LogicalSize = LogicalSize::new(400.0, 400.0);

/// Radio and combo rows paint their selection marker into the label, so a
/// lookup by accessible name has to spell the marker out.
const SELECTED: &str = "●";
const UNSELECTED: &str = "○";

struct RadioScene {
    choices: Vec<Choice<&'static str>>,
    selected: Option<&'static str>,
}

impl RadioScene {
    fn new() -> Self {
        Self {
            choices: vec![
                Choice::new("edit", "edit", "Edit"),
                Choice::new("inspect", "inspect", "Inspect"),
                Choice::new("present", "present", "Present").enabled(false),
            ],
            selected: Some("edit"),
        }
    }
}

impl Component for RadioScene {
    type Action = &'static str;
    type Effect = ();

    fn update(&mut self, action: &'static str, _context: &mut ComponentContext<'_, ()>) {
        self.selected = Some(action);
    }

    fn view(&self, _theme: &Theme) -> View<&'static str> {
        radio_group(&self.choices, self.selected.as_ref(), |value| value)
    }
}

#[test]
fn a_radio_group_marks_exactly_the_selected_choice() {
    let mut harness = Harness::new(RadioScene::new(), VIEWPORT).expect("the radio scene mounts");
    assert!(harness.try_find(&format!("{SELECTED} Edit")).is_some());
    assert!(harness.try_find(&format!("{UNSELECTED} Inspect")).is_some());

    harness.activate(&format!("{UNSELECTED} Inspect"));
    assert_eq!(harness.component().selected, Some("inspect"));
    assert!(harness.try_find(&format!("{UNSELECTED} Edit")).is_some());
    assert!(harness.try_find(&format!("{SELECTED} Inspect")).is_some());
}

#[test]
fn a_radio_group_with_no_selection_marks_nothing() {
    let mut scene = RadioScene::new();
    scene.selected = None;
    let harness = Harness::new(scene, VIEWPORT).expect("the radio scene mounts");
    let marked = harness
        .semantics()
        .iter()
        .filter(|node| node.data.label.starts_with(SELECTED))
        .count();
    assert_eq!(marked, 0);
}

#[test]
fn a_disabled_radio_choice_reports_no_selection() {
    let mut harness = Harness::new(RadioScene::new(), VIEWPORT).expect("the radio scene mounts");
    let present = harness.find(&format!("{UNSELECTED} Present"));
    assert!(!present.enabled);

    harness.activate(&format!("{UNSELECTED} Present"));
    // A disabled button must swallow the activation rather than report it, or
    // enablement becomes purely decorative.
    assert_eq!(harness.component().selected, Some("edit"));
}

#[test]
fn every_radio_choice_publishes_button_semantics() {
    let harness = Harness::new(RadioScene::new(), VIEWPORT).expect("the radio scene mounts");
    let buttons = harness
        .semantics()
        .iter()
        .filter(|node| node.data.role == SemanticRole::Button)
        .count();
    // One per choice, including the disabled one: an assistive client has to be
    // able to announce a choice it cannot pick.
    assert_eq!(buttons, 3);
}

#[derive(Clone, Debug, PartialEq)]
enum ComboAction {
    Toggle,
    Pick(u8),
}

struct ComboScene {
    open: bool,
    picked: Option<u8>,
}

impl ComboScene {
    const fn new() -> Self {
        Self {
            open: false,
            picked: None,
        }
    }

    fn options() -> Vec<ComboOption<u8>> {
        vec![
            ComboOption::new("linear", 0, "Linear"),
            ComboOption::new("smooth", 1, "Smooth"),
        ]
    }
}

impl Component for ComboScene {
    type Action = ComboAction;
    type Effect = ();

    fn update(&mut self, action: ComboAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            ComboAction::Toggle => self.open = !self.open,
            ComboAction::Pick(value) => {
                self.picked = Some(value);
                self.open = false;
            }
        }
    }

    fn view(&self, _theme: &Theme) -> View<ComboAction> {
        combo_box(
            "Interpolation",
            &Self::options(),
            self.picked.as_ref(),
            self.open,
            ComboAction::Toggle,
            ComboAction::Pick,
        )
    }
}

#[test]
fn a_closed_combo_box_publishes_a_placeholder_and_no_options() {
    let harness = Harness::new(ComboScene::new(), VIEWPORT).expect("the combo scene mounts");
    assert!(harness.try_find("Interpolation").is_some());
    // U+2026, the character the control itself uses for its unset state.
    assert!(harness.try_find("Select\u{2026}").is_some());
    assert!(harness.try_find("Linear").is_none());
    assert!(harness.try_find("Smooth").is_none());
}

#[test]
fn opening_a_combo_box_publishes_one_row_per_option() {
    let mut harness = Harness::new(ComboScene::new(), VIEWPORT).expect("the combo scene mounts");
    harness.activate("Select\u{2026}");
    assert!(harness.component().open);
    assert!(harness.try_find("Linear").is_some());
    assert!(harness.try_find("Smooth").is_some());
}

#[test]
fn picking_a_combo_option_reports_its_value_and_becomes_the_summary() {
    let mut harness = Harness::new(ComboScene::new(), VIEWPORT).expect("the combo scene mounts");
    harness.activate("Select\u{2026}");
    harness.activate("Smooth");
    assert_eq!(harness.component().picked, Some(1));
    assert!(!harness.component().open);

    // The closed control now summarizes the selection, and the popup rows are
    // gone - so "Smooth" resolves to the summary button, not a leftover row.
    let summary = harness.find("Smooth");
    assert_eq!(summary.data.role, SemanticRole::Button);
    assert_eq!(
        harness
            .semantics()
            .iter()
            .filter(|node| node.data.label == "Smooth")
            .count(),
        1,
    );
}

#[test]
fn a_combo_option_keeps_its_retained_identity_while_the_popup_stays_open() {
    let mut harness = Harness::new(ComboScene::new(), VIEWPORT).expect("the combo scene mounts");
    harness.activate("Select\u{2026}");
    let smooth = harness.find("Smooth").id;
    harness.refresh();
    // Options are keyed by `ComboOption::id`, so an unchanged option list must
    // reconcile in place rather than tear the popup down and rebuild it.
    assert_eq!(harness.find("Smooth").id, smooth);
}

#[derive(Clone, Debug, PartialEq)]
enum NumericAction {
    Changed(Result<i32, String>),
}

struct NumericScene {
    value: i32,
    last: Option<Result<i32, String>>,
}

impl Component for NumericScene {
    type Action = NumericAction;
    type Effect = ();

    fn update(&mut self, action: NumericAction, _context: &mut ComponentContext<'_, ()>) {
        let NumericAction::Changed(result) = action;
        if let Ok(value) = &result {
            self.value = *value;
        }
        self.last = Some(result);
    }

    fn view(&self, _theme: &Theme) -> View<NumericAction> {
        // Wrapped in a column so the field is not the component root, which is
        // how a numeric field appears in a real form.
        column((numeric_field(
            "Iterations",
            self.value,
            NumericAction::Changed,
        ),))
    }
}

/// Places the caret after the existing text so typed characters append.
fn click_after_text(harness: &mut Harness<NumericScene>) {
    let bounds = harness.bounds("Iterations");
    harness.click_at(LogicalPoint::new(
        bounds.origin.x + bounds.size.width - 4.0,
        bounds.origin.y + bounds.size.height * 0.5,
    ));
}

#[test]
fn a_numeric_field_renders_its_value_as_text() {
    let harness = Harness::new(
        NumericScene {
            value: 12,
            last: None,
        },
        VIEWPORT,
    )
    .expect("the numeric scene mounts");
    let field = harness.find("Iterations");
    assert_eq!(field.data.role, SemanticRole::TextField);
    assert_eq!(field.data.value.as_deref(), Some("12"));
}

#[test]
fn a_numeric_field_reports_a_parsed_value() {
    let mut harness = Harness::new(
        NumericScene {
            value: 12,
            last: None,
        },
        VIEWPORT,
    )
    .expect("the numeric scene mounts");
    click_after_text(&mut harness);
    harness.type_text("3");
    assert_eq!(harness.component().last, Some(Ok(123)));
    assert_eq!(harness.component().value, 123);
    assert_eq!(
        harness.find("Iterations").data.value.as_deref(),
        Some("123"),
    );
}

#[test]
fn a_numeric_field_reports_the_parse_error_instead_of_swallowing_it() {
    let mut harness = Harness::new(
        NumericScene {
            value: 12,
            last: None,
        },
        VIEWPORT,
    )
    .expect("the numeric scene mounts");
    click_after_text(&mut harness);
    harness.type_text("x");
    // The whole field text is re-parsed on every edit, so the reported error
    // describes "12x", not the single rejected character.
    let error = match harness.component().last.clone() {
        Some(Err(error)) => error,
        other => panic!("expected a parse error, got {other:?}"),
    };
    assert!(!error.is_empty(), "a rejected edit must carry a reason");
    assert_eq!(harness.component().value, 12, "the value must not change");
}

#[test]
#[ignore = "TextFieldView::rebuild (rxui-core/src/view.rs:2782) compares the incoming value \
            against the value the view last declared, not against the element's live text, so a \
            controlled field can never be reverted to a value it already holds in state"]
fn a_numeric_field_is_controlled_and_reverts_a_rejected_edit() {
    let mut harness = Harness::new(
        NumericScene {
            value: 12,
            last: None,
        },
        VIEWPORT,
    )
    .expect("the numeric scene mounts");
    click_after_text(&mut harness);
    harness.type_text("x");
    // A controlled field renders its controller's value. The controller kept 12,
    // so the rejected text must not survive. This is the property that makes
    // `Result` in the callback usable at all: the caller decides whether an
    // invalid edit is visible, and doing nothing has to mean it is not.
    assert_eq!(harness.find("Iterations").data.value.as_deref(), Some("12"));
}

#[test]
fn a_rejected_numeric_edit_currently_survives_every_later_rebuild() {
    let mut harness = Harness::new(
        NumericScene {
            value: 12,
            last: None,
        },
        VIEWPORT,
    )
    .expect("the numeric scene mounts");
    click_after_text(&mut harness);
    harness.type_text("x");
    // Documents the bug `a_numeric_field_is_controlled_and_reverts_a_rejected_edit`
    // states. The rebuild guard sees the declared value unchanged at 12 and
    // writes nothing, so the element keeps text its controller never accepted -
    // and no number of further rebuilds can dislodge it.
    assert_eq!(
        harness.find("Iterations").data.value.as_deref(),
        Some("12x"),
    );
    harness.refresh();
    harness.refresh();
    assert_eq!(
        harness.find("Iterations").data.value.as_deref(),
        Some("12x"),
    );
    // The same guard is what makes the divergence unrecoverable rather than
    // merely delayed: only a value the component has not declared before can
    // reach the element.
    harness.mutate(|scene| scene.value = 99);
    assert_eq!(harness.find("Iterations").data.value.as_deref(), Some("99"));
}
