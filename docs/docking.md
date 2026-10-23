# Controlled docking tree

`DockTree` is application-owned topology: binary splits and tab groups containing
globally unique panel `Key`s. It holds no entities, UI placements, windows or GPU
resources. `dock(&tree, resolver)` composes it using RXUI's existing controlled
splits and tabs. The resolver supplies each panel's title and arbitrary content.

```rust
use rxui::prelude::*;

fn initial_layout() -> Result<DockTree, DockError> {
    let mut layout = DockTree::from_panels(["editor", "preview"])?;
    let main = layout.root().id();
    let output = layout.split(
        main, DockSide::Bottom, "output", SplitPosition::Fraction(0.7),
    )?;
    layout.move_panel(&Key::from("preview"), output, 1)?;
    Ok(layout)
}
```

Application views describe content and bind a current-state listener:

```rust
use rxui::prelude::*;
struct Workspace { layout: DockTree }
impl View for Workspace {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        dock(&self.layout, |key| {
            dock_panel(format!("{key:?}"), text_input("Document")).closable(true)
        })
        .key("workspace")
        .min_pane_size(160., 120.)
        .on_event(cx.listener(|s, event: &DockEvent, _| {
            let _ = s.layout.apply(event);
        }))
    }
}
```

In an application, resolve keys through a document registry and return an
`Entity<View>` as content. Reading document data through `cx` subscribes the
describing component to that data. The resolver is called once per tree panel in
tree order, including inactive panels. It must describe every live key. The builder
owns its tree snapshot and descriptions; no borrowed model data escapes evaluation.

## Model invariants and edits

Read nodes with `root()`, `node(id)` and `group_for(&panel)`. `DockNode::Tabs` exposes
ordered keys and selection; `DockNode::Split` exposes axis, position and children.
Fields are read-only. Methods enforce these invariants:

- Panel keys are unique throughout one tree.
- Each nonempty group has one selected panel belonging to that group.
- Splits have two nonempty subtrees and finite, valid positions.
- Empty branches collapse. After the last removal, one empty root group remains.
- Failed checked edits leave the tree unchanged.

`DockNodeId` is opaque and unique across independent trees. New nodes do not reuse
removed IDs. Cloning a tree retains IDs for snapshots/undo; restoring that snapshot
restores its logical identities but does not resurrect old UI placement state.
IDs are in-process identities, not a persistent serialization format.

| Operation | Behavior |
| --- | --- |
| `new()` | One empty root group. |
| `from_panels(keys)` | One group with the first key selected; duplicate keys fail. |
| `insert(group, index, key)` | New panel at `0..=len`; preserves selection unless empty. |
| `select(group, &key)` | Selects a member; rejects wrong group/stale node. |
| `resize(split, position)` | Updates only the requested extent; preserves node identity. |
| `remove(&key)` | Removes if present; selects following then preceding neighbor. |
| `move_panel(&key, group, index)` | Reorders or moves, selecting the moved panel in the destination. |
| `split(group, side, new_key, position)` | Adds a new group alongside target; returns the new group's ID. |
| `dock_panel(&key, group, side, position)` | Moves an existing panel into a new adjacent group. |
| `apply(&event)` | Accepts a checked select, close, resize or drop proposal. |

Move indices describe the final position after source removal: for a same-group
reorder of N panels, `0..N`; for a different destination with N panels, `0..=N`.
A moved selected panel leaves the following, then preceding neighbor selected in
its source. Surviving subtree IDs remain unchanged when an empty source collapses.
Refresh structural node references after collapse; panel-key lookup remains valid.

Left/Right create horizontal splits; Top/Bottom create vertical splits. Position
always describes the first/left/top child, excluding the divider, regardless of
which side is new. Fraction and Pixels retain the existing split semantics. An
empty root is populated rather than split. A group's sole panel cannot be split
away from that same group; move it to a different group or add another panel first.

## Events, sizing and ownership

`DockEvent::Select { group, panel }`, `Close { group, panel }`,
`Resize { split, event }` and `Drop { source, panel, target }` are proposals.
The application can ignore, normalize,
defer or accept them. Close has no automatic document/entity disposal. Applying a
deferred close checks the original group, so it cannot accidentally close a panel
that has moved elsewhere. Absent nodes/changed membership return `DockError`.
Resize carries the normal `ResizeEvent`, including Begin/Drag/End/Cancel and
keyboard/accessibility phases. Keep accepting resize lifecycle events if the
application records them, even when the position is unchanged.

Without `on_event`, dividers are read-only and close buttons are unavailable.
Default tab activation proposes selection, so ignoring a selection leaves the
controlled active panel unchanged. Header keyboard focus can still move.

