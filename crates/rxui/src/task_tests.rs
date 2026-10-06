use crate::*;
use std::{
    cell::Cell,
    collections::VecDeque,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Default)]
struct Manual {
    futures: Mutex<VecDeque<BackgroundFuture>>,
    blocking: Mutex<VecDeque<BlockingJob>>,
}
impl TaskExecutor for Manual {
    fn spawn(&self, future: BackgroundFuture) -> Result<(), SpawnError> {
        self.futures.lock().unwrap().push_back(future);
        Ok(())
    }
    fn spawn_blocking(&self, job: BlockingJob) -> Result<(), SpawnError> {
        self.blocking.lock().unwrap().push_back(job);
        Ok(())
    }
}
impl Manual {
    fn tick(&self) {
        let Some(mut future) = self.futures.lock().unwrap().pop_front() else {
            return;
        };
        if futures_lite::future::block_on(futures_lite::future::poll_once(future.as_mut()))
            .is_none()
        {
            self.futures.lock().unwrap().push_back(future);
        }
    }
    fn blocking(&self) {
        let job = self.blocking.lock().unwrap().pop_front().unwrap();
        job();
    }
}
fn setup() -> (Runtime, Arc<Manual>, Arc<AtomicUsize>) {
    let mut runtime = Runtime::new();
    let executor = Arc::new(Manual::default());
    let wake = Arc::new(AtomicUsize::new(0));
    runtime
        .configure_tasks(executor.clone(), {
            let wake = wake.clone();
            move || {
                wake.fetch_add(1, Ordering::SeqCst);
            }
        })
        .unwrap();
    (runtime, executor, wake)
}
#[test]
fn completion_uses_live_owner_releases_access_and_defers_observers() {
    let (mut runtime, executor, _) = setup();
    let seen = Rc::new(Cell::new(0));
    let entity = runtime.update(|cx| cx.new(|_| 0_u32));
    let subscription = runtime
        .update(|cx| {
            cx.observe(&entity, {
                let seen = seen.clone();
                move |value, cx| seen.set(*value.read(cx))
            })
        })
        .unwrap();
    let task = runtime.update(|cx| {
        entity.update(cx, |_, cx| {
            cx.spawn(async { 7 }, |state, result, _| *state += result.unwrap())
        })
    });
    runtime.update(|cx| entity.update(cx, |state, _| *state = 10));
    executor.tick();
    assert_eq!(seen.get(), 0);
    assert!(!task.is_finished());
    assert_eq!(runtime.poll_tasks(), 1);
    assert_eq!(runtime.update(|cx| *entity.read(cx)), 17);
    assert_eq!(seen.get(), 0);
    runtime.flush().unwrap();
    assert_eq!(seen.get(), 17);
    assert!(task.is_finished());
    drop(subscription);
}
#[test]
fn drop_cancels_already_queued_output_and_detach_keeps_app_work() {
    let (mut runtime, executor, _) = setup();
    let seen = Rc::new(Cell::new(0));
    let task = runtime.update(|cx| {
        cx.spawn(async { 4 }, {
            let seen = seen.clone();
            move |result, _| seen.set(result.unwrap())
        })
    });
    executor.tick();
    drop(task);
    assert_eq!(runtime.poll_tasks(), 0);
    assert_eq!(seen.get(), 0);
    assert_eq!(runtime.pending_tasks(), 0);
    runtime.update(|cx| {
        cx.spawn(async { 9 }, {
            let seen = seen.clone();
            move |result, _| seen.set(result.unwrap())
        })
        .detach()
    });
    executor.tick();
    assert_eq!(runtime.poll_tasks(), 1);
    assert_eq!(seen.get(), 9);
}
#[test]
fn disposal_aborts_detached_owner_work_without_retaining_the_entity() {
    let (mut runtime, executor, _) = setup();
    let entity = runtime.update(|cx| cx.new(|_| 0_u32));
    let weak = entity.downgrade();
    runtime.update(|cx| {
        entity.update(cx, |_, cx| {
            cx.spawn(std::future::pending::<u32>(), |state, result, _| {
                *state = result.unwrap()
            })
            .detach()
        })
    });
    executor.tick();
    drop(entity);
    runtime.synchronize();
    assert!(weak.upgrade().is_none());
    assert_eq!(runtime.pending_tasks(), 0);
    executor.tick();
    assert_eq!(runtime.poll_tasks(), 0);
    assert!(executor.futures.lock().unwrap().is_empty());
}
#[test]
fn replacing_owned_task_suppresses_superseded_completion() {
    struct Model {
        task: Option<Task>,
        value: u32,
    }
    let (mut runtime, executor, _) = setup();
    let entity = runtime.update(|cx| {
        cx.new(|_| Model {
            task: None,
            value: 0,
        })
    });
    runtime.update(|cx| {
        entity.update(cx, |state, cx| {
            state.task = Some(cx.spawn(async { 1 }, |state, result, _| {
                state.value = result.unwrap()
            }))
        })
    });
    executor.tick();
    runtime.update(|cx| {
        entity.update(cx, |state, cx| {
            state.task = Some(cx.spawn(async { 2 }, |state, result, _| {
                state.value = result.unwrap()
            }))
        })
    });
    executor.tick();
    runtime.poll_tasks();
    assert_eq!(runtime.update(|cx| entity.read(cx).value), 2);
}
#[test]
fn future_and_blocking_panics_are_delivered_as_errors() {
    let (mut runtime, executor, _) = setup();
    let errors = Rc::new(Cell::new(0));
    let task = runtime.update(|cx| {
        cx.spawn(
            async {
                panic!("future panic");
            },
            {
                let errors = errors.clone();
                move |result: TaskResult<()>, _| {
                    assert_eq!(result, Err(TaskError::Panicked("future panic".into())));
                    errors.set(errors.get() + 1);
                }
            },
        )
    });
    executor.tick();
    runtime.poll_tasks();
    let blocking = runtime.update(|cx| {
        cx.spawn_blocking(|| panic!("blocking panic"), {
            let errors = errors.clone();
            move |result: TaskResult<()>, _| {
                assert_eq!(result, Err(TaskError::Panicked("blocking panic".into())));
                errors.set(errors.get() + 1);
            }
        })
    });
    executor.blocking();
    runtime.poll_tasks();
    assert_eq!(errors.get(), 2);
    assert!(task.is_finished() && blocking.is_finished());
}
#[test]
fn unconfigured_or_rejected_setup_is_fallible_and_releases_captures() {
    let mut runtime = Runtime::new();
    assert!(matches!(
        runtime.update(|cx| cx.try_spawn(async {}, |_, _| {})),
        Err(SpawnError::Unavailable)
    ));
    struct Reject;
    impl TaskExecutor for Reject {
        fn spawn(&self, _: BackgroundFuture) -> Result<(), SpawnError> {
            Err(SpawnError::Rejected("full".into()))
        }
    }
    runtime.configure_tasks(Arc::new(Reject), || {}).unwrap();
    assert_eq!(
        runtime.configure_tasks(Arc::new(Reject), || {}),
        Err(SpawnError::AlreadyConfigured)
    );
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    assert!(matches!(
        runtime.update(|cx| cx.try_spawn(async {}, move |_, _| drop(capture))),
        Err(SpawnError::Rejected(_))
    ));
    assert!(weak.upgrade().is_none());
    assert_eq!(runtime.pending_tasks(), 0);
    runtime.poll_tasks();
    assert!(matches!(
        runtime.update(|cx| cx.try_spawn_blocking(|| (), |_, _| {})),
        Err(SpawnError::BlockingUnavailable)
    ));
}
#[test]
fn executor_panic_releases_registration_and_callback_captures() {
    struct Panics;
    impl TaskExecutor for Panics {
        fn spawn(&self, _: BackgroundFuture) -> Result<(), SpawnError> {
            panic!("executor panic")
        }
    }
    let mut runtime = Runtime::new();
    runtime.configure_tasks(Arc::new(Panics), || {}).unwrap();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.update(|cx| cx.try_spawn(async {}, move |_, _| drop(capture)))
        }))
        .is_err()
    );
    assert!(weak.upgrade().is_none());
    assert_eq!(runtime.pending_tasks(), 0);
    assert_eq!(runtime.poll_tasks(), 0);
}
#[test]
fn completion_wakes_coalesce_and_budget_reschedules_remaining_results() {
    let (mut runtime, executor, wake) = setup();
    let seen = Rc::new(Cell::new(0));
    for _ in 0..1025 {
        runtime.update(|cx| {
            cx.spawn(async {}, {
                let seen = seen.clone();
                move |_, _| seen.set(seen.get() + 1)
            })
            .detach()
        });
        executor.tick();
    }
    assert_eq!(wake.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.poll_tasks(), 1024);
    assert_eq!(seen.get(), 1024);
    assert_eq!(wake.load(Ordering::SeqCst), 2);
    assert_eq!(runtime.poll_tasks(), 1);
    assert_eq!(seen.get(), 1025);
    assert_eq!(runtime.pending_tasks(), 0);
}
#[test]
fn callback_panic_preserves_later_results_and_runtime_remains_usable() {
    let (mut runtime, executor, _) = setup();
    let seen = Rc::new(Cell::new(false));
    runtime.update(|cx| cx.spawn(async {}, |_, _| panic!("callback panic")).detach());
    runtime.update(|cx| {
        cx.spawn(async {}, {
            let seen = seen.clone();
            move |_, _| seen.set(true)
        })
        .detach()
    });
    executor.tick();
    executor.tick();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.poll_tasks())).is_err()
    );
    assert_eq!(runtime.poll_tasks(), 1);
    assert!(seen.get());
}
#[test]
fn runtime_drop_cancels_external_handle_and_releases_ui_callback_captures() {
    let (mut runtime, executor, _) = setup();
    let value = Rc::new(());
    let weak = Rc::downgrade(&value);
    let task = runtime.update(|cx| cx.spawn(std::future::pending::<()>(), move |_, _| drop(value)));
    drop(runtime);
    assert!(task.is_finished());
    assert!(weak.upgrade().is_none());
    executor.tick();
    assert!(executor.futures.lock().unwrap().is_empty());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn default_pool_keeps_async_timer_progress_independent_of_blocking_work() {
    let executor = Arc::new(ThreadPoolExecutor::new(1, 1).unwrap());
    let mut runtime = Runtime::new();
    let (wake, ready) = std::sync::mpsc::channel();
    runtime
        .configure_tasks(executor, move || {
            let _ = wake.send(());
        })
        .unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let (started, start) = std::sync::mpsc::channel();
    let blocking = runtime.update(|cx| {
        cx.spawn_blocking(
            move || {
                started.send(()).unwrap();
                wait.recv().unwrap();
            },
            |result, _| result.unwrap(),
        )
    });
    start.recv_timeout(Duration::from_secs(2)).unwrap();
    let seen = Rc::new(Cell::new(false));
    let timer = runtime.update(|cx| {
        cx.spawn(
            async {
                sleep(Duration::from_millis(5)).await;
            },
            {
                let seen = seen.clone();
                move |result, _| {
                    result.unwrap();
                    seen.set(true);
                }
            },
        )
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    runtime.poll_tasks();
    assert!(seen.get());
    assert!(timer.is_finished());
    assert!(!blocking.is_finished());
    release.send(()).unwrap();
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    runtime.poll_tasks();
    assert!(blocking.is_finished());
}
