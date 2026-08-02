# RXUI v2: Ground-Up Rewrite

Status: approved design, implementation not started.
Companion document: [v2-implementation.md](v2-implementation.md) — the staged implementation plan.

## Context

RXUI v1 went through two architectures: the message/`App` model (archived as
`archive/v1-main`, ~34k lines, 8 crates) and the Elm-style `Component` model
built over astrelis's `astrelis-ui-next` retained tree (archived as
`archive/v1-ui-next-migration`, ~16k lines, 5 crates). The second line was
mechanically healthy — it built, its ~190 tests passed, clippy and fmt were
clean — but it was set aside for architectural reasons:

- **API ergonomics** are the primary goal for v2, with GPUI as the bar. The v1
  frictions: buttons take action *values* not closures (`Action: Clone` leaks
  onto every control); style variants are `_with`-suffixed sibling functions
  with style-before-children; manual `.key()` on essentially every child;
  tuple children capped at 8 with a `views()` adapter; verbose child-component
  nesting; `Props: PartialEq` is a silent stale-UI footgun; pervasive
  `Result<_, UiError>` for errors that cannot fail in practice.
- **Engine/framework split**: v1 had no boundary — astrelis types (`UiRoot`,
  `NodeId`, `Element`, `Invalidation`) ran through rxui's public API, and the
  unpublished `astrelis-ui-next` blocked publishing.
- **Performance and code quality**: the incremental dirty-bit engine is proven
  fast (0.147 ms selection on a 10k-row model, per `astrelis/rfcs/ui-next.md`);
  that property must survive the rewrite.

### Fixed decisions

1. **rxui owns the retained tree.** `astrelis-ui-next` (~6.1k lines) moves into
   rxui as `rxui-tree`; astrelis shrinks to a platform/GPU/paint/text/compositor
   provider. No renderer-trait abstraction — rxui depends on astrelis crates
   directly, the way GPUI depends on its own platform layer.
2. **Programming model: entity-retained hybrid** (see below).
3. **Fresh start in this repo.** Both v1 lines live on as `archive/*` branches;
   v2 is a new root history on `main`.
4. **Salvage internals**: the keyed reconciler, depth-ordered dirty flush,
   label-addressed test harness, and ViewStats instrumentation are ported; the
   public API is rewritten from scratch.

## Programming model: entity-retained hybrid (GPUI surface, rxui engine)

Four candidates were evaluated:

- **Elm-refined** — message-enum ceremony is intrinsic to the model; builders
  can be fixed but `Action` enums, `Effect` mapping, and `Props: PartialEq`
  cannot be removed.
