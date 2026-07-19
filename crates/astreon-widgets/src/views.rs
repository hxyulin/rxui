//! Virtualized editor tree and table views.

use std::{any::Any, collections::BTreeSet, fmt::Display, rc::Rc};

use astrelis_core::geometry::{LogicalRect, LogicalSize, Size};
use astrelis_paint::{Brush, Painter};
use astrelis_platform::{CursorIcon, DeviceId, ElementState, Key, NamedKey, PointerButton};
use astrelis_ui_core::{
    Alignment, Column, ElementHandle, EventContext, EventFilter, LayoutStyle, Length, RoutedEvent,
    RoutedEventKind, SemanticAction, SemanticActionKind, SemanticRole, Theme, Ui, UiError, Widget,
    WidgetContainerStyle,
};
use astrelis_ui_widgets::{VirtualList, VirtualListItem, VirtualListOptions};

/// One application-owned node displayed by a [`TreeView`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode<Id> {
    /// Stable application identity.
    pub id: Id,
    /// User-visible label.
    pub label: String,
    /// Nested child nodes.
    pub children: Vec<Self>,
    /// Initial/controlled expansion state.
    pub expanded: bool,
}

impl<Id> TreeNode<Id> {
    /// Creates a collapsed leaf node.
    pub fn leaf(id: Id, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            children: Vec::new(),
            expanded: false,
        }
    }

    /// Replaces the child collection.
    #[must_use]
    pub fn children(mut self, children: Vec<Self>) -> Self {
        self.children = children;
        self
    }

    /// Sets expansion state.
    #[must_use]
    pub const fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }
}