The dock fills a bounded parent. Default leaf minima are 96 by 96 logical units,
with an 8-unit divider. `min_pane_size(width, height)` and `divider_size(size)`
customize them. Minima combine bottom-up: sum plus divider along a split's axis,
maximum across the other axis. Computation is linear in the node count. Below the
combined minima, panes retain their minima and overflow clips; unavailable dividers
are read-only. Invalid minima/thickness produce `UiError::InvalidDockConfiguration`
during preparation, including for a single-group tree; the description can be
corrected and retried. Root sizing and full Taffy customization are available.

The existing tab roles, selected state, header/panel relationships, split roles,
numeric ranges, pointer capture, Escape cancellation and keyboard defaults apply.
There is no additional dock-specific accessibility role or renderer.

Model lookups/edits scan the tree and panel sequences rather than maintaining a
second mutable index. Building a dock description takes linear work in the nodes
and panels and stores an ordered composition snapshot, plus the resolver's own work.
It does not clone a second `DockTree` or build an all-panel hash map. This happens when
the owning view evaluates, not as an independent per-frame dock update. Drawing
uses existing retained UI preparation and rendering; no extra framebuffer or
composition pass is introduced by the docking tree. Large panel collections and
description allocation remain candidates for the later optimization pass.

## Header dragging and drop feedback

Header dragging is enabled by default when `on_event` is attached. A primary press
captures the header in its source UI placement. Motion becomes a drag at six
logical units; a smaller motion/release remains an ordinary tab click. Close
buttons and nested ordinary tab groups never start dock drags. `.draggable(false)`
disables the gesture while retaining normal tab selection/keyboard behavior.
`.drag_threshold(value)` changes the finite, nonnegative threshold.

By default destinations are restricted to the same dock root and window. Explicit
surface bindings can opt into transfers between regions, as described below:

- Header strips show an insertion marker before/after the nearest header midpoint.
  The proposal's index is adjusted for source removal, including same-group reorder.
- The center of another panel body highlights its body and appends to that group.
- The outer quarter of a body chooses the closest Left/Right/Top/Bottom edge and
  previews a new adjacent pane. Stock split drops propose `Fraction(0.5)`.

No-op same-group drops, splitting a group's sole panel away from itself, split
zones too small for both configured minima, blocked overlays, different dock roots
and positions outside eligible groups have no destination. Capture continues when
the pointer leaves the viewport, but its preview clears. Release hit-tests again
against current geometry; coalesced motion cannot commit an old highlighted target.
Escape and host/target cancellation clear the gesture without changing topology.
Routed listeners can prevent the stock gesture, and preventing Escape preserves
the application's control of cancellation.

Only a completed eligible drop emits
`DockEvent::Drop { source, panel, target: DockDropTarget }`. Stock motion emits no
DockEvent, mutates no tree data and does not dirty its owning view. Routed pointer
listeners still run and can update application state. Accepting `apply`
checks original source membership and delegates to the same checked move/split
operations. Rejection or cancellation leaves the layout unchanged. The drag and
capture clear before application callbacks, including callbacks that fail.
After preparation, an accepted drop focuses the panel's selected header in its new
group. An explicit queued focus request takes precedence over this restoration.

`Ui::dock_drag()` exposes a borrowed `DockDragInfo`: source group, panel key,
logical pointer position and optional `DockDropPreview`. Preview bounds represent
the current destination, not a simulated post-collapse layout; final pane allocation
can grow or change after source removal. Destination geometry refreshes during UI
preparation, including scroll/layout changes.

`UiPainter` draws a transient overlay after ordinary and isolated content, using
the source dock's Focus theme color. Pane previews use a translucent fill and outline;
header markers are solid. Feedback respects dock clipping, destination viewport,
DPI, caller scissor and source ancestor opacity. It adds shape draws without an
extra framebuffer or pass and does not participate in hit testing. Use
`.drop_preview(false)` to paint custom feedback from `Ui::dock_drag()`; gesture
recognition, cursor feedback and drop proposals remain enabled.

The stock gesture operates within one `Ui`, including explicitly bound regions.
It does not pick/drop panels through keyboard accessibility actions or automatically
coordinate native cross-window drag sessions.
Ordinary tab keyboard behavior remains available, and application buttons/commands
can expose checked reorder/move/split operations. Overflowing header strips retain
their existing wheel scrolling; stationary edge autoscroll and drag ghosts are
separate interaction polish.

`Dock::on_context_menu` receives header secondary-click and keyboard requests with
group, panel and logical position, without changing selection or starting a drag.
Applications supply controlled popup content and typed actions; see
[overlays-and-commands.md](overlays-and-commands.md) and the standalone
[desktop_window example](../crates/rxui/examples/desktop_window.rs).

## Retained placement boundaries

`TabContentPolicy::KeepMounted` is the default. Selection changes and reordering
within a surviving group preserve compatible keyed mounts, editing selection,
scroll and remembered focus. `MountSelected` releases inactive placements.

