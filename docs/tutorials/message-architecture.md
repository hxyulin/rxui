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

## Choose the smallest architecture that works

Feature enums and mapped contexts are optional. Keep a single root `match`
when it is still clear. RXUI does not require reducer traits, a global event
bus, serialization, or subscriptions, and mapping does not change FIFO
dispatch. Queue coalescing and declarative long-lived subscriptions are
separate tools for high-rate and lifecycle-owned sources.
