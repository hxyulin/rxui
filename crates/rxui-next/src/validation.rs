//! Controlled form-validation models and presentation.

use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
};

use crate::{
    ColorRole, ContainerStyle, LabelStyle, Space, View, column_with, label_with_style, text_field,
    views,
};

/// Validation urgency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationSeverity {
    /// Input should be corrected before submission.
    Error,
    /// Input is accepted but deserves attention.
    Warning,
}

/// One user-visible validation issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationIssue {
    /// Issue urgency.
    pub severity: ValidationSeverity,
    /// User-visible explanation.
    pub message: String,
}

impl ValidationIssue {
    /// Creates an error.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: ValidationSeverity::Error,
            message: message.into(),
        }
    }

    /// Creates a warning.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: ValidationSeverity::Warning,
            message: message.into(),
        }
    }
}

/// Validation result for one controlled field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationResult {
    /// Ordered visible issues.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    /// Creates a result containing one issue.
    pub fn issue(issue: ValidationIssue) -> Self {
        Self {
            issues: vec![issue],
        }
    }

    /// Returns whether any issue is an error.
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

/// Application-owned touched/error state for a controlled form.
#[derive(Clone, Debug)]
pub struct FormValidation<Key> {
    results: HashMap<Key, ValidationResult>,
    touched: HashSet<Key>,
    reveal_all: bool,
}

impl<Key> Default for FormValidation<Key> {
    fn default() -> Self {
        Self {
            results: HashMap::new(),
            touched: HashSet::new(),
            reveal_all: false,
        }
    }
}

impl<Key: Eq + Hash> FormValidation<Key> {
    /// Replaces one field result.
    pub fn set_result(&mut self, key: Key, result: ValidationResult) {
        self.results.insert(key, result);
    }

    /// Marks one field as interacted with.
    pub fn touch(&mut self, key: Key) {
        self.touched.insert(key);
    }

    /// Reveals every current issue, normally after a submit attempt.
    pub fn reveal_all(&mut self) {
        self.reveal_all = true;
    }

    /// Hides untouched issues again without discarding validation results.
    pub fn reset_presentation(&mut self) {
        self.touched.clear();
        self.reveal_all = false;
    }

    /// Returns the result when presentation policy permits it.
    pub fn visible_result(&self, key: &Key) -> Option<&ValidationResult> {
        (self.reveal_all || self.touched.contains(key))
            .then(|| self.results.get(key))
            .flatten()
    }

    /// Returns whether the full form has no errors.
    pub fn is_valid(&self) -> bool {
        self.results.values().all(|result| !result.has_errors())
    }

    /// Returns the first invalid key in map iteration order.
    pub fn first_error(&self) -> Option<&Key> {
        self.results
            .iter()
            .find(|(_, result)| result.has_errors())
            .map(|(key, _)| key)
    }
}

/// Builds a controlled text field with visible validation messages.
pub fn validated_text_field<Action: 'static>(
    label: impl Into<String>,
    value: impl Into<String>,
    result: Option<&ValidationResult>,
    on_changed: impl Fn(String) -> Action + 'static,
) -> View<Action> {
    let label = label.into();
    let messages = result
        .into_iter()
        .flat_map(|result| &result.issues)
        .enumerate()
        .map(|(index, issue)| {
            let role = match issue.severity {
                ValidationSeverity::Error => ColorRole::Danger,
                ValidationSeverity::Warning => ColorRole::Accent,
            };
            label_with_style(
                issue.message.clone(),
                LabelStyle::standard().font_size(12.0).role(role),
            )
            .key(index as u64)
        })
        .collect::<Vec<_>>();
    column_with(
        ContainerStyle::new().gap(Space::Xs),
        (
            text_field(label, value, on_changed).key("field"),
            column_with(ContainerStyle::new().gap(Space::Xs), views(messages)).key("issues"),
        ),
    )
}