/// User interaction emitted by a [`TreeView`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeAction<Id> {
    /// Select one node.
    Select(Id),
    /// Activate one node, conventionally by Enter or double click.
    Activate(Id),
    /// Change whether a branch is expanded.
    SetExpanded {
        /// Stable node identity.
        id: Id,
        /// Requested expansion state.
        expanded: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FlatNode<Id> {
    id: Id,
    label: String,
    depth: usize,
    has_children: bool,
    expanded: bool,
}

/// Virtualized, controlled single-selection hierarchy view.
pub struct TreeView<Id, Message> {
    list: VirtualList,
    flat: Vec<FlatNode<Id>>,
    map_action: Rc<dyn Fn(TreeAction<Id>) -> Message>,
}

impl<Id, Message> TreeView<Id, Message>
where
    Id: Clone + Eq + 'static,
    Message: 'static,
{
    /// Creates an empty tree view.
    pub fn new<T>(
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        map_action: impl Fn(TreeAction<Id>) -> Message + 'static,
    ) -> Result<Self, UiError> {
        let list = VirtualList::new(
            ui,
            parent,
            VirtualListOptions {
                item_extent: 30.0,
                overscan: 4,
            },
        )?;
        ui.set_semantic_role(list.scroll_view(), SemanticRole::Tree)?;
        Ok(Self {
            list,
            flat: Vec::new(),
            map_action: Rc::new(map_action),
        })
    }

    /// Returns the retained scroll surface.
    pub const fn root(&self) -> ElementHandle<astrelis_ui_core::ScrollView> {
        self.list.scroll_view()
    }

    /// Returns the number of retained rows after virtualization.
    pub fn realized_count(&self) -> usize {
        self.list.realized_count()
    }

    /// Reconciles visible nodes from the controlled hierarchy and selection.
    pub fn sync(
        &mut self,
        ui: &mut Ui<Message>,
        nodes: &[TreeNode<Id>],
        selected: Option<&Id>,
    ) -> Result<(), UiError> {
        let mut flat = Vec::new();
        flatten(nodes, 0, &mut flat);
        if flat != self.flat {
            self.flat = flat;
            self.list.invalidate_all(ui)?;
        }
        let selected_index =
            selected.and_then(|id| self.flat.iter().position(|node| &node.id == id));
        self.list.sync(ui, self.flat.len(), {
            let flat = self.flat.clone();
            let map = self.map_action.clone();
            move |ui, item, index| {
                build_tree_row(ui, item, &flat, index, map.clone(), selected_index)
            }
        })?;
        self.list.set_selected(ui, selected_index)
    }
}

fn flatten<Id: Clone>(nodes: &[TreeNode<Id>], depth: usize, output: &mut Vec<FlatNode<Id>>) {
    for node in nodes {
        output.push(FlatNode {
            id: node.id.clone(),
            label: node.label.clone(),
            depth,
            has_children: !node.children.is_empty(),
            expanded: node.expanded,
        });
        if node.expanded {
            flatten(&node.children, depth + 1, output);
        }
    }
}

fn build_tree_row<Id, Message>(
    ui: &mut Ui<Message>,
    item: ElementHandle<VirtualListItem>,
    flat: &[FlatNode<Id>],
    index: usize,
    map: Rc<dyn Fn(TreeAction<Id>) -> Message>,
    selected: Option<usize>,
) -> Result<(), UiError>
where
    Id: Clone + Eq + 'static,
    Message: 'static,
{
    let node = flat[index].clone();
    ui.set_semantic_role(item, SemanticRole::TreeItem)?;
    ui.set_semantic_selected(item, Some(selected == Some(index)))?;
    ui.set_semantic_expanded(item, node.has_children.then_some(node.expanded))?;
    ui.set_semantic_description(item, Some(format!("Level {}", node.depth + 1)))?;
    let row = ui.add_row(item)?;
    ui.set_flex(row, 4.0, Alignment::Center)?;
    ui.set_layout(
        row,
        LayoutStyle {
            width: Length::Percent(1.0),
            ..Default::default()
        },
    )?;
    let indent = ui.add_label(row, "")?;
    ui.set_layout(
        indent,
        LayoutStyle {
            width: Length::Px(node.depth as f32 * 16.0),
            shrink: 0.0,
            ..Default::default()
        },
    )?;
    if node.has_children {
        let disclosure = ui.add_button(row, if node.expanded { "Collapse" } else { "Expand" })?;
        ui.set_layout(
            disclosure,
            LayoutStyle {
                width: Length::Px(66.0),
                shrink: 0.0,
                ..Default::default()
            },
        )?;
        let id = node.id.clone();
        let mapper = map.clone();
        let expanded = !node.expanded;
        ui.listen(
            disclosure,
            None,
            EventFilter::Activate,
            move |context, _| {
                context.emit(mapper(TreeAction::SetExpanded {
                    id: id.clone(),
                    expanded,
                }));
            },
        )?;
    } else {
        let spacer = ui.add_label(row, "")?;
        ui.set_layout(
            spacer,
            LayoutStyle {
                width: Length::Px(66.0),
                shrink: 0.0,
                ..Default::default()
            },
        )?;
    }
    ui.add_label(row, &node.label)?;
    let id = node.id.clone();
    let mapper = map.clone();
    ui.listen(item, None, EventFilter::Pointer, move |context, event| {
        if matches!(
            event.kind,
            RoutedEventKind::PointerButton {
                button: PointerButton::Primary,
                state: ElementState::Pressed,
                ..
            }
        ) {
            context.emit(mapper(TreeAction::Select(id.clone())));
        }
    })?;
    let id = node.id;
    let mapper = map;
    let has_children = node.has_children;
    let expanded = node.expanded;
    ui.listen(item, None, EventFilter::Keyboard, move |context, event| {
        if let RoutedEventKind::Keyboard(input) = &event.kind
            && input.state == ElementState::Pressed
        {
            match &input.logical_key {
                Key::Named(NamedKey::Enter | NamedKey::Space) => {
                    context.emit(mapper(TreeAction::Activate(id.clone())))
                }
                Key::Named(NamedKey::Other(key))
                    if has_children && key == "ArrowRight" && !expanded =>
                {
                    context.emit(mapper(TreeAction::SetExpanded {
                        id: id.clone(),
                        expanded: true,
                    }));
                }
                Key::Named(NamedKey::Other(key))
                    if has_children && key == "ArrowLeft" && expanded =>
                {
                    context.emit(mapper(TreeAction::SetExpanded {
                        id: id.clone(),
                        expanded: false,
                    }));
                }
                _ => {}
            }
        }
    })?;
    Ok(())
}

/// Sort direction requested for a table column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// Smallest values first.
    Ascending,
    /// Largest values first.
    Descending,
}

