//! Desktop application hosting and shared application commands.

#![warn(missing_docs)]

mod command;
mod host;
mod menu;

pub use command::{Command, CommandError, CommandId, CommandRegistry, CommandRouter, Shortcut};
pub use host::{GraphicsContext, HostError, HostUpdate, WindowHost, WindowHostOptions};
pub use menu::{Menu, MenuBar, MenuEntry, MenuError, MenuRole};
