//! Drag handle resizing the inspector panel along its content-facing edge.

use std::rc::Rc;

use astrelis_core::geometry::LogicalRect;
use astrelis_paint::{Brush, Painter};
use astrelis_platform::{CursorIcon, DeviceId, ElementState, Key, NamedKey, PointerButton};
use astrelis_ui::widget_any;
use astrelis_ui_core::{
    Edges, ElementHandle, EventContext, Insets, LayoutStyle, Length, Overlay, Positioning,
    RoutedEvent, RoutedEventKind, SemanticAction, SemanticActionKind, SemanticRole, Theme, UiError,
    Widget, WidgetContainerStyle,
};

use crate::InspectorDock;

pub(crate) const RESIZER_THICKNESS: f32 = 6.0;
/// Smallest usable docked width; matches the historical panel minimum.
pub(crate) const MIN_PANEL_WIDTH: f32 = 220.0;
/// Smallest usable docked height for the bottom dock.
pub(crate) const MIN_PANEL_HEIGHT: f32 = 160.0;
/// Largest share of the viewport a docked panel may reserve.
pub(crate) const MAX_VIEWPORT_SHARE: f32 = 0.85;
const KEYBOARD_STEP: f32 = 8.0;

/// The panel overlay's own layout for one dock side and size.
pub(crate) fn panel_layout(dock: InspectorDock, size: f32) -> LayoutStyle {
    match dock {
        InspectorDock::Bottom => LayoutStyle {
            width: Length::Percent(1.0),
            height: Length::Px(size),
            ..LayoutStyle::default()
        },
        _ => LayoutStyle {
            width: Length::Px(size),
            height: Length::Percent(1.0),
            ..LayoutStyle::default()
        },
    }
}

/// The content inset reserving the docked strip; zero when floating or closed.
pub(crate) fn dock_inset(dock: InspectorDock, size: f32, open: bool) -> Insets {
    if !open {
        return Insets::default();
    }
    match dock {
        InspectorDock::Right => Insets {
            right: size,
            ..Insets::default()
        },
        InspectorDock::Bottom => Insets {
            bottom: size,
            ..Insets::default()
        },
        InspectorDock::Left => Insets {
            left: size,
            ..Insets::default()
        },
        InspectorDock::Overlay => Insets::default(),
    }
}

/// Absolute placement of the handle strip along the content-facing edge.
pub(crate) fn resizer_layout(dock: InspectorDock) -> LayoutStyle {
    let zero = Length::Px(0.0);
    let (inset, width, height) = match dock {
        InspectorDock::Bottom => (
            Edges {
                left: zero,
                right: zero,
                top: zero,
                bottom: Length::Auto,
            },
            Length::Auto,
            Length::Px(RESIZER_THICKNESS),
        ),
        InspectorDock::Left => (
            Edges {
                left: Length::Auto,
                right: zero,
                top: zero,
                bottom: zero,
            },
            Length::Px(RESIZER_THICKNESS),
            Length::Auto,
        ),
        _ => (
            Edges {
                left: zero,
                right: Length::Auto,
                top: zero,
                bottom: zero,
            },
            Length::Px(RESIZER_THICKNESS),
            Length::Auto,
        ),
    };
    LayoutStyle {
        positioning: Positioning::Absolute,
        inset,
        width,
        height,
        ..LayoutStyle::default()
    }
}

/// Margin keeping panel content clear of the handle strip.
pub(crate) fn body_margin(dock: InspectorDock) -> Edges<Length> {
    let strip = Length::Px(RESIZER_THICKNESS);
    let zero = Length::Px(0.0);
    match dock {
        InspectorDock::Bottom => Edges {
            top: strip,
            left: zero,
            right: zero,
            bottom: zero,
        },
        InspectorDock::Left => Edges {
            right: strip,
            left: zero,
            top: zero,
            bottom: zero,
        },
        _ => Edges {
            left: strip,
            right: zero,
            top: zero,
            bottom: zero,
        },
    }
}

/// The smallest panel extent along one dock side's resize axis.
pub(crate) fn min_panel_size(dock: InspectorDock) -> f32 {
    match dock {
        InspectorDock::Bottom => MIN_PANEL_HEIGHT,
        _ => MIN_PANEL_WIDTH,
    }
}

#[derive(Clone, Copy)]
struct Drag {
    device: DeviceId,
    start: f32,
    start_size: f32,
}

/// Custom widget dragging the panel's content-facing edge.
///
/// Dragging resizes the panel and reflows content live (deferred layout and
/// content-inset requests), then commits the final size through the controlled
/// action channel on release so the inspector's retained state stays canonical.
pub(crate) struct PanelResizer<Message> {
    panel: ElementHandle<Overlay>,
    pub(crate) dock: InspectorDock,
    pub(crate) size: f32,
    pub(crate) max: f32,
    drag: Option<Drag>,
    hovered: bool,
    on_commit: Rc<dyn Fn(f32) -> Message>,
}

impl<Message> PanelResizer<Message> {
    pub(crate) fn new(
        panel: ElementHandle<Overlay>,
        dock: InspectorDock,
        size: f32,
        on_commit: Rc<dyn Fn(f32) -> Message>,
    ) -> Self {
        Self {
            panel,
            dock,
            size,
            max: f32::INFINITY,
            drag: None,
            hovered: false,
            on_commit,
        }
    }

