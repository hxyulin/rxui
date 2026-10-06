# RXUI next: architecture and application API

Status: agreed architectural direction; the headless state milestone is implemented
on the new orphan `main`. The application/element examples below remain API
sketches, not compile-checked interfaces. They do not describe the previous RXUI
release or preserved prototype. No compatibility layer is required for the
existing retained, message-driven API.

## Implemented state contracts

The initial workspace contains one `rxui` crate. `Runtime`, `Entity<T>`,
`WeakEntity<T>`, `Mount<T>`, `AppContext`, `Context<T>`, `ViewContext<T>`,
`Listener<E>` and `Subscription` provide the headless foundation. The standalone
[console example](../crates/rxui/examples/counter_state.rs) uses actual APIs.

- Strong `read`/`update` return the read guard/callback result directly and panic
  on invalid access; `try_read`/`try_update` return `AccessError`. Weak `update`
  returns a `Result`. A read guard is tied to both the entity and read context.
- A listener dispatch returns `Handled` or `TargetGone`, with an access error for
  invalid runtime/borrowing. The binding is weak and mount-scoped. Element-level
  routing and disposal are the next milestone.
- Mutations and dependent-mount invalidation happen immediately. Observers and
  deferred work run only at explicit `Runtime::flush`, after update scopes end.
  Sources coalesce pending notifications; a bounded callback budget preserves
  unprocessed work on exhaustion. Subscriptions are removed with their last handle.
- Evaluation receives read-only owner state and tracked model reads. A normally
  returned callback commits its read set; panic or access failure leaves prior
  dependencies and dirty state intact. A returned application error is ordinary
  callback data, rather than a rollback signal.
- Same-entity reentrancy, foreign runtimes, stale generations and failed
  initialization are diagnosed. Unwind restores update access and invalidates
  dependents without undoing mutations.
- Separate mounts can share one entity without sharing placement identity.
  Read tracking and dependency sets are reused across evaluations. The runtime
  stores weak entity references and runs without native or GPU initialization.

Element builders, reconciliation, Taffy layout, widgets and the native application
host remain planned. Optional features currently expose the pinned dependencies;
they do not implement those integrations.

The priorities are API clarity, simplicity, and predictable performance. RXUI
will provide declarative, builder-based UI composition over persistent typed
state, with context-and-closure updates. Taffy is the selected layout engine.

## Agreed foundation

- Views describe the UI resulting from current state through element builders.
- Stateful views and shared models use one typed handle, provisionally `Entity<T>`.
- Mutation is synchronous on the UI thread through update scopes. UI work is
  deferred and coalesced; state mutation itself is not deferred.
- `cx.listener(...)` binds a callback to its owning component's current state.
- A completed update automatically invalidates the entity and dependent views;
  ordinary mutations do not require a separate `notify()` call.
- Model reads during view evaluation establish entity-level dependencies.
- Persistent widget state is scoped by mount and element identity.
- RXUI supplies a convenient application host and an embeddable UI runtime.
- Astrelis supplies rendering/text; astrelis-winit supplies native presentation
  lifecycle. RXUI owns UI semantics and application conventions.
- The initial implementation starts with one RXUI crate and clear internal
  modules. Native integration can be feature-gated.

## Responsibilities and lifecycle

| Layer | Owns |
| --- | --- |
| Astrelis | GPU resources, renderers, frames, passes, text shaping and glyph preparation |
| astrelis-winit | Native windows/surfaces, metrics, acquisition, resize, suspension, presentation and redraw scheduling |
| RXUI runtime | Entities, transactions, dependencies, reconciliation, Taffy layout, interaction state, focus, UI preparation, painting and semantic output |
| RXUI native host | Connects the runtime to astrelis-winit, native input, AccessKit, clipboard, IME, cursor state and application lifecycle |
| Application | Domain data, component composition, commands, service policy and custom rendering resources |

`Application::new().run(...)` is the normal entry point. The host uses the
astrelis-winit runner rather than duplicating surface recovery and scheduling.
Application initialization runs once on the initial active lifecycle. Repeated
native resume restores presentation; it does not recreate models or windows that
remain registered.

The native host installs accessibility adapters in the hidden `window_created`
phase before showing a managed window. Model state survives surface replacement,
MSAA changes and presentation suspension. Initial GPU setup may synchronously
wait through astrelis-winit's desktop convenience path; this is startup behavior,
not a per-frame completion wait.

The embeddable runtime must allow the application to supply input, prepare against
its viewport and attachment format, and paint into an existing Astrelis pass.
Neither entity updates nor headless component tests require owning a native
event loop. Embedded hosts apply platform output explicitly through an adapter.
Exact embedding method signatures are still to be designed.

