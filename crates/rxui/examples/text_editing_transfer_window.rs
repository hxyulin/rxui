//! Explicit committed editing transfer from an ordinary app event handler.
//! Split/merge remounts the field. Focus remains an explicit separate action.
use rxui::prelude::*;

struct Editor {
    value: String,
    version: u64,
    tree: DockTree,
    handle: TextEditingHandle,
    ticket: Option<TextEditingCapture>,
    focus: FocusHandle,
    status: String,
}
impl View for Editor {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut field = text_area(self.value.clone())
            .key("editor")
            .fill_width()
            .text_editing_identity(TextEditingIdentity::new("example-document", self.version))
            .text_editing_handle(self.handle.clone())
            .focus_handle(self.focus.clone())
            .on_change(cx.listener(|s, e: &TextChangeEvent, _| {
                s.value = e.value.clone();
                s.version += 1;
            }));
        if let Some(ticket) = &self.ticket {
            field = field.restore_text_editing(ticket.clone());
        }
        column().padding(20.).gap(12.)
            .child(label("Type, select text, then split or merge. Undo history and selection transfer."))
            .child(row().gap(8.)
                .child(button("Split / merge editor").on_click(cx.listener(|s, _, _| {
                    match s.handle.capture() {
                        Ok(ticket) => {
                            s.ticket = Some(ticket);
                            if s.tree.group_for(&"other".into()).is_some() {
                                s.tree.remove(&"other".into());
                            } else {
                                let group = s.tree.root().id();
                                s.tree.split(group, DockSide::Right, "other", SplitPosition::Fraction(0.5)).unwrap();
                            }
                            s.status = "Capture queued before teardown; replacement restores before layout.".into();
                        }
                        Err(error) => s.status = error.to_string(),
                    }
                })))
                .child(button("Focus editor").on_click(cx.listener(|s, _, cx| {
                    if let Err(error) = s.focus.focus(cx) { s.status = error.to_string(); }
                })))
                .child(button("Inspect result").on_click(cx.listener(|s, _, _| {
                    s.status = match &s.ticket {
                        Some(ticket) => format!("Capture: {:?}; restores: {:?}", ticket.snapshot(), ticket.restore_results()),
                        None => "No transfer requested".into(),
                    };
                }))))
            .child(label(self.status.clone()))
            .child(dock(&self.tree, |key| dock_panel(
                format!("{key:?}"),
                if *key == "editor".into() { field.clone() } else { label("Independent placement; layout rebuilt normally") },
            )).height(200.))
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let root = cx.new(|_| Editor {
            value: "Committed editing travels only when the application requests it.\nTry Undo after moving.".into(),
            version: 0,
            tree: DockTree::from_panels(["editor"]).unwrap(),
            handle: TextEditingHandle::new(), ticket: None,
            focus: FocusHandle::new(), status: String::new(),
        });
        cx.open_window(WindowOptions::new().title("RXUI — explicit editing transfer").size(900., 500.), root)?;
        Ok(())
    })
}
