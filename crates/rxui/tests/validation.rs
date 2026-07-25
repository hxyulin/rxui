//! Controlled validation behavior.

use astrelis_core::geometry::LogicalSize;
use rxui::{
    Component, ComponentContext, FormValidation, Theme, ValidationIssue, ValidationResult,
    validated_text_field,
};
use rxui_test_support::Harness;

#[test]
fn form_validation_separates_results_from_presentation_state() {
    let mut validation = FormValidation::default();
    validation.set_result(
        "name",
        ValidationResult::issue(ValidationIssue::error("Name is required")),
    );
    assert!(!validation.is_valid());
    assert!(validation.visible_result(&"name").is_none());
    validation.touch("name");
    assert_eq!(
        validation.visible_result(&"name").unwrap().message(),
        Some("Name is required")
    );
    validation.reset_presentation();
    assert!(validation.visible_result(&"name").is_none());
    validation.reveal_all();
    assert!(validation.visible_result(&"name").is_some());
}

struct ValidatedForm;

impl Component for ValidatedForm {
    type Action = String;
    type Effect = ();

    fn update(&mut self, _action: Self::Action, _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> rxui::View<Self::Action> {
        validated_text_field(
            "Name",
            "",
            Some(&ValidationResult::issue(ValidationIssue::error(
                "Name is required",
            ))),
            |value| value,
        )
    }
}

#[test]
fn validated_field_publishes_control_and_issue_semantics() {
    let harness = Harness::new(ValidatedForm, LogicalSize::new(320.0, 120.0)).unwrap();
    assert!(harness.find("Name").focusable);
    assert!(harness.try_find("Name is required").is_some());
}
