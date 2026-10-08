use crate::*;
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct Measure {
    calls: usize,
    generation: u64,
    fail: bool,
}
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, text: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        self.calls += 1;
        if self.fail {
            return Err(UiError::InvalidGeometry);
        }
        let intrinsic = text.text.chars().count() as f32 * text.font_size / 2.;
        let width = match text.width {
            TextWidth::Available(width) => intrinsic.min(width),
            TextWidth::MinContent => text.font_size / 2.,
            TextWidth::MaxContent => intrinsic,
        };
        let lines = if width > 0. {
            (intrinsic / width).ceil().max(1.)
        } else {
            1.
        };
        Ok([width, lines * text.font_size])
    }
    fn generation(&self) -> u64 {
        self.generation
    }
}
struct Counter {
    count: i32,
    items: Vec<i32>,
    disabled: bool,
    duplicate: bool,
    size: f32,
}
impl Default for Counter {
    fn default() -> Self {
        Self {
            count: 0,
            items: vec![1, 2, 3],
            disabled: false,
            duplicate: false,
            size: 100.,
        }
    }
}
impl View for Counter {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .gap(10.)
            .padding(20.)
            .children(self.items.iter().map(|item| {
                button(format!("Item {item}: {}", self.count))
                    .key(if self.duplicate { 1 } else { *item })
                    .width(self.size)
                    .height(40.)
                    .disabled(self.disabled)
                    .on_click(cx.listener(|this, _, _| this.count += 1))
            }))
    }
}
fn setup() -> (Runtime, Entity<Counter>, Ui<Counter>, Measure) {
    let mut runtime = Runtime::new();
    let counter = runtime.update(|cx| cx.new(|_| Counter::default()));
    let mut ui = Ui::new(&mut runtime, counter.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    (runtime, counter, ui, measure)
}
fn keyed<T: View>(ui: &Ui<T>, key: i32) -> ElementId {
    ui.elements()
        .find(|e| e.key == Some(&key.into()))
        .unwrap()
        .id
}
fn center<T: View>(ui: &Ui<T>, id: ElementId) -> [f32; 2] {
    let b = ui.element(id).unwrap().bounds;
    [b.x + b.width / 2., b.y + b.height / 2.]
}
#[test]
fn flex_geometry_and_idle_prepare_reuse_layout_and_descriptions() {
    let (mut runtime, _, mut ui, mut measure) = setup();
    let first = ui.element(keyed(&ui, 1)).unwrap().bounds;
    let second = ui.element(keyed(&ui, 2)).unwrap().bounds;
    assert_eq!(
        first,
        Bounds {
            x: 20.,
            y: 20.,
            width: 100.,
            height: 40.
        }
    );
    assert_eq!(second.y, 70.);
    let stats = ui.stats();
    let measurements = measure.calls;
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(ui.stats(), stats);
    assert_eq!(measure.calls, measurements);
}
#[test]
fn keyed_reorder_preserves_focus_capture_and_node_identity() {
    let (mut runtime, counter, mut ui, mut measure) = setup();
    let id = keyed(&ui, 2);
    let point = center(&ui, id);
    ui.pointer(&mut runtime, PointerEvent::Pressed(point))
        .unwrap();
    runtime.update(|cx| counter.update(cx, |this, _| this.items.reverse()));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(keyed(&ui, 2), id);
    assert!(ui.element(id).unwrap().focused);
    assert!(ui.element(id).unwrap().pressed);
    ui.pointer(&mut runtime, PointerEvent::Released(center(&ui, id)))
        .unwrap();
    assert_eq!(runtime.update(|cx| counter.read(cx).count), 1);
}
#[test]
fn removed_key_never_activates_replacement_and_clears_interaction() {
    let (mut runtime, counter, mut ui, mut measure) = setup();
    let old = keyed(&ui, 1);
    let point = center(&ui, old);
    ui.pointer(&mut runtime, PointerEvent::Pressed(point))
        .unwrap();
    runtime.update(|cx| counter.update(cx, |this, _| this.items = vec![4, 2, 3]));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert!(ui.element(old).is_none());
    assert_ne!(keyed(&ui, 4), old);
    ui.pointer(&mut runtime, PointerEvent::Released(point))
        .unwrap();
    assert_eq!(runtime.update(|cx| counter.read(cx).count), 0);
    assert!(!ui.elements().any(|e| e.focused || e.pressed));
}
#[test]
fn keyboard_and_pointer_use_same_activation_and_disabled_controls_are_skipped() {
    let (mut runtime, counter, mut ui, mut measure) = setup();
    assert!(ui.focus_next(false));
    assert!(ui.activate_focused(&mut runtime).unwrap());
    let id = keyed(&ui, 2);
    let point = center(&ui, id);
    ui.pointer(&mut runtime, PointerEvent::Pressed(point))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Released(point))
        .unwrap();
    assert_eq!(runtime.update(|cx| counter.read(cx).count), 2);
    runtime.update(|cx| counter.update(cx, |this, _| this.disabled = true));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert!(!ui.focus_next(false));
    assert!(!ui.activate_focused(&mut runtime).unwrap());
    assert!(ui.hit_test(point).is_none());
}
#[test]
fn releasing_outside_or_cancelling_capture_does_not_click() {
    let (mut runtime, counter, mut ui, _) = setup();
    let point = center(&ui, keyed(&ui, 1));
    ui.pointer(&mut runtime, PointerEvent::Pressed(point))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Released([350., 250.]))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Pressed(point))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Cancelled).unwrap();
    ui.pointer(&mut runtime, PointerEvent::Released(point))
        .unwrap();
    assert_eq!(runtime.update(|cx| counter.read(cx).count), 0);
}
#[test]
fn duplicate_keys_and_invalid_styles_preserve_dirty_retry_and_previous_nodes() {
    let (mut runtime, counter, mut ui, mut measure) = setup();
    let old = keyed(&ui, 2);
    let stats = ui.stats();
    runtime.update(|cx| counter.update(cx, |this, _| this.duplicate = true));
    assert!(matches!(
        ui.prepare(&mut runtime, [400., 300.], &mut measure),
        Err(UiError::DuplicateKey(_))
    ));
    assert_eq!(ui.stats(), stats);
    assert!(ui.elements().next().is_none());
    runtime.update(|cx| {
        counter.update(cx, |this, _| {
            this.duplicate = false;
            this.size = f32::NAN;
        })
    });
    assert!(matches!(
        ui.prepare(&mut runtime, [400., 300.], &mut measure),
        Err(UiError::InvalidStyle)
    ));
    runtime.update(|cx| counter.update(cx, |this, _| this.size = 100.));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(keyed(&ui, 2), old);
}

