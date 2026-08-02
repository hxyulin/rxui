use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use astrelis_core::geometry::LogicalSize;
use astrelis_text::FontDatabase;
use proptest::prelude::*;

use super::*;

fn app() -> App {
    App::new(LogicalSize::new(320.0, 240.0), FontDatabase::empty())
}

#[derive(Default)]
struct Source;

#[derive(Clone, Copy)]
struct Ping;

impl EventEmitter<Ping> for Source {}

#[derive(Default)]
struct Target {
    updates: usize,
}

#[test]
fn same_entity_update_during_update_panics_clearly_and_recovers() {
    let mut app = app();
    let entity = app.new_entity(|_| Target::default());
    let nested = entity.clone();

    let panic = catch_unwind(AssertUnwindSafe(|| {
        entity.update(&mut app, |_, cx| {
            cx.update(&nested, |_, _| {});
        });
    }))
    .expect_err("same-entity nested update must panic");
    let message = panic_message(panic);
    assert!(
        message.contains("already being updated")
            && message.contains(&format!("{:?}", entity.id())),
        "unexpected panic: {message}"
    );

    entity.update(&mut app, |target, _| target.updates += 1);
    assert_eq!(entity.read(&app).updates, 1);
}

#[test]
fn cross_entity_update_during_update_is_allowed() {
    let mut app = app();
    let first = app.new_entity(|_| Target::default());
    let second = app.new_entity(|_| Target::default());

    first.update(&mut app, |first, cx| {
        first.updates += 1;
        cx.update(&second, |second, _| second.updates += 1);
    });

    assert_eq!(first.read(&app).updates, 1);
    assert_eq!(second.read(&app).updates, 1);
}

#[test]
fn notify_and_emit_during_update_wait_until_the_borrow_ends() {
    let mut app = app();
    let source = app.new_entity(|_| Source);
    let delivered = Rc::new(Cell::new(0));
    let target = app.new_entity(|_| Target::default());
    let delivered_for_callback = delivered.clone();
    let _subscription = target.update(&mut app, |_, cx| {
        cx.subscribe(&source, move |target, _, _, _| {
            target.updates += 1;
            delivered_for_callback.set(delivered_for_callback.get() + 1);
        })
    });

    source.update(&mut app, |_, cx| {
        cx.notify();
        cx.emit(Ping);
        assert_eq!(delivered.get(), 0, "emit ran while source was borrowed");
    });

    assert_eq!(delivered.get(), 1);
    assert!(app.is_notified(source.id()));
    assert_eq!(app.pending_effect_count(), 0);
}

#[test]
fn emit_during_flush_joins_the_current_flush() {
    #[derive(Default)]
    struct Relay;
    impl EventEmitter<Ping> for Relay {}

    let mut app = app();
    let source = app.new_entity(|_| Source);
    let relay = app.new_entity(|_| Relay);
    let sink = app.new_entity(|_| Target::default());
    let source_deliveries = Rc::new(Cell::new(0));
    let sink_deliveries = Rc::new(Cell::new(0));

    let source_deliveries_callback = source_deliveries.clone();
    let relay_subscription = relay.update(&mut app, |_, cx| {
        cx.subscribe(&source, move |_, _, _, cx| {
            source_deliveries_callback.set(source_deliveries_callback.get() + 1);
            cx.emit(Ping);
        })
    });
    let sink_deliveries_callback = sink_deliveries.clone();
    let sink_subscription = sink.update(&mut app, |_, cx| {
        cx.subscribe(&relay, move |target, _, _, _| {
            target.updates += 1;
            sink_deliveries_callback.set(sink_deliveries_callback.get() + 1);
        })
    });

    source.update(&mut app, |_, cx| cx.emit(Ping));

    assert_eq!(source_deliveries.get(), 1);
    assert_eq!(sink_deliveries.get(), 1);
    assert_eq!(app.pending_effect_count(), 0);
    drop((relay_subscription, sink_subscription));
}

#[test]
fn dropped_entity_subscription_is_pruned_on_failed_upgrade() {
    let mut app = app();
    let source = app.new_entity(|_| Source);
    let target = app.new_entity(|_| Target::default());
    let deliveries = Rc::new(Cell::new(0));
    let deliveries_callback = deliveries.clone();
    let subscription = target.update(&mut app, |_, cx| {
        cx.subscribe(&source, move |_, _, _, _| {
            deliveries_callback.set(deliveries_callback.get() + 1);
        })
    });
    assert_eq!(app.subscription_count(), 1);

    drop(target);
    source.update(&mut app, |_, cx| cx.emit(Ping));

    assert_eq!(deliveries.get(), 0);
    assert_eq!(app.subscription_count(), 0);
    drop(subscription);
}

