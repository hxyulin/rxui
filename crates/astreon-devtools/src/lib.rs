//! Opt-in, custom-rendered developer tools for retained Astreon interfaces.

#![warn(missing_docs)]

mod details;
mod highlight;
mod model;

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use astrelis_core::geometry::LogicalSize;
use astrelis_platform::{
    ElementState, Key, KeyCode, Modifiers, NamedKey, PhysicalKey, PointerButton,
};
use astrelis_ui_core::{
    Alignment, Column, Edges, ElementHandle, ElementId, ElementInspection, EventFilter,
    FocusScopeOptions, Insets, Label, LayoutStyle, Length, Overlay, OverlayAlignment,
    OverlayOptions, OverlaySide, Padding, Positioning, Row, RoutedEventKind, SemanticRole, Ui,
    UiError, Visibility, WidgetStyle,
};
use astreon_widgets::{
    CommandButton, IconButton, TreeAction, TreeView, TreeViewOptions, icons,
};

use crate::{
    details::build_details,
    highlight::{BandSet, Highlight, clipped_bounds},
    model::{Model, RowMeta, kind_color, label_color, row_meta, semantic_labels, tree_nodes},
};

const INSPECTOR_Z: i32 = 20_000;
const TREE_ROW_EXTENT: f32 = 24.0;
const INFO_TAG_HEIGHT: f32 = 20.0;

/// Configuration for an in-application UI inspector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InspectorOptions {
    /// Whether the details panel starts open.
    pub initially_open: bool,
    /// Logical width of the details panel.
    pub panel_width: f32,
    /// Whether a small launcher remains visible while the panel is closed.
    pub show_launcher: bool,
}

impl Default for InspectorOptions {
    fn default() -> Self {
        Self {
            initially_open: false,
            panel_width: 340.0,
            show_launcher: true,
        }
    }
}

/// Controlled interaction emitted by [`UiInspector`].
///
/// Carries a `String` payload for tree filtering, so unlike releases before
/// 0.6 the action is `Clone` but no longer `Copy`.
#[derive(Clone, Debug, PartialEq)]
pub enum InspectorAction {
    /// Toggle panel visibility.
    Toggle,
    /// Open the details panel.
    Open,
    /// Close the details panel.
    Close,
    /// Start or stop selecting an application element with the pointer.
    SetPicking(bool),
    /// Invert the current picking state.
    TogglePicking,
    /// Select one retained element.
    Select(ElementId),
    /// Change whether one tree branch is expanded.
    SetExpanded {
        /// Element whose branch changed.
        id: ElementId,
        /// Requested expansion state.
        expanded: bool,
    },
    /// Preview one element under the pointer while picking.
    SetHover(Option<ElementId>),
    /// Replace the tree filter text.
    SetFilter(String),
    /// Re-sync retained rows, e.g. after scrolling the virtualized tree.
    Refresh,
}

/// Read-only in-application inspector for one retained UI tree.
///
/// Applications route emitted [`InspectorAction`] values back through
/// [`UiInspector::apply`], alongside their ordinary controlled messages.
pub struct UiInspector<Message> {
    panel: ElementHandle<Overlay>,
    launcher: ElementHandle<Overlay>,
    highlight_overlay: ElementHandle<Overlay>,
    highlight: ElementHandle<Highlight>,
    info_tag: ElementHandle<Label>,
    pick: ElementHandle<CommandButton<Message>>,
    tree: TreeView<ElementId, Message>,
    details_pad: ElementHandle<Padding>,
    details: ElementHandle<Column>,
    crumb_bar: ElementHandle<Row>,
    crumbs: ElementHandle<Row>,
    owned: Rc<RefCell<HashSet<ElementId>>>,
    picking: Rc<Cell<bool>>,
    open_state: Rc<Cell<bool>>,
    hover_cell: Rc<Cell<Option<ElementId>>>,
    open: bool,
    selected: Option<ElementId>,
    hovered: Option<ElementId>,
    filter: String,
    expanded: HashSet<ElementId>,
    seeded: bool,
    cache: HashMap<ElementId, ElementInspection>,
    meta: Rc<RefCell<HashMap<ElementId, RowMeta>>>,
    viewport: LogicalSize,
    last_details: Option<(ElementId, ElementInspection)>,
    last_crumbs: Option<Option<ElementId>>,
    show_launcher: bool,
    map_action: Rc<dyn Fn(InspectorAction) -> Message>,
}

