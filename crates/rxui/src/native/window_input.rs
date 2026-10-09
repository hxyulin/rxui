//! Winit window event translation shared by `Application` and custom hosts.
use super::{
    ClickTracker, HostedUi, deletion_input, input_button, input_modifiers, keyboard_key,
    native_cursor, standard_shortcut,
};
use crate::{
    Bounds, CommandId, CommandStatus, ElementId, PointerEvent, Runtime, TextInputEvent,
    TextMovement, Ui, UiError, UiPainter, View,
};
use astrelis_winit::{
    WindowMetrics,
    winit::{
        dpi::{LogicalPosition, LogicalSize},
        event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
        keyboard::{Key, ModifiersState, NamedKey},
        window::Window,
    },
};
use std::time::Instant;

/// Per-window input state that translates winit events into [`Ui`] input, for hosts
/// that own their event loop or `astrelis_winit::Handler`.
///
/// It is the translation `Application` uses: physical cursor positions become logical
/// pointer events, presses get platform-style click counts, modifiers are tracked,
/// keys reach listeners first, then standard shortcuts reach command handlers, then
/// focus traversal, activation and text editing. Wheel, focus and IME events are
/// translated too. Keep one per window and pass every event of that window.
///
/// ```no_run
/// # use rxui::{UiError, UiPainter, WindowInput, prelude::*};
/// # use rxui::astrelis_winit::{WindowMetrics, winit::{event::WindowEvent, window::Window}};
/// # fn host<T: View>(runtime: &mut Runtime, ui: &mut Ui<T>, painter: &mut UiPainter,
/// #     input: &mut WindowInput, window: &Window, metrics: WindowMetrics, event: WindowEvent)
/// #     -> Result<(), UiError> {
/// let result = input.handle(runtime, ui, painter, metrics, &event)?;
/// if let Some(command) = result.command {
///     // A standard shortcut, such as copy or quit, that no command handler took.
/// }
/// input.sync_window(ui, painter, window)?; // Cursor icon and IME state.
/// if result.changed {
///     window.request_redraw();
/// }
/// # Ok(()) }
/// ```
#[derive(Default)]
pub struct WindowInput {
    /// Physical cursor position.
    cursor: [f64; 2],
    clicks: ClickTracker,
    modifiers: ModifiersState,
    cursor_icon: Option<crate::Cursor>,
    ime_focus: Option<ElementId>,
    ime_reset_revision: u64,
    ime_allowed: bool,
    ime_area: Option<Bounds>,
    /// Click count of a press on a window drag region, applied by `sync`.
    window_drag: Option<u8>,
}
/// Outcome of [`WindowInput::handle`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowInputResult {
    /// Retained or application state changed; the window needs a redraw.
    pub changed: bool,
    /// A standard shortcut (undo, redo, copy, cut, paste, select all, close window,
    /// quit) that no command handler in the placement took. The host can apply its
    /// platform default, such as reading the clipboard into a
    /// [`TextInputEvent::Paste`].
    pub command: Option<CommandId>,
}
impl WindowInput {
    /// Fresh state: cursor at the origin, no modifiers, no IME session.
    pub fn new() -> Self {
        Self::default()
    }
    /// Current modifier state.
    pub fn modifiers(&self) -> crate::Modifiers {
        input_modifiers(self.modifiers)
    }
    /// Last cursor position in logical units.
    pub fn cursor_position(&self, scale_factor: f64) -> [f32; 2] {
        [
            (self.cursor[0] / scale_factor) as f32,
            (self.cursor[1] / scale_factor) as f32,
        ]
    }
    /// Prepares stale geometry at the window's logical size, then routes `event`.
    pub fn handle<T: View>(
        &mut self,
        runtime: &mut Runtime,
        ui: &mut Ui<T>,
        painter: &mut UiPainter,
        metrics: WindowMetrics,
        event: &WindowEvent,
    ) -> Result<WindowInputResult, UiError> {
        let ui: &mut dyn HostedUi = ui;
        ui.prepare_input(runtime, metrics, painter)?;
        let (mut changed, mut prevented) = self.key(runtime, ui, event)?;
        let mut command = None;
        if !prevented && let Some((id, repeat)) = self.shortcut(event) {
            let status = if repeat && Self::lifecycle(id) {
                CommandStatus::Disabled
            } else {
                ui.dispatch_command(runtime, id)?
            };
            prevented = status != CommandStatus::Unhandled;
            changed |= status == CommandStatus::Handled;
            command = (!prevented).then_some(id);
        }
        changed |= self.event(
            runtime,
            ui,
            painter,
            metrics.scale_factor(),
            event,
            prevented,
        )?;
        Ok(WindowInputResult { changed, command })
    }
    /// Applies the placement's cursor icon and IME enablement and area to `window`,
    /// and starts a pending window move from a press on a window drag region (a
    /// double press toggles maximization instead). Call it after input, within the
    /// same event callback, and after each preparation.
    pub fn sync_window<T: View>(
        &mut self,
        ui: &mut Ui<T>,
        painter: &mut UiPainter,
        window: &Window,
    ) -> Result<(), UiError> {
        self.sync(ui, painter, window)
    }