#[test]
fn dropped_subscription_stops_delivery() {
    let mut app = app();
    let source = app.new_entity(|_| Source);
    let target = app.new_entity(|_| Target::default());
    let subscription = target.update(&mut app, |_, cx| {
        cx.subscribe(&source, |target, _, _, _| target.updates += 1)
    });
    drop(subscription);

    source.update(&mut app, |_, cx| cx.emit(Ping));

    assert_eq!(target.read(&app).updates, 0);
    assert_eq!(app.subscription_count(), 0);
}

#[test]
fn routed_handler_updates_its_live_target_and_ignores_a_dropped_target() {
    let mut app = app();
    let target = app.new_entity(|_| Target::default());
    let handler = target.update(&mut app, |_, cx| {
        cx.listener(|target, (), cx| {
            target.updates += 3;
            cx.notify();
        })
    });
    assert!(app.dispatch(handler));
    assert_eq!(target.read(&app).updates, 3);

    let handler = target.update(&mut app, |_, cx| {
        cx.listener(|target, (), _| target.updates += 4)
    });
    let id = target.id();
    drop(target);
    assert!(!app.dispatch(handler));
    assert!(!app.contains_entity(id));
}

#[derive(Default)]
struct ProbeTarget {
    deliveries: Rc<Cell<usize>>,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn random_entity_effect_sequences_obey_runtime_properties(operations in prop::collection::vec(0_u8..10, 1..128)) {
        let mut app = app();
        let mut source: Option<Entity<Source>> = None;
        let mut target: Option<Entity<ProbeTarget>> = None;
        let mut subscriptions: Vec<Option<Subscription>> = Vec::new();
        let mut dropped_probes: Vec<(Rc<Cell<usize>>, usize)> = Vec::new();
        let mut defined_reentrancy_panics = 0;

        for operation in operations {
            let result = catch_unwind(AssertUnwindSafe(|| match operation {
                0 => source = Some(app.new_entity(|_| Source)),
                1 => target = Some(app.new_entity(|_| ProbeTarget::default())),
                2 => {
                    if let Some(target) = &target {
                        target.update(&mut app, |target, _| {
                            target.deliveries.set(target.deliveries.get() + 1);
                        });
                    }
                }
                3 => {
                    if let Some(target) = &target {
                        target.update(&mut app, |_, cx| cx.notify());
                    }
                }
                4 => {
                    if let Some(source) = &source {
                        source.update(&mut app, |_, cx| cx.emit(Ping));
                    }
                }
                5 => {
                    if let (Some(source), Some(target)) = (&source, &target) {
                        subscriptions.push(Some(target.update(&mut app, |_, cx| {
                            cx.subscribe(source, |target, _, _, _| {
                                target.deliveries.set(target.deliveries.get() + 1);
                            })
                        })));
                    }
                }
                6 => {
                    if let Some(target) = target.take() {
                        let probe = target.read(&app).deliveries.clone();
                        dropped_probes.push((probe.clone(), probe.get()));
                        drop(target);
                    }
                }
                7 => drop(source.take()),
                8 => {
                    if let Some(subscription) = subscriptions.iter_mut().find(|item| item.is_some()) {
                        drop(subscription.take());
                    }
                }
                9 => {
                    if let Some(target) = &target {
                        let nested = target.clone();
                        let panic = catch_unwind(AssertUnwindSafe(|| {
                            target.update(&mut app, |_, cx| cx.update(&nested, |_, _| {}));
                        }));
                        assert!(panic.is_err());
                        defined_reentrancy_panics += 1;
                    }
                }
                _ => unreachable!(),
            }));

            prop_assert!(result.is_ok(), "operation {operation} caused an unexpected panic");
            app.flush();
            prop_assert_eq!(app.pending_effect_count(), 0, "queued effect neither ran nor was pruned");
            for (probe, count_at_drop) in &dropped_probes {
                prop_assert_eq!(probe.get(), *count_at_drop, "a dropped entity received an effect");
            }
        }

        prop_assert!(defined_reentrancy_panics <= 127);
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_owned(),
            Err(_) => "non-string panic".to_owned(),
        },
    }
}
