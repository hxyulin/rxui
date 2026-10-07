//! Standalone single-line document workflow with native dialogs and desktop services.
//! Run: cargo run -p rxui --example document_window --features native-menus,native-dialogs,desktop-services
use rxui::prelude::*;
use std::{cell::RefCell, error::Error, path::PathBuf, rc::Rc};

struct Open;
impl Command for Open {}
struct Save;
impl Command for Save {}
struct SaveAs;
impl Command for SaveAs {}
#[derive(Clone, Copy)]
enum Intent {
    Open,
    Close,
    Quit,
}
struct Document {
    text: String,
    saved: String,
    path: Option<PathBuf>,
    window: Option<WindowHandle>,
    status: String,
    dialog: Option<DialogTask>,
    io: Option<Task>,
    service: Option<Task>,
}
impl Document {
    fn dirty(&self) -> bool {
        self.text != self.saved
    }
    fn busy(&self) -> bool {
        self.dialog.as_ref().is_some_and(|d| !d.is_finished())
            || self.io.as_ref().is_some_and(|t| !t.is_finished())
    }
    fn title(&self) {
        if let Some(window) = self.window.as_ref().and_then(WindowHandle::native_window) {
            let name = self
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".into());
            window.set_title(&format!(
                "{name}{} — RXUI",
                if self.dirty() { " *" } else { "" }
            ));
        }
    }
    fn intent(&mut self, intent: Intent, cx: &mut Context<'_, Self>) {
        if self.busy() {
            self.status = "Finish the current operation first.".into();
            return;
        }
        if !self.dirty() {
            self.finish_intent(intent, cx);
            return;
        }
        let options = MessageDialog::new()
            .parent(self.window.as_ref().unwrap())
            .title("Save changes?")
            .description("Save the changes to this document before continuing?")
            .level(MessageLevel::Warning)
            .buttons(MessageButtons::YesNoCancel);
        match cx.show_message(options, move |s, result, cx| {
            s.dialog = None;
            match result {
                Ok(MessageResponse::Yes) => s.save(false, Some(intent), cx),
                Ok(MessageResponse::No) => s.finish_intent(intent, cx),
                Ok(_) => s.status = "Cancelled; your changes are retained.".into(),
                Err(error) => s.status = error.to_string(),
            }
        }) {
            Ok(task) => {
                self.dialog = Some(task);
                self.status = "Waiting for your decision…".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
    fn finish_intent(&mut self, intent: Intent, cx: &mut Context<'_, Self>) {
        match intent {
            Intent::Open => self.open(cx),
            Intent::Close => {
                if let Some(window) = &self.window
                    && let Err(error) = cx.close_window(window)
                {
                    self.status = error.to_string();
                }
            }
            Intent::Quit => {
                if let Err(error) = cx.exit() {
                    self.status = error.to_string();
                }
            }
        }
    }
    fn open(&mut self, cx: &mut Context<'_, Self>) {
        let mut options = FileDialog::new()
            .parent(self.window.as_ref().unwrap())
            .title("Open a single-line document")
            .filter("Text", ["txt"]);
        if let Some(directory) = self.path.as_ref().and_then(|p| p.parent()) {
            options = options.directory(directory);
        }
        match cx.pick_file(options, |s, selection, cx| {
            s.dialog = None;
            match selection {
                Ok(Some(path)) => {
                    s.status = "Reading document…".into();
                    s.io = Some(cx.spawn_blocking(
                        move || {
                            let text = std::fs::read_to_string(&path)?;
                            // This example uses the current single-line input control.
                            // Reject multiline data rather than silently modifying it.
                            if text.contains(['\n', '\r', '\t']) {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "This example edits single-line text files only.",
                                ));
                            }
                            Ok::<_, std::io::Error>((path, text))
                        },
                        |s, result, _| {
                            s.io = None;
                            match result {
                                Ok(Ok((path, text))) => {
                                    s.path = Some(path);
                                    s.saved = text.clone();
                                    s.text = text;
                                    s.status = "Document opened.".into();
                                    s.title();
                                }
                                Ok(Err(error)) => s.status = error.to_string(),
                                Err(error) => s.status = error.to_string(),
                            }
                        },
                    ));
                }
                Ok(None) => s.status = "Open cancelled; the current document is retained.".into(),
                Err(error) => s.status = error.to_string(),
            }
        }) {
            Ok(task) => {
                self.dialog = Some(task);
                self.status = "Choose a document…".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
    fn save(&mut self, choose_path: bool, then: Option<Intent>, cx: &mut Context<'_, Self>) {
        if self.busy() {
            self.status = "Finish the current operation first.".into();
            return;
        }
        if !choose_path && let Some(path) = &self.path {
            self.write(path.clone(), then, cx);
            return;
        }
        let mut options = FileDialog::new()
            .parent(self.window.as_ref().unwrap())
            .title("Save document")
            .file_name("document.txt")
            .filter("Text", ["txt"]);
        if let Some(path) = &self.path {
            if let Some(directory) = path.parent() {
                options = options.directory(directory);
            }
            if let Some(name) = path.file_name() {
                options = options.file_name(name.to_string_lossy());
            }
        }
        match cx.save_file(options, move |s, selection, cx| {
            s.dialog = None;
            match selection {
                Ok(Some(path)) => s.write(path, then, cx),
                Ok(None) => s.status = "Save cancelled; your changes are retained.".into(),
                Err(error) => s.status = error.to_string(),
            }
        }) {
            Ok(task) => {
                self.dialog = Some(task);
                self.status = "Choose where to save…".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
    fn write(&mut self, path: PathBuf, then: Option<Intent>, cx: &mut Context<'_, Self>) {
        let text = self.text.clone();
        self.status = "Writing document…".into();
        self.io = Some(cx.spawn_blocking(move || {
            std::fs::write(&path, text.as_bytes())?;
            Ok::<_, std::io::Error>((path, text))
        }, move |s, result, cx| {
            s.io = None;
            match result {
                Ok(Ok((path, saved))) => {
                    s.path = Some(path); s.saved = saved; s.title(); s.status = "Document saved.".into();
                    if let Some(intent) = then {
                        if s.dirty() { s.status = "The document changed during saving; review it before continuing.".into(); }
                        else { s.finish_intent(intent, cx); }
                    }
                }
                Ok(Err(error)) => s.status = error.to_string(),
                Err(error) => s.status = error.to_string(),
            }
        }));
    }
}
impl View for Document {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let open = cx
            .command(Open, |s, _, cx| s.intent(Intent::Open, cx))
            .label("Open…")
            .enabled(!self.busy())
            .shortcut(Shortcut::primary("o"));
        let save = cx
            .command(Save, |s, _, cx| s.save(false, None, cx))
            .label("Save")
            .enabled(!self.busy() && self.dirty())
            .shortcut(Shortcut::primary("s"));
        let save_as = cx
            .command(SaveAs, |s, _, cx| s.save(true, None, cx))
            .label("Save As…")
            .enabled(!self.busy())
            .shortcut(Shortcut::primary("s").shift());
        column()
            .fill_width()
            .fill_height()
            .padding(24.)
            .gap(14.)
            .on_command(open.clone())
            .on_command(save.clone())
            .on_command(save_as.clone())
            .child(label("Native document workflow").font_size(28.))
            .child(label(
                "Single-line text documents. Open and Save use native dialogs.",
            ))
            .child(
                row()
                    .gap(10.)
                    .child(open.button())
                    .child(save.button())
                    .child(save_as.button())
                    .child(button("Close").on_click(cx.listener(|_, _, cx| {
                        if let Some(window) = cx.window() {
                            cx.request_close(&window).unwrap();
                        }
                    }))),
            )
            .child(
                text_input(self.text.clone())
                    .key("document")
                    .fill_width()
                    .disabled(self.busy())
                    .on_change(cx.listener(|s, event: &TextChangeEvent, _| {
                        s.text = event.value.clone();
                        s.status = "Document changed.".into();
                        s.title();
                    })),
            )
            .child(label(
                self.path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Untitled document".into()),
            ))
            .child(label(if self.dirty() {
                "Unsaved changes"
            } else {
                "Saved"
            }))
            .child(
                row()
                    .gap(10.)
                    .child(button("Copy Text").on_click(cx.listener(|s, _, cx| {
                        s.status = match cx.write_clipboard_text(s.text.clone()) {
                            Ok(()) => "Copied document text.".into(),
                            Err(error) => error.to_string(),
                        };
                    })))
                    .child(button("Read Clipboard").on_click(cx.listener(|s, _, cx| {
                        s.status = match cx.read_clipboard_text() {
                            Ok(text) => format!("Clipboard has {} bytes of text.", text.len()),
                            Err(error) => error.to_string(),
                        };
                    })))
                    .child(
                        button("Reveal File")
                            .disabled(self.path.is_none())
                            .on_click(cx.listener(|s, _, cx| {
                                if let Some(path) = &s.path {
                                    match cx.reveal_file(path.clone(), |s, result, _| {
                                        s.service = None;
                                        s.status = match result {
                                            Ok(()) => "File manager request accepted.".into(),
                                            Err(error) => error.to_string(),
                                        };
                                    }) {
                                        Ok(task) => {
                                            s.service = Some(task);
                                            s.status = "Opening file manager…".into();
                                        }
                                        Err(error) => s.status = error.to_string(),
                                    }
                                }
                            })),
                    ),
            )
            .child(label(self.status.clone()))
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    use rxui::standard_commands::*;
    let document: Rc<RefCell<Option<rxui::WeakEntity<Document>>>> = Rc::default();
    let closing = document.clone();
    let quitting = document.clone();
    let created = document.clone();
    let menus = NativeMenuBar::new()
        .menu(
            NativeMenu::new("RXUI")
                .role(NativeMenuRole::About)
                .separator()
                .command::<Quit>("Quit"),
        )
        .menu(
            NativeMenu::new("File")
                .command::<Open>("Open…")
                .command::<Save>("Save")
                .command::<SaveAs>("Save As…")
                .separator()
                .command::<CloseWindow>("Close Window"),
        )
        .menu(
            NativeMenu::new("Edit")
                .command::<Undo>("Undo")
                .command::<Redo>("Redo")
                .separator()
                .command::<Cut>("Cut")
                .command::<Copy>("Copy")
                .command::<Paste>("Paste")
                .separator()
                .command::<SelectAll>("Select All"),
        );
    Application::new()
        .menu_bar(menus)
        .window_created(move |_, cx| {
            if let Some(document) = created.borrow().as_ref().and_then(|e| e.upgrade()) {
                document.read(cx).title();
            }
        })
        .close_requested(move |_, cx| {
            if let Some(document) = closing.borrow().as_ref().and_then(|e| e.upgrade()) {
                document.update(cx, |s, cx| s.intent(Intent::Close, cx));
                CloseResponse::KeepOpen
            } else {
                CloseResponse::Close
            }
        })
        .quit_requested(move |cx| {
            if let Some(document) = quitting.borrow().as_ref().and_then(|e| e.upgrade()) {
                document.update(cx, |s, cx| s.intent(Intent::Quit, cx));
                CloseResponse::KeepOpen
            } else {
                CloseResponse::Close
            }
        })
        .run(|cx| {
            let root = cx.new(|_| Document {
                text: String::new(),
                saved: String::new(),
                path: None,
                window: None,
                status: "Ready.".into(),
                dialog: None,
                io: None,
                service: None,
            });
            *document.borrow_mut() = Some(root.downgrade());
            let window = cx.open_window(
                WindowOptions::new()
                    .title("Untitled — RXUI")
                    .size(820., 430.),
                root.clone(),
            )?;
            root.update(cx, |s, _| s.window = Some(window));
            Ok(())
        })?;
    Ok(())
}
