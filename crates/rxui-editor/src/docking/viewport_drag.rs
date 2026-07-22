use astrelis_core::geometry::LogicalPoint;
use astrelis_platform::{DeviceId, ElementState, PointerButton, WindowEvent, WindowId};
use astrelis_ui_core::{DragOperations, DragPayload, DragSessionId, DropOperation, Ui, UiError};

use super::PanelId;

/// Result of routing one platform event through a cross-window dock drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockViewportDragEvent {
    /// The drag remains active and no external drop was dispatched.
    Active,
    /// A destination accepted the drop and emitted its ordinary dock action.
    DropDispatched(DropOperation),
    /// The pointer was released without an accepting external destination.
    Ended,
}

/// Active dock-tab drag that can be routed through multiple native windows.
///
/// # Work in progress
///
/// This coordinator accepts either destination-window events or events
/// translated from captured source-window coordinates. The latter requires
/// the application shell to query client-area desktop positions; it remains
/// unavailable on backends such as Wayland that do not expose them.
///
/// Construct this from `DockAction::BeginViewportDrag`, retain it in the
/// application shell, and pass raw window events to
/// [`handle_window_event`](Self::handle_window_event).
/// Destination workspaces receive their normal `DockAction::Place` messages;
/// panel ownership and native window lifetime remain controlled by the shell's
/// [`super::MultiViewportDockLayout`].
#[derive(Clone, Debug)]
pub struct DockViewportDrag {
    session: DragSessionId,
    device_id: DeviceId,
    panel: PanelId,
    source: WindowId,
    target: Option<(WindowId, LogicalPoint)>,
}

impl DockViewportDrag {
    /// Starts coordination for one active tab drag.
    pub fn new(
        source: WindowId,
        session: DragSessionId,
        device_id: DeviceId,
        panel: PanelId,
    ) -> Self {
        Self {
            session,
            device_id,
            panel,
            source,
            target: None,
        }
    }

    /// Returns the source native window.
    pub const fn source(&self) -> WindowId {
        self.source
    }

    /// Returns the panel being moved.
    pub const fn panel(&self) -> &PanelId {
        &self.panel
    }

    /// Returns the retained drag identity.
    pub const fn session(&self) -> DragSessionId {
        self.session
    }

    /// Returns the pointer device that owns the drag.
    pub const fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Returns the native destination currently receiving the drag.
    pub const fn target(&self) -> Option<WindowId> {
        match self.target {
            Some((window, _)) => Some(window),
            None => None,
        }
    }

    /// Routes one raw event for `window` into external drag events when needed.
    pub fn handle_window_event<Message: 'static>(
        &mut self,
        ui: &mut Ui<Message>,
        window: WindowId,
        scale_factor: f32,
        event: &WindowEvent,
    ) -> Result<DockViewportDragEvent, UiError> {
        if self.source == window {
            return Ok(
                if matches!(
                    event,
                    WindowEvent::PointerButton {
                        device_id,
                        button: PointerButton::Primary,
                        state: ElementState::Released,
                    } if *device_id == self.device_id
                ) {
                    DockViewportDragEvent::Ended
                } else {
                    DockViewportDragEvent::Active
                },
            );
        }
        match event {
            WindowEvent::PointerMoved {
                device_id,
                position,
            } if *device_id == self.device_id => {
                let scale = scale_factor.max(f32::EPSILON);
                let logical =
                    LogicalPoint::new(position.x as f32 / scale, position.y as f32 / scale);
                ui.update_external_drag(
                    self.session,
                    self.device_id,
                    logical,
                    DragPayload::new(self.panel.clone()),
                    DragOperations::MOVE,
                )?;
                self.target = Some((window, logical));
                Ok(DockViewportDragEvent::Active)
            }
            WindowEvent::PointerLeft { device_id } if *device_id == self.device_id => {
                let position = self
                    .target
                    .filter(|(target, _)| *target == window)
                    .map(|(_, position)| position)
                    .unwrap_or(LogicalPoint::ZERO);
                ui.leave_external_drag(self.session, position)?;
                self.target = None;
                Ok(DockViewportDragEvent::Active)
            }
            WindowEvent::PointerButton {
                device_id,
                button: PointerButton::Primary,
                state: ElementState::Released,
            } if *device_id == self.device_id => {
                let Some((target, position)) = self.target else {
                    return Ok(DockViewportDragEvent::Ended);
                };
                if target != window {
                    return Ok(DockViewportDragEvent::Ended);
                }
                Ok(match ui.finish_external_drag(self.session, position)? {
                    Some(operation) => DockViewportDragEvent::DropDispatched(operation),
                    None => DockViewportDragEvent::Ended,
                })
            }
            _ => Ok(DockViewportDragEvent::Active),
        }
    }
}

#[cfg(test)]
mod tests {
    use astrelis_core::geometry::{Physical, Point, Size};
    use astrelis_platform::WindowId;
    use astrelis_text::FontDatabase;
    use astrelis_ui_core::{EventFilter, RoutedEventKind, Theme, Ui};

    use super::*;

    #[test]
    fn routes_a_pointer_release_into_another_window_tree() {
        let device = DeviceId(4);
        let mut ui = Ui::<()>::new(FontDatabase::default(), Theme::default());
        ui.set_viewport(Size::new(300.0, 180.0), 1.0);
        let target = ui.add_button(ui.root(), "Dock here").unwrap();
        ui.listen(target, None, EventFilter::Drag, |context, event| {
            if let RoutedEventKind::DragOver { device_id, .. } = event.kind {
                context.accept_drop(device_id, DropOperation::Move);
            }
        })
        .unwrap();
        let mut drag = DockViewportDrag::new(
            WindowId(1),
            DragSessionId::from_raw(8),
            device,
            PanelId::new("inspector").unwrap(),
        );
        assert_eq!(
            drag.handle_window_event(
                &mut ui,
                WindowId(2),
                1.0,
                &WindowEvent::PointerMoved {
                    device_id: device,
                    position: Point::<Physical, f64>::new(20.0, 20.0),
                },
            )
            .unwrap(),
            DockViewportDragEvent::Active
        );
        assert_eq!(
            drag.handle_window_event(
                &mut ui,
                WindowId(2),
                1.0,
                &WindowEvent::PointerButton {
                    device_id: device,
                    button: PointerButton::Primary,
                    state: ElementState::Released,
                },
            )
            .unwrap(),
            DockViewportDragEvent::DropDispatched(DropOperation::Move)
        );
    }
}
