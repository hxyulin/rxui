# Changelog

All notable public RXUI changes are documented here.

## 0.1.0-rc.1 - Unreleased

### The view set is open

- New public `ViewNode<Action>` trait, replacing the private `DynView`. All
  twenty builtin view kinds are implemented against it with no privileged
  access, which is the proof that a third-party crate can write a container that
  behaves exactly like `column`. Supporting surface: `ViewKind`, `AnyView::new`,
  `AnyView::kind`, `Mounted`, `MountedState`, `MountedChildren`, `ViewContext`,
  `RouteContext`, `RebuildContext`, `RoutedComponentAction`, and
  `ActionEmitter::{map, ptr_eq}`. See the new `docs/view-protocol.md`.
- `Mounted` keeps its fields private and hands out state through
  `state_mut::<S>() -> Result<&mut S, UiError>`, which replaces twenty
  `.expect("view kind and state agree")` panics with an error a host can report.
- The kind-mismatch decision lives once, in `ViewContext::rebuild_child`, and
  the descent used for action routing and the dirty drain is derived from a
  single `MountedState::visit_children`.

### Update isolation

- Reducing an action no longer rebuilds the tree. It updates state, maps
  effects, and records the reducing component as stale; the runtime then builds
  the root's view only if the root's own state changed and rebuilds the
  remaining stale components in depth order. One action into one of N sibling
  components costs the same work at any N.
- A nested `component` boundary is rebuilt only when its props compare unequal,
  its `Theme::revision` moved, it reduced one of its own actions, or an
  enclosing scope forced the pass. **`Props: PartialEq` is now load-bearing**,
  and a `view` that reads state the framework cannot see needs the new
  `ComponentContext::request_render`. See `docs/update-isolation.md`.
- New `ComponentRuntime::mark_dirty` and `flush`, plus `dispatch_all` and
  `dispatch_all_erased`, which reduce a batch of actions and reconcile once.
  `ComponentWindow::handle_event` and `ComponentHost::run_pending_services` use
  the batched path.
- `Theme::revision` is now read by reconciliation and must be bumped whenever
  theme tokens change.
- Reconciliation asks the engine for exact invalidation bits instead of a
  blanket layout pass, and a container whose child order did not move no longer
  republishes it.

### Breaking

- `RetainedSpec::changed` returns `Invalidation` instead of `bool`, so a
  specialized element can name the passes its change requires instead of getting
  `Invalidation::ALL`. Return `Invalidation::ALL` where the old implementation
  returned `true` for identical behavior.
- `RetainedSpec::{create, update}` take `&ActionEmitter<Action>` and a `&Theme`.
  An emitter is cloned rather than moved, and a spec can now resolve semantic
  colors.
- New `RetainedSpec::children`, defaulting to none. A specialized element can
  host child views and gets full keyed reconciliation, nested components
  included, without implementing `ViewNode`.
- `ViewKey` is an enum over `u64`, `&'static str`, and `Arc<str>` instead of a
  single `Arc<str>`. `From<&str>` narrowed to `From<&'static str>`; key a
  non-static borrowed string with `ViewKey::new`. Numeric and textual keys are
  now distinct spaces, so `ViewKey::from(7u64) != ViewKey::from("7")`.
- `label`, `label_with_width`, and `label_with_style` take
  `impl Into<Arc<str>>`. `dropdown_field`, `form_section`, and `dialog` take
  `impl Into<Arc<str>>` for the text they forward to a label.

### Component API cutover

- `rxui::*` and `rxui::prelude::*` now expose the typed component/view API.
- Components own durable state, reduce typed actions, emit typed effects, and
  return lightweight reconciled views.
- Fixed child structure uses tuples; dynamic collections require stable keys.
- Controls are controlled by component state while retaining transient focus,
  selection, caret, IME, hover, and pointer-capture state.
- Typed theme roles and style builders replace the old mutable widget facade.
- Headless and native component hosts share the same reconciliation and input
  paths.
- Clipboard, background work, and undo history use host-executed component
  service requests.
- Charts, node graphs, docking workspaces, images, render views, forms,
  validation, dialogs, toasts, toolbars, and inspection have component-native
  APIs.
- Synthetic interaction traces and reviewed semantic-geometry goldens cover
  layout, hover, focus, text input, splitters, keyed reconciliation, overlays,
  and specialized retained surfaces.
- The former `rxui-next` monolith is split into acyclic `rxui-core`,
  `rxui-controls`, `rxui-widgets`, `rxui-charts`, and `rxui-native`
  implementation crates. Application code continues to depend on the
  aggregate `rxui` facade.

### Fixed

- Icons compare by content. `Icon` gained a hand-written `PartialEq` over its
  view box, winding rule, and path verbs, because `Path`'s only identity is
  `Path::cache_id`, a per-allocation counter the renderer uses to key its mesh
  cache. Both `IconSpec::changed` and `ButtonView::rebuild` were keyed on that
  counter, and every `icons::*` constructor allocates a fresh path from inside
  `view()`, so **every icon and every `icon_button` invalidated itself on every
  pass** and dragged its container and the root through layout with it. One icon
  command in a toolbar cost three layouts and a rebuilt fragment per frame; it
  now costs nothing.
- Controlled leaf controls revert a refused change. `text_field`, `checkbox`, and
  `slider` all compared the incoming value against the one the *view* last
  declared rather than against the element. All three write their own state
  before emitting, so a component that reduces the change and keeps its old
  value left the two disagreeing while the declaration stood still: the guard saw
  no change, wrote nothing, and the rejected edit survived every later rebuild.
  That made `numeric_field`'s `Result<Number, String>` callback unusable as
  documented, since refusing an edit silently meant accepting it visually. All
  three now compare against the element's live state, which costs one tree lookup
  and leaves the accepted path writing nothing.
- `.visible(false)` and `.enabled(false)` survive their child being replaced.
  Both wrappers wrote to the engine only when the value they declared differed
  from the previous pass, which is right for a reconcile and wrong for a
  replacement: a changed child kind builds a fresh node, a fresh node is visible
  and enabled, and a wrapper whose own declaration had not changed wrote nothing
  to it.
- The node graph shapes each title once per change to that title, not once per
  layout pass. Combined with the narrowed `changed()` below, a pan, a zoom, a
  node drag, and a selection highlight are repaints.
- `NodeGraphSpec`, `ChartSpec`, `ImageSpec`, `RenderViewSpec`, and `IconSpec`
  name the passes each field feeds instead of asking for `Invalidation::ALL`. In
  every one of the five, the element's `layout` reads only its declared size, so
  content changes are paint and accessibility. A node-graph selection in the
  workbench example is now zero layouts, zero shapes, and one rebuilt fragment.

### Removed

- The transitional `rxui::next`, `rxui::legacy`, and `next-default` surfaces.
- The old message-application, mutable widget, editor facade, devtools facade,
  native-menu, and desktop-service packages.
- Examples and tutorials authored against those removed APIs.
- The `rxui-testing` crate. Its differential oracle against the old retained
  engine is replaced by exact component-side layout assertions, and its
  headless harness moved down into the unpublished `rxui-test-support`.

The old retained UI is no longer referenced by any RXUI crate. It still reaches
the dependency graph through `astrelis-ui-host`, which is an engine concern.

### Known gaps

- Native screen-reader adapters are not implemented yet.
- Multiline and rich/code text editing remain future work.
- Native menus, file dialogs, filesystem watching, persistence, and the former
  message/subscription runtime need component-native replacements before they
  return to the public framework.
