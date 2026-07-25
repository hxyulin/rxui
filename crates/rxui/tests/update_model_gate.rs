//! Update-model gate: the ratchet on per-interaction framework work.
//!
//! Reducing an action no longer rebuilds anything. It updates component state
//! and records which components went stale; the runtime then rebuilds the root
//! only if the root's own state changed, and drains the remaining stale
//! components in depth order. At each component boundary, a subtree is rebuilt
//! only when its props, its theme revision, or its own dirty flag says its
//! output can have changed. The engine's `PassStats` cannot see any of that,
//! because it measures layout, paint, and accessibility work *below* the
//! component boundary.
//!
//! `ViewStats` counters are deterministic — no timing, no sampling — so each is
//! asserted with exact equality and confirmed stable across repeated serial and
//! parallel runs. A change in either direction should force a deliberate edit
//! here, with a comment explaining the new number.
//!
//! The load-bearing property is in
//! [`one_action_into_one_panel_is_independent_of_sibling_count`]: one action
//! into one of N sibling components costs byte-identical `ViewStats` at N = 4
//! and N = 64.
//!
//! Every scenario ends in [`assert_incremental_matches_fresh`], which compares
//! the incrementally updated tree against a second host mounted fresh from the
//! same final state. That is what stops a counter "win" from being bought by
//! leaving retained state stale.

mod support;

use astrelis_core::geometry::{LogicalPoint, LogicalSize};
use astrelis_platform::{
    DeviceId, ElementState, Key, KeyLocation, KeyboardInput, Modifiers, PhysicalKey,
};
use astrelis_ui_next::{
    NodeId, PassStats, SemanticAction, SemanticData, SemanticNode, SemanticRole, UiInput,
};
use rxui::{
    ColorRole, Component, ComponentContext, ComponentHost, ComponentWithProps, Theme, View, button,
    column, component, diagnostics::ViewStats, label, panel, row, text_field, views,
};

use support::assert_incremental_matches_fresh;

const VIEWPORT: LogicalSize = LogicalSize::new(900.0, 720.0);

/// Rows in the moderately sized list shared by most scenarios.
const ROWS: usize = 12;

/// A second theme.
///
/// `Theme` only ships `dark()`, so the theme-change scenario supplies its own
/// light tokens. Every color differs, which is what forces the whole tree to
/// re-resolve styles and repaint.
fn light_theme() -> Theme {
    Theme {
        revision: 2,
        background: astrelis_core::color::Color::from_hex(0xf4f5f7),
        surface: astrelis_core::color::Color::from_hex(0xffffff),
        text: astrelis_core::color::Color::from_hex(0x1c1f24),
        muted: astrelis_core::color::Color::from_hex(0x6b7280),
        accent: astrelis_core::color::Color::from_hex(0x2563eb),
        danger: astrelis_core::color::Color::from_hex(0xdc2626),
    }
}

// ---------------------------------------------------------------- note editor

/// Props for the nested editor.
///
/// The text is carried in props so that a fresh host can reproduce the child's
/// final state; `assert_incremental_matches_fresh` has no other way to rebuild
/// component-local state it cannot see. The child still owns its own state and
/// still reduces its own action, which is what the keystroke scenario measures.
#[derive(Clone, PartialEq, Eq)]
struct NoteProps {
    text: String,
}

#[derive(Clone)]
enum NoteAction {
    Edit(String),
}

enum NoteEffect {
    Changed(String),
}

struct NoteEditor {
    text: String,
}

impl Component for NoteEditor {
    type Action = NoteAction;
    type Effect = NoteEffect;

    fn update(&mut self, action: NoteAction, context: &mut ComponentContext<'_, NoteEffect>) {
        match action {
            NoteAction::Edit(text) => {
                self.text = text.clone();
                context.emit(NoteEffect::Changed(text));
            }
        }
    }

    fn view(&self, _theme: &Theme) -> View<NoteAction> {
        column((
            label("Notes").keyed("caption"),
            text_field("Note", self.text.clone(), NoteAction::Edit).keyed("field"),
        ))
    }
}

impl ComponentWithProps for NoteEditor {
    type Props = NoteProps;

    fn create(props: &NoteProps) -> Self {
        Self {
            text: props.text.clone(),
        }
    }

    fn changed(&mut self, props: &NoteProps) {
        self.text = props.text.clone();
    }
}

// ------------------------------------------------------------------ workspace

