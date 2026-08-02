//! Controlled property grids.

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{SemanticData, SemanticRole};

use crate::{AnyView, ColorRole, column, label, panel_with_semantics, row, text_field, views};

/// Controlled property field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyField {
    /// Stable field identity.
    pub id: u64,
    /// User-visible label.
    pub label: String,
    /// Formatted value.
    pub value: String,
}

/// Builds a keyed reconciled property grid.
pub fn property_grid<Action: 'static>(fields: &[PropertyField]) -> AnyView<Action> {
    column(views(fields.iter().map(|field| {
        row((
            label(field.label.clone()).key("label"),
            panel_with_semantics(
                LogicalSize::new(160.0, 28.0),
                ColorRole::Surface,
                SemanticData {
                    role: SemanticRole::Field,
                    label: field.label.clone(),
                    value: Some(field.value.clone()),
                    ..SemanticData::default()
                },
            )
            .key("value"),
        ))
        .key(field.id)
    })))
}

/// Builds a keyed controlled property grid with real editable text controls.
pub fn editable_property_grid<Action: 'static>(
    fields: &[PropertyField],
    on_changed: impl Fn(u64, String) -> Action + Clone + 'static,
) -> AnyView<Action> {
    column(views(fields.iter().map(|field| {
        let id = field.id;
        let on_changed = on_changed.clone();
        row((
            label(field.label.clone()).key("label"),
            text_field(field.label.clone(), field.value.clone(), move |value| {
                on_changed(id, value)
            })
            .key("value"),
        ))
        .key(id)
    })))
}
