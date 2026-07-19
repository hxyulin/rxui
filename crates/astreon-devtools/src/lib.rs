//! Opt-in, custom-rendered developer tools for retained Astreon interfaces.

#![warn(missing_docs)]

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use astrelis_core::{
    color::Color,
    geometry::{LogicalRect, Point, Size},
};
use astrelis_paint::{Brush, Painter, StrokeStyle};
use astrelis_platform::{
    ElementState, Key, KeyCode, Modifiers, NamedKey, PhysicalKey, PointerButton,
};
use astrelis_ui::widget_any;
use astrelis_ui_core::{
    Alignment, Column, ElementHandle, ElementId, ElementInspection, EventFilter, FocusScopeOptions,
    LayoutStyle, Length, Overlay, OverlayAlignment, OverlayOptions, OverlaySide, RoutedEventKind,
    SemanticNode, SemanticRole, Theme, Ui, UiError, Visibility, Widget, WidgetContainerStyle,
    WidgetStyle,
};

const INSPECTOR_Z: i32 = 20_000;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorAction {
    /// Toggle panel visibility.
    Toggle,
    /// Open the details panel.
    Open,
    /// Close the details panel.
    Close,
    /// Start or stop selecting an application element with the pointer.
    SetPicking(bool),
    /// Select one retained element.
    Select(ElementId),
}

#[derive(Default)]
struct Highlight {
    bounds: Option<LogicalRect>,
}

