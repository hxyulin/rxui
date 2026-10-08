//! Typed command actions, scoped routing and explicit application registrations.
//!
//! ```
//! use rxui::prelude::*;
//! struct Save;
//! impl Command for Save {}
//! struct Editor { saves: usize, can_save: bool }
//! impl View for Editor {
//!     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
//!         let save = cx.command(Save, |s, _, _| s.saves += 1)
//!             .label("Save").enabled(self.can_save).shortcut(Shortcut::primary("s"));
//!         column().on_command(save.clone()).child(save.button())
//!     }
//! }
//! ```
use crate::*;
use std::{any::TypeId, cell::RefCell, fmt, marker::PhantomData, rc::Rc};

/// Application-defined command identity/payload. Implement this marker for each
/// distinct action type. Payloads need not implement Clone or Default.
pub trait Command: 'static {}

/// Erased command type identity. It stores no payload, callback or component.
/// Resolve it afresh at invocation, so current focus and state choose the handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CommandId(pub(crate) TypeId);
impl CommandId {
    /// Identifies an application or standard command type.
    pub fn of<C: Command>() -> Self {
        Self(TypeId::of::<C>())
    }
}
/// Snapshot of the nearest live command's public presentation properties.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandInfo {
    /// Human-readable caption from the currently resolved action.
    pub label: String,
    /// Whether invocation is currently permitted. Disabled handlers shadow parents.
    pub enabled: bool,
    /// Exact logical shortcuts in preference order.
    pub shortcuts: Vec<Shortcut>,
}
/// Standard desktop requests. Native hosts supply editing/lifecycle fallbacks;
/// scoped application handlers can override any of these command types.
pub mod standard_commands {
    use super::Command;
    /// Requests undo in the focused input; applications may override for document history.
    pub struct Undo;
    impl Command for Undo {}
    /// Requests redo in the focused input.
    pub struct Redo;
    impl Command for Redo {}
    /// Copies the current text selection, including from read-only inputs.
    pub struct Copy;
    impl Command for Copy {}
    /// Copies and removes the current editable text selection.
    pub struct Cut;
    impl Command for Cut {}
    /// Inserts clipboard text into the focused editable input.
    pub struct Paste;
    impl Command for Paste {}
    /// Selects the focused input's complete value.
    pub struct SelectAll;
    impl Command for SelectAll {}
    /// Requests closing the source window, honoring Application::close_requested.
    pub struct CloseWindow;
    impl Command for CloseWindow {}
    /// Requests application shutdown, honoring Application::quit_requested.
    pub struct Quit;
    impl Command for Quit {}
}
/// One exact logical key chord. Primary means Command on macOS and Control elsewhere.
/// Character spelling matches ASCII case-insensitively; modifiers match exactly.
#[derive(Clone, Debug)]
pub struct Shortcut {
    /// Logical key, independent of text insertion and IME composition.
    pub key: KeyboardKey,
    /// Required modifier snapshot; additional modifiers do not match.
    pub modifiers: Modifiers,
}
impl PartialEq for Shortcut {
    fn eq(&self, other: &Self) -> bool {
        self.modifiers == other.modifiers && same_key(&self.key, &other.key)
    }
}
impl Eq for Shortcut {}
impl Shortcut {
    /// Creates an exact key chord.
    pub fn new(key: KeyboardKey, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }
    /// Creates a conventional primary-modifier character chord.
    pub fn primary(character: impl Into<String>) -> Self {
        Self::new(
            KeyboardKey::Character(character.into()),
            Modifiers {
                meta: cfg!(target_os = "macos"),
                control: !cfg!(target_os = "macos"),
                ..Default::default()
            },
        )
    }
    /// Adds Shift to this chord.
    pub fn shift(mut self) -> Self {
        self.modifiers.shift = true;
        self
    }
    /// Adds Alt/Option to this chord.
    pub fn alt(mut self) -> Self {
        self.modifiers.alt = true;
        self
    }
    pub(crate) fn matches(&self, event: &KeyEvent) -> bool {
        self.modifiers == event.modifiers && same_key(&self.key, &event.key)
    }
}
fn same_key(a: &KeyboardKey, b: &KeyboardKey) -> bool {
    match (a, b) {
        (KeyboardKey::Character(a), KeyboardKey::Character(b)) => a.eq_ignore_ascii_case(b),
        (a, b) => a == b,
    }
}
type Callback = dyn Fn(&mut AppContext<'_>) -> Result<Dispatch, AccessError>;
#[derive(Clone)]
pub(crate) struct Action {
    pub allow_in_modal: bool,
    pub kind: TypeId,
    pub label: String,
    pub enabled: bool,
    pub repeat: bool,
    pub shortcuts: Vec<Shortcut>,
    pub callback: Rc<Callback>,
    pub live: Rc<dyn Fn() -> bool>,
}
/// Shared description of one typed action. Clone it into a scope, button or menu
/// item to share callback, label, enabled state and shortcut definitions. Properties
/// are immutable description snapshots; reevaluation supplies updated state.
pub struct CommandAction<C: Command> {
    pub(crate) action: Action,
    marker: PhantomData<fn() -> C>,
}
impl<C: Command> Clone for CommandAction<C> {
    fn clone(&self) -> Self {
        Self {
            action: self.action.clone(),
            marker: PhantomData,
        }
    }
}
impl<C: Command> fmt::Debug for CommandAction<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandAction")
            .field("label", &self.action.label)
            .field("enabled", &self.action.enabled)
            .field("shortcuts", &self.action.shortcuts)
            .finish_non_exhaustive()
    }
}
impl<C: Command> CommandAction<C> {
    fn new(
        callback: impl Fn(&mut AppContext<'_>) -> Result<Dispatch, AccessError> + 'static,
    ) -> Self {
        Self {
            action: Action {
                allow_in_modal: false,
                kind: TypeId::of::<C>(),
                label: std::any::type_name::<C>().into(),
                enabled: true,
                repeat: false,
                shortcuts: Vec::new(),
                callback: Rc::new(callback),
                live: Rc::new(|| true),
            },
            marker: PhantomData,
        }
    }
    /// Human-readable caption used by command buttons and menu items.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.action.label = label.into();
        self
    }
    /// Controlled availability. A disabled scoped action shadows outer actions of
    /// the same type/chord rather than accidentally invoking a different owner.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.action.enabled = enabled;
        self
    }
    /// Allows this action in outer/application scopes while a modal is open.
    /// Default false prevents background shortcuts from acting through a dialog;
    /// opt in explicitly for global actions such as application Help or Quit.
    pub fn allow_in_modal(mut self, allow: bool) -> Self {
        self.action.allow_in_modal = allow;
        self
    }
    /// Adds an exact chord. Register this action on a scope to activate shortcuts.
    pub fn shortcut(mut self, shortcut: Shortcut) -> Self {
        self.action.shortcuts.push(shortcut);
        self
    }
    /// Allows repeated keyboard invocations in RXUI's routed input (default false).
    /// Matching repeats are consumed even when repetition is disabled. Native menu
    /// accelerators use the platform menu system's repetition policy.
    pub fn repeat(mut self, repeat: bool) -> Self {
        self.action.repeat = repeat;
        self
    }
    /// Current description's enabled state.
    pub fn is_enabled(&self) -> bool {
        self.action.enabled
    }
    /// Current human-readable caption.
    pub fn caption(&self) -> &str {
        &self.action.label
    }
    /// Builds a button targeting this specific action, independent of focus routing.
    /// Its listener retains the action's weak mount/owner binding.
    pub fn button(&self) -> Element {
        let action = self.action.clone();
        button(action.label.clone())
            .disabled(!action.enabled)
            .on_click(Listener::from_callback(move |_: &ClickEvent, cx| {
                Ok(match invoke(&action, cx)? {
                    CommandStatus::Handled => Dispatch::Handled,
                    CommandStatus::Unhandled | CommandStatus::Disabled => Dispatch::TargetGone,
                })
            }))
    }
    /// Invokes this specific action inside an update scope. Disabled actions never
    /// run; wrong-runtime/reentrant accesses use the ordinary listener errors.
    pub fn invoke(&self, cx: &mut AppContext<'_>) -> Result<CommandStatus, AccessError> {
        invoke(&self.action, cx)
    }
}
/// Result of resolving and invoking a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandStatus {
    /// A live callback ran.
    Handled,
    /// No live handler was found.
    Unhandled,
    /// The nearest matching action is disabled; outer handlers are shadowed.
    Disabled,
}
pub(crate) fn invoke(
    action: &Action,
    cx: &mut AppContext<'_>,
) -> Result<CommandStatus, AccessError> {
    if !(action.live)() {
        return Ok(CommandStatus::Unhandled);
    }
    if !action.enabled {
        return Ok(CommandStatus::Disabled);
    }
    Ok(match (action.callback)(cx)? {
        Dispatch::Handled | Dispatch::Unchanged => CommandStatus::Handled,
        Dispatch::TargetGone => CommandStatus::Unhandled,
    })
}
impl<T> ViewContext<'_, T> {
    /// Creates an action weakly bound to this mounted component's live state. Attach
    /// with Element::on_command for scoped shortcut/type routing, or use button().
    pub fn command<C: Command>(
        &self,
        command: C,
        callback: impl Fn(&mut T, &C, &mut Context<'_, T>) + 'static,
    ) -> CommandAction<C> {
        let listener = self.listener(callback);
        let runtime = Rc::downgrade(self.runtime);
        let mount = self.placement_scope().expect("view placement");
        let mut action = CommandAction::new(move |cx| listener.dispatch(&command, cx));
        action.action.live =
            Rc::new(move || runtime.upgrade().is_some_and(|r| r.mount_live(mount)));
        action
    }
}
pub(crate) struct Registration {
    pub action: RefCell<Action>,
    runtime: u64,
}
/// Retained application fallback registration. The runtime stores a weak reference;
/// dropping the last clone unregisters it. Later registrations override earlier
/// ones, and source-window scopes always take precedence.
#[derive(Clone)]
pub struct CommandRegistration(Rc<Registration>);
impl fmt::Debug for CommandRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandRegistration")
            .finish_non_exhaustive()
    }
}
impl CommandRegistration {
    /// Replaces the captured action snapshot in an update scope belonging to the
    /// registration's runtime.
    pub fn replace<C: Command>(
        &self,
        action: &CommandAction<C>,
        cx: &mut AppContext<'_>,
    ) -> Result<(), AccessError> {
        if cx.runtime.id != self.0.runtime {
            return Err(AccessError::WrongRuntime);
        }
        *self.0.action.borrow_mut() = action.action.clone();
        Ok(())
    }
}
impl AppContext<'_> {
    /// Creates an application action. Capture weak entities/services where needed;
    /// the callback executes on the UI thread inside a fresh update scope.
    pub fn command<C: Command>(
        &self,
        command: C,
        callback: impl Fn(&C, &mut AppContext<'_>) + 'static,
    ) -> CommandAction<C> {
        let runtime = self.runtime.id;
        CommandAction::new(move |cx| {
            if cx.runtime.id != runtime {
                return Err(AccessError::WrongRuntime);
            }
            callback(&command, cx);
            Ok(Dispatch::Handled)
        })
    }
    /// Registers a retained application fallback. Keep the returned handle alive;
    /// refresh its snapshot explicitly if application availability changes.
    pub fn register_command<C: Command>(
        &mut self,
        action: &CommandAction<C>,
    ) -> CommandRegistration {
        let registration = Rc::new(Registration {
            action: RefCell::new(action.action.clone()),
            runtime: self.runtime.id,
        });
        let mut commands = self.runtime.commands.borrow_mut();
        commands.retain(|c| c.strong_count() != 0);
        commands.push(Rc::downgrade(&registration));
        CommandRegistration(registration)
    }
}
impl Element {
    /// Installs one typed action in this element's scope. Focused descendant scopes
    /// resolve before ancestors; duplicate action types/chords in a scope are errors.
    pub fn on_command<C: Command>(mut self, action: CommandAction<C>) -> Self {
        self.input
            .get_or_insert_with(Default::default)
            .commands
            .push(action.action);
        self
    }
}

