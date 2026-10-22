# Scoped background tasks

Status: implemented behind the optional `tasks` feature; the native Application
host supplies an executor and event-loop wakeup. The state/layout runtime creates
no workers until a host configures execution.

## Work and completion

A handler launches owned, Send + 'static work and receives the result in a fresh,
synchronous UI update. It cannot carry `&mut T`, a read guard or a context across
an await. Capture an owned snapshot of the inputs the job needs. Completion has
access to the current model, rather than the model at launch.

```rust
// Inside a Context<Model> update; Model owns task: Option<Task>.
let request = this.request;
this.task = Some(cx.spawn(async move {
    rxui::sleep(std::time::Duration::from_millis(100)).await;
    request * 100
}, move |this, result, _cx| {
    if this.request == request {
        match result {
            Ok(value) => this.value = value,
            Err(error) => this.error = Some(error.to_string()),
        }
    }
}));
```

`Context<T>::spawn` binds completion weakly to the current entity. Its callback
is `FnOnce(&mut T, TaskResult<R>, &mut Context<T>)`. `AppContext::spawn` is scoped
to the runtime instead; its callback receives `TaskResult<R>` and AppContext.
The corresponding `spawn_blocking` methods take an owned closure and run it on
the executor's blocking adapter. Both paths have `try_spawn`/`try_spawn_blocking`
variants returning `SpawnError`. Convenience methods panic on setup failure.

`TaskResult<R> = Result<R, TaskError>` distinguishes execution panics from the
future's ordinary output. Domain errors remain in that output: a future returning
`Result<Data, ApiError>` delivers `TaskResult<Result<Data, ApiError>>`. This keeps
successful outputs unrestricted and avoids requiring one framework error type.
Panic capture depends on unwinding; panic=abort cannot recover a process abort.
UI completion callback panics retain normal Rust unwind behavior.

## Ownership and cancellation

Keep the returned Task, usually in a component field. Dropping it cancels delivery
and aborts future polling. `cancel()` is explicit and idempotent; `detach(self)`
releases handle ownership while preserving the original entity/runtime scope.
`is_finished()` means cancellation or UI completion has been claimed; an output
still queued for delivery is not yet finished.

Cancellation suppresses a result even if it was already queued. Replacing a
stored Task cancels the previous job. Disposing the owner cancels even detached
owner-scoped work. Completion bindings do not themselves keep owners alive;
caller captures can still intentionally retain strong entities.

Running blocking work cannot be forcibly interrupted. Cancellation suppresses its
result and skips a job that has not started, but already running work may finish.
Runtime shutdown cancels registrations and releases UI callback captures without
joining arbitrary blocking jobs. External services need their own cooperative
cancellation if they must stop underlying side effects.

Task ownership follows the entity, not the initiating window. Two windows can
share one entity and its pending job. Closing one window leaves that job alive
while another placement or external strong handle retains the entity. For work
that belongs to one placement, capture `cx.task_scope()` in `View::view` (or
`mount.task_scope()` in a custom host), then start it from a listener:

```rust,ignore
let scope = cx.task_scope();
let owner = cx.listener(move |this, _, cx| {
    this.task = Some(scope.spawn(cx, async { 42 }, |result, cx| {
        // Update weak application entities or explicit placement handles here.
        println!("{:?}", result);
    }).unwrap());
});
```

`TaskScope::spawn` and `spawn_blocking` return `Result<Task, SpawnError>`.
The capability is weak and independent for each mount, even when two windows
share an entity. Removing the last strong Mount cancels its jobs during runtime
synchronization; delivery checks mount liveness too, so queued results cannot
escape disposal. Detaching preserves this scope. A replacement mount gets a fresh
scope; old capabilities return `Disposed`. Foreign runtime contexts return
`WrongRuntime`. Completions receive AppContext and have no implicit mount;
use weak entity captures to update shared data, or explicit placement handles.
Closing one window cancels only its mount jobs. Hidden but retained components
keep their scopes; removing an element within a component does not dispose the
component mount. Use a child view for a narrower lifetime.

Task completions have no implicit `cx.window()`; capture a WindowHandle explicitly
when an operation needs one and check its lifecycle.

Request generations remain useful for overlapping operations, detached jobs and
application-level stale-result policy. Cancelling delivery does not undo work
already applied by the external service.