The first native host targets the desktop platforms supported by astrelis-winit.
A browser/mobile host needs its own initialization and lifecycle design; the
current RXUI release's web support is not a claim about this rewrite.

## Application-facing example

```rust,ignore
use rxui::prelude::*;

#[derive(Default)]
struct Counter {
    value: i32,
}

impl View for Counter {
    fn view(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        column()
            .padding(24.)
            .gap(12.)
            .child(label("Counter").font_size(24.))
            .child(label(format!("Value: {}", self.value)))
            .child(
                row()
                    .gap(8.)
                    .child(
                        button("Decrease")
                            .key("decrease")
                            .on_click(cx.listener(|this, _event, _cx| {
                                this.value -= 1;
                            })),
                    )
                    .child(
                        button("Increase")
                            .key("increase")
                            .on_click(cx.listener(|this, _event, _cx| {
                                this.value += 1;
                            })),
                    ),
            )
    }
}

fn main() -> rxui::Result<()> {
    Application::new().run(|cx| {
        let counter = cx.new(|_| Counter::default());
        cx.open_window(
            WindowOptions::new().title("RXUI Counter").size(480., 320.),
            counter,
        )?;
        Ok(())
    })
}
```

Dimensions and spacing in the UI builders are logical units. The host converts
viewport and painting geometry using the window's cached scale factor; native
attachments use physical pixels. Exact unit types remain an API design detail.

## Entities and state ownership

`Entity<T>` identifies one persistent typed value managed by a runtime. Cloning
the handle shares its identity and value; it does not clone `T`. A value need not
implement View to participate in tracked model reads and updates. If it does
implement View, its entity can be mounted as a stateful component.

Strong handles retain the value. Weak handles identify it without extending its
lifetime. Handles must include runtime identity and generation information so
that a stale handle cannot refer to a replacement value or another application's
store. Entity access is mediated by a valid context on the UI thread.

An entity's lifetime and a component mount's lifetime are different. Removing a
mount releases its widget state, callbacks and mount-scoped resources. A model
or component entity may remain alive because the application retains another
strong handle. Closing a window releases its mounts without necessarily disposing
shared document data used by other windows.

| State | Typical owner |
| --- | --- |
| Local component data: expanded sections, filters, active tabs | Fields of a view entity |
| Shared domain data: documents, projects, settings, chart series | Model entities held by relevant views/services |
| Widget interaction: hover, capture, scroll, selection, IME preedit | Retained widget state scoped to its mount/key |
| Prepared text, pipeline variants and custom renderer resources | Rendering/preparation caches or explicitly owned resources |

There should be one authoritative owner for each piece of application data.
Derived values can be computed or cached from that owner instead of being kept
in independently mutable copies.

Ordinary Rust interior mutation outside a tracked update scope is not magically
observable. Shared domain changes must use the update API. Views are read-only
descriptions; ViewContext must not expose application mutation operations that
permit state writes during view evaluation.

## Contexts

Provisional context roles:

| Context | Capabilities |
| --- | --- |
| AppContext | Create entities/windows and perform application-level operations |
| Context<T> | Mutate the current T through the supplied reference, update other entities, and arrange effects/application operations |
| ViewContext<T> | Read tracked models and view environment, bind listeners, and describe UI for the current mount |
| Event/dispatch context | Event routing and platform/application operations for handlers without a component owner |

The current component is supplied as `&self` during view evaluation and `&mut T`
during updates. Contexts carry runtime capabilities and ownership information;
they are not a substitute for direct access to the component's fields.

The first slice validates the state-context split and scoped read guards through
executable and compile-fail tests. Window/environment and dispatch capabilities
will extend it in following milestones. References and update scopes must not
survive event dispatch, an await, or disposal.

## Update semantics and transactions

```rust,ignore
document.update(cx, |document, cx| {
    document.title = "Untitled".into();
    document.modified = true;
});
```

1. Validate the runtime/handle and acquire exclusive access to the entity.
2. Execute the closure synchronously with `&mut T` and Context<T>.
3. Release exclusive access and mark the entity and its dependents dirty.
4. At the host's explicit flush boundary, process deferred notifications/effects
   without holding the mutable entity borrow.
5. Coalesce required component evaluation and window redraw work.

Two increments in one closure change the value by two immediately. The displayed
UI follows during a subsequent preparation/paint cycle. An update transaction
does not promise rollback: mutations already performed are not implicitly undone
if application code subsequently fails. It is a scheduling/borrowing boundary.