#[derive(Clone)]
enum WorkspaceAction {
    Save,
    Reload,
    Select(u64),
    NoteChanged(String),
}

/// A moderately sized tree: a toolbar, a keyed list, and a nested component.
///
/// 46 retained nodes, of which 45 are view nodes below the root: 1 outer
/// column, 1 toolbar row, 2 buttons, 1 list column, 12 keyed rows each with a
/// selection mark and a label, and the nested editor's boundary, column,
/// caption, and field.
struct Workspace {
    selected: u64,
    note: String,
}

impl Component for Workspace {
    type Action = WorkspaceAction;
    type Effect = ();

    fn update(&mut self, action: WorkspaceAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            WorkspaceAction::Save | WorkspaceAction::Reload => {}
            WorkspaceAction::Select(id) => self.selected = id,
            WorkspaceAction::NoteChanged(note) => self.note = note,
        }
    }

    fn view(&self, _theme: &Theme) -> View<WorkspaceAction> {
        column((
            row((
                button("Save", WorkspaceAction::Save).keyed("save"),
                button("Reload", WorkspaceAction::Reload).keyed("reload"),
            ))
            .keyed("toolbar"),
            column(views((0..ROWS).map(|index| {
                let id = index as u64;
                let selected = self.selected == id;
                row((
                    panel(
                        LogicalSize::new(14.0, 18.0),
                        if selected {
                            ColorRole::Accent
                        } else {
                            ColorRole::Surface
                        },
                        Some(SemanticData {
                            role: SemanticRole::Row,
                            label: format!("Row {index}"),
                            selected: Some(selected),
                            ..SemanticData::default()
                        }),
                    )
                    .keyed("mark"),
                    label(format!("Row {index}")).keyed("label"),
                ))
                .keyed(id)
            })))
            .keyed("list"),
            component::<NoteEditor, WorkspaceAction>(
                NoteProps {
                    text: self.note.clone(),
                },
                |effect| match effect {
                    NoteEffect::Changed(text) => WorkspaceAction::NoteChanged(text),
                },
            )
            .keyed("notes"),
        ))
    }
}

fn workspace(note: &str, selected: u64) -> Workspace {
    Workspace {
        selected,
        note: note.to_owned(),
    }
}

fn mount_workspace(theme: Theme) -> ComponentHost<Workspace> {
    ComponentHost::new(workspace("", 0), VIEWPORT, theme).expect("workspace mounts")
}

// ------------------------------------------------------------------- fixtures

fn find(host: &ComponentHost<impl Component>, label: &str) -> SemanticNode {
    host.ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.label == label)
        .unwrap_or_else(|| panic!("semantic node `{label}` exists"))
}

fn center(node: &SemanticNode) -> LogicalPoint {
    LogicalPoint::new(
        node.bounds.origin.x + node.bounds.size.width * 0.5,
        node.bounds.origin.y + node.bounds.size.height * 0.5,
    )
}

/// Point just inside the trailing edge of a control.
///
/// Clicking past the end of a text field's content puts the caret at the end
/// of the text, which is also where a freshly created field starts. Without
/// that agreement the caret would paint at a different offset in the fresh
/// reference tree.
fn trailing_edge(node: &SemanticNode) -> LogicalPoint {
    LogicalPoint::new(
        node.bounds.origin.x + node.bounds.size.width - 6.0,
        node.bounds.origin.y + node.bounds.size.height * 0.5,
    )
}

fn keystroke(text: &str) -> UiInput {
    UiInput::Keyboard {
        input: KeyboardInput {
            device_id: DeviceId(1),
            physical_key: PhysicalKey::Unidentified,
            logical_key: Key::Character(text.into()),
            text: Some(text.into()),
            location: KeyLocation::Standard,
            state: ElementState::Pressed,
            repeat: false,
            synthetic: false,
        },
        modifiers: Modifiers::default(),
    }
}

/// Asserts the counters that no producer exists for yet.
///
/// Memoization and row virtualization arrive in later phases. Pinning these to
/// zero keeps the gate file stable and makes the phase that introduces a
/// producer visible as a deliberate change here.
fn assert_no_memo_or_row_activity(scenario: &str, view: ViewStats) {
    assert_eq!(view.memo_hits, 0, "{scenario}: memoization has no producer");
    assert_eq!(
        view.memo_misses, 0,
        "{scenario}: memoization has no producer"
    );
    assert_eq!(
        view.rows_realized, 0,
        "{scenario}: row virtualization has no producer"
    );
    assert_eq!(
        view.rows_recycled, 0,
        "{scenario}: row virtualization has no producer"
    );
}

