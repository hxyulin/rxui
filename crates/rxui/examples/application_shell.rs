//! RXUI application-shell showcase.
//!
//! Runs on the high-level [`rxui::app`] runner: the `window_event` hook feeds
//! shortcut routing, drag-and-drop, the window-placement tracker, and toolbar
//! overflow; the close hooks persist placement plus recent files through
//! [`JsonStateStore`]; toast deadlines arrive as [`Message::ExpireToasts`]
//! timer messages; and [`DesktopServices`] drives the open/save dialogs and
//! website launching.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::{
    any::Any,
    io,
    path::{Path, PathBuf},
};

use astrelis_core::geometry::Size;
use astrelis_ui_core::{EventFilter, RoutedEventKind, TextField};
use rxui::app::WindowAttributes;
use rxui::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
enum Message {
    Increment,
    Undo,
    Redo,
    Settings,
    OpenFile,
    SaveFileAs,
    FileOpened(Option<PathBuf>),
    SavePathChosen(Option<PathBuf>),
    FileDropped(PathBuf),
    OpenRecent(usize),
    VisitWebsite,
    NameChanged(String),
    SaveSettings,
    CancelSettings,
    Dismiss(ToastId),
    Retry,
    ExpireToasts,
}

const STATE_VERSION: u32 = 2;
const RECENT_CAPACITY: usize = 8;
const WEBSITE_URL: &str = "https://github.com/hxyulin/rxui";

/// Persisted shell state. Version 1 stored a bare [`WindowPlacement`];
/// version 2 wraps it and adds the recent-documents list. Stale version-1
/// files fail the version check on load and are simply discarded, which is
/// acceptable for an example.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedShellState {
    placement: Option<WindowPlacement>,
    recents: RecentDocuments,
}

fn primary_shift(key: &str) -> Shortcut {
    let mut shortcut = Shortcut::primary(key);
    shortcut.modifiers.shift = true;
    shortcut
}

fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn text_dialog_options(title: &str) -> FileDialogOptions {
    FileDialogOptions::new()
        .title(title)
        .filter("Text", &["txt", "md"])
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
    commands: CommandRegistry<Message>,
    router: CommandRouter,
    services: DesktopServices,
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
    recents: RecentDocuments,
    recents_panel: Option<ElementHandle<Column>>,
    recents_list: Option<ElementHandle<Column>>,
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
        commands
            .register(
                Command::new(
                    CommandId::new("file.open").unwrap(),
                    "Open File…",
                    Message::OpenFile,
                )
                .shortcut(Shortcut::primary("o")),
            )
            .unwrap();
        commands
            .register(
                Command::new(
                    CommandId::new("file.save-as").unwrap(),
                    "Save As…",
                    Message::SaveFileAs,
                )
                .shortcut(primary_shift("s")),
            )
            .unwrap();
        commands
            .register(
                Command::new(
                    CommandId::new("help.website").unwrap(),
                    "Visit Website",
                    Message::VisitWebsite,
                )
                .shortcut(primary_shift("h")),
            )
            .unwrap();
        let state_store = JsonStateStore::for_app("dev", "RXUI", "ApplicationShell").ok();
        // Missing, unreadable, or stale-version state (including old
        // version-1 placement-only files) falls back to the defaults below.
        let saved_state: Option<PersistedShellState> = state_store
            .as_ref()
            .and_then(|store| store.load(STATE_VERSION).ok().flatten());
        let (saved_placement, recents) = saved_state.map_or_else(
            || (None, RecentDocuments::new(RECENT_CAPACITY)),
            |state| (state.placement, state.recents),
        );
        let placement = saved_placement.clone().map_or_else(
            WindowPlacementTracker::default,
            WindowPlacementTracker::from_placement,
        );
        Self {
            commands,
            router: CommandRouter::new(),
            services: DesktopServices::native(),
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
            recents,
            recents_panel: None,
            recents_list: None,
            state_store,
        }
    }

    fn refresh_shell(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        sync_undo_commands(&mut self.commands, &self.undo);
        let ui = cx.source_ui()?;
        ui.set_label_text(
            self.value_label.expect("value label"),
            format!("Undoable value: {}", self.value),
        )?;
        self.toolbar
            .as_ref()
            .expect("toolbar")
            .sync(ui, &self.commands)?;
        Ok(())
    }

    fn sync_toasts(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let ui = cx.source_ui()?;
        self.toast_host.as_mut().expect("toast host").sync(
            ui,
            &self.toast_queue,
            Message::Dismiss,
        )?;
        Ok(())
    }

    fn notify(&mut self, cx: &mut AppCx<'_, Message>, toast: Toast<Message>) -> rxui::Result<()> {
        let now = cx.now();
        self.toast_queue.push(toast, now);
        self.sync_toasts(cx)?;
        if let Some(deadline) = self.toast_queue.next_deadline() {
            cx.set_timeout(
                deadline.saturating_duration_since(now),
                Message::ExpireToasts,
            );
        }
        Ok(())
    }

    fn open_settings(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let ui = cx.source_ui()?;
        let mut binding = None;
        self.dialogs.show(
            ui,
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
        )?;
        self.name_binding = binding;
        Ok(())
    }

    /// Rebuilds the sidebar list of recent files, most recent first.
    fn refresh_recents(&mut self, ui: &mut Ui<Message>) -> rxui::Result<()> {
        let panel = self.recents_panel.expect("recents panel");
        if let Some(list) = self.recents_list.take() {
            ui.remove(list)?;
        }
        let list = ui.add_column(panel)?;
        if self.recents.is_empty() {
            ui.add_label(list, "No recent files yet")?;
        }
        for (index, path) in self.recents.iter().enumerate() {
            let button = ui.add_button(list, file_label(path))?;
            ui.listen(button, None, EventFilter::Activate, move |context, _| {
                context.emit(Message::OpenRecent(index))
            })?;
        }
        self.recents_list = Some(list);
        Ok(())
    }

    /// Records a file use: bumps it in the recents list, refreshes the
    /// sidebar, and announces the action as a toast.
    fn record_recent(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        path: PathBuf,
        toast: Toast<Message>,
    ) -> rxui::Result<()> {
        self.recents.touch(path);
        self.refresh_recents(cx.source_ui()?)?;
        self.notify(cx, toast)
    }

    fn save_state(&self) {
        if let Some(store) = &self.state_store {
            let _ = store.save(
                STATE_VERSION,
                &PersistedShellState {
                    placement: self.placement.placement().cloned(),
                    recents: self.recents.clone(),
                },
            );
        }
    }
}

