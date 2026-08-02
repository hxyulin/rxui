//! Reusable instrumented element for retained-tree tests.

use std::{cell::RefCell, rc::Rc};

use astrelis_core::geometry::{LogicalPoint, LogicalSize};
use astrelis_platform::{CursorIcon, Key, NamedKey};
use rxui_tree::{
    ClipboardOperation, Constraints, Element, EventResult, LayoutContext, SemanticAction,
    SemanticActionKind, SemanticData, SemanticRole, UiInput,
};

/// One observable event delivered to a [`Probe`].
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeEvent {
    /// A pointer press localized to the probe.
    PointerPressed(LogicalPoint),
    /// A pointer move localized to the probe.
    PointerMoved(LogicalPoint),
    /// A pointer release localized to the probe.
    PointerReleased(LogicalPoint),
    /// The pointer entered or left the probe.
    HoverChanged(bool),
    /// Keyboard focus was gained or lost.
    FocusChanged(bool),
    /// A logical keyboard key was delivered.
    Key(Key),
    /// Pointer, keyboard, or semantic activation occurred.
    Activated,
}

/// Shared observation handle for a [`Probe`].
#[derive(Clone, Debug, Default)]
pub struct ProbeLog(Rc<RefCell<Vec<ProbeEvent>>>);

impl ProbeLog {
    /// Returns a snapshot of all recorded events.
    pub fn events(&self) -> Vec<ProbeEvent> {
        self.0.borrow().clone()
    }

    /// Clears all recorded events.
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    /// Counts activation events.
    pub fn activations(&self) -> usize {
        self.0
            .borrow()
            .iter()
            .filter(|event| matches!(event, ProbeEvent::Activated))
            .count()
    }

    fn push(&self, event: ProbeEvent) {
        self.0.borrow_mut().push(event);
    }
}

/// Focusable, hit-testable, semantically labeled test element.
pub struct Probe {
    label: String,
    size: LogicalSize,
    cursor: CursorIcon,
    log: ProbeLog,
    clipboard: Option<(NamedKey, String)>,
}

impl Probe {
    /// Creates a button-like probe and its shared event log.
    pub fn new(label: impl Into<String>, size: LogicalSize) -> (Self, ProbeLog) {
        let log = ProbeLog::default();
        (
            Self {
                label: label.into(),
                size,
                cursor: CursorIcon::Default,
                log: log.clone(),
                clipboard: None,
            },
            log,
        )
    }

    /// Selects the cursor exposed while this probe is hovered or captured.
    pub const fn with_cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = cursor;
        self
    }

    /// Requests a clipboard write when `key` is delivered.
    pub fn with_clipboard_write(mut self, key: NamedKey, text: impl Into<String>) -> Self {
        self.clipboard = Some((key, text.into()));
        self
    }
}

impl Element for Probe {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LogicalSize {
        constraints.constrain(self.size)
    }

    fn accessibility(&self) -> Option<SemanticData> {
        Some(SemanticData {
            role: SemanticRole::Button,
            label: self.label.clone(),
            ..SemanticData::default()
        })
    }

    fn event(&mut self, input: UiInput) -> EventResult {
        match input {
            UiInput::PointerPressed(point) => {
                self.log.push(ProbeEvent::PointerPressed(point));
                EventResult {
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::PointerMoved(point) => {
                self.log.push(ProbeEvent::PointerMoved(point));
                EventResult::default()
            }
            UiInput::PointerReleased(point) => {
                self.log.push(ProbeEvent::PointerReleased(point));
                self.log.push(ProbeEvent::Activated);
                EventResult {
                    handled: true,
                    ..EventResult::default()
                }
            }
            UiInput::HoverChanged(hovered) => {
                self.log.push(ProbeEvent::HoverChanged(hovered));
                EventResult::default()
            }
            UiInput::FocusChanged(focused) => {
                self.log.push(ProbeEvent::FocusChanged(focused));
                EventResult::default()
            }
            UiInput::Keyboard { input, .. } => {
                let key = input.logical_key;
                self.log.push(ProbeEvent::Key(key.clone()));
                if matches!(key, Key::Named(NamedKey::Enter | NamedKey::Space)) {
                    self.log.push(ProbeEvent::Activated);
                    return EventResult {
                        handled: true,
                        ..EventResult::default()
                    };
                }
                let clipboard = self.clipboard.as_ref().and_then(|(wanted, text)| {
                    (key == Key::Named(wanted.clone()))
                        .then(|| ClipboardOperation::WriteText(text.clone()))
                });
                EventResult {
                    handled: clipboard.is_some(),
                    clipboard,
                    ..EventResult::default()
                }
            }
            _ => EventResult::default(),
        }
    }

    fn semantic_actions(&self) -> Vec<SemanticActionKind> {
        vec![SemanticActionKind::Focus, SemanticActionKind::Activate]
    }

    fn semantic_action(&mut self, action: SemanticAction) -> EventResult {
        if matches!(action, SemanticAction::Activate) {
            self.log.push(ProbeEvent::Activated);
            EventResult {
                handled: true,
                ..EventResult::default()
            }
        } else {
            EventResult::default()
        }
    }

    fn hit_testable(&self) -> bool {
        true
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor_icon(&self) -> CursorIcon {
        self.cursor
    }
}
