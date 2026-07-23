# RXUI Next migration

The `ui-next-migration` branch exposes the new implementation as
`rxui::next`. Existing retained APIs remain available during the compatibility
window; new application surfaces should use the component API.

## Completed foundation

- Incremental Astrelis arena, cached paint fragments, property-aware mutation,
  hit testing, and semantic deltas.
- Production native window/GPU host with pointer, keyboard, focus, and IME
  normalization.
- Bounded flex allocation, intrinsic controls, centered alignment, dedicated
  split panes, pointer capture, retained hover transitions, and native cursor
  feedback.
- Root and nested state-owning components, typed local actions and effects,
  controlled values, keyed dynamic collections, and mapped action scopes.
- Labels, panels, rows, columns, overlay stacks, spacers, buttons, text
  fields, checkboxes, sliders, typed theme roles, variants, spacing, padding,
  backgrounds, disabled subtrees, validated vector icons, and controlled form
  validation.
- Tree-order keyboard traversal, keyboard bubbling, modal focus scopes with
  restoration, Escape dismissal, and semantic Focus, Activate, SetText,
  SetSelection, and SetValue operations.
- Native chart and graph hover, image/render adapters, docking workspace,
  semantic inspection, and a complete native workbench validation example.

## Catalog migration order

| Surface | Migration strategy | Status |
| --- | --- | --- |
| Form controls and layout | Native Next views | Foundation complete |
| Property grid, tree, table | Keyed/virtual Next views | Implemented vertical slice |
| Toolbar, dialogs, toasts | Components over stack and semantic actions | Implemented |
| Combo/radio/numeric fields | Controlled composite components | Implemented vertical slice |
| Command palette | Controlled query/selection with bubbled keyboard ownership | Implemented |
| Images and charts | Custom retained elements behind `View` adapters | Implemented |
| Icons and validation | Native Next views and form presentation | Implemented vertical slice |
| Docking and node graph | Imperative retained escape hatch with component shell | Implemented vertical slice |
| Devtools/testing | Semantic snapshots and component-host drivers | Implemented vertical slice |
| Clipboard, undo, async tasks | Host/component services | Implemented vertical slice |
| Platform accessibility bridge | Semantic delta/action adapter | Implemented and covered |

Legacy APIs are removed only after their application examples and behavior
tests have migrated. Specialized retained elements are intentionally preserved;
the migration replaces authoring and state plumbing, not every retained
workload with a virtual tree.

Component reducers request clipboard reads/writes and typed background work
through `ComponentContext`. Native coordinators drain
`ComponentServiceRequest` values, execute platform work, and return completion
actions through `ComponentWindow::complete_service`. `ComponentHost` supplies a
deterministic synchronous executor and `MemoryClipboard` for tests. Undo remains
controlled application state through bounded `UndoHistory<T>` snapshots.

## Clean replacement after merge

The merge can remain non-breaking while applications migrate through
`rxui::next`. The `next-default` feature already switches `rxui::*` and
`rxui::prelude::*` to the component API, while `rxui::legacy` preserves explicit
access to the retained application and widget crates. CI can validate both
facades before changing defaults:

```sh
cargo test -p rxui --test next_facade
cargo test -p rxui --no-default-features --features next-default --test next_facade
cargo run -p rxui --no-default-features --features next-default --example next_counter
```

Once every shipped example and downstream application is on the new API,
perform the public cutover as one mechanical commit:

1. Add `next-default` to the facade's default feature set.
2. Run the workspace examples and both compatibility modes above.
3. Keep `rxui::legacy` for one deprecation window.
4. Remove `rxui::legacy` and the old implementation crates in a separate
   deletion-only commit.

Keeping the facade change separate from the deletion makes the replacement
easy to review, bisect, and revert. No application source rewrite should be
mixed with the final removal commit.