/// Controlled table sort state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSort<ColumnId> {
    /// Column providing sort keys.
    pub column: ColumnId,
    /// Requested ordering.
    pub direction: SortDirection,
}

/// One table column and its controlled width.
#[derive(Clone, Debug, PartialEq)]
pub struct TableColumn<Id> {
    /// Stable application identity.
    pub id: Id,
    /// User-visible header.
    pub label: String,
    /// Controlled logical width.
    pub width: f32,
    /// Smallest accepted logical width.
    pub min_width: f32,
    /// Largest accepted logical width.
    pub max_width: f32,
    /// Whether activating the header requests sorting.
    pub sortable: bool,
}

impl<Id> TableColumn<Id> {
    /// Creates a sortable column with conventional width limits.
    pub fn new(id: Id, label: impl Into<String>, width: f32) -> Self {
        Self {
            id,
            label: label.into(),
            width,
            min_width: 48.0,
            max_width: 800.0,
            sortable: true,
        }
    }
}

/// One application-owned table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow<Id> {
    /// Stable application identity.
    pub id: Id,
    /// Cell text in column order.
    pub cells: Vec<String>,
}

/// User interaction emitted by a [`TableView`].
#[derive(Clone, Debug, PartialEq)]
pub enum TableAction<RowId, ColumnId> {
    /// Select one row.
    Select(RowId),
    /// Activate one row.
    Activate(RowId),
    /// Replace controlled sort state.
    SetSort(TableSort<ColumnId>),
    /// Replace one controlled column width.
    ResizeColumn {
        /// Stable column identity.
        column: ColumnId,
        /// Requested clamped logical width.
        width: f32,
    },
}

/// Virtualized single-selection table with sortable headers.
pub struct TableView<RowId, ColumnId, Message> {
    root: ElementHandle<Column>,
    header: ElementHandle<astrelis_ui_core::Row>,
    list: VirtualList,
    header_cells: Vec<ElementHandle<astrelis_ui_core::Row>>,
    columns: Vec<TableColumn<ColumnId>>,
    rows: Vec<TableRow<RowId>>,
    sort: Option<TableSort<ColumnId>>,
    map_action: Rc<dyn Fn(TableAction<RowId, ColumnId>) -> Message>,
}

