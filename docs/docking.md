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
| `apply(&event)` | Accepts a checked select, close or resize proposal. |

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

`DockEvent::Select { group, panel }`, `Close { group, panel }` and
`Resize { split, event }` are proposals. The application can ignore, normalize,
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
second mutable index. Building a dock description clones the topology and visits
each node/panel once, plus the application resolver's own work. This happens when
the owning view evaluates, not as an independent per-frame dock update. Drawing
uses existing retained UI preparation and rendering; no extra framebuffer or
composition pass is introduced by the docking tree. Large panel collections and
description allocation remain candidates for the later optimization pass.

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
policy and a shared window. Restore layout reopens retained document models.
It contains no support module or smoke-test mode.

Tests cover all split sides, recursive collapse, IDs, insertion/reorder/move,
selection, failed edits, stale events, repeated topology edits, bounded nested
layout, read-only constrained dividers, controlled acceptance/rejection, retained
same-group editing and capture cancellation/remount boundaries. A real-font GPU
test samples each visible panel's background after splitting, moving and collapsing,
including closing the final panel, and checks GPU validation errors.

macOS computer-use checks exercised note editing, numeric scrollbar/divider actions,
tab switching and same-group reorder with a retained 200-unit scroll offset,
cross-group moves, splitting Preview, recursive close/collapse through an empty dock,
adding a new panel, restoring document models, mounting-policy changes and opening
a shared window. Editing and resizing in the shared window propagated to the
original. The temporary copies used normal window attributes with background
creation, and both windows were closed afterward. These are functional checks,
not frame-pacing measurements or full VoiceOver usability certification.

This stage provides the tree and programmatic edits. Header dragging, drop-zone
hit testing/previews, undo/persistence, placement transfer and native cross-window
detach are separate work. The next interaction stage can turn drag gestures into
these same checked move/split operations.
