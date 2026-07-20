//! Astreon 0.3 application-shell showcase.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::{any::Any, io};

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig};
use astrelis_core::geometry::Size;
use astrelis_platform::{WindowAttributes, WindowEvent, WindowId};
use astrelis_text::FontDatabase;
use astrelis_ui_core::{EventFilter, RoutedEventKind, TextField};
use astreon::prelude::*;

#[derive(Clone, Debug)]
enum Message {
    Increment,
    Undo,
    Redo,
    Settings,
    NameChanged(String),
    SaveSettings,
    CancelSettings,
    Dismiss(ToastId),
    Retry,
}

struct ChangeValue {
    amount: i32,
}
impl UndoAction<i32, io::Error> for ChangeValue {
    fn label(&self) -> &str {
        "Value"
    }
    fn redo(&mut self, state: &mut i32) -> Result<(), io::Error> {
        *state += self.amount;
        Ok(())
    }
    fn undo(&mut self, state: &mut i32) -> Result<(), io::Error> {
        *state -= self.amount;
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct Shell {
    graphics: GraphicsContext,
    host: Option<WindowHost<Message>>,
    commands: CommandRegistry<Message>,
    toolbar: Option<Toolbar<Message>>,
    dialogs: DialogHost,
    toast_queue: ToastQueue<Message>,
    toast_host: Option<ToastHost>,
    undo: UndoStack<i32, io::Error>,
    value: i32,
    value_label: Option<ElementHandle<Label>>,
    name: String,
    validation: FormValidation<&'static str>,
    name_binding: Option<FieldValidation<TextField>>,
    placement: WindowPlacementTracker,
    saved_placement: Option<WindowPlacement>,
    state_store: Option<JsonStateStore>,
}

impl Shell {
    fn new() -> Self {
        let mut commands = CommandRegistry::new();
        commands
            .register(Command::new(
                CommandId::new("edit.increment").unwrap(),
                "Increment",
                Message::Increment,
            ))
            .unwrap();
        commands
            .register(
                Command::new(undo_command_id(), "Undo", Message::Undo)
                    .shortcut(Shortcut::primary("z"))
                    .enabled(false),
            )
            .unwrap();
        commands
            .register(
                Command::new(redo_command_id(), "Redo", Message::Redo)
                    .shortcut(Shortcut::primary("y"))
                    .enabled(false),
            )
            .unwrap();
        commands
            .register(Command::new(
                CommandId::new("app.settings").unwrap(),
                "Settings",
                Message::Settings,
            ))
            .unwrap();
        let state_store = JsonStateStore::for_app("dev", "Astreon", "ApplicationShell").ok();
        let saved_placement = state_store
            .as_ref()
            .and_then(|store| store.load(1).ok().flatten());
        let placement = saved_placement.clone().map_or_else(
            WindowPlacementTracker::default,
            WindowPlacementTracker::from_placement,
        );
        Self {
            graphics: GraphicsContext::new(),
            host: None,
            commands,
            toolbar: None,
            dialogs: DialogHost::new(),
            toast_queue: ToastQueue::default(),
            toast_host: None,
            undo: UndoStack::new(100),
            value: 0,
            value_label: None,
            name: String::new(),
            validation: FormValidation::new(),
            name_binding: None,
            placement,
            saved_placement,
            state_store,
        }
    }

    fn refresh_shell(&mut self) -> Result<(), io::Error> {
        sync_undo_commands(&mut self.commands, &self.undo);
        let host = self.host.as_mut().expect("host exists");
        host.ui_mut()
            .set_label_text(
                self.value_label.expect("value label"),
                format!("Undoable value: {}", self.value),
            )
            .map_err(io::Error::other)?;
        self.toolbar
            .as_ref()
            .expect("toolbar")
            .sync(host.ui_mut(), &self.commands)
            .map_err(io::Error::other)
    }

    fn sync_toasts(&mut self) -> Result<(), io::Error> {
        let host = self.host.as_mut().expect("host exists");
        self.toast_host
            .as_mut()
            .expect("toast host")
            .sync(host.ui_mut(), &self.toast_queue, Message::Dismiss)
            .map_err(io::Error::other)
    }

    fn notify(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        toast: Toast<Message>,
    ) -> Result<(), io::Error> {
        let now = context.now();
        self.toast_queue.push(toast, now);
        self.sync_toasts()?;
        if let Some(deadline) = self.toast_queue.next_deadline() {
            context.set_timeout(deadline.saturating_duration_since(now), |app, context| {
                app.toast_queue.expire(context.now());
                app.sync_toasts()?;
                if let Some(host) = &app.host {
                    context.invalidate_window(host.id());
                }
                Ok(())
            });
        }
        Ok(())
    }

    fn open_settings(&mut self) -> Result<(), io::Error> {
        let host = self.host.as_mut().expect("host exists");
        let mut binding = None;
        self.dialogs
            .show(
                host.ui_mut(),
                DialogOptions {
                    title: "Settings".into(),
                    description: Some("Validation appears after editing or submitting.".into()),
                },
                vec![
                    DialogAction {
                        label: "Cancel".into(),
                        message: Message::CancelSettings,
                        role: DialogActionRole::Cancel,
                        enabled: true,
                    },
                    DialogAction {
                        label: "Save".into(),
                        message: Message::SaveSettings,
                        role: DialogActionRole::Primary,
                        enabled: true,
                    },
                ],
                |ui, content| {
                    ui.add_label(content, "Display name")?;
                    let field = ui.add_text_field(content, &self.name)?;
                    ui.listen(field, None, EventFilter::ValueChanged, |context, event| {
                        if let RoutedEventKind::TextChanged(value) = &event.kind {
                            context.emit(Message::NameChanged(value.clone()));
                        }
                    })?;
                    binding = Some(FieldValidation::new(ui, field, content)?);
                    Ok(())
                },
            )
            .map_err(io::Error::other)?;
        self.name_binding = binding;
        Ok(())
    }

    fn apply(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        message: Message,
    ) -> Result<(), io::Error> {
        match message {
            Message::Increment => {
                self.undo
                    .execute(ChangeValue { amount: 1 }, &mut self.value)?;
                self.refresh_shell()?;
                let mut toast = Toast::new(
                    ToastLevel::Success,
                    "Value changed",
                    "Use Undo to restore it, or run the sample action.",
                );
                toast.action = Some(ToastAction {
                    label: "Retry".into(),
                    message: Message::Retry,
                });
                self.notify(context, toast)?;
            }
            Message::Undo => {
                self.undo.undo(&mut self.value)?;
                self.refresh_shell()?;
            }
            Message::Redo => {
                self.undo.redo(&mut self.value)?;
                self.refresh_shell()?;
            }
            Message::Settings => self.open_settings()?,
            Message::NameChanged(value) => {
                self.name = value;
                self.validation.set_result(
                    "name",
                    if self.name.trim().is_empty() {
                        ValidationResult::issue(ValidationIssue::error(
                            "A display name is required.",
                        ))
                    } else {
                        ValidationResult::valid()
                    },
                );
                self.validation.touch(&"name");
                if let (Some(host), Some(binding)) = (&mut self.host, &self.name_binding) {
                    binding
                        .sync(host.ui_mut(), self.validation.visible_result(&"name"))
                        .map_err(io::Error::other)?;
                }
            }
            Message::SaveSettings => {
                self.validation.set_result(
                    "name",
                    if self.name.trim().is_empty() {
                        ValidationResult::issue(ValidationIssue::error(
                            "A display name is required.",
                        ))
                    } else {
                        ValidationResult::valid()
                    },
                );
                self.validation.submit();
                let valid = self.validation.is_valid();
                if let (Some(host), Some(binding)) = (&mut self.host, &self.name_binding) {
                    binding
                        .sync(host.ui_mut(), self.validation.visible_result(&"name"))
                        .map_err(io::Error::other)?;
                    if !valid {
                        binding.focus(host.ui_mut()).map_err(io::Error::other)?;
                    }
                }
                if valid {
                    self.dialogs
                        .close(self.host.as_mut().unwrap().ui_mut())
                        .map_err(io::Error::other)?;
                    self.name_binding = None;
                    self.notify(
                        context,
                        Toast::new(
                            ToastLevel::Info,
                            "Settings saved",
                            format!("Hello, {}!", self.name),
                        ),
                    )?;
                }
            }
            Message::CancelSettings => {
                self.dialogs
                    .close(self.host.as_mut().unwrap().ui_mut())
                    .map_err(io::Error::other)?;
                self.name_binding = None;
            }
            Message::Dismiss(id) => {
                self.toast_queue.dismiss(id, context.now());
                self.sync_toasts()?;
            }
            Message::Retry => self.notify(
                context,
                Toast::new(
                    ToastLevel::Success,
                    "Retry complete",
                    "The action succeeded.",
                ),
            )?,
        }
        Ok(())
    }
}

impl App for Shell {
    type Error = io::Error;
    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.host.is_some() {
            return Ok(());
        }
        let mut ui = Ui::new(FontDatabase::default(), Theme::dark());
        let content = ui
            .padding(ui.root(), Insets::all(20.0))
            .grow(1.0)
            .column()
            .finish();
        let toolbar = Toolbar::new(
            &mut ui,
            content,
            vec![
                ToolbarItem::Command {
                    id: CommandId::new("edit.increment").unwrap(),
                    icon: Some(icons::add()),
                },
                ToolbarItem::Separator,
                ToolbarItem::Command {
                    id: undo_command_id(),
                    icon: Some(icons::undo()),
                },
                ToolbarItem::Command {
                    id: redo_command_id(),
                    icon: Some(icons::redo()),
                },
                ToolbarItem::FlexibleSpace,
                ToolbarItem::Command {
                    id: CommandId::new("app.settings").unwrap(),
                    icon: Some(icons::settings()),
                },
            ],
            &self.commands,
            ToolbarOptions::default(),
        )
        .map_err(io::Error::other)?;
        toolbar
            .update_overflow(&mut ui, 760.0)
            .map_err(io::Error::other)?;
        ui.add_label(content, "Astreon 0.3 application shell")
            .map_err(io::Error::other)?;
        let value_label = ui
            .add_label(content, "Undoable value: 0")
            .map_err(io::Error::other)?;
        ui.add_label(content, "Resize the window to exercise toolbar overflow. Open Settings to see a validated modal.").map_err(io::Error::other)?;
        let toast_host = ToastHost::new(&mut ui).map_err(io::Error::other)?;
        let mut attributes = WindowAttributes {
            title: "Astreon application shell".into(),
            inner_size: Some(Size::new(820.0, 480.0)),
            ..Default::default()
        };
        if let Some(saved) = &self.saved_placement {
            let monitors = context.available_monitors();
            let primary = context.primary_monitor();
            saved.apply(&mut attributes, &monitors, primary.as_ref());
        }
        let host = WindowHost::open(
            context,
            &self.graphics,
            ui,
            WindowHostOptions {
                window: attributes,
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
        self.toolbar = Some(toolbar);
        self.toast_host = Some(toast_host);
        self.value_label = Some(value_label);
        self.host = Some(host);
        Ok(())
    }
    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        let Some(host) = &mut self.host else {
            return Ok(());
        };
        self.placement.handle_event(host.window(), &event);
        let update = host
            .handle_event(&context.clipboard(), &event)
            .map_err(io::Error::other)?;
        if let WindowEvent::Resized(size) = event {
            let width = size.width as f32 / host.window().scale_factor() as f32;
            self.toolbar
                .as_ref()
                .unwrap()
                .update_overflow(host.ui_mut(), width - 40.0)
                .map_err(io::Error::other)?;
        }
        if update.close_requested {
            self.placement.capture(host.window());
            if let (Some(store), Some(placement)) = (&self.state_store, self.placement.placement())
            {
                let _ = store.save(1, placement);
            }
            context.unregister_window(id);
            self.host = None;
            context.exit();
            return Ok(());
        }
        let messages = host.drain_messages().collect::<Vec<_>>();
        for message in messages {
            self.apply(context, message)?;
        }
        if update.redraw
            || self
                .host
                .as_ref()
                .is_some_and(|host| host.ui().needs_redraw())
        {
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
        Shell::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