impl<RowId, ColumnId, Message> TableView<RowId, ColumnId, Message>
where
    RowId: Clone + Eq + 'static,
    ColumnId: Clone + Eq + 'static,
    Message: 'static,
{
    /// Creates an empty table.
    pub fn new<T>(
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        map_action: impl Fn(TableAction<RowId, ColumnId>) -> Message + 'static,
    ) -> Result<Self, UiError> {
        let root = ui.add_column(parent)?;
        ui.set_semantic_role(root, SemanticRole::Table)?;
        let header = ui.add_row(root)?;
        ui.set_flex(header, 0.0, Alignment::Stretch)?;
        ui.set_semantic_role(header, SemanticRole::TableHeader)?;
        let list = VirtualList::new(
            ui,
            root,
            VirtualListOptions {
                item_extent: 30.0,
                overscan: 4,
            },
        )?;
        ui.set_layout(
            list.scroll_view(),
            LayoutStyle {
                grow: 1.0,
                ..Default::default()
            },
        )?;
        Ok(Self {
            root,
            header,
            list,
            header_cells: Vec::new(),
            columns: Vec::new(),
            rows: Vec::new(),
            sort: None,
            map_action: Rc::new(map_action),
        })
    }

    /// Returns the table root.
    pub const fn root(&self) -> ElementHandle<Column> {
        self.root
    }

    /// Returns the number of retained data rows after virtualization.
    pub fn realized_count(&self) -> usize {
        self.list.realized_count()
    }

    /// Reconciles controlled columns, rows, sort, and selection.
    pub fn sync(
        &mut self,
        ui: &mut Ui<Message>,
        columns: &[TableColumn<ColumnId>],
        rows: &[TableRow<RowId>],
        sort: Option<&TableSort<ColumnId>>,
        selected: Option<&RowId>,
    ) -> Result<(), UiError> {
        if self.columns != columns || self.sort.as_ref() != sort {
            for child in self.header_cells.drain(..) {
                ui.remove(child)?;
            }
            self.columns = columns.to_vec();
            self.sort = sort.cloned();
            self.header_cells = build_headers(
                ui,
                self.header,
                &self.columns,
                sort,
                self.map_action.clone(),
            )?;
            self.list.invalidate_all(ui)?;
        }
        if self.rows != rows {
            self.rows = rows.to_vec();
            self.list.invalidate_all(ui)?;
        }
        let selected_index = selected.and_then(|id| self.rows.iter().position(|row| &row.id == id));
        self.list.sync(ui, self.rows.len(), {
            let rows = self.rows.clone();
            let columns = self.columns.clone();
            let map = self.map_action.clone();
            move |ui, item, index| {
                build_table_row(
                    ui,
                    item,
                    &rows[index],
                    &columns,
                    map.clone(),
                    selected_index == Some(index),
                )
            }
        })?;
        self.list.set_selected(ui, selected_index)
    }
}

fn build_headers<RowId, ColumnId, Message>(
    ui: &mut Ui<Message>,
    header: ElementHandle<astrelis_ui_core::Row>,
    columns: &[TableColumn<ColumnId>],
    sort: Option<&TableSort<ColumnId>>,
    map: Rc<dyn Fn(TableAction<RowId, ColumnId>) -> Message>,
) -> Result<Vec<ElementHandle<astrelis_ui_core::Row>>, UiError>
where
    RowId: Clone + 'static,
    ColumnId: Clone + Eq + 'static,
    Message: 'static,
{
    let mut cells = Vec::new();
    for column in columns {
        let cell = ui.add_row(header)?;
        ui.set_flex(cell, 0.0, Alignment::Stretch)?;
        ui.set_layout(
            cell,
            LayoutStyle {
                width: Length::Px(clamp_column_width(
                    column.width,
                    column.min_width,
                    column.max_width,
                )),
                shrink: 0.0,
                ..Default::default()
            },
        )?;
        let marker = sort
            .filter(|value| value.column == column.id)
            .map(|value| match value.direction {
                SortDirection::Ascending => " ↑",
                SortDirection::Descending => " ↓",
            })
            .unwrap_or("");
        let button = ui.add_button(cell, format!("{}{marker}", column.label))?;
        ui.set_semantic_role(button, SemanticRole::ColumnHeader)?;
        ui.set_layout(
            button,
            LayoutStyle {
                grow: 1.0,
                ..Default::default()
            },
        )?;
        if column.sortable {
            let next = match sort
                .filter(|value| value.column == column.id)
                .map(|value| value.direction)
            {
                Some(SortDirection::Ascending) => SortDirection::Descending,
                _ => SortDirection::Ascending,
            };
            let id = column.id.clone();
            let mapper = map.clone();
            ui.listen(button, None, EventFilter::Activate, move |context, _| {
                context.emit(mapper(TableAction::SetSort(TableSort {
                    column: id.clone(),
                    direction: next,
                })))
            })?;
        }
        let resizer = ui.add_widget(
            cell,
            ColumnResizer::new(
                column.id.clone(),
                column.width,
                column.min_width,
                column.max_width,
                map.clone(),
            ),
        )?;
        ui.set_layout(
            resizer,
            LayoutStyle {
                width: Length::Px(6.0),
                height: Length::Percent(1.0),
                shrink: 0.0,
                ..Default::default()
            },
        )?;
        cells.push(cell);
    }
    Ok(cells)
}

