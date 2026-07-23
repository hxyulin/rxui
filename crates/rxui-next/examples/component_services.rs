//! Headless clipboard, task, and undo service example.

use std::num::NonZeroUsize;

use astrelis_core::geometry::LogicalSize;
use rxui_next::{
    Component, ComponentContext, ComponentHost, MemoryClipboard, Theme, UndoHistory, View, button,
};

#[derive(Clone)]
enum Action {
    Copy,
    Paste,
    Pasted(Option<String>),
    Calculate,
    Calculated(u64),
    Edit(String),
    Undo,
    Redo,
}

struct ServiceDemo {
    document: String,
    result: Option<u64>,
    history: UndoHistory<String>,
}

impl Component for ServiceDemo {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Action, context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Copy => context.write_clipboard(self.document.clone()),
            Action::Paste => context.read_clipboard(Action::Pasted),
            Action::Pasted(Some(text)) | Action::Edit(text) => {
                self.history.checkpoint(self.document.clone());
                self.document = text;
            }
            Action::Pasted(None) => {}
            Action::Calculate => context.spawn(|| (1..=20).product(), Action::Calculated),
            Action::Calculated(result) => self.result = Some(result),
            Action::Undo => {
                self.history.undo(&mut self.document);
            }
            Action::Redo => {
                self.history.redo(&mut self.document);
            }
        }
    }

    fn view(&self, _theme: &Theme) -> View<Action> {
        button("Copy", Action::Copy)
    }
}

fn main() {
    let mut host = ComponentHost::new(
        ServiceDemo {
            document: "Astrelis".into(),
            result: None,
            history: UndoHistory::new(NonZeroUsize::new(32).unwrap()),
        },
        LogicalSize::new(240.0, 80.0),
        Theme::dark(),
    )
    .unwrap();
    let mut clipboard = MemoryClipboard::default();

    host.dispatch(Action::Copy).unwrap();
    host.dispatch(Action::Edit("RXUI Next".into())).unwrap();
    host.dispatch(Action::Paste).unwrap();
    host.dispatch(Action::Calculate).unwrap();
    host.run_pending_services(&mut clipboard).unwrap();

    assert_eq!(clipboard.text(), Some("Astrelis"));
    assert_eq!(host.component().document, "Astrelis");
    assert_eq!(host.component().result, Some(2_432_902_008_176_640_000));

    host.dispatch(Action::Undo).unwrap();
    assert_eq!(host.component().document, "RXUI Next");
    host.dispatch(Action::Redo).unwrap();
    assert_eq!(host.component().document, "Astrelis");
}
