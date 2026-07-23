# RXUI Next authoring API

Status: migration candidate

This document defines the public authoring model to use while porting RXUI.
The retained implementation may change without changing these semantics.

## Components

Application state is ordinary Rust owned by a component. Local actions mutate
that state; effects communicate with application coordination:

```rust
trait Component: 'static {
    type Action: 'static;
    type Effect: 'static;

    fn update(
        &mut self,
        action: Self::Action,
        cx: &mut ComponentContext<'_, Self::Effect>,
    );

    fn view(&self, theme: &Theme) -> View<Self::Action>;
}
```

`view` is a lightweight description, not mounted state. It may format values
and select child views, but must not retain element handles or perform
application effects.

## Static and dynamic children

Static children use tuples. This makes fixed structure visible and avoids
allocation syntax at call sites:

```rust
column((
    label("Document"),
    text_field("Name", name, Action::Rename),
    row((
        button("Cancel", Action::Cancel),
        button("Save", Action::Save),
    )),
))
```

Dynamic children use the explicit `views` adapter and stable keys:

```rust
column(views(items.iter().map(|item| {
    item_row(item).key(item.id)
})))
```

Every sibling in a dynamic keyed collection must have a key. Duplicate or
partially keyed collections are errors. A matching key and view kind preserve
mounted state when reordered.

## Controlled values

Fields display component-owned values and emit replacement values:

```rust
text_field("Name", self.name.clone(), Action::Rename)
```

The control retains transient interaction state such as focus, selection,
caret affinity, and IME composition. The component owns durable document
state. Reconciliation updates the retained control without recreating it.

## Composition and action scopes

Reusable view functions keep a local action vocabulary:

```rust
fn inspector(model: &InspectorModel) -> View<InspectorAction> {
    editable_property_grid(&model.fields, InspectorAction::Changed)
}

inspector(&self.inspector)
    .map_action(EditorAction::Inspector)
    .key("inspector")
```

`map_action` changes only action routing. It does not introduce a new
state-owning component instance.

The production nested-component primitive will add independent state,
lifecycle, effects, tasks, and focus restoration. Its intended surface is:

```rust,ignore
component(
    Inspector::new,
    InspectorProps {
        selection: self.selection,
    },
)
.key("inspector")
.map_effect(EditorAction::Inspector)
```

This primitive is intentionally not emulated with hidden global state during
the initial migration.

## Effects

Effects leave a component after its reducer has completed:

```rust
Action::Save => cx.emit(Effect::SaveDocument(self.document.clone()))
```

Hosts drain effects and coordinate documents, commands, services, windows,
and persistence. Effects are not routed back through the local action channel
implicitly.

## Styling

Views use semantic typed roles. Themes resolve roles into concrete retained
properties:

```rust
panel(size, ColorRole::Surface, semantics)
```

There are no selectors, string classes, specificity rules, or implicit CSS
cascade. Widget-specific builders will expose typed variants and metrics.

## Retained escape hatch

Most application and editor code returns views. Specialized surfaces such as
node graphs, timelines, code editors, virtual collections, and render
viewports may implement retained `Element` behavior directly.

Application code must not choose invalidation flags. Before the retained core
becomes public, raw `update(handle, flags, closure)` calls will be confined to
framework internals and replaced by property-aware setters.

## Migration rules

- Port leaf controls and semantic tokens before composite editor widgets.
- Preserve existing behavior and accessibility while replacing authoring
  code; do not combine visual redesign with migration.
- Use controlled values for document state and retained state for transient
  interaction.
- Key collections by domain identity, never by visible position.
- Use local actions inside reusable surfaces and effects at coordination
  boundaries.
- Keep imperative adapters for workloads that do not benefit from view
  reconciliation.