- **Pure GPUI** — rebuilds the whole window element tree every frame. Fine for
  chat-app-sized trees, but abandons the zero-work-when-idle / O(change)
  property the ui-next engine proved (its adoption gate — "local paint changes
  rebuild one fragment" — is unachievable), and makes deterministic ViewStats
  testing meaningless.
- **Xilem** — elegant closures, but every view is generic over app state;
  `adapt`/`lens` plumbing and hostile compile errors.
- **Signals (Leptos/Dioxus)** — finest granularity, but requires a second
  reactive runtime stacked on the tree's existing dirty-bit invalidation. Two
  invalidation graphs is the worst code-quality outcome.

**The chosen hybrid**: GPUI's *authoring model* — `Entity<T>`, `Context<T>`, a
`Render` trait, `cx.listener(...)` closure handlers, `cx.notify()`,
`cx.emit`/`cx.subscribe` typed events, fluent chainable builders with
`.child()`/`.children()` — over rxui's *execution model*: `render()` produces a
lightweight description that the salvaged allocation-free reconciler diffs into
the retained dirty-bit tree, and only notified entities re-render (the salvaged
depth-ordered flush, keyed by entity).

Target API:

```rust
struct Counter { value: i32 }
struct Saved(i32);
impl EventEmitter<Saved> for Counter {}

impl Render for Counter {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        column()
            .gap(12.0)
            .child(label(format!("Value: {}", self.value)))
            .child(row().gap(8.0)
                .child(button("−").on_click(cx.listener(|this, _, cx| {
                    this.value -= 1; cx.notify();
                })))
                .child(button("+").on_click(cx.listener(|this, _, cx| {
                    this.value += 1; cx.notify();
                })))
                .child(button("Save").on_click(cx.listener(|this, _, cx| {
                    cx.emit(Saved(this.value));
                }))))
    }
}
```

Dynamic children:
`.children(self.items.iter().map(|item| row().key(item.id).child(...)))` —
keys are required only inside reorderable collections; static children are
positional. `cx.listener` borrows only `cx` (which holds a weak self-handle),
so listeners can be built while iterating `&self.items`; the
`&mut self` / `&mut Context` split is what makes this compile.

How each v1 friction dissolves:

- Closures replace action enums; `Effect` plumbing becomes `cx.emit`/`cx.subscribe`.
- Fluent builders replace `_with` siblings and tuple-arity limits.
- `.key()` optional for static children (structural position is identity).
- `Props: PartialEq` gone: a child entity re-renders when *it* is notified,
  never because a parent guessed.
- `Element` is concrete, not generic over an action type — routing is entity id
  plus boxed closure (`RoutedHandler { target: EntityId, invoke: Box<dyn
  FnOnce(&mut dyn Any, &mut App)> }`, a direct evolution of v1's
  `RoutedComponentAction`). Reentrancy is handled GPUI-style with a
  deferred-effects queue.
- **Error policy**: `Result<_, UiError>` leaves the authoring surface. Tree
  invariants become `debug_assert!`/panics (v1's reconciler already documented
  them as "authoring mistakes"). Fallible APIs remain only for window/GPU init.
- Stateless view functions (plain `fn(...) -> Element`, no entity) are offered
  for leaf composition — they are free in this model.

Performance is preserved because the engine is untouched: idle frames are
zero-work, dirty-entity renders are O(change), and ViewStats/PassStats
exact-equality testing still works.

### GPUI license note

GPUI is Apache-2.0 (the rest of Zed is GPL/AGPL, but the `gpui` crate itself is
Apache-2.0). RXUI takes only API *ideas* — entity handles, listener closures,
fluent builders — which copyright does not protect; no GPUI code is copied, so
no license obligation attaches and rxui stays MIT. If code were ever copied
from GPUI, Apache-2.0 → MIT-project inclusion is permitted but requires
retaining the Apache license text and notices for those portions — the simpler
rule is: don't copy, design from the documented API shape only.

## Crate architecture

```
rxui              facade, prelude, examples
├── rxui-core     entities, Context, Render, Element builders, reconciler,
│                 dirty flush, theme/styling, ViewStats, built-in controls
│   └── rxui-tree retained tree: arena, dirty bits (TREE/LAYOUT/COMPOSE/PAINT/
│                 ACCESSIBILITY/HIT_TEST), layout, compose cache, hit testing,
│                 semantics, scroll, focus, paint fragments, shaping memos, PassStats
│       └── astrelis-core, astrelis-paint, astrelis-text, astrelis-platform (types)
├── rxui-widgets  chart, graph, docking, inspector (one feature each)
├── rxui-host     windowing, event pump, accessibility adapter
│   └── astrelis-app, -platform-winit, -gpu(-wgpu), -paint-gpu, -text-gpu, -compositor
└── rxui-test     label-addressed harness, SemanticScene, goldens
    └── astrelis-platform-test
```

**rxui-tree absorbs `astrelis-ui-next` module by module**
(source: `astrelis/crates/astrelis-ui-next/src/`):

| ui-next module | Destination | Notes |
|---|---|---|
| `tree.rs` (1,797) | rxui-tree | Arena, generational NodeId, passes, compose cache, hit testing, PassStats, Scene |
| `element.rs` (336) | rxui-tree | Element trait, Constraints, LayoutContext, UiInput; UiError removed |
| `mutation.rs` (904) | rxui-tree | Adopt the RFC's recommendation: element-specific `NodeMut` setters so an invalidation bit cannot be omitted |
| `builtins.rs` (1,231) | rxui-tree (layout) + rxui-core (controls) | |
| `controls.rs` (463), `text_field.rs` (559) | rxui-core | Text field keeps IME/caret/selection |
| `scroll.rs`, `semantics.rs`, `media.rs`, `shaping.rs` | rxui-tree | `ShapingMemo`/`KeyedShapingMemo` verbatim — recent, load-bearing perf work |

**Salvaged from v1** (reference via `git show archive/v1-ui-next-migration:<path>`):

- `crates/rxui-core/src/view/reconcile.rs` — `MountedChildren`: positional fast
  path, cleared-not-dropped scratch, published-order memo. Adapted to the
  non-generic `Element` description.
- `crates/rxui-core/src/component.rs` — `DirtySet` + depth-ordered `flush`,
  re-keyed by entity id; routing machinery evolves into `RoutedHandler`.
- `crates/rxui-core/src/diagnostics.rs` — `ViewStats` verbatim.
- `crates/rxui-core/src/views/retained.rs` — `RetainedSpec` re-expressed as a
  `CustomElement` trait (chart/graph/docking keep this imperative path).
- `crates/rxui-test-support/` — `click("Save")`/`activate("Save")`/`hover`
  label-addressing surface intact; `dispatch(action)` becomes
  `update(&entity, |state, cx| ...)`.
- `astrelis/crates/astrelis-ui-host/src/next.rs` (711 lines: NextWindowHost,
  NextAccessibilityAdapter, RetainedWork) merges with v1's `rxui-native`
  (343 lines) into rxui-host.

## Astrelis-side changes

- **Moves out**: `astrelis-ui-next` (→ rxui-tree; this also unblocks astrelis
  publishing, since it was the `publish = false` crate), plus `ui-host/next.rs`.
- **Deleted now**: `astrelis-ui-docking` (already deprecated).
- **Frozen at 0.3, removed in 0.4**: `astrelis-ui-core`, `astrelis-ui`,
  `astrelis-ui-widgets`, `astrelis-ui-testing`, remaining `astrelis-ui-host` —
  removed in one tagged commit once rxui v2 reaches Stage 5.
- **No astrelis-retained game UI layer.** Debug overlays draw directly with
  `astrelis-paint::Painter`; games can embed headless rxui-tree via the
  compositor once stable. If an immediate-mode layer is wanted later, it is a
  new small crate over astrelis-paint, not a ui-core revival.
- **Versioning**: keep the exact-rev git pin + `.cargo/config.toml` local patch.
  New discipline: astrelis tags `rxui-baseline/S<n>` at each pinned rev; pin
  bumps happen only at stage boundaries (prevents the drift that reached 26
  commits during v1). Pins become version requirements when astrelis 0.3
  publishes.

## Performance strategy

- The dirty-bit engine's measured profile (0.147 ms selection, 0.013 ms hit
  test) is the baseline; Stage 0 reproduces its benches before any new surface
  exists.