struct ColumnResizer<RowId, ColumnId, Message> {
    column: ColumnId,
    width: f32,
    min_width: f32,
    max_width: f32,
    dragging: Option<(DeviceId, f32, f32)>,
    map: Rc<dyn Fn(TableAction<RowId, ColumnId>) -> Message>,
}

impl<RowId, ColumnId, Message> ColumnResizer<RowId, ColumnId, Message> {
    fn new(
        column: ColumnId,
        width: f32,
        min_width: f32,
        max_width: f32,
        map: Rc<dyn Fn(TableAction<RowId, ColumnId>) -> Message>,
    ) -> Self {
        Self {
            column,
            width,
            min_width,
            max_width,
            dragging: None,
            map,
        }
    }
}

impl<RowId, ColumnId, Message> Widget<Message> for ColumnResizer<RowId, ColumnId, Message>
where
    RowId: Clone + 'static,
    ColumnId: Clone + 'static,
    Message: 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn intrinsic_size(&self, _theme: &Theme) -> LogicalSize {
        Size::new(6.0, 30.0)
    }
    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle::structural()
    }
    fn hit_testable(&self) -> bool {
        true
    }
    fn focusable(&self) -> bool {
        true
    }
    fn cursor_icon(&self) -> Option<CursorIcon> {
        Some(CursorIcon::EwResize)
    }
    fn event(&mut self, context: &mut EventContext<'_, Message>, event: &RoutedEvent) {
        match &event.kind {
            RoutedEventKind::PointerButton {
                device_id,
                position,
                button: PointerButton::Primary,
                state: ElementState::Pressed,
            } => {
                self.dragging = Some((*device_id, position.x, self.width));
                context.capture_pointer(*device_id);
                context.request_focus();
            }
            RoutedEventKind::PointerMoved {
                device_id,
                position,
            } if self.dragging.is_some_and(|(id, _, _)| id == *device_id) => {
                let (_, start, _) = self.dragging.expect("checked");
                let width = clamp_column_width(
                    self.width + position.x - start,
                    self.min_width,
                    self.max_width,
                );
                self.dragging = Some((*device_id, start, width));
                context.request_paint();
            }
            RoutedEventKind::PointerButton {
                device_id,
                button: PointerButton::Primary,
                state: ElementState::Released,
                ..
            } if self.dragging.is_some_and(|(id, _, _)| id == *device_id) => {
                let (_, _, width) = self.dragging.take().expect("checked");
                context.release_pointer(*device_id);
                context.emit((self.map)(TableAction::ResizeColumn {
                    column: self.column.clone(),
                    width,
                }));
            }
            RoutedEventKind::PointerCancelled { device_id }
                if self.dragging.is_some_and(|(id, _, _)| id == *device_id) =>
            {
                self.dragging = None;
                context.release_pointer(*device_id);
            }
            RoutedEventKind::Keyboard(input) if input.state == ElementState::Pressed => {
                let delta = match &input.logical_key {
                    Key::Named(NamedKey::Other(key)) if key == "ArrowLeft" => -8.0,
                    Key::Named(NamedKey::Other(key)) if key == "ArrowRight" => 8.0,
                    _ => return,
                };
                context.emit((self.map)(TableAction::ResizeColumn {
                    column: self.column.clone(),
                    width: clamp_column_width(self.width + delta, self.min_width, self.max_width),
                }));
                context.prevent_default();
            }
            _ => {}
        }
    }
    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        let divider = LogicalRect::from_xywh(
            bounds.origin.x + (bounds.size.width - 1.0) * 0.5,
            bounds.origin.y,
            1.0,
            bounds.size.height,
        );
        painter.fill_rect(divider, Brush::Solid(theme.border))?;
        Ok(())
    }
    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((
            SemanticRole::Separator,
            "Resize column".into(),
            Some(self.width.to_string()),
        ))
    }
    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![SemanticActionKind::Focus, SemanticActionKind::SetValue]
    }
    fn semantic_action(
        &mut self,
        context: &mut EventContext<'_, Message>,
        action: &SemanticAction,
    ) -> bool {
        match action {
            SemanticAction::Focus => {
                context.request_focus();
                true
            }
            SemanticAction::SetValue(width) => {
                context.emit((self.map)(TableAction::ResizeColumn {
                    column: self.column.clone(),
                    width: clamp_column_width(*width, self.min_width, self.max_width),
                }));
                true
            }
            _ => false,
        }
    }
}