impl Action {
    pub(crate) fn info(&self) -> CommandInfo {
        CommandInfo {
            label: self.label.clone(),
            enabled: self.enabled,
            shortcuts: self.shortcuts.clone(),
        }
    }
}
impl AppContext<'_> {
    /// Queries the latest live application fallback without selecting a window.
    pub fn query_command<C: Command>(&self) -> Option<CommandInfo> {
        self.application_command(CommandId::of::<C>())
            .map(|a| a.info())
    }
    /// Invokes an application registration without selecting a window. View scopes
    /// are resolved by Ui::dispatch_command or the native host instead.
    pub fn dispatch_command<C: Command>(&mut self) -> Result<CommandStatus, AccessError> {
        self.dispatch_application_command(CommandId::of::<C>())
    }
    pub(crate) fn application_command(&self, id: CommandId) -> Option<Action> {
        self.runtime
            .commands
            .borrow()
            .iter()
            .rev()
            .filter_map(|r| r.upgrade())
            .find_map(|r| {
                let a = r.action.borrow();
                (a.kind == id.0 && (a.live)()).then(|| a.clone())
            })
    }
    pub(crate) fn dispatch_application_command(
        &mut self,
        id: CommandId,
    ) -> Result<CommandStatus, AccessError> {
        let Some(a) = self.application_command(id) else {
            return Ok(CommandStatus::Unhandled);
        };
        invoke(&a, self)
    }
}

