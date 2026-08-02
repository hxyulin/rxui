# Update isolation

Mutating an entity does not implicitly rebuild the application. This document
defines the v2 entity update model, the dependencies authors must make
explicit, and the exact-statistics contract that prevents isolation from
regressing.

## The model

`Entity::update` borrows exactly one entity. A listener or update callback may
change its state, but rendering begins only when it calls `cx.notify()`.
Notifications and emitted events are queued until the current entity borrow
ends, so neither rendering nor a subscription callback can observe a
half-written state.

`App::flush()` then does the following:

1. It drains queued notifications and typed events. Events emitted by a
   subscription join the same drain; a 65,536-effect cap turns a cycle into a
   clear panic instead of starving the frame.
2. It renders notified mounted entities in `(depth, EntityId)` order. Parents
   therefore render before children, and the ordered dirty set coalesces
   repeated notifications for one entity.
3. Each rendered `Element` is reconciled against that entity's retained
   boundary. Unchanged child order is not republished.
4. Retained layout, composition, paint, and accessibility passes run once, at
   the end of the flush.

An entity boundary is an isolation gate. If a parent renders and inserts the
same child `Entity`, reconciliation retains the boundary without calling the
child's `Render` implementation. Conversely, notifying a child does not call
its parent's `Render` implementation. State ownership, rather than a props
comparison, tells the runtime which output may have changed.

Batches cost one render per distinct notified entity and one retained pass.
Perform the updates first and call `App::flush()` after the batch. Input driven
through `EntityHarness` already routes its `ActionBox<RoutedHandler>` through
`App` and flushes before returning.

## What this requires from entity authors

### Notify after render-visible state changes

```rust
button("+").on_click(cx.listener(|this, _, cx| {
    this.value += 1;
    cx.notify();
}))
```

Without `notify`, the state change is valid and observable through
`Entity::read`, but the retained view intentionally stays unchanged. This is
useful for caches and non-visual bookkeeping; it is a bug when `render` reads
the changed value.

Calling `notify` more than once before a flush is safe. The dirty set retains
one entry for the entity.

### Make shared-state invalidation explicit

```rust
struct Inspector {
    document: Rc<RefCell<Document>>,
}
```

If several entities read the same interior-mutable value, the runtime cannot
infer which render outputs depend on it. The update that mutates the document
must notify every dependent entity, or the shared value should be owned by one
entity and exposed through typed events to the others. `Rc<RefCell<_>>`,
`Arc<Mutex<_>>`, statics, clocks, and external handles all have the same rule.

Prefer visible ownership and targeted notifications. There is deliberately no
whole-subtree `request_render` escape hatch in the Stage 3 surface.

### Treat same-entity reentrancy as an authoring error

Updating a different entity during an update is supported. Updating the same
entity while it is already borrowed panics and names its `EntityId`. Defer that
work through an event or return to the outer callback instead. Routed handlers
and subscriptions use weak targets, so work addressed to a dropped entity is
silently pruned rather than resurrecting it.

## Themes

`Theme::revision` is the application-wide style invalidation identity.
`App::set_theme` compares the replacement theme and marks every mounted entity
dirty when it changes; advancing `revision` is the documented way to identify
a new token set. The next flush renders all boundaries in depth order and runs
the retained passes once.

Always advance `revision` when changing a token. `set_theme` also detects a
whole-theme difference at an unchanged revision to preserve correctness, but
code outside that entry point may use the revision as its cache identity.

## Keys and retained identity

Keys govern identity only among siblings in a reorderable collection. Static
children remain positional. A dynamic sibling list must key either every child
or none; mixed and duplicate keys panic because either case makes retained
identity ambiguous.

On a keyed reverse, reconciliation matches existing children by key, updates
them in place, and publishes the new order once. The `NodeId` attached to each
item survives. On an unchanged keyed render, the positional fast path avoids a
map and the published-order memo avoids `UiTree::set_children` entirely.

## Verifying isolation

`crates/rxui-core/tests/update_model_gate.rs` is the ratchet. Its counters are
deterministic—there is no timing or sampling—and every expected `PassStats` and
`ViewStats` value is asserted with exact equality.

The two layers answer different questions:

- `ViewStats` counts entity renders and description reconciliation above the
  retained boundary. A one-child notification must have the same `ViewStats`
  with 4 siblings and with 64, proving untouched siblings add no view work.
- `PassStats` records layout, fragments, text, and accessibility below the
  boundary. Exact values make engine work visible even when isolation above it
  is unchanged.

The gate preserves these invariants:

- notifying an entity whose render output is unchanged performs one render and
  reconciliation, builds zero nodes, republishes zero child lists, and causes
  zero retained mutations;
- notifying a child renders that child once and its parent zero times;
- the child update's complete `ViewStats` is identical at small and large
  sibling counts;
- a theme revision change renders every mounted entity exactly once;
- memo and row-virtualization counters remain exactly zero until those systems
  gain producers.

Counter and todo integration tests add observable correctness: typed `Saved`
events reach a live subscription, and add/remove/toggle/reverse operations keep
the `NodeId` of every surviving keyed todo row. When changing the runtime, an
expected counter may move in either direction, but the gate must be edited
deliberately with an explanation of the new work model.
