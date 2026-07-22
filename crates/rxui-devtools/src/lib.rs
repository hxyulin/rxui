//! Opt-in, custom-rendered developer tools for retained RXUI interfaces.

#![warn(missing_docs)]

mod details;
mod edit;
mod highlight;
mod model;
mod resize;

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

use astrelis_core::geometry::LogicalSize;
use astrelis_platform::{
    ElementState, Key, KeyCode, Modifiers, NamedKey, PhysicalKey, PointerButton,
};
use astrelis_ui_core::{
    Alignment, Column, Edges, ElementHandle, ElementId, ElementInspection, ElementKind,
    EventFilter, FocusScopeOptions, Insets, Label, LayoutStyle, Length, Overlay, OverlayAlignment,
    OverlayOptions, OverlaySide, Padding, Positioning, RoutedEventKind, Row, SemanticRole, Ui,
    UiError, Visibility, WidgetStyle,
};
use rxui_app::{
    DeliveryPolicy, MessageOrigin, MessageOutcome, RuntimeSnapshot, SubscriptionKind,
    SubscriptionStatus, TaskKind,
};
use rxui_widgets::{
    CommandButton, IconButton, IconView, TreeAction, TreeView, TreeViewOptions,
    foundation::{Menu, MenuItem},
    icons,
};

use crate::{
    details::build_details,
    highlight::{BandSet, Highlight, clipped_bounds},
    model::{Model, RowMeta, kind_color, label_color, row_meta, semantic_labels, tree_nodes},
    resize::{
        MAX_VIEWPORT_SHARE, PanelResizer, body_margin, dock_inset, min_panel_size, panel_layout,
        resizer_layout,
    },
};

const INSPECTOR_Z: i32 = 20_000;
const TREE_ROW_EXTENT: f32 = 24.0;
const INFO_TAG_HEIGHT: f32 = 20.0;

fn format_elapsed(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.1} s", duration.as_secs_f64())
    } else {
        format!("{} ms", duration.as_millis())
    }
}

fn format_message_origin(origin: MessageOrigin) -> String {
    match origin {
        MessageOrigin::Ui => "ui".into(),
        MessageOrigin::Posted => "posted".into(),
        MessageOrigin::Proxy => "proxy".into(),
        MessageOrigin::Timeout(timer) => format!("timeout #{}", timer.raw()),
        MessageOrigin::Interval(timer) => format!("interval #{}", timer.raw()),
        MessageOrigin::Task(task) => format!("task #{}", task.raw()),
        MessageOrigin::Subscription(subscription) => format!(
            "subscription {}#{}",
            subscription.namespace(),
            subscription.instance()
        ),
        MessageOrigin::External => "external".into(),
        _ => "runtime".into(),
    }
}

/// Where the inspector panel mounts relative to application content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorDock {
    /// Docked to the right edge; content reflows beside it.
    #[default]
    Right,
    /// Docked to the bottom edge; content reflows above it.
    Bottom,
    /// Docked to the left edge; content reflows beside it.
    Left,
    /// Floats over the right edge without reflowing content — the panel
    /// behavior of releases before 0.6.
    Overlay,
}

/// Content shown in the inspector panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorView {
    /// Retained element tree, selection, and properties.
    #[default]
    Ui,
    /// Runner-owned active tasks and subscriptions.
    Runtime,
}

/// Configuration for an in-application UI inspector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InspectorOptions {
    /// Whether the details panel starts open.
    pub initially_open: bool,
    /// Logical width of the details panel when docked left or right.
    pub panel_width: f32,
    /// Logical height of the details panel when docked to the bottom.
    pub panel_height: f32,
    /// Where the panel docks; docked sides reflow application content.
    pub dock: InspectorDock,
    /// Whether a small launcher remains visible while the panel is closed.
    pub show_launcher: bool,
    /// Whether the details pane offers live editing of the selected element's
    /// state, layout, style, and text.
    pub allow_editing: bool,
}