struct Child {
    model: Entity<u32>,
    evaluations: Rc<Cell<u32>>,
}
impl View for Child {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.evaluations.set(self.evaluations.get() + 1);
        label(format!("{}", *self.model.read(cx)))
    }
}
struct Parent {
    children: Vec<Entity<Child>>,
    evaluations: Rc<Cell<u32>>,
}
impl View for Parent {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.evaluations.set(self.evaluations.get() + 1);
        column().children(
            self.children
                .iter()
                .enumerate()
                .map(|(i, child)| child.clone().key(i)),
        )
    }
}
#[test]
fn model_change_evaluates_only_dependent_child_component() {
    let mut runtime = Runtime::new();
    let parent_evals = Rc::new(Cell::new(0));
    let child_evals = Rc::new(Cell::new(0));
    let (model, parent) = runtime.update(|cx| {
        let model = cx.new(|_| 0_u32);
        let child = cx.new(|_| Child {
            model: model.clone(),
            evaluations: child_evals.clone(),
        });
        let parent = cx.new(|_| Parent {
            children: vec![child],
            evaluations: parent_evals.clone(),
        });
        (model, parent)
    });
    let mut ui = Ui::new(&mut runtime, parent).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [300., 200.], &mut measure)
        .unwrap();
    runtime.update(|cx| model.update(cx, |value, _| *value += 1));
    ui.prepare(&mut runtime, [300., 200.], &mut measure)
        .unwrap();
    assert_eq!(parent_evals.get(), 1);
    assert_eq!(child_evals.get(), 2);
    assert!(ui.elements().any(|e| e.text == Some("1")));
}
#[test]
fn one_entity_in_two_uis_has_independent_focus_and_disposes_child_mounts() {
    let (mut runtime, counter, mut first, mut measure) = setup();
    let mut second = Ui::new(&mut runtime, counter).unwrap();
    second
        .prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    first.focus_next(false);
    assert!(first.elements().any(|e| e.focused));
    assert!(!second.elements().any(|e| e.focused));
    assert_ne!(keyed(&first, 1), keyed(&second, 1));
    drop(first);
    drop(second);
    runtime.synchronize();
    assert!(runtime.dirty_mounts().is_empty());
}
struct TextView {
    color: Color,
    size: f32,
}
impl View for TextView {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        label("Retained text")
            .font_size(self.size)
            .color(self.color)
    }
}
#[test]
fn color_handler_and_idle_changes_retain_measurement_but_font_generation_invalidates() {
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| {
        cx.new(|_| TextView {
            color: [1.; 4],
            size: 16.,
        })
    });
    let mut ui = Ui::new(&mut runtime, entity.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    let calls = measure.calls;
    runtime.update(|cx| entity.update(cx, |this, _| this.color = [0.5; 4]));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(measure.calls, calls);
    measure.generation += 1;
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert!(measure.calls > calls);
}
#[test]
fn failed_measurement_retries_without_a_state_update_and_rejects_stale_geometry() {
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| {
        cx.new(|_| TextView {
            color: [1.; 4],
            size: 16.,
        })
    });
    let mut ui = Ui::new(&mut runtime, entity).unwrap();
    let mut measure = Measure {
        fail: true,
        ..Measure::default()
    };
    assert!(
        ui.prepare(&mut runtime, [400., 300.], &mut measure)
            .is_err()
    );
    assert!(ui.hit_test([10., 10.]).is_none());
    assert!(ui.elements().next().is_none());
    measure.fail = false;
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(ui.elements().count(), 1);
}
#[test]
fn foreign_runtime_and_invalid_geometry_are_errors() {
    let (_, _, mut ui, mut measure) = setup();
    let mut foreign = Runtime::new();
    assert!(matches!(
        ui.prepare(&mut foreign, [400., 300.], &mut measure),
        Err(UiError::Access(AccessError::WrongRuntime))
    ));
    let (mut runtime, _, mut ui, mut measure) = setup();
    assert!(matches!(
        ui.prepare(&mut runtime, [f32::INFINITY, 300.], &mut measure),
        Err(UiError::InvalidGeometry)
    ));
    assert!(
        ui.pointer(&mut runtime, PointerEvent::Pressed([f32::NAN, 0.]))
            .is_err()
    );
}

