use crate::*;
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

fn setup<T: 'static>(value: T) -> (Runtime, Entity<T>, Mount<T>) {
    let mut runtime = Runtime::new();
    let (entity, mount) = runtime.update(|cx| {
        let entity = cx.new(|_| value);
        let mount = cx.mount(&entity).unwrap();
        (entity, mount)
    });
    (runtime, entity, mount)
}

#[test]
fn immediate_mutation_returns_data_and_coalesces_dirty_mounts() {
    let (mut runtime, count, mount) = setup(0_u32);
    runtime.evaluate(&mount, |_, _| ()).unwrap();
    let result = runtime.update(|cx| {
        count.update(cx, |count, _| {
            *count += 1;
            *count += 1;
            *count
        })
    });
    assert_eq!(result, 2);
    assert_eq!(runtime.revision(&count), Ok(1));
    runtime.update(|cx| count.update(cx, |count, _| *count += 1));
    assert_eq!(runtime.dirty_mounts(), vec![mount.id()]);
    assert_eq!(runtime.evaluate(&mount, |count, _| *count), Ok(3));
    assert!(!runtime.is_dirty(&mount).unwrap());
}

#[test]
fn listener_reads_current_fields_even_when_description_is_old() {
    struct Counter {
        value: i32,
        step: i32,
        history: Vec<i32>,
    }
    let (mut runtime, counter, mount) = setup(Counter {
        value: 0,
        step: 1,
        history: vec![],
    });
    let listener = runtime
        .evaluate(&mount, |_, cx| {
            cx.listener(|state, event: &i32, _| {
                state.value += state.step * event;
                state.history.push(state.value);
            })
        })
        .unwrap();
    runtime.update(|cx| {
        counter.update(cx, |state, _| {
            state.value = 40;
            state.step = 2;
        })
    });
    assert_eq!(
        runtime.update(|cx| listener.dispatch(&3, cx)),
        Ok(Dispatch::Handled)
    );
    runtime.update(|cx| {
        let state = counter.read(cx);
        assert_eq!(state.value, 46);
        assert_eq!(state.history, vec![46]);
    });
}

#[test]
fn listeners_do_not_retain_owner_or_mount_and_do_not_bind_replacement() {
    let (mut runtime, count, mount) = setup(0_u32);
    let weak = count.downgrade();
    let listener = runtime
        .evaluate(&mount, |_, cx| cx.listener(|state, _: &(), _| *state += 1))
        .unwrap();
    let old_mount = mount.id();
    drop(mount);
    assert_eq!(
        runtime.update(|cx| listener.dispatch(&(), cx)),
        Ok(Dispatch::TargetGone)
    );
    let replacement = runtime.update(|cx| cx.mount(&count).unwrap());
    assert_ne!(replacement.id(), old_mount);
    assert_eq!(
        runtime.update(|cx| listener.dispatch(&(), cx)),
        Ok(Dispatch::TargetGone)
    );
    assert_eq!(runtime.evaluate(&replacement, |count, _| *count), Ok(0));
    drop(replacement);
    drop(count);
    runtime.synchronize();
    assert!(weak.upgrade().is_none());
}

#[test]
fn mount_clone_retains_placement_until_its_last_handle_drops() {
    let (mut runtime, count, mount) = setup(0_u32);
    let listener = runtime
        .evaluate(&mount, |_, cx| cx.listener(|state, _: &(), _| *state += 1))
        .unwrap();
    let clone = mount.clone();
    drop(mount);
    assert_eq!(
        runtime.update(|cx| listener.dispatch(&(), cx)),
        Ok(Dispatch::Handled)
    );
    drop(clone);
    assert_eq!(
        runtime.update(|cx| listener.dispatch(&(), cx)),
        Ok(Dispatch::TargetGone)
    );
    assert_eq!(runtime.update(|cx| *count.read(cx)), 1);
}

#[test]
fn generation_reuse_never_upgrades_an_old_weak_handle() {
    let mut runtime = Runtime::new();
    let old = runtime.update(|cx| cx.new(|_| 17));
    let id = old.id();
    let weak = old.downgrade();
    drop(old);
    let new = runtime.update(|cx| cx.new(|_| "replacement"));
    assert_eq!(id.slot, new.id().slot);
    assert_ne!(id.generation, new.id().generation);
    assert!(weak.upgrade().is_none());
    assert_eq!(
        runtime.update(|cx| weak.update(cx, |state, _| *state += 1)),
        Err(AccessError::Disposed)
    );
}