The supplied `&mut T` is used for the entity already being updated. Reading or
updating that same entity through its handle during the scope is reentrant access
and must be diagnosed. Updating a different entity is allowed, with observers
deferred until enclosing mutable accesses are released. Observer/effect cycles
also require diagnostics; batching alone does not eliminate cycles.

The first implementation invalidates conservatively at entity granularity. It
does not inspect which struct field changed or require T: PartialEq. An update
that changes nothing can still cause evaluation. Selectors or an explicit
change-sensitive operation can be added if measured workloads justify them.

The headless implementation settles strong/weak access and borrowing diagnostics
as described above. Event routing and handler error policy beyond the typed
listener remain open. Safe runtime access must not permit aliased mutable
references, and weak-target disappearance has an ordinary fallible/absent result.

## Listener binding and event flow

```rust,ignore
button("Increase")
    .on_click(cx.listener(|this, event, cx| {
        this.value += this.step;
        this.history.push(this.value);
    }))
```

The callback receives the owning component's current mutable value, the typed
event, and its update context. All component fields and methods are accessible
through `this`, subject to normal Rust privacy. The closure does not borrow the
`self` from the earlier view call.

`cx.listener` constructs a callback wrapper associated with a weak owner and the
current mount. `on_click` attaches that wrapper to an element. No handler is run
while describing the tree. Dispatch validates that the target/mount and owner
are live, enters an update scope, invokes the handler and applies automatic
invalidation after releasing the borrow. Stale callbacks do not resurrect their
owner or target a newly reused identity.

Conceptually the wrapper performs `owner.update(..., |state, cx| callback(state,
event, cx))`. It may be implemented differently to avoid unnecessary allocation
or temporary handle operations. Weak ownership, current-state access, dispatch
validity and update behavior are the public contracts.

Methods are usable as handlers:

```rust,ignore
impl Counter {
    fn increment(&mut self, _event: &ClickEvent, _cx: &mut Context<Self>) {
        self.value += 1;
    }
}

button("Increase").on_click(cx.listener(Self::increment))
```

Plain callbacks can receive the event and dispatch context without a component
owner. A listener passed to a stateless child remains bound to the component that
created it; it does not become owned by the child merely because that child
attaches it to a button.

Captured values retain ordinary Rust semantics. Capturing an earlier count or
document ID deliberately captures a snapshot; changing data should normally be
read through `this` or a tracked entity access at execution time. No mandatory
central message enum is needed for ordinary component interactions. Typed domain
events/commands remain useful where explicit communication is appropriate.

Pointer, keyboard and accessibility activation must converge on the same control
behavior. Events use retained hit-test geometry, focus and capture. Propagation,
default-action cancellation and error behavior need precise dispatch tests.

## Shared models, props and dependencies

```rust,ignore
struct Document { title: String, modified: bool }
struct Editor { document: Entity<Document>, cursor: usize }
struct StatusBar { document: Entity<Document> }

let document = cx.new(|_| Document {
    title: "Untitled".into(), modified: false,
});
let editor = cx.new(|_| Editor { document: document.clone(), cursor: 0 });
let status = cx.new(|_| StatusBar { document: document.clone() });
```

During a view's evaluation, `self.document.read(cx)` records a dependency on the
document for that mounted view. The new read set replaces its old dependency set
when evaluation returns normally. If it stops reading a model, it stops depending on
that model. Arbitrary reads in update handlers do not register view dependencies.

An editor callback can update the document through `this.document.update(cx,
...)`. Dependent editor/status mounts become dirty. Other components that did
not read the document are unaffected by that model's invalidation. Reads of
different fields still depend on the whole document in the initial design.

Mounting a child Entity<View> establishes a component boundary; it does not
implicitly subscribe the parent to every field in the child. If the parent reads
child data explicitly, that read does establish a dependency. A parent update
can still change a child's inputs, placement or available constraints.

Stateless composition is an ordinary function receiving data and callbacks:

```rust,ignore
fn counter_controls(value: i32, increment: Listener<ClickEvent>) -> impl IntoElement {
    row()
        .child(label(format!("{value}")))
        .child(button("+").on_click(increment))
}
```

Its description is evaluated in its caller's component scope. Stateful child
components can be composed through `.child(self.editor.clone())`. Entity-backed
component props/reconfiguration need an explicit design before implementation;
passing an entity handle does not itself synchronize new constructor arguments.
Typed scoped providers for themes or shared services are a possible later API,
not a prerequisite for ordinary explicit data flow.

## Identity, reconciliation and retained widget state

| Identity | Meaning |
| --- | --- |
| Entity | One persistent model/component value |
| Mount | One placement/instance of a view in a window/tree |
| Element key | A child/widget identity within its parent scope |

