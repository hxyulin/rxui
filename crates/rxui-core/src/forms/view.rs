//! Validation presentation.

use std::sync::Arc;

use super::{ValidationResult, ValidationSeverity};
use crate::{
    ColorRole, ContainerStyle, LabelStyle, Space, View, column_with, label, label_with_style,
    text_field, views,
};

/// A titled form region.
pub fn form_section<Action: 'static>(
    title: impl Into<Arc<str>>,
    content: View<Action>,
) -> View<Action> {
    column_with(
        ContainerStyle::new()
            .gap(Space::Sm)
            .padding(Space::Md)
            .background(ColorRole::Surface),
        (label(title), content),
    )
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
