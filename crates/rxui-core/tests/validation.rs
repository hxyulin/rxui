//! Entity-form translations of the v1 validation model invariants.

use rxui_core::{
    Context, Element, EntityHarness, Render,
    forms::{FieldState, FormModel, ValidationIssue, ValidationResult, ValidationSeverity},
    text_field,
};

fn errored() -> ValidationResult {
    ValidationResult::issue(ValidationIssue::error("Required"))
}

#[test]
fn field_state_tracks_pristine_dirty_and_validated_presentation() {
    let mut field = FieldState::new();
    field.set_result(errored());
    assert!(field.is_pristine());
    assert!(!field.is_validated());
    assert!(field.visible_result().is_none());
    field.mark_dirty();
    assert!(field.is_dirty());
    assert_eq!(field.visible_result(), Some(&errored()));
    field.reset_presentation();
    assert!(field.visible_result().is_none());
    field.validate();
    assert!(field.is_validated());
    assert_eq!(field.visible_result(), Some(&errored()));
}

#[test]
fn form_results_are_ordered_and_separate_from_presentation() {
    let mut form = FormModel::default();
    form.set_result("email", errored());
    form.set_result("name", errored());
    assert!(!form.is_valid());
    assert_eq!(form.first_error(), Some(&"email"));
    assert!(form.visible_result(&"email").is_none());
    form.touch("name");
    assert!(form.visible_result(&"email").is_none());
    assert_eq!(form.visible_result(&"name"), Some(&errored()));
    form.reveal_all();
    assert_eq!(form.visible_result(&"email"), Some(&errored()));
    form.reset_presentation();
    assert!(form.visible_result(&"name").is_none());
    assert_eq!(form.first_error(), Some(&"email"));
    form.set_result("email", ValidationResult::default());
    assert_eq!(form.first_error(), Some(&"name"));

    let mut untouched = FormModel::<&str>::default();
    untouched.touch("phone");
    assert!(untouched.visible_result(&"phone").is_none());
}

#[test]
fn empty_form_warnings_and_replacements_preserve_v1_verdicts() {
    let mut form = FormModel::<&str>::default();
    assert!(form.is_valid());
    assert_eq!(form.first_error(), None);
    let warning = ValidationResult::issue(ValidationIssue::warning("Unusual"));
    assert_eq!(warning.issues[0].severity, ValidationSeverity::Warning);
    form.set_result("name", warning.clone());
    form.touch("name");
    assert!(form.is_valid());
    assert_eq!(form.visible_result(&"name"), Some(&warning));
    form.set_result("name", errored());
    assert!(!form.is_valid());
    form.set_result("name", ValidationResult::default());
    assert!(form.is_valid());
}

#[test]
fn first_error_is_stable_and_replacement_keeps_registration_order() {
    for _ in 0..256 {
        let mut form = FormModel::default();
        form.set_result("email", errored());
        form.set_result("name", errored());
        assert_eq!(form.first_error(), Some(&"email"));
    }
    let mut form = FormModel::default();
    form.set_result("email", ValidationResult::default());
    form.set_result("name", errored());
    form.set_result("email", errored());
    assert_eq!(form.first_error(), Some(&"email"));
}

struct Validated;

impl Render for Validated {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        text_field("Name", "")
            .on_input(cx.listener_value(|_, _: String, _| {}))
            .error("Name is required")
    }
}

#[test]
fn validated_field_surfaces_control_and_issue_to_semantics() {
    let harness = EntityHarness::new(|cx| cx.new(|_| Validated));
    assert!(harness.find("Name").focusable);
    assert!(harness.try_find("Name is required").is_some());
    assert!(
        harness
            .semantics()
            .iter()
            .all(|node| !node.data.label.is_empty())
    );
}

#[test]
fn reveal_all_is_sticky_for_fields_registered_after_submit() {
    let mut form = FormModel::default();
    form.reveal_all();
    form.set_result("late", errored());
    assert_eq!(form.visible_result(&"late"), Some(&errored()));
}