## Executor and host boundary

`TaskExecutor` defines `spawn(BackgroundFuture)` and optional
`spawn_blocking(BlockingJob)`. Adapters must reject work without starting it and
schedule without blocking the UI thread. `Application::executor(Arc<dyn
TaskExecutor>)` accepts an application's service-runtime adapter. Futures needing
a particular reactor must run on a compatible executor; RXUI does not implicitly
install Tokio or a network runtime.

The default desktop host creates two async workers and two separate blocking
workers when run begins. It uses async-executor to poll futures; `rxui::sleep`
uses async-io's timer. Blocking jobs do not occupy async polling threads. The
headless runtime can call `configure_tasks(executor, wake)` once and deliver ready
results using `poll_tasks()` on the UI thread, followed by its normal `flush()`.

Completion wakes the native event loop through its proxy. Wake notifications
coalesce; the host does not periodically poll or redraw to discover jobs.
`poll_tasks` consumes up to 1,024 queue records per call and reschedules if its
budget is exhausted. Callbacks run without the task registry or model metadata
borrowed, so they can update entities, schedule other jobs or cancel handles.
Observers remain deferred until flush after those update scopes end.

Application/task progression is independent of frame acquisition. Completions
and close/exit commands can run while presentation is suspended; new native
window creation waits for resume. Frame preparation processes pending work before
acquisition, and queued closes prevent that window from acquiring another frame.

Tests cover live-state completion, disposal, queued-result cancellation,
superseded tasks, panic/setup failures, callback unwind, coalesced wakeups and
bounded draining. A real worker-pool test checks that a timer progresses while a
blocking worker is occupied. The native example exercises shared-window jobs.

Executor implementation references: [async-executor](https://docs.rs/async-executor/latest/async_executor/struct.Executor.html),
[futures-util Abortable](https://docs.rs/futures-util/latest/futures_util/future/struct.Abortable.html),
and [async-channel](https://docs.rs/async-channel/latest/async_channel/).

## Streaming updates from external runtimes

A long-lived service does not need one RXUI task per sample. Send owned values over
a channel, then call an `ApplicationWakeHandle` obtained from `cx.wake_handle()`.
`Application::woken` drains the receiver inside a fresh UI update, even while
presentation is suspended. Keep RXUI entities and window handles on the UI thread;
only the channel sender, owned values and wake handle cross threads.

For example, an application with an existing Tokio runtime can publish at 50 Hz.
This sketch assumes `Model` implements `View` and has a `sample: u64` field;
`tokio_handle` is the application's `tokio::runtime::Handle`:

```rust
use std::{cell::RefCell, rc::Rc, sync::mpsc, time::Duration};
use rxui::prelude::*;

let (sender, receiver) = mpsc::sync_channel(1);
let model = Rc::new(RefCell::new(None::<Entity<Model>>));
let target = model.clone();
Application::new()
    .woken(move |cx| {
        // Keep only the latest sample when several arrive before delivery.
        let latest = receiver.try_iter().last();
        if let (Some(sample), Some(model)) = (latest, target.borrow().as_ref()) {
            model.update(cx, |this, _| this.sample = sample);
        }
    })
    .run(move |cx| {
        let entity = cx.new(|_| Model { sample: 0 });
        *model.borrow_mut() = Some(entity.clone());
        cx.open_window(WindowOptions::new(), entity)?;
        let wake = cx.wake_handle()?;
        tokio_handle.spawn(async move {
            let mut ticks = tokio::time::interval(Duration::from_millis(20));
            let mut sample = 0;
            loop {
                ticks.tick().await;
                sample += 1;
                match sender.try_send(sample) {
                    Ok(()) => if wake.wake().is_err() { break; },
                    Err(mpsc::TrySendError::Full(_)) => {},
                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                }
            }
        });
        Ok(())
    })?;
```

The bounded channel drops a new sample while one is pending. A shared latest-value
mailbox can instead replace pending data; choose the stream's overflow policy
explicitly. Wake messages carry no data and each successful call schedules a
`woken` callback. For higher rates, coalesce notifications with a pending flag or
send one batch per wake. The service owns its timer, cancellation and shutdown;
RXUI supplies UI-thread delivery and event-loop wakeup, without installing Tokio.
