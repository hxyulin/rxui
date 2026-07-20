//! Desktop application hosting and shared application commands.

#![warn(missing_docs)]

mod command;
mod host;
mod menu;
mod state;
mod undo;

pub use command::{Command, CommandError, CommandId, CommandRegistry, CommandRouter, Shortcut};
pub use host::{GraphicsContext, HostError, HostStatus, HostUpdate, WindowHost, WindowHostOptions};
pub use menu::{Menu, MenuBar, MenuEntry, MenuError, MenuRole};
pub use state::{
    JsonStateStore, PersistError, StateEnvelope, WindowPlacement, WindowPlacementTracker,
};
pub use undo::{UndoAction, UndoStack, redo_command_id, sync_undo_commands, undo_command_id};

/// Backend-neutral GPU types used by application-owned render views.
pub use astrelis_gpu as gpu;
