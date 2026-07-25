//! Application surfaces: the toolbar, the toast stack, and the modal dialog.
//!
//! What separates these from leaf controls is policy - keyed decorations, focus
//! scoping, escape dismissal, background disablement - and policy is exactly
//! what a golden cannot check. Each test below drives one policy decision.

use astrelis_core::geometry::LogicalSize;
use astrelis_paint::PathVerb;
use astrelis_platform::NamedKey;
use rxui::core::{PassStats, SemanticRole};
use rxui::{
    ButtonVariant, Component, ComponentContext, DialogAction, Icon, IconButtonStyle, Theme, Toast,
    ToastLevel, ToolbarItem, View, dialog, icons, label, toasts, toolbar,
};
use rxui_test_support::Harness;

const VIEWPORT: LogicalSize = LogicalSize::new(640.0, 480.0);

/// Asserts a settled frame asked the engine for nothing.
///
/// `PassStats::default()` is deliberately not the bar: a settled frame still
/// reports the fragments the paint pass *reused*, and reuse is the evidence that
/// nothing was rebuilt. What has to be zero is the work - measurement, shaping,
/// paint, and the accessibility delta.
#[track_caller]
fn assert_no_retained_work(what: &str, stats: PassStats) {
    assert_eq!(stats.layout_elements, 0, "{what}: re-laid-out");
    assert_eq!(stats.rebuilt_fragments, 0, "{what}: repainted");
    assert_eq!(stats.shaped_text, 0, "{what}: re-shaped text");
    assert_eq!(
        stats.accessibility_nodes, 0,
        "{what}: republished semantics",
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Command {
    Save,
    Open,
    Locked,
}

struct ToolbarScene {
    invoked: Vec<Command>,
    icons: bool,
    /// Replaces the save glyph, for the icon-identity tests.
    glyph: Option<Icon>,
    /// Logical edge requested for the save glyph.
    glyph_size: f32,
}

impl ToolbarScene {
    fn new(icons: bool) -> Self {
        Self {
            invoked: Vec::new(),
            icons,
            glyph: None,
            glyph_size: IconButtonStyle::compact().icon_size,
        }
    }
}

/// Rebuilds an icon's geometry into a separately allocated `Path`.
fn rebuilt_path(icon: &Icon) -> Icon {
    Icon::from_verbs(icon.view_box(), icon.path().verbs().iter().copied())
        .expect("re-recording a valid icon's verbs yields a valid icon")
        .with_fill_rule(icon.fill_rule())
}

/// Rebuilds an icon with exactly one of its verbs displaced by a logical unit.
fn one_verb_moved(icon: &Icon) -> Icon {
    let mut verbs = icon.path().verbs().to_vec();
    let point = verbs
        .iter_mut()
        .find_map(|verb| match verb {
            PathVerb::LineTo(point) => Some(point),
            _ => None,
        })
        .expect("the save glyph draws line segments");
    point.x += 1.0;
    Icon::from_verbs(icon.view_box(), verbs)
        .expect("displacing one point keeps the icon valid")
        .with_fill_rule(icon.fill_rule())
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
                // Left to the default, this allocates a fresh `Path` on every
                // pass, which is what an unchanged icon button has to survive.
                icon: self.glyph.clone().unwrap_or_else(icons::save),
                label: "Save".into(),
                action: Command::Save,
                enabled: true,
                style: IconButtonStyle::compact()
                    .show_label(true)
                    .icon_size(self.glyph_size),
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
fn refreshing_a_toolbar_of_icon_commands_does_no_retained_work() {
    let mut harness = Harness::new(ToolbarScene::new(true), VIEWPORT).expect("the toolbar mounts");
    harness.refresh();
    assert_no_retained_work("an icon toolbar", harness.stats());
}

#[test]
fn an_icon_command_rebuilt_from_the_same_verbs_does_no_retained_work() {
    let mut harness = Harness::new(ToolbarScene::new(true), VIEWPORT).expect("the toolbar mounts");
    harness.mutate(|scene| scene.glyph = Some(rebuilt_path(&icons::save())));
    // The button compares its glyph by the verbs it records, so a separately
    // allocated path describing the same picture is not a change. Folding
    // `Path::cache_id` - a per-allocation counter - into the key instead is what
    // made every icon button in a toolbar relayout on every frame.
    assert_no_retained_work("an icon toolbar with a re-recorded path", harness.stats());
}

#[test]
fn an_icon_command_with_a_different_glyph_repaints_without_relayout() {
    let mut harness = Harness::new(ToolbarScene::new(true), VIEWPORT).expect("the toolbar mounts");
    harness.mutate(|scene| scene.glyph = Some(one_verb_moved(&icons::save())));
    let stats = harness.stats();
    assert_eq!(stats.rebuilt_fragments, 1);
    // `Button::layout` measures the glyph's requested edge, never its verbs: the
    // path is scaled into the square already reserved for it. So a different
    // picture at the same edge costs one repaint and no measurement.
    assert_eq!(stats.layout_elements, 0);
    assert_eq!(stats.shaped_text, 0);
}

#[test]
fn an_icon_command_with_a_larger_glyph_relayouts_the_row() {
    let mut harness = Harness::new(ToolbarScene::new(true), VIEWPORT).expect("the toolbar mounts");
    let before = harness.bounds("Save").size;
    harness.mutate(|scene| scene.glyph_size = 28.0);
    let stats = harness.stats();
    // The edge feeds the button's intrinsic size, so this is the one icon change
    // that has to re-measure: the button, the toolbar row, and the root container
    // that positions it.
    assert_eq!(stats.layout_elements, 3);
    // Two fragments: the button, and the row itself, whose surface background is
    // drawn to a height the taller button just changed.
    assert_eq!(stats.rebuilt_fragments, 2);
    assert!(
        harness.bounds("Save").size.width > before.width,
        "a wider glyph has to widen the button that reserves room for it",
    );
}

#[test]
fn refreshing_a_toolbar_of_text_commands_does_no_retained_work() {
    let mut harness = Harness::new(ToolbarScene::new(false), VIEWPORT).expect("the toolbar mounts");
    harness.refresh();
    // Text commands compare cleanly, which is the baseline the icon variant above
    // now meets as well.
    assert_no_retained_work("a text toolbar", harness.stats());
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
    assert_no_retained_work("an open dialog", harness.stats());
}
