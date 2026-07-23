//! Typed component service and undo behavior.

use std::num::NonZeroUsize;

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::SemanticAction;
use rxui_next::{
    Component, ComponentContext, ComponentHost, ComponentWithProps, MemoryClipboard, Theme,
    UndoHistory, View, button, component,
};

#[derive(Clone)]
enum Action {
    Start,
    Clipboard(Option<String>),
    Task(i32),
}

struct Services {
    clipboard: Option<String>,
    task: Option<i32>,
}

impl Component for Services {
    type Action = Action;
    type Effect = ();

    fn update(&mut self, action: Action, context: &mut ComponentContext<'_, ()>) {
        match action {
            Action::Start => {
                context.write_clipboard("Astrelis");
                context.read_clipboard(Action::Clipboard);
                context.spawn(|| 6 * 7, Action::Task);
            }
            Action::Clipboard(text) => self.clipboard = text,
            Action::Task(value) => self.task = Some(value),
        }
    }

    fn view(&self, _theme: &Theme) -> View<Action> {
        button("Start", Action::Start)
    }
}

#[test]
fn headless_host_executes_clipboard_and_tasks_deterministically() {
    let mut host = ComponentHost::new(
        Services {
            clipboard: None,
            task: None,
        },
        LogicalSize::new(200.0, 80.0),
        Theme::dark(),
    )
    .unwrap();
    host.dispatch(Action::Start).unwrap();
    let mut clipboard = MemoryClipboard::default();
    assert_eq!(host.run_pending_services(&mut clipboard).unwrap(), 3);
    assert_eq!(clipboard.text(), Some("Astrelis"));
    assert_eq!(host.component().clipboard.as_deref(), Some("Astrelis"));
    assert_eq!(host.component().task, Some(42));
}

#[derive(Clone, PartialEq)]
struct ChildProps;

#[derive(Clone)]
enum ChildAction {
    Paste,
    Loaded(Option<String>),
}

enum ChildEffect {
    Loaded(String),
}

struct Child;

impl Component for Child {
    type Action = ChildAction;
    type Effect = ChildEffect;

    fn update(&mut self, action: ChildAction, context: &mut ComponentContext<'_, ChildEffect>) {
        match action {
            ChildAction::Paste => context.read_clipboard(ChildAction::Loaded),
            ChildAction::Loaded(Some(text)) => context.emit(ChildEffect::Loaded(text)),
            ChildAction::Loaded(None) => {}
        }
    }

    fn view(&self, _theme: &Theme) -> View<ChildAction> {
        button("Paste", ChildAction::Paste)
    }
}

impl ComponentWithProps for Child {
    type Props = ChildProps;

    fn create(_props: &Self::Props) -> Self {
        Self
    }

    fn changed(&mut self, _props: &Self::Props) {}
}

#[derive(Clone)]
enum ParentAction {
    Loaded(String),
}

struct Parent {
    loaded: Option<String>,
}

impl Component for Parent {
    type Action = ParentAction;
    type Effect = ();

    fn update(&mut self, action: ParentAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            ParentAction::Loaded(text) => self.loaded = Some(text),
        }
    }

    fn view(&self, _theme: &Theme) -> View<ParentAction> {
        component::<Child, ParentAction>(ChildProps, |effect| match effect {
            ChildEffect::Loaded(text) => ParentAction::Loaded(text),
        })
    }
}

#[test]
fn nested_service_completion_returns_to_the_component_that_requested_it() {
    let mut host = ComponentHost::new(
        Parent { loaded: None },
        LogicalSize::new(200.0, 80.0),
        Theme::dark(),
    )
    .unwrap();
    let paste = host
        .ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == "Paste")
        .unwrap()
        .id;
    host.semantic_action(paste, SemanticAction::Activate)
        .unwrap();
    let mut clipboard = MemoryClipboard::new(Some("nested".into()));
    assert_eq!(host.run_pending_services(&mut clipboard).unwrap(), 1);
    assert_eq!(host.component().loaded.as_deref(), Some("nested"));
}

#[test]
fn undo_history_is_bounded_and_clears_redo_on_new_edits() {
    let mut history = UndoHistory::new(NonZeroUsize::new(2).unwrap());
    let mut value = 0;
    history.checkpoint(value);
    value = 1;
    history.checkpoint(value);
    value = 2;
    history.checkpoint(value);
    value = 3;

    assert!(history.undo(&mut value));
    assert_eq!(value, 2);
    assert!(history.undo(&mut value));
    assert_eq!(value, 1);
    assert!(!history.undo(&mut value));
    assert!(history.redo(&mut value));
    assert_eq!(value, 2);

    history.checkpoint(value);
    value = 9;
    assert!(!history.can_redo());
    assert!(history.can_undo());
    assert_eq!(value, 9);
}