impl<Message> UiInspector<Message>
where
    Message: Clone + 'static,
{
    /// Mounts an inspector over the viewport owned by `ui.root()`.
    pub fn new(
        ui: &mut Ui<Message>,
        options: InspectorOptions,
        map_action: impl Fn(InspectorAction) -> Message + 'static,
    ) -> Result<Self, UiError> {
        let map_action: Rc<dyn Fn(InspectorAction) -> Message> = Rc::new(map_action);
        let picking = Rc::new(Cell::new(false));
        let open_state = Rc::new(Cell::new(options.initially_open));
        let owned = Rc::new(RefCell::new(HashSet::new()));
        let hover_cell = Rc::new(Cell::new(None));
        let meta = Rc::new(RefCell::new(HashMap::new()));
        let root = ui.root();
        let theme_surface = ui.theme().surface;
        let theme_border = ui.theme().border;
        let theme_overlay = ui.theme().overlay;
        let heading = ui.theme().type_scale.heading;
        let heading_weight = ui.theme().type_scale.heading_weight;
        let caption = ui.theme().type_scale.caption;
        let spacing_sm = ui.theme().spacing.sm;

        // Highlight overlay: box-model bands plus the floating info tag.
        let highlight_overlay = ui.add_overlay(
            root,
            OverlayOptions {
                side: OverlaySide::Center,
                alignment: OverlayAlignment::Center,
                z_index: INSPECTOR_Z,
                paint_surface: false,
                ..OverlayOptions::default()
            },
        )?;
        ui.set_layout(
            highlight_overlay,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let highlight = ui.add_widget(highlight_overlay, Highlight::default())?;
        ui.set_layout(
            highlight,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let info_tag = ui.add_label(highlight_overlay, "")?;
        ui.set_layout(
            info_tag,
            LayoutStyle {
                positioning: Positioning::Absolute,
                inset: Edges {
                    left: Length::Px(0.0),
                    top: Length::Px(0.0),
                    ..Edges::default()
                },
                ..LayoutStyle::default()
            },
        )?;
        ui.set_widget_style(
            info_tag,
            WidgetStyle {
                background: Some(theme_overlay),
                font_size: Some(caption),
                ..WidgetStyle::default()
            },
        )?;
        ui.set_visibility(info_tag, Visibility::Hidden)?;

        // Right-docked panel: chrome, search, tree, details, breadcrumbs.
        let panel = ui.add_overlay(
            root,
            OverlayOptions {
                side: OverlaySide::Right,
                alignment: OverlayAlignment::Start,
                z_index: INSPECTOR_Z + 1,
                focus: FocusScopeOptions {
                    restore_focus: true,
                    ..FocusScopeOptions::default()
                },
                ..OverlayOptions::default()
            },
        )?;
        ui.set_layout(
            panel,
            LayoutStyle {
                width: Length::Px(options.panel_width.max(220.0)),
                height: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        ui.set_widget_style(
            panel,
            WidgetStyle {
                background: Some(theme_surface),
                ..WidgetStyle::default()
            },
        )?;
        ui.set_semantic_role(panel, SemanticRole::Dialog)?;
        ui.set_semantic_description(panel, Some("Retained UI inspector".into()))?;

        let outer = ui.add_row(panel)?;
        ui.set_layout(
            outer,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let edge = ui.add_column(outer)?;
        ui.set_layout(
            edge,
            LayoutStyle {
                width: Length::Px(1.0),
                height: Length::Percent(1.0),
                shrink: 0.0,
                ..LayoutStyle::default()
            },
        )?;
        ui.set_widget_style(
            edge,
            WidgetStyle {
                background: Some(theme_border),
                ..WidgetStyle::default()
            },
        )?;
        let body = ui.add_column(outer)?;
        ui.set_layout(
            body,
            LayoutStyle {
                grow: 1.0,
                height: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;

        let header_pad = ui.add_padding(
            body,
            Insets {
                left: spacing_sm,
                top: spacing_sm,
                right: spacing_sm,
                bottom: spacing_sm,
            },
        )?;
        let header = ui.add_row(header_pad)?;
        ui.set_flex(header, 6.0, Alignment::Center)?;
        ui.set_layout(
            header,
            LayoutStyle {
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let title = ui.add_label(header, "UI Inspector")?;
        ui.set_widget_style(
            title,
            WidgetStyle {
                font_size: Some(heading),
                font_weight: Some(heading_weight),
                ..WidgetStyle::default()
            },
        )?;
        ui.set_layout(
            title,
            LayoutStyle {
                grow: 1.0,
                ..LayoutStyle::default()
            },
        )?;
        let pick = ui.add_widget(
            header,
            CommandButton::new("Pick", (map_action)(InspectorAction::TogglePicking))
                .icon(Some(icons::crosshair())),
        )?;
        let close = ui.add_widget(
            header,
            IconButton::icon_only(
                icons::close(),
                "Close inspector",
                (map_action)(InspectorAction::Close),
            ),
        )?;
        ui.set_layout(
            close,
            LayoutStyle {
                width: Length::Px(28.0),
                height: Length::Px(28.0),
                shrink: 0.0,
                ..LayoutStyle::default()
            },
        )?;

        let search_pad = ui.add_padding(
            body,
            Insets {
                left: spacing_sm,
                top: 0.0,
                right: spacing_sm,
                bottom: spacing_sm,
            },
        )?;
        let search = ui.add_text_field(search_pad, "")?;
        ui.set_placeholder(search, "Filter by kind, role, or label")?;
        ui.set_layout(
            search,
            LayoutStyle {
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let map = map_action.clone();
        ui.listen(search, None, EventFilter::ValueChanged, move |context, event| {
            if let RoutedEventKind::TextChanged(text) = &event.kind {
                context.emit(map(InspectorAction::SetFilter(text.clone())));
            }
        })?;

        let map = map_action.clone();
        let mut tree = TreeView::with_options(
            ui,
            body,
            TreeViewOptions {
                row_extent: TREE_ROW_EXTENT,
                indent_guides: true,
            },
            move |action| match action {
                TreeAction::Select(id) | TreeAction::Activate(id) => {
                    map(InspectorAction::Select(id))
                }
                TreeAction::SetExpanded { id, expanded } => {
                    map(InspectorAction::SetExpanded { id, expanded })
                }
            },
        )?;
        ui.set_layout(
            tree.root(),
            LayoutStyle {
                grow: 1.2,
                basis: Length::Px(0.0),
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        tree.set_row_content(ui, Self::row_renderer(meta.clone()))?;
        let map = map_action.clone();
        ui.listen(tree.root(), None, EventFilter::Scroll, move |context, _| {
            context.emit(map(InspectorAction::Refresh));
        })?;

        let divider = ui.add_column(body)?;
        ui.set_layout(
            divider,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Px(1.0),
                shrink: 0.0,
                ..LayoutStyle::default()
            },
        )?;
        ui.set_widget_style(
            divider,
            WidgetStyle {
                background: Some(theme_border),
                ..WidgetStyle::default()
            },
        )?;

        let details_scroll = ui.add_scroll_view(body)?;
        ui.set_layout(
            details_scroll,
            LayoutStyle {
                grow: 1.0,
                basis: Length::Px(0.0),
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let details_pad = ui.add_padding(
            details_scroll,
            Insets {
                left: 10.0,
                top: 6.0,
                right: 10.0,
                bottom: 10.0,
            },
        )?;
        let details = ui.add_column(details_pad)?;
        ui.set_layout(
            details,
            LayoutStyle {
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;

        let crumb_divider = ui.add_column(body)?;
        ui.set_layout(
            crumb_divider,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Px(1.0),
                shrink: 0.0,
                ..LayoutStyle::default()
            },
        )?;
        ui.set_widget_style(
            crumb_divider,
            WidgetStyle {
                background: Some(theme_border),
                ..WidgetStyle::default()
            },
        )?;
        let crumb_bar = ui.add_row(body)?;
        ui.set_flex(crumb_bar, 0.0, Alignment::Center)?;
        ui.set_layout(
            crumb_bar,
            LayoutStyle {
                width: Length::Percent(1.0),
                height: Length::Px(26.0),
                shrink: 0.0,
                ..LayoutStyle::default()
            },
        )?;
        let crumbs = ui.add_row(crumb_bar)?;

        let launcher = ui.add_overlay(
            root,
            OverlayOptions {
                side: OverlaySide::Right,
                alignment: OverlayAlignment::Start,
                z_index: INSPECTOR_Z + 2,
                ..OverlayOptions::default()
            },
        )?;
        let launch = ui.add_button(launcher, "Inspect")?;
        let map = map_action.clone();
        ui.listen(launch, None, EventFilter::Activate, move |context, _| {
            context.emit(map(InspectorAction::Open));
        })?;

        let map = map_action.clone();
        let picker = picking.clone();
        let inspector_open = open_state.clone();
        let inspector_nodes = owned.clone();
        let hover = hover_cell.clone();
        ui.listen(
            root,
            None,
            EventFilter::Any,
            move |context, event| match &event.kind {
                RoutedEventKind::Keyboard(input)
                    if input.state == ElementState::Pressed
                        && is_toggle_shortcut(input, context.modifiers()) =>
                {
                    context.prevent_default();
                    context.stop_propagation();
                    context.emit(map(InspectorAction::Toggle));
                }
                RoutedEventKind::Keyboard(input)
                    if input.state == ElementState::Pressed
                        && input.logical_key == Key::Named(NamedKey::Escape)
                        && inspector_open.get() =>
                {
                    context.prevent_default();
                    context.stop_propagation();
                    context.emit(map(if picker.get() {
                        InspectorAction::SetPicking(false)
                    } else {
                        InspectorAction::Close
                    }));
                }
                RoutedEventKind::PointerEntered { .. }
                    if picker.get()
                        && !inspector_nodes.borrow().contains(&event.target)
                        && hover.get() != Some(event.target) =>
                {
                    hover.set(Some(event.target));
                    context.emit(map(InspectorAction::SetHover(Some(event.target))));
                }
                RoutedEventKind::PointerButton {
                    button: PointerButton::Primary,
                    state: ElementState::Pressed,
                    ..
                } if picker.get() && !inspector_nodes.borrow().contains(&event.target) => {
                    context.prevent_default();
                    context.stop_propagation();
                    context.emit(map(InspectorAction::Select(event.target)));
                }
                _ => {}
            },
        )?;

        let mut inspector = Self {
            panel,
            launcher,
            highlight_overlay,
            highlight,
            info_tag,
            pick,
            tree,
            details_pad,
            details,
            crumb_bar,
            crumbs,
            owned,
            picking,
            open_state,
            hover_cell,
            open: options.initially_open,
            selected: None,
            hovered: None,
            filter: String::new(),
            expanded: HashSet::new(),
            seeded: false,
            cache: HashMap::new(),
            meta,
            viewport: LogicalSize::ZERO,
            last_details: None,
            last_crumbs: None,
            show_launcher: options.show_launcher,
            map_action,
        };
        inspector.update_visibility(ui)?;
        inspector.sync(ui)?;
        Ok(inspector)
    }

    /// Returns whether the details panel is open.
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Returns the selected retained element, if it still exists.
    pub const fn selected(&self) -> Option<ElementId> {
        self.selected
    }

    /// Applies one controlled inspector action and refreshes its presentation.
    pub fn apply(&mut self, ui: &mut Ui<Message>, action: InspectorAction) -> Result<(), UiError> {
        match action {
            InspectorAction::Toggle => self.open = !self.open,
            InspectorAction::Open => self.open = true,
            InspectorAction::Close => {
                self.open = false;
                self.set_picking(false);
            }
            InspectorAction::SetPicking(value) => {
                self.open = true;
                self.set_picking(value);
            }
            InspectorAction::TogglePicking => {
                self.open = true;
                let picking = !self.picking.get();
                self.set_picking(picking);
            }
            InspectorAction::Select(id) => {
                self.open = true;
                self.selected = Some(id);
                self.set_picking(false);
                let mut current = id;
                while let Some(parent) = self.cache.get(&current).and_then(|node| node.parent) {
                    if !self.cache.contains_key(&parent) {
                        break;
                    }
                    self.expanded.insert(parent);
                    current = parent;
                }
                self.tree.reveal(&id);
            }
            InspectorAction::SetExpanded { id, expanded } => {
                if expanded {
                    self.expanded.insert(id);
                } else {
                    self.expanded.remove(&id);
                }
            }
            InspectorAction::SetHover(id) => {
                // Hot path while picking: reposition the preview without
                // rebuilding the tree or details.
                self.hovered = id;
                self.hover_cell.set(id);
                return self.apply_hover(ui);
            }
            InspectorAction::SetFilter(text) => self.filter = text,
            InspectorAction::Refresh => {}
        }
        self.open_state.set(self.open);
        let picking = self.picking.get();
        ui.update_widget(self.pick, |button| button.sync("Pick", true, picking))?;
        self.update_visibility(ui)?;
        self.sync(ui)
    }

    /// Rebuilds the tree and selected-element details from current retained state.
    ///
    /// Hosts must call this after mutating their own UI **and after window
    /// resizes** — displayed bounds and the virtualized tree's realized rows
    /// are viewport dependent.
    pub fn sync(&mut self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        let inspection = ui.inspect()?;
        let semantics = ui.semantic_tree()?;
        self.viewport = inspection.viewport;
        let excluded = self.owned_roots(&inspection.nodes);
        let nodes = inspection
            .nodes
            .into_iter()
            .filter(|node| !excluded.contains(&node.id))
            .collect::<Vec<_>>();
        let labels = semantic_labels(&semantics);
        let model = Model::build(nodes);
        if self
            .selected
            .is_some_and(|id| !model.nodes.contains_key(&id))
        {
            self.selected = None;
        }
        if self
            .hovered
            .is_some_and(|id| !model.nodes.contains_key(&id))
        {
            self.hovered = None;
            self.hover_cell.set(None);
        }
        if !self.seeded {
            self.expanded = model.ids_up_to_depth(2);
            self.seeded = true;
        }
        *self.meta.borrow_mut() = row_meta(&model, &labels);

        let tree_data = {
            let meta = self.meta.borrow();
            tree_nodes(&model, &meta, &self.expanded, &self.filter)
        };
        self.tree.sync(ui, &tree_data, self.selected.as_ref())?;

        if self.last_crumbs != Some(self.selected) {
            self.rebuild_crumbs(ui, &model)?;
            self.last_crumbs = Some(self.selected);
        }

        let details_key = self
            .selected
            .and_then(|id| model.nodes.get(&id))
            .map(|node| (node.id, node.clone()));
        if details_key != self.last_details {
            self.rebuild_details(ui, details_key.as_ref().map(|(_, node)| node))?;
            self.last_details = details_key;
        }

        let selection = self
            .selected
            .and_then(|id| model.nodes.get(&id))
            .map(BandSet::of)
            .filter(|_| self.open);
        let hover = self
            .hovered
            .and_then(|id| model.nodes.get(&id))
            .map(clipped_bounds)
            .filter(|_| self.open);
        ui.update_widget(self.highlight, |highlight| {
            highlight.selection = selection;
            highlight.hover = hover;
        })?;
        self.cache = model.nodes;
        self.update_info_tag(ui)?;
        self.refresh_owned(ui)
    }

    /// Reapplies theme-derived colors after a host `set_theme` call.
    ///
    /// Widget-style color overrides snapshot the theme they were resolved
    /// against, so hosts switching themes at runtime should call this to keep
    /// the inspector chrome, rows, and details in the current palette.
    pub fn restyle(&mut self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        let surface = ui.theme().surface;
        ui.set_widget_style(
            self.panel,
            WidgetStyle {
                background: Some(surface),
                ..WidgetStyle::default()
            },
        )?;
        self.tree
            .set_row_content(ui, Self::row_renderer(self.meta.clone()))?;
        self.last_details = None;
        self.last_crumbs = None;
        self.sync(ui)
    }

    fn set_picking(&mut self, value: bool) {
        self.picking.set(value);
        if !value {
            self.hovered = None;
            self.hover_cell.set(None);
        }
    }

    fn row_renderer(
        meta: Rc<RefCell<HashMap<ElementId, RowMeta>>>,
    ) -> impl Fn(&mut Ui<Message>, ElementHandle<Row>, &ElementId) -> Result<(), UiError> {
        move |ui, row, id| {
            let caption = ui.theme().type_scale.caption;
            let muted = ui.theme().muted_foreground;
            let Some(row_meta) = meta.borrow().get(id).cloned() else {
                ui.add_label(row, "Element")?;
                return Ok(());
            };
            let kind = ui.add_label(row, format!("{:?}", row_meta.kind))?;
            ui.set_widget_style(
                kind,
                WidgetStyle {
                    foreground: Some(kind_color(row_meta.kind)),
                    ..WidgetStyle::default()
                },
            )?;
            if let Some(role) = row_meta.role {
                let role = ui.add_label(row, format!("{role:?}"))?;
                ui.set_widget_style(
                    role,
                    WidgetStyle {
                        foreground: Some(muted),
                        font_size: Some(caption),
                        ..WidgetStyle::default()
                    },
                )?;
            }
            if !row_meta.label.is_empty() {
                let mut text = row_meta.label;
                if text.chars().count() > 24 {
                    text = format!("{}…", text.chars().take(23).collect::<String>());
                }
                let label = ui.add_label(row, format!("\"{text}\""))?;
                ui.set_widget_style(
                    label,
                    WidgetStyle {
                        foreground: Some(label_color()),
                        font_size: Some(caption),
                        ..WidgetStyle::default()
                    },
                )?;
            }
            Ok(())
        }
    }

    fn rebuild_details(
        &mut self,
        ui: &mut Ui<Message>,
        node: Option<&ElementInspection>,
    ) -> Result<(), UiError> {
        ui.remove(self.details)?;
        self.details = ui.add_column(self.details_pad)?;
        ui.set_layout(
            self.details,
            LayoutStyle {
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        match node {
            Some(node) => {
                let meta = self.meta.borrow().get(&node.id).cloned();
                build_details(ui, self.details, node, meta.as_ref())?;
            }
            None => {
                let muted = ui.theme().muted_foreground;
                let empty = ui.add_label(self.details, "Select an element to inspect")?;
                ui.set_widget_style(
                    empty,
                    WidgetStyle {
                        foreground: Some(muted),
                        ..WidgetStyle::default()
                    },
                )?;
            }
        }
        Ok(())
    }

    fn rebuild_crumbs(&mut self, ui: &mut Ui<Message>, model: &Model) -> Result<(), UiError> {
        let caption = ui.theme().type_scale.caption;
        let muted = ui.theme().muted_foreground;
        let accent = ui.theme().accent;
        ui.remove(self.crumbs)?;
        self.crumbs = ui.add_row(self.crumb_bar)?;
        ui.set_flex(self.crumbs, 2.0, Alignment::Center)?;
        ui.set_layout(
            self.crumbs,
            LayoutStyle {
                grow: 1.0,
                height: Length::Percent(1.0),
                margin: Edges {
                    left: Length::Px(8.0),
                    right: Length::Px(8.0),
                    ..Edges::all(Length::Px(0.0))
                },
                ..LayoutStyle::default()
            },
        )?;
        let Some(selected) = self.selected else {
            return Ok(());
        };
        let mut chain = model.ancestor_chain(selected);
        if chain.len() > 4 {
            let ellipsis = ui.add_label(self.crumbs, "…")?;
            ui.set_widget_style(
                ellipsis,
                WidgetStyle {
                    foreground: Some(muted),
                    font_size: Some(caption),
                    ..WidgetStyle::default()
                },
            )?;
            chain = chain.split_off(chain.len() - 4);
        }
        let meta = self.meta.borrow();
        for (index, id) in chain.iter().copied().enumerate() {
            if index > 0 || model.ancestor_chain(selected).len() > 4 {
                let separator = ui.add_label(self.crumbs, "›")?;
                ui.set_widget_style(
                    separator,
                    WidgetStyle {
                        foreground: Some(muted),
                        font_size: Some(caption),
                        ..WidgetStyle::default()
                    },
                )?;
            }
            let name = meta
                .get(&id)
                .map_or_else(|| "Element".to_string(), |row| format!("{:?}", row.kind));
            let crumb = ui.add_label(self.crumbs, name)?;
            ui.set_widget_style(
                crumb,
                WidgetStyle {
                    foreground: Some(if id == selected { accent } else { muted }),
                    font_size: Some(caption),
                    ..WidgetStyle::default()
                },
            )?;
            if id != selected {
                let map = self.map_action.clone();
                ui.listen(crumb, None, EventFilter::Pointer, move |context, event| {
                    if matches!(
                        event.kind,
                        RoutedEventKind::PointerButton {
                            button: PointerButton::Primary,
                            state: ElementState::Pressed,
                            ..
                        }
                    ) {
                        context.emit(map(InspectorAction::Select(id)));
                    }
                })?;
            }
        }
        Ok(())
    }

    /// Lightweight hover refresh used while the pointer moves in pick mode.
    fn apply_hover(&mut self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        let hover = self
            .hovered
            .and_then(|id| self.cache.get(&id))
            .map(clipped_bounds)
            .filter(|_| self.open);
        ui.update_widget(self.highlight, |highlight| highlight.hover = hover)?;
        self.update_info_tag(ui)
    }

    fn update_info_tag(&self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        let node = self
            .hovered
            .filter(|_| self.open && self.picking.get())
            .and_then(|id| self.cache.get(&id));
        let Some(node) = node else {
            return ui.set_visibility(self.info_tag, Visibility::Hidden);
        };
        let bounds = clipped_bounds(node);
        let label = self
            .meta
            .borrow()
            .get(&node.id)
            .filter(|meta| !meta.label.is_empty())
            .map(|meta| format!(" \"{}\"", meta.label))
            .unwrap_or_default();
        ui.set_label_text(
            self.info_tag,
            format!(
                "{:?}{label} · {} × {}",
                node.kind,
                details::number(bounds.size.width),
                details::number(bounds.size.height)
            ),
        )?;
        let above = bounds.origin.y - INFO_TAG_HEIGHT - 4.0;
        let y = if above >= 2.0 {
            above
        } else {
            bounds.origin.y + bounds.size.height + 4.0
        };
        let x = bounds
            .origin
            .x
            .clamp(4.0, (self.viewport.width - 180.0).max(4.0));
        ui.set_layout(
            self.info_tag,
            LayoutStyle {
                positioning: Positioning::Absolute,
                inset: Edges {
                    left: Length::Px(x),
                    top: Length::Px(y),
                    ..Edges::default()
                },
                ..LayoutStyle::default()
            },
        )?;
        ui.set_visibility(self.info_tag, Visibility::Visible)
    }

    fn update_visibility(&self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        ui.set_visibility(
            self.panel,
            if self.open {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        )?;
        ui.set_visibility(
            self.highlight_overlay,
            if self.open {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        )?;
        ui.set_visibility(
            self.launcher,
            if !self.open && self.show_launcher {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        )
    }

    fn owned_roots(&self, nodes: &[ElementInspection]) -> HashSet<ElementId> {
        descendants_of(nodes, self.panel.id())
            .into_iter()
            .chain(descendants_of(nodes, self.launcher.id()))
            .chain(descendants_of(nodes, self.highlight_overlay.id()))
            .collect()
    }

    fn refresh_owned(&self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        let inspection = ui.inspect()?;
        let owned = self.owned_roots(&inspection.nodes);
        *self.owned.borrow_mut() = owned;
        Ok(())
    }
}

fn descendants_of(nodes: &[ElementInspection], root: ElementId) -> HashSet<ElementId> {
    let mut output = HashSet::from([root]);
    loop {
        let before = output.len();
        for node in nodes {
            if node.parent.is_some_and(|parent| output.contains(&parent)) {
                output.insert(node.id);
            }
        }
        if output.len() == before {
            return output;
        }
    }
}

fn is_toggle_shortcut(input: &astrelis_platform::KeyboardInput, modifiers: Modifiers) -> bool {
    let f12 = matches!(&input.logical_key, Key::Named(NamedKey::Other(key)) if key == "F12")
        || matches!(&input.physical_key, PhysicalKey::Code(KeyCode::Other(key)) if key == "F12");
    let mac_fallback = modifiers.super_key
        && modifiers.alt
        && matches!(&input.logical_key, Key::Character(key) if key.eq_ignore_ascii_case("i"));
    f12 || mac_fallback
}

#[cfg(test)]
mod tests {
    use astrelis_core::geometry::Size;
    use astrelis_text::FontDatabase;
    use astrelis_ui_core::{SemanticNode, SemanticRole, Theme};

    use super::*;

    #[derive(Clone)]
    #[allow(dead_code)]
    enum Message {
        Inspector(InspectorAction),
    }

    fn harness() -> (Ui<Message>, ElementHandle<astrelis_ui_core::Button>) {
        let mut ui = Ui::new(FontDatabase::default(), Theme::dark());
        ui.set_viewport(Size::new(800.0, 600.0), 1.0);
        let button = ui.add_button(ui.root(), "Application button").unwrap();
        (ui, button)
    }

    #[test]
    fn inspector_opens_selects_and_excludes_its_own_tree() {
        let (mut ui, button) = harness();
        let mut inspector =
            UiInspector::new(&mut ui, InspectorOptions::default(), Message::Inspector).unwrap();
        assert!(!inspector.is_open());
        inspector
            .apply(&mut ui, InspectorAction::Select(button.id()))
            .unwrap();
        assert!(inspector.is_open());
        assert_eq!(inspector.selected(), Some(button.id()));
        let tree = ui.semantic_tree().unwrap();
        assert!(contains(&tree, SemanticRole::Dialog, ""));
        assert!(!inspector.owned.borrow().contains(&button.id()));
        let inspection = ui.inspect().unwrap();
        let application = inspection
            .nodes
            .iter()
            .find(|node| node.id == button.id())
            .unwrap();
        let highlight = inspection
            .nodes
            .iter()
            .find(|node| node.id == inspector.highlight.id())
            .unwrap();
        assert!(highlight.paint_rank > application.paint_rank);
        let bands = ui.widget(inspector.highlight).unwrap().selection.unwrap();
        assert_eq!(bands.bounds, application.world_bounds);
    }

    #[test]
    fn filter_keeps_matches_and_ancestors() {
        let (mut ui, _button) = harness();
        let sibling = ui.add_label(ui.root(), "Unrelated sibling").unwrap();
        let mut inspector =
            UiInspector::new(&mut ui, InspectorOptions::default(), Message::Inspector).unwrap();
        inspector.apply(&mut ui, InspectorAction::Open).unwrap();
        inspector
            .apply(&mut ui, InspectorAction::SetFilter("button".into()))
            .unwrap();
        let semantics = ui.semantic_tree().unwrap();
        let mut labels = Vec::new();
        collect_labels(&semantics, &mut labels);
        assert!(
            labels.iter().any(|label| label.contains("Button")),
            "filtered tree should keep the matching button row"
        );
        drop(inspector);
        let _ = sibling;
    }

    #[test]
    fn select_expands_ancestors_and_reveals() {
        let mut ui: Ui<Message> = Ui::new(FontDatabase::default(), Theme::dark());
        ui.set_viewport(Size::new(800.0, 600.0), 1.0);
        let root = ui.root();
        let mut parent = ui.add_column(root).unwrap();
        for _ in 0..5 {
            parent = {
                let next = ui.add_column(parent).unwrap();
                next
            };
        }
        let deep = ui.add_button(parent, "Deep").unwrap();
        let mut inspector =
            UiInspector::new(&mut ui, InspectorOptions::default(), Message::Inspector).unwrap();
        inspector
            .apply(&mut ui, InspectorAction::Select(deep.id()))
            .unwrap();
        assert!(inspector.expanded.contains(&parent.id()));
        assert_eq!(inspector.selected(), Some(deep.id()));
    }

    #[test]
    fn hover_updates_highlight_without_rebuilding_the_tree() {
        let (mut ui, button) = harness();
        let mut inspector =
            UiInspector::new(&mut ui, InspectorOptions::default(), Message::Inspector).unwrap();
        inspector
            .apply(&mut ui, InspectorAction::SetPicking(true))
            .unwrap();
        let realized = inspector.tree.realized_count();
        inspector
            .apply(&mut ui, InspectorAction::SetHover(Some(button.id())))
            .unwrap();
        assert_eq!(inspector.tree.realized_count(), realized);
        let expected = ui.inspect_element(button).unwrap().world_bounds;
        assert_eq!(ui.widget(inspector.highlight).unwrap().hover, Some(expected));
    }

    #[test]
    fn expansion_round_trips_through_actions() {
        let (mut ui, _button) = harness();
        let container = ui.add_column(ui.root()).unwrap();
        ui.add_label(container, "Child").unwrap();
        let mut inspector =
            UiInspector::new(&mut ui, InspectorOptions::default(), Message::Inspector).unwrap();
        inspector.apply(&mut ui, InspectorAction::Open).unwrap();
        assert!(inspector.expanded.contains(&container.id()));
        inspector
            .apply(
                &mut ui,
                InspectorAction::SetExpanded {
                    id: container.id(),
                    expanded: false,
                },
            )
            .unwrap();
        assert!(!inspector.expanded.contains(&container.id()));
        inspector
            .apply(
                &mut ui,
                InspectorAction::SetExpanded {
                    id: container.id(),
                    expanded: true,
                },
            )
            .unwrap();
        assert!(inspector.expanded.contains(&container.id()));
    }

    fn collect_labels(node: &SemanticNode, output: &mut Vec<String>) {
        output.push(node.label.clone());
        for child in &node.children {
            collect_labels(child, output);
        }
    }

    fn contains(node: &SemanticNode, role: SemanticRole, label: &str) -> bool {
        (node.role == role && (label.is_empty() || node.label == label))
            || node
                .children
                .iter()
                .any(|child| contains(child, role, label))
    }
}