Unkeyed fixed children may use structural position and compatible element type.
Dynamic children use stable keys. Keys are unique among siblings, not globally;
duplicate sibling keys are diagnosed. Changing a widget type or removing its
mount resets the associated widget state unless an explicit persistence policy
exists. Entity identity does not make one widget state global across windows.

```rust,ignore
column().children(self.items.iter().map(|item| {
    label(item.title.clone()).key(item.id)
}))
```

Reordering keyed items preserves editing/focus state for surviving compatible
widgets. Recreating entities on every view call would create new state, so
ViewContext is not an unrestricted factory for persistent models. Children are
created during initialization/updates or through a future explicitly keyed
mount-state facility.

Element descriptions are temporary values. Reconciliation retains compatible
nodes and resources, updates properties/handlers, and releases removed scopes.
Handlers cannot retain a mutable view borrow. Type erasure and storage strategy
must be measured; the API must not require separately boxed allocations for every
style property or every draw.

## Layout, text and rendering

Taffy provides the layout solver, initially for flex-based rows/columns. RXUI
provides node ownership, dirty propagation and text/custom-element measurement.
Changes to available constraints can affect descendants and ancestors; component
boundaries do not prevent necessary layout propagation.

Text measurement uses Astrelis's CPU shaping/layout. Cache keys must account for
content, fonts/style, relevant constraints and wrap behavior. Raster density and
attachment compatibility are preparation inputs, with GPU caches scoped to the
appropriate device. Ordinary style/placement changes must not reload fonts,
reshape unchanged text, regenerate glyphs or recreate pipelines.

| Change | Expected affected work |
| --- | --- |
| Hover, selection, caret blink | Interaction/painting; retain shaped text |
| Text color/opacity | Painting; retain text layout and geometry |
| Text/font properties | Relevant shaping/layout and prepared text |
| Width/constraints | Relevant layout and text reflow |
| Event callback | Handler association; no layout work by itself |
| Chart samples | Custom resource update/painting; no unrelated text work |

Preparation is completed before managed surface acquisition. Pipeline variants
are selected using Astrelis's current RenderFormat, including samples and
depth/stencil. A convenient host opens a theme-configured UI pass; the embedded
API accepts an existing pass. Custom element preparation/painting must support
charts and 2D/3D content, with explicit bounds, transforms, clipping and ordering.
Custom element trait signatures are a follow-up design task.

A freshly acquired surface generally requires repainting visible UI in order.
Retained descriptions, geometry and layout do not imply automatic partial-surface
redraw. Retained layers and damage-based compositing are separate optimizations.

## Input, editable text and semantics

Runtime widget state retains hover, capture, focus, selection, scrolling and IME
composition across description rebuilds. Application-controlled text/value props
remain authoritative. A text control must preserve ordered pending edits between
native events and reconciliation; a delayed redraw must not drop successive edits.
IME preedit is transient editing state, while commit changes the controlled value.
Application acceptance, normalization or rejection of proposed edits must have a
defined reconciliation policy.

AccessKit integration is installed before showing the native window. Semantic
IDs follow mount/key identity and remain consistent with input routing. Native
actions use the same handlers as keyboard/pointer input. Lazy accessibility output
must still account for activation, focus, text selection and virtualized content.

Focus movement and widget visual changes can invalidate painting without invoking
an application's state listener. Scheduling component evaluation, layout,
painting and semantic publication are distinct dirty states.

## Tasks, effects and scheduling

UI mutation scopes cannot cross an await or move onto worker threads. Background
jobs compute owned data; completions return to the UI thread and enter a new
update scope. Exact executor/proxy APIs are open. Jobs/subscriptions need explicit
entity- or mount-scoped ownership and cancellation/disposal behavior. Out-of-order
results require application/request generation checks where relevant; reading
live state in a listener does not solve asynchronous result races.

Native model updates, task completions and application operations continue when
a window is hidden or its surface is unavailable. Presentation availability
gates acquisition/painting rather than all application progression. Dirty work is
retained and processed when useful; a hidden surface must not cause a retry loop.

OnDemand is the default. Timed visual work, such as caret blink and animation,
uses deadlines; continuous redraw is explicit. The host delegates native redraw
coalescing, retry pacing and surface recovery to astrelis-winit.

The normal pipeline is native input -> routed event/update -> coalesced
invalidation -> dirty view evaluation/reconciliation -> affected layout and
resource preparation -> acquisition -> paint/submission. Platform/semantic
updates have their own validity requirements and must not be starved indefinitely
by failed presentation. Input/layout/semantic geometry must use coherent retained
snapshots; preparation and presentation are not rollback transactions.

