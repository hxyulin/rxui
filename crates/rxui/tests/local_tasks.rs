//! Local task lifetime, delivery, fairness, and idle-host regressions.
#![cfg(feature = "tasks")]
use rxui::*;
use std::{
    cell::Cell,
    future::Future,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Wake, Waker},
};
#[derive(Default)]
struct Executor(async_executor::LocalExecutor<'static>);
impl LocalTaskExecutor for Executor {
    fn spawn_local(&self, future: LocalFuture) -> Result<(), SpawnError> {
        self.0.spawn(future).detach();
        Ok(())
    }
}
fn setup() -> (Runtime, Rc<Executor>, Rc<Cell<usize>>) {
    let mut runtime = Runtime::new();
    let executor = Rc::new(Executor::default());
    let wakes = Rc::new(Cell::new(0));
    let count = wakes.clone();
    runtime
        .configure_local_tasks(executor.clone(), move || count.set(count.get() + 1))
        .unwrap();
    (runtime, executor, wakes)
}
fn drain(executor: &Executor) {
    while executor.0.try_tick() {}
}
#[derive(Default)]
struct Notice(AtomicUsize);
impl Wake for Notice {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[test]
fn pending_rc_future_and_rc_result_wake_idle_host_then_defer_delivery() {
    let (mut rt, ex, wakes) = setup();
    let (send, recv) = async_channel::bounded::<()>(1);
    let capture = Rc::new(42);
    let weak = Rc::downgrade(&capture);
    let seen = Rc::new(Cell::new(0));
    let output = seen.clone();
    let task = rt.update(|cx| {
        cx.spawn_local(
            async move {
                recv.recv().await.unwrap();
                capture
            },
            move |r, _| output.set(*r.unwrap()),
        )
    });
    assert!(ex.0.try_tick());
    let notice = Arc::new(Notice::default());
    let waker = Waker::from(notice.clone());
    let mut cx = std::task::Context::from_waker(&waker);
    let mut tick = Box::pin(ex.0.tick());
    assert!(tick.as_mut().poll(&mut cx).is_pending());
    send.try_send(()).unwrap();
    assert!(notice.0.load(Ordering::SeqCst) > 0);
    assert!(tick.as_mut().poll(&mut cx).is_ready());
    assert_eq!(seen.get(), 0);
    assert_eq!(wakes.get(), 1);
    assert!(!task.is_finished());
    assert_eq!(rt.poll_tasks(), 1);
    assert_eq!(seen.get(), 42);
    assert!(task.is_finished());
    assert!(weak.upgrade().is_none());
}
#[test]
fn queued_success_and_panic_are_cancelled_by_cancel_or_drop() {
    let (mut rt, ex, _) = setup();
    for panics in [false, true] {
        let task = rt.update(|cx| {
            cx.spawn_local(
                async move {
                    assert!(!panics, "work panic");
                    Rc::new(1)
                },
                |_, _| panic!("cancelled callback"),
            )
        });
        drain(&ex);
        if panics {
            drop(task);
        } else {
            assert!(task.cancel());
            assert!(!task.cancel());
        }
        assert_eq!(rt.poll_tasks(), 0);
        assert_eq!(rt.pending_tasks(), 0);
    }
}
#[test]
fn owner_and_mount_disposal_replaced_slots_and_foreign_scope() {
    let (mut rt, ex, _) = setup();
    let model = rt.update(|cx| cx.new(|_| 0));
    let mount = rt.update(|cx| cx.mount(&model).unwrap());
    let scope = mount.task_scope();
    let mut other = Runtime::new();
    assert!(matches!(
        other.update(|cx| scope.spawn_local(cx, async {}, |_, _| {})),
        Err(SpawnError::WrongRuntime)
    ));
    let task = rt.update(|cx| {
        model.update(cx, |_, cx| {
            cx.spawn_local(async { Rc::new(9) }, |_, _, _| panic!("dead owner"))
        })
    });
    rt.update(|cx| {
        scope
            .spawn_local(cx, async {}, |_, _| panic!("dead mount"))
            .unwrap()
            .detach()
    });
    drain(&ex);
    drop(mount);
    drop(model);
    rt.synchronize();
    let replacement = rt.update(|cx| cx.new(|_| 77));
    assert!(task.is_finished());
    assert_eq!(rt.poll_tasks(), 0);
    assert_eq!(rt.update(|cx| *replacement.read(cx)), 77);
    assert!(matches!(
        rt.update(|cx| scope.spawn_local(cx, async {}, |_, _| {})),
        Err(SpawnError::Disposed)
    ));
}
#[test]
fn runtime_disposal_aborts_detached_future_without_retaining_mailbox() {
    let (mut rt, ex, wakes) = setup();
    let value = Rc::new(());
    let weak = Rc::downgrade(&value);
    let callback = Rc::new(());
    let callback_weak = Rc::downgrade(&callback);
    let task = rt.update(|cx| {
        cx.spawn_local(
            async move {
                std::future::pending::<()>().await;
                drop(value)
            },
            move |_, _| drop(callback),
        )
    });
    assert!(ex.0.try_tick());
    drop(rt);
    assert!(task.is_finished());
    assert!(callback_weak.upgrade().is_none());
    assert_eq!(
        Rc::strong_count(&wakes),
        1,
        "pending futures and handles must not retain the wake mailbox"
    );
    let before = wakes.get();
    drain(&ex);
    assert!(weak.upgrade().is_none());
    assert_eq!(wakes.get(), before);
    drop(task);
}
#[test]
fn callbacks_can_spawn_and_cancel_without_registry_borrows() {
    let (mut rt, ex, _) = setup();
    let target = Rc::new(std::cell::RefCell::new(None::<LocalTask>));
    let cancellation = target.clone();
    rt.update(|cx| {
        cx.spawn_local(async {}, move |_, cx| {
            cancellation.borrow_mut().take().unwrap().cancel();
            cx.spawn_local(async { Rc::new(1) }, |r, _| assert_eq!(*r.unwrap(), 1))
                .detach();
        })
        .detach();
    });
    drain(&ex); // Enqueue cancelling callback first, regardless of executor order.
    *target.borrow_mut() =
        Some(rt.update(|cx| cx.spawn_local(async {}, |_, _| panic!("cancelled"))));
    drain(&ex);
    assert_eq!(rt.poll_tasks(), 1);
    drain(&ex);
    assert_eq!(rt.poll_tasks(), 1);
}
#[test]
fn callback_unwind_rearms_wake_and_preserves_original_panic_and_later_results() {
    let (mut rt, ex, wakes) = setup();
    rt.update(|cx| {
        cx.spawn_local(async {}, |_, _| panic!("original callback"))
            .detach()
    });
    drain(&ex);
    let seen = Rc::new(Cell::new(false));
    let out = seen.clone();
    rt.update(|cx| cx.spawn_local(async {}, move |_, _| out.set(true)).detach());
    drain(&ex);
    assert_eq!(wakes.get(), 1);
    let panic =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rt.poll_tasks())).unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"original callback"));
    assert_eq!(wakes.get(), 2);
    assert_eq!(rt.poll_tasks(), 1);
    assert!(seen.get());
}
#[test]
fn rejected_and_unwinding_executors_release_registration_and_captures() {
    struct Reject(bool);
    impl LocalTaskExecutor for Reject {
        fn spawn_local(&self, _: LocalFuture) -> Result<(), SpawnError> {
            if self.0 {
                panic!("executor panic")
            }
            Err(SpawnError::Rejected("full".into()))
        }
    }
    for panics in [false, true] {
        let mut rt = Runtime::new();
        rt.configure_local_tasks(Rc::new(Reject(panics)), || {})
            .unwrap();
        let capture = Rc::new(());
        let weak = Rc::downgrade(&capture);
        let future = Rc::new(());
        let future_weak = Rc::downgrade(&future);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rt.update(|cx| {
                cx.try_spawn_local(async move { drop(future) }, move |_, _| drop(capture))
            })
        }));
        assert!(if panics {
            result.is_err()
        } else {
            matches!(result, Ok(Err(SpawnError::Rejected(_))))
        });
        assert!(weak.upgrade().is_none());
        assert!(future_weak.upgrade().is_none());
        assert_eq!(rt.pending_tasks(), 0);
        assert_eq!(rt.poll_tasks(), 0);
    }
    let mut rt = Runtime::new();
    assert!(matches!(
        rt.update(|cx| cx.try_spawn_local(async {}, |_, _| {})),
        Err(SpawnError::Unavailable)
    ));
}
#[test]
fn local_budget_reschedules_and_background_is_not_starved() {
    struct Immediate;
    impl TaskExecutor for Immediate {
        fn spawn(&self, future: BackgroundFuture) -> Result<(), SpawnError> {
            futures_lite::future::block_on(future);
            Ok(())
        }
    }
    let (mut rt, ex, wakes) = setup();
    rt.configure_tasks(Arc::new(Immediate), || {}).unwrap();
    for _ in 0..1025 {
        rt.update(|cx| cx.spawn_local(async {}, |_, _| {}).detach());
    }
    drain(&ex);
    let background = rt.update(|cx| cx.spawn(async {}, |_, _| {}));
    assert_eq!(rt.poll_tasks(), 1024);
    assert!(background.is_finished());
    assert_eq!(wakes.get(), 2);
    assert_eq!(rt.pending_tasks(), 2);
    assert_eq!(rt.poll_tasks(), 2);
}

