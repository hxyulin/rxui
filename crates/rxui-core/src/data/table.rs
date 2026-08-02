//! Keyed controlled table rows.

use std::ops::Range;

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{SemanticData, SemanticRole};

use crate::{
    AnyView, ColorRole, column, data::visible_rows, label_with_width, panel_with_semantics, row,
    views,
};

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
        let marker = panel_with_semantics(
            LogicalSize::new(4.0, 24.0),
            if row_selected {
                ColorRole::Accent
            } else {
                ColorRole::Transparent
            },
            SemanticData {
                role: SemanticRole::Row,
                label: item.cells.join(" "),
                selected: Some(row_selected),
                ..SemanticData::default()
            },
        )
        .key("selection");
        // A cell's identity inside its row is its column.
        let cells = item.cells.iter().enumerate().map(|(index, cell)| {
            label_with_width(cell.clone(), widths.get(index).copied()).key(index as u64)
        });
        row(views(std::iter::once(marker).chain(cells))).key(item.id)
    })))
}

#[cfg(test)]
mod tests {
    use astrelis_core::geometry::LogicalSize;
    use astrelis_ui_next::SemanticNode;

    use super::*;
    use crate::{Component, ComponentContext, ComponentHost, Theme, View};

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
