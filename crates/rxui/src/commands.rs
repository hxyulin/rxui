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
    /// Allows native key-repeat invocations (default false). Matching repeat events
    /// are consumed even when repetition is disabled.
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
    if !action.enabled {
        return Ok(CommandStatus::Disabled);
    }
    Ok(match (action.callback)(cx)? {
        Dispatch::Handled => CommandStatus::Handled,
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
        CommandAction::new(move |cx| listener.dispatch(&command, cx))
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
