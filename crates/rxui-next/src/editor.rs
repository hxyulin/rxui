//! Editor-oriented reconciled view compositions.

use std::ops::Range;

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{SemanticData, SemanticRole};

use crate::{AnyView, ColorRole, column, label, label_with_width, panel, row, text_field};

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
    column(
        fields
            .iter()
            .map(|field| {
                row(vec![
                    label(field.label.clone()).keyed("label"),
                    panel(
                        LogicalSize::new(160.0, 28.0),
                        ColorRole::Surface,
                        Some(SemanticData {
                            role: SemanticRole::Field,
                            label: field.label.clone(),
                            value: Some(field.value.clone()),
                            ..SemanticData::default()
                        }),
                    )
                    .keyed("value"),
                ])
                .keyed(field.id)
            })
            .collect(),
    )
}

/// Builds a keyed controlled property grid with real editable text controls.
pub fn editable_property_grid<Action: 'static>(
    fields: &[PropertyField],
    on_changed: impl Fn(u64, String) -> Action + Clone + 'static,
) -> AnyView<Action> {
    column(
        fields
            .iter()
            .map(|field| {
                let id = field.id;
                let on_changed = on_changed.clone();
                row(vec![
                    label(field.label.clone()).keyed("label"),
                    text_field(field.label.clone(), field.value.clone(), move |value| {
                        on_changed(id, value)
                    })
                    .keyed("value"),
                ])
                .keyed(id)
            })
            .collect(),
    )
}

/// One flattened tree row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    /// Stable row identity.
    pub id: u64,
    /// Depth in the hierarchy.
    pub depth: usize,
    /// User-visible label.
    pub label: String,
}

/// Builds only the requested visible tree range.
pub fn virtual_tree<Action: 'static>(
    rows: &[TreeRow],
    visible: Range<usize>,
    selected: Option<u64>,
) -> AnyView<Action> {
    let end = visible.end.min(rows.len());
    let start = visible.start.min(end);
    column(
        rows[start..end]
            .iter()
            .map(|item| {
                row(vec![
                    panel(
                        LogicalSize::new(8.0 + item.depth as f32 * 12.0, 24.0),
                        if selected == Some(item.id) {
                            ColorRole::Accent
                        } else {
                            ColorRole::Surface
                        },
                        Some(SemanticData {
                            role: SemanticRole::Row,
                            label: item.label.clone(),
                            selected: Some(selected == Some(item.id)),
                            ..SemanticData::default()
                        }),
                    )
                    .keyed("selection"),
                    label(item.label.clone()).keyed("label"),
                ])
                .keyed(item.id)
            })
            .collect(),
    )
}

/// One controlled table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow {
    /// Stable row identity.
    pub id: u64,
    /// Ordered formatted cells.
    pub cells: Vec<String>,
}

/// Builds a keyed visible table range.
pub fn virtual_table<Action: 'static>(
    rows: &[TableRow],
    visible: Range<usize>,
    _selected: Option<u64>,
) -> AnyView<Action> {
    virtual_table_with_widths(rows, visible, _selected, &[])
}

/// Builds a keyed visible table range with controlled column widths.
pub fn virtual_table_with_widths<Action: 'static>(
    rows: &[TableRow],
    visible: Range<usize>,
    _selected: Option<u64>,
    widths: &[f32],
) -> AnyView<Action> {
    let end = visible.end.min(rows.len());
    let start = visible.start.min(end);
    column(
        rows[start..end]
            .iter()
            .map(|item| {
                row(item
                    .cells
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| {
                        label_with_width(cell.clone(), widths.get(index).copied())
                            .keyed(index as u64)
                    })
                    .collect())
                .keyed(item.id)
            })
            .collect(),
    )
}

/// Builds a retained placeholder for an application-rendered viewport.
pub fn render_view<Action: 'static>(label: impl Into<String>) -> AnyView<Action> {
    panel(
        LogicalSize::new(640.0, 360.0),
        ColorRole::Background,
        Some(SemanticData {
            role: SemanticRole::RenderView,
            label: label.into(),
            ..SemanticData::default()
        }),
    )
}