impl App for Shell {
    type Message = Message;

    fn build(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let mut ui = cx.new_ui();
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
                    id: CommandId::new("file.open").unwrap(),
                    icon: Some(icons::folder()),
                },
                ToolbarItem::Command {
                    id: CommandId::new("file.save-as").unwrap(),
                    icon: Some(icons::save()),
                },
                ToolbarItem::Separator,
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
        )?;
        toolbar.update_overflow(&mut ui, 760.0)?;
        let body = ui
            .row(content)
            .grow(1.0)
            .flex(FlexStyle {
                column_gap: 16.0,
                align_items: Alignment::Stretch,
                ..Default::default()
            })
            .finish();
        let sidebar = ui.column(body).width(Length::px(200.0)).finish();
        ui.add_label(sidebar, "Recent files")?;
        self.recents_panel = Some(sidebar);
        self.refresh_recents(&mut ui)?;
        let main = ui.column(body).grow(1.0).finish();
        ui.add_label(main, "RXUI application shell")?;
        let value_label = ui.add_label(main, "Undoable value: 0")?;
        ui.add_label(main, "Resize the window to exercise toolbar overflow. Open Settings to see a validated modal. Drop a file anywhere to add it to the recents sidebar.")?;
        let toast_host = ToastHost::new(&mut ui)?;
        let mut attributes = WindowAttributes {
            title: "RXUI application shell".into(),
            inner_size: Some(Size::new(820.0, 480.0)),
            ..Default::default()
        };
        if let Some(saved) = &self.saved_placement {
            let monitors = cx.available_monitors();
            let primary = cx.primary_monitor();
            saved.apply(&mut attributes, &monitors, primary.as_ref());
        }
        self.toolbar = Some(toolbar);
        self.toast_host = Some(toast_host);
        self.value_label = Some(value_label);
        cx.open_window(WindowConfig::default().attributes(attributes), ui)?;
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        match message {
            Message::Increment => {
                self.undo
                    .execute(ChangeValue { amount: 1 }, &mut self.value)?;
                self.refresh_shell(cx)?;
                let mut toast = Toast::new(
                    ToastLevel::Success,
                    "Value changed",
                    "Use Undo to restore it, or run the sample action.",
                );
                toast.action = Some(ToastAction {
                    label: "Retry".into(),
                    message: Message::Retry,
                });
                self.notify(cx, toast)?;
            }
            Message::Undo => {
                self.undo.undo(&mut self.value)?;
                self.refresh_shell(cx)?;
            }
            Message::Redo => {
                self.undo.redo(&mut self.value)?;
                self.refresh_shell(cx)?;
            }
            Message::Settings => self.open_settings(cx)?,
            Message::OpenFile => {
                // Delivery runs off the UI thread; route the result back into
                // `update` through a message proxy.
                let proxy = cx.proxy();
                self.services
                    .pick_file(text_dialog_options("Open File"), move |path| {
                        let _ = proxy.post(Message::FileOpened(path));
                    });
            }
            Message::SaveFileAs => {
                let proxy = cx.proxy();
                self.services.save_file(
                    text_dialog_options("Save As").file_name("untitled.txt"),
                    move |path| {
                        let _ = proxy.post(Message::SavePathChosen(path));
                    },
                );
            }
            Message::FileOpened(Some(path)) => {
                let toast = Toast::new(
                    ToastLevel::Success,
                    "File opened",
                    format!("Opened {}.", file_label(&path)),
                );
                self.record_recent(cx, path, toast)?;
            }
            Message::FileOpened(None) => self.notify(
                cx,
                Toast::new(ToastLevel::Info, "Open cancelled", "No file was chosen."),
            )?,
            Message::SavePathChosen(Some(path)) => {
                let toast = Toast::new(
                    ToastLevel::Success,
                    "File saved",
                    format!("Saved {}.", file_label(&path)),
                );
                self.record_recent(cx, path, toast)?;
            }
            Message::SavePathChosen(None) => self.notify(
                cx,
                Toast::new(
                    ToastLevel::Info,
                    "Save cancelled",
                    "No destination was chosen.",
                ),
            )?,
            Message::FileDropped(path) => {
                let toast = Toast::new(
                    ToastLevel::Info,
                    "File dropped",
                    format!("Added {} to recent files.", file_label(&path)),
                );
                self.record_recent(cx, path, toast)?;
            }
            Message::OpenRecent(index) => {
                let recent = self.recents.iter().nth(index).map(Path::to_path_buf);
                if let Some(path) = recent {
                    let toast = Toast::new(
                        ToastLevel::Success,
                        "File opened",
                        format!("Opened {}.", file_label(&path)),
                    );
                    self.record_recent(cx, path, toast)?;
                }
            }
            Message::VisitWebsite => {
                let toast = match self.services.open_url(WEBSITE_URL) {
                    Ok(()) => Toast::new(
                        ToastLevel::Info,
                        "Opening website",
                        format!("{WEBSITE_URL} opens in your browser."),
                    ),
                    Err(error) => Toast::new(
                        ToastLevel::Error,
                        "Website failed",
                        format!("Could not open {WEBSITE_URL}: {error}"),
                    ),
                };
                self.notify(cx, toast)?;
            }
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
                if let Some(binding) = &self.name_binding {
                    binding.sync(cx.source_ui()?, self.validation.visible_result(&"name"))?;
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
                if let Some(binding) = &self.name_binding {
                    let ui = cx.source_ui()?;
                    binding.sync(ui, self.validation.visible_result(&"name"))?;
                    if !valid {
                        binding.focus(ui)?;
                    }
                }
                if valid {
                    self.dialogs.close(cx.source_ui()?)?;
                    self.name_binding = None;
                    self.notify(
                        cx,
                        Toast::new(
                            ToastLevel::Info,
                            "Settings saved",
                            format!("Hello, {}!", self.name),
                        ),
                    )?;
                }
            }
            Message::CancelSettings => {
                self.dialogs.close(cx.source_ui()?)?;
                self.name_binding = None;
            }
            Message::Dismiss(id) => {
                self.toast_queue.dismiss(id, cx.now());
                self.sync_toasts(cx)?;
            }
            Message::Retry => self.notify(
                cx,
                Toast::new(
                    ToastLevel::Success,
                    "Retry complete",
                    "The action succeeded.",
                ),
            )?,
            Message::ExpireToasts => {
                self.toast_queue.expire(cx.now());
                self.sync_toasts(cx)?;
            }
        }
        Ok(())
    }

    fn window_event(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        window: WindowId,
        event: &WindowEvent,
    ) -> rxui::Result<()> {
        // Resolve registered keyboard shortcuts. Toolbar and dialog clicks
        // arrive as their own messages, so the router never double-dispatches.
        if let Some(message) = self.router.handle_event(event, &self.commands) {
            cx.post(message);
        }
        self.placement.handle_event(cx.window(window)?, event);
        if let WindowEvent::DroppedFile(path) = event {
            cx.post(Message::FileDropped(path.clone()));
        }
        if let WindowEvent::Resized(size) = event {
            let width = size.width as f32 / cx.window(window)?.scale_factor() as f32;
            self.toolbar
                .as_ref()
                .expect("toolbar")
                .update_overflow(cx.ui(window)?, width - 40.0)?;
        }
        Ok(())
    }

    fn close_requested(
        &mut self,
        cx: &mut AppCx<'_, Message>,
        window: WindowId,
    ) -> rxui::Result<CloseResponse> {
        self.placement.capture(cx.window(window)?);
        Ok(CloseResponse::Close)
    }

    fn window_closed(
        &mut self,
        _cx: &mut AppCx<'_, Message>,
        _window: WindowId,
    ) -> rxui::Result<()> {
        self.save_state();
        Ok(())
    }

    fn exiting(&mut self, _cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        self.save_state();
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> MainResult {
    run_with(Shell::new(), AppConfig::default().theme(Theme::dark()))
}

#[cfg(target_arch = "wasm32")]
fn main() {}