// ------------------------------------------------------------------- scenarios

#[test]
fn hover_over_a_control_does_no_view_work() {
    let mut host = mount_workspace(Theme::dark());
    let point = center(&find(&host, "Save"));

    let ((), view) = ViewStats::measure(|| {
        host.input(UiInput::PointerMoved(point)).expect("hover");
    });
    let pass = host.ui().stats();

    // Hover is the one interaction that already costs nothing above the
    // engine: the pointer move produces no component action, so
    // `ComponentRuntime::input` short-circuits to `update_passes` and never
    // touches `view()` or the shadow tree.
    assert_eq!(view, ViewStats::new(), "hover must do no view work");
    assert_no_memo_or_row_activity("hover", view);

    // 0 = hover changes no layout input; the button repaints in place.
    assert_eq!(pass.layout_elements, 0);
    // 1 = only the hovered button's fragment; the other 45 are reused.
    assert_eq!(pass.rebuilt_fragments, 1);
    assert_eq!(pass.reused_fragments, 45);
    // 0 = hover is not an accessibility-visible property here.
    assert_eq!(pass.accessibility_nodes, 0);
    assert_eq!(pass.shaped_text, 0);
    // Always 0: `update_passes` resets `PassStats` before running, and hit
    // testing happens in `dispatch` beforehand, so the traversal count is
    // never observable from the frame's stats.
    assert_eq!(pass.hit_test_nodes, 0);

    assert_incremental_matches_fresh(
        "hover",
        &host,
        workspace("", 0),
        VIEWPORT,
        Theme::dark(),
        |fresh| {
            // Replay the pointer move so the reference tree carries the same
            // engine-owned hover state.
            let point = center(&find(fresh, "Save"));
            fresh.input(UiInput::PointerMoved(point)).expect("hover");
        },
    );
}

#[test]
fn keystroke_into_a_nested_child_field_rebuilds_only_the_root_and_that_child() {
    let mut host = mount_workspace(Theme::dark());
    let point = trailing_edge(&find(&host, "Note"));
    host.input(UiInput::PointerPressed(point)).expect("focus");

    let ((), view) = ViewStats::measure(|| {
        host.input(keystroke("!")).expect("keystroke");
    });
    let pass = host.ui().stats();

    assert_eq!(host.component().note, "!");

    // This is the headline case, and the floor for it is 2 rather than 1.
    // Routing no longer rebuilds: the child reduces its own action, marks
    // itself dirty, and emits `NoteEffect::Changed`, which the root reduces
    // into `self.note`. That is a genuine change to root state, so:
    //   1. the root re-renders once, in `flush`;
    //   2. reconciling the child's `ComponentView` sees changed props and
    //      rebuilds the child once — which also clears the child's own dirty
    //      entry, so the depth-ordered drain finds nothing left to do.
    // The third call is gone: the child's subtree is now diffed once per
    // keystroke instead of twice. Reaching 1 would require the root *not* to
    // re-render, which cannot be correct while `Workspace::note` mirrors the
    // child's text.
    assert_eq!(view.component_views, 2);
    // 0 = retained identity is fully preserved; nothing is remounted.
    assert_eq!(view.nodes_built, 0);
    // 45 = every view node below the root, diffed exactly once. Was 48, which
    // was 45 plus the child's 3 nodes diffed a second time by the routing pass.
    assert_eq!(view.nodes_rebuilt, 45);
    // 16 = every container in the tree, each reconciled once. Was 17, which
    // double-counted the child's column.
    assert_eq!(view.containers_reconciled, 16);
    // 0 = no container's child order changed, and `MountedChildren` now
    // compares against the order it last published instead of republishing it.
    // Every `set_children` call carried `Invalidation::ALL`, so this is the
    // largest single reduction in retained work in the whole change.
    assert_eq!(view.set_children_calls, 0);
    assert_no_memo_or_row_activity("keystroke", view);

    // For contrast, the engine's own view of the same keystroke is nearly
    // free: 5 = the field and its four ancestors re-measure, 1 fragment
    // repaints, 45 are reused, 1 accessibility node changes.
    assert_eq!(pass.layout_elements, 5);
    assert_eq!(pass.rebuilt_fragments, 1);
    assert_eq!(pass.reused_fragments, 45);
    assert_eq!(pass.accessibility_nodes, 1);
    // 1 = only the edited value re-shapes. The field also shapes a placeholder
    // every pass to keep its height stable, but the engine's per-element
    // shaping memo now serves that from cache, so this was 2 before the memo.
    assert_eq!(pass.shaped_text, 1);

    assert_incremental_matches_fresh(
        "keystroke",
        &host,
        workspace("!", 0),
        VIEWPORT,
        Theme::dark(),
        |fresh| {
            // Replay the press so the reference field is focused with its
            // caret at the same offset.
            let point = trailing_edge(&find(fresh, "Note"));
            fresh.input(UiInput::PointerPressed(point)).expect("focus");
        },
    );
}

