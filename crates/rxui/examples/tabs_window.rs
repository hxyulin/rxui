//! Controlled tabs, retained panel state, placement-local focus and shared models.
use rxui::prelude::*;

struct Document {
    id: u32,
    name: String,
    note: String,
    focus: FocusHandle,
    scroll: ScrollHandle,
    cycle: bool,
}
impl View for Document {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let name = text_input(self.name.clone())
            .fill_width()
            .accessibility_label("Document name")
            .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.name = e.value.clone()));
        let note = text_input(self.note.clone())
            .fill_width()
            .accessibility_label("Document note")
            .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.note = e.value.clone()));
        let cycle = button(if self.cycle {
            "Focus cycle on — Escape to release"
        } else {
            "Cycle focus within this document"
        })
        .variant(if self.cycle {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Default
        })
        .on_click(cx.listener(|s, _, _| s.cycle = !s.cycle));
        let rows = column().fill_width().gap(4.).children(
            (0..100).map(|i| label(format!("Document {} — retained row {i:03}", self.id)).key(i)),
        );
        let hint = label(
            "Switch tabs to retain selection and scrolling. Arrow keys select headers; Delete proposes closing a tab.",
        )
        .color(ThemeColor::TextMuted);
        let mut panel = column()
            .fill_width()
            .fill_height()
            .min_height(0.)
            .padding(16.)
            .gap(12.)
            .focus_handle(self.focus.clone())
            .focus_scope(if self.cycle {
                FocusScope::Cycle
            } else {
                FocusScope::Group
            })
            .child(label(format!("Document {}", self.id)).font_size(24.))
            .child(name)
            .child(note)
            .child(cycle)
            .child(hint)
            .child(scroll_area(rows).handle(self.scroll.clone()));
        if self.cycle {
            panel = panel.on_key_down_capture(cx.listener(|s, e: &KeyInput, _| {
                if e.event.key == KeyboardKey::Escape {
                    s.cycle = false;
                    e.prevent_default();
                }
            }));
        }
        panel
    }
}
struct Workspace {
    documents: Vec<Entity<Document>>,
    selected: Option<Key>,
    next_id: u32,
    policy: TabContentPolicy,
}
impl Workspace {
    fn add(&mut self, cx: &mut Context<'_, Self>) {
        let id = self.next_id;
        self.next_id += 1;
        self.documents.push(cx.new(|_| Document {
            id,
            name: format!("Document {id}"),
            note: "Edit this note; each window keeps its own caret.".into(),
            focus: FocusHandle::new(),
            scroll: ScrollHandle::new(),
            cycle: false,
        }));
        self.selected = Some(id.into());
    }
}
impl View for Workspace {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut group = tabs()
            .key("documents")
            .content_policy(self.policy)
            .tabs(self.documents.iter().map(|doc| {
                let state = doc.read(cx);
                tab(state.id, state.name.clone(), doc.clone()).closable(true)
            }))
            .on_select(cx.listener(|s, e: &TabSelectEvent, _| s.selected = Some(e.key.clone())))
            .on_close(cx.listener(|s, e: &TabCloseEvent, cx| {
                s.documents
                    .retain(|doc| Key::from(doc.read(cx).id) != e.key);
                s.selected = e.next_selection.clone();
            }));
        if let Some(key) = &self.selected {
            group = group.selected(key.clone());
        }
        column()
            .fill_width()
            .fill_height()
            .min_height(0.)
            .child(
                row()
                    .padding(12.)
                    .gap(8.)
                    .child(button("New document").on_click(cx.listener(|s, _, cx| s.add(cx))))
                    .child(
                        button("Restore document focus").on_click(cx.listener(|s, _, cx| {
                            if let Some(doc) = s
                                .documents
                                .iter()
                                .find(|doc| Some(Key::from(doc.read(cx).id)) == s.selected)
                            {
                                let focus = doc.read(cx).focus.clone();
                                focus.focus(cx).expect("active document placement");
                            }
                        })),
                    )
                    .child(
                        button(if self.policy == TabContentPolicy::KeepMounted {
                            "Panels retained"
                        } else {
                            "Mount selected only"
                        })
                        .on_click(cx.listener(|s, _, _| {
                            s.policy = if s.policy == TabContentPolicy::KeepMounted {
                                TabContentPolicy::MountSelected
                            } else {
                                TabContentPolicy::KeepMounted
                            }
                        })),
                    )
                    .child(button("Shared window").on_click(cx.listener(|_, _, cx| {
                        let root = cx.entity().upgrade().expect("live workspace");
                        cx.open_window(
                            WindowOptions::new()
                                .title("RXUI — shared tabs")
                                .size(900., 650.),
                            root,
                        )
                        .expect("valid window options");
                    }))),
            )
            .child(group)
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let root = cx.new(|_| Workspace {
            documents: Vec::new(),
            selected: None,
            next_id: 0,
            policy: TabContentPolicy::KeepMounted,
        });
        root.update(cx, |s, cx| {
            s.add(cx);
            s.add(cx);
            s.selected = Some(0.into());
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — tabs and focus")
                .size(1040., 720.),
            root,
        )?;
        Ok(())
    })
}
