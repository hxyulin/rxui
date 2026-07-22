# Scale an application with feature-local messages

RXUI applications always dispatch one typed root message through `App::update`.
That direct model remains the best starting point for a small application:

```rust
enum Message {
    Increment,
    Save,
}
```

As an application grows, split messages and state by product feature rather
than by widget type. The root enum remains the ordering and dispatch boundary,
while each feature handles its own smaller enum:

```rust
enum Message {
    Workspace(WorkspaceMessage),
    Services(ServiceMessage),
}

enum WorkspaceMessage {
    Counter(CounterId, CounterMessage),
}

enum CounterMessage {
    Increment,
    ResetLater,
    ResetNow,
}
```

Run `cargo run -p rxui --example message_architecture` for the complete
two-counter companion application.

## Retain a mapper with the feature UI

`MessageMapper<Local, Root>` wraps a local message in its parent variants. It
is cheap to clone, so retained widget listeners can own it without repeating
the root topology at every emission site:

```rust
let workspace_messages = MessageMapper::new(Message::Workspace);
let id = CounterId(1);
let counter_messages = workspace_messages
    .map_child(move |message| WorkspaceMessage::Counter(id, message));

let increment = ui.button(parent, "+1").finish();
ui.on_click(increment, move |event| {
    counter_messages.emit(event, CounterMessage::Increment);
});
```

Mapping is typed and synchronous. It adds one closure call, does not create a
second queue, and does not require either message type to implement `Clone`,
`Send`, `Debug`, or `Serialize`. The mapping closure itself is `Send + Sync`
so the same abstraction also composes with a cross-thread message proxy.

Use small, stable IDs such as a newtype around an integer, UUID, or persistent
domain key for repeated feature instances. Do not identify features by their
current row index, display name, or retained widget handle: those change when
the UI is sorted, renamed, or rebuilt.

## Update a feature through `MappedAppCx`

The root update matches once, locates the feature by ID, and gives it a context
that accepts the feature's local message type:

```rust
fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
    let Message::Workspace(WorkspaceMessage::Counter(id, message)) = message;
    let counter = self.counter_mut(id);
    let mapper = MessageMapper::new(move |message| {
        Message::Workspace(WorkspaceMessage::Counter(id, message))
    });
    let mut counter_cx = cx.map_messages(mapper);
    counter.update(&mut counter_cx, message)
}
```

Inside `counter.update`, the familiar message-producing operations take
`CounterMessage`:

```rust
fn update(
    &mut self,
    cx: &mut MappedAppCx<'_, CounterMessage, Message>,
    message: CounterMessage,
) {
    match message {
        CounterMessage::ResetLater => {
            self.reset_timer = Some(cx.set_timeout(
                Duration::from_secs(2),
                CounterMessage::ResetNow,
            ));
        }
        CounterMessage::ResetNow => self.value = 0,
        // ...
    }
}
```

`post`, `set_timeout`, `set_interval`, `set_interval_with`, and `proxy` all map
back into the root queue. Ordinary UI, window, clipboard, clock, and rendering
operations are forwarded. `cx.root()` is the explicit escape hatch when a
feature intentionally needs a root-level operation or message.

Mapped contexts may be nested. Given a `MessageMapper<ChildMessage,
FeatureMessage>`, calling `feature_cx.map_messages(child_mapper)` composes the
child, feature, and root layers without type erasure.

## Preserve event meaning and lifecycle

Name domain messages for what happened or what the user intends, not for the
widget that happened to produce them. A useful vocabulary is:

- **Intent:** `SaveRequested`, `DeleteSelection`, `ZoomToFit`.
- **Result:** `DocumentLoaded(Result<...>)`, `ExportCompleted(...)`.
- **External event:** `FileChanged`, `ConnectionLost`, `SamplesArrived`.
- **Interaction lifecycle:** begin, preview, commit, and cancel.

Continuous interactions should make their lifecycle explicit when undo,
autosave, collaboration, or cancellation depends on it:

```rust
enum InteractionPhase {
    Begin,
    Preview,
    Commit,
    Cancel,
}

enum DiagramMessage {
    MoveNodes {
        ids: Vec<NodeId>,
        delta: Point,
        phase: InteractionPhase,
    },
}
```

Keep ephemeral mechanics such as hover, pressed state, text composition, and
pointer capture in retained widgets unless application behavior must observe
them. Promote state into a message only when it affects the domain, commands,
undo, persistence, or another feature.

## Keep payloads bounded

Messages should normally carry stable IDs, compact edits, or shared immutable
snapshots rather than copying entire documents and data series:

- Keep retained chart and document storage outside the message queue.
- Use `Arc` for immutable snapshots shared between producers and the UI.
- Bound live-data buffers explicitly.
- Avoid borrowed references, closures, and UI handles in domain messages.
- Use a result message to return finite asynchronous work through `update`.

