# RXUI v2: Implementation Plan

Companion to [v2-rewrite.md](v2-rewrite.md) (the design). This document splits
the rewrite into stages, each independently verifiable and each ending in a
green, committable state. Relative effort is a share of the total.

Salvage sources:

- v1 rxui: `git show archive/v1-ui-next-migration:<path>` in this repo.
- Engine: `astrelis/crates/astrelis-ui-next/src/` (copied in at Stage 0, then
  deleted from astrelis).

Standing gates for **every** stage:

- `cargo test --workspace --all-features` green; clippy clean (`-D warnings`);
  `cargo fmt --check` clean.
- No `todo!()` / `unimplemented!()` outside `#[ignore]`d tests with reasons.
- PassStats / ViewStats assertions are exact equalities, not `>=`.

---

## Stage 0 — `rxui-tree` headless (15%)

The retained engine, absorbed and re-owned.

- Workspace skeleton: `crates/rxui-tree`, workspace `Cargo.toml`, CI (fmt,
  clippy, test, boundary checks), astrelis pinned at current HEAD
  (`rxui-baseline/S0` tag on the astrelis side).
- Copy `astrelis-ui-next/src/{tree,element,mutation,scroll,semantics,media,shaping}.rs`
  and the layout half of `builtins.rs` into `rxui-tree`; rename crate-level
  types (drop the `Next`/`Ui` prefixes where they exist only for coexistence).
- Strip `UiError` from tree invariants → `debug_assert!`/panic per the design's
  error policy.
- Replace `update(handle, flags, closure)` mutation with element-specific
  `NodeMut` setters (the ui-next RFC's own production recommendation) so an
  invalidation bit cannot be omitted.
- Port ui-next's test suites (`invalidation`, `mutation_bits`, `layout_passes`,
  `accessibility_passes`, `compose_equivalence`, `runtime`, `text_memo`) and
  the criterion bench (`benches/incremental.rs`).

**Done when**: ported tests green; a hand-built (no view layer) flex tree lays
out, hit-tests, and produces semantic snapshots; benches reproduce the RFC's
1,000-node baselines within noise.

## Stage 1 — Paint and text (10%)

- Paint fragments, `Scene` emission, `ShapingMemo`/`KeyedShapingMemo`,
  `Label`/`Frame` painting.
- Port `assert_text_golden` and the text-scene golden tests from
  `rxui-test-support/src/golden.rs`.

**Done when**: scene goldens green; "change one label" asserts
`PassStats.rebuilt_fragments == 1`.

## Stage 2 — Input, focus, semantics routing (10%)

- `UiInput` dispatch, capture, hover tracking, focus traversal and scopes,
  semantic actions, clipboard operations.
- Port `rxui-test` harness (`harness.rs`, `scene.rs`) against a hand-built
  tree: `click(label)`, `hover(label)`, `activate(label)`, key events.

**Done when**: `harness.click("Save")` and `harness.activate("Save")` work
headlessly; hit-test subtree rejection covered by a bench.

## Stage 3 — Entity surface and reconciler (25%, the new code)

The centerpiece; everything before it is a port.

- Entity slotmap (`App`, `Entity<T>`, `WeakEntity<T>`), `Context<T>`,
  `Render` trait, `cx.listener` / `cx.notify` / `cx.emit` / `cx.subscribe`,
  deferred-effects queue (reentrancy: update-during-update, emit-during-flush,
  subscription into dropped entity — property-test these first).
- `Element` description type + fluent builders (`column()`, `row()`, `label()`,
  `button()`, `.gap()`, `.child()`, `.children()`, `.key()`).
- Port the keyed reconciler (`MountedChildren` from v1 `view/reconcile.rs`)
  against the non-generic `Element`; port `DirtySet` + depth-ordered flush from
  v1 `component.rs`, keyed by entity id; port `ViewStats` verbatim.
- Theme + revision-based invalidation.
- Stateless view functions (`fn(...) -> Element`).
- Rewrite `docs/update-isolation.md` for the entity model.

**Done when**: counter and todo examples run headlessly; unchanged re-render
produces zero retained mutations (the ui-next adoption-gate invariant); keyed
reverse preserves retained identity; exact ViewStats assertions pass.

## Stage 4 — Controls (20%)

- Button, checkbox, slider, text field (IME/caret/selection ported from
  ui-next `text_field.rs`), scroll view, split pane, list.
- Each control ships with keyboard behavior, focus, semantics, and
  deterministic tests — port the invariants of v1's `controls.rs`,
  `validation.rs`, `accessibility.rs` suites and the forms model.

**Done when**: ported control suites green; a settings-form example runs
headlessly.

## Stage 5 — Native host (10%)

- `rxui-host`: merge `astrelis-ui-host/src/next.rs` (window host,
  accessibility adapter scaffolding, retained-work scheduling) with v1's
  `rxui-native` (winit pump, wgpu surface, compositor views, cursor,
  clipboard).
- Native counter / settings / workbench examples.
- Astrelis-side: delete `astrelis-ui-next`, `astrelis-ui-docking`, and
  `ui-host/next.rs` (tag `archive/pre-ui-extraction` first); begin native
  fragment consumption in astrelis-paint-gpu.

**Done when**: native workbench runs on macOS; the headless workbench suite is
green against the same model; idle windows schedule zero passes.

## Stage 6 — Widgets, accessibility, hardening (10%)

- Chart / graph / docking via the `CustomElement` escape hatch (port v1
  `rxui-widgets/src/element/*` and `compose/docking.rs`), one feature each.
- AccessKit adapter spike behind an `rxui-host` feature.
- Conformance goldens; CI bench-regression gate (>10% fails) on the canonical
  workloads: selection, table resize, controlled edit, warm scene.

**Done when**: the editor-vertical-slice example matches or beats the RFC's
measured update times; VoiceOver reads the counter (spike-level).

---

## Test port strategy

v1's ~5.5k lines of integration tests are ported per stage, not up front.
Harness-driven test bodies survive with mechanical edits
(`Harness::new(Counter{..})` → `Harness::new(|cx| cx.new(|_| Counter{..}))`;
`dispatch(Action::X)` → `update(|c, cx| ...)`; `click`/`activate`/goldens
unchanged). Tests asserting Elm-specific mechanics (`update_model_gate.rs`)
are rewritten preserving their *invariants* — update isolation and exact
ViewStats equality — rather than their text.

| v1 test file | Ported at |
|---|---|
| ui-next engine suites | Stage 0 |
| `layout_contracts.rs` | Stage 0 |
| text goldens | Stage 1 |
| harness self-tests | Stage 2 |
| `reconciliation.rs`, `update_model_gate.rs` (rewritten) | Stage 3 |
| `controls.rs`, `validation.rs`, `accessibility.rs` | Stage 4 |
| `workbench.rs`, `surfaces.rs` | Stage 5 |
| `retained_specs.rs`, `conformance.rs` goldens | Stage 6 |
