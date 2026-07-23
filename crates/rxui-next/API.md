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

Components which need independent state implement `ComponentWithProps` and
mount through `component`:

```rust
component::<Inspector, EditorAction>(
    InspectorProps {
        selection: self.selection,
    },
    EditorAction::Inspector,
)
.key("inspector")
```

The instance retains its reducer state and subtree identity. Changed props call
`changed` without recreating local state. Child actions reduce locally; typed
effects are mapped into parent actions. Component-owned tasks belong to the
later async-runtime migration stage.

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
column_with(
    ContainerStyle::new()
        .gap(Space::Md)
        .padding(Space::Lg)
        .background(ColorRole::Surface),
    (
        label("Preferences"),
        button_with(
            "Apply",
            Action::Apply,
            ButtonStyle::standard().variant(ButtonVariant::Primary),
        ),
    ),
)
```

There are no selectors, string classes, specificity rules, or implicit CSS
cascade. Compact defaults retain the short `column`, `row`, and `button`
forms. Typed option values extend controls without long positional argument
lists.

## Native hosting

`ComponentWindow<C>` connects the same component runtime to an Astrelis
window, normalized input, incremental retained passes, and GPU presentation.
Headless tests use `ComponentHost<C>`; native applications use
`ComponentWindow<C>` and keep service/effect coordination in their `App`.

Tab and Shift-Tab follow retained tree order. Pointer focus, Enter/Space
activation, IME text input, and semantic Focus/Activate/SetValue/SetText
operations all enter through the same normalized control path.

## Retained escape hatch

Most application and editor code returns views. Specialized surfaces such as
node graphs, timelines, code editors, virtual collections, and render
viewports may implement retained `Element` behavior directly.

Application code does not choose invalidation flags. Reconciliation uses
property-aware setters; raw `update(handle, flags, closure)` remains only as a
low-level framework escape hatch.

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
