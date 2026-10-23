//! Winit window event translation shared by `Application` and custom hosts.
use super::{
    ClickTracker, HostedUi, deletion_input, input_button, input_modifiers, keyboard_key,
    native_cursor, physical_key, standard_shortcut,
};
use crate::{
    Bounds, CommandId, CommandStatus, ElementId, KeyEvent, PointerButton, PointerButtons,
    PointerEvent, Runtime, TextInputEvent, TextMovement, Ui, UiError, UiPainter, View,
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
use std::{cell::RefCell, rc::Rc, time::Instant};

/// Per-window input state that translates winit events into [`Ui`] input, for hosts
/// that own their event loop or `astrelis_winit::Handler`.
///
/// It is the translation `Application` uses: physical cursor positions become logical
/// pointer events, presses get platform-style click counts, modifiers are tracked,
/// keys reach listeners first, then standard shortcuts reach command handlers, then
/// focus traversal, activation and text editing, or the produced text for a focused
/// text input client. Wheel, focus and IME events are translated too. Keep one per window and pass every event of that window.
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
pub struct WindowInput {
    pub(super) pointer: Rc<RefCell<super::pointer::State>>,
    standard_shortcuts: bool,
    active: bool,
    pressed_keys: Vec<KeyEvent>,
    buttons: PointerButtons,
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
impl Default for WindowInput {
    fn default() -> Self {
        Self {
            pointer: Rc::default(),
            standard_shortcuts: true,
            active: true,
            pressed_keys: Vec::new(),
            buttons: PointerButtons::default(),
            cursor: [0.; 2],
            clicks: ClickTracker::default(),
            modifiers: ModifiersState::default(),
            cursor_icon: None,
            ime_focus: None,
            ime_reset_revision: 0,
            ime_allowed: false,
            ime_area: None,
            window_drag: None,
        }
    }
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
    pub(super) fn with_standard_shortcuts(enabled: bool) -> Self {
        Self {
            standard_shortcuts: enabled,
            ..Self::default()
        }
    }
    /// Enables standard editing and close/quit shortcuts (enabled by default).
    pub fn set_standard_shortcuts(&mut self, enabled: bool) {
        self.standard_shortcuts = enabled;
    }
    /// Locks and hides the native pointer, falling back to confinement. Unlocks
    /// on request and on deactivation; focus gain never automatically re-locks it.
    /// Returns the selected mode or the native error if neither grab works.
    pub fn set_pointer_lock(
        &mut self,
        window: &Window,
        locked: bool,
    ) -> Result<Option<super::PointerLock>, astrelis_winit::winit::error::ExternalError> {
        self.pointer.borrow_mut().set_lock(window, locked)
    }
    /// Hides the cursor independently of hovered icons and pointer locking.
    pub fn set_cursor_hidden(&mut self, window: &Window, hidden: bool) {
        let mut pointer = self.pointer.borrow_mut();
        pointer.hidden = hidden;
        pointer.visibility(window, self.cursor_icon == Some(crate::Cursor::Hidden));
    }
    /// Takes a native error from automatic pointer release on deactivation.
    pub fn take_pointer_lock_error(&mut self) -> Option<String> {
        self.pointer.borrow_mut().error.take()
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

    #[cfg(all(feature = "native-docking", not(target_arch = "wasm32")))]
    pub(super) fn native_release_position(&mut self, position: [f64; 2]) {
        if position.iter().all(|n| n.is_finite()) {
            self.cursor = position;
        }
    }
    #[cfg(all(feature = "native-docking", not(target_arch = "wasm32")))]
    pub(super) fn cancel_primary(
        &mut self,
        runtime: &mut Runtime,
        ui: &mut dyn HostedUi,
        painter: &mut UiPainter,
        scale: f64,
    ) -> Result<(), UiError> {
        let held = self.buttons.contains(PointerButton::Primary);
        self.buttons.set(PointerButton::Primary, false);
        self.clicks = ClickTracker::default();
        let release = if held {
            ui.pointer(
                runtime,
                PointerEvent::CancelledUp {
                    position: self.cursor_position(scale),
                    button: PointerButton::Primary,
                    modifiers: self.modifiers(),
                },
                painter,
                false,
                1,
            )
        } else {
            Ok(false)
        };
        let cancel = ui.pointer(runtime, PointerEvent::Cancelled, painter, false, 1);
        release?;
        cancel?;
        Ok(())
    }
    /// Close and quit requests do not repeat while a save or veto is outstanding.
    pub(super) fn lifecycle(id: CommandId) -> bool {
        use crate::standard_commands::{CloseWindow, Quit};
        id == CommandId::of::<Quit>() || id == CommandId::of::<CloseWindow>()
    }
    /// Routes a keyboard event to key listeners: `(changed, default_prevented)`.
    pub(super) fn key(
        &mut self,
        runtime: &mut Runtime,
        ui: &mut dyn HostedUi,
        event: &WindowEvent,
    ) -> Result<(bool, bool), UiError> {
        let WindowEvent::KeyboardInput {
            event,
            is_synthetic,
            ..
        } = event
        else {
            return Ok((false, false));
        };
        let key = KeyEvent {
            key: keyboard_key(&event.logical_key),
            physical_key: physical_key(event.physical_key),
            pressed: event.state == ElementState::Pressed,
            repeat: event.repeat,
            modifiers: input_modifiers(self.modifiers),
        };
        let Some(key) = self.track_key(key, *is_synthetic) else {
            return Ok((false, true));
        };
        let result = ui.key(runtime, key)?;
        Ok((result.changed, result.default_prevented))
    }
    fn track_key(&mut self, mut key: KeyEvent, synthetic: bool) -> Option<KeyEvent> {
        // Synthetic focus-gain presses must not re-arm controls; unmatched late
        // releases have already been synthesized on deactivation.
        let held = self
            .pressed_keys
            .iter()
            .position(|held| held.physical_key == key.physical_key);
        if key.pressed {
            if !self.active || synthetic {
                return None;
            }
            if held.is_none() {
                self.pressed_keys.push(key.clone());
            }
        } else {
            let index = held?;
            // Preserve the press identity across layout/modifier changes.
            key.key = self.pressed_keys.remove(index).key;
        }
        Some(key)
    }
    fn deactivate(&mut self) -> (Vec<KeyEvent>, PointerButtons) {
        self.active = false;
        self.pointer.borrow_mut().release_pending = true;
        self.clicks = ClickTracker::default();
        self.window_drag = None;
        self.modifiers = ModifiersState::default();
        let mut keys = std::mem::take(&mut self.pressed_keys);
        for key in &mut keys {
            key.pressed = false;
            key.repeat = false;
            key.modifiers = crate::Modifiers::default();
        }
        (keys, std::mem::take(&mut self.buttons))
    }
    fn shortcut_key(
        &self,
        key: &crate::KeyboardKey,
        modifiers: crate::Modifiers,
    ) -> Option<CommandId> {
        (self.standard_shortcuts && self.active)
            .then(|| standard_shortcut(key, modifiers))
            .flatten()
    }
    /// The standard shortcut of a key press, with its repeat flag.
    pub(super) fn shortcut(&self, event: &WindowEvent) -> Option<(CommandId, bool)> {
        if !self.standard_shortcuts || !self.active {
            return None;
        }
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
        self.shortcut_key(
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
                if !self.active || (!pressed && !self.buttons.contains(button)) {
                    return Ok(false);
                }
                self.buttons.set(button, pressed);
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
                // Wheel steps may animate; trackpad pixel deltas are already smooth.
                let (delta, source) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        ([-x * 40., -y * 40.], crate::WheelSource::Lines)
                    }
                    MouseScrollDelta::PixelDelta(p) => (
                        [-(p.x / scale) as f32, -(p.y / scale) as f32],
                        crate::WheelSource::Precise,
                    ),
                };
                let position = self.cursor_position(scale);
                changed = ui.wheel(runtime, position, delta, modifiers, source)?;
            }
            WindowEvent::Focused(active) => {
                let mut first_error = None;
                if !active {
                    // Clear host state before dispatch, even if a listener fails.
                    let (keys, buttons) = self.deactivate();
                    let mut released = |result: Result<bool, UiError>| match result {
                        Ok(updated) => changed |= updated,
                        Err(error) => {
                            first_error.get_or_insert(error);
                        }
                    };
                    for key in keys {
                        released(ui.key(runtime, key).map(|result| result.changed));
                    }
                    for button in [
                        PointerButton::Primary,
                        PointerButton::Secondary,
                        PointerButton::Middle,
                    ] {
                        if buttons.contains(button) {
                            released(ui.pointer(
                                runtime,
                                PointerEvent::CancelledUp {
                                    position: self.cursor_position(scale),
                                    button,
                                    modifiers: crate::Modifiers::default(),
                                },
                                painter,
                                false,
                                1,
                            ));
                        }
                    }
                    released(ui.pointer(runtime, PointerEvent::Cancelled, painter, false, 1));
                }
                self.active = *active;
                changed |= ui.active(*active);
                if let Some(error) = first_error {
                    return Err(error);
                }
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) if !ui.raw_keyboard() && self.active => {
                let event = TextInputEvent::Preedit {
                    text: text.clone(),
                    cursor: *cursor,
                };
                changed = ui.text_input(runtime, event, painter)?;
            }
            WindowEvent::Ime(Ime::Commit(text)) if !ui.raw_keyboard() && self.active => {
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
            } if event.state == ElementState::Pressed && !key_prevented && self.active => {
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
                } else if ui.has_text_client_focus() {
                    // A text input client handles editing keys in its key listeners;
                    // only the produced text and Escape's IME cancellation remain.
                    let input = match &event.logical_key {
                        Key::Named(NamedKey::Escape) => Some(TextInputEvent::CancelComposition),
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
            if let Some(icon) = native_cursor(cursor) {
                window.set_cursor(icon);
            }
            self.cursor_icon = Some(cursor);
        }
        self.pointer
            .borrow_mut()
            .sync(window, cursor == crate::Cursor::Hidden);
        // Password inputs keep IME disabled; they take committed text only. Text
        // input clients enable it like editable inputs.
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

    fn press() -> KeyEvent {
        KeyEvent {
            key: crate::KeyboardKey::Character("z".into()),
            physical_key: crate::PhysicalKey::KeyW,
            pressed: true,
            repeat: false,
            modifiers: crate::Modifiers {
                shift: true,
                ..Default::default()
            },
        }
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features native -- --ignored"]
    fn resilience_focus_loss_dispatches_every_release_after_listener_errors() {
        use crate::{Dispatch, IntoElement, KeyInput, Listener, PointerInput, ViewContext, column};
        #[derive(Default)]
        struct Controls {
            releases: Rc<RefCell<Vec<String>>>,
        }
        impl View for Controls {
            fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
                let keys = self.releases.clone();
                let pointers = self.releases.clone();
                column()
                    .size(100., 100.)
                    .on_key_up(Listener::from_callback(move |event: &KeyInput, _| {
                        keys.borrow_mut()
                            .push(format!("{:?}", event.event.physical_key));
                        Err(crate::AccessError::WrongRuntime)
                    }))
                    .on_pointer_up(Listener::from_callback(move |event: &PointerInput, _| {
                        pointers.borrow_mut().push(format!("{:?}", event.button));
                        // A second failure must not replace the first one.
                        if event.button == Some(PointerButton::Primary) {
                            Err(crate::AccessError::Borrowed)
                        } else {
                            Ok(Dispatch::Unchanged)
                        }
                    }))
            }
        }
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        let controls = Controls::default();
        let releases = controls.releases.clone();
        let mut test = crate::TestUi::new(&graphics, [100, 100], controls).unwrap();
        let (runtime, ui, painter) = test.parts();
        let mut input = WindowInput::new();
        input.track_key(press(), false);
        input.track_key(
            KeyEvent {
                physical_key: crate::PhysicalKey::KeyA,
                ..press()
            },
            false,
        );
        for button in [
            PointerButton::Primary,
            PointerButton::Secondary,
            PointerButton::Middle,
        ] {
            input.buttons.set(button, true);
        }
        assert!(matches!(
            input.handle(
                runtime,
                ui,
                painter,
                WindowMetrics::new(PhysicalSize::new(100, 100), 1.).unwrap(),
                &WindowEvent::Focused(false),
            ),
            Err(UiError::Access(crate::AccessError::WrongRuntime))
        ));
        assert_eq!(
            *releases.borrow(),
            [
                "KeyW",
                "KeyA",
                "Some(Primary)",
                "Some(Secondary)",
                "Some(Middle)"
            ]
        );
        assert!(!input.active);
        assert!(input.pressed_keys.is_empty());
        assert!(!input.buttons.any());
        assert!(!ui.active(false));
    }
    #[test]
    fn held_inputs_release_once_and_synthetic_presses_do_not_rearm() {
        let mut input = WindowInput::new();
        let key = press();
        assert_eq!(input.track_key(key.clone(), false), Some(key.clone()));
        assert!(
            input
                .track_key(
                    KeyEvent {
                        repeat: true,
                        ..key.clone()
                    },
                    false
                )
                .is_some()
        );
        input.buttons.set(PointerButton::Primary, true);
        input.buttons.set(PointerButton::Secondary, true);
        input.modifiers = ModifiersState::SHIFT;
        let (keys, buttons) = input.deactivate();
        assert_eq!(
            keys,
            vec![KeyEvent {
                pressed: false,
                modifiers: Default::default(),
                ..key.clone()
            }]
        );
        assert!(buttons.contains(PointerButton::Primary));
        assert!(buttons.contains(PointerButton::Secondary));
        assert_eq!(input.modifiers(), crate::Modifiers::default());
        assert!(
            input
                .track_key(
                    KeyEvent {
                        pressed: false,
                        ..key.clone()
                    },
                    true
                )
                .is_none()
        );
        assert!(input.deactivate().0.is_empty());
        assert!(!input.deactivate().1.any());
        input.active = true;
        assert!(input.track_key(key.clone(), true).is_none());
        assert!(input.track_key(key.clone(), false).is_some());
        let released = input
            .track_key(
                KeyEvent {
                    key: crate::KeyboardKey::Character("w".into()),
                    pressed: false,
                    ..key.clone()
                },
                false,
            )
            .unwrap();
        assert_eq!(released.key, key.key);
        assert_eq!(released.physical_key, crate::PhysicalKey::KeyW);
    }
    #[test]
    fn standard_shortcuts_can_be_disabled_per_window() {
        let mut input = WindowInput::new();
        for character in ["w", "q", "z", "y", "c", "x", "v", "a"] {
            let shortcut = crate::Shortcut::primary(character);
            assert_eq!(
                input
                    .shortcut_key(&shortcut.key, shortcut.modifiers)
                    .is_some(),
                character != "y" || !cfg!(target_os = "macos")
            );
            input.set_standard_shortcuts(false);
            assert!(
                input
                    .shortcut_key(&shortcut.key, shortcut.modifiers)
                    .is_none()
            );
            input.set_standard_shortcuts(true);
        }
        assert!(
            !super::super::WindowOptions::new()
                .standard_shortcuts(false)
                .standard_shortcuts
        );
    }
    #[test]
    fn physical_positions_and_named_modifiers_are_typed() {
        use astrelis_winit::winit::keyboard::{KeyCode, PhysicalKey};
        assert_eq!(
            physical_key(PhysicalKey::Code(KeyCode::KeyW)),
            crate::PhysicalKey::KeyW
        );
        assert_eq!(
            physical_key(PhysicalKey::Code(KeyCode::ShiftRight)),
            crate::PhysicalKey::ShiftRight
        );
        assert_eq!(
            keyboard_key(&Key::Named(NamedKey::Control)),
            crate::KeyboardKey::Control
        );
        assert_eq!(
            keyboard_key(&Key::Named(NamedKey::F12)),
            crate::KeyboardKey::Function(12)
        );
    }
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

    #[test]
    #[ignore = "requires a native GPU; run with --features native -- --ignored"]
    fn routes_ime_to_a_focused_text_input_client() {
        use crate::{
            CustomElement, ImeEvent, ImeInput, IntoElement, TextInsertInput, View, ViewContext,
            column, custom,
        };
        #[derive(PartialEq)]
        struct Surface;
        impl CustomElement for Surface {
            type State = ();
        }
        #[derive(Default)]
        struct Client {
            log: Vec<String>,
        }
        impl View for Client {
            fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
                column().child(
                    custom(Surface)
                        .key("client")
                        .size(200., 40.)
                        .text_input_client(true)
                        .on_pointer_down(cx.listener(|_, e: &crate::PointerInput, _| e.focus()))
                        .on_text_insert(cx.listener(|s, e: &TextInsertInput, _| {
                            s.log.push(format!("{:?} {}", e.source, e.text))
                        }))
                        .on_ime(cx.listener(|s, e: &ImeInput, _| {
                            s.log.push(match &e.event {
                                ImeEvent::Preedit { text, .. } => format!("preedit {text}"),
                                ImeEvent::Cancel(reason) => format!("cancel {reason:?}"),
                            })
                        })),
                )
            }
        }
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        let mut test = crate::TestUi::new(&graphics, [320, 240], Client::default()).unwrap();
        test.click("client").unwrap();
        let metrics = WindowMetrics::new(PhysicalSize::new(640, 480), 2.).unwrap();
        let (runtime, ui, painter) = test.parts();
        assert!(ui.accepts_ime());
        let mut input = WindowInput::new();
        let mut send = |input: &mut WindowInput, event: WindowEvent| {
            input.handle(runtime, ui, painter, metrics, &event).unwrap()
        };
        let preedit = WindowEvent::Ime(Ime::Preedit("か".into(), Some((0, 3))));
        assert!(send(&mut input, preedit).changed);
        assert!(send(&mut input, WindowEvent::Ime(Ime::Commit("可".into()))).changed);
        assert!(send(&mut input, WindowEvent::Ime(Ime::Preedit("x".into(), None))).changed);
        assert!(send(&mut input, WindowEvent::Ime(Ime::Disabled)).changed);
        assert!(send(&mut input, WindowEvent::Ime(Ime::Preedit("y".into(), None))).changed);
        assert!(send(&mut input, WindowEvent::Focused(false)).changed);
        test.prepare().unwrap();
        assert_eq!(
            test.read(|c| c.log.clone()),
            [
                "preedit か",
                "Ime 可",
                "preedit x",
                "cancel Host",
                "preedit y",
                "cancel Deactivated"
            ]
        );
    }
}