#[test]
fn selection_change_rebuilds_the_root_and_skips_the_nested_editor() {
    let mut host = mount_workspace(Theme::dark());

    let ((), view) = ViewStats::measure(|| {
        host.dispatch(WorkspaceAction::Select(5)).expect("select");
    });
    let pass = host.ui().stats();

    // 1 = the root re-render only. The nested editor's props and theme
    // revision are both unchanged and it reduced nothing, so its
    // `ComponentView` is skipped outright. Was 2.
    assert_eq!(view.component_views, 1);
    // 0 = keyed rows keep their retained identity across the selection move.
    assert_eq!(view.nodes_built, 0);
    // 41 = 45 minus the 4 nodes behind the skipped editor boundary (the
    // boundary itself, its column, its caption, and its field).
    assert_eq!(view.nodes_rebuilt, 41);
    // 15 = 16 minus the skipped editor's column.
    assert_eq!(view.containers_reconciled, 15);
    // 0 = no child order moved.
    assert_eq!(view.set_children_calls, 0);
    assert_no_memo_or_row_activity("selection", view);

    // 0 = a selection mark only changes its color and its `selected` flag, so
    // exact invalidation bits now ask for paint and accessibility without
    // layout. Was 7, because `set_box` discarded the correct bits the view had
    // already computed and asked for `LAYOUT_ALL`.
    assert_eq!(pass.layout_elements, 0);
    // Unchanged: the same 2 fragments repaint, 44 are reused, and the same 2
    // accessibility nodes change (selected went false/true).
    assert_eq!(pass.rebuilt_fragments, 2);
    assert_eq!(pass.reused_fragments, 44);
    assert_eq!(pass.accessibility_nodes, 2);
    assert_eq!(pass.shaped_text, 0);

    assert_incremental_matches_fresh(
        "selection",
        &host,
        workspace("", 5),
        VIEWPORT,
        Theme::dark(),
        |_| {},
    );
}

#[test]
fn theme_change_rebuilds_and_repaints_the_whole_tree() {
    let mut host = mount_workspace(Theme::dark());

    let ((), view) = ViewStats::measure(|| {
        host.set_theme(light_theme()).expect("theme");
    });
    let pass = host.ui().stats();

    // 2 = the root re-render plus the nested editor's. A theme change is the
    // one scenario where rebuilding everything is legitimate, so these
    // numbers are the natural floor rather than waste — they are recorded so
    // that update isolation can be shown *not* to break global invalidation.
    //
    // This is the assertion that proves `Theme::revision` is load-bearing: the
    // editor's props are identical across the switch, so the *only* reason it
    // re-renders is that the revision it last rendered at no longer matches.
    assert_eq!(view.component_views, 2);
    assert_eq!(view.nodes_built, 0);
    assert_eq!(view.nodes_rebuilt, 45);
    assert_eq!(view.containers_reconciled, 16);
    // 0 = a theme switch changes colors, not structure, so no container's child
    // order moves. Was 16.
    assert_eq!(view.set_children_calls, 0);
    assert_no_memo_or_row_activity("theme", view);

    // 31 = the 13 labels and 1 text field whose glyph color changed, plus every
    // ancestor on their paths. Was 46 (literally every element), because a
    // box or button whose fill changed asked for `LAYOUT_ALL`. Text is the only
    // thing here that genuinely must relayout, because the shaper bakes the
    // brush into glyph runs and re-shaping happens during layout.
    assert_eq!(pass.layout_elements, 31);
    // Unchanged at 28 of 46: exactly the elements rxui touched repaint, and it
    // touches the same set as before — only the bits it asks for narrowed.
    assert_eq!(pass.rebuilt_fragments, 28);
    assert_eq!(pass.reused_fragments, 18);
    // 14 = text whose glyph color actually changed must re-shape, because the
    // shaper bakes the brush into every glyph run. The engine's shaping memo
    // absorbs the rest (this was 17 before it existed). Giving paint a text
    // brush is what would take this to 0 and make a theme switch a repaint
    // rather than a reshape.
    assert_eq!(pass.shaped_text, 14);
    // 0 = colors carry no accessibility meaning.
    assert_eq!(pass.accessibility_nodes, 0);

    assert_incremental_matches_fresh(
        "theme",
        &host,
        workspace("", 0),
        VIEWPORT,
        light_theme(),
        |_| {},
    );
}

