# RXUI Next migration

The `ui-next-migration` branch exposes the new implementation as
`rxui::next`. Existing retained APIs remain available during the compatibility
window; new application surfaces should use the component API.

## Completed foundation

- Incremental Astrelis arena, cached paint fragments, property-aware mutation,
  hit testing, and semantic deltas.
- Production native window/GPU host with pointer, keyboard, focus, and IME
  normalization.
- Root and nested state-owning components, typed local actions and effects,
  controlled values, keyed dynamic collections, and mapped action scopes.
- Labels, panels, rows, columns, overlay stacks, spacers, buttons, text
  fields, checkboxes, sliders, typed theme roles, variants, spacing, padding,
  backgrounds, and disabled subtrees.
- Tree-order keyboard traversal and semantic Focus, Activate, SetText,
  SetSelection, and SetValue operations.
- Headless reconciliation examples plus native counter and nested settings
  examples.

## Catalog migration order

| Surface | Migration strategy | Status |
| --- | --- | --- |
| Form controls and layout | Native Next views | In progress |
| Property grid, tree, table | Keyed/virtual Next views | Implemented vertical slice |
| Toolbar, dialogs, toasts | Components over stack and semantic actions | Next |
| Combo/radio/numeric fields | Controlled composite components | Next |
| Command palette | State-owning component plus virtual results | Next |
| Images, icons, charts | Custom retained elements behind `View` adapters | Pending |
| Docking and node graph | Imperative retained escape hatch with component shell | Pending |
| Devtools/testing | Semantic snapshots and component-host drivers | Pending |
| Clipboard, undo, async tasks | Host/component services | Pending |
| Platform accessibility bridge | Semantic delta/action adapter | Pending |

Legacy APIs are removed only after their application examples and behavior
tests have migrated. Specialized retained elements are intentionally preserved;
the migration replaces authoring and state plumbing, not every retained
workload with a virtual tree.