- Allocation discipline: reconciler stays allocation-free steady-state; Element
  descriptions for dirty entities are the only per-interaction allocations —
  pooled in a per-flush bump arena; SmallVec child lists; listeners boxed once
  per dirty-entity render.
- Layout: per-node `last_constraints` memo; custom layout boundaries for
  virtual lists/tables (O(visible) realized nodes).
- Text: ShapingMemo/KeyedShapingMemo port verbatim; every text element holds one.
- Damage: native fragment consumption in astrelis-paint-gpu (cache compiled
  geometry by fragment identity/revision, upload only changed fragments) —
  astrelis-side work alongside Stage 5.
- Regression gates from day one: PassStats + ViewStats exact-equality in tests;
  criterion benches with checked-in baselines; CI fails on >10% regression for
  the canonical workloads.

## Risks and open questions

1. **Entity reentrancy** (update-during-update, emit-during-flush,
   subscriptions into dropped entities) — the subtlest new code in Stage 3;
   deferred effects + weak handles, property-tested early.
2. **Engine drift**: re-pin astrelis at Stage 0, then stage-boundary bumps only.
3. **Accessibility**: NextAccessibilityAdapter has no platform implementation;
   AccessKit spike at Stage 6, adapter lives in rxui-host.
4. **Rich text / multiline editing**: out of scope; custom-layout boundary
   reserved. Priority decision after Stage 6.
5. **Key rule**: `.children(iter)` without keys = positional reconcile plus a
   debug warning when children are stateful; validate in Stage 3 against the
   todo and workbench examples.
6. **Web/wasm target**: deferred behind a tracking issue.