fn build_table_row<RowId, ColumnId, Message>(
    ui: &mut Ui<Message>,
    item: ElementHandle<VirtualListItem>,
    row_data: &TableRow<RowId>,
    columns: &[TableColumn<ColumnId>],
    map: Rc<dyn Fn(TableAction<RowId, ColumnId>) -> Message>,
    selected: bool,
) -> Result<(), UiError>
where
    RowId: Clone + 'static,
    ColumnId: Clone + 'static,
    Message: 'static,
{
    ui.set_semantic_role(item, SemanticRole::TableRow)?;
    ui.set_semantic_selected(item, Some(selected))?;
    let row = ui.add_row(item)?;
    ui.set_flex(row, 0.0, Alignment::Stretch)?;
    for (index, column) in columns.iter().enumerate() {
        let cell = ui.add_label(
            row,
            row_data
                .cells
                .get(index)
                .map(String::as_str)
                .unwrap_or_default(),
        )?;
        ui.set_semantic_role(cell, SemanticRole::Cell)?;
        ui.set_layout(
            cell,
            LayoutStyle {
                width: Length::Px(column.width.clamp(column.min_width, column.max_width)),
                shrink: 0.0,
                ..Default::default()
            },
        )?;
    }
    let id = row_data.id.clone();
    let mapper = map.clone();
    ui.listen(item, None, EventFilter::Pointer, move |context, event| {
        if matches!(
            event.kind,
            RoutedEventKind::PointerButton {
                button: PointerButton::Primary,
                state: ElementState::Pressed,
                ..
            }
        ) {
            context.emit(mapper(TableAction::Select(id.clone())));
        }
    })?;
    let id = row_data.id.clone();
    ui.listen(item, None, EventFilter::Keyboard, move |context, event| {
        if let RoutedEventKind::Keyboard(input) = &event.kind
            && input.state == ElementState::Pressed
            && matches!(
                input.logical_key,
                Key::Named(NamedKey::Enter | NamedKey::Space)
            )
        {
            context.emit(map(TableAction::Activate(id.clone())));
        }
    })?;
    Ok(())
}

/// Validates and clamps a requested column width.
pub fn clamp_column_width(width: f32, min_width: f32, max_width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(min_width.max(1.0), max_width.max(min_width.max(1.0)))
    } else {
        min_width.max(1.0)
    }
}

/// Returns stable identities found more than once in one tree.
pub fn duplicate_tree_ids<Id>(nodes: &[TreeNode<Id>]) -> BTreeSet<Id>
where
    Id: Clone + Ord,
{
    fn visit<Id: Clone + Ord>(
        nodes: &[TreeNode<Id>],
        seen: &mut BTreeSet<Id>,
        duplicates: &mut BTreeSet<Id>,
    ) {
        for node in nodes {
            if !seen.insert(node.id.clone()) {
                duplicates.insert(node.id.clone());
            }
            visit(&node.children, seen, duplicates);
        }
    }
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    visit(nodes, &mut seen, &mut duplicates);
    duplicates
}

