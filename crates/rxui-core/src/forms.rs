//! Application-owned state for controlled forms.

use std::hash::Hash;

/// Validation urgency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationSeverity {
    /// Input must be corrected before submission.
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
    /// Creates an error issue.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: ValidationSeverity::Error,
            message: message.into(),
        }
    }

    /// Creates a warning issue.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: ValidationSeverity::Warning,
            message: message.into(),
        }
    }
}

/// Current validation result for one field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationResult {
    /// Stable issue order as produced by the validator.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    /// Creates a result containing one issue.
    pub fn issue(issue: ValidationIssue) -> Self {
        Self {
            issues: vec![issue],
        }
    }
    /// Returns whether any issue prevents submission.
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

/// Presentation and validation state belonging to one controlled field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldState {
    result: Option<ValidationResult>,
    dirty: bool,
    validated: bool,
}

impl FieldState {
    /// Creates pristine, unvalidated field state.
    pub fn new() -> Self {
        Self::default()
    }
    /// Returns whether the user has not changed this field.
    pub const fn is_pristine(&self) -> bool {
        !self.dirty
    }
    /// Returns whether the user has changed this field.
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }
    /// Returns whether validation has been explicitly surfaced.
    pub const fn is_validated(&self) -> bool {
        self.validated
    }
    /// Records user interaction and permits current issues to surface.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
    /// Records the latest validator output without changing presentation state.
    pub fn set_result(&mut self, result: ValidationResult) {
        self.result = Some(result);
    }
    /// Explicitly surfaces the current validator output.
    pub fn validate(&mut self) {
        self.validated = true;
    }
    /// Resets presentation state while preserving the validator output.
    pub fn reset_presentation(&mut self) {
        self.dirty = false;
        self.validated = false;
    }
    /// Returns the complete current validator output.
    pub const fn result(&self) -> Option<&ValidationResult> {
        self.result.as_ref()
    }
    /// Returns issues only after interaction or explicit validation.
    pub fn visible_result(&self) -> Option<&ValidationResult> {
        (self.dirty || self.validated)
            .then_some(self.result.as_ref())
            .flatten()
    }
}

/// Ordered validation state for an entity-owned controlled form.
#[derive(Clone, Debug)]
pub struct FormModel<Key> {
    fields: Vec<(Key, FieldState)>,
    reveal_all: bool,
}

impl<Key> Default for FormModel<Key> {
    fn default() -> Self {
        Self {
            fields: Vec::new(),
            reveal_all: false,
        }
    }
}

impl<Key: Eq + Hash> FormModel<Key> {
    fn ensure(&mut self, key: Key) -> &mut FieldState {
        if let Some(index) = self.fields.iter().position(|(entry, _)| *entry == key) {
            return &mut self.fields[index].1;
        }
        self.fields.push((key, FieldState::new()));
        &mut self.fields.last_mut().expect("field was inserted").1
    }

    /// Replaces one field result while retaining its original form order.
    pub fn set_result(&mut self, key: Key, result: ValidationResult) {
        self.ensure(key).set_result(result);
    }
    /// Marks one field dirty.
    pub fn mark_dirty(&mut self, key: Key) {
        self.ensure(key).mark_dirty();
    }
    /// Compatibility spelling for user interaction.
    pub fn touch(&mut self, key: Key) {
        self.mark_dirty(key);
    }
    /// Reveals every current field result, normally after submit.
    pub fn validate_all(&mut self) {
        self.reveal_all = true;
        for (_, field) in &mut self.fields {
            field.validate();
        }
    }
    /// Compatibility spelling for submit-time presentation.
    pub fn reveal_all(&mut self) {
        self.validate_all();
    }
    /// Hides issues again without discarding results or field order.
    pub fn reset_presentation(&mut self) {
        self.reveal_all = false;
        for (_, field) in &mut self.fields {
            field.reset_presentation();
        }
    }
    /// Returns a field state when it has been registered.
    pub fn field(&self, key: &Key) -> Option<&FieldState> {
        self.fields
            .iter()
            .find(|(entry, _)| entry == key)
            .map(|(_, field)| field)
    }
    /// Returns the result when presentation policy permits it.
    pub fn visible_result(&self, key: &Key) -> Option<&ValidationResult> {
        self.field(key).and_then(FieldState::visible_result)
    }
    /// Returns whether no current field result contains an error.
    pub fn is_valid(&self) -> bool {
        self.fields
            .iter()
            .all(|(_, field)| field.result().is_none_or(|result| !result.has_errors()))
    }
    /// Returns the earliest registered field currently containing an error.
    pub fn first_error(&self) -> Option<&Key> {
        self.fields
            .iter()
            .find(|(_, field)| field.result().is_some_and(ValidationResult::has_errors))
            .map(|(key, _)| key)
    }
}

/// Backwards-readable name for the complete form model.
pub type FormValidation<Key> = FormModel<Key>;
