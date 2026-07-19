//! Native application menu integration for Astreon.

#![warn(missing_docs)]

use std::{error::Error, fmt};

use astrelis_platform::Window;
use astreon_app::{CommandRegistry, MenuBar};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod supported;

/// Opaque activation delivered by the native menu event bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeMenuEvent {
    id: String,
}

/// Installed application-level native menu.
pub struct ApplicationMenu {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    inner: supported::ApplicationMenu,
}

impl ApplicationMenu {
    /// Builds and installs an application menu for the first window.
    ///
    /// `wake` should forward the event into the application's Astrelis event
    /// loop proxy. Only one application menu may be installed per process.
    pub fn install<Message, F>(
        window: &Window,
        model: MenuBar,
        commands: &CommandRegistry<Message>,
        wake: F,
    ) -> Result<Self, NativeMenuError>
    where
        F: Fn(NativeMenuEvent) + Send + Sync + 'static,
    {
        model
            .validate(commands)
            .map_err(NativeMenuError::from_display)?;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            supported::ApplicationMenu::install(window, model, commands, wake)
                .map(|inner| Self { inner })
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (window, model, commands, wake);
            Err(NativeMenuError::unsupported())
        }
    }

    /// Attaches the application menu to another opted-in window.
    ///
    /// This is a no-op after tracking the window on macOS, where the menu is
    /// process-global.
    pub fn attach_window(&mut self, window: &Window) -> Result<(), NativeMenuError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.inner.attach_window(window)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = window;
            Err(NativeMenuError::unsupported())
        }
    }

    /// Detaches the menu from a previously opted-in window.
    pub fn detach_window(&mut self, window: &Window) -> Result<(), NativeMenuError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.inner.detach_window(window)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = window;
            Err(NativeMenuError::unsupported())
        }
    }

    /// Synchronizes labels, enabled state, checked state, and shortcuts.
    pub fn sync<Message>(
        &mut self,
        commands: &CommandRegistry<Message>,
    ) -> Result<(), NativeMenuError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.inner.sync(commands)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = commands;
            Err(NativeMenuError::unsupported())
        }
    }

    /// Resolves a native activation through the current command registry.
    pub fn dispatch<Message: Clone>(
        &self,
        event: &NativeMenuEvent,
        commands: &CommandRegistry<Message>,
    ) -> Option<Message> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.inner.dispatch(event, commands)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (event, commands);
            None
        }
    }
}

/// Native menu installation, model, or platform error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeMenuError {
    /// The current target has no Astreon native-menu backend.
    UnsupportedPlatform,
    /// Invalid model, installation failure, or native backend error.
    Backend(String),
}

impl NativeMenuError {
    pub(crate) fn from_display(error: impl fmt::Display) -> Self {
        Self::Backend(error.to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn unsupported() -> Self {
        Self::UnsupportedPlatform
    }
}

impl fmt::Display for NativeMenuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter
                .write_str("native application menus are supported only on macOS and Windows"),
            Self::Backend(message) => formatter.write_str(message),
        }
    }
}

impl Error for NativeMenuError {}

#[cfg(test)]
mod tests {
    use astreon_app::{Command, CommandId, Menu};

    use super::*;

    #[test]
    fn invalid_models_fail_before_platform_installation() {
        let commands = CommandRegistry::<()>::new();
        let model =
            MenuBar::new().menu(Menu::new("File").command(CommandId::new("file.missing").unwrap()));
        let error = model.validate(&commands).unwrap_err();
        assert!(error.to_string().contains("unknown command"));
    }

    #[test]
    fn command_models_validate_independently_of_the_platform() {
        let id = CommandId::new("file.save").unwrap();
        let mut commands = CommandRegistry::new();
        commands
            .register(Command::new(id.clone(), "Save", 1))
            .unwrap();
        let model = MenuBar::new().menu(Menu::new("File").command(id));
        assert!(model.validate(&commands).is_ok());
    }
}