// --------------------------------------------------------------- panel board

/// Props for one independent panel.
///
/// `count` is mirrored here for the same reason `NoteProps::text` is: the
/// fresh reference host must be able to reproduce the panel's final state.
/// The panel still holds and mutates its own state and reduces its own action.
#[derive(Clone, PartialEq, Eq)]
struct PanelProps {
    index: usize,
    count: i32,
}

#[derive(Clone)]
enum PanelAction {
    Bump,
}

enum PanelEffect {
    Bumped(i32),
}

struct StatefulPanel {
    index: usize,
    count: i32,
}

impl Component for StatefulPanel {
    type Action = PanelAction;
    type Effect = PanelEffect;

    fn update(&mut self, action: PanelAction, context: &mut ComponentContext<'_, PanelEffect>) {
        match action {
            PanelAction::Bump => {
                self.count += 1;
                context.emit(PanelEffect::Bumped(self.count));
            }
        }
    }

    fn view(&self, _theme: &Theme) -> View<PanelAction> {
        row((
            label(format!("Panel {}", self.index)).keyed("title"),
            button(format!("Bump {}", self.index), PanelAction::Bump).keyed("bump"),
            label(format!("Count {}", self.count)).keyed("count"),
        ))
    }
}

impl ComponentWithProps for StatefulPanel {
    type Props = PanelProps;

    fn create(props: &PanelProps) -> Self {
        Self {
            index: props.index,
            count: props.count,
        }
    }

    fn changed(&mut self, props: &PanelProps) {
        self.index = props.index;
        self.count = props.count;
    }
}

#[derive(Clone)]
enum BoardAction {
    Bumped(usize, i32),
}

struct PanelBoard {
    counts: Vec<i32>,
}

impl Component for PanelBoard {
    type Action = BoardAction;
    type Effect = ();

    fn update(&mut self, action: BoardAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            BoardAction::Bumped(index, count) => self.counts[index] = count,
        }
    }

    fn view(&self, _theme: &Theme) -> View<BoardAction> {
        column(views(self.counts.iter().enumerate().map(
            |(index, count)| {
                component::<StatefulPanel, BoardAction>(
                    PanelProps {
                        index,
                        count: *count,
                    },
                    move |effect| match effect {
                        PanelEffect::Bumped(value) => BoardAction::Bumped(index, value),
                    },
                )
                .keyed(index as u64)
            },
        )))
    }
}

fn board(counts: Vec<i32>) -> PanelBoard {
    PanelBoard { counts }
}

fn target(host: &ComponentHost<PanelBoard>, label: &str) -> NodeId {
    find(host, label).id
}

/// Bumps panel 0 of an `panels`-wide board and returns the work it cost.
///
/// The action is delivered as a semantic activation rather than a pointer
/// click so the measurement does not depend on all 64 panels fitting inside
/// the viewport.
fn bump_first_panel(panels: usize) -> (ViewStats, PassStats) {
    let mut host = ComponentHost::new(board(vec![0; panels]), VIEWPORT, Theme::dark())
        .expect("panel board mounts");
    let bump = target(&host, "Bump 0");

    let ((), view) = ViewStats::measure(|| {
        host.semantic_action(bump, SemanticAction::Activate)
            .expect("bump");
    });
    let pass = host.ui().stats();

    assert_eq!(host.component().counts[0], 1);
    assert!(host.component().counts[1..].iter().all(|count| *count == 0));

    let mut counts = vec![0; panels];
    counts[0] = 1;
    assert_incremental_matches_fresh(
        "panels",
        &host,
        board(counts),
        VIEWPORT,
        Theme::dark(),
        |_| {},
    );

    (view, pass)
}

