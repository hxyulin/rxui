use std::{any::Any, cell::Cell, rc::Rc};

use astrelis_core::geometry::{LogicalPoint, LogicalRect, LogicalSize, Size};
use astrelis_paint::{Brush, Painter, StrokeStyle};
use astrelis_platform::{CursorIcon, DeviceId, ElementState, Key, NamedKey, PointerButton};
use astrelis_ui_core::{
    Column, ControlState, DragOperations, DragOptions, DragPayload, DropOperation, Edges,
    ElementHandle, ElementId, EventFilter, EventPhase, Insets, LayoutStyle, Length, ListenerId,
    Positioning, RoutedEventKind, SemanticAction, SemanticActionKind, SemanticRole, Theme, Ui,
    UiError, Visibility, Widget, WidgetContainerStyle,
};
use astrelis_ui_widgets::{SplitAxis, SplitPane, SplitPaneOptions};

use crate::{
    DockAxis, DockError, DockLayout, DockNode, DockPlacement, DockSide, FloatingRect,
    NormalizationReport, PanelDescriptor, PanelId, PreferredPlacement,
};

/// One branch in a runtime path to a split node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitBranch {
    /// Descend through the first child.
    First,
    /// Descend through the second child.
    Second,
}

/// Typed request emitted by docking UI interactions.
#[derive(Clone, Debug, PartialEq)]
pub enum DockAction {
    /// Activate and focus a panel.
    Activate(PanelId),
    /// Close a panel if its descriptor permits it.
    Close(PanelId),
    /// Move or insert a panel at a concrete destination.
    Place {
        /// Panel being moved.
        panel: PanelId,
        /// Destination.
        placement: DockPlacement,
    },
    /// Update a split ratio using its current runtime tree path.
    SetSplitRatio {
        /// Path from the root to the split.
        path: Vec<SplitBranch>,
        /// New first-child fraction.
        ratio: f32,
    },
    /// Update one floating group's logical geometry.
    SetFloatingBounds {
        /// Any panel currently in the floating group.
        anchor: PanelId,
        /// New geometry.
        bounds: FloatingRect,
    },
    /// Raise the floating group containing a panel.
    RaiseFloating(PanelId),
}

/// Result of applying one docking action.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DockOutcome {
    /// Whether serialized layout state changed.
    pub layout_changed: bool,
    /// Whether docking chrome was structurally reconciled.
    pub structure_changed: bool,
    /// Panel activated by the operation, when applicable.
    pub active_panel: Option<PanelId>,
}

/// Configurable logical dimensions for a docking workspace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockStyle {
    /// Height of tab and floating movement chrome.
    pub tab_height: f32,
    /// Thickness of split divider hit targets.
    pub divider_size: f32,
    /// Painted thickness within split divider hit targets.
    pub divider_visual_size: f32,
    /// Fractional keyboard step for split dividers.
    pub split_keyboard_step: f32,
    /// Default logical geometry for a newly floated panel.
    pub default_float: FloatingRect,
    /// Amount of floating title chrome kept reachable.
    pub reachable_title: f32,
    /// Thickness of the interactive floating-frame border.
    pub float_border: f32,
    /// Width of the dedicated floating-group movement grip.
    pub move_grip_width: f32,
}

impl Default for DockStyle {
    fn default() -> Self {
        Self {
            tab_height: 30.0,
            divider_size: 8.0,
            divider_visual_size: 2.0,
            split_keyboard_step: 0.02,
            default_float: FloatingRect::new(40.0, 40.0, 360.0, 260.0),
            reachable_title: 48.0,
            float_border: 8.0,
            move_grip_width: 30.0,
        }
    }
}

struct RegisteredPanel {
    descriptor: PanelDescriptor,
    host: ElementHandle<Column>,
}

enum ChromeHandle<Message> {
    Column(ElementHandle<Column>),
    Float(ElementHandle<DockFloatFrame<Message>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FloatGeometryMode {
    Move,
    Resize {
        left: bool,
        right: bool,
        top: bool,
        bottom: bool,
    },
}

/// Full docking workspace surface and background floating-drop target.
pub struct DockWorkspaceSurface<Message> {
    default_float: FloatingRect,
    map_action: Rc<dyn Fn(DockAction) -> Message>,
    hovering: Rc<Cell<bool>>,
}

impl<Message> DockWorkspaceSurface<Message> {
    fn new(default_float: FloatingRect, map_action: Rc<dyn Fn(DockAction) -> Message>) -> Self {
        Self {
            default_float,
            map_action,
            hovering: Rc::new(Cell::new(false)),
        }
    }

    /// Returns whether a panel drag is over the workspace background.
    pub fn is_drag_hovered(&self) -> bool {
        self.hovering.get()
    }

    fn preview_state(&self) -> Rc<Cell<bool>> {
        self.hovering.clone()
    }
}

impl<Message: 'static> Widget<Message> for DockWorkspaceSurface<Message> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle::structural()
    }

    fn hit_testable(&self) -> bool {
        true
    }

    fn event(
        &mut self,
        context: &mut astrelis_ui_core::EventContext<'_, Message>,
        event: &astrelis_ui_core::RoutedEvent,
    ) {
        if event.target != event.current_target {
            return;
        }
        match &event.kind {
            RoutedEventKind::DragEntered {
                device_id, payload, ..
            }
            | RoutedEventKind::DragOver {
                device_id, payload, ..
            } if payload.downcast_ref::<PanelId>().is_some() => {
                self.hovering.set(true);
                context.accept_drop(*device_id, DropOperation::Move);
                context.request_paint();
            }
            RoutedEventKind::DragLeft { .. } => {
                self.hovering.set(false);
                context.request_paint();
            }
            RoutedEventKind::Dropped {
                payload, position, ..
            } => {
                self.hovering.set(false);
                if let Some(panel) = payload.downcast_ref::<PanelId>() {
                    let local = context.window_to_local(*position).unwrap_or(*position);
                    context.emit((self.map_action)(DockAction::Place {
                        panel: panel.clone(),
                        placement: DockPlacement::Floating(FloatingRect::new(
                            local.x - 24.0,
                            local.y - 12.0,
                            self.default_float.width,
                            self.default_float.height,
                        )),
                    }));
                }
                context.request_paint();
            }
            _ => {}
        }
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((SemanticRole::Group, "Docking workspace".into(), None))
    }
}

struct DockWorkspacePreview {
    visible: Rc<Cell<bool>>,
}

impl DockWorkspacePreview {
    fn new(visible: Rc<Cell<bool>>) -> Self {
        Self { visible }
    }
}