#[test]
fn compatible_keys_are_scoped_to_parent_and_type_changes_reset_identity() {
    struct Choice {
        button: bool,
    }
    impl View for Choice {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column()
                .child(row().child(if self.button {
                    button("A").key(1)
                } else {
                    label("A").key(1)
                }))
                .child(row().child(label("B").key(1)))
        }
    }
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| cx.new(|_| Choice { button: true }));
    let mut ui = Ui::new(&mut runtime, entity.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    let first = ui.elements().find(|e| e.text == Some("A")).unwrap().id;
    let second = ui.elements().find(|e| e.text == Some("B")).unwrap().id;
    runtime.update(|cx| entity.update(cx, |this, _| this.button = false));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_ne!(
        ui.elements().find(|e| e.text == Some("A")).unwrap().id,
        first
    );
    assert_eq!(
        ui.elements().find(|e| e.text == Some("B")).unwrap().id,
        second
    );
}

#[test]
fn recursive_component_placement_is_diagnosed_without_recursing_forever() {
    struct Recursive {
        me: WeakEntity<Self>,
        recurse: bool,
    }
    impl View for Recursive {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            if self.recurse {
                self.me.upgrade().unwrap().into_element()
            } else {
                label("Recovered")
            }
        }
    }
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| {
        cx.new(|cx| Recursive {
            me: cx.entity(),
            recurse: true,
        })
    });
    let mut ui = Ui::new(&mut runtime, entity.clone()).unwrap();
    let mut measure = Measure::default();
    assert!(matches!(
        ui.prepare(&mut runtime, [400., 300.], &mut measure),
        Err(UiError::RecursiveComponent(_))
    ));
    runtime.update(|cx| entity.update(cx, |this, _| this.recurse = false));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert!(ui.elements().any(|e| e.text == Some("Recovered")));
}

