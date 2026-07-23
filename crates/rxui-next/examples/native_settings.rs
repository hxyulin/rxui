//! Native settings form with a state-owning nested component.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::io;

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig};
use astrelis_core::geometry::Size;
use astrelis_platform::{WindowAttributes, WindowEvent, WindowId};
use astrelis_ui_host::{GraphicsContext, WindowHostOptions};
use rxui_next::{
    ButtonStyle, ButtonVariant, ColorRole, Component, ComponentContext, ComponentWindow,
    ComponentWithProps, ContainerStyle, Space, Theme, View, button_with, checkbox, column_with,
    component, label, slider_with_step, text_field,
};

#[derive(Clone, Debug, PartialEq)]
struct PreferencesProps {
    profile: String,
}

#[derive(Clone)]
enum PreferencesAction {
    SetNotifications(bool),
    SetScale(f32),
    Apply,
}

#[derive(Clone)]
struct PreferencesSnapshot {
    profile: String,
    notifications: bool,
    scale: f32,
}

struct Preferences {
    profile: String,
    notifications: bool,
    scale: f32,
}

impl Component for Preferences {
    type Action = PreferencesAction;
    type Effect = PreferencesSnapshot;

    fn update(&mut self, action: Self::Action, context: &mut ComponentContext<'_, Self::Effect>) {
        match action {
            PreferencesAction::SetNotifications(value) => self.notifications = value,
            PreferencesAction::SetScale(value) => self.scale = value,
            PreferencesAction::Apply => context.emit(PreferencesSnapshot {
                profile: self.profile.clone(),
                notifications: self.notifications,
                scale: self.scale,
            }),
        }
    }

    fn view(&self, _theme: &Theme) -> View<Self::Action> {
        column_with(
            ContainerStyle::new()
                .gap(Space::Md)
                .padding(Space::Lg)
                .background(ColorRole::Surface),
            (
                label(format!("Local preferences for {}", self.profile)).key("heading"),
                checkbox(
                    "Notifications",
                    self.notifications,
                    PreferencesAction::SetNotifications,
                )
                .key("notifications"),
                slider_with_step(
                    "Interface scale",
                    self.scale,
                    0.75..=2.0,
                    0.05,
                    PreferencesAction::SetScale,
                )
                .key("scale"),
                button_with(
                    "Apply preferences",
                    PreferencesAction::Apply,
                    ButtonStyle::standard().variant(ButtonVariant::Primary),
                )
                .key("apply"),
            ),
        )
    }
}

impl ComponentWithProps for Preferences {
    type Props = PreferencesProps;

    fn create(props: &Self::Props) -> Self {
        Self {
            profile: props.profile.clone(),
            notifications: true,
            scale: 1.0,
        }
    }

    fn changed(&mut self, props: &Self::Props) {
        self.profile.clone_from(&props.profile);
    }
}

#[derive(Clone)]
enum Action {
    Rename(String),
    PreferencesApplied(PreferencesSnapshot),
}

struct Settings {
    profile: String,
    status: String,
}

impl Component for Settings {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Self::Action, _context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Rename(name) => self.profile = name,
            Action::PreferencesApplied(snapshot) => {
                self.status = format!(
                    "Applied {}: notifications={}, scale={:.2}",
                    snapshot.profile, snapshot.notifications, snapshot.scale
                );
            }
        }
    }

    fn view(&self, _theme: &Theme) -> View<Self::Action> {
        column_with(
            ContainerStyle::new()
                .gap(Space::Md)
                .padding(Space::Lg)
                .background(ColorRole::Background),
            (
                label("RXUI Next settings").key("title"),
                text_field("Profile", self.profile.clone(), Action::Rename).key("profile"),
                component::<Preferences, Action>(
                    PreferencesProps {
                        profile: self.profile.clone(),
                    },
                    Action::PreferencesApplied,
                )
                .key("preferences"),
                label(self.status.clone()).key("status"),
            ),
        )
    }
}

struct NativeSettings {
    graphics: GraphicsContext,
    window: Option<ComponentWindow<Settings>>,
}

impl NativeSettings {
    fn new() -> Self {
        Self {
            graphics: GraphicsContext::new(),
            window: None,
        }
    }
}

impl App for NativeSettings {
    type Error = io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.window.is_none() {
            let window = ComponentWindow::open(
                context,
                &self.graphics,
                Settings {
                    profile: "Astrelis".into(),
                    status: "No changes applied".into(),
                },
                Theme::dark(),
                WindowHostOptions {
                    window: WindowAttributes {
                        title: "RXUI Next settings".into(),
                        inner_size: Some(Size::new(520.0, 360.0)),
                        ..WindowAttributes::default()
                    },
                    ..WindowHostOptions::default()
                },
            )
            .map_err(io::Error::other)?;
            context.invalidate_window(window.window().id());
            self.window = Some(window);
        }
        Ok(())
    }

    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        let Some(window) = &mut self.window else {
            return Ok(());
        };
        if window.window().id() != id {
            return Ok(());
        }
        let update = window.handle_event(&event).map_err(io::Error::other)?;
        if update.close_requested {
            self.window = None;
            context.unregister_window(id);
            context.exit();
        } else if update.redraw {
            context.invalidate_window(id);
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        _context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
    ) -> Result<(), Self::Error> {
        if let Some(window) = &mut self.window
            && window.window().id() == id
        {
            window.redraw().map_err(io::Error::other)?;
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), astrelis_app::RuntimeError<io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        NativeSettings::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