## Initial implementation sequence and acceptance

1. **Headless state runtime (implemented):** entity ownership/generations, read/update leases,
   transactions, weak targets, deferred notifications, dependency replacement and
   disposal. Settle return/error types with executable API tests.
2. **Declarative core:** View/IntoElement, builders, scoped identity, keyed
   reconciliation, Taffy integration and incremental measurement/cache behavior.
3. **Essential controls:** labels, buttons, text input, focus traversal, clipping,
   controlled editing/IME behavior and a semantic model.
4. **Native host:** lifecycle over astrelis-winit, default fonts/theme, adapter
   setup, platform input/output and custom-loop embedding.
5. **Validation application:** shared data in two windows, keyed editable items,
   background completion and a custom chart. Add virtualization before large-data
   list benchmarks; rendering every row is not a scalability strategy.

Acceptance includes:

- Live-state listener access and immediate repeated mutations, with one coalesced
  UI update when appropriate.
- Shared-model reads invalidate the correct mounts; obsolete dependencies are
  removed and unrelated component descriptions remain unevaluated.
- Keyed reorder preserves compatible interaction state; removal disposes scopes
  and stale callbacks/handles cannot target reused identities.
- Reentrancy, weak disappearance and asynchronous disposal have defined behavior.
- Rebuilding unchanged descriptions does not increase shaping/raster/geometry or
  pipeline counters; selection/hover/caret work retains expensive text resources.
- Layout invalidation remains correct when constraints or sibling sizes change.
- Keyboard, pointer, native accessibility and controlled text events agree.
- Two windows share domain data but retain independent focus/scroll and scheduling.
- Idle applications have no periodic redraw; model/task progression survives
  presentation suspension, and custom painting respects clipping/DPI/order.
- Headless behavior can be tested without opening a native window. GPU/native
  checks measure preparation and painting separately from presentation latency.

A broad widget catalog, docking/editor compositions, React-style positional
hooks, field-level signal graphs, damage compositing and browser/mobile runners
are later work. They must build on the same state/identity/access contracts.

## Open API details for following milestones

- Routed handler errors and non-component dispatch capabilities.
- Window/environment capabilities added to the implemented context split.
- Element/listener representation, closures and reconciliation storage.
- Stateful child props, mount hooks and scoped disposal APIs.
- Style/value types, inherited theme/default fonts and logical unit types.
- Event propagation/default actions and editable-value reconciliation policy.
- Task execution, completion proxies, cancellation and effect ordering.
- Exact embedded preparation/platform-output/custom-element interfaces.

These refine the agreed model; they should be resolved through small consumer
examples and headless tests before growing the framework.

## Repository transition

The rewrite now runs on a fresh orphan `main`. The previous tracked history at
`ae5af66c8b202fbed42fd7fc566422c01df0065d` is preserved under `legacy/main`.
The former source, working documentation, ignored lockfile and untracked
prototype were also preserved in the local sibling export
`rxui-legacy-20261006-100735`, with a `PRESERVATION.json` inventory. The prototype
was moved there rather than imported into the new workspace. Remote `main`
remains untouched. The initial new history should include this design and minimal
workspace; importing old code is selective, with no compatibility obligation.

The prototype supplies evidence for control editing, retained text, virtualization
and accessibility. Its message/action API and explicit rectangles do not define
the new declarative/state API.

## References and existing evidence

- [Astrelis window/runner contract](https://github.com/hxyulin/astrelis/blob/1f773d4a13057db8e15adc768c1d59cf979e65ed/docs/winit.md).
- [Astrelis text interaction and performance](https://github.com/hxyulin/astrelis/blob/1f773d4a13057db8e15adc768c1d59cf979e65ed/docs/performance/text-interaction.md).
- Existing isolated RXUI prototype: preserved in the local legacy export described
  above; it is not part of the new workspace.
- [Taffy](https://docs.rs/taffy/latest/taffy/): selected layout infrastructure.
- [GPUI Entity](https://docs.rs/gpui/latest/gpui/struct.Entity.html): reference for
  typed persistent handles and context/closure access. RXUI's automatic
  invalidation behavior is its own proposed contract.
- [eframe](https://docs.rs/eframe/latest/eframe/) and
  [egui-winit State](https://docs.rs/egui-winit/latest/egui_winit/struct.State.html):
  references for convenient application hosting and separable native integration.
- [React state snapshots](https://react.dev/learn/state-as-a-snapshot) and
  [shared state ownership](https://react.dev/learn/sharing-state-between-components):
  context for declarative data flow. RXUI's live mutable update references do not
  have React function components' render-snapshot setter semantics.