    fn clamp(&self, size: f32) -> f32 {
        let min = min_panel_size(self.dock);
        size.clamp(min, self.max.max(min))
    }

    /// Pointer coordinate along the resize axis.
    fn axis(&self, position: astrelis_core::geometry::LogicalPoint) -> f32 {
        match self.dock {
            InspectorDock::Bottom => position.y,
            _ => position.x,
        }
    }

    /// Size delta for a pointer displacement, oriented so dragging away from
    /// the panel's docked edge grows it.
    fn delta(&self, start: f32, current: f32) -> f32 {
        match self.dock {
            InspectorDock::Left => current - start,
            _ => start - current,
        }
    }

    fn apply_live(&self, context: &mut EventContext<'_, Message>, size: f32) {
        context.set_layout(self.panel, panel_layout(self.dock, size));
        if self.dock != InspectorDock::Overlay {
            context.set_content_inset(dock_inset(self.dock, size, true));
        }
        context.request_paint();
    }
}

impl<Message: 'static> Widget<Message> for PanelResizer<Message> {
    widget_any!();

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
        Some(match self.dock {
            InspectorDock::Bottom => CursorIcon::NsResize,
            _ => CursorIcon::EwResize,
        })
    }

    fn event(&mut self, context: &mut EventContext<'_, Message>, event: &RoutedEvent) {
        match &event.kind {
            RoutedEventKind::PointerEntered { .. } => {
                self.hovered = true;
                context.request_paint();
            }
            RoutedEventKind::PointerLeft { .. } => {
                self.hovered = false;
                context.request_paint();
            }
            RoutedEventKind::PointerButton {
                device_id,
                position,
                button: PointerButton::Primary,
                state: ElementState::Pressed,
            } => {
                self.drag = Some(Drag {
                    device: *device_id,
                    start: self.axis(*position),
                    start_size: self.size,
                });
                context.capture_pointer(*device_id);
                context.request_focus();
            }
            RoutedEventKind::PointerMoved {
                device_id,
                position,
            } if self.drag.is_some_and(|drag| drag.device == *device_id) => {
                let drag = self.drag.expect("checked");
                let size =
                    self.clamp(drag.start_size + self.delta(drag.start, self.axis(*position)));
                self.size = size;
                self.apply_live(context, size);
            }
            RoutedEventKind::PointerButton {
                device_id,
                button: PointerButton::Primary,
                state: ElementState::Released,
                ..
            } if self.drag.is_some_and(|drag| drag.device == *device_id) => {
                self.drag = None;
                context.release_pointer(*device_id);
                context.emit((self.on_commit)(self.size));
            }
            RoutedEventKind::PointerCancelled { device_id }
                if self.drag.is_some_and(|drag| drag.device == *device_id) =>
            {
                let drag = self.drag.take().expect("checked");
                self.size = drag.start_size;
                self.apply_live(context, drag.start_size);
                context.release_pointer(*device_id);
            }
            RoutedEventKind::Keyboard(input) if input.state == ElementState::Pressed => {
                let grow = match (&input.logical_key, self.dock) {
                    (Key::Named(NamedKey::Other(key)), InspectorDock::Bottom)
                        if key == "ArrowUp" =>
                    {
                        KEYBOARD_STEP
                    }
                    (Key::Named(NamedKey::Other(key)), InspectorDock::Bottom)
                        if key == "ArrowDown" =>
                    {
                        -KEYBOARD_STEP
                    }
                    (Key::Named(NamedKey::Other(key)), InspectorDock::Left)
                        if key == "ArrowRight" =>
                    {
                        KEYBOARD_STEP
                    }
                    (Key::Named(NamedKey::Other(key)), InspectorDock::Left)
                        if key == "ArrowLeft" =>
                    {
                        -KEYBOARD_STEP
                    }
                    (Key::Named(NamedKey::Other(key)), _) if key == "ArrowLeft" => KEYBOARD_STEP,
                    (Key::Named(NamedKey::Other(key)), _) if key == "ArrowRight" => -KEYBOARD_STEP,
                    _ => return,
                };
                context.emit((self.on_commit)(self.clamp(self.size + grow)));
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
        // A hairline border marks the panel edge; the strip lights up in the
        // accent while hovered or dragging so the affordance is discoverable.
        let active = self.hovered || self.drag.is_some();
        let thickness = if active { 2.0 } else { 1.0 };
        let line = match self.dock {
            InspectorDock::Bottom => LogicalRect::from_xywh(
                bounds.origin.x,
                bounds.origin.y,
                bounds.size.width,
                thickness,
            ),
            InspectorDock::Left => LogicalRect::from_xywh(
                bounds.max_x() - thickness,
                bounds.origin.y,
                thickness,
                bounds.size.height,
            ),
            _ => LogicalRect::from_xywh(
                bounds.origin.x,
                bounds.origin.y,
                thickness,
                bounds.size.height,
            ),
        };
        let color = if active { theme.accent } else { theme.border };
        painter.fill_rect(line, Brush::Solid(color))?;
        Ok(())
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((
            SemanticRole::Separator,
            "Resize inspector panel".into(),
            Some(format!("{:.0}", self.size)),
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
            SemanticAction::SetValue(size) => {
                context.emit((self.on_commit)(self.clamp(*size)));
                true
            }
            _ => false,
        }
    }
}
