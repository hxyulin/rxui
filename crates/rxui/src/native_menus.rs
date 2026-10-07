//! Declarative native menu bars routed through RXUI's current command scopes.
//!
//! Menu entries keep command identities, never captured view callbacks. Their live
//! caption, availability and first representable shortcut come from the nearest
//! action in the source window. Missing handlers use the supplied fallback caption
//! and appear disabled. Standard editing and lifecycle commands have host fallbacks.
//! The `native-menus` feature supports macOS application menus and Windows window
//! menu bars. The winit host cannot attach Muda's GTK menu bars on Linux/BSD; asking
//! for a menu bar there returns an error before the event loop starts.
use crate::{Command, CommandId};

/// Application menu bar description, installed by Application::menu_bar.
#[derive(Clone, Debug, Default)]
pub struct NativeMenuBar {
    pub(crate) menus: Vec<NativeMenu>,
}
impl NativeMenuBar {
    /// Creates an empty menu bar. Add top-level submenus with menu().
    pub fn new() -> Self {
        Self::default()
    }
    /// Appends a top-level menu in declaration order. On macOS put the
    /// application menu first; its displayed title comes from the app bundle.
    pub fn menu(mut self, menu: NativeMenu) -> Self {
        self.menus.push(menu);
        self
    }
}
/// One top-level or nested native submenu.
#[derive(Clone, Debug)]
pub struct NativeMenu {
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(crate) label: String,
    pub(crate) entries: Vec<Entry>,
}
impl NativeMenu {
    /// Creates a submenu with a literal caption (ampersands are not mnemonics).
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            entries: Vec::new(),
        }
    }
    /// Appends a typed command. The fallback caption is shown when no live action
    /// resolves; a live action supplies its own caption, enabled state and shortcuts.
    pub fn command<C: Command>(mut self, fallback_label: impl Into<String>) -> Self {
        self.entries
            .push(Entry::Command(CommandId::of::<C>(), fallback_label.into()));
        self
    }
    /// Appends a visual separator.
    pub fn separator(mut self) -> Self {
        self.entries.push(Entry::Separator);
        self
    }
    /// Appends a nested submenu.
    pub fn submenu(mut self, menu: NativeMenu) -> Self {
        self.entries.push(Entry::Submenu(menu));
        self
    }
    /// Appends a macOS application role. These roles are omitted on Windows.
    /// Editing, closing and quitting use standard_commands instead, so RXUI's
    /// custom controls and application veto hooks remain authoritative.
    pub fn role(mut self, role: NativeMenuRole) -> Self {
        self.entries.push(Entry::Role(role));
        self
    }
}
/// System-managed macOS application menu roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeMenuRole {
    /// Standard About panel using native application metadata.
    About,
    /// System Services submenu.
    Services,
    /// Hides this application.
    Hide,
    /// Hides other applications.
    HideOthers,
    /// Reveals all applications.
    ShowAll,
}
#[derive(Clone, Debug)]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub(crate) enum Entry {
    Command(CommandId, String),
    Separator,
    Submenu(NativeMenu),
    Role(NativeMenuRole),
}
