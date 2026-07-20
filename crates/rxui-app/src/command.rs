//! Typed commands shared by menus, toolbars, shortcuts, and palettes.

use std::{collections::BTreeMap, error::Error, fmt};

use astrelis_platform::WindowEvent;
use astrelis_platform::{ElementState, Key, KeyboardInput, Modifiers};

/// Stable application-defined command identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandId(String);

impl CommandId {
    /// Creates an identifier such as `file.save` or `editor.find`.
    pub fn new(value: impl Into<String>) -> Result<Self, CommandError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(CommandError::new("command identifiers cannot be empty"));
        }
        if value.chars().any(char::is_whitespace) {
            return Err(CommandError::new(
                "command identifiers cannot contain whitespace",
            ));
        }
        Ok(Self(value))
    }

    /// Returns the stable string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A logical key plus the modifiers required to invoke a command.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    /// Logical key matched by the shortcut.
    pub key: Key,
    /// Exact modifier set required by the shortcut.
    pub modifiers: Modifiers,
}

impl Shortcut {
    /// Creates a shortcut with an explicit modifier set.
    pub const fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    /// Creates a conventional primary-modifier shortcut.
    ///
    /// The primary modifier is Command on macOS and Control elsewhere.
    pub fn primary(key: impl Into<String>) -> Self {
        Self {
            key: Key::Character(key.into()),
            modifiers: Modifiers {
                super_key: cfg!(target_os = "macos"),
                control: !cfg!(target_os = "macos"),
                ..Default::default()
            },
        }
    }

    /// Returns whether a pressed keyboard event matches this shortcut.
    pub fn matches(&self, input: &KeyboardInput, modifiers: Modifiers) -> bool {
        input.state == ElementState::Pressed
            && self.modifiers == modifiers
            && keys_equal(&self.key, &input.logical_key)
    }

    /// Formats this shortcut using the current platform's conventional names.
    pub fn display_label(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.control {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.alt {
            parts.push(
                if cfg!(target_os = "macos") {
                    "Option"
                } else {
                    "Alt"
                }
                .to_owned(),
            );
        }
        if self.modifiers.shift {
            parts.push("Shift".to_owned());
        }
        if self.modifiers.super_key {
            parts.push(
                if cfg!(target_os = "macos") {
                    "Command"
                } else {
                    "Super"
                }
                .to_owned(),
            );
        }
        parts.push(key_label(&self.key));
        parts.join("+")
    }
}

fn key_label(key: &Key) -> String {
    match key {
        Key::Character(value) => value.to_uppercase(),
        Key::Named(value) => format!("{value:?}"),
        Key::Native(value) => format!("{value:?}"),
        Key::Unidentified => "Unidentified".into(),
        _ => "Unknown".into(),
    }
}

fn keys_equal(expected: &Key, actual: &Key) -> bool {
    match (expected, actual) {
        (Key::Character(expected), Key::Character(actual)) => {
            expected.to_lowercase() == actual.to_lowercase()
        }
        _ => expected == actual,
    }
}

/// One user-visible application command.
#[derive(Clone, Debug, PartialEq)]
pub struct Command<Message> {
    /// Stable command identity.
    pub id: CommandId,
    /// Concise user-visible label.
    pub label: String,
    /// Optional longer explanation for palettes and tooltips.
    pub description: Option<String>,
    /// Optional keyboard shortcut.
    pub shortcut: Option<Shortcut>,
    /// Whether the command may currently be invoked.
    pub enabled: bool,
    /// Optional toggle state displayed by command surfaces.
    pub checked: Option<bool>,
    /// Typed application message emitted on invocation.
    pub message: Message,
}

impl<Message> Command<Message> {
    /// Creates an enabled command without a shortcut.
    pub fn new(id: CommandId, label: impl Into<String>, message: Message) -> Self {
        Self {
            id,
            label: label.into(),
            description: None,
            shortcut: None,
            enabled: true,
            checked: None,
            message,
        }
    }

    /// Adds descriptive text.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Assigns a shortcut.
    pub fn shortcut(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Sets initial enablement.
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Makes the command checkable and sets its initial state.
    pub const fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }
}

/// Ordered registry of application commands.
#[derive(Clone, Debug, Default)]
pub struct CommandRegistry<Message> {
    commands: BTreeMap<CommandId, Command<Message>>,
}

impl<Message> CommandRegistry<Message> {
    /// Creates an empty registry.
    pub const fn new() -> Self {
        Self {
            commands: BTreeMap::new(),
        }
    }

    /// Registers a command, rejecting duplicate identities and shortcuts.
    pub fn register(&mut self, command: Command<Message>) -> Result<(), CommandError> {
        if self.commands.contains_key(&command.id) {
            return Err(CommandError::new(format!(
                "command `{}` is already registered",
                command.id
            )));
        }
        if let Some(shortcut) = &command.shortcut
            && self
                .commands
                .values()
                .any(|existing| existing.shortcut.as_ref() == Some(shortcut))
        {
            return Err(CommandError::new("command shortcut is already registered"));
        }
        self.commands.insert(command.id.clone(), command);
        Ok(())
    }

    /// Removes and returns a command.
    pub fn remove(&mut self, id: &CommandId) -> Option<Command<Message>> {
        self.commands.remove(id)
    }