#[test]
fn one_action_into_one_panel_is_independent_of_sibling_count() {
    let (small, small_pass) = bump_first_panel(4);
    let (large, large_pass) = bump_first_panel(64);

    // THE ASSERTION THAT MATTERS. Bumping one panel is logically independent of
    // how many siblings exist, and every view counter now says so: a 16x change
    // in sibling count produces byte-identical `ViewStats`. This replaces the
    // `assert_ne!` the measurement phase recorded here.
    assert_eq!(
        small, large,
        "one action into one panel must cost the same at any sibling count"
    );

    // 2, independent of N:
    //   1 for the root re-render — `BoardAction::Bumped` really does change
    //     `PanelBoard::counts`, so the root's view is genuinely stale;
    //   1 for the bumped panel, whose props changed.
    // Every other panel's props compare equal, its theme revision is unchanged,
    // and it reduced nothing, so its `ComponentView` is skipped. The bumped
    // panel's own dirty entry is cleared by the same pass, so the depth-ordered
    // drain that follows has nothing left to do. Was N + 2.
    assert_eq!(small.component_views, 2);
    assert_eq!(large.component_views, 2);

    // 6, independent of N: the root column, the bumped panel's boundary, its
    // row, and its three children. Skipped boundaries deliberately record
    // nothing — see `ComponentView::rebuild_counted` — because a boundary that
    // reconciles neither its own state nor its subtree is not a rebuilt node,
    // and counting it would reintroduce the dependence on N. Was 5N + 5.
    assert_eq!(small.nodes_rebuilt, 6);
    assert_eq!(large.nodes_rebuilt, 6);

    // 2, independent of N: the root column and the bumped panel's row. Was
    // N + 2, which included every sibling's row plus the bumped row twice.
    assert_eq!(small.containers_reconciled, 2);
    assert_eq!(large.containers_reconciled, 2);
    // 0: neither child order moved, so neither list is republished. Was N + 2.
    assert_eq!(small.set_children_calls, 0);
    assert_eq!(large.set_children_calls, 0);

    // Nothing is remounted at either size.
    assert_eq!(small.nodes_built, 0);
    assert_eq!(large.nodes_built, 0);
    assert_no_memo_or_row_activity("panels N=4", small);
    assert_no_memo_or_row_activity("panels N=64", large);

    // The engine was already independent of sibling count: identical layout,
    // paint, accessibility, and shaping work at both sizes. Only
    // `reused_fragments` grows, and only because there are more untouched
    // fragments to reuse. The view counters above now have the same shape.
    assert_eq!(small_pass.layout_elements, 5);
    assert_eq!(large_pass.layout_elements, 5);
    assert_eq!(small_pass.rebuilt_fragments, 1);
    assert_eq!(large_pass.rebuilt_fragments, 1);
    assert_eq!(small_pass.accessibility_nodes, 1);
    assert_eq!(large_pass.accessibility_nodes, 1);
    assert_eq!(small_pass.shaped_text, 1);
    assert_eq!(large_pass.shaped_text, 1);
    assert_eq!(small_pass.reused_fragments, 21); // 5 * 4 + 1
    assert_eq!(large_pass.reused_fragments, 321); // 5 * 64 + 1
}

// ------------------------------------------------------------ effect-free child

/// A child that keeps its state entirely to itself.
///
/// `Effect = ()` and nothing is ever emitted, so its action reaches no ancestor.
/// This is the only shape that exercises the scoped drain: the root stays clean,
/// so the flush has to locate and rebuild one stale component without building
/// the root's view at all.
#[derive(Clone, PartialEq, Eq)]
struct TallyProps {
    caption: &'static str,
}

#[derive(Clone)]
enum TallyAction {
    Bump,
}

struct Tally {
    caption: &'static str,
    count: i32,
}

impl Component for Tally {
    type Action = TallyAction;
    type Effect = ();

    fn update(&mut self, action: TallyAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            TallyAction::Bump => self.count += 1,
        }
    }

    fn view(&self, _theme: &Theme) -> View<TallyAction> {
        row((
            button(format!("Bump {}", self.caption), TallyAction::Bump).keyed("bump"),
            label(format!("{} is {}", self.caption, self.count)).keyed("count"),
        ))
    }
}

impl ComponentWithProps for Tally {
    type Props = TallyProps;

    fn create(props: &TallyProps) -> Self {
        Self {
            caption: props.caption,
            count: 0,
        }
    }

    fn changed(&mut self, props: &TallyProps) {
        self.caption = props.caption;
    }
}

#[derive(Clone)]
enum TallyBoardAction {
    /// Unreachable: `Tally` emits no effects. Present only to type the mapping.
    Never,
}

struct TallyBoard;

