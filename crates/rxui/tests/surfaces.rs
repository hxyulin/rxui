//! Application surfaces: the toolbar, the toast stack, and the modal dialog.
//!
//! What separates these from leaf controls is policy - keyed decorations, focus
//! scoping, escape dismissal, background disablement - and policy is exactly
//! what a golden cannot check. Each test below drives one policy decision.

use astrelis_core::geometry::LogicalSize;
use astrelis_platform::NamedKey;
use rxui::core::{PassStats, SemanticRole};
use rxui::{
    ButtonVariant, Component, ComponentContext, DialogAction, IconButtonStyle, Theme, Toast,
    ToastLevel, ToolbarItem, View, dialog, icons, label, toasts, toolbar,
};
use rxui_test_support::Harness;

const VIEWPORT: LogicalSize = LogicalSize::new(640.0, 480.0);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Command {
    Save,
    Open,
    Locked,
}

struct ToolbarScene {
    invoked: Vec<Command>,
    icons: bool,
}

impl ToolbarScene {
    const fn new(icons: bool) -> Self {
        Self {
            invoked: Vec::new(),
            icons,
        }
    }
}

impl Component for ToolbarScene {
    type Action = Command;
    type Effect = ();

    fn update(&mut self, action: Command, _context: &mut ComponentContext<'_, ()>) {
        self.invoked.push(action);
    }

    fn view(&self, _theme: &Theme) -> View<Command> {
        let save = if self.icons {
            ToolbarItem::IconCommand {
                id: "save".into(),
                icon: icons::save(),
                label: "Save".into(),
                action: Command::Save,
                enabled: true,
                style: IconButtonStyle::compact().show_label(true),
            }
        } else {
            ToolbarItem::Command {
                id: "save".into(),
                label: "Save".into(),
                action: Command::Save,
                enabled: true,
                variant: ButtonVariant::Primary,
            }
        };
        toolbar(&[
            save,
            ToolbarItem::Separator,
            ToolbarItem::Command {
                id: "open".into(),
                label: "Open".into(),
                action: Command::Open,
                enabled: true,
                variant: ButtonVariant::Standard,
            },
            ToolbarItem::Space(24.0),
            ToolbarItem::Command {
                id: "locked".into(),
                label: "Locked".into(),
                action: Command::Locked,
                enabled: false,
                variant: ButtonVariant::Quiet,
            },
        ])
    }
}

#[test]
fn a_toolbar_publishes_one_button_per_command_and_nothing_for_decorations() {
    let harness = Harness::new(ToolbarScene::new(false), VIEWPORT).expect("the toolbar mounts");
    let buttons = harness
        .semantics()
        .iter()
        .filter(|node| node.data.role == SemanticRole::Button)
        .count();
    // Three commands. The separator and the spacer are decoration: they carry
    // no accessible identity, so an assistive client never announces them.
    assert_eq!(buttons, 3);
    assert!(harness.try_find("Save").is_some());
    assert!(harness.try_find("Open").is_some());
}

#[test]
fn a_toolbar_lays_its_items_out_in_declaration_order() {
    let harness = Harness::new(ToolbarScene::new(false), VIEWPORT).expect("the toolbar mounts");
    let save = harness.bounds("Save");
    let open = harness.bounds("Open");
    let locked = harness.bounds("Locked");
    assert!(save.origin.x < open.origin.x);
    assert!(open.origin.x < locked.origin.x);
    // The `Space(24.0)` decoration sits between Open and Locked, so their gap
    // has to exceed the container's own `Space::Xs` gap.
    assert!(
        locked.origin.x - (open.origin.x + open.size.width) > 24.0,
        "a spacer item must widen the gap it sits in",
    );
}

#[test]
fn a_disabled_toolbar_command_reports_nothing() {
    let mut harness = Harness::new(ToolbarScene::new(false), VIEWPORT).expect("the toolbar mounts");
    assert!(!harness.find("Locked").enabled);
    harness.activate("Locked");
    assert!(harness.component().invoked.is_empty());

    harness.activate("Save");
    assert_eq!(harness.component().invoked, vec![Command::Save]);
}

#[test]
fn an_icon_toolbar_command_keeps_button_semantics() {
    let harness = Harness::new(ToolbarScene::new(true), VIEWPORT).expect("the toolbar mounts");
    let save = harness.find("Save");
    assert_eq!(save.data.role, SemanticRole::Button);
    assert!(save.focusable);
}

#[test]
#[ignore = "ButtonView::rebuild (rxui-core/src/view.rs:2648) folds Path::cache_id into its \
            change key, and icons::save() allocates a fresh Path per view() call, so an \
            unchanged icon button re-runs LAYOUT_ALL and repaints every frame"]
