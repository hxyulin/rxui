//! Desktop application hosting and shared application commands.

#![warn(missing_docs)]

mod command;
mod host;

pub use command::{Command, CommandError, CommandId, CommandRegistry, Shortcut};
pub use host::{GraphicsContext, HostError, HostUpdate, WindowHost, WindowHostOptions};
