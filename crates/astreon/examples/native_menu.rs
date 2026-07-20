//! Native application menu backed by Astreon commands.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::io;

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig};
use astrelis_core::geometry::Size;
use astrelis_platform::{WindowAttributes, WindowEvent, WindowId};
use astrelis_text::FontDatabase;
use astreon::prelude::*;

#[derive(Clone, Debug)]
enum Message {
    New,
    Save,
    ToggleSidebar,
}

struct NativeMenuDemo {
    graphics: GraphicsContext,
    host: Option<WindowHost<Message>>,
    commands: CommandRegistry<Message>,
    router: CommandRouter,
    menu: Option<ApplicationMenu>,
    status: Option<ElementHandle<Label>>,
    sidebar_id: CommandId,
}

impl NativeMenuDemo {
    fn new() -> Self {
        let sidebar_id = CommandId::new("view.sidebar").expect("static command id");
        let mut commands = CommandRegistry::new();
        commands
            .register(
                Command::new(
                    CommandId::new("file.new").expect("static command id"),
                    "New",
                    Message::New,
                )
                .shortcut(Shortcut::primary("n")),
            )
            .expect("unique command");
        commands
            .register(
                Command::new(
                    CommandId::new("file.save").expect("static command id"),
                    "Save",
                    Message::Save,
                )
                .shortcut(Shortcut::primary("s")),
            )
            .expect("unique command");
        commands
            .register(
                Command::new(sidebar_id.clone(), "Show Sidebar", Message::ToggleSidebar)
                    .checked(true),
            )
            .expect("unique command");
        Self {
            graphics: GraphicsContext::new(),
            host: None,
            commands,
            router: CommandRouter::new(),
            menu: None,
            status: None,
            sidebar_id,
        }
    }

    fn menu_model(&self) -> MenuBar {
        let app = Menu::new("Astreon")
            .role(MenuRole::About)
            .separator()
            .role(MenuRole::Quit);
        let file = Menu::new("File")
            .command(CommandId::new("file.new").expect("static command id"))
            .command(CommandId::new("file.save").expect("static command id"));
        let edit = Menu::new("Edit")
            .role(MenuRole::Undo)
            .role(MenuRole::Redo)
            .separator()
            .role(MenuRole::Cut)
            .role(MenuRole::Copy)
            .role(MenuRole::Paste)
            .role(MenuRole::SelectAll);
        let view = Menu::new("View").command(self.sidebar_id.clone());
        let window = Menu::new("Window")
            .role(MenuRole::Minimize)
            .role(MenuRole::Maximize)
            .role(MenuRole::CloseWindow);
        MenuBar::new()
            .menu(app)
            .menu(file)
            .menu(edit)
            .menu(view)
            .menu(window)
    }

    fn apply_message(
        &mut self,
        message: Message,
        context: &mut AppContext<'_, '_, Self>,
    ) -> Result<(), io::Error> {
        let text = match message {
            Message::New => "New command activated".to_owned(),
            Message::Save => "Save command activated".to_owned(),
            Message::ToggleSidebar => {
                let command = self
                    .commands
                    .get_mut(&self.sidebar_id)
                    .expect("sidebar command remains registered");
                let checked = !command.checked.unwrap_or(false);
                command.checked = Some(checked);
                format!("Sidebar is {}", if checked { "visible" } else { "hidden" })
            }
        };
        if let (Some(host), Some(status)) = (&mut self.host, self.status) {
            host.ui_mut()
                .set_label_text(status, text)
                .map_err(io::Error::other)?;
            context.invalidate_window(host.id());
        }
        if let Some(menu) = &mut self.menu {
            menu.sync(&self.commands).map_err(io::Error::other)?;
        }
        Ok(())
    }

    fn apply_native(
        &mut self,
        event: NativeMenuEvent,
        context: &mut AppContext<'_, '_, Self>,
    ) -> Result<(), io::Error> {
        let message = self
            .menu
            .as_ref()
            .and_then(|menu| menu.dispatch(&event, &self.commands));
        if let Some(message) = message {
            self.apply_message(message, context)?;
        }
        Ok(())
    }
}

impl App for NativeMenuDemo {
    type Error = io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.host.is_some() {
            return Ok(());
        }
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        let content = ui
            .padding(ui.root(), Insets::all(28.0))
            .grow(1.0)
            .column()
            .finish();
        ui.label(content, "Native File / Edit / View / Window menu")
            .finish();
        ui.label(content, "Try Command/Ctrl+N and Command/Ctrl+S")
            .finish();
        let status = ui.label(content, "Ready").finish();
        let host = WindowHost::open(
            context,
            &self.graphics,
            ui,
            WindowHostOptions {
                window: WindowAttributes {
                    title: "Astreon native menu".into(),
                    inner_size: Some(Size::new(640.0, 320.0)),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;

        let proxy = context.proxy();
        let menu = ApplicationMenu::install(
            host.window(),
            self.menu_model(),
            &self.commands,
            move |event| {
                let _ =
                    proxy.run_on_main_thread(move |app, context| app.apply_native(event, context));
            },
        );
        match menu {
            Ok(menu) => self.menu = Some(menu),
            Err(error @ NativeMenuError::UnsupportedPlatform) => {
                eprintln!("native menu unavailable: {error}");
            }
            Err(error) => return Err(io::Error::other(error)),
        }
        self.status = Some(status);
        self.host = Some(host);
        Ok(())
    }

    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        if let Some(message) = self.router.handle_event(&event, &self.commands) {
            self.apply_message(message, context)?;
        }
        let Some(host) = &mut self.host else {
            return Ok(());
        };
        let update = host
            .handle_event(&context.clipboard(), &event)
            .map_err(io::Error::other)?;
        if update.close_requested {
            self.menu = None;
            context.unregister_window(id);
            self.host = None;
            context.exit();
        } else if update.redraw || host.ui().needs_redraw() {
            context.invalidate_window(id);
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        _context: &mut AppContext<'_, '_, Self>,
        _window: WindowId,
    ) -> Result<(), Self::Error> {
        if let Some(host) = &mut self.host {
            host.redraw().map_err(io::Error::other)?;
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), astrelis_app::RuntimeError<io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        NativeMenuDemo::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