impl Component for TallyBoard {
    type Action = TallyBoardAction;
    type Effect = ();

    fn update(&mut self, action: TallyBoardAction, _context: &mut ComponentContext<'_, ()>) {
        match action {
            TallyBoardAction::Never => unreachable!("Tally emits no effects"),
        }
    }

    fn view(&self, _theme: &Theme) -> View<TallyBoardAction> {
        column(views(["left", "right"].into_iter().map(|caption| {
            component::<Tally, TallyBoardAction>(TallyProps { caption }, |()| {
                TallyBoardAction::Never
            })
            .keyed(caption)
        })))
    }
}

/// A nested action with no parent effect must cost zero root renders.
///
/// This scenario cannot use `assert_incremental_matches_fresh`: the child's
/// state is unreachable from the parent by construction, so no `TallyBoard`
/// value can reproduce it in a second host — which is exactly the property under
/// test. The semantic assertions stand in for it, and they are what would fail
/// if the scoped drain rebuilt the wrong component, or none.
#[test]
fn a_nested_action_with_no_parent_effect_never_renders_the_root() {
    let mut host =
        ComponentHost::new(TallyBoard, VIEWPORT, Theme::dark()).expect("tally board mounts");
    let bump = find(&host, "Bump left").id;

    let ((), view) = ViewStats::measure(|| {
        host.semantic_action(bump, SemanticAction::Activate)
            .expect("bump");
    });

    // The bumped child re-rendered and its sibling did not.
    assert!(
        host.ui()
            .semantic_snapshot()
            .iter()
            .any(|node| node.data.label == "left is 1"),
        "the bumped child must not be left stale"
    );
    assert!(
        host.ui()
            .semantic_snapshot()
            .iter()
            .any(|node| node.data.label == "right is 0"),
        "the sibling must not be disturbed"
    );

    // 1 = the bumped child only. The root's state did not change, so its view is
    // never built; the depth-ordered drain locates the one stale component and
    // rebuilds just that subtree.
    assert_eq!(view.component_views, 1);
    assert_eq!(view.nodes_built, 0);
    // 3 = the child's row and its two children. The component boundary itself is
    // not reconciled on this path — the drain rebuilds the subtree directly
    // rather than descending through a parent's `ComponentView`.
    assert_eq!(view.nodes_rebuilt, 3);
    // 1 = the child's row. The root column is not touched at all.
    assert_eq!(view.containers_reconciled, 1);
    assert_eq!(view.set_children_calls, 0);
    assert_no_memo_or_row_activity("effect-free child", view);
}

// ------------------------------------------------------- coalesced dispatch

#[test]
fn a_batch_of_actions_costs_one_root_render() {
    let mut host = mount_workspace(Theme::dark());

    let ((), view) = ViewStats::measure(|| {
        host.dispatch_all([
            WorkspaceAction::Select(1),
            WorkspaceAction::Select(2),
            WorkspaceAction::Select(3),
        ])
        .expect("batch");
    });

    assert_eq!(host.component().selected, 3);
    // 1, not 3. Every action is reduced first and the tree is reconciled once.
    // Looping `dispatch` instead would cost one full rebuild per action, which
    // is what a window host used to pay for every event it drained.
    assert_eq!(view.component_views, 1);
    // Only the marks for rows 0 and 3 differ from the mounted state, and the
    // two intermediate selections never reach the retained tree at all.
    assert_eq!(view.nodes_built, 0);
    assert_no_memo_or_row_activity("batch", view);

    assert_incremental_matches_fresh(
        "batch",
        &host,
        workspace("", 3),
        VIEWPORT,
        Theme::dark(),
        |_| {},
    );
}

// ------------------------------------------------------------- shared state

/// State a child renders but does not own, reached through interior mutability.
///
/// This is the component shape update isolation breaks: the parent's edit changes
/// no props on the child's boundary, so nothing about the child's inputs tells
/// the framework its output moved.
#[derive(Clone, PartialEq, Eq)]
struct MirrorProps {
    title: std::rc::Rc<std::cell::RefCell<String>>,
}

struct Mirror {
    title: std::rc::Rc<std::cell::RefCell<String>>,
}

impl Component for Mirror {
    type Action = ();
    type Effect = ();

    fn update(&mut self, _action: (), _context: &mut ComponentContext<'_, ()>) {}

    fn view(&self, _theme: &Theme) -> View<()> {
        label(self.title.borrow().clone()).keyed("mirror")
    }
}

impl ComponentWithProps for Mirror {
    type Props = MirrorProps;