#[test]
fn removing_child_mount_disables_old_listener_even_when_entity_is_retained() {
    use std::cell::RefCell;
    struct ChildButton {
        value: u32,
        saved: Rc<RefCell<Option<Listener<ClickEvent>>>>,
    }
    impl View for ChildButton {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let listener = cx.listener(|this, _, _| this.value += 1);
            *self.saved.borrow_mut() = Some(listener.clone());
            button("Child").on_click(listener)
        }
    }
    struct Holder {
        child: Option<Entity<ChildButton>>,
    }
    impl View for Holder {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column().children(self.child.iter().cloned())
        }
    }
    let mut runtime = Runtime::new();
    let saved = Rc::new(RefCell::new(None));
    let (child, parent) = runtime.update(|cx| {
        let child = cx.new(|_| ChildButton {
            value: 0,
            saved: saved.clone(),
        });
        let parent = cx.new(|_| Holder {
            child: Some(child.clone()),
        });
        (child, parent)
    });
    let mut ui = Ui::new(&mut runtime, parent.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    let old = saved.borrow().as_ref().unwrap().clone();
    runtime.update(|cx| parent.update(cx, |this, _| this.child = None));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(
        runtime.update(|cx| old.dispatch(&ClickEvent, cx)).unwrap(),
        Dispatch::TargetGone
    );
    assert_eq!(runtime.update(|cx| child.read(cx).value), 0);
}

#[test]
fn child_panic_during_replacement_restores_tree_relationships_on_retry() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    struct Panicking {
        panic: bool,
    }
    impl View for Panicking {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            assert!(!self.panic, "planned child panic");
            label("Restored")
        }
    }
    struct Switch {
        child: Entity<Panicking>,
        show: bool,
    }
    impl View for Switch {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column().padding(20.).child(if self.show {
                self.child.clone().into_element()
            } else {
                label("Before")
            })
        }
    }
    let mut runtime = Runtime::new();
    let (child, parent) = runtime.update(|cx| {
        let child = cx.new(|_| Panicking { panic: true });
        let parent = cx.new(|_| Switch {
            child: child.clone(),
            show: false,
        });
        (child, parent)
    });
    let mut ui = Ui::new(&mut runtime, parent.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    runtime.update(|cx| parent.update(cx, |this, _| this.show = true));
    assert!(
        catch_unwind(AssertUnwindSafe(|| ui.prepare(
            &mut runtime,
            [400., 300.],
            &mut measure
        )))
        .is_err()
    );
    runtime.update(|cx| child.update(cx, |this, _| this.panic = false));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    let restored = ui.elements().find(|e| e.text == Some("Restored")).unwrap();
    assert_eq!([restored.bounds.x, restored.bounds.y], [20., 20.]);
    assert!(restored.bounds.width > 0.);
    assert!(restored.bounds.height > 0.);
}

#[test]
fn viewport_resize_reflows_text_without_reevaluating_component() {
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| {
        cx.new(|_| TextView {
            color: [1.; 4],
            size: 16.,
        })
    });
    let mut ui = Ui::new(&mut runtime, entity).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    let stats = ui.stats();
    let height = ui.elements().next().unwrap().bounds.height;
    ui.prepare(&mut runtime, [40., 300.], &mut measure).unwrap();
    assert_eq!(
        ui.stats().component_evaluations,
        stats.component_evaluations
    );
    assert!(ui.elements().next().unwrap().bounds.height > height);
}

