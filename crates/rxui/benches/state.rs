//! Headless state operations; no layout, GPU work or native calls are timed.
//! CSV stdout. Batches report per-operation means, then median/p95 across batches.
use rxui::Runtime;
use std::{cell::Cell, hint::black_box, rc::Rc, time::Instant};
const SAMPLES: usize = 40;
const ITERATIONS: u32 = 10_000;
fn measure(name: &str, mounts: usize, mut operation: impl FnMut()) {
    for _ in 0..1000 {
        operation();
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            operation();
        }
        samples.push(start.elapsed().as_secs_f64() * 1e9 / f64::from(ITERATIONS));
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "{name},{mounts},{SAMPLES},{ITERATIONS},{:.3},{:.3}",
        samples[SAMPLES / 2],
        samples[(SAMPLES * 95).div_ceil(100) - 1]
    );
}
fn main() {
    println!("operation,mounts,samples,iterations_per_sample,median_ns,p95_ns");
    let mut runtime = Runtime::new();
    let model = runtime.update(|cx| cx.new(|_| 0_u32));
    measure("entity_update", 0, || {
        runtime.update(|cx| model.update(cx, |state, _| *state = black_box(*state).wrapping_add(1)))
    });

    for count in [1, 16, 256] {
        let mut runtime = Runtime::new();
        let (model, mounts) = runtime.update(|cx| {
            let model = cx.new(|_| 0_u32);
            let mounts: Vec<_> = (0..count).map(|_| cx.mount(&model).unwrap()).collect();
            (model, mounts)
        });
        for mount in &mounts {
            runtime.evaluate(mount, |state, _| *state).unwrap();
        }
        measure("update_invalidate", count, || {
            runtime.update(|cx| {
                model.update(cx, |state, _| *state = black_box(*state).wrapping_add(1))
            })
        });
        assert_eq!(runtime.dirty_mounts().len(), count);
    }

    let mut runtime = Runtime::new();
    let (model, mount) = runtime.update(|cx| {
        let model = cx.new(|_| 0_u32);
        let mount = cx.mount(&model).unwrap();
        (model, mount)
    });
    let listener = runtime
        .evaluate(&mount, |_, cx| {
            cx.listener(|state, _: &(), _| *state = black_box(*state).wrapping_add(1))
        })
        .unwrap();
    measure("bound_listener", 1, || {
        black_box(runtime.update(|cx| listener.dispatch(&(), cx)).unwrap());
    });
    assert_eq!(
        runtime.update(|cx| *model.read(cx)),
        SAMPLES as u32 * ITERATIONS + 1000
    );

    let mut runtime = Runtime::new();
    let (model, mount) = runtime.update(|cx| {
        let model = cx.new(|_| 0_u32);
        let owner = cx.new(|_| ());
        let mount = cx.mount(&owner).unwrap();
        (model, mount)
    });
    runtime.evaluate(&mount, |_, cx| *model.read(cx)).unwrap();
    measure("update_evaluate", 1, || {
        runtime
            .update(|cx| model.update(cx, |state, _| *state = black_box(*state).wrapping_add(1)));
        black_box(runtime.evaluate(&mount, |_, cx| *model.read(cx)).unwrap());
    });
    assert!(!runtime.is_dirty(&mount).unwrap());

    let mut runtime = Runtime::new();
    let seen = Rc::new(Cell::new(0_u32));
    let model = runtime.update(|cx| cx.new(|_| 0_u32));
    let subscription = runtime.update(|cx| {
        cx.observe(&model, {
            let seen = seen.clone();
            move |model, cx| seen.set(*model.read(cx))
        })
        .unwrap()
    });
    measure("update_observe_flush", 0, || {
        runtime
            .update(|cx| model.update(cx, |state, _| *state = black_box(*state).wrapping_add(1)));
        runtime.flush().unwrap();
    });
    assert_eq!(seen.get(), runtime.update(|cx| *model.read(cx)));
    drop(subscription);
    #[cfg(feature = "tasks")]
    dispose_task_owners();
}

/// Disposing entities cancels their owner-scoped tasks. Each operation creates
/// `owners` entities with one pending task each, then drops and synchronizes them.
#[cfg(feature = "tasks")]
fn dispose_task_owners() {
    use rxui::{BackgroundFuture, BlockingJob, SpawnError, TaskExecutor};
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Parked(Mutex<Vec<BackgroundFuture>>);
    impl TaskExecutor for Parked {
        fn spawn(&self, future: BackgroundFuture) -> Result<(), SpawnError> {
            self.0.lock().unwrap().push(future);
            Ok(())
        }
        fn spawn_blocking(&self, _: BlockingJob) -> Result<(), SpawnError> {
            Err(SpawnError::BlockingUnavailable)
        }
    }
    for owners in [16, 256, 1024] {
        let mut runtime = Runtime::new();
        let executor = Arc::new(Parked::default());
        runtime.configure_tasks(executor.clone(), || {}).unwrap();
        let mut samples = Vec::new();
        for sample in 0..23 {
            let start = Instant::now();
            for _ in 0..10 {
                let entities: Vec<_> = runtime.update(|cx| {
                    (0..owners)
                        .map(|_| {
                            cx.new(|cx: &mut rxui::Context<'_, Option<rxui::Task>>| {
                                Some(cx.spawn(std::future::pending::<()>(), |_, _, _| {}))
                            })
                        })
                        .collect()
                });
                drop(entities);
                runtime.synchronize();
                executor.0.lock().unwrap().clear();
            }
            // The first three samples warm up.
            if sample >= 3 {
                samples.push(start.elapsed().as_secs_f64() * 1e9 / 10.);
            }
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "dispose_task_owners,{owners},20,10,{:.3},{:.3}",
            samples[10], samples[18]
        );
    }
}
