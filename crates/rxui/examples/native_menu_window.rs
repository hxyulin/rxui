//! Native menu bars with live scoped commands, editing and vetoable lifecycle.
//! Run: cargo run -p rxui --example native_menu_window --features native-menus
use rxui::prelude::*;
use std::{cell::RefCell, collections::HashMap, error::Error, rc::Rc};

struct Save;
impl Command for Save {}
struct NewWindow;
impl Command for NewWindow {}
struct SaveAll;
impl Command for SaveAll {}

type Editors = Rc<RefCell<HashMap<rxui::WindowId, rxui::WeakEntity<Editor>>>>;
struct Editor {
    text: String,
    saved: String,
    saves: usize,
    status: String,
}
impl Editor {
    fn dirty(&self) -> bool {
        self.text != self.saved
    }
    fn save(&mut self) {
        self.saved = self.text.clone();
        self.saves += 1;
        self.status = format!("Saved {} time(s).", self.saves);
    }
}
impl View for Editor {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let save = cx
            .command(Save, |s, _, _| s.save())
            .label(if self.dirty() {
                "Save Document"
            } else {
                "Save Document (saved)"
            })
            .enabled(self.dirty())
            .shortcut(Shortcut::primary("s"));
        column()
            .fill_width()
            .fill_height()
            .padding(24.)
            .gap(16.)
            .on_command(save.clone())
            .child(label("Native menus and scoped commands").font_size(26.))
            .child(label(
                "Use File → New Window to test independent documents and focused-window routing.",
            ))
            .child(label(
                "Edit → Cut / Copy / Paste / Select All uses this RXUI input.",
            ))
            .child(
                text_input(self.text.clone())
                    .key("document")
                    .fill_width()
                    .on_change(cx.listener(|s, e: &TextChangeEvent, _| {
                        s.text = e.value.clone();
                        s.status = "Document changed.".into();
                    })),
            )
            .child(
                row()
                    .gap(12.)
                    .child(save.button())
                    .child(button("Request Close").on_click(cx.listener(|_, _, cx| {
                        if let Some(window) = cx.window() {
                            cx.request_close(&window).unwrap();
                        }
                    }))),
            )
            .child(label(if self.dirty() {
                "Unsaved — Close and Quit are vetoed until you save."
            } else {
                "Saved — Close and Quit are allowed."
            }))
            .child(label(self.status.clone()))
    }
}
fn open_editor(editors: &Editors, cx: &mut rxui::AppContext<'_>) -> Result<(), ApplicationError> {
    let editor = cx.new(|_| Editor {
        text: "Select me, then use the Edit menu.".into(),
        saved: "Select me, then use the Edit menu.".into(),
        saves: 0,
        status: "Ready.".into(),
    });
    let weak = editor.downgrade();
    let window = cx.open_window(
        WindowOptions::new()
            .title("RXUI — native menu document")
            .size(760., 390.),
        editor,
    )?;
    editors.borrow_mut().retain(|_, e| e.upgrade().is_some());
    editors.borrow_mut().insert(window.id(), weak);
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    use rxui::standard_commands::*;
    let editors: Editors = Rc::default();
    let closing = editors.clone();
    let quitting = editors.clone();
    let mut registrations = Vec::new();
    let menu = NativeMenuBar::new()
        .menu(
            NativeMenu::new("RXUI")
                .role(NativeMenuRole::About)
                .separator()
                .role(NativeMenuRole::Services)
                .separator()
                .role(NativeMenuRole::Hide)
                .role(NativeMenuRole::HideOthers)
                .role(NativeMenuRole::ShowAll)
                .separator()
                .command::<Quit>("Quit"),
        )
        .menu(
            NativeMenu::new("File")
                .command::<NewWindow>("New Window")
                .command::<Save>("Save Document")
                .separator()
                .submenu(NativeMenu::new("Documents").command::<SaveAll>("Save All Documents"))
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
        .menu_bar(menu)
        .close_requested(move |window, cx| {
            let editor = closing.borrow().get(&window.id()).and_then(|e| e.upgrade());
            if let Some(editor) = editor
                && editor.read(cx).dirty()
            {
                editor.update(cx, |s, _| {
                    s.status = "Close deferred: save this document first.".into()
                });
                CloseResponse::KeepOpen
            } else {
                CloseResponse::Close
            }
        })
        .quit_requested(move |cx| {
            let live: Vec<_> = quitting
                .borrow()
                .values()
                .filter_map(|e| e.upgrade())
                .collect();
            let dirty = live.iter().any(|e| e.read(cx).dirty());
            if dirty {
                for editor in live {
                    editor.update(cx, |s, _| {
                        s.status =
                            "Quit deferred: use File → Documents → Save All Documents.".into()
                    });
                }
                CloseResponse::KeepOpen
            } else {
                CloseResponse::Close
            }
        })
        .run(|cx| {
            let new_editors = editors.clone();
            let new = cx
                .command(NewWindow, move |_, cx| {
                    open_editor(&new_editors, cx).unwrap();
                })
                .label("New Window")
                .shortcut(Shortcut::primary("n"));
            registrations.push(cx.register_command(&new));
            let save_editors = editors.clone();
            let save_all = cx
                .command(SaveAll, move |_, cx| {
                    let live: Vec<_> = save_editors
                        .borrow()
                        .values()
                        .filter_map(|e| e.upgrade())
                        .collect();
                    for editor in live {
                        editor.update(cx, |s, _| s.save());
                    }
                })
                .label("Save All Documents")
                .shortcut(Shortcut::primary("s").shift());
            registrations.push(cx.register_command(&save_all));
            open_editor(&editors, cx)
        })?;
    Ok(())
}