impl<Message: 'static> Widget<Message> for DockWorkspacePreview {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        if self.visible.get() {
            painter.stroke_rect(
                bounds,
                StrokeStyle {
                    width: 3.0,
                    ..Default::default()
                },
                Brush::Solid(theme.accent),
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupDropZone {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

/// One docked or floating tab group and its drop-zone policy.
pub struct DockGroup<Message> {
    anchor: PanelId,
    tab_count: usize,
    floating: bool,
    map_action: Rc<dyn Fn(DockAction) -> Message>,
    preview: Rc<Cell<Option<GroupDropZone>>>,
}

impl<Message> DockGroup<Message> {
    fn new(
        anchor: PanelId,
        tab_count: usize,
        floating: bool,
        map_action: Rc<dyn Fn(DockAction) -> Message>,
    ) -> Self {
        Self {
            anchor,
            tab_count,
            floating,
            map_action,
            preview: Rc::new(Cell::new(None)),
        }
    }

    /// Returns whether this group currently displays a drop preview.
    pub fn has_drop_preview(&self) -> bool {
        self.preview.get().is_some()
    }

    fn preview_state(&self) -> Rc<Cell<Option<GroupDropZone>>> {
        self.preview.clone()
    }

    fn zone(&self, local: LogicalPoint, size: LogicalSize) -> GroupDropZone {
        if self.floating {
            GroupDropZone::Center
        } else if local.x < size.width * 0.25 {
            GroupDropZone::Left
        } else if local.x > size.width * 0.75 {
            GroupDropZone::Right
        } else if local.y < size.height * 0.25 {
            GroupDropZone::Top
        } else if local.y > size.height * 0.75 {
            GroupDropZone::Bottom
        } else {
            GroupDropZone::Center
        }
    }

    fn placement(&self, zone: GroupDropZone) -> DockPlacement {
        match zone {
            GroupDropZone::Center => DockPlacement::Tab {
                anchor: self.anchor.clone(),
                index: self.tab_count,
            },
            GroupDropZone::Left => DockPlacement::Split {
                anchor: self.anchor.clone(),
                side: DockSide::Left,
            },
            GroupDropZone::Right => DockPlacement::Split {
                anchor: self.anchor.clone(),
                side: DockSide::Right,
            },
            GroupDropZone::Top => DockPlacement::Split {
                anchor: self.anchor.clone(),
                side: DockSide::Top,
            },
            GroupDropZone::Bottom => DockPlacement::Split {
                anchor: self.anchor.clone(),
                side: DockSide::Bottom,
            },
        }
    }
}

impl<Message: 'static> Widget<Message> for DockGroup<Message> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle::structural()
    }

    fn hit_testable(&self) -> bool {
        true
    }

    fn event(
        &mut self,
        context: &mut astrelis_ui_core::EventContext<'_, Message>,
        event: &astrelis_ui_core::RoutedEvent,
    ) {
        if event.phase == EventPhase::Capture {
            return;
        }
        match &event.kind {
            RoutedEventKind::DragEntered {
                device_id,
                position,
                payload,
                ..
            }
            | RoutedEventKind::DragOver {
                device_id,
                position,
                payload,
                ..
            } if payload.downcast_ref::<PanelId>().is_some() => {
                let local = context
                    .window_to_local(*position)
                    .unwrap_or(LogicalPoint::ZERO);
                self.preview
                    .set(Some(self.zone(local, context.bounds().size)));
                context.accept_drop(*device_id, DropOperation::Move);
                context.request_paint();
            }
            RoutedEventKind::DragLeft { .. } => {
                self.preview.set(None);
                context.request_paint();
            }
            RoutedEventKind::Dropped {
                payload, position, ..
            } => {
                let local = context
                    .window_to_local(*position)
                    .unwrap_or(LogicalPoint::ZERO);
                let zone = self.zone(local, context.bounds().size);
                self.preview.set(None);
                if let Some(panel) = payload.downcast_ref::<PanelId>()
                    && !(panel == &self.anchor && self.tab_count == 1)
                {
                    context.emit((self.map_action)(DockAction::Place {
                        panel: panel.clone(),
                        placement: self.placement(zone),
                    }));
                }
                context.request_paint();
            }
            _ => {}
        }
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((SemanticRole::Group, "Dock panel group".into(), None))
    }
}

struct DockGroupPreview {
    zone: Rc<Cell<Option<GroupDropZone>>>,
}

impl DockGroupPreview {
    fn new(zone: Rc<Cell<Option<GroupDropZone>>>) -> Self {
        Self { zone }
    }
}

impl<Message: 'static> Widget<Message> for DockGroupPreview {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        let Some(zone) = self.zone.get() else {
            return Ok(());
        };
        let preview = match zone {
            GroupDropZone::Center => LogicalRect::from_xywh(
                bounds.origin.x + bounds.size.width * 0.25,
                bounds.origin.y + bounds.size.height * 0.25,
                bounds.size.width * 0.5,
                bounds.size.height * 0.5,
            ),
            GroupDropZone::Left => LogicalRect::from_xywh(
                bounds.origin.x,
                bounds.origin.y,
                bounds.size.width * 0.35,
                bounds.size.height,
            ),
            GroupDropZone::Right => LogicalRect::from_xywh(
                bounds.origin.x + bounds.size.width * 0.65,
                bounds.origin.y,
                bounds.size.width * 0.35,
                bounds.size.height,
            ),
            GroupDropZone::Top => LogicalRect::from_xywh(
                bounds.origin.x,
                bounds.origin.y,
                bounds.size.width,
                bounds.size.height * 0.35,
            ),
            GroupDropZone::Bottom => LogicalRect::from_xywh(
                bounds.origin.x,
                bounds.origin.y + bounds.size.height * 0.65,
                bounds.size.width,
                bounds.size.height * 0.35,
            ),
        };
        painter.stroke_rect(
            preview,
            StrokeStyle {
                width: 3.0,
                ..Default::default()
            },
            Brush::Solid(theme.accent),
        )?;
        Ok(())
    }
}

/// One accessible, draggable panel tab.
pub struct DockTab<Message> {
    panel: PanelId,
    title: String,
    selected: bool,
    index: usize,
    anchor: PanelId,
    map_action: Rc<dyn Fn(DockAction) -> Message>,
    pressed: Option<DeviceId>,
    dragging: bool,
    hovered: bool,
    height: f32,
}

impl<Message> DockTab<Message> {
    fn new(
        panel: PanelId,
        title: String,
        selected: bool,
        index: usize,
        anchor: PanelId,
        height: f32,
        map_action: Rc<dyn Fn(DockAction) -> Message>,
    ) -> Self {
        Self {
            panel,
            title,
            selected,
            index,
            anchor,
            map_action,
            pressed: None,
            dragging: false,
            hovered: false,
            height,
        }
    }

    /// Returns the panel represented by this tab.
    pub const fn panel(&self) -> &PanelId {
        &self.panel
    }

    /// Returns whether this tab is selected.
    pub const fn is_selected(&self) -> bool {
        self.selected
    }

    fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }
}