#[test]
fn wrong_runtime_is_detected_for_reads_updates_mounts_and_listeners() {
    let (mut first, entity, mount) = setup(1_u32);
    let mut second = Runtime::new();
    let listener = first
        .evaluate(&mount, |_, cx| cx.listener(|state, _: &(), _| *state += 1))
        .unwrap();
    second.update(|cx| {
        assert!(matches!(
            entity.try_read(cx),
            Err(AccessError::WrongRuntime)
        ));
        assert_eq!(
            entity.try_update(cx, |_, _| ()),
            Err(AccessError::WrongRuntime)
        );
        assert!(matches!(cx.mount(&entity), Err(AccessError::WrongRuntime)));
        assert_eq!(listener.dispatch(&(), cx), Err(AccessError::WrongRuntime));
    });
    assert_eq!(second.is_dirty(&mount), Err(AccessError::WrongRuntime));
    assert_eq!(
        second.evaluate(&mount, |_, _| ()),
        Err(AccessError::WrongRuntime)
    );
    assert_eq!(first.update(|cx| *entity.read(cx)), 1);
}

#[test]
fn current_owner_is_leased_but_different_entity_updates_work() {
    let (mut runtime, owner, _) = setup(0_u32);
    let other = runtime.update(|cx| cx.new(|_| 0_u32));
    runtime.update(|cx| {
        owner.update(cx, |state, cx| {
            assert!(matches!(owner.try_read(cx), Err(AccessError::Borrowed)));
            assert_eq!(owner.try_update(cx, |_, _| ()), Err(AccessError::Borrowed));
            assert_eq!(
                cx.entity().update(cx, |_, _| ()),
                Err(AccessError::Borrowed)
            );
            other.update(cx, |state, _| *state += 4);
            *state += 3;
        })
    });
    assert_eq!(
        runtime.update(|cx| (*owner.read(cx), *other.read(cx))),
        (3, 4)
    );
}

#[test]
fn constructor_cannot_reenter_uninitialized_owner() {
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| {
        cx.new(|cx: &mut Context<'_, u32>| {
            let own = cx.entity().upgrade().unwrap();
            assert!(matches!(own.try_read(cx), Err(AccessError::Borrowed)));
            assert_eq!(own.try_update(cx, |_, _| ()), Err(AccessError::Borrowed));
            8
        })
    });
    assert_eq!(runtime.update(|cx| *entity.read(cx)), 8);
}

#[test]
fn failed_constructor_invalidates_escaped_handles_and_releases_slot() {
    let mut runtime = Runtime::new();
    let escaped = Rc::new(std::cell::RefCell::new(None));
    let panic = catch_unwind(AssertUnwindSafe(|| {
        runtime.update(|cx| {
            cx.new(|cx: &mut Context<'_, u32>| {
                *escaped.borrow_mut() = cx.entity().upgrade();
                panic!("constructor failed")
            })
        })
    }));
    assert!(panic.is_err());
    let failed = escaped.borrow_mut().take().unwrap();
    let weak = failed.downgrade();
    assert!(weak.upgrade().is_none());
    runtime.update(|cx| assert!(matches!(failed.try_read(cx), Err(AccessError::Disposed))));
    let replacement = runtime.update(|cx| cx.new(|_| 42));
    assert_eq!(failed.id().slot, replacement.id().slot);
    assert_ne!(failed.id().generation, replacement.id().generation);
    drop(failed);
    runtime.synchronize();
    assert_eq!(runtime.update(|cx| *replacement.read(cx)), 42);
}

#[test]
fn panic_restores_mutated_value_marks_dirty_and_releases_borrow() {
    let (mut runtime, entity, mount) = setup(0_u32);
    runtime.evaluate(&mount, |_, _| ()).unwrap();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        runtime.update(|cx| {
            entity.update(cx, |state, _| {
                *state = 9;
                panic!("application panic")
            })
        })
    }));
    assert!(panic.is_err());
    assert!(runtime.is_dirty(&mount).unwrap());
    assert_eq!(runtime.update(|cx| *entity.read(cx)), 9);
    runtime.update(|cx| entity.update(cx, |state, _| *state += 1));
    assert_eq!(runtime.evaluate(&mount, |state, _| *state), Ok(10));
}

