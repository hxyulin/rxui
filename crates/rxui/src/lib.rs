//! Batteries-included retained-mode desktop UI framework.

#![warn(missing_docs)]

/// Low-level retained UI escape hatch.
pub use astrelis_ui as ui;
/// Application hosting and shared commands.
pub use rxui_app as app;
pub use rxui_app::{Error, Result};
#[cfg(feature = "devtools")]
/// Optional in-application developer tools.
pub use rxui_devtools as devtools;
#[cfg(feature = "editor")]
/// Editor workspace components.
pub use rxui_editor as editor;
/// Native macOS and Windows application menus.
pub use rxui_native_menu as native_menu;
/// Typed component, reconciled-view, and incremental native hosting API.
pub use rxui_next as next;
/// Desktop services: file dialogs, launching, recent documents.
pub use rxui_services as services;
#[cfg(feature = "testing")]
/// Deterministic testing helpers.
pub use rxui_testing as testing;
/// Application-oriented widgets.
pub use rxui_widgets as widgets;

/// Common types for ordinary RXUI application code.
pub mod prelude {
    pub use astrelis_ui::prelude::*;
    #[cfg(target_arch = "wasm32")]
    pub use rxui_app::spawn_on_canvas;
    pub use rxui_app::{
        ActiveSubscriptionSnapshot, ActiveTaskSnapshot, App, AppConfig, AppCx, CloseResponse,
        Command, CommandId, CommandRegistry, CommandRouter, DeliveryPolicy, GraphicsContext,
        HostStatus, Instant, JsonStateStore, MainResult, MappedAppCx, Menu, MenuBar, MenuEntry,
        MenuRole, MessageKey, MessageMapper, MessageMetadata, MessageOrigin, MessageOutcome,
        MessageProxy, MessageTrace, MessageTraceIdentity, RuntimeEvent,
        RuntimeInstrumentationConfig, RuntimeLifecycleEvent, RuntimeLifecycleTrace,
        RuntimeObserver, RuntimeResource, RuntimeSnapshot, Shortcut, Subscription, SubscriptionId,
        SubscriptionKind, SubscriptionStatus, Subscriptions, TaskCompletion, TaskCompletionStatus,
        TaskConfig, TaskError, TaskId, TaskKind, TaskSpawnError, TimerId, UndoAction, UndoStack,
        UpdateInfo, WindowConfig, WindowEvent, WindowHost, WindowHostOptions, WindowId,
        WindowPlacement, WindowPlacementTracker, redo_command_id, sync_undo_commands,
        undo_command_id,
    };
    #[cfg(not(target_arch = "wasm32"))]
    pub use rxui_app::{run, run_with};
    #[cfg(feature = "devtools")]
    pub use rxui_devtools::{
        InspectorAction, InspectorOptions, InspectorView, RuntimeSection, UiInspector,
    };
    #[cfg(feature = "editor")]
    pub use rxui_editor::{
        GraphEdge, GraphEndpoint, GraphInteractionPhase, GraphNode, GraphPoint, GraphPort,
        GraphPortDirection, GraphSize, GraphViewport, NodeGraphAction, NodeGraphDocument,
        NodeGraphError, NodeGraphOptions, NodeGraphSelection, NodeGraphView, PropertyAction,
        PropertyField, PropertyGrid, PropertySection, PropertyValue, SavedLayout, SavedLayoutError,
        WorkspaceState,
    };
    pub use rxui_native_menu::{ApplicationMenu, NativeMenuError, NativeMenuEvent};
    pub use rxui_services::{
        DesktopServices, FileDialogOptions, FileFilter, FileWatchEvent, FileWatchKind,
        FileWatchOptions, FileWatcher, RecentDocuments, SavedFile, SelectedFile, ServiceError,
    };
    pub use rxui_widgets::{
        AxisOptions, ChartAction, ChartAxes, ChartError, ChartInteractionOptions, ChartOptions,
        ChartPoint, ChartSelection, ChartSeries, ChartSeriesKind, ChartView, ChartViewport,
        ComboBox, ComboBoxItem, CommandButton, CommandPalette, CommandPaletteEvent,
        CommandPaletteState, DialogAction, DialogActionRole, DialogHost, DialogOptions,
        FieldValidation, FormSection, FormValidation, Icon, IconButton, IconView, ImageAlignment,
        ImageDecodeError, ImageFit, ImageView, NumericField, NumericFieldOptions, RadioGroup,
        RadioOption, ScrollNavigation, SortDirection, TableAction, TableColumn, TableRow,
        TableSort, TableView, ThemePreference, ThemeSet, Toast, ToastAction, ToastHost, ToastId,
        ToastLevel, ToastQueue, Toolbar, ToolbarItem, ToolbarOptions, TreeAction, TreeNode,
        TreeView, ValidationIssue, ValidationResult, ValidationSeverity,
        ViewportNavigationBindings, ViewportNavigationIntent, decode_image, icons,
    };
}