    /// Close and quit requests do not repeat while a save or veto is outstanding.
    pub(super) fn lifecycle(id: CommandId) -> bool {
        use crate::standard_commands::{CloseWindow, Quit};
        id == CommandId::of::<Quit>() || id == CommandId::of::<CloseWindow>()
    }
    /// Routes a keyboard event to key listeners: `(changed, default_prevented)`.
    pub(super) fn key(
        &self,
        runtime: &mut Runtime,
        ui: &mut dyn HostedUi,
        event: &WindowEvent,
    ) -> Result<(bool, bool), UiError> {
        let WindowEvent::KeyboardInput {
            event,
            is_synthetic: false,
            ..
        } = event
        else {
            return Ok((false, false));
        };
        let result = ui.key(
            runtime,
            crate::KeyEvent {
                key: keyboard_key(&event.logical_key),
                pressed: event.state == ElementState::Pressed,
                repeat: event.repeat,
                modifiers: input_modifiers(self.modifiers),
            },
        )?;
        Ok((result.changed, result.default_prevented))
    }
    /// The standard shortcut of a key press, with its repeat flag.
    pub(super) fn shortcut(&self, event: &WindowEvent) -> Option<(CommandId, bool)> {
        let WindowEvent::KeyboardInput {
            event,
            is_synthetic: false,
            ..
        } = event
        else {
            return None;
        };
        if event.state != ElementState::Pressed {
            return None;
        }
        standard_shortcut(
            &keyboard_key(&event.logical_key),
            input_modifiers(self.modifiers),
        )
        .map(|id| (id, event.repeat))
    }
    /// Everything after key listeners and shortcuts. `key_prevented` suppresses the
    /// default key handling.
    pub(super) fn event(
        &mut self,
        runtime: &mut Runtime,
        ui: &mut dyn HostedUi,
        painter: &mut UiPainter,
        scale: f64,
        event: &WindowEvent,
        key_prevented: bool,
    ) -> Result<bool, UiError> {
        let modifiers = input_modifiers(self.modifiers);
        let extend = self.modifiers.shift_key();
        let mut changed = false;
        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                ui.invalidate_geometry()
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = [position.x, position.y];
                let position = self.cursor_position(scale);
                self.clicks.motion(position);
                changed = ui.pointer(
                    runtime,
                    PointerEvent::Motion {
                        position,
                        modifiers,
                    },
                    painter,
                    extend,
                    1,
                )?;
            }
            WindowEvent::CursorLeft { .. } => {
                changed = ui.pointer(runtime, PointerEvent::Left, painter, false, 1)?
            }
            WindowEvent::MouseInput { state, button, .. }
                if matches!(
                    button,
                    MouseButton::Left | MouseButton::Right | MouseButton::Middle
                ) =>
            {
                let position = self.cursor_position(scale);
                let pressed = *state == ElementState::Pressed;
                let count = if pressed && *button == MouseButton::Left {
                    self.clicks
                        .press(Instant::now(), position, ui.hit_test(position))
                } else {
                    1
                };
                // A drag region moves the window instead of reaching the UI, like a
                // native title bar; focus and capture stay where they were.
                if pressed && *button == MouseButton::Left && ui.window_drag_region_at(position) {
                    self.window_drag = Some(count);
                    return Ok(false);
                }
                let button = input_button(*button);
                let event = if pressed {
                    PointerEvent::Down {
                        position,
                        button,
                        modifiers,
                    }
                } else {
                    PointerEvent::Up {
                        position,
                        button,
                        modifiers,
                    }
                };
                changed = ui.pointer(runtime, event, painter, extend, count)?;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => [-x * 40., -y * 40.],
                    MouseScrollDelta::PixelDelta(p) => {
                        [-(p.x / scale) as f32, -(p.y / scale) as f32]
                    }
                };
                changed = ui.wheel(runtime, self.cursor_position(scale), delta, modifiers)?;
            }
            WindowEvent::Focused(active) => {
                if !active {
                    self.clicks = ClickTracker::default();
                    changed |= ui.pointer(runtime, PointerEvent::Cancelled, painter, false, 1)?;
                }
                changed |= ui.active(*active);
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                let event = TextInputEvent::Preedit {
                    text: text.clone(),
                    cursor: *cursor,
                };
                changed = ui.text_input(runtime, event, painter)?;
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                changed = ui.text_input(runtime, TextInputEvent::Commit(text.clone()), painter)?;
            }
            WindowEvent::Ime(Ime::Disabled) => {
                changed = ui.text_input(runtime, TextInputEvent::CancelComposition, painter)?;
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed && !key_prevented => {
                let modifiers = self.modifiers;
                let primary = if cfg!(target_os = "macos") {
                    modifiers.super_key()
                } else {
                    modifiers.control_key() && !modifiers.alt_key()
                };
                let word = if cfg!(target_os = "macos") {
                    modifiers.alt_key()
                } else {
                    modifiers.control_key()
                };
                let mac = cfg!(target_os = "macos");
                let movement = |movement| Some(TextInputEvent::Move { movement, extend });
                if event.logical_key == Key::Named(NamedKey::Tab) && !event.repeat {
                    changed |= ui.focus_next(extend);
                } else if ui.has_text_focus() {
                    let input = match &event.logical_key {
                        Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
                            let right = event.logical_key == Key::Named(NamedKey::ArrowRight);
                            let movement = if mac && primary {
                                if right {
                                    TextMovement::LineEnd
                                } else {
                                    TextMovement::LineStart
                                }
                            } else if word {
                                if right {
                                    TextMovement::WordRight
                                } else {
                                    TextMovement::WordLeft
                                }
                            } else if right {
                                TextMovement::Right
                            } else {
                                TextMovement::Left
                            };
                            Some(TextInputEvent::Move { movement, extend })
                        }
                        // Command+Up/Down on macOS, and Control+Home/End elsewhere,
                        // reach the start/end of the value.
                        Key::Named(NamedKey::ArrowUp) if mac && primary => {
                            movement(TextMovement::Start)
                        }
                        Key::Named(NamedKey::ArrowDown) if mac && primary => {
                            movement(TextMovement::End)
                        }
                        Key::Named(NamedKey::ArrowUp) => movement(TextMovement::Up),
                        Key::Named(NamedKey::ArrowDown) => movement(TextMovement::Down),
                        Key::Named(NamedKey::PageUp) => movement(TextMovement::PageUp),
                        Key::Named(NamedKey::PageDown) => movement(TextMovement::PageDown),
                        // macOS Home/End reach the document edges, as in Cocoa text
                        // views; elsewhere they stay on the line.
                        Key::Named(NamedKey::Home) => movement(if mac || primary {
                            TextMovement::Start
                        } else {
                            TextMovement::LineStart
                        }),
                        Key::Named(NamedKey::End) => movement(if mac || primary {
                            TextMovement::End
                        } else {
                            TextMovement::LineEnd
                        }),
                        Key::Named(NamedKey::Backspace) => {
                            Some(deletion_input(true, primary, word))
                        }
                        Key::Named(NamedKey::Delete) => Some(deletion_input(false, primary, word)),
                        Key::Named(NamedKey::Escape) => Some(TextInputEvent::CancelComposition),
                        Key::Named(NamedKey::Enter) => Some(TextInputEvent::Enter {
                            modifiers: input_modifiers(modifiers),
                            repeat: event.repeat,
                        }),
                        _ if !primary && (!modifiers.control_key() || modifiers.alt_key()) => event
                            .text
                            .as_ref()
                            .filter(|text| text.chars().any(|c| !c.is_control()))
                            .map(|text| TextInputEvent::Insert(text.to_string())),
                        _ => None,
                    };
                    if let Some(input) = input {
                        changed |= ui.text_input(runtime, input, painter)?;
                    }
                } else if !event.repeat
                    && matches!(
                        event.logical_key,
                        Key::Named(NamedKey::Enter | NamedKey::Space)
                    )
                {
                    changed |= ui.activate(runtime)?;
                }
            }
            _ => {}
        }
        Ok(changed)
    }
    /// Cursor icon and IME state; see [`Self::sync_window`].
    pub(super) fn sync(
        &mut self,
        ui: &mut dyn HostedUi,
        painter: &mut UiPainter,
        window: &Window,
    ) -> Result<(), UiError> {
        match self.window_drag.take() {
            Some(2) => window.set_maximized(!window.is_maximized()),
            // A failed move (no button held, unsupported backend) is not an error.
            Some(_) => drop(window.drag_window()),
            None => {}
        }
        let cursor = ui.cursor_icon();
        if self.cursor_icon != Some(cursor) {
            let hidden = |c: Option<crate::Cursor>| c == Some(crate::Cursor::Hidden);
            match native_cursor(cursor) {
                Some(icon) => {
                    window.set_cursor(icon);
                    if hidden(self.cursor_icon) {
                        window.set_cursor_visible(true);
                    }
                }
                None => window.set_cursor_visible(false),
            }
            self.cursor_icon = Some(cursor);
        }
        // Password inputs keep IME disabled; they take committed text only.
        let focus = ui.accepts_ime().then(|| ui.focused_element()).flatten();
        let reset_revision = ui.ime_reset_revision();
        if focus != self.ime_focus || reset_revision != self.ime_reset_revision {
            self.ime_reset_revision = reset_revision;
            if self.ime_allowed {
                window.set_ime_allowed(false);
            }
            self.ime_focus = focus;
            self.ime_allowed = focus.is_some();
            window.set_ime_allowed(self.ime_allowed);
            self.ime_area = None;
        }
        if self.ime_allowed
            && let Some(area) = ui.ime_area(painter)?
            && self.ime_area != Some(area)
        {
            window.set_ime_cursor_area(
                LogicalPosition::new(area.x as f64, area.y as f64),
                LogicalSize::new(area.width as f64, area.height as f64),
            );
            self.ime_area = Some(area);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astrelis::GraphicsContext;
    use astrelis_winit::winit::{dpi::PhysicalSize, event::Modifiers};

    #[test]
    #[ignore = "requires a native GPU; run with --features native -- --ignored"]
    fn translates_modifiers_focus_and_ime_commits() {
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        let mut test = crate::testing::tests::form(&graphics);
        test.click("name").unwrap();
        let metrics = WindowMetrics::new(PhysicalSize::new(320, 240), 2.).unwrap();
        let (runtime, ui, painter) = test.parts();
        let mut input = WindowInput::new();
        let mut send = |input: &mut WindowInput, event: WindowEvent| {
            input.handle(runtime, ui, painter, metrics, &event).unwrap()
        };

        let shift = Modifiers::from(ModifiersState::SHIFT);
        assert!(!send(&mut input, WindowEvent::ModifiersChanged(shift)).changed);
        assert!(input.modifiers().shift);
        let commit = WindowEvent::Ime(Ime::Commit("Ada".into()));
        assert_eq!(
            send(&mut input, commit),
            WindowInputResult {
                changed: true,
                command: None
            }
        );
        assert!(send(&mut input, WindowEvent::Focused(false)).changed);
        assert_eq!(input.cursor_position(2.), [0., 0.]);
        assert_eq!(test.read(|form| form.name.clone()), "Ada");
    }
}
