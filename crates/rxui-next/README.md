# rxui-next

Migration implementation of RXUI's typed component and reconciled-view
authoring model.

It includes root and nested state-owning components, local typed actions,
application effects, keyed views, typed theme and container options, shaped
labels, editable fields, buttons, checkboxes and sliders, property-grid
reconciliation, width-controlled visible-range tree/table views, a render-view
slot, and a native incremental window host.

```text
cargo run -p rxui-next --example editor_vertical_slice --offline
cargo run -p rxui-next --example native_counter --offline
cargo run -p rxui-next --example native_settings --offline
```

See [`docs/rfcs/ui-next.md`](../../docs/rfcs/ui-next.md).
The migration-facing contract is documented in [`API.md`](API.md).