impl<Message: 'static> Widget<Message> for DockTab<Message> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn mounted(
        &mut self,
        context: &mut astrelis_ui_core::MountContext<'_, Message>,
    ) -> Result<(), UiError> {
        context.add_label(self.title.clone())?;
        Ok(())
    }

    fn container_style(&self, theme: &Theme) -> WidgetContainerStyle {
        let vertical = ((self.height - theme.type_scale.body) * 0.5).max(0.0);
        WidgetContainerStyle {
            padding: Insets {
                left: theme.control_padding.left,
                top: vertical,
                right: theme.control_padding.right,
                bottom: vertical,
            },
            gap: 0.0,
        }
    }

    fn event(
        &mut self,
        context: &mut astrelis_ui_core::EventContext<'_, Message>,
        event: &astrelis_ui_core::RoutedEvent,
    ) {
        match &event.kind {
            RoutedEventKind::PointerEntered { .. } => {
                self.hovered = true;
                context.request_paint();
            }
            RoutedEventKind::PointerLeft { .. } if !self.dragging => {
                self.hovered = false;
                context.request_paint();
            }
            RoutedEventKind::PointerButton {
                device_id,
                position,
                button: PointerButton::Primary,
                state: ElementState::Pressed,
            } if event.target == event.current_target => {
                self.pressed = Some(*device_id);
                context.request_focus();
                context.begin_drag(
                    *device_id,
                    *position,
                    DragPayload::new(self.panel.clone()),
                    DragOptions {
                        allowed: DragOperations::MOVE,
                        ..Default::default()
                    },
                );
                context.prevent_default();
                context.request_paint();
            }
            RoutedEventKind::PointerButton {
                device_id,
                button: PointerButton::Primary,
                state: ElementState::Released,
                ..
            } if self.pressed == Some(*device_id) => {
                if !self.dragging {
                    context.emit((self.map_action)(DockAction::Activate(self.panel.clone())));
                }
                self.pressed = None;
                context.prevent_default();
                context.request_paint();
            }
            RoutedEventKind::DragStarted { .. } => {
                self.dragging = true;
                context.request_paint();
            }
            RoutedEventKind::DragEnded { .. } => {
                self.dragging = false;
                self.pressed = None;
                context.request_paint();
            }
            RoutedEventKind::DragEntered {
                device_id, payload, ..
            }
            | RoutedEventKind::DragOver {
                device_id, payload, ..
            } if payload.downcast_ref::<PanelId>().is_some() => {
                context.accept_drop(*device_id, DropOperation::Move);
                context.stop_propagation();
            }
            RoutedEventKind::Dropped {
                payload, position, ..
            } => {
                if let Some(panel) = payload.downcast_ref::<PanelId>()
                    && panel != &self.panel
                {
                    let after = context
                        .window_to_local(*position)
                        .is_some_and(|point| point.x >= context.bounds().size.width * 0.5);
                    context.emit((self.map_action)(DockAction::Place {
                        panel: panel.clone(),
                        placement: DockPlacement::Tab {
                            anchor: self.anchor.clone(),
                            index: self.index + usize::from(after),
                        },
                    }));
                }
                context.stop_propagation();
            }
            RoutedEventKind::Keyboard(input) if input.state == ElementState::Pressed => {
                if matches!(
                    input.logical_key,
                    Key::Named(NamedKey::Enter | NamedKey::Space)
                ) {
                    context.emit((self.map_action)(DockAction::Activate(self.panel.clone())));
                    context.prevent_default();
                }
            }
            _ => {}
        }
    }

    fn hit_testable(&self) -> bool {
        true
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor_icon(&self) -> Option<CursorIcon> {
        Some(CursorIcon::Pointer)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        let color = theme.button.resolve(ControlState {
            enabled: true,
            hovered: self.hovered || self.pressed.is_some(),
            pressed: self.selected || self.dragging,
        });
        painter.fill_rect(bounds, Brush::Solid(color))?;
        Ok(())
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((
            SemanticRole::Tab,
            self.title.clone(),
            Some(
                if self.selected {
                    "selected"
                } else {
                    "not selected"
                }
                .into(),
            ),
        ))
    }

    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![SemanticActionKind::Focus, SemanticActionKind::Activate]
    }

    fn semantic_action(
        &mut self,
        context: &mut astrelis_ui_core::EventContext<'_, Message>,
        action: &SemanticAction,
    ) -> bool {
        match action {
            SemanticAction::Focus => {
                context.request_focus();
                true
            }
            SemanticAction::Activate => {
                context.emit((self.map_action)(DockAction::Activate(self.panel.clone())));
                true
            }
            _ => false,
        }
    }
}

/// In-window floating frame with point-aware border resizing.
pub struct DockFloatFrame<Message> {
    anchor: PanelId,
    bounds: FloatingRect,
    minimum: LogicalSize,
    border: f32,
    map_action: Rc<dyn Fn(DockAction) -> Message>,
    move_grip: Option<ElementId>,
    interaction: Option<FloatGeometryMode>,
    drag: Option<(DeviceId, LogicalPoint, FloatingRect)>,
}

impl<Message> DockFloatFrame<Message> {
    fn new(
        anchor: PanelId,
        bounds: FloatingRect,
        minimum: LogicalSize,
        border: f32,
        map_action: Rc<dyn Fn(DockAction) -> Message>,
    ) -> Self {
        Self {
            anchor,
            bounds,
            minimum,
            border,
            map_action,
            move_grip: None,
            interaction: None,
            drag: None,
        }
    }

    fn set_move_grip(&mut self, grip: ElementId) {
        self.move_grip = Some(grip);
    }

    /// Returns the current logical floating bounds.
    pub const fn bounds(&self) -> FloatingRect {
        self.bounds
    }

    fn resize_mode(&self, point: LogicalPoint, size: LogicalSize) -> Option<FloatGeometryMode> {
        let left = point.x <= self.border;
        let right = point.x >= size.width - self.border;
        let top = point.y <= self.border;
        let bottom = point.y >= size.height - self.border;
        (left || right || top || bottom).then_some(FloatGeometryMode::Resize {
            left,
            right,
            top,
            bottom,
        })
    }

    fn mode_cursor(mode: FloatGeometryMode) -> CursorIcon {
        match mode {
            FloatGeometryMode::Move => CursorIcon::Move,
            FloatGeometryMode::Resize {
                left,
                right,
                top,
                bottom,
            } if (left || right) && (top || bottom) => {
                if (left && top) || (right && bottom) {
                    CursorIcon::NwseResize
                } else {
                    CursorIcon::NeswResize
                }
            }
            FloatGeometryMode::Resize { left, right, .. } if left || right => CursorIcon::EwResize,
            FloatGeometryMode::Resize { .. } => CursorIcon::NsResize,
        }
    }
}

impl<Message: 'static> Widget<Message> for DockFloatFrame<Message> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle {
            padding: Insets::all(self.border),
            gap: 0.0,
        }
    }

    fn event(
        &mut self,
        context: &mut astrelis_ui_core::EventContext<'_, Message>,
        event: &astrelis_ui_core::RoutedEvent,
    ) {
        match &event.kind {
            RoutedEventKind::PointerMoved {
                device_id,
                position,
            } if self.drag.is_some_and(|drag| drag.0 == *device_id) => {
                let (_, start, initial) = self.drag.expect("drag matched above");
                let dx = position.x - start.x;
                let dy = position.y - start.y;
                let mode = self.interaction.unwrap_or(FloatGeometryMode::Move);
                let next = resize_float(initial, mode, dx, dy, self.minimum);
                self.bounds = next;
                context.set_current_layout(floating_layout(next));
                context.emit((self.map_action)(DockAction::SetFloatingBounds {
                    anchor: self.anchor.clone(),
                    bounds: next,
                }));
                context.prevent_default();
            }
            RoutedEventKind::PointerMoved { position, .. } => {
                let local = context
                    .window_to_local(*position)
                    .unwrap_or(LogicalPoint::ZERO);
                self.interaction = self.resize_mode(local, context.bounds().size);
                context.request_paint();
            }
            RoutedEventKind::PointerButton {
                device_id,
                position,
                button: PointerButton::Primary,
                state: ElementState::Pressed,
            } => {
                let mode = if self.move_grip == Some(event.target) {
                    Some(FloatGeometryMode::Move)
                } else if event.target == event.current_target {
                    let local = context
                        .window_to_local(*position)
                        .unwrap_or(LogicalPoint::ZERO);
                    self.resize_mode(local, context.bounds().size)
                } else {
                    None
                };
                if let Some(mode) = mode {
                    self.interaction = Some(mode);
                    self.drag = Some((*device_id, *position, self.bounds));
                    context.capture_pointer(*device_id);
                    context.prevent_default();
                    context.request_paint();
                }
            }
            RoutedEventKind::PointerButton {
                device_id,
                button: PointerButton::Primary,
                state: ElementState::Released,
                ..
            }
            | RoutedEventKind::PointerCancelled { device_id }
                if self.drag.is_some_and(|drag| drag.0 == *device_id) =>
            {
                self.drag = None;
                context.release_pointer(*device_id);
                context.prevent_default();
                context.request_paint();
            }
            _ => {}
        }
    }

    fn hit_testable(&self) -> bool {
        true
    }

    fn hit_test(&self, point: LogicalPoint, bounds: LogicalRect) -> bool {
        let local = LogicalPoint::new(point.x - bounds.origin.x, point.y - bounds.origin.y);
        self.resize_mode(local, bounds.size).is_some()
    }

    fn cursor_icon(&self) -> Option<CursorIcon> {
        self.interaction.map(Self::mode_cursor)
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        painter.fill_rect(bounds, Brush::Solid(theme.field_background))?;
        painter.stroke_rect(
            bounds,
            StrokeStyle {
                width: self.border.max(1.0),
                ..Default::default()
            },
            Brush::Solid(if self.drag.is_some() {
                theme.accent
            } else {
                theme.button.hovered
            }),
        )?;
        Ok(())
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((
            SemanticRole::Group,
            format!("Floating panel group containing {}", self.anchor),
            None,
        ))
    }
}