#[test]
fn background_handle_stays_send_and_sync() {
    fn assert_traits<T: Send + Sync>() {}
    assert_traits::<Task>();
}

#[test]
fn local_only_configuration_is_independent_and_future_panics_are_delivered() {
    let (mut rt, ex, _) = setup();
    assert_eq!(
        rt.configure_local_tasks(ex.clone(), || {}),
        Err(SpawnError::AlreadyConfigured)
    );
    assert!(matches!(
        rt.update(|cx| cx.try_spawn(async {}, |_, _| {})),
        Err(SpawnError::Unavailable)
    ));
    let task = rt.update(|cx| {
        cx.spawn_local(
            async {
                panic!("local future panic");
            },
            |result: TaskResult<()>, _| {
                assert_eq!(
                    result,
                    Err(TaskError::Panicked("local future panic".into()))
                );
            },
        )
    });
    drain(&ex);
    assert_eq!(rt.poll_tasks(), 1);
    assert!(task.is_finished());
}

#[test]
fn replacing_local_handle_cancels_queued_result_and_releases_rc_output() {
    struct Model {
        task: Option<LocalTask>,
        value: i32,
    }
    let (mut rt, ex, _) = setup();
    let model = rt.update(|cx| {
        cx.new(|_| Model {
            task: None,
            value: 0,
        })
    });
    let first = Rc::new(1);
    let weak = Rc::downgrade(&first);
    rt.update(|cx| {
        model.update(cx, |state, cx| {
            state.task = Some(cx.spawn_local(async move { first }, |state, result, _| {
                state.value = *result.unwrap()
            }));
        })
    });
    drain(&ex);
    rt.update(|cx| {
        model.update(cx, |state, cx| {
            state.task = Some(cx.spawn_local(async { Rc::new(2) }, |state, result, _| {
                state.value = *result.unwrap()
            }));
        })
    });
    drain(&ex);
    assert_eq!(rt.poll_tasks(), 1);
    assert!(weak.upgrade().is_none());
    assert_eq!(rt.update(|cx| model.read(cx).value), 2);
}

