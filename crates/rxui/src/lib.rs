//! Batteries-included retained-mode desktop UI framework.

#![warn(missing_docs)]

/// Low-level retained UI escape hatch.
pub use astrelis_ui as ui;
/// Application hosting and shared commands.
pub use rxui_app as app;
#[cfg(feature = "devtools")]
/// Optional in-application developer tools.
pub use rxui_devtools as devtools;
#[cfg(feature = "editor")]
/// Editor workspace components.
pub use rxui_editor as editor;
/// Native macOS and Windows application menus.
pub use rxui_native_menu as native_menu;
#[cfg(feature = "testing")]
/// Deterministic testing helpers.
pub use rxui_testing as testing;
/// Application-oriented widgets.
pub use rxui_widgets as widgets;

/// Common types for ordinary RXUI application code.
pub mod prelude {
    pub use astrelis_ui::prelude::*;
    pub use rxui_app::{
        Command, CommandId, CommandRegistry, CommandRouter, GraphicsContext, HostStatus,
        JsonStateStore, Menu, MenuBar, MenuEntry, MenuRole, Shortcut, UndoAction, UndoStack,
        WindowHost, WindowHostOptions, WindowPlacement, WindowPlacementTracker, redo_command_id,
        sync_undo_commands, undo_command_id,
    };
    #[cfg(feature = "devtools")]
    pub use rxui_devtools::{InspectorAction, InspectorOptions, UiInspector};
    #[cfg(feature = "editor")]
    pub use rxui_editor::{
        PropertyAction, PropertyField, PropertyGrid, PropertySection, PropertyValue, SavedLayout,
        SavedLayoutError, WorkspaceState,
    };
    pub use rxui_native_menu::{ApplicationMenu, NativeMenuError, NativeMenuEvent};
    pub use rxui_widgets::{
        ComboBox, ComboBoxItem, CommandButton, CommandPalette, CommandPaletteEvent,
        CommandPaletteState, DialogAction, DialogActionRole, DialogHost, DialogOptions,
        FieldValidation, FormSection, FormValidation, Icon, IconButton, IconView, NumericField,
        NumericFieldOptions, RadioGroup, RadioOption, SortDirection, TableAction, TableColumn,
        TableRow, TableSort, TableView, ThemePreference, ThemeSet, Toast, ToastAction, ToastHost,
        ToastId, ToastLevel, ToastQueue, Toolbar, ToolbarItem, ToolbarOptions, TreeAction,
        TreeNode, TreeView, ValidationIssue, ValidationResult, ValidationSeverity, icons,
    };
}
