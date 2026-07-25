# Update isolation

Reducing a component action used to rebuild the whole tree. It no longer does.
This document defines the new update model and the one migration it forces.

## The model

An action never rebuilds anything. Reducing it does three things: it mutates
component state, it maps effects toward the parent, and it records the reducing
component as stale.

The runtime then flushes:

1. The **root** component's view is built only if the root's own state changed.
2. Remaining stale components are drained in `(depth, id)` order, so a parent
   always rebuilds before its descendants and a child reached by its parent's
   cascade is never rebuilt twice.
3. The retained passes run **once**, at the end.

At each `component` boundary the subtree is rebuilt only when at least one of
the following holds:

- its `Props` compare unequal to the previous ones,
- the `Theme::revision` it last rendered at changed,
- it reduced one of its own actions since the last flush,
- an enclosing scope called `request_render()` or `mark_dirty()`.

Otherwise the boundary is skipped: no `view()` call, no diff, no retained
mutation anywhere below it.

Batches of actions cost one flush, not one per action. Use `dispatch_all` and
`dispatch_all_erased` when you have more than one action in hand; the native
window host already does this for every event it drains.

## What this changes for you

`Props: Clone + PartialEq` was previously advisory. It is now load-bearing:
**props equality is the framework's only evidence that a subtree's output cannot
have changed.**

Two component shapes break.

### 1. `Props` that under-reports change

```rust
#[derive(Clone, PartialEq)]
struct RowProps {
    id: u64,
    // `label` is missing from Props but read by `view`
}
```

Anything `view` reads must be in `Props`, or reachable only through this
component's own actions. A `PartialEq` that reports equality for values that
render differently now yields a stale subtree rather than a redundant rebuild.

Note that a hand-written `PartialEq` which ignores a field - a common trick for
skipping an expensive comparison - is exactly this bug.

### 2. `view` that reads state the framework cannot see

```rust
struct Inspector {
    document: Rc<RefCell<Document>>, // shared with an ancestor
}

impl Component for Inspector {
    fn view(&self, _theme: &Theme) -> View<InspectorAction> {
        label(self.document.borrow().title.clone()) // stale after an ancestor edit
    }
}
```

Interior mutability, `Arc<Mutex<_>>`, statics, clocks, and any handle shared
with an ancestor all fall here. The ancestor's edit changes no props on the
`Inspector` boundary, so the boundary is skipped and the old title keeps
painting.

There are two fixes, in order of preference.

**Put the value in props.** Either the value itself, or a revision counter that
changes whenever the shared state does:

```rust
#[derive(Clone, PartialEq)]
struct InspectorProps {
    document_revision: u64,
}
```

This is the fix to reach for. It keeps the isolation, and it makes the data
dependency visible at the call site.

**Or call `request_render()`** from the reducer that mutated the shared state:

```rust
fn update(&mut self, action: EditorAction, cx: &mut ComponentContext<'_, Effect>) {
    match action {
        EditorAction::Rename(name) => {
            self.document.borrow_mut().title = name;
            cx.request_render(); // descendants read `document` without props
        }
    }
}
```

`request_render()` marks the reducing component stale *and* disables
props-equality pruning for that whole flush, so descendants sharing the same
state refresh too. That is deliberately blunt: it reinstates the old
unconditional whole-subtree work for one frame. Treat it as an escape hatch, not
a pattern.

For state the application owns outside any component, the equivalent entry
points on the host are:

- `mark_dirty()` - mark the root stale and force the next flush, without
  reconciling yet. Use it when the resulting frame will be flushed alongside
  other pending actions.
- `refresh()` - `mark_dirty()` plus an immediate `flush()`. Its behavior is
  unchanged from before update isolation: a full unconditional rebuild.

## Themes

A theme change cascades through `Theme::revision`. Every mounted component
records the revision it rendered at and rebuilds when it differs, which is what
lets a global restyle reach a component whose props did not change.

**Bump `revision` whenever you change any token.** `set_theme` compares the
whole `Theme` and forces the pass if tokens differ while `revision` does not, so
forgetting to bump it still produces a correct frame - but it produces it by
falling back to a full rebuild, and nothing else in the framework will notice
the omission.

## Keys

`ViewKey` no longer stores every key as a string. `.key(item.id)` keeps a `u64`
and `.key("header")` keeps a `&'static str`, so neither allocates per frame.

Two consequences:

- `From<&str>` narrowed to `From<&'static str>`. Key a non-static borrowed
  string with `ViewKey::new(text)` or `.key(text.to_owned())`.
- `ViewKey::from(7u64)` is no longer equal to `ViewKey::from("7")`. Keys from a
  literal and from a `String` still interoperate; numeric and textual keys no
  longer do. Mixing the two spaces inside one collection was never meaningful.

## Verifying isolation

`crates/rxui/tests/update_model_gate.rs` asserts exact `ViewStats` counters for
each interaction shape, and every scenario ends in
`assert_incremental_matches_fresh`, which mounts a second host from the same
final state and compares semantic and fragment/geometry digests. A skipped
subtree that should not have been skipped fails there, not in the counters.

If you suspect staleness in your own component, the same technique works: mount
a fresh host from the state you expect and compare
`ui().semantic_snapshot()`.