#[test]
fn disposal_aborts_detached_pending_local_tasks_and_wakes_retained_ticker() {
    let (mut rt, ex, _) = setup();
    let model = rt.update(|cx| cx.new(|_| ()));
    let value = Rc::new(());
    let weak = Rc::downgrade(&value);
    rt.update(|cx| {
        model.update(cx, |_, cx| {
            cx.spawn_local(
                async move {
                    std::future::pending::<()>().await;
                    drop(value);
                },
                |_, _, _| panic!("disposed"),
            )
            .detach();
        })
    });
    assert!(ex.0.try_tick());
    let notice = Arc::new(Notice::default());
    let waker = Waker::from(notice.clone());
    let mut cx = std::task::Context::from_waker(&waker);
    let mut tick = Box::pin(ex.0.tick());
    assert!(tick.as_mut().poll(&mut cx).is_pending());
    drop(model);
    rt.synchronize();
    assert_eq!(rt.pending_tasks(), 0);
    assert!(notice.0.load(Ordering::SeqCst) > 0);
    assert!(tick.as_mut().poll(&mut cx).is_ready());
    assert!(weak.upgrade().is_none());
    assert_eq!(rt.poll_tasks(), 0);
    let value = Rc::new(());
    let weak = Rc::downgrade(&value);
    rt.update(|cx| {
        cx.spawn_local(
            async move {
                std::future::pending::<()>().await;
                drop(value);
            },
            |_, _| panic!("dead runtime"),
        )
        .detach()
    });
    drain(&ex);
    drop(rt);
    drain(&ex);
    assert!(weak.upgrade().is_none());
}

