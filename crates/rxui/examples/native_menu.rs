//! Native application menu backed by RXUI commands.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use rxui::app::{WindowEvent, WindowId};
use rxui::prelude::*;

#[derive(Clone, Debug)]
enum Message {
    New,
    Save,
    ToggleSidebar,
    Menu(NativeMenuEvent),
}

struct NativeMenuDemo {
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
            commands,
            router: CommandRouter::new(),
            menu: None,
            status: None,
            sidebar_id,
        }
    }

    fn menu_model(&self) -> MenuBar {
        let app = Menu::new("RXUI")
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
}

impl App for NativeMenuDemo {
    type Message = Message;

    fn build(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let mut ui = cx.new_ui();
        let content = ui
            .padding(ui.root(), Insets::all(28.0))
            .grow(1.0)
            .column()
            .finish();
        ui.label(content, "Native File / Edit / View / Window menu")
            .finish();
        ui.label(content, "Try Command/Ctrl+N and Command/Ctrl+S")
            .finish();
        self.status = Some(ui.label(content, "Ready").finish());
        let window =
            cx.open_window(WindowConfig::new("RXUI native menu").size(640.0, 320.0), ui)?;

        let proxy = cx.proxy();
        let menu = ApplicationMenu::install(
            cx.window(window)?,
            self.menu_model(),
            &self.commands,
            move |event| {
                let _ = proxy.post(Message::Menu(event));
            },
        );
        match menu {
            Ok(menu) => self.menu = Some(menu),
            Err(error @ NativeMenuError::UnsupportedPlatform) => {
                eprintln!("native menu unavailable: {error}");
            }
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        let text = match message {
            Message::Menu(event) => {
                // Resolve the native activation through the command registry
                // and feed the mapped message back through `update`.
                let mapped = self
                    .menu
                    .as_ref()
                    .and_then(|menu| menu.dispatch(&event, &self.commands));
                if let Some(mapped) = mapped {
                    cx.post(mapped);
                }
                return Ok(());
            }
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
        if let Some(status) = self.status {
            cx.source_ui()?.set_label_text(status, text)?;
        }
        if let Some(menu) = &mut self.menu {
            menu.sync(&self.commands)?;
        }
        Ok(())
    }

    fn window_event(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        _window: WindowId,
        event: &WindowEvent,
    ) -> rxui::Result<()> {
        // Route keyboard shortcuts through the command registry before the
        // widget layer sees the event.
        if let Some(message) = self.router.handle_event(event, &self.commands) {
            cx.post(message);
        }
        Ok(())
    }

    fn window_closed(
        &mut self,
        _cx: &mut AppCx<'_, Message>,
        _window: WindowId,
    ) -> rxui::Result<()> {
        self.menu = None;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> MainResult {
    run(NativeMenuDemo::new())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