#[test]
fn application_error_return_is_not_rollback() {
    let (mut runtime, entity, _) = setup(0_u32);
    let result = runtime.update(|cx| {
        entity.update(cx, |state, _| {
            *state = 7;
            Err::<(), _>("failed later")
        })
    });
    assert_eq!(result, Err("failed later"));
    assert_eq!(runtime.update(|cx| *entity.read(cx)), 7);
}

#[test]
fn shared_model_invalidates_only_dependent_mounts_and_replaces_read_sets() {
    let mut runtime = Runtime::new();
    let (a, b, root, other, first, second, unrelated) = runtime.update(|cx| {
        let a = cx.new(|_| 10);
        let b = cx.new(|_| 20);
        let root = cx.new(|_| ());
        let other = cx.new(|_| ());
        let first = cx.mount(&root).unwrap();
        let second = cx.mount(&root).unwrap();
        let unrelated = cx.mount(&other).unwrap();
        (a, b, root, other, first, second, unrelated)
    });
    assert_ne!(first.id(), second.id());
    runtime.evaluate(&first, |_, cx| *a.read(cx)).unwrap();
    runtime.evaluate(&second, |_, cx| *a.read(cx)).unwrap();
    runtime.evaluate(&unrelated, |_, _| ()).unwrap();
    runtime.update(|cx| a.update(cx, |a, _| *a += 1));
    assert!(runtime.is_dirty(&first).unwrap());
    assert!(runtime.is_dirty(&second).unwrap());
    assert!(!runtime.is_dirty(&unrelated).unwrap());
    runtime.evaluate(&first, |_, cx| *b.read(cx)).unwrap();
    runtime.evaluate(&second, |_, cx| *a.read(cx)).unwrap();
    runtime.update(|cx| a.update(cx, |a, _| *a += 1));
    assert!(!runtime.is_dirty(&first).unwrap());
    assert!(runtime.is_dirty(&second).unwrap());
    runtime.update(|cx| b.update(cx, |b, _| *b += 1));
    assert!(runtime.is_dirty(&first).unwrap());
    drop((root, other));
}

#[test]
fn reading_in_update_does_not_subscribe_a_mount() {
    let (mut runtime, owner, mount) = setup(0_u32);
    let model = runtime.update(|cx| cx.new(|_| 8));
    runtime.update(|cx| owner.update(cx, |owner, cx| *owner = *model.read(cx)));
    runtime.evaluate(&mount, |owner, _| *owner).unwrap();
    runtime.update(|cx| model.update(cx, |model, _| *model += 1));
    assert!(!runtime.is_dirty(&mount).unwrap());
}

#[test]
fn mounting_a_child_does_not_implicitly_subscribe_parent() {
    let (mut runtime, parent, parent_mount) = setup(0);
    let (child, child_mount) = runtime.update(|cx| {
        let e = cx.new(|_| 1);
        let m = cx.mount(&e).unwrap();
        (e, m)
    });
    runtime
        .evaluate(&parent_mount, |_, _| child_mount.id())
        .unwrap();
    runtime.evaluate(&child_mount, |_, _| ()).unwrap();
    runtime.update(|cx| child.update(cx, |state, _| *state += 1));
    assert!(!runtime.is_dirty(&parent_mount).unwrap());
    assert!(runtime.is_dirty(&child_mount).unwrap());
    drop(parent);
}

#[test]
fn failed_evaluation_preserves_dependencies_and_can_retry() {
    let (mut runtime, _, mount) = setup(());
    let (a, b) = runtime.update(|cx| (cx.new(|_| 1), cx.new(|_| 2)));
    runtime.evaluate(&mount, |_, cx| *a.read(cx)).unwrap();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        runtime.evaluate(&mount, |_, cx| {
            let _ = *b.read(cx);
            panic!("build failed")
        })
    }));
    assert!(panic.is_err());
    assert!(runtime.is_dirty(&mount).unwrap());
    runtime.evaluate(&mount, |_, cx| *a.read(cx)).unwrap();
    runtime.update(|cx| b.update(cx, |state, _| *state += 1));
    assert!(!runtime.is_dirty(&mount).unwrap());
    runtime.update(|cx| a.update(cx, |state, _| *state += 1));
    assert!(runtime.is_dirty(&mount).unwrap());
}

