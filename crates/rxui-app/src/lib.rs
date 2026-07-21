//! Desktop application hosting and shared application commands.

#![warn(missing_docs)]

mod command;
mod error;
mod host;
mod menu;
mod runner;
mod state;
mod undo;

pub use command::{Command, CommandError, CommandId, CommandRegistry, CommandRouter, Shortcut};
pub use error::{Error, Result};
pub use host::{GraphicsContext, HostError, HostStatus, HostUpdate, WindowHost, WindowHostOptions};
pub use menu::{Menu, MenuBar, MenuEntry, MenuError, MenuRole};
#[cfg(target_arch = "wasm32")]
pub use runner::spawn_on_canvas;
pub use runner::{
    App, AppBackend, AppConfig, AppCx, Clipboard, CloseResponse, FixedStep, FontDatabaseOptions,
    Instant, MainResult, MessageProxy, Monitor, ProxyClosed, RunError, RuntimeConfig,
    RuntimePolicy, Theme, TimerId, Ui, UpdateInfo, WindowAttributes, WindowConfig, WindowEvent,
    WindowId,
};
#[cfg(not(target_arch = "wasm32"))]
pub use runner::{run, run_with};
pub use state::{
    JsonStateStore, PersistError, StateEnvelope, WindowPlacement, WindowPlacementTracker,
};
pub use undo::{UndoAction, UndoStack, redo_command_id, sync_undo_commands, undo_command_id};

/// Backend-neutral GPU types used by application-owned render views.
pub use astrelis_gpu as gpu;
