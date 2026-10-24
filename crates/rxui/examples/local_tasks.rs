//! A sleeping native host: no frame loop, busy polling, or background task pool.
//! Run: cargo run -p rxui --example local_tasks --no-default-features --features tasks
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use rxui::{LocalFuture, LocalTaskExecutor, Runtime, SpawnError};
    use std::{rc::Rc, time::Duration};
    #[derive(Default)]
    struct Adapter(async_executor::LocalExecutor<'static>);
    impl LocalTaskExecutor for Adapter {
        fn spawn_local(&self, future: LocalFuture) -> Result<(), SpawnError> {
            self.0.spawn(future).detach();
            Ok(())
        }
    }
    let executor = Rc::new(Adapter::default());
    let (wake, events) = async_channel::unbounded();
    let mut runtime = Runtime::new();
    runtime
        .configure_local_tasks(executor.clone(), move || {
            let _ = wake.try_send(()); // Schedule a host turn; never poll UI inline.
        })
        .unwrap();
    let model = runtime.update(|cx| cx.new(|_| None::<Rc<String>>));
    runtime.update(|cx| {
        model.update(cx, |_, cx| {
            let value = Rc::new(String::from("local result after idle await"));
            cx.spawn_local(
                async move {
                    rxui::sleep(Duration::from_millis(20)).await;
                    value
                },
                |state, result, _| *state = Some(result.unwrap()),
            )
            .detach();
        })
    });
    // LocalExecutor::run retains an executor waker while the host sleeps on its
    // event queue. try_tick alone does not arrange an idle-host wake.
    futures_lite::future::block_on(executor.0.run(async {
        events.recv().await.unwrap();
        assert_eq!(runtime.update(|cx| model.read(cx).clone()), None);
        assert_eq!(runtime.poll_tasks(), 1);
        println!(
            "{}",
            runtime.update(|cx| model.read(cx).as_ref().unwrap().clone())
        );
    }));
}
#[cfg(target_arch = "wasm32")]
fn main() {}