/// Retained controller binding app-owned panel subtrees to a [`DockLayout`].
pub struct DockWorkspace<Message> {
    root: ElementHandle<DockWorkspaceSurface<Message>>,
    parking: ElementHandle<Column>,
    chrome: Vec<ChromeHandle<Message>>,
    panels: Vec<RegisteredPanel>,
    layout: DockLayout,
    default_layout: DockLayout,
    style: DockStyle,
    map_action: Rc<dyn Fn(DockAction) -> Message>,
    tab_buttons: Vec<(ElementHandle<DockTab<Message>>, PanelId)>,
    rendered_floats: Vec<(PanelId, ElementHandle<DockFloatFrame<Message>>)>,
    keyboard_listener: Option<ListenerId>,
}

impl<Message: 'static> DockWorkspace<Message> {
    /// Mounts an initially empty workspace below `parent`.
    pub fn new<T>(
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        style: DockStyle,
        map_action: impl Fn(DockAction) -> Message + 'static,
    ) -> Result<Self, UiError> {
        let mapper = Rc::new(map_action);
        let root = ui.add_widget(
            parent,
            DockWorkspaceSurface::new(style.default_float, mapper.clone()),
        )?;
        ui.set_layout(root, fill_layout())?;
        ui.set_overflow(root, astrelis_ui_core::Overflow::Clip)?;
        let preview_state = ui.widget(root)?.preview_state();
        let preview = ui.add_widget(root, DockWorkspacePreview::new(preview_state))?;
        ui.set_layout(preview, inset_fill_layout(0.0))?;
        ui.set_z_index(preview, 1_000)?;
        let parking = ui.add_column(root)?;
        ui.set_visibility(parking, Visibility::Collapsed)?;
        Ok(Self {
            root,
            parking,
            chrome: Vec::new(),
            panels: Vec::new(),
            layout: DockLayout::default(),
            default_layout: DockLayout::default(),
            style,
            map_action: mapper,
            tab_buttons: Vec::new(),
            rendered_floats: Vec::new(),
            keyboard_listener: None,
        })
    }

    /// Returns the workspace's retained root.
    pub const fn root(&self) -> ElementHandle<DockWorkspaceSurface<Message>> {
        self.root
    }

    /// Registers an app-owned subtree and reparents it into a stable panel host.
    pub fn register_panel<T>(
        &mut self,
        ui: &mut Ui<Message>,
        descriptor: PanelDescriptor,
        content: ElementHandle<T>,
    ) -> Result<ElementHandle<Column>, DockError> {
        if self
            .panels
            .iter()
            .any(|panel| panel.descriptor.id == descriptor.id)
        {
            return Err(DockError::new(format!(
                "panel {} is already registered",
                descriptor.id
            )));
        }
        if descriptor.title.trim().is_empty()
            || !descriptor.minimum_size.width.is_finite()
            || !descriptor.minimum_size.height.is_finite()
            || descriptor.minimum_size.width < 0.0
            || descriptor.minimum_size.height < 0.0
        {
            return Err(DockError::new(
                "panel descriptor has invalid title or minimum size",
            ));
        }
        let host = ui
            .add_column(self.parking)
            .and_then(|host| {
                ui.reparent(content, host)?;
                ui.set_semantic_role(host, SemanticRole::TabPanel)?;
                Ok(host)
            })
            .map_err(|error| DockError::new(error.to_string()))?;
        self.panels.push(RegisteredPanel { descriptor, host });
        Ok(host)
    }

    /// Restores, normalizes, and renders a layout after all panels are registered.
    pub fn restore(
        &mut self,
        ui: &mut Ui<Message>,
        saved: DockLayout,
        default_layout: DockLayout,
    ) -> Result<NormalizationReport, DockError> {
        self.default_layout = default_layout;
        self.layout = saved;
        let descriptors = self
            .panels
            .iter()
            .map(|panel| panel.descriptor.clone())
            .collect::<Vec<_>>();
        let report =
            self.layout
                .normalize(&descriptors, &self.default_layout, self.style.default_float);
        self.reconcile(ui)?;
        Ok(report)
    }

    /// Returns the current serializable layout state.
    pub const fn layout(&self) -> &DockLayout {
        &self.layout
    }

    /// Applies an interaction emitted by this workspace.
    pub fn apply(
        &mut self,
        ui: &mut Ui<Message>,
        action: DockAction,
    ) -> Result<DockOutcome, DockError> {
        match action {
            DockAction::Activate(panel) => {
                self.layout.activate(&panel)?;
                self.sync_active_state(ui)?;
                Ok(DockOutcome {
                    layout_changed: true,
                    structure_changed: false,
                    active_panel: Some(panel),
                })
            }
            DockAction::Close(panel) => {
                let descriptor = self.descriptor(&panel)?;
                if !descriptor.closable {
                    return Err(DockError::new(format!("panel {panel} is not closable")));
                }
                if !self.layout.remove_panel(&panel) {
                    return Ok(DockOutcome::default());
                }
                self.reconcile(ui)?;
                Ok(DockOutcome {
                    layout_changed: true,
                    structure_changed: true,
                    active_panel: None,
                })
            }
            DockAction::Place { panel, placement } => {
                self.descriptor(&panel)?;
                self.layout.place_panel(panel.clone(), placement)?;
                self.reconcile(ui)?;
                Ok(DockOutcome {
                    layout_changed: true,
                    structure_changed: true,
                    active_panel: Some(panel),
                })
            }
            DockAction::SetSplitRatio { path, ratio } => {
                let split = split_at_path_mut(self.layout.root.as_mut(), &path)
                    .ok_or_else(|| DockError::new("stale split path"))?;
                if let DockNode::Split {
                    ratio: stored_ratio,
                    ..
                } = split
                {
                    *stored_ratio = if ratio.is_finite() {
                        ratio.clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                }
                Ok(DockOutcome {
                    layout_changed: true,
                    ..Default::default()
                })
            }
            DockAction::SetFloatingBounds { anchor, bounds } => {
                let minimum = self.group_minimum(&anchor)?;
                let group = self
                    .layout
                    .floating
                    .iter_mut()
                    .find(|group| group.tabs.panels.contains(&anchor))
                    .ok_or_else(|| DockError::new("floating group no longer exists"))?;
                group.bounds.width = bounds.width.max(minimum.width);
                group.bounds.height = bounds.height.max(minimum.height + self.style.tab_height);
                group.bounds.x = bounds.x;
                group.bounds.y = bounds.y;
                Ok(DockOutcome {
                    layout_changed: true,
                    ..Default::default()
                })
            }
            DockAction::RaiseFloating(panel) => {
                self.layout.activate(&panel)?;
                self.sync_active_state(ui)?;
                Ok(DockOutcome {
                    layout_changed: true,
                    active_panel: Some(panel),
                    ..Default::default()
                })
            }
        }
    }

    /// Opens a registered hidden panel at its descriptor's preferred location.
    pub fn open(
        &mut self,
        ui: &mut Ui<Message>,
        panel: &PanelId,
    ) -> Result<DockOutcome, DockError> {
        if self.layout.contains(panel) {
            return self.apply(ui, DockAction::Activate(panel.clone()));
        }
        let preferred = self.descriptor(panel)?.preferred.clone();
        let placement = self.resolve_preferred(&preferred);
        self.apply(
            ui,
            DockAction::Place {
                panel: panel.clone(),
                placement,
            },
        )
    }

    /// Re-clamps floating groups after the workspace viewport changes.
    pub fn clamp_floating(
        &mut self,
        ui: &mut Ui<Message>,
        viewport: LogicalSize,
    ) -> Result<bool, DockError> {
        let mut changed = false;
        for index in 0..self.layout.floating.len() {
            let anchor = self.layout.floating[index].tabs.panels[0].clone();
            let minimum = self.group_minimum(&anchor)?;
            let next = self.layout.floating[index].bounds.clamp_to_viewport(
                viewport,
                Size::new(minimum.width, minimum.height + self.style.tab_height),
                self.style.reachable_title,
            );
            changed |= next != self.layout.floating[index].bounds;
            self.layout.floating[index].bounds = next;
        }
        if changed {
            self.reconcile(ui)?;
        }
        Ok(changed)
    }

    fn descriptor(&self, id: &PanelId) -> Result<&PanelDescriptor, DockError> {
        self.panels
            .iter()
            .find(|panel| panel.descriptor.id == *id)
            .map(|panel| &panel.descriptor)
            .ok_or_else(|| DockError::new(format!("panel {id} is not registered")))
    }

    fn resolve_preferred(&self, preferred: &PreferredPlacement) -> DockPlacement {
        match preferred {
            PreferredPlacement::Root => DockPlacement::Root { index: usize::MAX },
            PreferredPlacement::Tab(anchor) if self.layout.contains(anchor) => DockPlacement::Tab {
                anchor: anchor.clone(),
                index: usize::MAX,
            },
            PreferredPlacement::Split { anchor, side }
                if self
                    .layout
                    .root
                    .as_ref()
                    .is_some_and(|root| node_has_panel(root, anchor)) =>
            {
                DockPlacement::Split {
                    anchor: anchor.clone(),
                    side: *side,
                }
            }
            PreferredPlacement::Floating(bounds) => DockPlacement::Floating(*bounds),
            _ => DockPlacement::Root { index: usize::MAX },
        }
    }

    fn group_minimum(&self, anchor: &PanelId) -> Result<LogicalSize, DockError> {
        let panels = if let Some(tabs) = self
            .layout
            .root
            .as_ref()
            .and_then(|node| find_tabs(node, anchor))
        {
            &tabs.panels
        } else {
            &self
                .layout
                .floating
                .iter()
                .find(|group| group.tabs.panels.contains(anchor))
                .ok_or_else(|| DockError::new("panel group is not visible"))?
                .tabs
                .panels
        };
        Ok(panels.iter().fold(Size::new(0.0, 0.0), |minimum, id| {
            let size = self
                .descriptor(id)
                .map_or(Size::new(0.0, 0.0), |descriptor| descriptor.minimum_size);
            Size::new(
                minimum.width.max(size.width),
                minimum.height.max(size.height),
            )
        }))
    }

    fn reconcile(&mut self, ui: &mut Ui<Message>) -> Result<(), DockError> {
        if let Some(listener) = self.keyboard_listener.take() {
            ui.remove_listener(listener);
        }
        self.tab_buttons.clear();
        self.rendered_floats.clear();
        for panel in &self.panels {
            ui.reparent(panel.host, self.parking)
                .map_err(|error| DockError::new(error.to_string()))?;
        }
        for root in self.chrome.drain(..) {
            let result = match root {
                ChromeHandle::Column(handle) => ui.remove(handle),
                ChromeHandle::Float(handle) => ui.remove(handle),
            };
            result.map_err(|error| DockError::new(error.to_string()))?;
        }
        let layout = self.layout.clone();
        if let Some(root_node) = layout.root {
            let main = ui
                .add_column(self.root)
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_layout(main, inset_fill_layout(8.0))
                .map_err(|error| DockError::new(error.to_string()))?;
            self.chrome.push(ChromeHandle::Column(main));
            self.build_node(ui, main, &root_node, Vec::new())?;
        } else {
            let empty = ui
                .add_column(self.root)
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_layout(empty, inset_fill_layout(8.0))
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.add_label(empty, "Empty workspace — drag or open a panel")
                .map_err(|error| DockError::new(error.to_string()))?;
            self.chrome.push(ChromeHandle::Column(empty));
        }
        for group in layout.floating {
            let content_minimum = self.group_minimum(&group.tabs.panels[0])?;
            let minimum = Size::new(
                content_minimum.width,
                content_minimum.height + self.style.tab_height,
            );
            let surface = ui
                .add_widget(
                    self.root,
                    DockFloatFrame::new(
                        group.tabs.panels[0].clone(),
                        group.bounds,
                        minimum,
                        self.style.float_border,
                        self.map_action.clone(),
                    ),
                )
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_layout(surface, floating_layout(group.bounds))
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_z_index(surface, 100 + self.chrome.len() as i32)
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_overflow(surface, astrelis_ui_core::Overflow::Clip)
                .map_err(|error| DockError::new(error.to_string()))?;
            self.chrome.push(ChromeHandle::Float(surface));
            self.rendered_floats
                .push((group.tabs.panels[0].clone(), surface));
            self.build_tabs(ui, surface, &group.tabs, Some(surface))?;
        }
        self.install_workspace_keyboard(ui)?;
        Ok(())
    }

    fn sync_active_state(&mut self, ui: &mut Ui<Message>) -> Result<(), DockError> {
        for (tab, panel) in &self.tab_buttons {
            let selected = panel_is_active(&self.layout, panel);
            ui.update_widget(*tab, |tab| tab.set_selected(selected))
                .map_err(|error| DockError::new(error.to_string()))?;
        }
        for registered in &self.panels {
            if self.layout.contains(&registered.descriptor.id) {
                ui.set_visibility(
                    registered.host,
                    if panel_is_active(&self.layout, &registered.descriptor.id) {
                        Visibility::Visible
                    } else {
                        Visibility::Collapsed
                    },
                )
                .map_err(|error| DockError::new(error.to_string()))?;
            }
        }
        for (index, group) in self.layout.floating.iter().enumerate() {
            if let Some((_, frame)) = self
                .rendered_floats
                .iter()
                .find(|(anchor, _)| group.tabs.panels.contains(anchor))
            {
                ui.set_z_index(*frame, 100 + index as i32)
                    .map_err(|error| DockError::new(error.to_string()))?;
            }
        }
        Ok(())
    }

    fn install_workspace_keyboard(&mut self, ui: &mut Ui<Message>) -> Result<(), DockError> {
        let tabs = self.tab_buttons.clone();
        if tabs.is_empty() {
            return Ok(());
        }
        let mapper = self.map_action.clone();
        self.keyboard_listener = Some(
            ui.listen(
                self.root,
                Some(EventPhase::Capture),
                EventFilter::Keyboard,
                move |context, event| {
                    let RoutedEventKind::Keyboard(input) = &event.kind else {
                        return;
                    };
                    if input.state != ElementState::Pressed
                        || !matches!(
                            &input.logical_key,
                            Key::Named(NamedKey::Other(key)) if key == "F6"
                        )
                    {
                        return;
                    }
                    let current = tabs
                        .iter()
                        .position(|(button, _)| context.route_contains(*button));
                    let next = current.map_or(0, |index| (index + 1) % tabs.len());
                    context.request_focus_for(tabs[next].0);
                    context.emit(mapper(DockAction::Activate(tabs[next].1.clone())));
                    context.prevent_default();
                },
            )
            .map_err(|error| DockError::new(error.to_string()))?,
        );
        Ok(())
    }

    fn build_node<T>(
        &mut self,
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        node: &DockNode,
        path: Vec<SplitBranch>,
    ) -> Result<(), DockError> {
        match node {
            DockNode::Tabs(tabs) => self.build_tabs(ui, parent, tabs, None),
            DockNode::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let first_min = node_minimum(first, &self.panels, self.style.tab_height);
                let second_min = node_minimum(second, &self.panels, self.style.tab_height);
                let options = SplitPaneOptions {
                    axis: match axis {
                        DockAxis::Horizontal => SplitAxis::Horizontal,
                        DockAxis::Vertical => SplitAxis::Vertical,
                    },
                    ratio: *ratio,
                    first_min: match axis {
                        DockAxis::Horizontal => first_min.width,
                        DockAxis::Vertical => first_min.height,
                    },
                    second_min: match axis {
                        DockAxis::Horizontal => second_min.width,
                        DockAxis::Vertical => second_min.height,
                    },
                    divider_size: self.style.divider_size,
                    divider_visual_size: self.style.divider_visual_size,
                    keyboard_step: self.style.split_keyboard_step,
                };
                let mapper = self.map_action.clone();
                let callback_path = path.clone();
                let split = SplitPane::new_with_on_change(ui, parent, options, move |ratio| {
                    mapper(DockAction::SetSplitRatio {
                        path: callback_path.clone(),
                        ratio,
                    })
                })
                .map_err(|error| DockError::new(error.to_string()))?;
                split
                    .set_container_layout(ui, fill_layout())
                    .map_err(|error| DockError::new(error.to_string()))?;
                let mut first_path = path.clone();
                first_path.push(SplitBranch::First);
                self.build_node(ui, split.first(), first, first_path)?;
                let mut second_path = path;
                second_path.push(SplitBranch::Second);
                self.build_node(ui, split.second(), second, second_path)
            }
        }
    }

    fn build_tabs<T>(
        &mut self,
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        tabs: &crate::DockTabs,
        float_frame: Option<ElementHandle<DockFloatFrame<Message>>>,
    ) -> Result<(), DockError> {
        let group = ui
            .add_widget(
                parent,
                DockGroup::new(
                    tabs.panels[0].clone(),
                    tabs.panels.len(),
                    float_frame.is_some(),
                    self.map_action.clone(),
                ),
            )
            .map_err(|error| DockError::new(error.to_string()))?;
        let preview_state = ui
            .widget(group)
            .map_err(|error| DockError::new(error.to_string()))?
            .preview_state();
        ui.set_layout(group, flex_content_layout())
            .map_err(|error| DockError::new(error.to_string()))?;
        let strip = ui
            .add_row(group)
            .map_err(|error| DockError::new(error.to_string()))?;
        ui.set_semantic_role(strip, SemanticRole::TabList)
            .map_err(|error| DockError::new(error.to_string()))?;
        ui.set_layout(
            strip,
            LayoutStyle {
                height: Length::Px(self.style.tab_height),
                min_height: Length::Px(self.style.tab_height),
                shrink: 0.0,
                ..Default::default()
            },
        )
        .map_err(|error| DockError::new(error.to_string()))?;
        if let Some(frame) = float_frame {
            let grip = ui
                .add_button(strip, "::")
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_layout(
                grip,
                LayoutStyle {
                    width: Length::Px(self.style.move_grip_width),
                    min_width: Length::Px(self.style.move_grip_width),
                    shrink: 0.0,
                    ..Default::default()
                },
            )
            .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_cursor_icon(grip, Some(CursorIcon::Move))
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.update_widget(frame, |frame| frame.set_move_grip(grip.id()))
                .map_err(|error| DockError::new(error.to_string()))?;
        }
        let mut buttons = Vec::new();
        for (index, panel_id) in tabs.panels.iter().enumerate() {
            let descriptor = self.descriptor(panel_id)?;
            let title = descriptor.title.clone();
            let closable = descriptor.closable;
            let selected = tabs.active == *panel_id;
            let button = ui
                .add_widget(
                    strip,
                    DockTab::new(
                        panel_id.clone(),
                        title.clone(),
                        selected,
                        index,
                        tabs.panels[0].clone(),
                        self.style.tab_height,
                        self.map_action.clone(),
                    ),
                )
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_layout(
                button,
                LayoutStyle {
                    width: Length::Px((title.chars().count() as f32 * 9.0 + 28.0).max(88.0)),
                    height: Length::Percent(1.0),
                    shrink: 0.0,
                    ..Default::default()
                },
            )
            .map_err(|error| DockError::new(error.to_string()))?;
            if closable {
                let close = ui
                    .add_button(strip, "×")
                    .map_err(|error| DockError::new(error.to_string()))?;
                let mapper = self.map_action.clone();
                let id = panel_id.clone();
                ui.listen(close, None, EventFilter::Activate, move |context, _| {
                    context.emit(mapper(DockAction::Close(id.clone())));
                })
                .map_err(|error| DockError::new(error.to_string()))?;
            }
            buttons.push((button, panel_id.clone(), closable));
            self.tab_buttons.push((button, panel_id.clone()));
        }
        install_tab_keyboard(ui, &buttons, self.map_action.clone())?;
        let content = ui
            .add_column(group)
            .map_err(|error| DockError::new(error.to_string()))?;
        ui.set_layout(content, flex_content_layout())
            .map_err(|error| DockError::new(error.to_string()))?;
        for panel_id in &tabs.panels {
            let registered = self
                .panels
                .iter()
                .find(|panel| panel.descriptor.id == *panel_id)
                .ok_or_else(|| DockError::new(format!("panel {panel_id} is not registered")))?;
            ui.reparent(registered.host, content)
                .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_visibility(
                registered.host,
                if tabs.active == *panel_id {
                    Visibility::Visible
                } else {
                    Visibility::Collapsed
                },
            )
            .map_err(|error| DockError::new(error.to_string()))?;
            ui.set_layout(registered.host, fill_layout())
                .map_err(|error| DockError::new(error.to_string()))?;
        }
        let preview = ui
            .add_widget(group, DockGroupPreview::new(preview_state))
            .map_err(|error| DockError::new(error.to_string()))?;
        ui.set_layout(preview, inset_fill_layout(0.0))
            .map_err(|error| DockError::new(error.to_string()))?;
        ui.set_z_index(preview, 1_000)
            .map_err(|error| DockError::new(error.to_string()))?;
        Ok(())
    }
}

