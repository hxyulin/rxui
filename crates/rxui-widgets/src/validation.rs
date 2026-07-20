//! Synchronous field and form validation state.

use std::hash::Hash;

use astrelis_ui_core::{ElementHandle, Label, Ui, UiError, Visibility, WidgetStyle};

/// Importance of a validation issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationSeverity {
    /// Non-blocking guidance.
    Warning,
    /// Blocking invalid state.
    Error,
}

/// One user-visible validation issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationIssue {
    /// Issue importance.
    pub severity: ValidationSeverity,
    /// User-visible explanation.
    pub message: String,
}
impl ValidationIssue {
    /// Creates a blocking error.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: ValidationSeverity::Error,
            message: message.into(),
        }
    }
    /// Creates a non-blocking warning.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: ValidationSeverity::Warning,
            message: message.into(),
        }
    }
}

/// Complete synchronous result for one field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationResult {
    /// Ordered issues for the field.
    pub issues: Vec<ValidationIssue>,
}
impl ValidationResult {
    /// A valid result.
    pub const fn valid() -> Self {
        Self { issues: Vec::new() }
    }
    /// A result containing one issue.
    pub fn issue(issue: ValidationIssue) -> Self {
        Self {
            issues: vec![issue],
        }
    }
    /// Returns whether any blocking error exists.
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == ValidationSeverity::Error)
    }
    /// Returns the first issue message.
    pub fn message(&self) -> Option<&str> {
        self.issues.first().map(|issue| issue.message.as_str())
    }
}

#[derive(Clone, Debug)]
struct FieldState<Key> {
    key: Key,
    result: ValidationResult,
    touched: bool,
}

/// Ordered validation state for a heterogeneous form.
#[derive(Clone, Debug, Default)]
pub struct FormValidation<Key> {
    fields: Vec<FieldState<Key>>,
    submitted: bool,
}
impl<Key: Eq + Hash + Clone> FormValidation<Key> {
    /// Creates an empty form.
    pub const fn new() -> Self {
        Self {
            fields: Vec::new(),
            submitted: false,
        }
    }
    /// Inserts or replaces a field result while preserving first insertion order.
    pub fn set_result(&mut self, key: Key, result: ValidationResult) {
        if let Some(field) = self.fields.iter_mut().find(|field| field.key == key) {
            field.result = result;
        } else {
            self.fields.push(FieldState {
                key,
                result,
                touched: false,
            });
        }
    }
    /// Marks one field as interacted with.
    pub fn touch(&mut self, key: &Key) {
        if let Some(field) = self.fields.iter_mut().find(|field| &field.key == key) {
            field.touched = true;
        }
    }
    /// Reveals all validation and marks a submission attempt.
    pub const fn submit(&mut self) {
        self.submitted = true;
    }
    /// Clears interaction and submission presentation state without discarding results.
    pub fn reset_presentation(&mut self) {
        self.submitted = false;
        for field in &mut self.fields {
            field.touched = false;
        }
    }
    /// Returns the result when it should currently be presented.
    pub fn visible_result(&self, key: &Key) -> Option<&ValidationResult> {
        self.fields
            .iter()
            .find(|field| &field.key == key)
            .filter(|field| self.submitted || field.touched)
            .map(|field| &field.result)
    }
    /// Returns whether submission may proceed.
    pub fn is_valid(&self) -> bool {
        self.fields.iter().all(|field| !field.result.has_errors())
    }
    /// Returns the first invalid key in stable field order.
    pub fn first_error(&self) -> Option<&Key> {
        self.fields
            .iter()
            .find(|field| field.result.has_errors())
            .map(|field| &field.key)
    }
    /// Iterates currently visible issues with their field keys.
    pub fn visible_issues(&self) -> impl Iterator<Item = (&Key, &ValidationIssue)> {
        self.fields
            .iter()
            .filter(move |field| self.submitted || field.touched)
            .flat_map(|field| {
                field
                    .result
                    .issues
                    .iter()
                    .map(move |issue| (&field.key, issue))
            })
    }
}

/// Retained presentation binding for one validated control.
pub struct FieldValidation<T> {
    control: ElementHandle<T>,
    message: ElementHandle<Label>,
}
impl<T> FieldValidation<T> {
    /// Attaches a hidden validation label beneath a control.
    pub fn new<Message: 'static, P>(
        ui: &mut Ui<Message>,
        control: ElementHandle<T>,
        parent: ElementHandle<P>,
    ) -> Result<Self, UiError> {
        let message = ui.add_label(parent, "")?;
        ui.set_visibility(message, Visibility::Hidden)?;
        Ok(Self { control, message })
    }
    /// Synchronizes visible and semantic validation state.
    pub fn sync<Message: 'static>(
        &self,
        ui: &mut Ui<Message>,
        result: Option<&ValidationResult>,
    ) -> Result<(), UiError> {
        let text = result
            .and_then(ValidationResult::message)
            .unwrap_or_default();
        let invalid = result.is_some_and(ValidationResult::has_errors);
        ui.set_label_text(self.message, text)?;
        let foreground = result
            .and_then(|result| result.issues.first())
            .map(|issue| match issue.severity {
                ValidationSeverity::Warning => ui.theme().warning,
                ValidationSeverity::Error => ui.theme().danger,
            });
        ui.set_widget_style(
            self.message,
            WidgetStyle {
                foreground,
                font_size: Some(ui.theme().type_scale.caption),
                ..Default::default()
            },
        )?;
        ui.set_visibility(
            self.message,
            if text.is_empty() {
                Visibility::Hidden
            } else {
                Visibility::Visible
            },
        )?;
        ui.set_semantic_description(self.control, (!text.is_empty()).then(|| text.to_owned()))?;
        ui.set_semantic_invalid(self.control, invalid)
    }
    /// Requests focus for the control.
    pub fn focus<Message: 'static>(&self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        ui.focus(self.control)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn form_reveals_touched_fields_and_orders_errors() {
        let mut form = FormValidation::new();
        form.set_result(
            "name",
            ValidationResult::issue(ValidationIssue::error("Required")),
        );
        form.set_result(
            "path",
            ValidationResult::issue(ValidationIssue::warning("Relative")),
        );
        assert_eq!(form.visible_issues().count(), 0);
        form.touch(&"name");
        assert_eq!(form.visible_issues().count(), 1);
        assert_eq!(form.first_error(), Some(&"name"));
        assert!(!form.is_valid());
    }
}