fn refreshing_a_toolbar_of_icon_commands_does_no_retained_work() {
    let mut harness = Harness::new(ToolbarScene::new(true), VIEWPORT).expect("the toolbar mounts");
    harness.refresh();
    assert_eq!(harness.stats(), PassStats::default());
}

#[test]
fn refreshing_a_toolbar_of_text_commands_does_no_retained_work() {
    let mut harness = Harness::new(ToolbarScene::new(false), VIEWPORT).expect("the toolbar mounts");
    harness.refresh();
    let stats = harness.stats();
    // Text commands compare cleanly, which is what makes the icon result below
    // a bug rather than a property of toolbars.
    assert_eq!(stats.shaped_text, 0);
    assert_eq!(stats.layout_elements, 0);
    assert_eq!(stats.rebuilt_fragments, 0);
}

#[test]
fn refreshing_a_toolbar_of_icon_commands_currently_relayouts_and_repaints() {
    let mut harness = Harness::new(ToolbarScene::new(true), VIEWPORT).expect("the toolbar mounts");
    harness.refresh();
    let stats = harness.stats();
    // Documents the bug `refreshing_a_toolbar_of_icon_commands_does_no_retained_work`
    // states. Three layouts: the icon button that declared itself stale, plus
    // the toolbar row and the root container that have to re-measure it. One
    // fragment - the button's - is rebuilt while the other six are reused.
    assert_eq!(stats.layout_elements, 3);
    assert_eq!(stats.rebuilt_fragments, 1);
    // Not reshaped, and only because the *engine* memoizes shaping per element
    // now. The wasted work is layout and paint, and it scales with the number
    // of icon buttons on screen.
    assert_eq!(stats.shaped_text, 0);
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ToastAction {
    Dismiss(u64),
}

struct ToastScene {
    items: Vec<Toast<ToastAction>>,
}

impl ToastScene {
    fn new() -> Self {
        Self {
            items: vec![
                Toast {
                    id: 1,
                    message: "Build started".into(),
                    level: ToastLevel::Info,
                    action: None,
                },
                Toast {
                    id: 2,
                    message: "Build failed".into(),
                    level: ToastLevel::Error,
                    action: Some(("Retry".into(), ToastAction::Dismiss(2))),
                },
            ],
        }
    }
}

impl Component for ToastScene {
    type Action = ToastAction;
    type Effect = ();

    fn update(&mut self, action: ToastAction, _context: &mut ComponentContext<'_, ()>) {
        let ToastAction::Dismiss(id) = action;
        self.items.retain(|toast| toast.id != id);
    }

    fn view(&self, _theme: &Theme) -> View<ToastAction> {
        toasts(&self.items)
    }
}

#[test]
fn a_toast_publishes_its_message_and_only_an_action_toast_publishes_a_button() {
    let harness = Harness::new(ToastScene::new(), VIEWPORT).expect("the toast stack mounts");
    assert!(harness.try_find("Build started").is_some());
    assert!(harness.try_find("Build failed").is_some());
    let buttons = harness
        .semantics()
        .iter()
        .filter(|node| node.data.role == SemanticRole::Button)
        .count();
    // Only the second toast carries an action, so exactly one button exists.
    assert_eq!(buttons, 1);
    assert!(harness.try_find("Retry").is_some());
}

#[test]
fn a_toast_stack_aligns_to_the_top_trailing_corner() {
    let harness = Harness::new(ToastScene::new(), VIEWPORT).expect("the toast stack mounts");
    let first = harness.bounds("Build started");
    let second = harness.bounds("Build failed");
    assert!(first.origin.y < second.origin.y, "toasts stack downward");
    // The stack is capped at 360 logical units wide and aligned trailing inside
    // a 640-wide viewport, so every toast starts right of the midpoint.
    assert!(first.origin.x > VIEWPORT.width * 0.5);
}

#[test]
fn dismissing_a_toast_leaves_the_others_retained() {
    let mut harness = Harness::new(ToastScene::new(), VIEWPORT).expect("the toast stack mounts");
    let survivor = harness.find("Build started").id;
    harness.activate("Retry");

    assert!(harness.try_find("Build failed").is_none());
    assert!(harness.try_find("Retry").is_none());
    // Toasts are keyed by `Toast::id`, so removing one must not renumber the
    // rest into each other's retained nodes.
    assert_eq!(harness.find("Build started").id, survivor);
}

#[test]
fn an_empty_toast_stack_publishes_no_messages() {
    let mut harness = Harness::new(ToastScene::new(), VIEWPORT).expect("the toast stack mounts");
    harness.mutate(|scene| scene.items.clear());
    assert!(harness.try_find("Build started").is_none());
    assert!(harness.try_find("Build failed").is_none());
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DialogEvent {
    Open,
    Cancel,
    Confirm,
    Background,
}

struct DialogScene {
    open: bool,
    seen: Vec<DialogEvent>,
}

impl DialogScene {
    const fn new(open: bool) -> Self {
        Self {
            open,
            seen: Vec::new(),
        }
    }
}

impl Component for DialogScene {
    type Action = DialogEvent;
    type Effect = ();

    fn update(&mut self, action: DialogEvent, _context: &mut ComponentContext<'_, ()>) {
        match action {
            DialogEvent::Open => self.open = true,
            DialogEvent::Cancel | DialogEvent::Confirm => self.open = false,
            DialogEvent::Background => {}
        }
        self.seen.push(action);
    }

    fn view(&self, _theme: &Theme) -> View<DialogEvent> {
        dialog(
            self.open,
            "Discard changes?",
            rxui::button("Edit document", DialogEvent::Background),
            label("Unsaved edits will be lost."),
            DialogEvent::Cancel,
            &[
                DialogAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    action: DialogEvent::Cancel,
                    variant: ButtonVariant::Quiet,
                },
                DialogAction {
                    id: "confirm".into(),
                    label: "Discard".into(),
                    action: DialogEvent::Confirm,
                    variant: ButtonVariant::Primary,
                },
            ],
        )
    }
}

#[test]
fn a_closed_dialog_publishes_nothing_and_leaves_the_background_usable() {
    let mut harness = Harness::new(DialogScene::new(false), VIEWPORT).expect("the dialog mounts");
    assert!(harness.try_find("Discard changes?").is_none());
    assert!(harness.try_find("Cancel").is_none());
    assert!(harness.find("Edit document").enabled);

    harness.activate("Edit document");
    assert_eq!(harness.component().seen, vec![DialogEvent::Background]);
}

#[test]
fn an_open_dialog_disables_the_background_and_autofocuses_its_first_action() {
    let harness = Harness::new(DialogScene::new(true), VIEWPORT).expect("the dialog mounts");
    assert!(!harness.find("Edit document").enabled);
    assert!(harness.try_find("Discard changes?").is_some());
    // The dialog's focus scope claims focus on open, and the action row is
    // declared before nothing else focusable, so Cancel takes it.
    assert!(harness.find("Cancel").focused);
}

#[test]
fn an_open_dialog_routes_its_actions_and_swallows_the_background() {
    let mut harness = Harness::new(DialogScene::new(true), VIEWPORT).expect("the dialog mounts");
    harness.activate("Edit document");
    assert!(
        harness.component().seen.is_empty(),
        "background is disabled"
    );

    harness.activate("Discard");
    assert_eq!(harness.component().seen, vec![DialogEvent::Confirm]);
    assert!(!harness.component().open);
}

#[test]
fn escape_dismisses_an_open_dialog_through_its_declared_action() {
    let mut harness = Harness::new(DialogScene::new(true), VIEWPORT).expect("the dialog mounts");
    harness.press(NamedKey::Escape);
    assert_eq!(harness.component().seen, vec![DialogEvent::Cancel]);
    assert!(!harness.component().open);

    // A closed dialog must stop listening, or Escape becomes a global key that
    // fires an action nobody can see the source of.
    harness.press(NamedKey::Escape);
    assert_eq!(harness.component().seen, vec![DialogEvent::Cancel]);
}

#[test]
fn reopening_a_dialog_reuses_its_retained_action_buttons() {
    let mut harness = Harness::new(DialogScene::new(true), VIEWPORT).expect("the dialog mounts");
    let cancel = harness.find("Cancel").id;
    harness.activate("Cancel");
    harness.dispatch(DialogEvent::Open);
    // `dialog` hides its modal with `.visible(open)` rather than dropping it, so
    // reopening is a visibility flip, not a rebuild. That is what keeps a
    // dialog's caret and scroll position across a close/open cycle.
    assert_eq!(harness.find("Cancel").id, cancel);
}

#[test]
fn refreshing_an_open_dialog_does_no_retained_work() {
    let mut harness = Harness::new(DialogScene::new(true), VIEWPORT).expect("the dialog mounts");
    harness.refresh();
    let stats = harness.stats();
    assert_eq!(stats.shaped_text, 0);
    assert_eq!(stats.layout_elements, 0);
    assert_eq!(stats.rebuilt_fragments, 0);
}