#[test]
fn disposed_dependency_invalidates_then_reevaluates_as_absent() {
    let mut runtime = Runtime::new();
    let model = runtime.update(|cx| cx.new(|_| 7));
    let weak = model.downgrade();
    let root = runtime.update(|cx| cx.new(|_| weak));
    let mount = runtime.update(|cx| cx.mount(&root).unwrap());
    assert_eq!(
        runtime.evaluate(&mount, |weak, cx| weak.upgrade().map(|e| *e.read(cx))),
        Ok(Some(7))
    );
    drop(model);
    runtime.synchronize();
    assert!(runtime.is_dirty(&mount).unwrap());
    assert_eq!(
        runtime.evaluate(&mount, |weak, cx| weak.upgrade().map(|e| *e.read(cx))),
        Ok(None)
    );
}

#[test]
fn observers_are_deferred_coalesced_and_see_final_state() {
    let (mut runtime, model, _) = setup(0_u32);
    let seen = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let subscription = runtime.update(|cx| {
        cx.observe(&model, {
            let seen = seen.clone();
            let calls = calls.clone();
            move |model, cx| {
                seen.set(*model.read(cx));
                calls.set(calls.get() + 1)
            }
        })
        .unwrap()
    });
    runtime.update(|cx| {
        model.update(cx, |state, _| *state = 1);
        model.update(cx, |state, _| *state = 2);
        assert_eq!(calls.get(), 0);
    });
    runtime.flush().unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(seen.get(), 2);
    drop(subscription);
    runtime.update(|cx| model.update(cx, |state, _| *state = 3));
    runtime.flush().unwrap();
    assert_eq!(calls.get(), 1);
}

#[test]
fn nested_updates_release_all_owners_before_observers_execute() {
    let (mut runtime, owner, _) = setup(0_u32);
    let source = runtime.update(|cx| cx.new(|_| 0_u32));
    let subscription = runtime.update(|cx| {
        cx.observe(&source, {
            let owner = owner.clone();
            move |_, cx| owner.update(cx, |state, _| *state += 10)
        })
        .unwrap()
    });
    runtime.update(|cx| {
        owner.update(cx, |state, cx| {
            source.update(cx, |state, _| *state += 1);
            *state += 1;
        })
    });
    assert_eq!(runtime.update(|cx| *owner.read(cx)), 1);
    runtime.flush().unwrap();
    assert_eq!(runtime.update(|cx| *owner.read(cx)), 11);
    drop(subscription);
}

#[test]
fn typed_observer_weakly_binds_owner_and_disposes_with_it() {
    struct Owner {
        seen: u32,
        _subscription: Subscription,
    }
    let mut runtime = Runtime::new();
    let source = runtime.update(|cx| cx.new(|_| 0_u32));
    let owner = runtime.update(|cx| {
        cx.new(|cx: &mut Context<'_, Owner>| Owner {
            seen: 0,
            _subscription: cx
                .observe(&source, |owner, source, cx| owner.seen = *source.read(cx))
                .unwrap(),
        })
    });
    runtime.update(|cx| source.update(cx, |source, _| *source = 4));
    runtime.flush().unwrap();
    assert_eq!(runtime.update(|cx| owner.read(cx).seen), 4);
    let weak = owner.downgrade();
    drop(owner);
    assert!(weak.upgrade().is_none());
    runtime.update(|cx| source.update(cx, |source, _| *source = 5));
    runtime.flush().unwrap();
}

#[test]
fn dropping_subscription_during_flush_cancels_later_snapshot_callback() {
    let (mut runtime, model, _) = setup(0_u32);
    let calls = Rc::new(Cell::new(0));
    let holder = Rc::new(std::cell::RefCell::new(None));
    let first = runtime.update(|cx| {
        cx.observe(&model, {
            let holder = holder.clone();
            move |_, _| {
                holder.borrow_mut().take();
            }
        })
        .unwrap()
    });
    let second = runtime.update(|cx| {
        cx.observe(&model, {
            let calls = calls.clone();
            move |_, _| calls.set(calls.get() + 1)
        })
        .unwrap()
    });
    *holder.borrow_mut() = Some(second);
    runtime.update(|cx| model.update(cx, |state, _| *state += 1));
    runtime.flush().unwrap();
    assert_eq!(calls.get(), 0);
    drop(first);
}