    /// Returns a command by identity.
    pub fn get(&self, id: &CommandId) -> Option<&Command<Message>> {
        self.commands.get(id)
    }

    /// Returns a mutable command for state updates.
    pub fn get_mut(&mut self, id: &CommandId) -> Option<&mut Command<Message>> {
        self.commands.get_mut(id)
    }

    /// Iterates in stable identifier order.
    pub fn iter(&self) -> impl Iterator<Item = &Command<Message>> {
        self.commands.values()
    }

    /// Returns the number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns whether no commands are registered.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

impl<Message: Clone> CommandRegistry<Message> {
    /// Invokes an enabled command and returns its typed message.
    pub fn invoke(&self, id: &CommandId) -> Option<Message> {
        self.get(id)
            .filter(|command| command.enabled)
            .map(|command| command.message.clone())
    }

    /// Invokes the enabled command bound to a keyboard event.
    pub fn invoke_shortcut(&self, input: &KeyboardInput, modifiers: Modifiers) -> Option<Message> {
        self.commands
            .values()
            .find(|command| {
                command.enabled
                    && command
                        .shortcut
                        .as_ref()
                        .is_some_and(|shortcut| shortcut.matches(input, modifiers))
            })
            .map(|command| command.message.clone())
    }
}

/// Tracks modifier state and dispatches command shortcuts from window events.
#[derive(Clone, Copy, Debug, Default)]
pub struct CommandRouter {
    modifiers: Modifiers,
}

impl CommandRouter {
    /// Creates a router with no modifiers held.
    pub const fn new() -> Self {
        Self {
            modifiers: Modifiers {
                shift: false,
                control: false,
                alt: false,
                super_key: false,
            },
        }
    }

    /// Returns the most recently observed modifier state.
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Updates modifier state and invokes a matching enabled command.
    pub fn handle_event<Message: Clone>(
        &mut self,
        event: &WindowEvent,
        commands: &CommandRegistry<Message>,
    ) -> Option<Message> {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
                None
            }
            WindowEvent::Focused(false) => {
                self.modifiers = Modifiers::default();
                None
            }
            WindowEvent::KeyboardInput(input) if !input.repeat => {
                commands.invoke_shortcut(input, self.modifiers)
            }
            _ => None,
        }
    }
}

/// Invalid command registration or identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandError(String);

impl CommandError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CommandError {}

#[cfg(test)]
mod tests {
    use astrelis_platform::{DeviceId, KeyLocation, NamedKey, PhysicalKey};

    use super::*;

    fn key(value: &str) -> KeyboardInput {
        KeyboardInput {
            device_id: DeviceId(1),
            physical_key: PhysicalKey::Unidentified,
            logical_key: Key::Character(value.into()),
            text: Some(value.into()),
            location: KeyLocation::Standard,
            state: ElementState::Pressed,
            repeat: false,
            synthetic: false,
        }
    }

    #[test]
    fn disabled_commands_do_not_invoke() {
        let id = CommandId::new("file.save").unwrap();
        let mut commands = CommandRegistry::new();
        commands
            .register(Command::new(id.clone(), "Save", 7).enabled(false))
            .unwrap();
        assert_eq!(commands.invoke(&id), None);
    }

    #[test]
    fn shortcuts_are_case_insensitive_and_modifier_exact() {
        let modifiers = Modifiers {
            control: true,
            ..Default::default()
        };
        let mut commands = CommandRegistry::new();
        commands
            .register(
                Command::new(CommandId::new("file.save").unwrap(), "Save", 7)
                    .shortcut(Shortcut::new(Key::Character("s".into()), modifiers)),
            )
            .unwrap();
        assert_eq!(commands.invoke_shortcut(&key("S"), modifiers), Some(7));
        assert_eq!(
            commands.invoke_shortcut(&key("S"), Modifiers::default()),
            None
        );
    }

    #[test]
    fn named_keys_can_be_bound() {
        let shortcut = Shortcut::new(Key::Named(NamedKey::Escape), Modifiers::default());
        assert_eq!(shortcut.key, Key::Named(NamedKey::Escape));
    }

    #[test]
    fn router_tracks_modifiers_and_ignores_repeats() {
        let modifiers = Modifiers {
            control: true,
            ..Default::default()
        };
        let mut commands = CommandRegistry::new();
        commands
            .register(
                Command::new(CommandId::new("file.save").unwrap(), "Save", 9)
                    .shortcut(Shortcut::new(Key::Character("s".into()), modifiers)),
            )
            .unwrap();
        let mut router = CommandRouter::new();
        assert_eq!(
            router.handle_event(&WindowEvent::ModifiersChanged(modifiers), &commands),
            None
        );
        assert_eq!(
            router.handle_event(&WindowEvent::KeyboardInput(key("s")), &commands),
            Some(9)
        );
        let mut repeated = key("s");
        repeated.repeat = true;
        assert_eq!(
            router.handle_event(&WindowEvent::KeyboardInput(repeated), &commands),
            None
        );
        assert_eq!(
            commands
                .get(&CommandId::new("file.save").unwrap())
                .unwrap()
                .shortcut
                .as_ref()
                .unwrap()
                .display_label(),
            "Ctrl+S"
        );
    }
}
