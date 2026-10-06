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
that should belong to a particular placement, store the Task in a separately
owned per-window view entity. A dedicated mount-scoped task API is future work.
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