fn install_tab_keyboard<Message: 'static>(
    ui: &mut Ui<Message>,
    buttons: &[(ElementHandle<DockTab<Message>>, PanelId, bool)],
    mapper: Rc<dyn Fn(DockAction) -> Message>,
) -> Result<(), DockError> {
    for (index, (button, panel, closable)) in buttons.iter().cloned().enumerate() {
        let all = buttons.to_vec();
        let mapper = mapper.clone();
        ui.listen(button, None, EventFilter::Keyboard, move |context, event| {
            let RoutedEventKind::Keyboard(input) = &event.kind else {
                return;
            };
            if input.state != ElementState::Pressed {
                return;
            }
            if closable
                && matches!(&input.logical_key, Key::Character(key) if key.eq_ignore_ascii_case("w"))
                && context.modifiers().control
            {
                context.emit(mapper(DockAction::Close(panel.clone())));
                context.prevent_default();
                return;
            }
            let key = match &input.logical_key {
                Key::Named(NamedKey::Other(key)) => key.as_str(),
                _ => return,
            };
            let next = match key {
                "ArrowRight" | "ArrowDown" => (index + 1) % all.len(),
                "ArrowLeft" | "ArrowUp" => (index + all.len() - 1) % all.len(),
                "Home" => 0,
                "End" => all.len() - 1,
                _ => return,
            };
            context.request_focus_for(all[next].0);
            context.emit(mapper(DockAction::Activate(all[next].1.clone())));
            context.prevent_default();
        })
        .map_err(|error| DockError::new(error.to_string()))?;
    }
    Ok(())
}