#[test]
fn cycle_budget_preserves_remaining_work_and_clear_does_not_rollback() {
    let (mut runtime, model, mount) = setup(0_u32);
    let subscription = runtime.update(|cx| {
        cx.observe(&model, |model, cx| model.update(cx, |state, _| *state += 1))
            .unwrap()
    });
    runtime.update(|cx| model.update(cx, |state, _| *state = 1));
    assert_eq!(runtime.flush_with_limit(3), Err(EffectCycle));
    assert_eq!(runtime.update(|cx| *model.read(cx)), 4);
    assert!(runtime.is_dirty(&mount).unwrap());
    drop(subscription);
    runtime.clear_effects();
    runtime.flush().unwrap();
    assert_eq!(runtime.update(|cx| *model.read(cx)), 4);
}

#[test]
fn budget_retry_does_not_discard_unexecuted_observers() {
    let (mut runtime, model, _) = setup(0);
    let first_calls = Rc::new(Cell::new(0));
    let second_calls = Rc::new(Cell::new(0));
    let first = runtime.update(|cx| {
        cx.observe(&model, {
            let calls = first_calls.clone();
            move |_, _| calls.set(calls.get() + 1)
        })
        .unwrap()
    });
    let second = runtime.update(|cx| {
        cx.observe(&model, {
            let calls = second_calls.clone();
            move |_, _| calls.set(calls.get() + 1)
        })
        .unwrap()
    });
    runtime.update(|cx| model.update(cx, |state, _| *state += 1));
    assert_eq!(runtime.flush_with_limit(1), Err(EffectCycle));
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 0);
    runtime.flush_with_limit(1).unwrap();
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 1);
    drop((first, second));
}

#[test]
fn deferred_effects_run_after_access_and_can_enqueue_more_work() {
    let (mut runtime, model, _) = setup(0_u32);
    let model2 = model.clone();
    runtime.update(|cx| {
        model.update(cx, |state, cx| {
            *state = 1;
            cx.defer(move |cx| {
                model2.update(cx, |state, _| *state += 2);
                cx.defer(|_| ());
            });
        })
    });
    assert_eq!(runtime.update(|cx| *model.read(cx)), 1);
    runtime.flush().unwrap();
    assert_eq!(runtime.update(|cx| *model.read(cx)), 3);
}

#[test]
fn panic_in_effect_restores_flush_state_and_keeps_later_effects() {
    let mut runtime = Runtime::new();
    let calls = Rc::new(Cell::new(0));
    runtime.update(|cx| {
        cx.defer(|_| panic!("effect failed"));
        cx.defer({
            let calls = calls.clone();
            move |_| calls.set(calls.get() + 1)
        });
    });
    assert!(catch_unwind(AssertUnwindSafe(|| runtime.flush())).is_err());
    runtime.flush().unwrap();
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_runtime_drop_prevents_weak_resurrection_even_with_external_strong_handle() {
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| cx.new(|_| 12));
    let weak = entity.downgrade();
    drop(runtime);
    assert!(weak.upgrade().is_none());
    drop(entity);
}

#[test]
fn many_mounts_and_dependency_changes_leave_no_stale_dirty_ids() {
    let mut runtime = Runtime::new();
    let model = runtime.update(|cx| cx.new(|_| 0));
    let owner = runtime.update(|cx| cx.new(|_| ()));
    for _ in 0..50 {
        let mounts: Vec<_> =
            runtime.update(|cx| (0..32).map(|_| cx.mount(&owner).unwrap()).collect());
        for mount in &mounts {
            runtime.evaluate(mount, |_, cx| *model.read(cx)).unwrap();
        }
        runtime.update(|cx| model.update(cx, |state, _| *state += 1));
        assert_eq!(runtime.dirty_mounts().len(), 32);
        drop(mounts);
        runtime.synchronize();
        assert!(runtime.dirty_mounts().is_empty());
    }
}
