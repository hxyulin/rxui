//! Batteries-included retained-mode desktop UI framework.

#![warn(missing_docs)]

/// Low-level retained UI escape hatch.
pub use astrelis_ui as ui;
/// Application hosting and shared commands.
pub use astreon_app as app;
#[cfg(feature = "editor")]
/// Editor workspace components.
pub use astreon_editor as editor;
/// Native macOS and Windows application menus.
pub use astreon_native_menu as native_menu;
#[cfg(feature = "testing")]
/// Deterministic testing helpers.
pub use astreon_testing as testing;
/// Application-oriented widgets.
pub use astreon_widgets as widgets;

/// Common types for ordinary Astreon application code.
pub mod prelude {
    pub use astrelis_ui::prelude::*;
    pub use astreon_app::{
        Command, CommandId, CommandRegistry, CommandRouter, GraphicsContext, JsonStateStore, Menu,
        MenuBar, MenuEntry, MenuRole, Shortcut, UndoAction, UndoStack, WindowHost,
        WindowHostOptions, WindowPlacement, WindowPlacementTracker, redo_command_id,
        sync_undo_commands, undo_command_id,
    };
    #[cfg(feature = "editor")]
    pub use astreon_editor::{
        PropertyAction, PropertyField, PropertyGrid, PropertySection, PropertyValue, SavedLayout,
        SavedLayoutError, WorkspaceState,
    };
    pub use astreon_native_menu::{ApplicationMenu, NativeMenuError, NativeMenuEvent};
    pub use astreon_widgets::{
        ComboBox, ComboBoxItem, CommandButton, CommandPalette, CommandPaletteEvent,
        CommandPaletteState, DialogAction, DialogActionRole, DialogHost, DialogOptions,
        FieldValidation, FormSection, FormValidation, Icon, IconButton, IconView, NumericField,
        NumericFieldOptions, RadioGroup, RadioOption, SortDirection, TableAction, TableColumn,
        TableRow, TableSort, TableView, ThemePreference, ThemeSet, Toast, ToastAction, ToastHost,
        ToastId, ToastLevel, ToastQueue, Toolbar, ToolbarItem, ToolbarOptions, TreeAction,
        TreeNode, TreeView, ValidationIssue, ValidationResult, ValidationSeverity, icons,
    };
}