#[cfg(feature = "native")]
#[derive(Default)]
pub(crate) struct StandardEditing {
    pub text: bool,
    pub editable: bool,
    pub selection: bool,
    pub undo: bool,
    pub redo: bool,
}

#[cfg(feature = "native")]
pub(crate) fn standard_info(
    id: CommandId,
    window: bool,
    editing: StandardEditing,
    modal: bool,
) -> Option<CommandInfo> {
    use standard_commands::*;
    let StandardEditing {
        text,
        editable,
        selection,
        undo,
        redo,
    } = editing;
    let (label, enabled, key) = if id == CommandId::of::<Undo>() {
        ("Undo", editable && undo, "z")
    } else if id == CommandId::of::<Redo>() {
        ("Redo", editable && redo, "z")
    } else if id == CommandId::of::<Copy>() {
        ("Copy", text && selection, "c")
    } else if id == CommandId::of::<Cut>() {
        ("Cut", editable && selection, "x")
    } else if id == CommandId::of::<Paste>() {
        ("Paste", editable, "v")
    } else if id == CommandId::of::<SelectAll>() {
        ("Select All", text, "a")
    } else if id == CommandId::of::<CloseWindow>() {
        ("Close Window", window && !modal, "w")
    } else if id == CommandId::of::<Quit>() {
        ("Quit", true, "q")
    } else {
        return None;
    };
    Some(CommandInfo {
        label: label.into(),
        enabled,
        shortcuts: if id == CommandId::of::<Redo>() {
            if cfg!(target_os = "macos") {
                vec![Shortcut::primary("z").shift()]
            } else {
                vec![Shortcut::primary("y"), Shortcut::primary("z").shift()]
            }
        } else {
            vec![Shortcut::primary(key)]
        },
    })
}
