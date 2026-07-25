# The RXUI view protocol

RXUI's view set is open. The twenty view kinds `rxui-core` ships - containers,
controls, modifiers, the component boundary - are written against exactly the
surface this document describes, with no privileged access to anything a
third-party crate cannot reach. If a view kind you need is missing, write it;
if writing it requires reaching around this surface, that is a gap in the
protocol and worth reporting.

This is the extension guide. For the authoring model - components, actions,
effects, controlled values - see [component-api.md](component-api.md).

## The two trees

A `View<Action>` is a *description*. Building one allocates a small tree of
`ViewNode` values and nothing else: no retained nodes, no handles, no layout.
Mounting walks that description once and produces a parallel tree of `Mounted`
nodes, each owning a retained identity and the state needed to maintain it.
Every pass after the first reconciles a fresh description against the mounted
tree.

The pairing rule is one line: a fresh view reconciles against a mounted node
when, and only when, their `ViewKind` agrees. `ViewKind` is the implementing
Rust type, so your node reconciles against its own previous instance and never
against a builtin or another crate's node. On a mismatch the mounted subtree is
removed and the new view is mounted from scratch.

## Writing a view kind

Three pieces:

1. A **view value** - the description. Owns whatever the element needs; it is
   consumed on every pass, so it can move `String`s and `Vec`s rather than
   cloning them.
2. A **mounted state** implementing `MountedState<Action>` - the retained
   handle, the values last written to the element, and any mounted children.
3. An `impl ViewNode<Action>` with `build` and `rebuild`.

```rust
impl<Action: 'static> ViewNode<Action> for MyView<Action> {
    fn build(
        self: Box<Self>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<Mounted<Action>, UiError> {
        let handle = context.append(MyElement::new(self.value))?;
        Ok(Mounted::new(handle.id(), MyState { handle, value: /* ... */ }))
    }

    fn rebuild(
        self: Box<Self>,
        mounted: &mut Mounted<Action>,
        context: &mut ViewContext<'_, Action>,
    ) -> Result<(), UiError> {
        let state = mounted.state_mut::<MyState>()?;
        if state.value == self.value {
            return Ok(());
        }
        context
            .ui()
            .update(state.handle, Invalidation::PAINT, |element| {
                element.value = self.value;
            })?;
        state.value = self.value;
        Ok(())
    }
}
```

Expose it with `AnyView::new(MyView { .. })`, which is what every builtin
constructor does.

`self: Box<Self>` is deliberate. It is object-safe, and it is what lets `build`
move owned data straight into the retained element instead of cloning it out of
a borrow.

### The rules

- **Append exactly one node, or reuse your child's.** A view contributes one
  retained identity to its parent's child list. A container appends its own node
  and mounts children below it; a transparent wrapper - `enabled`, `visible`,
  `map_action` - appends nothing and returns its child's node.
- **`rebuild` must not remove its own node.** Replacement is the framework's
  decision, made in `ViewContext::rebuild_child`, and it is what preserves
  retained state across a reorder.
- **Compare before writing, and ask for exact bits.** `context.ui().update`
  takes the `Invalidation` you name. Returning early when nothing changed is
  what keeps an untouched subtree free; `Invalidation::ALL` on a color change is
  what makes a theme switch reshape ten thousand labels.
- **`state_mut` cannot fail in practice.** The framework calls `rebuild` only
  when the kinds agree, and the kind is your own type. It returns a `Result`
  rather than panicking because a view protocol should not be able to abort the
  host process.

## Children

A container owns a `MountedChildren<Action>`. That is the whole of keyed
reconciliation:

```rust
// build
let mut children = MountedChildren::new();
children.build(self.children, &mut context.child(handle.id()))?;

// rebuild
state.children.reconcile(self.children, &mut context.child(parent))?;

// MountedState
fn visit_children(
    &mut self,
    visit: &mut dyn FnMut(&mut Mounted<Action>) -> ControlFlow<()>,
) {
    self.children.visit(visit);
}
```

`context.child(node)` re-scopes the context so children attach below `node`.
`reconcile` preserves retained identity across reorders, mounts and unmounts
what appeared and disappeared, and publishes the child order to the engine only
when it actually moved.

`visit_children` is not optional bookkeeping. It is how the framework reaches
components nested anywhere below your node: both `MountedState::route` (an
action unwinding towards the component that owns it) and
`MountedState::rebuild_component` (the depth-ordered dirty drain) are defined in
terms of it. A container that does not report its children silently swallows
its descendants' actions and leaves them stale, and no test of your own node
will notice.

### Keying

Within one child list, either every child is keyed or none is; a partially keyed
or duplicate-keyed sequence is an error from `reconcile`. Key by domain
identity, never by visible position - see the authoring rules in
[component-api.md](component-api.md).

## Actions

Retained elements emit through an `ActionEmitter<Action>`, which erases a typed
action so the runtime can route it back to the component that owns it. Get one
from `context.emitter()` and **store it**: its identity is stable for the
lifetime of a mounted node, which is what lets an element install its callback
once at mount instead of on every pass. Re-deriving it per pass allocates and
destroys that stability.

A node that gives its subtree a different action vocabulary derives a child
emitter with `ActionEmitter::map`, stores it, and passes it to
`context.scoped(parent, &emitter)` on every pass. Because its children are
`Mounted<Child>` rather than `Mounted<Action>`, it cannot report them through
`visit_children`; it must override `route` - mapping the child's returned
actions out - and `rebuild_component` - forwarding the descent - instead. The
builtin `map_action` and the component boundary are both exactly this shape.

## Retained elements without a view node

Most specialized surfaces - charts, node graphs, render viewports - do not need
a `ViewNode` at all. `RetainedSpec` is the shorter path:

```rust
impl<Action: 'static> RetainedSpec<Action> for MySpec {
    type Element = MyElement;

    fn create(&self, emitter: &ActionEmitter<Action>, theme: &Theme) -> MyElement { .. }
    fn update(&self, element: &mut MyElement, emitter: &ActionEmitter<Action>, theme: &Theme) { .. }
    fn changed(&self, previous: &Self) -> Invalidation { .. }
    fn children(&self) -> Vec<AnyView<Action>> { Vec::new() }
}
```

Mount it with `retained(spec)`.

A pass over a spec does two things, and they never overlap. First the element is
written, *inside* the engine's `update`, which hands out `&mut Element` and
nothing else - that is why `create` and `update` take an emitter and a theme
rather than a context. Then children are reconciled, with the whole tree
available. `children` defaults to none, so a leaf spec pays nothing; a container
element returns its children there and gets full keyed reconciliation, nested
components included.

`changed` names the passes the change requires. An empty `Invalidation` means
the element is visually and semantically identical to the previous one and no
pass has to run over it. Reporting more than changed is merely slow; reporting
less leaves a stale frame.

## Instrumentation

`rxui_core::diagnostics::ViewStats` counts framework work per interaction, and
the counters are asserted with exact equality in
`crates/rxui/tests/update_model_gate.rs`. Your node is counted automatically:
`build_child` records `nodes_built`, `rebuild_child` records `nodes_rebuilt`,
and `MountedChildren::reconcile` records `containers_reconciled`.

One case needs your help. A node whose reconciliation may legitimately do
*nothing at all* - a component boundary whose inputs all agree with the previous
pass, a memo whose dependencies compare equal - must return `true` from
`ViewNode::records_own_rebuild` and call `ViewStats::record_node_rebuilt()`
itself on the paths that really reconcile. Otherwise `nodes_rebuilt` grows with
the number of *untouched* nodes, which is exactly the dependence that skipping
exists to remove.