impl<Message: 'static> Widget<Message> for Highlight {
    widget_any!();

    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle::structural()
    }

    fn paint(
        &self,
        painter: &mut Painter,
        _bounds: LogicalRect,
        _theme: &Theme,
    ) -> Result<(), UiError> {
        if let Some(bounds) = self.bounds {
            painter.stroke_rect(
                bounds,
                StrokeStyle {
                    width: 2.0,
                    ..StrokeStyle::default()
                },
                Brush::Solid(Color::from_hex(0x4c8dff)),
            )?;
        }
        Ok(())
    }
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
    content: ElementHandle<Column>,
    rendered: Vec<ElementHandle<Column>>,
    owned: Rc<RefCell<HashSet<ElementId>>>,
    picking: Rc<Cell<bool>>,
    open_state: Rc<Cell<bool>>,
    open: bool,
    selected: Option<ElementId>,
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
        let root = ui.root();

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
                background: Some(Color::from_hex(0x17181d)),
                ..WidgetStyle::default()
            },
        )?;
        ui.set_semantic_role(panel, SemanticRole::Dialog)?;
        ui.set_semantic_description(panel, Some("Retained UI inspector".into()))?;

        let header = ui.add_row(panel)?;
        ui.set_flex(header, 8.0, Alignment::Center)?;
        let title = ui.add_label(header, "UI Inspector")?;
        ui.set_layout(
            title,
            LayoutStyle {
                grow: 1.0,
                ..LayoutStyle::default()
            },
        )?;
        let pick = ui.add_button(header, "Pick")?;
        let map = map_action.clone();
        let picker = picking.clone();
        ui.listen(pick, None, EventFilter::Activate, move |context, _| {
            context.emit(map(InspectorAction::SetPicking(!picker.get())));
        })?;
        let close = ui.add_button(header, "Close")?;
        let map = map_action.clone();
        ui.listen(close, None, EventFilter::Activate, move |context, _| {
            context.emit(map(InspectorAction::Close));
        })?;
        let scroll = ui.add_scroll_view(panel)?;
        ui.set_layout(
            scroll,
            LayoutStyle {
                grow: 1.0,
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;
        let content = ui.add_column(scroll)?;
        ui.set_layout(
            content,
            LayoutStyle {
                width: Length::Percent(1.0),
                ..LayoutStyle::default()
            },
        )?;

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
            content,
            rendered: Vec::new(),
            owned,
            picking,
            open_state,
            open: options.initially_open,
            selected: None,
            show_launcher: options.show_launcher,
            map_action,
        };
        inspector.refresh_owned(ui)?;
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
                self.picking.set(false);
            }
            InspectorAction::SetPicking(value) => {
                self.open = true;
                self.picking.set(value);
            }
            InspectorAction::Select(id) => {
                self.open = true;
                self.selected = Some(id);
                self.picking.set(false);
            }
        }
        self.open_state.set(self.open);
        self.update_visibility(ui)?;
        self.sync(ui)
    }

    /// Rebuilds the tree and selected-element details from current retained state.
    pub fn sync(&mut self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        let inspection = ui.inspect()?;
        let semantics = ui.semantic_tree()?;
        let excluded = descendants_of(&inspection.nodes, self.panel.id())
            .into_iter()
            .chain(descendants_of(&inspection.nodes, self.launcher.id()))
            .chain(descendants_of(
                &inspection.nodes,
                self.highlight_overlay.id(),
            ))
            .collect::<HashSet<_>>();
        let nodes = inspection
            .nodes
            .into_iter()
            .filter(|node| !excluded.contains(&node.id))
            .collect::<Vec<_>>();
        let semantic_labels = semantic_labels(&semantics);
        if self
            .selected
            .is_some_and(|id| !nodes.iter().any(|node| node.id == id))
        {
            self.selected = None;
        }

        for row in self.rendered.drain(..) {
            ui.remove(row)?;
        }
        let by_id = nodes
            .iter()
            .map(|node| (node.id, node))
            .collect::<HashMap<_, _>>();
        for node in &nodes {
            let row = ui.add_column(self.content)?;
            let depth = depth_of(node, &by_id);
            let semantic = semantic_labels.get(&node.id);
            let suffix = semantic
                .filter(|(_, label)| !label.is_empty())
                .map(|(role, label)| format!(" {role:?} \"{label}\""))
                .unwrap_or_default();
            let button = ui.add_button(
                row,
                format!("{}{:?}{suffix}", "  ".repeat(depth), node.kind),
            )?;
            ui.set_layout(
                button,
                LayoutStyle {
                    width: Length::Percent(1.0),
                    ..LayoutStyle::default()
                },
            )?;
            let id = node.id;
            let map = self.map_action.clone();
            ui.listen(button, None, EventFilter::Activate, move |context, _| {
                context.emit(map(InspectorAction::Select(id)));
            })?;
            self.rendered.push(row);
        }

        let details = ui.add_column(self.content)?;
        if let Some(selected) = self.selected.and_then(|id| by_id.get(&id).copied()) {
            for line in detail_lines(selected, semantic_labels.get(&selected.id)) {
                ui.add_label(details, line)?;
            }
            let highlight_bounds = selected.clip.map_or(selected.world_bounds, |clip| {
                intersect(selected.world_bounds, clip).unwrap_or(selected.world_bounds)
            });
            ui.update_widget(self.highlight, |highlight| {
                highlight.bounds = self.open.then_some(highlight_bounds);
            })?;
        } else {
            ui.add_label(details, "Select an element to inspect")?;
            ui.update_widget(self.highlight, |highlight| highlight.bounds = None)?;
        }
        self.rendered.push(details);
        self.refresh_owned(ui)
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

    fn refresh_owned(&self, ui: &mut Ui<Message>) -> Result<(), UiError> {
        let inspection = ui.inspect()?;
        let mut owned = self.owned.borrow_mut();
        owned.clear();
        owned.extend(descendants_of(&inspection.nodes, self.panel.id()));
        owned.extend(descendants_of(&inspection.nodes, self.launcher.id()));
        owned.extend(descendants_of(
            &inspection.nodes,
            self.highlight_overlay.id(),
        ));
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

fn semantic_labels(root: &SemanticNode) -> HashMap<ElementId, (SemanticRole, String)> {
    fn visit(node: &SemanticNode, output: &mut HashMap<ElementId, (SemanticRole, String)>) {
        output.insert(node.id, (node.role, node.label.clone()));
        for child in &node.children {
            visit(child, output);
        }
    }
    let mut output = HashMap::new();
    visit(root, &mut output);
    output
}

fn depth_of(node: &ElementInspection, nodes: &HashMap<ElementId, &ElementInspection>) -> usize {
    let mut depth = 0;
    let mut parent = node.parent;
    while let Some(id) = parent {
        depth += 1;
        parent = nodes.get(&id).and_then(|node| node.parent);
    }
    depth
}

fn detail_lines(
    node: &ElementInspection,
    semantic: Option<&(SemanticRole, String)>,
) -> Vec<String> {
    let mut lines = vec![
        format!("Kind: {:?}", node.kind),
        format!("Layout: {:?}", node.declared_layout),
        format!("Layout bounds: {:?}", node.layout_bounds),
        format!("World bounds: {:?}", node.world_bounds),
        format!("Physical bounds: {:?}", node.physical_bounds),
        format!("Clip: {:?}", node.clip),
        format!("Transform: {:?}", node.world_transform),
        format!("Paint: z={} rank={}", node.z_index, node.paint_rank),
        format!(
            "State: enabled={} visible={} interactive={} focused={} hovered={}",
            node.enabled, node.effectively_visible, node.interactive, node.focused, node.hovered
        ),
        format!(
            "Hit testable: {} focusable: {}",
            node.hit_testable, node.focusable
        ),
    ];
    if let Some((role, label)) = semantic {
        lines.push(format!("Semantics: {role:?} \"{label}\""));
    }
    lines
}

fn intersect(left: LogicalRect, right: LogicalRect) -> Option<LogicalRect> {
    let x = left.origin.x.max(right.origin.x);
    let y = left.origin.y.max(right.origin.y);
    let max_x = (left.origin.x + left.size.width).min(right.origin.x + right.size.width);
    let max_y = (left.origin.y + left.size.height).min(right.origin.y + right.size.height);
    (max_x >= x && max_y >= y)
        .then(|| LogicalRect::new(Point::new(x, y), Size::new(max_x - x, max_y - y)))
}

#[cfg(test)]
mod tests {
    use astrelis_text::FontDatabase;
    use astrelis_ui_core::{SemanticRole, Theme};

    use super::*;

    #[derive(Clone)]
    #[allow(dead_code)]
    enum Message {
        Inspector(InspectorAction),
    }

    #[test]
    fn inspector_opens_selects_and_excludes_its_own_tree() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::dark());
        ui.set_viewport(Size::new(800.0, 600.0), 1.0);
        let button = ui.add_button(ui.root(), "Application button").unwrap();
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
        assert_eq!(
            ui.widget(inspector.highlight).unwrap().bounds,
            Some(application.world_bounds)
        );
    }

    fn contains(node: &SemanticNode, role: SemanticRole, label: &str) -> bool {
        (node.role == role && (label.is_empty() || node.label == label))
            || node
                .children
                .iter()
                .any(|child| contains(child, role, label))
    }
}
