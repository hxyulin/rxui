//! Editor-oriented reconciled view compositions.

use std::ops::Range;

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{SemanticData, SemanticRole};

use rxui_core::{
    AnyView, ColorRole, column, label, label_with_width, panel, row, text_field, views,
};

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

/// Narrows a caller-supplied visible range onto the rows that exist.
///
/// Callers own scrolling, so the range can name rows past the end or be
/// inverted after a shrink; both must yield an empty selection, never a panic.
fn visible_rows(visible: Range<usize>, len: usize) -> Range<usize> {
    let end = visible.end.min(len);
    visible.start.min(end)..end
}

/// Builds only the requested visible tree range.
pub fn virtual_tree<Action: 'static>(
    rows: &[TreeRow],
    visible: Range<usize>,
    selected: Option<u64>,
) -> AnyView<Action> {
    let visible = visible_rows(visible, rows.len());
    column(views(rows[visible].iter().map(|item| {
        row((
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
            .key("selection"),
            label(item.label.clone()).key("label"),
        ))
        .key(item.id)
    })))
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
    selected: Option<u64>,
) -> AnyView<Action> {
    virtual_table_with_widths(rows, visible, selected, &[])
}

/// Builds a keyed visible table range with controlled column widths.
///
/// The selected row is highlighted and publishes its selection to
/// accessibility; selection follows the row identity, not its position.
pub fn virtual_table_with_widths<Action: 'static>(
    rows: &[TableRow],
    visible: Range<usize>,
    selected: Option<u64>,
    widths: &[f32],
) -> AnyView<Action> {
    let visible = visible_rows(visible, rows.len());
    column(views(rows[visible].iter().map(|item| {
        let row_selected = selected == Some(item.id);
        let marker = panel(
            LogicalSize::new(4.0, 24.0),
            if row_selected {
                ColorRole::Accent
            } else {
                ColorRole::Transparent
            },
            Some(SemanticData {
                role: SemanticRole::Row,
                label: item.cells.join(" "),
                selected: Some(row_selected),
                ..SemanticData::default()
            }),
        )
        .key("selection");
        // A cell's identity inside its row is its column.
        let cells = item.cells.iter().enumerate().map(|(index, cell)| {
            label_with_width(cell.clone(), widths.get(index).copied()).key(index as u64)
        });
        row(views(std::iter::once(marker).chain(cells))).key(item.id)
    })))
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

#[cfg(test)]
mod tests {
    use astrelis_core::geometry::LogicalSize;
    use astrelis_ui_next::SemanticNode;
    use rxui_core::{Component, ComponentContext, ComponentHost, Theme, View};

    use super::*;

    #[test]
    fn a_visible_range_inside_the_rows_is_used_as_given() {
        assert_eq!(visible_rows(2..5, 8), 2..5);
        assert_eq!(visible_rows(0..8, 8), 0..8);
    }

    #[test]
    fn a_visible_range_past_the_last_row_is_truncated() {
        assert_eq!(visible_rows(6..40, 8), 6..8);
        assert_eq!(visible_rows(40..80, 8), 8..8);
        assert_eq!(visible_rows(0..4, 0), 0..0);
    }

    #[test]
    fn an_inverted_visible_range_selects_nothing() {
        // Written as struct literals: `5..2` is a compile-time lint, but a
        // scroll position computed at runtime can still invert.
        assert_eq!(visible_rows(Range { start: 5, end: 2 }, 8), 2..2);
        assert_eq!(visible_rows(Range { start: 9, end: 1 }, 8), 1..1);
    }

    struct Table {
        rows: Vec<TableRow>,
        selected: Option<u64>,
        widths: Vec<f32>,
    }

    impl Component for Table {
        type Action = ();
        type Effect = ();

        fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

        fn view(&self, _theme: &Theme) -> View<()> {
            virtual_table_with_widths(&self.rows, 0..8, self.selected, &self.widths)
        }
    }

    fn table_row(id: u64, name: &str) -> TableRow {
        TableRow {
            id,
            cells: vec![name.into(), format!("{id}")],
        }
    }

    fn rows(host: &ComponentHost<Table>) -> Vec<SemanticNode> {
        host.ui()
            .semantic_snapshot()
            .into_iter()
            .filter(|node| node.data.role == SemanticRole::Row)
            .collect()
    }

    fn host(selected: Option<u64>) -> ComponentHost<Table> {
        ComponentHost::new(
            Table {
                rows: vec![table_row(20, "Beta"), table_row(30, "Gamma")],
                selected,
                widths: vec![80.0, 40.0],
            },
            LogicalSize::new(320.0, 240.0),
            Theme::dark(),
        )
        .unwrap()
    }

    #[test]
    fn the_selected_table_row_publishes_its_selection() {
        let host = host(Some(30));
        let rows = rows(&host);
        assert_eq!(rows.len(), 2);
        for node in rows {
            assert_eq!(
                node.data.selected,
                Some(node.data.label == "Gamma 30"),
                "{:?}",
                node.data
            );
        }
    }

    #[test]
    fn no_table_row_is_selected_without_a_selection() {
        for selection in [None, Some(999)] {
            let host = host(selection);
            assert!(
                rows(&host)
                    .iter()
                    .all(|node| node.data.selected == Some(false)),
                "{selection:?}"
            );
        }
    }

    #[test]
    fn table_selection_follows_the_row_not_its_position() {
        let mut host = host(Some(30));
        host.component_mut().rows.insert(0, table_row(10, "Alpha"));
        host.refresh().unwrap();
        // Accessibility order is retained-arena order, so match on labels.
        let selection = rows(&host)
            .into_iter()
            .map(|node| (node.data.label, node.data.selected))
            .collect::<Vec<_>>();
        assert_eq!(selection.len(), 3);
        for (label, selected) in selection {
            assert_eq!(selected, Some(label == "Gamma 30"), "{label}");
        }
    }

    #[test]
    fn table_columns_keep_their_controlled_widths_beside_the_selection_marker() {
        let host = host(None);
        let labels = host
            .ui()
            .semantic_snapshot()
            .into_iter()
            .filter(|node| node.data.role == SemanticRole::Label)
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 4);
        assert!(
            labels
                .iter()
                .all(|node| node.bounds.size.width == 80.0 || node.bounds.size.width == 40.0),
            "{labels:?}"
        );
    }
}