fn fill_layout() -> LayoutStyle {
    LayoutStyle {
        width: Length::Percent(1.0),
        height: Length::Percent(1.0),
        grow: 1.0,
        ..Default::default()
    }
}

fn flex_content_layout() -> LayoutStyle {
    LayoutStyle {
        width: Length::Percent(1.0),
        grow: 1.0,
        min_height: Length::Px(0.0),
        ..Default::default()
    }
}

fn resize_float(
    initial: FloatingRect,
    mode: FloatGeometryMode,
    dx: f32,
    dy: f32,
    minimum: LogicalSize,
) -> FloatingRect {
    match mode {
        FloatGeometryMode::Move => FloatingRect::new(
            initial.x + dx,
            initial.y + dy,
            initial.width,
            initial.height,
        ),
        FloatGeometryMode::Resize {
            left,
            right,
            top,
            bottom,
        } => {
            let width_delta = if left {
                -dx
            } else if right {
                dx
            } else {
                0.0
            };
            let height_delta = if top {
                -dy
            } else if bottom {
                dy
            } else {
                0.0
            };
            let width = (initial.width + width_delta).max(minimum.width);
            let height = (initial.height + height_delta).max(minimum.height);
            FloatingRect::new(
                if left {
                    initial.x + initial.width - width
                } else {
                    initial.x
                },
                if top {
                    initial.y + initial.height - height
                } else {
                    initial.y
                },
                width,
                height,
            )
        }
    }
}