impl<ColumnId: Display> Display for TableSort<ColumnId> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}", self.column, self.direction)
    }
}

#[cfg(test)]
mod tests {
    use astrelis_core::geometry::Size;
    use astrelis_text::FontDatabase;
    use astrelis_ui_core::{SemanticNode, Theme};

    use super::*;

    fn semantic_nodes(node: &SemanticNode, output: &mut Vec<SemanticNode>) {
        output.push(node.clone());
        for child in &node.children {
            semantic_nodes(child, output);
        }
    }

    #[test]
    fn flatten_obeys_expansion_and_reports_duplicates() {
        let nodes = vec![
            TreeNode::leaf(1, "Root")
                .expanded(true)
                .children(vec![TreeNode::leaf(2, "Child"), TreeNode::leaf(2, "Again")]),
        ];
        let mut flat = Vec::new();
        flatten(&nodes, 0, &mut flat);
        assert_eq!(
            flat.iter().map(|node| node.depth).collect::<Vec<_>>(),
            vec![0, 1, 1]
        );
        assert_eq!(duplicate_tree_ids(&nodes), BTreeSet::from([2]));
    }

    #[test]
    fn column_widths_repair_non_finite_and_clamp() {
        assert_eq!(clamp_column_width(f32::NAN, 48.0, 200.0), 48.0);
        assert_eq!(clamp_column_width(900.0, 48.0, 200.0), 200.0);
    }

    #[test]
    fn tree_virtualizes_and_exposes_selection_and_expansion_semantics() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        ui.set_viewport(Size::new(400.0, 240.0), 1.0);
        let root = ui.root();
        let mut tree = TreeView::new(&mut ui, root, |_| ()).unwrap();
        ui.set_layout(
            tree.root(),
            LayoutStyle {
                height: Length::Px(180.0),
                ..Default::default()
            },
        )
        .unwrap();
        let nodes = (0..10_000)
            .map(|id| TreeNode::leaf(id, format!("Node {id}")))
            .collect::<Vec<_>>();
        tree.sync(&mut ui, &nodes, Some(&2)).unwrap();
        assert!(tree.realized_count() < 20);
        let mut semantics = Vec::new();
        semantic_nodes(&ui.semantic_tree().unwrap(), &mut semantics);
        assert!(semantics.iter().any(|node| node.role == SemanticRole::Tree));
        assert!(
            semantics
                .iter()
                .any(|node| node.role == SemanticRole::TreeItem && node.selected == Some(true))
        );
    }

    #[test]
    fn table_virtualizes_and_exposes_table_semantics() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        ui.set_viewport(Size::new(500.0, 260.0), 1.0);
        let root = ui.root();
        let mut table = TableView::new(&mut ui, root, |_| ()).unwrap();
        ui.set_layout(
            table.root(),
            LayoutStyle {
                height: Length::Px(220.0),
                ..Default::default()
            },
        )
        .unwrap();
        let columns = vec![TableColumn::new("name", "Name", 180.0)];
        let rows = (0..10_000)
            .map(|id| TableRow {
                id,
                cells: vec![format!("Row {id}")],
            })
            .collect::<Vec<_>>();
        table
            .sync(&mut ui, &columns, &rows, None, Some(&3))
            .unwrap();
        assert!(table.realized_count() < 20);
        let mut semantics = Vec::new();
        semantic_nodes(&ui.semantic_tree().unwrap(), &mut semantics);
        assert!(
            semantics
                .iter()
                .any(|node| node.role == SemanticRole::Table)
        );
        assert!(
            semantics
                .iter()
                .any(|node| node.role == SemanticRole::ColumnHeader)
        );
        assert!(
            semantics
                .iter()
                .any(|node| node.role == SemanticRole::TableRow && node.selected == Some(true))
        );
    }
}