Moving between groups, inserting a split around a group, collapsing an ancestor
or rebuilding the tree changes structural parentage. Such changes can remount
content, clear focus and cancel pointer capture or mount-scoped tasks. KeepMounted
does not transfer a retained subtree to a new parent. Strong document entities
retain application data. Store document edits in entities; treat caret/scroll as
placement state, or explicitly model them if application behavior requires transfer.
Weak focus/scroll placements do not follow reparented replacements.

A shared workspace entity shares its docking layout, selections and requested
split sizes between windows. Each window has independent caret, scroll, capture
and focus. Separate workspace entities referencing shared documents provide
independent layouts. Native detach will need an explicit lifecycle/placement policy.

## Example and verification

Run the standalone [docking_window example](../crates/rxui/examples/docking_window.rs):

```sh
cargo run -p rxui --example docking_window --features native --locked
```

It demonstrates nested splits, editable panel entities, controlled resizing,
same-group reordering, moving/splitting preview, closing, adding tabs, mounting
policy, pointer header drops and a shared window. Restore layout reopens retained
document models.
It contains no support module or smoke-test mode.

Tests cover all split sides, recursive collapse, IDs, insertion/reorder/move,
selection, failed edits, stale events, repeated topology edits, bounded nested
layout, read-only constrained dividers, controlled acceptance/rejection, retained
same-group editing and capture cancellation/remount boundaries. A real-font GPU
test samples each visible panel's background after splitting, moving and collapsing,
including closing the final panel, and checks GPU validation errors.

Header gesture tests cover thresholds, all drop zones, controlled rejection,
release-time hit testing, blockers, nested ordinary tabs, cancellation, stale
sources, callback errors and placement-local focus restoration. Repeated stock
motion leaves the owning entity revision and component evaluation count unchanged.
A second real-font GPU test verifies the preview with 4x MSAA, 2x DPI, a custom
theme, isolated content, caller viewport/scissor, disabled stock feedback and
cancellation; it also checks GPU validation errors.

macOS computer-use checks exercised note editing, numeric scrollbar/divider actions,
tab switching and same-group reorder with a retained 200-unit scroll offset,
cross-group moves, splitting Preview, recursive close/collapse through an empty dock,
adding a new panel, restoring document models, mounting-policy changes and opening
a shared window. Editing and resizing in the shared window propagated to the
original. The temporary copies used normal window attributes with background
creation, and both windows were closed afterward. These are functional checks,
not frame-pacing measurements or full VoiceOver usability certification.

Additional macOS computer-use checks dragged headers to reorder, merge into another
group and split at an edge. A document edit survived a cross-group move, and focus
followed the selected header after a drop. These checks used a temporary copy of
the standalone example in a normal background-created window, which was closed
afterward. Held-drag preview pixels and cancellation are covered by the automated
tests above.

## Lazy content and application drop policy

`dock_panel_lazy(title, move || content)` defers content construction until the dock
is converted into an element. With `MountSelected`, inactive closures are dropped
without running. `KeepMounted` constructs all content. Header resolution remains
eager, once per live panel in tree order, including inactive panels. Existing
`dock_panel` remains eager. This is deferred construction, not memoization: later
view evaluations may construct the content again. Read entity dependencies through
`cx` in the eager resolver and move owned values into the `'static` closure.

`.drop_policy(move |query: &DockDropQuery| ...)` filters stock targets before a
preview appears and again at release. The query includes panel, original group,
target, and optional source/destination surface IDs. For example, match `panel`
to keep a tool in its region, or reject `DockDropTarget::Split` to allow tab-only
transfers. Between regions both source and destination policies must accept.
Policies should be inexpensive and free of side effects. They read snapshots
captured during description; final event acceptance remains application-owned.
Rejecting a target never turns that target into a detach request.

## Multiple surfaces, regions and independent windows

`DockWorkspace` owns several `DockTree`s and enforces unique panel keys across
all surfaces. `DockSurfaceId` is an opaque in-process identity. Trees are exposed
read-only through `surface` and `surfaces`; edits use `apply`, `select`, `insert`,
`split`, `transfer` and `detach`. `create_surface` accepts an owned tree;
`remove_surface` returns its owned tree and does not destroy document entities or
close a native window. Snapshot cloning retains surface and node IDs.

```rust
use rxui::prelude::*;
let mut workspace = DockWorkspace::new();
let left = workspace.create_surface(DockTree::from_panels(["editor", "preview"])?)?;
let right = workspace.create_surface(DockTree::new())?;
let source = workspace.surface(left).unwrap().group_for(&Key::from("preview")).unwrap();
let target = workspace.surface(right).unwrap().root().id();
let change = workspace.transfer(left, source, &Key::from("preview"), right,
    DockDropTarget::Tab { group: target, index: 0 })?;
assert_eq!(change.destination_surface, right);
# Ok::<(), DockWorkspaceError>(())
```