fn floating_layout(bounds: FloatingRect) -> LayoutStyle {
    LayoutStyle {
        width: Length::Px(bounds.width),
        height: Length::Px(bounds.height),
        positioning: Positioning::Absolute,
        inset: Edges {
            left: Length::Px(bounds.x),
            top: Length::Px(bounds.y),
            right: Length::Auto,
            bottom: Length::Auto,
        },
        ..Default::default()
    }
}

fn inset_fill_layout(inset: f32) -> LayoutStyle {
    LayoutStyle {
        positioning: Positioning::Absolute,
        inset: Edges::all(Length::Px(inset)),
        ..Default::default()
    }
}

fn node_has_panel(node: &DockNode, panel: &PanelId) -> bool {
    match node {
        DockNode::Tabs(tabs) => tabs.panels.contains(panel),
        DockNode::Split { first, second, .. } => {
            node_has_panel(first, panel) || node_has_panel(second, panel)
        }
    }
}

fn find_tabs<'a>(node: &'a DockNode, panel: &PanelId) -> Option<&'a crate::DockTabs> {
    match node {
        DockNode::Tabs(tabs) => tabs.panels.contains(panel).then_some(tabs),
        DockNode::Split { first, second, .. } => {
            find_tabs(first, panel).or_else(|| find_tabs(second, panel))
        }
    }
}

fn panel_is_active(layout: &DockLayout, panel: &PanelId) -> bool {
    fn active_in_node(node: &DockNode, panel: &PanelId) -> bool {
        match node {
            DockNode::Tabs(tabs) => tabs.active == *panel,
            DockNode::Split { first, second, .. } => {
                active_in_node(first, panel) || active_in_node(second, panel)
            }
        }
    }

    layout
        .root
        .as_ref()
        .is_some_and(|root| active_in_node(root, panel))
        || layout
            .floating
            .iter()
            .any(|group| group.tabs.active == *panel)
}

fn node_minimum(node: &DockNode, panels: &[RegisteredPanel], tab_height: f32) -> LogicalSize {
    match node {
        DockNode::Tabs(tabs) => tabs
            .panels
            .iter()
            .filter_map(|id| {
                panels
                    .iter()
                    .find(|panel| panel.descriptor.id == *id)
                    .map(|panel| panel.descriptor.minimum_size)
            })
            .fold(Size::new(0.0, tab_height), |minimum, size| {
                Size::new(
                    minimum.width.max(size.width),
                    minimum.height.max(size.height + tab_height),
                )
            }),
        DockNode::Split {
            axis,
            first,
            second,
            ..
        } => {
            let first = node_minimum(first, panels, tab_height);
            let second = node_minimum(second, panels, tab_height);
            match axis {
                DockAxis::Horizontal => {
                    Size::new(first.width + second.width, first.height.max(second.height))
                }
                DockAxis::Vertical => {
                    Size::new(first.width.max(second.width), first.height + second.height)
                }
            }
        }
    }
}

fn split_at_path_mut<'a>(
    root: Option<&'a mut DockNode>,
    path: &[SplitBranch],
) -> Option<&'a mut DockNode> {
    let mut node = root?;
    for branch in path {
        let DockNode::Split { first, second, .. } = node else {
            return None;
        };
        node = match branch {
            SplitBranch::First => first,
            SplitBranch::Second => second,
        };
    }
    matches!(node, DockNode::Split { .. }).then_some(node)
}

#[cfg(test)]
mod tests {
    use astrelis_text::FontDatabase;
    use astrelis_ui_core::{SemanticNode, Theme};

    use super::*;
    use crate::{DockTabs, FloatingGroup};

    fn id(value: &str) -> PanelId {
        PanelId::new(value).unwrap()
    }

