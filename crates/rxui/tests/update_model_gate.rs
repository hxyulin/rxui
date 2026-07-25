//! Update-model gate: a recorded baseline of per-interaction framework work.
//!
//! The framework currently does full work on every interaction. A nested
//! component's action rebuilds that component's own subtree, and then the root
//! view is rebuilt unconditionally and the whole shadow tree is diffed. The
//! engine's `PassStats` cannot see any of that, because it measures layout,
//! paint, and accessibility work *below* the component boundary.
//!
//! Every number asserted here is a measurement of today's behavior, not a
//! target. `ViewStats` counters are deterministic — no timing, no sampling —
//! so each is asserted with exact equality and confirmed stable across
//! repeated serial and parallel runs. A later update-isolation phase is
//! expected to ratchet them down; a change in either direction should force a
//! deliberate edit here.
//!
//! Every scenario ends in [`assert_incremental_matches_fresh`], which compares
//! the incrementally updated tree against a second host mounted fresh from the
//! same final state. That is what stops a future counter "win" from being
//! bought by leaving retained state stale.

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
    // 7 = the nodes the pointer walked to reach the button. Hit testing runs in
    // `dispatch`, before the pass, and the engine now carries this counter
    // across `update_passes`'s reset so it stays readable afterwards; it read a
    // structural 0 here before that fix.
    assert_eq!(pass.hit_test_nodes, 7);

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
fn keystroke_into_a_nested_child_field_rebuilds_the_whole_root() {
    let mut host = mount_workspace(Theme::dark());
    let point = trailing_edge(&find(&host, "Note"));
    host.input(UiInput::PointerPressed(point)).expect("focus");

    let ((), view) = ViewStats::measure(|| {
        host.input(keystroke("!")).expect("keystroke");
    });
    let pass = host.ui().stats();

    assert_eq!(host.component().note, "!");

    // This is the headline case. One character typed into a field owned by a
    // nested child costs three user `view()` calls:
    //   1. the child reduces its own action and rebuilds its own subtree
    //      (`ComponentState::rebuild_child` from the routing pass);
    //   2. the root re-renders unconditionally in `dispatch_erased`;
    //   3. that root rebuild reconciles the child's `ComponentView`, which
    //      calls `rebuild_child` a *second* time for the same keystroke.
    // Update isolation should reduce this to 1.
    assert_eq!(view.component_views, 3);
    // 0 = retained identity is fully preserved; nothing is remounted.
    assert_eq!(view.nodes_built, 0);
    // 48 = 45 view nodes below the root, diffed by the full root rebuild,
    // plus the child's 3 nodes (column, caption, field) diffed a second time
    // by the routing pass that ran before it.
    assert_eq!(view.nodes_rebuilt, 48);
    // 17 = 16 containers in the tree (outer column, toolbar row, list column,
    // 12 keyed rows, the child's column) plus the child's column again from
    // the routing pass.
    assert_eq!(view.containers_reconciled, 17);
    // Equal to `containers_reconciled`: today every reconciliation pass
    // republishes its child list even when the order is unchanged.
    assert_eq!(view.set_children_calls, 17);
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
fn selection_change_rebuilds_the_whole_tree() {
    let mut host = mount_workspace(Theme::dark());

    let ((), view) = ViewStats::measure(|| {
        host.dispatch(WorkspaceAction::Select(5)).expect("select");
    });
    let pass = host.ui().stats();

    // 2 = the root re-render, plus the nested editor re-rendering because the
    // root rebuild walks through its `ComponentView` — even though selection
    // cannot affect the editor at all.
    assert_eq!(view.component_views, 2);
    // 0 = keyed rows keep their retained identity across the selection move.
    assert_eq!(view.nodes_built, 0);
    // 45 = every view node below the root, for a change that affects 2 of
    // them (the previously and newly selected marks).
    assert_eq!(view.nodes_rebuilt, 45);
    // 16 = every container in the tree.
    assert_eq!(view.containers_reconciled, 16);
    assert_eq!(view.set_children_calls, 16);
    assert_no_memo_or_row_activity("selection", view);

    // The engine correctly localizes the same change: 7 = the two changed
    // marks and their ancestors, 2 fragments repainted, 44 reused, 2
    // accessibility nodes changed (selected went false/true), no re-shaping.
    assert_eq!(pass.layout_elements, 7);
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
    assert_eq!(view.component_views, 2);
    assert_eq!(view.nodes_built, 0);
    assert_eq!(view.nodes_rebuilt, 45);
    assert_eq!(view.containers_reconciled, 16);
    assert_eq!(view.set_children_calls, 16);
    assert_no_memo_or_row_activity("theme", view);

    // Unlike every other scenario, the engine does real work here: 46 = every
    // element re-measures and 28 of 46 fragments repaint.
    assert_eq!(pass.layout_elements, 46);
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
fn one_action_into_one_panel_is_not_independent_of_sibling_count() {
    let (small, small_pass) = bump_first_panel(4);
    let (large, large_pass) = bump_first_panel(64);

    // THE ASSERTION THAT MATTERS. Bumping one panel is logically independent
    // of how many siblings exist, so once update isolation lands these two
    // `ViewStats` must be *equal*, and this test should become
    // `assert_eq!(small, large)`.
    //
    // Today they are not, because the routed action's own subtree rebuild is
    // followed by an unconditional root re-render that walks and re-renders
    // every sibling panel. The measured values below are recorded reality.
    assert_ne!(
        small, large,
        "if this now passes, update isolation landed: replace this whole \
         block with assert_eq!(small, large)"
    );

    // component_views = N + 2:
    //   1 for the bumped panel reducing its own action and rebuilding itself,
    //   1 for the root re-render,
    //   N for every panel re-rendered by that root rebuild (including the
    //     bumped one, for the second time).
    assert_eq!(small.component_views, 6); // 4 + 2
    assert_eq!(large.component_views, 66); // 64 + 2

    // nodes_rebuilt = 5N + 5: the root column plus, per panel, its component
    // boundary and its row of three children (4N + 1 + N), plus the bumped
    // panel's own 4-node subtree diffed again by the routing pass.
    assert_eq!(small.nodes_rebuilt, 25); // 5 * 4 + 5
    assert_eq!(large.nodes_rebuilt, 325); // 5 * 64 + 5

    // containers_reconciled = N + 2: the root column, every panel's row, and
    // the bumped panel's row a second time.
    assert_eq!(small.containers_reconciled, 6);
    assert_eq!(large.containers_reconciled, 66);
    assert_eq!(small.set_children_calls, 6);
    assert_eq!(large.set_children_calls, 66);

    // Nothing is remounted at either size.
    assert_eq!(small.nodes_built, 0);
    assert_eq!(large.nodes_built, 0);
    assert_no_memo_or_row_activity("panels N=4", small);
    assert_no_memo_or_row_activity("panels N=64", large);

    // The engine, by contrast, already *is* independent of sibling count:
    // identical layout, paint, accessibility, and shaping work at both sizes.
    // Only `reused_fragments` grows, and only because there are more
    // untouched fragments to reuse. This is the shape the view counters above
    // should eventually take.
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