impl Default for InspectorOptions {
    fn default() -> Self {
        Self {
            initially_open: false,
            panel_width: 340.0,
            panel_height: 320.0,
            dock: InspectorDock::default(),
            show_launcher: true,
            allow_editing: false,
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
    /// Switch between retained-UI and runtime inspection.
    SetView(InspectorView),
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
    /// Move the panel to another dock side.
    SetDock(InspectorDock),
    /// Commit a new panel extent along the current dock's resize axis.
    SetPanelSize(f32),
    /// Replace an application element's visibility.
    SetElementVisibility {
        /// Element being edited.
        id: ElementId,
        /// Requested visibility.
        visibility: Visibility,
    },
    /// Replace an application element's enabled state.
    SetElementEnabled {
        /// Element being edited.
        id: ElementId,
        /// Requested enabled state.
        enabled: bool,
    },
    /// Replace an application element's declared layout.
    SetElementLayout {
        /// Element being edited.
        id: ElementId,
        /// Complete replacement layout.
        layout: LayoutStyle,
    },
    /// Replace a padding container's insets.
    SetElementPadding {
        /// Element being edited.
        id: ElementId,
        /// Complete replacement insets.
        padding: Insets,
    },
    /// Replace an application element's declared widget style.
    SetElementStyle {
        /// Element being edited.
        id: ElementId,
        /// Complete replacement style overrides.
        style: WidgetStyle,
    },
    /// Replace a label's, button's, or text field's text content.
    SetElementText {
        /// Element being edited.
        id: ElementId,
        /// Replacement text.
        text: String,
    },
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
    body: ElementHandle<Column>,
    ui_tab: ElementHandle<CommandButton<Message>>,
    runtime_tab: ElementHandle<CommandButton<Message>>,
    ui_content: ElementHandle<Column>,
    runtime_content: ElementHandle<Padding>,
    runtime_details: ElementHandle<Column>,
    resizer: ElementHandle<PanelResizer<Message>>,
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
    view: InspectorView,
    dock: InspectorDock,
    panel_width: f32,
    panel_height: f32,
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
    allow_editing: bool,
    runtime_snapshot: RuntimeSnapshot,
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
        let panel_width = options
            .panel_width
            .max(min_panel_size(InspectorDock::Right));
        let panel_height = options
            .panel_height
            .max(min_panel_size(InspectorDock::Bottom));
        ui.set_layout(
            panel,
            panel_layout(
                options.dock,
                match options.dock {
                    InspectorDock::Bottom => panel_height,
                    _ => panel_width,
                },
            ),
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

        // Panel content keeps clear of the absolutely-placed resize handle via
        // a margin on the docked edge; the handle itself paints the border.
        let body = ui.add_column(panel)?;
        ui.set_layout(
            body,
            LayoutStyle {
                grow: 1.0,
                margin: body_margin(options.dock),
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
        let dock_button = ui.add_button(header, "")?;
        ui.set_layout(
            dock_button,
            LayoutStyle {
                width: Length::Px(28.0),
                height: Length::Px(28.0),
                shrink: 0.0,
                ..LayoutStyle::default()
            },
        )?;
        ui.set_semantic_description(dock_button, Some("Dock side".into()))?;
        let dock_icon = ui.add_widget(dock_button, IconView::new(icons::dock(), 16.0))?;
        ui.set_layout(
            dock_icon,
            LayoutStyle {
                width: Length::Px(16.0),
                height: Length::Px(16.0),
                positioning: Positioning::Absolute,
                inset: Edges {
                    left: Length::Px(6.0),
                    top: Length::Px(6.0),
                    ..Edges::default()
                },
                ..LayoutStyle::default()
            },
        )?;
        let dock_choice = |dock: InspectorDock, label: &str| MenuItem {
            label: label.into(),
            message: (map_action)(InspectorAction::SetDock(dock)),
            enabled: true,
        };
        let dock_menu = Menu::new(
            ui,
            dock_button,
            vec![
                dock_choice(InspectorDock::Right, "Dock right"),
                dock_choice(InspectorDock::Bottom, "Dock bottom"),
                dock_choice(InspectorDock::Left, "Dock left"),
                dock_choice(InspectorDock::Overlay, "Float over content"),
            ],
        )?;
        // Menus default to a modest z-index; hoist this one above the panel
        // overlay or it would open invisibly underneath it.
        ui.set_z_index(dock_menu.popover().content(), INSPECTOR_Z + 3)?;
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

        let tabs_pad = ui.add_padding(
            body,
            Insets {
                left: spacing_sm,
                top: 0.0,
                right: spacing_sm,
                bottom: spacing_sm,
            },
        )?;
        let tabs = ui.add_row(tabs_pad)?;
        ui.set_flex(tabs, spacing_sm, Alignment::Center)?;
        let ui_tab = ui.add_widget(
            tabs,
            CommandButton::new(
                "UI",
                (map_action)(InspectorAction::SetView(InspectorView::Ui)),
            ),
        )?;
        let runtime_tab = ui.add_widget(
            tabs,
            CommandButton::new(
                "Runtime",
                (map_action)(InspectorAction::SetView(InspectorView::Runtime)),
            ),
        )?;

        let ui_content = ui.add_column(body)?;
        ui.set_layout(
            ui_content,
            LayoutStyle {
                grow: 1.0,
                basis: Length::Px(0.0),
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;

        let search_pad = ui.add_padding(
            ui_content,
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
        ui.listen(
            search,
            None,
            EventFilter::ValueChanged,
            move |context, event| {
                if let RoutedEventKind::TextChanged(text) = &event.kind {
                    context.emit(map(InspectorAction::SetFilter(text.clone())));
                }
            },
        )?;

        let map = map_action.clone();
        let mut tree = TreeView::with_options(
            ui,
            ui_content,
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

        let divider = ui.add_column(ui_content)?;
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

        let details_scroll = ui.add_scroll_view(ui_content)?;
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

        let crumb_divider = ui.add_column(ui_content)?;
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
        let crumb_bar = ui.add_row(ui_content)?;
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

        let runtime_content = ui.add_padding(
            body,
            Insets {
                left: 10.0,
                top: 6.0,
                right: 10.0,
                bottom: 10.0,
            },
        )?;
        ui.set_layout(
            runtime_content,
            LayoutStyle {
                grow: 1.0,
                basis: Length::Px(0.0),
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let runtime_scroll = ui.add_scroll_view(runtime_content)?;
        ui.set_layout(
            runtime_scroll,
            LayoutStyle {
                grow: 1.0,
                basis: Length::Px(0.0),
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let runtime_details = ui.add_column(runtime_scroll)?;
        ui.set_layout(
            runtime_details,
            LayoutStyle {
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;

        let map = map_action.clone();
        let resizer = ui.add_widget(
            panel,
            PanelResizer::new(
                panel,
                options.dock,
                match options.dock {
                    InspectorDock::Bottom => panel_height,
                    _ => panel_width,
                },
                Rc::new(move |size| map(InspectorAction::SetPanelSize(size))),
            ),
        )?;
        ui.set_layout(resizer, resizer_layout(options.dock))?;

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
            body,
            ui_tab,
            runtime_tab,
            ui_content,
            runtime_content,
            runtime_details,
            resizer,
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
            view: InspectorView::Ui,
            dock: options.dock,
            panel_width,
            panel_height,
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
            allow_editing: options.allow_editing,
            runtime_snapshot: RuntimeSnapshot::default(),
            map_action,
        };
        ui.update_widget(inspector.ui_tab, |button| button.sync("UI", true, true))?;
        ui.update_widget(inspector.runtime_tab, |button| {
            button.sync("Runtime", true, false)
        })?;
        inspector.update_visibility(ui)?;
        inspector.rebuild_runtime(ui)?;
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
            InspectorAction::SetView(view) => {
                self.open = true;
                self.view = view;
                if view == InspectorView::Runtime {
                    self.set_picking(false);
                }
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
            InspectorAction::SetDock(dock) => self.dock = dock,
            InspectorAction::SetPanelSize(size) => {
                let clamped = self.clamp_panel_size(size);
                match self.dock {
                    InspectorDock::Bottom => self.panel_height = clamped,
                    _ => self.panel_width = clamped,
                }
            }
            InspectorAction::SetElementVisibility { id, visibility } => {
                if let Some(handle) = ui.any_handle(id) {
                    ui.set_visibility(handle, visibility)?;
                }
                self.reseed_details();
            }
            InspectorAction::SetElementEnabled { id, enabled } => {
                if let Some(handle) = ui.any_handle(id) {
                    ui.set_enabled(handle, enabled)?;
                }
                self.reseed_details();
            }
            InspectorAction::SetElementLayout { id, layout } => {
                if let Some(handle) = ui.any_handle(id) {
                    ui.set_layout(handle, layout)?;
                }
                self.reseed_details();
            }
            InspectorAction::SetElementPadding { id, padding } => {
                if let Some(handle) = ui.typed_handle::<Padding>(id) {
                    ui.set_padding_insets(handle, padding)?;
                }
                self.reseed_details();
            }
            InspectorAction::SetElementStyle { id, style } => {
                if let Some(handle) = ui.any_handle(id) {
                    ui.set_widget_style(handle, style)?;
                }
                self.reseed_details();
            }
            InspectorAction::SetElementText { id, text } => {
                match self.cache.get(&id).map(|node| node.kind) {
                    Some(ElementKind::Label) => {
                        if let Some(handle) = ui.typed_handle::<Label>(id) {
                            ui.set_label_text(handle, text)?;
                        }
                    }
                    Some(ElementKind::Button) => {
                        if let Some(handle) = ui.typed_handle::<astrelis_ui_core::Button>(id) {
                            ui.set_button_text(handle, text)?;
                        }
                    }
                    Some(ElementKind::TextField) => {
                        if let Some(handle) = ui.typed_handle::<astrelis_ui_core::TextField>(id) {
                            ui.set_text(handle, text)?;
                        }
                    }
                    _ => {}
                }
                self.reseed_details();
            }
            InspectorAction::Refresh => {}
        }
        self.open_state.set(self.open);
        let picking = self.picking.get();
        ui.update_widget(self.pick, |button| button.sync("Pick", true, picking))?;
        ui.update_widget(self.ui_tab, |button| {
            button.sync("UI", true, self.view == InspectorView::Ui)
        })?;
        ui.update_widget(self.runtime_tab, |button| {
            button.sync("Runtime", true, self.view == InspectorView::Runtime)
        })?;
        self.update_visibility(ui)?;
        self.sync(ui)
    }

    /// Rebuilds the Runtime tab from one explicit runner snapshot.
    ///
    /// Call this after task or subscription lifecycle changes. Automatic
    /// streaming is intentionally deferred to runtime instrumentation.
    pub fn sync_runtime(
        &mut self,
        ui: &mut Ui<Message>,
        snapshot: &RuntimeSnapshot,
    ) -> Result<(), UiError> {
        self.runtime_snapshot = snapshot.clone();
        self.rebuild_runtime(ui)
    }

    /// Rebuilds the tree and selected-element details from current retained state.
    ///
    /// Hosts must call this after mutating their own UI **and after window
    /// resizes** — displayed bounds and the virtualized tree's realized rows
    /// are viewport dependent.
    pub fn sync(&mut self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        // Dock geometry first, so the inspection below reports post-reflow
        // bounds rather than a stale layout from before a dock change.
        self.apply_dock(ui)?;
        let inspection = ui.inspect()?;
        let semantics = ui.semantic_tree()?;
        self.viewport = inspection.viewport;
        // The highlight overlay pins to the full viewport so world-coordinate
        // bands stay aligned regardless of the content inset; the percent
        // fallback from construction covers the pre-viewport window.
        if self.viewport.width > 0.0 && self.viewport.height > 0.0 {
            ui.set_layout(
                self.highlight_overlay,
                LayoutStyle {
                    width: Length::Px(self.viewport.width),
                    height: Length::Px(self.viewport.height),
                    ..LayoutStyle::default()
                },
            )?;
        }
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

    /// Forces the next sync to rebuild the details pane, reseeding editors.
    ///
    /// Applied edits change the selected element's inspection and would
    /// rebuild anyway; a rejected or no-op edit would not, leaving stale text
    /// in the editor that produced it — this snaps such fields back.
    fn reseed_details(&mut self) {
        self.last_details = None;
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
                let editors = self.allow_editing.then(|| self.map_action.clone());
                build_details(ui, self.details, node, meta.as_ref(), editors.as_ref())?;
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

    fn rebuild_runtime(&mut self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        ui.remove(self.runtime_details)?;
        self.runtime_details = ui.add_column(self.runtime_content)?;
        ui.set_layout(
            self.runtime_details,
            LayoutStyle {
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let heading = ui.theme().type_scale.heading;
        let heading_weight = ui.theme().type_scale.heading_weight;
        let caption = ui.theme().type_scale.caption;
        let muted = ui.theme().muted_foreground;

        let messages = ui.add_label(
            self.runtime_details,
            format!(
                "Messages ({})",
                self.runtime_snapshot.message_traces().len()
            ),
        )?;
        ui.set_widget_style(
            messages,
            WidgetStyle {
                font_size: Some(heading),
                font_weight: Some(heading_weight),
                ..WidgetStyle::default()
            },
        )?;
        let queue = ui.add_label(
            self.runtime_details,
            format!(
                "{} pending · {} coalesced replacements",
                self.runtime_snapshot.pending_messages(),
                self.runtime_snapshot.coalesced_replacements()
            ),
        )?;
        ui.set_widget_style(
            queue,
            WidgetStyle {
                foreground: Some(muted),
                font_size: Some(caption),
                ..WidgetStyle::default()
            },
        )?;
        if self.runtime_snapshot.message_traces().is_empty() {
            let empty = ui.add_label(self.runtime_details, "No recorded messages")?;
            ui.set_widget_style(
                empty,
                WidgetStyle {
                    foreground: Some(muted),
                    font_size: Some(caption),
                    ..WidgetStyle::default()
                },
            )?;
        } else {
            for trace in self.runtime_snapshot.message_traces().iter().rev() {
                let identity = trace.identity();
                let status = match trace.outcome() {
                    MessageOutcome::Success => "ok",
                    MessageOutcome::Error => "error",
                };
                ui.add_label(
                    self.runtime_details,
                    format!(
                        "#{} {} · {}",
                        identity.sequence(),
                        identity.metadata().name(),
                        identity.metadata().category()
                    ),
                )?;
                let source = identity
                    .source()
                    .map_or_else(|| "application".into(), |window| format!("{window:?}"));
                let key = trace.key().map_or_else(String::new, |key| {
                    format!(" · {}#{}", key.namespace(), key.instance())
                });
                let detail = ui.add_label(
                    self.runtime_details,
                    format!(
                        "{} · {} · wait {} · update {} · emitted {} · replaced {}{} · {}",
                        format_message_origin(identity.origin()),
                        source,
                        format_elapsed(trace.queue_latency()),
                        format_elapsed(trace.update_duration()),
                        trace.emitted(),
                        trace.replacements(),
                        key,
                        status,
                    ),
                )?;
                ui.set_widget_style(
                    detail,
                    WidgetStyle {
                        foreground: Some(muted),
                        font_size: Some(caption),
                        ..WidgetStyle::default()
                    },
                )?;
            }
        }

        let tasks = ui.add_label(
            self.runtime_details,
            format!("Tasks ({})", self.runtime_snapshot.active_tasks().len()),
        )?;
        ui.set_layout(
            tasks,
            LayoutStyle {
                margin: Edges {
                    top: Length::Px(12.0),
                    ..Edges::default()
                },
                ..LayoutStyle::default()
            },
        )?;
        ui.set_widget_style(
            tasks,
            WidgetStyle {
                font_size: Some(heading),
                font_weight: Some(heading_weight),
                ..WidgetStyle::default()
            },
        )?;
        if self.runtime_snapshot.active_tasks().is_empty() {
            let empty = ui.add_label(self.runtime_details, "No active tasks")?;
            ui.set_widget_style(
                empty,
                WidgetStyle {
                    foreground: Some(muted),
                    font_size: Some(caption),
                    ..WidgetStyle::default()
                },
            )?;
        } else {
            for task in self.runtime_snapshot.active_tasks() {
                let kind = match task.kind() {
                    TaskKind::External => "external",
                    TaskKind::Blocking => "blocking",
                    _ => "task",
                };
                ui.add_label(
                    self.runtime_details,
                    format!(
                        "{} — #{} · {} · {}",
                        task.name(),
                        task.id().raw(),
                        kind,
                        format_elapsed(task.elapsed())
                    ),
                )?;
            }
        }

        let subscriptions = ui.add_label(
            self.runtime_details,
            format!(
                "Subscriptions ({})",
                self.runtime_snapshot.active_subscriptions().len()
            ),
        )?;
        ui.set_layout(
            subscriptions,
            LayoutStyle {
                margin: Edges {
                    top: Length::Px(12.0),
                    ..Edges::default()
                },
                ..LayoutStyle::default()
            },
        )?;
        ui.set_widget_style(
            subscriptions,
            WidgetStyle {
                font_size: Some(heading),
                font_weight: Some(heading_weight),
                ..WidgetStyle::default()
            },
        )?;
        if self.runtime_snapshot.active_subscriptions().is_empty() {
            let empty = ui.add_label(self.runtime_details, "No active subscriptions")?;
            ui.set_widget_style(
                empty,
                WidgetStyle {
                    foreground: Some(muted),
                    font_size: Some(caption),
                    ..WidgetStyle::default()
                },
            )?;
        } else {
            for subscription in self.runtime_snapshot.active_subscriptions() {
                let kind = match subscription.kind() {
                    SubscriptionKind::Interval => {
                        format!(
                            "every {}",
                            format_elapsed(
                                subscription
                                    .interval()
                                    .expect("interval snapshot has a period")
                            )
                        )
                    }
                    SubscriptionKind::FileWatch => "filesystem watch".into(),
                    _ => "subscription".into(),
                };
                let delivery = match subscription.delivery_policy() {
                    DeliveryPolicy::Latest => "latest",
                    DeliveryPolicy::Every => "every event",
                };
                let status = match subscription.status() {
                    SubscriptionStatus::Running => "running",
                    SubscriptionStatus::Failed => "failed",
                };
                ui.add_label(
                    self.runtime_details,
                    format!(
                        "{}#{} — {} · {} · {} · started {}×",
                        subscription.id().namespace(),
                        subscription.id().instance(),
                        kind,
                        delivery,
                        status,
                        subscription.starts()
                    ),
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

    /// The panel's extent along the current dock's resize axis.
    fn panel_size(&self) -> f32 {
        match self.dock {
            InspectorDock::Bottom => self.panel_height,
            _ => self.panel_width,
        }
    }

    fn clamp_panel_size(&self, size: f32) -> f32 {
        let min = min_panel_size(self.dock);
        let extent = match self.dock {
            InspectorDock::Bottom => self.viewport.height,
            _ => self.viewport.width,
        };
        let max = if extent > 0.0 {
            (extent * MAX_VIEWPORT_SHARE).max(min)
        } else {
            f32::INFINITY
        };
        size.clamp(min, max)
    }

    /// Applies the current dock side: overlay placement, panel and handle
    /// geometry, and the content inset reserving the docked strip.
    fn apply_dock(&self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        let size = self.panel_size();
        ui.set_overlay_options(
            self.panel,
            OverlayOptions {
                side: match self.dock {
                    InspectorDock::Bottom => OverlaySide::Below,
                    InspectorDock::Left => OverlaySide::Left,
                    _ => OverlaySide::Right,
                },
                alignment: OverlayAlignment::Start,
                z_index: INSPECTOR_Z + 1,
                focus: FocusScopeOptions {
                    restore_focus: true,
                    ..FocusScopeOptions::default()
                },
                ..OverlayOptions::default()
            },
        )?;
        ui.set_layout(self.panel, panel_layout(self.dock, size))?;
        ui.set_layout(
            self.body,
            LayoutStyle {
                grow: 1.0,
                margin: body_margin(self.dock),
                ..LayoutStyle::default()
            },
        )?;
        ui.set_layout(self.resizer, resizer_layout(self.dock))?;
        let dock = self.dock;
        let max = if self.viewport.width > 0.0 {
            self.clamp_panel_size(f32::INFINITY)
        } else {
            f32::INFINITY
        };
        ui.update_widget(self.resizer, |resizer| {
            resizer.dock = dock;
            resizer.size = size;
            resizer.max = max;
        })?;
        ui.set_content_inset(dock_inset(self.dock, size, self.open));
        Ok(())
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
            if self.open && self.view == InspectorView::Ui {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        )?;
        ui.set_visibility(
            self.ui_content,
            if self.view == InspectorView::Ui {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        )?;
        ui.set_visibility(
            self.runtime_content,
            if self.view == InspectorView::Runtime {
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
    fn runtime_view_renders_explicit_task_and_subscription_snapshots() {
        use rxui_app::{
            ActiveSubscriptionSnapshot, ActiveTaskSnapshot, InstrumentationState, MessageMetadata,
            MessageOrigin, MessageOutcome, QueuedMessage, RuntimeInstrumentationConfig,
            SubscriptionId, SubscriptionStatus, TaskId,
        };

        let (mut ui, _button) = harness();
        let mut inspector =
            UiInspector::new(&mut ui, InspectorOptions::default(), Message::Inspector).unwrap();
        let mut instrumentation =
            InstrumentationState::new(RuntimeInstrumentationConfig::default().message_history(4));
        let now = rxui_app::Instant::now();
        let trace = instrumentation.queued(
            MessageMetadata::new("RefreshPreview", "document"),
            None,
            MessageOrigin::Task(TaskId::from_raw(9)),
            now,
            1,
            None,
        );
        let (_, _, mut dispatch) = QueuedMessage::new((), None, trace).into_parts();
        instrumentation.start_message(&mut dispatch, now);
        instrumentation.finish_message(dispatch, now, MessageOutcome::Success);
        let (message_traces, replacements) = instrumentation.snapshot();
        let snapshot = RuntimeSnapshot::with_messages(
            vec![ActiveTaskSnapshot::new(
                TaskId::from_raw(9),
                "Load preview".into(),
                TaskKind::Blocking,
                Duration::from_millis(250),
            )],
            vec![ActiveSubscriptionSnapshot::new(
                SubscriptionId::singleton("preview.live"),
                SubscriptionKind::Interval,
                DeliveryPolicy::Latest,
                Some(Duration::from_millis(120)),
                Duration::from_secs(2),
                1,
                SubscriptionStatus::Running,
            )],
            message_traces,
            0,
            replacements,
        );
        inspector.sync_runtime(&mut ui, &snapshot).unwrap();
        inspector
            .apply(&mut ui, InspectorAction::SetView(InspectorView::Runtime))
            .unwrap();
        let semantics = ui.semantic_tree().unwrap();
        let mut labels = Vec::new();
        collect_labels(&semantics, &mut labels);
        assert!(labels.iter().any(|label| label.contains("Load preview")));
        assert!(labels.iter().any(|label| label.contains("preview.live#0")));
        assert!(labels.iter().any(|label| label.contains("RefreshPreview")));
        assert!(labels.iter().any(|label| label.contains("task #9")));
    }

    #[test]
    fn select_expands_ancestors_and_reveals() {
        let mut ui: Ui<Message> = Ui::new(FontDatabase::default(), Theme::dark());
        ui.set_viewport(Size::new(800.0, 600.0), 1.0);
        let root = ui.root();
        let mut parent = ui.add_column(root).unwrap();
        for _ in 0..5 {
            parent = ui.add_column(parent).unwrap();
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
        assert_eq!(
            ui.widget(inspector.highlight).unwrap().hover,
            Some(expected)
        );
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

    #[test]
    fn docked_panel_reserves_a_content_inset_only_while_open() {
        let (mut ui, _button) = harness();
        let mut inspector =
            UiInspector::new(&mut ui, InspectorOptions::default(), Message::Inspector).unwrap();
        assert_eq!(ui.content_inset(), Insets::default());
        inspector.apply(&mut ui, InspectorAction::Open).unwrap();
        assert_eq!(
            ui.content_inset(),
            Insets {
                right: 340.0,
                ..Insets::default()
            }
        );
        let panel = ui.inspect_element(inspector.panel).unwrap();
        assert_eq!(panel.layout_bounds.origin.x, 460.0);
        assert_eq!(panel.layout_bounds.size.width, 340.0);
        inspector.apply(&mut ui, InspectorAction::Close).unwrap();
        assert_eq!(ui.content_inset(), Insets::default());
    }

    #[test]
    fn dock_sides_relayout_the_panel_and_inset() {
        let (mut ui, _button) = harness();
        let mut inspector = UiInspector::new(
            &mut ui,
            InspectorOptions {
                initially_open: true,
                ..InspectorOptions::default()
            },
            Message::Inspector,
        )
        .unwrap();
        inspector
            .apply(&mut ui, InspectorAction::SetDock(InspectorDock::Bottom))
            .unwrap();
        assert_eq!(
            ui.content_inset(),
            Insets {
                bottom: 320.0,
                ..Insets::default()
            }
        );
        let panel = ui.inspect_element(inspector.panel).unwrap();
        assert_eq!(panel.layout_bounds.origin.y, 280.0);
        assert_eq!(panel.layout_bounds.size.height, 320.0);
        assert_eq!(panel.layout_bounds.size.width, 800.0);
        inspector
            .apply(&mut ui, InspectorAction::SetDock(InspectorDock::Left))
            .unwrap();
        assert_eq!(
            ui.content_inset(),
            Insets {
                left: 340.0,
                ..Insets::default()
            }
        );
        let panel = ui.inspect_element(inspector.panel).unwrap();
        assert_eq!(panel.layout_bounds.origin.x, 0.0);
        inspector
            .apply(&mut ui, InspectorAction::SetDock(InspectorDock::Overlay))
            .unwrap();
        assert_eq!(
            ui.content_inset(),
            Insets::default(),
            "floating over content must not reflow the application"
        );
        let panel = ui.inspect_element(inspector.panel).unwrap();
        assert_eq!(panel.layout_bounds.origin.x, 460.0);
    }

    #[test]
    fn panel_size_commits_clamp_to_the_viewport_share() {
        let (mut ui, _button) = harness();
        let mut inspector = UiInspector::new(
            &mut ui,
            InspectorOptions {
                initially_open: true,
                ..InspectorOptions::default()
            },
            Message::Inspector,
        )
        .unwrap();
        inspector
            .apply(&mut ui, InspectorAction::SetPanelSize(100.0))
            .unwrap();
        assert_eq!(
            ui.content_inset().right,
            220.0,
            "sizes clamp up to the minimum panel width"
        );
        inspector
            .apply(&mut ui, InspectorAction::SetPanelSize(4000.0))
            .unwrap();
        assert_eq!(
            ui.content_inset().right,
            800.0 * 0.85,
            "sizes clamp down to the viewport share"
        );
        inspector
            .apply(&mut ui, InspectorAction::SetPanelSize(400.0))
            .unwrap();
        assert_eq!(ui.content_inset().right, 400.0);
        let panel = ui.inspect_element(inspector.panel).unwrap();
        assert_eq!(panel.layout_bounds.size.width, 400.0);
    }

    #[test]
    fn panel_popups_paint_above_the_panel_overlay() {
        let (mut ui, button) = harness();
        let mut inspector = UiInspector::new(
            &mut ui,
            InspectorOptions {
                initially_open: true,
                allow_editing: true,
                ..InspectorOptions::default()
            },
            Message::Inspector,
        )
        .unwrap();
        inspector
            .apply(&mut ui, InspectorAction::Select(button.id()))
            .unwrap();
        let inspection = ui.inspect().unwrap();
        let panel_z = inspection
            .nodes
            .iter()
            .find(|node| node.id == inspector.panel.id())
            .unwrap()
            .z_index;
        let raised_popups = inspection
            .nodes
            .iter()
            .filter(|node| node.kind == ElementKind::Overlay && node.z_index > panel_z)
            .count();
        assert!(
            raised_popups >= 2,
            "the dock menu and the visibility dropdown must paint above the \
             panel, found {raised_popups} raised popup overlays"
        );
    }

    #[test]
    fn element_edits_apply_to_the_host_tree() {
        let (mut ui, button) = harness();
        let padding = ui.add_padding(ui.root(), Insets::all(8.0)).unwrap();
        ui.add_label(padding, "Padded").unwrap();
        let mut inspector = UiInspector::new(
            &mut ui,
            InspectorOptions {
                initially_open: true,
                allow_editing: true,
                ..InspectorOptions::default()
            },
            Message::Inspector,
        )
        .unwrap();
        inspector
            .apply(
                &mut ui,
                InspectorAction::SetElementLayout {
                    id: button.id(),
                    layout: LayoutStyle {
                        width: Length::Px(240.0),
                        ..LayoutStyle::default()
                    },
                },
            )
            .unwrap();
        assert_eq!(
            ui.inspect_element(button).unwrap().declared_layout.width,
            Length::Px(240.0)
        );
        inspector
            .apply(
                &mut ui,
                InspectorAction::SetElementText {
                    id: button.id(),
                    text: "Renamed".into(),
                },
            )
            .unwrap();
        let semantics = ui.semantic_tree().unwrap();
        assert!(contains(&semantics, SemanticRole::Button, "Renamed"));
        inspector
            .apply(
                &mut ui,
                InspectorAction::SetElementEnabled {
                    id: button.id(),
                    enabled: false,
                },
            )
            .unwrap();
        assert!(!ui.inspect_element(button).unwrap().enabled);
        inspector
            .apply(
                &mut ui,
                InspectorAction::SetElementPadding {
                    id: padding.id(),
                    padding: Insets::all(20.0),
                },
            )
            .unwrap();
        assert_eq!(
            ui.inspect_element(padding).unwrap().resolved_padding,
            Insets::all(20.0)
        );
        // Collapsing removes the element from layout and from semantics.
        inspector
            .apply(
                &mut ui,
                InspectorAction::SetElementVisibility {
                    id: button.id(),
                    visibility: Visibility::Collapsed,
                },
            )
            .unwrap();
        assert_eq!(
            ui.inspect_element(button).unwrap().visibility,
            Visibility::Collapsed
        );
        let semantics = ui.semantic_tree().unwrap();
        assert!(!contains(&semantics, SemanticRole::Button, "Renamed"));
    }

    #[test]
    fn edits_to_removed_elements_are_ignored() {
        let (mut ui, button) = harness();
        let mut inspector = UiInspector::new(
            &mut ui,
            InspectorOptions {
                allow_editing: true,
                ..InspectorOptions::default()
            },
            Message::Inspector,
        )
        .unwrap();
        ui.remove(button).unwrap();
        inspector
            .apply(
                &mut ui,
                InspectorAction::SetElementEnabled {
                    id: button.id(),
                    enabled: false,
                },
            )
            .unwrap();
    }

    #[test]
    fn editors_only_render_when_editing_is_allowed() {
        let count_text_fields = |ui: &mut Ui<Message>| {
            ui.inspect()
                .unwrap()
                .nodes
                .iter()
                .filter(|node| node.kind == ElementKind::TextField)
                .count()
        };
        let (mut ui, button) = harness();
        let mut inspector =
            UiInspector::new(&mut ui, InspectorOptions::default(), Message::Inspector).unwrap();
        inspector
            .apply(&mut ui, InspectorAction::Select(button.id()))
            .unwrap();
        // The search field is the only text field in the read-only inspector.
        assert_eq!(count_text_fields(&mut ui), 1);
        drop(inspector);

        let (mut ui, button) = harness();
        let mut inspector = UiInspector::new(
            &mut ui,
            InspectorOptions {
                allow_editing: true,
                ..InspectorOptions::default()
            },
            Message::Inspector,
        )
        .unwrap();
        inspector
            .apply(&mut ui, InspectorAction::Select(button.id()))
            .unwrap();
        assert!(
            count_text_fields(&mut ui) > 1,
            "editable details should add editor text fields"
        );
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
