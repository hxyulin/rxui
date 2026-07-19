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
        Command, CommandId, CommandRegistry, CommandRouter, GraphicsContext, Menu, MenuBar,
        MenuEntry, MenuRole, Shortcut, WindowHost, WindowHostOptions,
    };
    pub use astreon_native_menu::{ApplicationMenu, NativeMenuError, NativeMenuEvent};
    pub use astreon_widgets::{
        ComboBox, ComboBoxItem, FormSection, Icon, IconButton, IconView, NumericField,
        NumericFieldOptions, RadioGroup, RadioOption, ThemePreference, ThemeSet, icons,
    };
}
