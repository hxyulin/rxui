//! Keyed flattened tree rows.

use std::ops::Range;

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::{SemanticData, SemanticRole};

use crate::{
    AnyView, ColorRole, column, data::visible_rows, label, panel_with_semantics, row, views,
};

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
    let visible = visible_rows(visible, rows.len());
    column(views(rows[visible].iter().map(|item| {
        row((
            panel_with_semantics(
                LogicalSize::new(8.0 + item.depth as f32 * 12.0, 24.0),
                if selected == Some(item.id) {
                    ColorRole::Accent
                } else {
                    ColorRole::Surface
                },
                SemanticData {
                    role: SemanticRole::Row,
                    label: item.label.clone(),
                    selected: Some(selected == Some(item.id)),
                    ..SemanticData::default()
                },
            )
            .key("selection"),
            label(item.label.clone()).key("label"),
        ))
        .key(item.id)
    })))
}

/// Builds a retained placeholder for an application-rendered viewport.
///
/// The panel only reserves the space and announces the role; nothing is drawn
/// into it. Use [`crate::media::render_view`] for a viewport the application
/// actually renders.
pub fn render_view_placeholder<Action: 'static>(label: impl Into<String>) -> AnyView<Action> {
    panel_with_semantics(
        LogicalSize::new(640.0, 360.0),
        ColorRole::Background,
        SemanticData {
            role: SemanticRole::RenderView,
            label: label.into(),
            ..SemanticData::default()
        },
    )
}