    fn semantic_node<'a>(
        node: &'a SemanticNode,
        role: SemanticRole,
        label: &str,
    ) -> Option<&'a SemanticNode> {
        if node.role == role && node.label == label {
            return Some(node);
        }
        node.children
            .iter()
            .find_map(|child| semantic_node(child, role, label))
    }

    #[test]
    fn workspace_reparents_content_without_recreating_it() {
        let mut ui: Ui<DockAction> = Ui::new(FontDatabase::default(), Theme::default());
        ui.set_viewport(Size::new(900.0, 600.0), 1.0);
        let root = ui.root();
        let content_a = ui.add_column(root).unwrap();
        let retained_label = ui.add_label(content_a, "Retained state").unwrap();
        let content_b = ui.add_column(root).unwrap();
        let mut workspace =
            DockWorkspace::new(&mut ui, root, DockStyle::default(), |action| action).unwrap();
        workspace
            .register_panel(&mut ui, PanelDescriptor::new(id("a"), "A"), content_a)
            .unwrap();
        workspace
            .register_panel(&mut ui, PanelDescriptor::new(id("b"), "B"), content_b)
            .unwrap();
        let initial = DockLayout {
            root: Some(DockNode::Tabs(
                DockTabs::new(vec![id("a"), id("b")]).unwrap(),
            )),
            floating: Vec::new(),
        };
        workspace
            .restore(&mut ui, initial.clone(), initial)
            .unwrap();
        ui.display_list().unwrap();
        let semantics = ui.semantic_tree().unwrap();
        let tab = semantic_node(&semantics, SemanticRole::Tab, "A").unwrap();
        let label = semantic_node(tab, SemanticRole::Label, "A").unwrap();
        assert_eq!(
            label.bounds.origin.y + label.bounds.size.height * 0.5,
            tab.bounds.origin.y + tab.bounds.size.height * 0.5,
        );
        workspace
            .apply(
                &mut ui,
                DockAction::Place {
                    panel: id("b"),
                    placement: DockPlacement::Split {
                        anchor: id("a"),
                        side: DockSide::Right,
                    },
                },
            )
            .unwrap();
        ui.display_list().unwrap();
        assert!(ui.inspect_element(retained_label).is_ok());
        assert!(matches!(
            workspace.layout().root,
            Some(DockNode::Split { .. })
        ));
    }

    #[test]
    fn floating_bounds_enforce_group_minimum_and_clamp_to_viewport() {
        let mut ui: Ui<DockAction> = Ui::new(FontDatabase::default(), Theme::default());
        ui.set_viewport(Size::new(500.0, 300.0), 1.0);
        let root = ui.root();
        let content = ui.add_column(root).unwrap();
        let mut descriptor = PanelDescriptor::new(id("float"), "Float");
        descriptor.minimum_size = Size::new(240.0, 160.0);
        let mut workspace =
            DockWorkspace::new(&mut ui, root, DockStyle::default(), |action| action).unwrap();
        workspace
            .register_panel(&mut ui, descriptor, content)
            .unwrap();
        let initial = DockLayout {
            root: None,
            floating: vec![FloatingGroup {
                tabs: DockTabs::new(vec![id("float")]).unwrap(),
                bounds: FloatingRect::new(900.0, 900.0, 10.0, 10.0),
            }],
        };
        workspace
            .restore(&mut ui, initial.clone(), initial)
            .unwrap();
        workspace
            .apply(
                &mut ui,
                DockAction::SetFloatingBounds {
                    anchor: id("float"),
                    bounds: FloatingRect::new(900.0, 900.0, 10.0, 10.0),
                },
            )
            .unwrap();
        workspace
            .clamp_floating(&mut ui, Size::new(500.0, 300.0))
            .unwrap();
        let bounds = workspace.layout().floating[0].bounds;
        assert!(bounds.width >= 240.0);
        assert!(bounds.height >= 160.0 + DockStyle::default().tab_height);
        assert!(bounds.x < 500.0);
        assert!(bounds.y < 300.0);
    }

    #[test]
    fn floating_frame_border_hits_without_covering_content() {
        let mut ui: Ui<DockAction> = Ui::new(FontDatabase::default(), Theme::default());
        ui.set_viewport(Size::new(640.0, 480.0), 1.0);
        let root = ui.root();
        let frame = ui
            .add_widget(
                root,
                DockFloatFrame::new(
                    id("float"),
                    FloatingRect::new(100.0, 80.0, 300.0, 220.0),
                    Size::new(120.0, 100.0),
                    8.0,
                    Rc::new(|action| action),
                ),
            )
            .unwrap();
        ui.set_layout(
            frame,
            floating_layout(FloatingRect::new(100.0, 80.0, 300.0, 220.0)),
        )
        .unwrap();
        let content = ui.add_button(frame, "Panel content").unwrap();
        ui.set_layout(content, fill_layout()).unwrap();
        ui.display_list().unwrap();

        assert_eq!(
            ui.hit_test_at(LogicalPoint::new(250.0, 190.0)).unwrap(),
            Some(content.id())
        );
        assert_eq!(
            ui.hit_test_at(LogicalPoint::new(102.0, 190.0)).unwrap(),
            Some(frame.id())
        );
        let content_bounds = ui.layout_bounds(content).unwrap();
        assert!(content_bounds.origin.x >= 108.0);
        assert!(content_bounds.origin.y >= 88.0);
        assert!(content_bounds.max_x() <= 392.0);
        assert!(content_bounds.max_y() <= 292.0);
    }

    #[test]
    fn empty_group_space_targets_group_not_workspace_surface() {
        let mut ui: Ui<DockAction> = Ui::new(FontDatabase::default(), Theme::default());
        ui.set_viewport(Size::new(640.0, 480.0), 1.0);
        let root = ui.root();
        let workspace = ui
            .add_widget(
                root,
                DockWorkspaceSurface::new(
                    FloatingRect::new(40.0, 40.0, 320.0, 240.0),
                    Rc::new(|action| action),
                ),
            )
            .unwrap();
        ui.set_layout(workspace, fill_layout()).unwrap();
        let group = ui
            .add_widget(
                workspace,
                DockGroup::new(id("panel"), 1, false, Rc::new(|action| action)),
            )
            .unwrap();
        ui.set_layout(group, inset_fill_layout(8.0)).unwrap();
        ui.display_list().unwrap();

        assert_eq!(
            ui.hit_test_at(LogicalPoint::new(320.0, 240.0)).unwrap(),
            Some(group.id())
        );
        assert_eq!(
            ui.hit_test_at(LogicalPoint::new(2.0, 2.0)).unwrap(),
            Some(workspace.id())
        );
    }

    #[test]
    fn left_and_top_resize_move_origin_and_enforce_minimum() {
        let initial = FloatingRect::new(100.0, 80.0, 300.0, 220.0);
        let resized = resize_float(
            initial,
            FloatGeometryMode::Resize {
                left: true,
                right: false,
                top: true,
                bottom: false,
            },
            260.0,
            180.0,
            Size::new(180.0, 120.0),
        );
        assert_eq!(resized.width, 180.0);
        assert_eq!(resized.height, 120.0);
        assert_eq!(resized.x, 220.0);
        assert_eq!(resized.y, 180.0);
        assert_eq!(resized.x + resized.width, initial.x + initial.width);
        assert_eq!(resized.y + resized.height, initial.y + initial.height);
    }
}