Transfers validate original source membership. Failed transfers leave both trees
unchanged. Accepted cross-surface transfers stage only the two affected trees;
same-surface operations use the existing checked edit directly. The result reports
destination group, whether anything changed, and whether the source is empty.
An empty source surface remains until explicitly removed. Detach creates a new
surface atomically and returns the same result; reattach is an ordinary transfer.
Windows, document models, undo storage and persistence remain application-owned.

For stock dragging between regions in one `Ui`, bind each dock with
`.surface(surface_id).transfer_scope(workspace_key)` and attach `.on_transfer(...)`
to the source. Scope keys must distinguish independent workspaces within that Ui.
Unbound docks retain their original isolation. `DockTransferEvent` carries both
surface IDs, original group, panel and target. Accept it with:

```rust,ignore
.on_transfer(cx.listener(|s, e: &DockTransferEvent, _| {
    let _ = s.workspace.transfer(e.source_surface, e.source_group, &e.panel,
        e.target_surface, e.target);
}))
```

`.surface(id).on_detach(...)` opts into `DockDetachEvent` on a completed drag
released outside every visible dock root. Its position is in logical source-window
coordinates, including coordinates outside the client area; it is not a desktop
position. Blocked, denied and invalid destinations do not detach. The gesture and
capture clear before callbacks. Decide whether to create a native window at release,
then publish the detached layout. Creating/focusing a window during motion can
cancel source capture. Keep the old layout if window queueing fails, and handle
later native creation failures through the host's normal error lifecycle.

The [docking_surfaces_window example](../crates/rxui/examples/docking_surfaces_window.rs)
shows two regions, independent native window layouts, shared editable document
entities, detach buttons/gestures, and reattachment on button or window close.
The main window stays open while detached windows still need a return destination.

```sh
mbx run -p rxui --example docking_surfaces_window --features native --locked
```

Cross-group/surface/window transfers can remount content. Strong document entities
preserve document data; mount-local caret, scroll, focus, tasks and `use_state`
do not automatically follow. `KeepMounted` preserves inactive content within a
surviving parent; it is not a placement-transfer guarantee.

## Cross-window coordination for custom hosts

Automatic native cross-window dragging is not installed by `Application`.
Custom hosts can use the explicit `DockDragSession` bridge between separate Uis:

1. During motion, query `source.dock_drag_session()`, then
   `destination.external_dock_preview(&snapshot, destination_logical_position)`.
   The snapshot cannot commit. Paint any external preview through the host.
2. At native primary-button release, arm the source with
   `source.prepare_external_dock_release()`, then route the normal release through
   source `WindowInput`. Its captured listeners receive the release and can prevent
   transfer. Normal release routing clears capture, pressed state and host button
   bookkeeping, and suppresses local drop/detach when an external release is armed.
3. Call `source.take_dock_drag_session()` immediately after routing to obtain the
   non-cloneable commit token. Cancellation or prevention yields no token; another
   source pointer, key or wheel input expires an untaken token, as does deactivation.
   Request a source redraw to erase feedback.
   Call `destination.drop_external_dock(runtime, token, position)`,
   then request destination preparation/redraw. The token is consumed; dropping it
   cancels. Destination geometry and policy are checked again before invoking the
   original source listener. The application must accept through checked transfer.

The bridge requires explicit surface/scope bindings. Source policy is captured in
the session; destination policy is current. Source deactivation/removal before
release retains normal cancellation. Hosts must determine the actual
native destination, including occlusion/stacking, and map coordinates correctly.
Raw window rectangles cannot establish native z-order or foreign-window occlusion.
macOS window origins from winit must be normalized for each window's scale before
mixed-DPI mapping; Wayland does not expose general global window coordinates.
Pointer locking/confinement is not a substitute for cross-window drag routing.

## CPU measurements

`mbx bench -p rxui --bench docking` emits CSV batches for description, selection,
resize and preparation using deterministic CPU text measurement. Description
timing includes construction, conversion and destruction. Local medians for 256
tabs in one group changed from 541.54 to 371.82 microseconds with `KeepMounted`
(31% lower), and from 358.87 to 189.49 with `MountSelected` (47% lower). An added
32-label-per-panel case measured 873.47 microseconds eager versus 174.83 lazy with
`MountSelected`; this compares two content construction APIs after the refactor.

These are illustrative local measurements, not portable thresholds. Concurrent
build activity and repeated runs varied timings. Resize/preparation showed no
clear gain (about 3% slower in the sampled 16-group cases); no GPU frame-pacing
claim follows from these CPU measurements. The benchmark separates these paths
so later indexing or invalidation work can be justified by evidence.

Undo/persistence, retained placement transfer, automatic native window targeting,
per-group sizing and custom header composition remain separate work.