#[test]
fn separate_mount_scopes_cancel_only_disposed_placement() {
    let (mut rt, ex, _) = setup();
    let model = rt.update(|cx| cx.new(|_| ()));
    let a = rt.update(|cx| cx.mount(&model).unwrap());
    let b = rt.update(|cx| cx.mount(&model).unwrap());
    let seen = Rc::new(Cell::new(0));
    rt.update(|cx| {
        for mount in [&a, &b] {
            let out = seen.clone();
            mount
                .task_scope()
                .spawn_local(cx, async { Rc::new(1) }, move |r, _| {
                    out.set(out.get() + *r.unwrap())
                })
                .unwrap()
                .detach();
        }
    });
    drain(&ex);
    drop(a);
    assert_eq!(rt.poll_tasks(), 1);
    assert_eq!(seen.get(), 1);
}

#[test]
fn callback_panic_is_preserved_even_if_replacement_wake_panics() {
    let mut rt = Runtime::new();
    let ex = Rc::new(Executor::default());
    let wakes = Rc::new(Cell::new(0));
    let count = wakes.clone();
    rt.configure_local_tasks(ex.clone(), move || {
        count.set(count.get() + 1);
        assert_eq!(count.get(), 1, "host wake panic");
    })
    .unwrap();
    rt.update(|cx| {
        cx.spawn_local(async {}, |_, _| panic!("original panic"))
            .detach()
    });
    drain(&ex);
    rt.update(|cx| cx.spawn_local(async {}, |_, _| {}).detach());
    drain(&ex);
    let panic =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rt.poll_tasks())).unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"original panic"));
    assert_eq!(wakes.get(), 2);
    assert_eq!(rt.poll_tasks(), 1);
}

#[test]
fn callback_unwind_rearms_both_queues_even_if_background_notification_panics() {
    struct Immediate;
    impl TaskExecutor for Immediate {
        fn spawn(&self, future: BackgroundFuture) -> Result<(), SpawnError> {
            futures_lite::future::block_on(future);
            Ok(())
        }
    }
    let (mut rt, ex, local_wakes) = setup();
    let fail_wake = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = fail_wake.clone();
    rt.configure_tasks(Arc::new(Immediate), move || {
        assert!(!flag.load(Ordering::SeqCst), "background wake failure");
    })
    .unwrap();
    let delivered = Rc::new(Cell::new(0));
    let local_seen = delivered.clone();
    let background_seen = delivered.clone();
    rt.update(|cx| {
        cx.spawn_local(async {}, |_, _| panic!("completion failure"))
            .detach();
        cx.spawn_local(async {}, move |_, _| local_seen.set(local_seen.get() + 1))
            .detach();
        cx.spawn(async {}, move |_, _| {
            background_seen.set(background_seen.get() + 1)
        })
        .detach();
    });
    drain(&ex);
    assert_eq!(local_wakes.get(), 1);
    fail_wake.store(true, Ordering::SeqCst);
    let panic =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rt.poll_tasks())).unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"completion failure"));
    assert_eq!(
        local_wakes.get(),
        2,
        "local queue needs its own replacement wake"
    );
    assert_eq!(rt.pending_tasks(), 2);
    fail_wake.store(false, Ordering::SeqCst);
    assert_eq!(rt.poll_tasks(), 2);
    assert_eq!(delivered.get(), 2);
}