struct ScrollList {
    items: Vec<u32>,
    clicked: Option<u32>,
}
impl View for ScrollList {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .width(120.)
            .height(100.)
            .scroll_y()
            .key("viewport")
            .children(self.items.iter().map(|item| {
                let id = *item;
                button(format!("Item {id}"))
                    .key(id)
                    .height(30.)
                    .on_click(cx.listener(move |this, _, _| this.clicked = Some(id)))
            }))
    }
}
fn scroll_setup() -> (Runtime, Entity<ScrollList>, Ui<ScrollList>, Measure) {
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| {
        cx.new(|_| ScrollList {
            items: (0..5).collect(),
            clicked: None,
        })
    });
    let mut ui = Ui::new(&mut runtime, entity.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [300., 300.], &mut measure)
        .unwrap();
    (runtime, entity, ui, measure)
}
#[test]
fn scroll_retains_text_layout_identity_and_clips_hit_testing() {
    let (mut runtime, entity, mut ui, mut measure) = scroll_setup();
    let hidden = keyed(&ui, 4);
    assert!(ui.hit_test([10., 130.]).is_none());
    let stats = ui.stats();
    let calls = measure.calls;
    assert!(ui.scroll([10., 20.], [0., 100.]).unwrap());
    let viewport = ui
        .elements()
        .find(|e| e.key == Some(&"viewport".into()))
        .unwrap();
    assert_eq!(viewport.scroll_offset, [0., 50.]);
    assert_eq!(viewport.scroll_range, [0., 50.]);
    assert_eq!(ui.element(hidden).unwrap().bounds.y, 70.);
    assert_eq!(ui.hit_test([10., 80.]), Some(hidden));
    ui.pointer(&mut runtime, PointerEvent::Pressed([10., 80.]))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Released([10., 80.]))
        .unwrap();
    assert_eq!(runtime.update(|cx| entity.read(cx).clicked), Some(4));
    assert_eq!(ui.stats(), stats);
    assert_eq!(measure.calls, calls);
    ui.prepare(&mut runtime, [300., 300.], &mut measure)
        .unwrap();
    assert_eq!(keyed(&ui, 4), hidden);
    assert_eq!(ui.stats().layout_passes, stats.layout_passes);
}
#[test]
fn focus_reveals_offscreen_button_and_scroll_is_per_placement() {
    let (mut runtime, entity, mut first, mut measure) = scroll_setup();
    let mut second = Ui::new(&mut runtime, entity).unwrap();
    second
        .prepare(&mut runtime, [300., 300.], &mut measure)
        .unwrap();
    for _ in 0..5 {
        first.focus_next(false);
    }
    let focused = first.elements().find(|e| e.focused).unwrap();
    assert_eq!(focused.key, Some(&4.into()));
    assert!(
        focused
            .clip_bounds
            .contains([focused.bounds.x + 1., focused.bounds.y + 1.])
    );
    assert_eq!(first.elements().next().unwrap().scroll_offset, [0., 50.]);
    assert_eq!(second.elements().next().unwrap().scroll_offset, [0., 0.]);
}
#[test]
fn scroll_clamps_after_content_removal_and_rejects_nonfinite_motion() {
    let (mut runtime, entity, mut ui, mut measure) = scroll_setup();
    ui.scroll([10., 20.], [0., 50.]).unwrap();
    runtime.update(|cx| entity.update(cx, |this, _| this.items.truncate(2)));
    ui.prepare(&mut runtime, [300., 300.], &mut measure)
        .unwrap();
    assert_eq!(ui.elements().next().unwrap().scroll_offset, [0., 0.]);
    assert!(!ui.scroll([10., 20.], [0., 1.]).unwrap());
    assert!(matches!(
        ui.scroll([10., 20.], [0., f32::NAN]),
        Err(UiError::InvalidGeometry)
    ));
}
#[test]
fn nested_scroll_chains_unconsumed_delta_and_reorder_retains_offset() {
    struct Nested {
        reverse: bool,
    }
    impl View for Nested {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let mut items: Vec<_> = (0..4).collect();
            if self.reverse {
                items.reverse();
            }
            column()
                .width(140.)
                .height(100.)
                .scroll_y()
                .key("outer")
                .child(
                    column()
                        .width(120.)
                        .height(60.)
                        .scroll_y()
                        .key("inner")
                        .children(
                            items
                                .iter()
                                .map(|id| label(format!("{id}")).key(*id).height(30.)),
                        ),
                )
                .child(label("Footer").height(80.))
        }
    }
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| cx.new(|_| Nested { reverse: false }));
    let mut ui = Ui::new(&mut runtime, entity.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [300., 300.], &mut measure)
        .unwrap();
    ui.scroll([10., 20.], [0., 80.]).unwrap();
    assert_eq!(
        ui.elements()
            .find(|e| e.key == Some(&"inner".into()))
            .unwrap()
            .scroll_offset,
        [0., 60.]
    );
    assert_eq!(
        ui.elements()
            .find(|e| e.key == Some(&"outer".into()))
            .unwrap()
            .scroll_offset,
        [0., 20.]
    );
    runtime.update(|cx| entity.update(cx, |this, _| this.reverse = true));
    ui.prepare(&mut runtime, [300., 300.], &mut measure)
        .unwrap();
    assert_eq!(
        ui.elements()
            .find(|e| e.key == Some(&"inner".into()))
            .unwrap()
            .scroll_offset,
        [0., 60.]
    );
}
#[test]
fn unkeyed_children_keep_identity_when_keyed_siblings_come_and_go() {
    struct Page {
        before: bool,
        between: bool,
    }
    impl View for Page {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let mut list = column();
            if self.before {
                list = list.child(button("Before").key("before"));
            }
            list = list.child(label("First"));
            if self.between {
                list = list.child(button("Between").key("between"));
            }
            list.child(row())
        }
    }
    let mut runtime = Runtime::new();
    let page = runtime.update(|cx| {
        cx.new(|_| Page {
            before: false,
            between: false,
        })
    });
    let mut ui = Ui::new(&mut runtime, page.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    let unkeyed = |ui: &Ui<Page>| {
        let find = |kind| {
            ui.elements()
                .find(|e| e.kind == kind && e.key.is_none())
                .unwrap()
                .id
        };
        (find(ElementType::Label), find(ElementType::Row))
    };
    let ids = unkeyed(&ui);
    for (before, between) in [(true, false), (true, true), (false, true), (false, false)] {
        runtime.update(|cx| {
            page.update(cx, |s, _| {
                s.before = before;
                s.between = between;
            })
        });
        ui.prepare(&mut runtime, [400., 300.], &mut measure)
            .unwrap();
        assert_eq!(unkeyed(&ui), ids, "before={before} between={between}");
    }
}
#[test]
fn custom_leaves_measure_hit_test_and_describe_themselves() {
    // A disc that wants a square of `diameter` and only takes pointer input inside it.
    #[derive(PartialEq)]
    struct Disc {
        diameter: f32,
        measured: Rc<Cell<u32>>,
    }
    impl CustomElement for Disc {
        type State = ();
        fn measure(&self, request: CustomMeasure) -> [f32; 2] {
            self.measured.set(self.measured.get() + 1);
            assert!(request.font_size > 0.);
            [self.diameter; 2]
        }
        fn hit_test(&self, point: [f32; 2], size: [f32; 2]) -> bool {
            let r = size[0] / 2.;
            (point[0] - r).hypot(point[1] - r) <= r
        }
    }
    struct Page {
        diameter: f32,
        clicks: u32,
        measured: Rc<Cell<u32>>,
    }
    impl View for Page {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            row().child(
                custom(Disc {
                    diameter: self.diameter,
                    measured: self.measured.clone(),
                })
                .key("disc")
                .accessibility_label("Status")
                .on_pointer_down(cx.listener(|this, _, _| this.clicks += 1)),
            )
        }
    }
    let measured = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new();
    let page = runtime.update(|cx| {
        cx.new(|_| Page {
            diameter: 40.,
            clicks: 0,
            measured: measured.clone(),
        })
    });
    let mut ui = Ui::new(&mut runtime, page.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    let disc = ui
        .elements()
        .find(|e| e.key == Some(&"disc".into()))
        .unwrap();
    assert_eq!(disc.kind, ElementType::Custom);
    assert_eq!((disc.bounds.width, disc.bounds.height), (40., 40.));
    let id = disc.id;
    let semantic = ui.semantic_node(id).unwrap();
    assert_eq!(
        (semantic.role, semantic.label),
        (SemanticRole::Container, Some("Status"))
    );
    // Corners of the box miss the disc; its center hits.
    ui.pointer(&mut runtime, PointerEvent::Pressed([2., 2.]))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Released([2., 2.]))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Pressed([20., 20.]))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Released([20., 20.]))
        .unwrap();
    assert_eq!(runtime.update(|cx| page.read(cx).clicks), 1);
    // Equal descriptions keep the measurement; a changed one is measured again.
    runtime.update(|cx| page.update(cx, |this, _| this.clicks = 2));
    let before = measured.get();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(measured.get(), before);
    runtime.update(|cx| page.update(cx, |this, _| this.diameter = 60.));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert!(measured.get() > before);
    let disc = ui.element(id).unwrap();
    assert_eq!((disc.bounds.width, disc.bounds.height), (60., 60.));
    assert!(matches!(
        custom(Disc {
            diameter: 1.,
            measured: measured.clone(),
        })
        .child(label("No"))
        .validate(),
        Err(UiError::LeafChildren)
    ));
}
#[test]
fn use_state_is_placement_local_keyed_and_released_with_the_placement() {
    struct Dropped(Rc<Cell<u32>>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    struct Counter {
        drops: Rc<Cell<u32>>,
        rows: Vec<u32>,
    }
    impl View for Counter {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let drops = self.drops.clone();
            let count = cx.use_state(|_| 0_u32);
            let _guard = cx.use_state(move |_| Dropped(drops));
            let bump = count.clone();
            column()
                .child(
                    button(format!("Count {}", *count.read(cx)))
                        .key("bump")
                        .on_click(cx.listener(move |_, _, cx| bump.update(cx, |n, _| *n += 1))),
                )
                .children(self.rows.iter().map(|row| {
                    let value = cx.use_keyed_state(row, |_| *row * 10);
                    label(format!("{}", *value.read(cx))).key(*row)
                }))
        }
    }
    struct Host {
        show: bool,
        child: Entity<Counter>,
    }
    impl View for Host {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let mut root = column();
            if self.show {
                root = root.child(self.child.clone());
            }
            root
        }
    }
    let drops = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new();
    let child = runtime.update(|cx| {
        cx.new(|_| Counter {
            drops: drops.clone(),
            rows: vec![1, 2],
        })
    });
    let host = runtime.update(|cx| {
        cx.new(|_| Host {
            show: true,
            child: child.clone(),
        })
    });
    let mut ui = Ui::new(&mut runtime, host.clone()).unwrap();
    let mut other = Ui::new(&mut runtime, child.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    other
        .prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    let labels = |ui: &Ui<Host>| {
        ui.elements()
            .filter_map(|e| e.text.map(str::to_owned))
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&ui), ["Count 0", "10", "20"]);
    let point = |ui: &Ui<Host>| {
        let b = ui
            .elements()
            .find(|e| e.key == Some(&"bump".into()))
            .unwrap()
            .bounds;
        [b.x + 2., b.y + 2.]
    };
    for _ in 0..2 {
        ui.pointer(&mut runtime, PointerEvent::Pressed(point(&ui)))
            .unwrap();
        ui.pointer(&mut runtime, PointerEvent::Released(point(&ui)))
            .unwrap();
        ui.prepare(&mut runtime, [400., 300.], &mut measure)
            .unwrap();
    }
    // Keyed slots follow their keys when rows reorder or are added.
    runtime.update(|cx| child.update(cx, |c, _| c.rows = vec![3, 1]));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(labels(&ui), ["Count 2", "30", "10"]);
    // The other window's placement keeps its own count.
    other
        .prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert!(other.elements().any(|e| e.text == Some("Count 0")));
    // Removing the placement releases its state; the other placement keeps its own.
    runtime.update(|cx| host.update(cx, |h, _| h.show = false));
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    runtime.synchronize();
    assert_eq!(drops.get(), 1);
    drop(other);
    runtime.synchronize();
    assert_eq!(drops.get(), 2);
}
#[test]
fn views_reading_the_theme_reevaluate_when_their_placement_theme_changes() {
    struct Reader {
        evaluations: Rc<Cell<u32>>,
    }
    impl View for Reader {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            self.evaluations.set(self.evaluations.get() + 1);
            let size = cx.theme().sizes().font_size;
            label(format!("{size}")).key("size")
        }
    }
    struct Ignorer {
        evaluations: Rc<Cell<u32>>,
    }
    impl View for Ignorer {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            self.evaluations.set(self.evaluations.get() + 1);
            label("Static")
        }
    }
    struct Root {
        reader: Entity<Reader>,
        ignorer: Entity<Ignorer>,
        scoped: Option<Theme>,
    }
    impl View for Root {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let mut scope = column()
                .child(self.reader.clone())
                .child(self.ignorer.clone());
            if let Some(theme) = &self.scoped {
                scope = scope.theme(theme.clone());
            }
            column().child(scope)
        }
    }
    let read = Rc::new(Cell::new(0));
    let ignored = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new();
    let reader = runtime.update(|cx| {
        cx.new(|_| Reader {
            evaluations: read.clone(),
        })
    });
    let ignorer = runtime.update(|cx| {
        cx.new(|_| Ignorer {
            evaluations: ignored.clone(),
        })
    });
    let root = runtime.update(|cx| {
        cx.new(|_| Root {
            reader,
            ignorer,
            scoped: None,
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    let size = |ui: &Ui<Root>| {
        ui.elements()
            .find(|e| e.key == Some(&"size".into()))
            .unwrap()
            .text
            .unwrap()
            .to_owned()
    };
    assert_eq!(size(&ui), "14");
    assert_eq!((read.get(), ignored.get()), (1, 1));
    ui.set_theme(Theme::dark().compact()).unwrap();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(size(&ui), "13");
    assert_eq!((read.get(), ignored.get()), (2, 1));
    // A palette-only switch leaves the font size alone but still changes the theme.
    ui.set_theme(Theme::light().compact()).unwrap();
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!((read.get(), ignored.get()), (3, 1));
    // A subtree override above the component is what its view sees.
    runtime.update(|cx| {
        root.update(cx, |r, _| {
            r.scoped = Some(Theme::dark().metrics(|m| m.font_size = 20.))
        })
    });
    ui.prepare(&mut runtime, [400., 300.], &mut measure)
        .unwrap();
    assert_eq!(size(&ui), "20");
    assert_eq!(ignored.get(), 1);
}
#[test]
fn animation_frames_reevaluate_requesting_views_until_they_stop() {
    use std::time::Duration;
    struct Fade {
        evaluations: Rc<Cell<u32>>,
    }
    impl View for Fade {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            self.evaluations.set(self.evaluations.get() + 1);
            let progress = (cx.frame_time().as_secs_f32() / 0.5).min(1.);
            if progress < 1. {
                cx.request_animation_frame();
            }
            column().opacity(progress).key("fade")
        }
    }
    let evaluations = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Fade {
            evaluations: evaluations.clone(),
        })
    });
    let mut ui = Ui::new(&mut runtime, root).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [100., 100.], &mut measure)
        .unwrap();
    let opacity = |ui: &Ui<Fade>| ui.elements().next().unwrap().opacity;
    assert!(runtime.animation_frame_requested());
    assert!(!ui.needs_prepare(&runtime).unwrap());
    for (ms, expected) in [(250, 0.5), (500, 1.)] {
        assert!(runtime.begin_frame(Duration::from_millis(ms)));
        assert!(ui.needs_prepare(&runtime).unwrap());
        ui.prepare(&mut runtime, [100., 100.], &mut measure)
            .unwrap();
        assert_eq!(opacity(&ui), expected);
    }
    // The finished animation stops requesting frames; later frames do no work.
    assert!(!runtime.animation_frame_requested());
    assert!(!runtime.begin_frame(Duration::from_millis(750)));
    assert!(!ui.needs_prepare(&runtime).unwrap());
    assert_eq!(evaluations.get(), 3);
}
