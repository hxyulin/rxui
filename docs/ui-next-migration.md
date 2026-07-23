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
| Clipboard, undo, async tasks | Host/component services | Pending |
| Platform accessibility bridge | Semantic delta/action adapter | Pending |

Legacy APIs are removed only after their application examples and behavior
tests have migrated. Specialized retained elements are intentionally preserved;
the migration replaces authoring and state plumbing, not every retained
workload with a virtual tree.

## Clean replacement after merge

The merge can remain non-breaking while applications migrate through
`rxui::next`. Once every shipped example and downstream application is on the
new API, perform the public cutover as one mechanical commit:

1. Re-export `rxui_next::*` from `rxui` and switch `rxui::prelude` to the new
   component/view types.
2. Move the current legacy re-exports under `rxui::legacy` for one deprecation
   window.
3. Run the workspace examples and compatibility compile tests.
4. Remove `rxui::legacy` and the old implementation crates in a separate
   deletion-only commit.

Keeping the facade change separate from the deletion makes the replacement
easy to review, bisect, and revert. No application source rewrite should be
mixed with the final removal commit.
