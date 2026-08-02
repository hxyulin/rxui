//! Application-owned validation state.

use std::{collections::HashSet, hash::Hash};

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
///
/// Results keep the order in which fields first reported one, so every
/// order-sensitive query has one answer for a given sequence of calls.
#[derive(Clone, Debug)]
pub struct FormValidation<Key> {
    results: Vec<(Key, ValidationResult)>,
    touched: HashSet<Key>,
    reveal_all: bool,
}

impl<Key> Default for FormValidation<Key> {
    fn default() -> Self {
        Self {
            results: Vec::new(),
            touched: HashSet::new(),
            reveal_all: false,
        }
    }
}

impl<Key: Eq + Hash> FormValidation<Key> {
    /// Replaces one field result, keeping that field's existing position.
    pub fn set_result(&mut self, key: Key, result: ValidationResult) {
        match self.results.iter_mut().find(|(entry, _)| *entry == key) {
            Some((_, current)) => *current = result,
            None => self.results.push((key, result)),
        }
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
            .then(|| self.result(key))
            .flatten()
    }

    /// Returns whether the full form has no errors.
    pub fn is_valid(&self) -> bool {
        self.results.iter().all(|(_, result)| !result.has_errors())
    }

    /// Returns the first field with an error, in the order results were first
    /// recorded, which for a form is the order its fields were validated.
    ///
    /// Callers use this to focus or scroll to the offending field, so the
    /// answer must not depend on hashing.
    pub fn first_error(&self) -> Option<&Key> {
        self.results
            .iter()
            .find(|(_, result)| result.has_errors())
            .map(|(key, _)| key)
    }

    fn result(&self, key: &Key) -> Option<&ValidationResult> {
        self.results
            .iter()
            .find(|(entry, _)| entry == key)
            .map(|(_, result)| result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errored() -> ValidationResult {
        ValidationResult::issue(ValidationIssue::error("Required"))
    }

    fn warned() -> ValidationResult {
        ValidationResult::issue(ValidationIssue::warning("Unusual"))
    }

    #[test]
    fn an_empty_form_is_valid_and_shows_nothing() {
        let validation = FormValidation::<&str>::default();
        assert!(validation.is_valid());
        assert_eq!(validation.first_error(), None);
        assert_eq!(validation.visible_result(&"name"), None);
    }

    #[test]
    fn results_are_hidden_until_the_field_is_touched() {
        let mut validation = FormValidation::default();
        validation.set_result("name", errored());
        assert!(!validation.is_valid());
        assert_eq!(validation.visible_result(&"name"), None);
        validation.touch("name");
        assert_eq!(validation.visible_result(&"name"), Some(&errored()));
    }

    #[test]
    fn touching_one_field_reveals_only_that_field() {
        let mut validation = FormValidation::default();
        validation.set_result("name", errored());
        validation.set_result("email", errored());
        validation.touch("email");
        assert_eq!(validation.visible_result(&"name"), None);
        assert_eq!(validation.visible_result(&"email"), Some(&errored()));
    }

    #[test]
    fn reveal_all_shows_untouched_fields_and_reset_hides_them_again() {
        let mut validation = FormValidation::default();
        validation.set_result("name", errored());
        validation.reveal_all();
        assert_eq!(validation.visible_result(&"name"), Some(&errored()));
        validation.reset_presentation();
        assert_eq!(validation.visible_result(&"name"), None);
        // Presentation state is the only thing reset; the verdict survives.
        assert!(!validation.is_valid());
        assert_eq!(validation.first_error(), Some(&"name"));
    }

    #[test]
    fn reset_presentation_also_forgets_touches() {
        let mut validation = FormValidation::default();
        validation.set_result("name", errored());
        validation.touch("name");
        validation.reset_presentation();
        assert_eq!(validation.visible_result(&"name"), None);
    }

    #[test]
    fn touching_a_field_without_a_result_reveals_nothing() {
        let mut validation = FormValidation::<&str>::default();
        validation.touch("name");
        assert_eq!(validation.visible_result(&"name"), None);
    }

    #[test]
    fn warnings_are_visible_without_invalidating_the_form() {
        let mut validation = FormValidation::default();
        validation.set_result("name", warned());
        validation.touch("name");
        assert!(validation.is_valid());
        assert_eq!(validation.first_error(), None);
        assert_eq!(validation.visible_result(&"name"), Some(&warned()));
    }

    #[test]
    fn replacing_a_result_clears_the_previous_verdict() {
        let mut validation = FormValidation::default();
        validation.set_result("name", errored());
        validation.set_result("name", ValidationResult::default());
        assert!(validation.is_valid());
        assert_eq!(validation.first_error(), None);
    }

    #[test]
    fn first_error_reports_the_earliest_recorded_error() {
        let mut validation = FormValidation::default();
        validation.set_result("name", ValidationResult::default());
        validation.set_result("email", errored());
        validation.set_result("phone", errored());
        assert_eq!(validation.first_error(), Some(&"email"));
    }

    #[test]
    fn first_error_is_stable_across_instances() {
        // The results used to live in a `HashMap`, which made this public
        // answer depend on per-instance hash seeding: with two errored fields
        // it returned either one about half the time.
        for _ in 0..256 {
            let mut validation = FormValidation::default();
            validation.set_result("email", errored());
            validation.set_result("name", errored());
            assert_eq!(validation.first_error(), Some(&"email"));
        }
    }

    #[test]
    fn replacing_a_result_keeps_the_field_in_place() {
        let mut validation = FormValidation::default();
        validation.set_result("email", ValidationResult::default());
        validation.set_result("name", errored());
        validation.set_result("email", errored());
        assert_eq!(validation.first_error(), Some(&"email"));
    }
}