    fn create(props: &MirrorProps) -> Self {
        Self {
            title: props.title.clone(),
        }
    }

    fn changed(&mut self, props: &MirrorProps) {
        self.title = props.title.clone();
    }
}

#[derive(Clone)]
enum ShellAction {
    /// Rename without telling the framework the shared value moved.
    RenameQuietly(&'static str),
    /// Rename and request a render, which is the documented migration.
    RenameAndRequestRender(&'static str),
}

struct Shell {
    title: std::rc::Rc<std::cell::RefCell<String>>,
}

impl Component for Shell {
    type Action = ShellAction;
    type Effect = ();

    fn update(&mut self, action: ShellAction, context: &mut ComponentContext<'_, ()>) {
        match action {
            ShellAction::RenameQuietly(title) => *self.title.borrow_mut() = title.to_owned(),
            ShellAction::RenameAndRequestRender(title) => {
                *self.title.borrow_mut() = title.to_owned();
                context.request_render();
            }
        }
    }

    fn view(&self, _theme: &Theme) -> View<ShellAction> {
        column((component::<Mirror, ShellAction>(
            MirrorProps {
                title: self.title.clone(),
            },
            |()| ShellAction::RenameQuietly(""),
        )
        .keyed("mirror"),))
    }
}

fn mirrored_label(host: &ComponentHost<Shell>) -> Option<String> {
    host.ui()
        .semantic_snapshot()
        .into_iter()
        .find(|node| node.data.role == SemanticRole::Label)
        .map(|node| node.data.label)
}

/// Documents the exact cost of update isolation, and that `request_render` pays it.
///
/// The two halves are deliberately in one test: the stale half is only
/// meaningful next to the fixed half, and separating them invites deleting the
/// uncomfortable one.
#[test]
fn a_child_reading_shared_state_needs_request_render() {
    let shared = std::rc::Rc::new(std::cell::RefCell::new("before".to_owned()));
    let mut host = ComponentHost::new(
        Shell {
            title: shared.clone(),
        },
        VIEWPORT,
        Theme::dark(),
    )
    .expect("shell mounts");
    assert_eq!(mirrored_label(&host).as_deref(), Some("before"));

    // The props are an `Rc`, so they compare equal by pointer no matter what the
    // pointee says. The child is skipped and keeps painting the old title.
    host.dispatch(ShellAction::RenameQuietly("quiet"))
        .expect("quiet rename");
    assert_eq!(
        mirrored_label(&host).as_deref(),
        Some("before"),
        "a skipped boundary cannot see through its own props"
    );

    // `request_render` marks the reducing component stale and disables
    // props-equality pruning for the flush, so the child refreshes.
    host.dispatch(ShellAction::RenameAndRequestRender("loud"))
        .expect("loud rename");
    assert_eq!(mirrored_label(&host).as_deref(), Some("loud"));

    // `mark_dirty` is the same escape hatch for application-owned mutation.
    *shared.borrow_mut() = "external".to_owned();
    host.mark_dirty();
    host.flush().expect("flush");
    assert_eq!(mirrored_label(&host).as_deref(), Some("external"));
}

// ------------------------------------------------------------ future scenarios

/// Wheel-driven scrolling over a virtualized list.
///
/// Not expressible today. `virtual_tree` and `virtual_table` take a
/// caller-supplied visible range and own no row pool, so a wheel event moves
/// the engine's scroll offset without realizing or recycling any rows, and
/// `rows_realized` / `rows_recycled` have no producer to observe. Enable this
/// in the phase that introduces framework-owned row virtualization, and assert
/// that a one-row scroll realizes and recycles a bounded number of rows
/// independent of the total row count.
#[test]
#[ignore = "needs framework-owned row virtualization (later phase)"]
fn wheel_scroll_recycles_a_bounded_number_of_virtualized_rows() {
    unimplemented!("enable with row virtualization");
}

/// Memoized subtrees skipped on an unrelated update.
///
/// Not expressible today: there is no memo boundary to hit, so `memo_hits` has
/// no producer. Enable this in the memoization phase and assert that an action
/// touching one subtree produces `memo_hits` for every sibling subtree whose
/// inputs did not change.
#[test]
#[ignore = "needs subtree memoization (later phase)"]
fn unrelated_update_hits_the_memo_for_untouched_subtrees() {
    unimplemented!("enable with subtree memoization");
}