A mapper does not make a local message `Send`. Synchronous widget emissions
may contain `Rc` or other UI-thread-only values. Calling `MappedAppCx::proxy`
requires both the local and root messages to be `Send`, making the thread
boundary explicit at compile time:

```rust
let proxy: MessageProxy<CounterMessage> = counter_cx.proxy();
worker(move || {
    let _ = proxy.post(CounterMessage::Loaded(result));
});
```

Proxy and timer deliveries have no source window. Widget messages, and posts
made while handling them, retain the originating window. In a multi-window
application, put the target window or feature ID in externally delivered
messages rather than relying on `source_window()`.

## Coalesce replaceable previews

Ordinary `post` preserves every message in FIFO order. When only the newest
pending observation matters, use a stable `MessageKey` with `post_latest`:

```rust
let key = MessageKey::new("counter.preview", u64::from(self.id.0));
for value in 1..=1_000 {
    cx.post_latest(key, CounterMessage::PreviewValue(value));
}
```

The first post reserves a queue position. Later posts with the same key replace
its payload in place, so this example delivers one `PreviewValue(1000)` without
moving it past unrelated messages. The newest source-window metadata also wins.
Once the entry has been removed for dispatch it is no longer replaceable; a
reentrant `post_latest` starts a new pending entry and remains subject to the
normal bounded re-drain guard.

Use descriptive static namespaces and stable instance IDs. Two counter
instances can both use `"counter.preview"` because their numeric IDs keep the
keys distinct. `MessageKey::singleton("window.resize")` is convenient when
there is only one producer in the application.

Latest-value posting fits pointer or resize previews, chart viewports,
progress, and invalidation notifications. Do not use it for button actions,
undo commits, transactions, file results, or anything whose count and complete
ordering carry meaning. Coalescing is limited to queued posts; direct widget
emissions and cross-thread `MessageProxy` deliveries remain ordinary messages.

## Create timeout messages when they fire

`set_timeout` retains a message immediately. Use `set_timeout_with` when the
message should be constructed on the UI thread at the deadline:

```rust
cx.set_timeout_with(Duration::from_millis(250), || {
    CounterMessage::PreviewValue(expensive_snapshot())
});
```

The factory runs exactly once. Cancelling the returned `TimerId` drops it
without invoking it, and neither the message nor the factory needs to be
`Clone` or `Send`. The deterministic harness coalesces missed interval periods
the same way as the production event loop: one interval delivery per
`advance`, followed by a deadline strictly after the advanced time.

## Deliver finite background work as tasks

RXUI owns task identity, cancellation, and result delivery without requiring
an async runtime. Register a one-shot completion and move it into the executor
already used by your application:

```rust
let completion = cx.register_task(Message::DocumentLoaded);
let task = completion.id();

executor.spawn(async move {
    let result = load_document().await;
    let _ = completion.complete(result);
});
```

`TaskCompletion` is one-shot. Dropping it before completion abandons the task;
calling `cx.cancel_task(task)` suppresses a late result. Cancellation does not
forcibly abort an external future, so combine `TaskId` with the executor's own
abort handle when stopping the underlying work matters.

The completed value and mapping closure are `Send` because they cross the
runtime wakeup bridge. The future itself may remain non-`Send` in a browser
`spawn_local`, and the application message need not be `Send`: RXUI invokes
the mapper and constructs that message on the event-loop thread. Mapped
contexts automatically wrap the resulting feature-local message.

On native targets, finite filesystem, decoding, and similar blocking work can
use the application-owned bounded pool:

```rust
let task = cx.spawn_blocking(
    move || std::fs::read(path),
    |result| Message::DocumentLoaded(result),
)?;
```

The mapper receives `Result<Output, TaskError>`, where a caught worker panic is
reported as `TaskError::Panicked` without exposing its payload. Pool saturation
or worker startup failure is returned immediately as `TaskSpawnError`.
Configure worker and waiting-queue bounds by passing a customized `TaskConfig`
to `AppConfig::tasks`; defaults use one to four workers based on available
parallelism and a 64-job waiting queue.

Tasks are application-scoped and their messages have no source window. Closing
one window does not cancel them; a window-owned feature should cancel its IDs
from `window_closed`. Application shutdown cancels all active task delivery,
and running blocking code is allowed to finish without delaying shutdown.

`AppHarness` records work instead of creating threads. Use
`pending_task_ids`, `run_blocking_task`, and `complete_task` to select completion
order deterministically.

## Choose the smallest architecture that works

Feature enums and mapped contexts are optional. Keep a single root `match`
when it is still clear. RXUI does not require reducer traits, a global event
bus, serialization, or subscriptions, and mapping does not change FIFO
dispatch. Queue coalescing and declarative long-lived subscriptions are
separate tools for high-rate and lifecycle-owned sources.
