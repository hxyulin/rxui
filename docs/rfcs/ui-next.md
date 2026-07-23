# RXUI Next: typed components over a lean retained core

Status: experimental implementation  
Prototype: `crates/rxui-next`  
Engine RFC: `../astrelis/rfcs/ui-next.md`

## Decision

RXUI owns the primary authoring model. Application and editor code produces
lightweight views from ordinary Rust state; RXUI reconciles those values into
Astrelis retained nodes. Imperative retained access remains an escape hatch
for custom render views, large text editors, and specialized elements.

The selected update model is component-scoped typed actions, not a whole-window
virtual DOM, mutation closures, or a signal graph.

## Component contract

A component defines local `Action` and parent-facing `Effect` types:

```rust
trait Component {
    type Action: 'static;
    type Effect: 'static;

    fn update(
        &mut self,
        action: Self::Action,
        cx: &mut ComponentContext<'_, Self::Effect>,
    );

    fn view(&self, theme: &Theme) -> AnyView<Self::Action>;
}
```

The migration candidate names the erased implementation simply
`View<Action>`. Static children use tuples, dynamic children use
`views(iterator)`, and `.key(domain_id)` declares retained identity. See
`crates/rxui-next/API.md` for the complete authoring contract.

The runtime owns the component and mounted view state. A local action runs its
reducer and reconciles that component. Effects leave the local action channel
for document coordination, commands, services, and other windows. Internally
erased event payloads are downcast at the component boundary.

The prototype implements root component instances and nested typed action
scopes. `AnyView<LocalAction>::map_action` lets a reusable editor subtree keep
its local action vocabulary while the mounted control emits the parent action
directly. Production still needs nested state-owning instances with stable
`ComponentId`s: actions target their owner, props invoke `changed`, effects map
into the parent, and component-owned tasks/timers cancel on unmount.

## View reconciliation

`AnyView<Action>` is a short-lived configuration. Mounted state owns retained
handles and previous resolved properties.

- Static sequences reconcile positionally.
- Dynamic sequences must key every child.
- Duplicate and partially keyed sequences fail deterministically.
- Matching key and kind reuse retained state.
- Keyed moves reorder without removing nodes.
- Changed keys or kinds replace only that subtree.
- Equal resolved properties perform no retained mutation.

The prototype uses internal trait-object erasure. Production may add
monomorphic tuple/sequence implementations without changing semantics.
Explicit component or memo boundaries are preferred over automatic signal
dependency tracking.

## Styling

Themes expose semantic typed color, spacing, typography, metric, motion, and
control-variant tokens. Views compare resolved values during reconciliation,
so theme changes invalidate only changed properties. There are no selectors,
string classes, specificity rules, or CSS cascade. Astrelis receives concrete
layout and paint values.

## Editor vertical slice

The implementation includes a keyed editable property grid, visible-range tree
and width-controlled table backed by 10,000 model rows, a semantic render-view
slot, and separate selection, table-resize, and property-edit workloads.
Labels and fields use real shaped text. The field path includes pointer caret
placement, selection, keyboard editing, IME, focus, controlled reconciliation,
and mapped local-to-parent actions. Ordinary view code retains no
`ElementHandle`s and calls no `sync`.

Before adoption, the slice must add native nested component hosting, real
compositor-view painting, complete routed keyboard navigation and focus
restoration, semantic actions, clipboard/undo policy, and
GPU-upload/allocation instrumentation.

Release measurements on the development machine:

| RXUI Next workload | Average | Visible affected work |
| --- | ---: | --- |
| Selection + property value | 0.147 ms | 1 shape, 3 fragments |
| 30-row table column resize | 0.256 ms | 30 shapes, 61 fragments |
| Controlled property edit | 0.151 ms | 1 shape, 1 fragment |

Current RXUI's existing reference-editor benchmark measured 1.218 ms for
selection and 2.402 ms for table resize. These numbers are not yet
apples-to-apples because the current case includes docking and realizes a
different surface. They validate affected-slice scaling, not replacement.

## Compatibility and rollout

`rxui-next` is unpublished and does not change RXUI 0.1. If performance and
behavior gates pass, publish a migration RFC, stabilize Astrelis interfaces,
port controls by workload with imperative adapters, migrate examples/testing,
and remove old authoring APIs only in an announced breaking release.

Mobile, game-first HUD policy, selector styling, pervasive signals, macros,
partial surface damage, and a complete widget-catalog port are out of scope.
