//! Explicit application-owned transfer of committed built-in editing state.
use super::*;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

/// Application document identity and accepted-value version. Increment the version
/// on every accepted/external replacement; use distinct keys for distinct documents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEditingIdentity {
    /// Stable application document key, independent of element keys and mounts.
    pub document: Key,
    /// Application-owned version of the exact controlled value.
    pub version: u64,
}
impl TextEditingIdentity {
    /// Creates an explicit document/version contract.
    pub fn new(document: impl Into<Key>, version: u64) -> Self {
        Self {
            document: document.into(),
            version,
        }
    }
}
/// Committed selection (including direction/affinity) and bounded undo/redo.
/// Contains the exact value, potentially including secrets; keep it in trusted
/// application memory. No preedit, geometry, focus, drag or mount state is retained.
#[derive(Clone)]
pub struct TextEditingSnapshot {
    identity: TextEditingIdentity,
    value: String,
    selection: TextSelection,
    history: crate::editing::History,
    multiline: bool,
    masked: bool,
}
impl fmt::Debug for TextEditingSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextEditingSnapshot")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}
/// Capture or restore failure; neither operation consumes the snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEditingError {
    /// The weak placement no longer exists.
    Disposed,
    /// More than one live placement uses the handle. Use separate session handles
    /// or the explicit Ui API to select a placement.
    Ambiguous,
    /// Target is not a built-in text field with an explicit identity.
    Unbound,
    /// Document key or application version differs.
    IdentityMismatch,
    /// Controlled bytes differ, even if the application version is equal.
    ValueMismatch,
    /// Single/multiline or password mode differs.
    ModeMismatch,
    /// An unaccepted proposal exists, or restore would overwrite active preedit/drag.
    Busy,
}
impl fmt::Display for TextEditingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "text editing transfer: {self:?}")
    }
}
impl Error for TextEditingError {}
#[derive(Clone, Default)]
pub(crate) struct Properties {
    life: Rc<()>,
    pub identity: Option<TextEditingIdentity>,
    pub handle: Option<TextEditingHandle>,
    pub restore: Option<TextEditingCapture>,
}
struct Binding {
    target: ElementId,
    life: Weak<()>,
    description: Weak<()>,
    commands: Weak<RefCell<Vec<CaptureCommand>>>,
}
/// Weak lookup/queued capture capability for ordinary application event handlers.
/// This is session state, not generic mount retention. Use one handle per editor
/// session; simultaneous views of a shared document should use independent handles.
#[derive(Clone, Default)]
pub struct TextEditingHandle(Rc<RefCell<Vec<Binding>>>);
impl fmt::Debug for TextEditingHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextEditingHandle").finish_non_exhaustive()
    }
}
impl TextEditingHandle {
    /// Creates an unbound handle.
    pub fn new() -> Self {
        Self::default()
    }
    /// Queues nondestructive capture before the source's next description refresh,
    /// including a refresh that removes it. Does not cancel IME or pointer capture.
    pub fn capture(&self) -> Result<TextEditingCapture, TextEditingError> {
        let mut bindings = self.0.borrow_mut();
        bindings.retain(|b| {
            b.life.strong_count() != 0
                && b.description.strong_count() != 0
                && b.commands.strong_count() != 0
        });
        let binding = match bindings.as_slice() {
            [] => return Err(TextEditingError::Disposed),
            [binding] => binding,
            _ => return Err(TextEditingError::Ambiguous),
        };
        let commands = binding
            .commands
            .upgrade()
            .ok_or(TextEditingError::Disposed)?;
        let capture = TextEditingCapture(Rc::new(RefCell::new(CaptureData::default())));
        commands.borrow_mut().push(CaptureCommand {
            target: binding.target,
            description: binding.description.clone(),
            capture: capture.clone(),
        });
        Ok(capture)
    }
}
/// App-owned capture ticket and destination results. Clone into a replacement's
/// `.restore_text_editing(ticket)`. Pending tickets do not cause preparation loops;
/// readiness makes `Ui::needs_prepare` true in destinations on the next host turn.
#[derive(Clone)]
pub struct TextEditingCapture(Rc<RefCell<CaptureData>>);
#[derive(Default)]
struct CaptureData {
    snapshot: Option<Result<TextEditingSnapshot, TextEditingError>>,
    restores: HashMap<ElementId, Result<(), TextEditingError>>,
}
impl fmt::Debug for TextEditingCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextEditingCapture").finish_non_exhaustive()
    }
}
impl TextEditingCapture {
    /// Creates a ready ticket from a previously saved snapshot.
    pub fn from_snapshot(snapshot: TextEditingSnapshot) -> Self {
        Self(Rc::new(RefCell::new(CaptureData {
            snapshot: Some(Ok(snapshot)),
            ..Default::default()
        })))
    }
    /// None while queued; an error or cloneable snapshot after source preparation.
    pub fn snapshot(&self) -> Option<Result<TextEditingSnapshot, TextEditingError>> {
        self.0.borrow().snapshot.clone()
    }
    /// Last attempt for an explicit destination. None until a ready ticket was tried.
    pub fn restore_result(&self, destination: ElementId) -> Option<Result<(), TextEditingError>> {
        self.0.borrow().restores.get(&destination).copied()
    }
    /// All attempted destination results, for app handlers without access to Ui.
    pub fn restore_results(&self) -> Vec<(ElementId, Result<(), TextEditingError>)> {
        self.0
            .borrow()
            .restores
            .iter()
            .map(|(id, result)| (*id, *result))
            .collect()
    }
    /// Explicitly retries failed restores during the next preparation, after the
    /// application repairs the contract. Successful placements are never replayed.
    pub fn retry_restore(&self) {
        self.0
            .borrow_mut()
            .restores
            .retain(|_, result| result.is_ok());
    }
}
pub(super) struct CaptureCommand {
    target: ElementId,
    description: Weak<()>,
    capture: TextEditingCapture,
}
impl Drop for CaptureCommand {
    fn drop(&mut self) {
        let mut data = self.capture.0.borrow_mut();
        if data.snapshot.is_none() {
            data.snapshot = Some(Err(TextEditingError::Disposed));
        }
    }
}
impl<T: View> Ui<T> {
    /// Nondestructively captures the last reconciled committed state. Reconcile
    /// external model changes first. Preedit is excluded; source composition stays
    /// active until normal removal/deactivation/cancellation. Pending edits fail.
    pub fn capture_text_editing(
        &self,
        id: ElementId,
    ) -> Result<TextEditingSnapshot, TextEditingError> {
        let node = self.nodes.get(&id).ok_or(TextEditingError::Disposed)?;
        if !matches!(node.element.kind, ElementKind::TextInput { .. }) {
            return Err(TextEditingError::Unbound);
        }
        let identity = node
            .element
            .text_editing
            .as_ref()
            .and_then(|p| p.identity.clone())
            .ok_or(TextEditingError::Unbound)?;
        let editor = node.editor.as_ref().ok_or(TextEditingError::Unbound)?;
        if editor.pending.is_some() {
            return Err(TextEditingError::Busy);
        }
        Ok(TextEditingSnapshot {
            identity,
            value: node.edit_value().to_owned(),
            selection: editor
                .composition
                .as_ref()
                .map_or(editor.selection, |c| c.replacement),
            history: editor.history.clone(),
            multiline: node.multiline(),
            masked: node.masked(),
        })
    }
    /// Restores only after validating identity, exact value and field mode.
    /// Failure leaves every destination field unchanged. Focus is a separate opt-in
    /// (`FocusHandle` or host focus); this never activates a native window.
    /// Successful direct restoration invalidates geometry; prepare before hit testing.
    pub fn restore_text_editing(
        &mut self,
        id: ElementId,
        snapshot: &TextEditingSnapshot,
    ) -> Result<(), TextEditingError> {
        let node = self.nodes.get(&id).ok_or(TextEditingError::Disposed)?;
        if !matches!(node.element.kind, ElementKind::TextInput { .. }) {
            return Err(TextEditingError::Unbound);
        }
        let identity = node
            .element
            .text_editing
            .as_ref()
            .and_then(|p| p.identity.as_ref())
            .ok_or(TextEditingError::Unbound)?;
        if *identity != snapshot.identity {
            return Err(TextEditingError::IdentityMismatch);
        }
        if node.edit_value() != snapshot.value {
            return Err(TextEditingError::ValueMismatch);
        }
        if node.multiline() != snapshot.multiline || node.masked() != snapshot.masked {
            return Err(TextEditingError::ModeMismatch);
        }
        let editor = node.editor.as_ref().ok_or(TextEditingError::Unbound)?;
        if editor.pending.is_some()
            || editor.composition.is_some()
            || editor.drag_selection.is_some()
            || self.captured_pointer() == Some(id)
        {
            return Err(TextEditingError::Busy);
        }
        let editor = self.nodes.get_mut(&id).unwrap().editor.as_mut().unwrap();
        editor.selection = snapshot.selection;
        editor.history = snapshot.history.clone();
        editor.break_group();
        editor.goal = None;
        editor.drag_selection = None;
        editor.drag_line = false;
        editor.scroll_x = 0.;
        editor.reveal = true;
        self.caret_visible = true;
        self.geometry_ready = false;
        Ok(())
    }
    pub(super) fn capture_editing_commands(&mut self) {
        let commands = std::mem::take(&mut *self.editing_commands.borrow_mut());
        for command in commands {
            let current = self
                .nodes
                .get(&command.target)
                .and_then(|node| node.element.text_editing.as_ref())
                .is_some_and(|props| command.description.ptr_eq(&Rc::downgrade(&props.life)));
            command.capture.0.borrow_mut().snapshot = Some(if current {
                self.capture_text_editing(command.target)
            } else {
                Err(TextEditingError::Disposed)
            });
        }
    }
    pub(super) fn publish_editing_bindings(&mut self) {
        // Rebuild after reconciliation, including retained nodes whose restore
        // property changed. Keep pending and attempted tickets indexed so source
        // readiness and explicit retries are observed without polling all editors.
        self.editing_restore_nodes.clear();
        for id in &self.editor_nodes {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            let Some(props) = node.element.text_editing.as_ref() else {
                continue;
            };
            if props.restore.is_some() {
                self.editing_restore_nodes.push(*id);
            }
            let Some(handle) = props.handle.as_ref() else {
                continue;
            };
            let Some(editor) = node.editor.as_ref() else {
                continue;
            };
            let mut bindings = handle.0.borrow_mut();
            bindings.retain(|b| {
                b.life.strong_count() != 0
                    && b.description.strong_count() != 0
                    && b.commands.strong_count() != 0
                    && b.target != *id
            });
            bindings.push(Binding {
                target: *id,
                life: Rc::downgrade(&editor.life),
                description: Rc::downgrade(&props.life),
                commands: Rc::downgrade(&self.editing_commands),
            });
        }
    }
    pub(super) fn editing_transfer_pending(&self) -> bool {
        !self.editing_commands.borrow().is_empty()
            || self.editing_restore_nodes.iter().any(|id| {
                self.nodes
                    .get(id)
                    .and_then(|n| n.element.text_editing.as_ref())
                    .and_then(|p| p.restore.as_ref())
                    .is_some_and(|capture| {
                        let data = capture.0.borrow();
                        data.snapshot.is_some() && !data.restores.contains_key(id)
                    })
            })
    }
    pub(super) fn restore_editing_requests(&mut self) {
        for index in 0..self.editing_restore_nodes.len() {
            let id = self.editing_restore_nodes[index];
            let Some(capture) = self
                .nodes
                .get(&id)
                .and_then(|n| n.element.text_editing.as_ref())
                .and_then(|p| p.restore.clone())
            else {
                continue;
            };
            let data = capture.0.borrow();
            if data.restores.contains_key(&id) {
                continue;
            }
            let Some(snapshot) = data.snapshot.clone() else {
                continue;
            };
            drop(data);
            let result = snapshot.and_then(|snapshot| self.restore_text_editing(id, &snapshot));
            capture.0.borrow_mut().restores.insert(id, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    struct Measure;
    impl TextMeasure for Measure {
        fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
            Ok([r.text.len() as f32 * 8., 20.])
        }
    }
    struct Session {
        value: String,
        identity: TextEditingIdentity,
        handle: TextEditingHandle,
        ticket: Option<TextEditingCapture>,
        tree: DockTree,
        show: bool,
        multiline: bool,
        policy: u8,
        cancels: usize,
    }
    impl View for Session {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            if !self.show {
                return column();
            }
            let field = if self.multiline {
                text_area(self.value.clone())
            } else {
                text_input(self.value.clone())
            };
            let mut field = field
                .key("field")
                .text_editing_identity(self.identity.clone())
                .text_editing_handle(self.handle.clone())
                .on_pointer_down(cx.listener(|_, e: &PointerInput, _| e.capture_pointer()))
                .on_pointer_cancel(cx.listener(|s, _, _| s.cancels += 1))
                .on_change(cx.listener(|s, e: &TextChangeEvent, _| {
                    if s.policy == 2 {
                        return;
                    }
                    s.value = if s.policy == 1 {
                        e.value.to_uppercase()
                    } else {
                        e.value.clone()
                    };
                    s.identity.version += 1;
                }));
            if let Some(ticket) = &self.ticket {
                field = field.restore_text_editing(ticket.clone());
            }
            column()
                .fill_width()
                .fill_height()
                .child(dock(&self.tree, |key| {
                    dock_panel(
                        "Editor",
                        if *key == Key::from("editor") {
                            field.clone()
                        } else {
                            label("Other")
                        },
                    )
                }))
        }
    }
    fn setup(r: &mut Runtime, multiline: bool) -> (Entity<Session>, Ui<Session>) {
        let root = r.update(|cx| {
            cx.new(|_| Session {
                value: "abc".into(),
                identity: TextEditingIdentity::new("doc", 0),
                handle: TextEditingHandle::new(),
                ticket: None,
                tree: DockTree::from_panels(["editor"]).unwrap(),
                show: true,
                multiline,
                policy: 0,
                cancels: 0,
            })
        });
        let mut ui = Ui::new(r, root.clone()).unwrap();
        prepare(&mut ui, r);
        let id = field(&ui);
        ui.focus(id);
        (root, ui)
    }
    fn prepare(ui: &mut Ui<Session>, r: &mut Runtime) {
        ui.prepare(r, [500., 300.], &mut Measure).unwrap();
    }
    fn field(ui: &Ui<Session>) -> ElementId {
        ui.editor_nodes
            .iter()
            .copied()
            .find(|id| ui.nodes.contains_key(id))
            .unwrap()
    }
    fn edit(ui: &mut Ui<Session>, r: &mut Runtime, e: TextInputEvent) {
        ui.text_input(r, e, &mut Measure).unwrap();
    }
    fn capture(r: &mut Runtime, root: &Entity<Session>) -> TextEditingCapture {
        r.update(|cx| root.read(cx).handle.capture().unwrap())
    }
    fn bytes(r: &mut Runtime, root: &Entity<Session>) -> String {
        r.update(|cx| root.read(cx).value.clone())
    }
    fn selection() -> TextSelection {
        TextSelection {
            anchor: TextPosition {
                byte_offset: 3,
                affinity: TextAffinity::Upstream,
            },
            focus: TextPosition {
                byte_offset: 1,
                affinity: TextAffinity::Downstream,
            },
        }
    }
    #[test]
    fn docking_remount_preserves_direction_affinity_and_bounded_history_without_focus() {
        for multiline in [false, true] {
            let mut r = Runtime::new();
            let (root, mut ui) = setup(&mut r, multiline);
            edit(&mut ui, &mut r, TextInputEvent::Paste(" xyz".into()));
            let old = field(&ui);
            ui.nodes
                .get_mut(&old)
                .unwrap()
                .editor
                .as_mut()
                .unwrap()
                .selection = selection();
            let saved = ui.capture_text_editing(old).unwrap();
            let ticket = capture(&mut r, &root);
            r.update(|cx| {
                root.update(cx, |s, _| {
                    s.ticket = Some(ticket.clone());
                    let group = s.tree.root().id();
                    s.tree
                        .split(
                            group,
                            DockSide::Right,
                            "other",
                            SplitPosition::Fraction(0.5),
                        )
                        .unwrap();
                })
            });
            prepare(&mut ui, &mut r);
            let new = field(&ui);
            assert_ne!(old, new);
            assert!(ui.text_input_info(old).is_none());
            assert_eq!(
                ui.capture_text_editing(old).unwrap_err(),
                TextEditingError::Disposed
            );
            assert_eq!(ticket.restore_result(new), Some(Ok(())));
            assert_eq!(ui.text_input_info(new).unwrap().selection, selection());
            assert_ne!(ui.focused_element(), Some(new));
            assert!(ui.text_input_info(new).unwrap().can_undo);
            // Snapshot remains reusable and source capture did not consume history.
            ui.restore_text_editing(new, &saved).unwrap();
            prepare(&mut ui, &mut r);
            ui.focus(new);
            edit(&mut ui, &mut r, TextInputEvent::Undo);
            assert_eq!(bytes(&mut r, &root), "abc");
            edit(&mut ui, &mut r, TextInputEvent::Redo);
            assert_eq!(bytes(&mut r, &root), "abc xyz");
        }
    }
    #[test]
    fn separate_uis_both_preparation_orders_pending_readiness_and_independent_sessions() {
        for destination_first in [false, true] {
            let mut r = Runtime::new();
            let (source_root, mut source) = setup(&mut r, true);
            let (destination_root, mut destination) = setup(&mut r, true);
            destination
                .set_theme(Theme::light().metrics(|m| m.font_size = 27.))
                .unwrap();
            edit(&mut source, &mut r, TextInputEvent::Paste("!".into()));
            let id = field(&source);
            source
                .nodes
                .get_mut(&id)
                .unwrap()
                .editor
                .as_mut()
                .unwrap()
                .selection = selection();
            let ticket = capture(&mut r, &source_root);
            r.update(|cx| {
                let s = source_root.read(cx);
                let (value, identity) = (s.value.clone(), s.identity.clone());
                drop(s);
                destination_root.update(cx, |d, _| {
                    d.value = value;
                    d.identity = identity;
                    d.ticket = Some(ticket.clone());
                });
                source_root.update(cx, |s, _| s.show = false);
            });
            if destination_first {
                prepare(&mut destination, &mut r);
                assert!(ticket.snapshot().is_none());
                assert!(
                    !destination.needs_prepare(&r).unwrap(),
                    "pending capture must not spin frames"
                );
                prepare(&mut source, &mut r);
                assert!(
                    destination.needs_prepare(&r).unwrap(),
                    "capture readiness must schedule destination work"
                );
            } else {
                prepare(&mut source, &mut r);
            }
            destination
                .prepare(&mut r, [300., 300.], &mut Measure)
                .unwrap();
            let new = field(&destination);
            assert_eq!(destination.nodes[&new].font_size, 27.);
            assert!(destination.nodes[&new].bounds.width < 400.);
            assert_eq!(ticket.restore_result(new), Some(Ok(())));
            assert_eq!(
                destination.text_input_info(new).unwrap().selection,
                selection()
            );
            assert!(!destination.needs_prepare(&r).unwrap());
        }
    }
    #[test]
    fn removed_restore_properties_and_nodes_stop_readiness_and_rebind() {
        for remove_node in [false, true] {
            let mut r = Runtime::new();
            let (source_root, mut source) = setup(&mut r, false);
            let (destination_root, mut destination) = setup(&mut r, false);
            assert!(!destination.needs_prepare(&r).unwrap());
            let ticket = capture(&mut r, &source_root);
            r.update(|cx| {
                destination_root.update(cx, |s, _| s.ticket = Some(ticket.clone()));
            });
            prepare(&mut destination, &mut r);
            let old = field(&destination);
            assert!(ticket.snapshot().is_none());
            assert!(!destination.needs_prepare(&r).unwrap());

            r.update(|cx| {
                destination_root.update(cx, |s, _| {
                    if remove_node {
                        s.show = false;
                    } else {
                        s.ticket = None;
                    }
                });
            });
            prepare(&mut destination, &mut r);
            prepare(&mut source, &mut r);
            assert!(ticket.snapshot().unwrap().is_ok());
            assert!(!destination.needs_prepare(&r).unwrap());
            ticket.retry_restore();
            assert!(!destination.needs_prepare(&r).unwrap());
            assert!(ticket.restore_results().is_empty());

            r.update(|cx| {
                destination_root.update(cx, |s, _| {
                    s.show = true;
                    s.ticket = Some(ticket.clone());
                });
            });
            prepare(&mut destination, &mut r);
            let new = field(&destination);
            assert_eq!(new == old, !remove_node);
            assert_eq!(ticket.restore_result(new), Some(Ok(())));
            assert!(!destination.needs_prepare(&r).unwrap());

            // Replace the ticket on a retained field; the new description must
            // restore even though this ElementId already handled another ticket.
            let mut saved = destination.capture_text_editing(new).unwrap();
            saved.selection = selection();
            let replacement = TextEditingCapture::from_snapshot(saved);
            r.update(|cx| {
                destination_root.update(cx, |s, _| s.ticket = Some(replacement.clone()));
            });
            prepare(&mut destination, &mut r);
            assert_eq!(field(&destination), new);
            assert_eq!(replacement.restore_result(new), Some(Ok(())));
            assert_eq!(
                destination.text_input_info(new).unwrap().selection,
                selection()
            );
            assert!(!destination.needs_prepare(&r).unwrap());
        }
    }
    #[test]
    fn validation_is_atomic_and_failed_ticket_can_retry_without_spinning() {
        let mut r = Runtime::new();
        let (root, mut ui) = setup(&mut r, false);
        edit(&mut ui, &mut r, TextInputEvent::Paste("!".into()));
        let id = field(&ui);
        let saved = ui.capture_text_editing(id).unwrap();
        let selection_before = ui.text_input_info(id).unwrap().selection;
        for error in [
            TextEditingError::IdentityMismatch,
            TextEditingError::ValueMismatch,
            TextEditingError::ModeMismatch,
        ] {
            let mut incompatible = saved.clone();
            match error {
                TextEditingError::IdentityMismatch => incompatible.identity.version += 1,
                TextEditingError::ValueMismatch => incompatible.value.push('x'),
                _ => incompatible.multiline = true,
            }
            assert_eq!(ui.restore_text_editing(id, &incompatible), Err(error));
            assert_eq!(ui.text_input_info(id).unwrap().selection, selection_before);
            assert!(ui.text_input_info(id).unwrap().can_undo);
        }
        let ticket = TextEditingCapture::from_snapshot(saved.clone());
        r.update(|cx| {
            root.update(cx, |s, _| {
                s.identity.version += 1;
                s.ticket = Some(ticket.clone());
            })
        });
        prepare(&mut ui, &mut r);
        assert_eq!(
            ticket.restore_result(id),
            Some(Err(TextEditingError::IdentityMismatch))
        );
        assert!(!ui.needs_prepare(&r).unwrap());
        assert!(ticket.snapshot().unwrap().is_ok());
        r.update(|cx| root.update(cx, |s, _| s.identity = saved.identity.clone()));
        prepare(&mut ui, &mut r);
        assert!(!ui.needs_prepare(&r).unwrap());
        ticket.retry_restore();
        assert!(ui.needs_prepare(&r).unwrap());
        prepare(&mut ui, &mut r);
        assert_eq!(ticket.restore_results(), vec![(id, Ok(()))]);
        assert!(!ui.needs_prepare(&r).unwrap());
        ui.focus(id);
        edit(&mut ui, &mut r, TextInputEvent::Undo);
        assert_eq!(bytes(&mut r, &root), "abc");
    }
    #[test]
    fn capture_excludes_preedit_without_cancelling_and_busy_restore_is_atomic() {
        let mut r = Runtime::new();
        let (root, mut ui) = setup(&mut r, true);
        let id = field(&ui);
        ui.nodes
            .get_mut(&id)
            .unwrap()
            .editor
            .as_mut()
            .unwrap()
            .selection = selection();
        edit(
            &mut ui,
            &mut r,
            TextInputEvent::Preedit {
                text: "候補".into(),
                cursor: Some((0, 3)),
            },
        );
        let before = ui.ime_reset_revision();
        let ticket = capture(&mut r, &root);
        prepare(&mut ui, &mut r);
        let saved = ticket.snapshot().unwrap().unwrap();
        assert_eq!(saved.value, "abc");
        assert_eq!(saved.selection, selection());
        assert!(ui.nodes[&id].editor.as_ref().unwrap().composition.is_some());
        assert_eq!(ui.ime_reset_revision(), before);
        assert_eq!(
            ui.restore_text_editing(id, &saved),
            Err(TextEditingError::Busy)
        );
        assert_eq!(ui.text_input_info(id).unwrap().selection, selection());
        edit(&mut ui, &mut r, TextInputEvent::CancelComposition);
        assert_eq!(ui.ime_reset_revision(), before + 1);
        edit(&mut ui, &mut r, TextInputEvent::CancelComposition);
        assert_eq!(ui.ime_reset_revision(), before + 1);
        ui.restore_text_editing(id, &saved).unwrap();
        let editor = ui.nodes.get_mut(&id).unwrap().editor.as_mut().unwrap();
        editor.pending = Some(Pending {
            proposed: "unaccepted".into(),
            previous: selection(),
            action: EditAction::Edit(EditKind::Separate),
        });
        assert_eq!(
            ui.capture_text_editing(id).unwrap_err(),
            TextEditingError::Busy
        );
        assert_eq!(
            ui.restore_text_editing(id, &saved),
            Err(TextEditingError::Busy)
        );
    }
    #[test]
    fn queued_unsettled_capture_failure_is_observable() {
        let mut r = Runtime::new();
        let (root, mut ui) = setup(&mut r, true);
        let id = field(&ui);
        ui.nodes
            .get_mut(&id)
            .unwrap()
            .editor
            .as_mut()
            .unwrap()
            .pending = Some(Pending {
            proposed: "unaccepted".into(),
            previous: selection(),
            action: EditAction::Edit(EditKind::Separate),
        });
        let ticket = capture(&mut r, &root);
        ui.capture_editing_commands();
        assert_eq!(
            ticket.snapshot().unwrap().unwrap_err(),
            TextEditingError::Busy
        );
    }
    #[test]
    fn duplicate_sessions_are_ambiguous_old_handles_stay_stale_and_source_drop_resolves_ticket() {
        let mut r = Runtime::new();
        let (root, mut first) = setup(&mut r, false);
        let mut second = Ui::new(&mut r, root.clone()).unwrap();
        prepare(&mut second, &mut r);
        let handle = r.update(|cx| root.read(cx).handle.clone());
        assert_eq!(handle.capture().unwrap_err(), TextEditingError::Ambiguous);
        drop(second);
        let ticket = handle.capture().unwrap();
        drop(first);
        assert_eq!(
            ticket.snapshot().unwrap().unwrap_err(),
            TextEditingError::Disposed
        );
        assert_eq!(handle.capture().unwrap_err(), TextEditingError::Disposed);
        first = Ui::new(&mut r, root.clone()).unwrap();
        prepare(&mut first, &mut r);
        let old = field(&first);
        let old_handle = handle.clone();
        let _retained_description = first.nodes[&old].element.clone();
        r.update(|cx| root.update(cx, |s, _| s.handle = TextEditingHandle::new()));
        prepare(&mut first, &mut r);
        assert_eq!(
            old_handle.capture().unwrap_err(),
            TextEditingError::Disposed
        );
        assert!(first.capture_text_editing(old).is_ok());
    }
    #[test]
    fn restored_undo_keeps_controlled_rejection_and_normalization_semantics() {
        let mut r = Runtime::new();
        let (root, mut ui) = setup(&mut r, false);
        edit(&mut ui, &mut r, TextInputEvent::Paste("x".into()));
        let id = field(&ui);
        let saved = ui.capture_text_editing(id).unwrap();
        ui.restore_text_editing(id, &saved).unwrap();
        r.update(|cx| root.update(cx, |s, _| s.policy = 2));
        edit(&mut ui, &mut r, TextInputEvent::Undo);
        assert_eq!(bytes(&mut r, &root), "abcx");
        assert!(ui.text_input_info(id).unwrap().can_undo);
        assert!(!ui.text_input_info(id).unwrap().can_redo);
        r.update(|cx| root.update(cx, |s, _| s.policy = 1));
        edit(&mut ui, &mut r, TextInputEvent::Undo);
        assert_eq!(bytes(&mut r, &root), "ABC");
        assert!(!ui.text_input_info(id).unwrap().can_undo);
        assert!(!ui.text_input_info(id).unwrap().can_redo);
    }
    #[test]
    fn snapshot_preserves_both_history_stacks_and_existing_history_budget() {
        let mut r = Runtime::new();
        let (root, mut ui) = setup(&mut r, false);
        for _ in 0..140 {
            edit(&mut ui, &mut r, TextInputEvent::Paste("x".into()));
        }
        let id = field(&ui);
        edit(&mut ui, &mut r, TextInputEvent::Undo);
        let saved = ui.capture_text_editing(id).unwrap();
        ui.restore_text_editing(id, &saved).unwrap();
        assert!(ui.text_input_info(id).unwrap().can_undo);
        assert!(ui.text_input_info(id).unwrap().can_redo);
        edit(&mut ui, &mut r, TextInputEvent::Redo);
        let mut steps = 0;
        while ui.text_input_info(id).unwrap().can_undo {
            edit(&mut ui, &mut r, TextInputEvent::Undo);
            steps += 1;
        }
        assert_eq!(steps, 128);
        assert_eq!(bytes(&mut r, &root), format!("abc{}", "x".repeat(12)));
    }
    #[test]
    fn remount_cancels_source_capture_once_and_does_not_transfer_preedit() {
        let mut r = Runtime::new();
        let (root, mut ui) = setup(&mut r, true);
        let old = field(&ui);
        let bounds = ui.nodes[&old].bounds;
        ui.pointer(
            &mut r,
            PointerEvent::Pressed([bounds.x + 2., bounds.y + 2.]),
        )
        .unwrap();
        assert_eq!(ui.captured_pointer(), Some(old));
        edit(
            &mut ui,
            &mut r,
            TextInputEvent::Preedit {
                text: "候補".into(),
                cursor: None,
            },
        );
        let ticket = capture(&mut r, &root);
        r.update(|cx| {
            root.update(cx, |s, _| {
                s.ticket = Some(ticket.clone());
                let group = s.tree.root().id();
                s.tree
                    .split(
                        group,
                        DockSide::Right,
                        "other",
                        SplitPosition::Fraction(0.5),
                    )
                    .unwrap();
            })
        });
        prepare(&mut ui, &mut r);
        let new = field(&ui);
        assert_ne!(old, new);
        assert_eq!(ticket.restore_result(new), Some(Ok(())));
        assert!(
            ui.nodes[&new]
                .editor
                .as_ref()
                .unwrap()
                .composition
                .is_none()
        );
        assert_eq!(ui.captured_pointer(), None);
        assert_eq!(bytes(&mut r, &root), "abc");
        prepare(&mut ui, &mut r);
        r.update(|cx| assert_eq!(root.read(cx).cancels, 1));
    }
    #[test]
    fn equal_bytes_new_document_resets_session_but_version_alone_preserves_history() {
        let mut r = Runtime::new();
        let (root, mut ui) = setup(&mut r, false);
        edit(&mut ui, &mut r, TextInputEvent::Paste("x".into()));
        let id = field(&ui);
        ui.nodes
            .get_mut(&id)
            .unwrap()
            .editor
            .as_mut()
            .unwrap()
            .selection = selection();
        let saved = ui.capture_text_editing(id).unwrap();
        r.update(|cx| root.update(cx, |s, _| s.identity.document = "other-document".into()));
        prepare(&mut ui, &mut r);
        assert!(!ui.text_input_info(id).unwrap().can_undo);
        assert_eq!(
            ui.text_input_info(id).unwrap().selection,
            TextSelection::caret(4)
        );
        assert_eq!(
            ui.restore_text_editing(id, &saved),
            Err(TextEditingError::IdentityMismatch)
        );
        edit(&mut ui, &mut r, TextInputEvent::Paste("y".into()));
        assert!(ui.text_input_info(id).unwrap().can_undo);
        r.update(|cx| root.update(cx, |s, _| s.identity.version += 1));
        prepare(&mut ui, &mut r);
        assert!(ui.text_input_info(id).unwrap().can_undo);
    }
}
