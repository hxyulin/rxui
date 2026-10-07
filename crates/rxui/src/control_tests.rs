use crate::*;
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
struct SplitDemo {
    position: SplitPosition,
    phases: Vec<ResizePhase>,
    minimum: f32,
}
impl View for SplitDemo {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        split_row(
            column().background([1., 0., 0., 1.]),
            column().background([0., 0., 1., 1.]),
        )
        .size(400., 200.)
        .min_first(self.minimum)
        .min_second(80.)
        .position(self.position)
        .on_resize(cx.listener(|s, e: &ResizeEvent, _| {
            s.position = e.position;
            s.phases.push(e.phase);
        }))
        .key("split")
    }
}
fn split_setup(position: SplitPosition) -> (Runtime, Entity<SplitDemo>, Ui<SplitDemo>) {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| SplitDemo {
            position,
            phases: Vec::new(),
            minimum: 80.,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
    (r, e, ui)
}
fn divider<T: View>(ui: &Ui<T>) -> ElementInfo<'_> {
    ui.elements()
        .find(|e| e.kind == ElementType::Splitter)
        .unwrap()
}
#[test]
fn controlled_fraction_and_pixel_splits_layout_constraints_drag_escape_and_keys() {
    for position in [SplitPosition::Fraction(0.5), SplitPosition::Pixels(196.)] {
        let (mut r, e, mut ui) = split_setup(position);
        let id = divider(&ui).id;
        let start = divider(&ui).bounds.x;
        assert!((start - 196.).abs() < 0.1, "first pane was {start}");
        ui.pointer(&mut r, PointerEvent::Pressed([start + 3., 20.]))
            .unwrap();
        assert_eq!(ui.captured_pointer(), Some(id));
        ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
        ui.pointer(&mut r, PointerEvent::Moved([start + 43., 150.]))
            .unwrap();
        ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
        assert!((divider(&ui).bounds.x - 236.).abs() < 0.1);
        assert_eq!(divider(&ui).id, id);
        ui.key(
            &mut r,
            KeyEvent {
                key: KeyboardKey::Escape,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            },
        )
        .unwrap();
        ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
        assert!((divider(&ui).bounds.x - start).abs() < 0.1);
        assert_eq!(ui.captured_pointer(), None);
        ui.key(
            &mut r,
            KeyEvent {
                key: KeyboardKey::End,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            },
        )
        .unwrap();
        ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
        assert!((divider(&ui).bounds.x - 312.).abs() < 0.1);
        ui.semantic_action(
            &mut r,
            SemanticAction::SetNumericValue {
                target: id,
                value: 10.,
            },
            &mut Measure,
        )
        .unwrap();
        ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
        assert!((divider(&ui).bounds.x - 80.).abs() < 0.1);
        r.update(|cx| assert!(e.read(cx).phases.contains(&ResizePhase::Cancel)));
    }
}
struct ScrollDemo {
    handle: ScrollHandle,
    slot: Option<ScrollPlacement>,
    metrics: bool,
}
impl View for ScrollDemo {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let caption = if self.metrics {
            format!("{:?}", self.handle.state(cx).map(|s| s.offset))
        } else {
            "Jump".into()
        };
        column()
            .size(200., 120.)
            .child(
                scroll_area(
                    column().children((0..25).map(|i| label(format!("row{i}")).height(20.))),
                )
                .handle(self.handle.clone())
                .height(100.),
            )
            .child(
                button(caption)
                    .height(20.)
                    .padding(0.)
                    .on_click(cx.listener(|s, _, cx| {
                        let slot = s.handle.placement(cx).unwrap();
                        slot.scroll_to(cx, [0., 300.]).unwrap();
                        s.slot = Some(slot);
                    })),
            )
    }
}
fn scroll_setup(metrics: bool) -> (Runtime, Entity<ScrollDemo>, Ui<ScrollDemo>) {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| ScrollDemo {
            handle: ScrollHandle::new(),
            slot: None,
            metrics,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    ui.prepare(&mut r, [200., 120.], &mut Measure).unwrap();
    (r, e, ui)
}
#[test]
fn scrollbars_drag_keys_and_explicit_commands_are_placement_scoped_and_disposable() {
    let (mut r, e, mut first) = scroll_setup(false);
    let mut second = Ui::new(&mut r, e.clone()).unwrap();
    second.prepare(&mut r, [200., 120.], &mut Measure).unwrap();
    let bar = first
        .elements()
        .find(|e| e.kind == ElementType::Scrollbar)
        .unwrap();
    let id = bar.id;
    let thumb = bar.range.unwrap().thumb_bounds.unwrap();
    let range = bar.range.unwrap().max;
    assert!((range - 400.).abs() < 0.1, "range {range}");
    first
        .pointer(&mut r, PointerEvent::Pressed([thumb.x + 2., thumb.y + 2.]))
        .unwrap();
    assert_eq!(first.captured_pointer(), Some(id));
    first
        .pointer(&mut r, PointerEvent::Moved([100., 80.]))
        .unwrap();
    assert!(first.element(id).unwrap().range.unwrap().value > 100.);
    first
        .pointer(&mut r, PointerEvent::Released([100., 80.]))
        .unwrap();
    assert_eq!(first.captured_pointer(), None);
    let stats = first.stats();
    first
        .key(
            &mut r,
            KeyEvent {
                key: KeyboardKey::End,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            },
        )
        .unwrap();
    assert_eq!(first.element(id).unwrap().range.unwrap().value, range);
    assert_eq!(first.stats(), stats);
    assert_eq!(
        second
            .elements()
            .find(|e| e.kind == ElementType::Scrollbar)
            .unwrap()
            .range
            .unwrap()
            .value,
        0.
    );
    first
        .pointer(&mut r, PointerEvent::Pressed([10., 110.]))
        .unwrap();
    first
        .pointer(&mut r, PointerEvent::Released([10., 110.]))
        .unwrap();
    first.prepare(&mut r, [200., 120.], &mut Measure).unwrap();
    let slot = r.update(|cx| e.read(cx).slot.clone().unwrap());
    assert_eq!(slot.state().unwrap().offset[1], 300.);
    r.update(|cx| slot.scroll_by(cx, [0., 1000.]).unwrap());
    assert!(first.needs_prepare(&r).unwrap());
    first.prepare(&mut r, [200., 120.], &mut Measure).unwrap();
    assert_eq!(slot.state().unwrap().offset[1], 400.);
    let mut foreign = Runtime::new();
    assert_eq!(
        foreign.update(|cx| slot.scroll_to(cx, [0.; 2])),
        Err(ScrollError::WrongRuntime)
    );
    drop(first);
    assert_eq!(slot.state(), Err(ScrollError::Disposed));
    assert_eq!(
        second
            .elements()
            .find(|e| e.kind == ElementType::Scrollbar)
            .unwrap()
            .range
            .unwrap()
            .value,
        0.
    );
}
#[test]
fn reading_scroll_metrics_subscribes_only_its_mount_and_settles_initial_layout() {
    let (mut r, e, mut first) = scroll_setup(true);
    let mut second = Ui::new(&mut r, e).unwrap();
    second.prepare(&mut r, [200., 120.], &mut Measure).unwrap();
    assert!(!first.needs_prepare(&r).unwrap());
    assert!(!second.needs_prepare(&r).unwrap());
    first.scroll([10., 10.], [0., 20.]).unwrap();
    assert!(first.needs_prepare(&r).unwrap());
    assert!(!second.needs_prepare(&r).unwrap());
    first.prepare(&mut r, [200., 120.], &mut Measure).unwrap();
    assert!(!first.needs_prepare(&r).unwrap());
}
#[test]
fn invalid_split_and_handle_bindings_are_rejected_without_panics() {
    let (mut r, e, mut ui) = split_setup(SplitPosition::Fraction(0.5));
    for position in [
        SplitPosition::Fraction(f32::NAN),
        SplitPosition::Fraction(1.1),
        SplitPosition::Pixels(-1.),
    ] {
        r.update(|cx| e.update(cx, |s, _| s.position = position));
        assert!(matches!(
            ui.prepare(&mut r, [400., 200.], &mut Measure),
            Err(UiError::InvalidRangeControl)
        ));
        r.update(|cx| e.update(cx, |s, _| s.position = SplitPosition::Fraction(0.5)));
        ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
    }
}

fn key<T: View>(ui: &mut Ui<T>, r: &mut Runtime, key: KeyboardKey) -> InputResult {
    ui.key(
        r,
        KeyEvent {
            key,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap()
}
#[test]
fn oversized_pixel_intent_survives_press_release_escape_and_host_cancellation() {
    let (mut r, e, mut ui) = split_setup(SplitPosition::Pixels(900.));
    assert_eq!(divider(&ui).range.unwrap().value, 312.);
    for escape in [false, true] {
        ui.pointer(&mut r, PointerEvent::Pressed([315., 20.]))
            .unwrap();
        ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
        r.update(|cx| assert_eq!(e.read(cx).position, SplitPosition::Pixels(900.)));
        if escape {
            ui.pointer(&mut r, PointerEvent::Moved([115., 20.]))
                .unwrap();
            ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
            assert_eq!(divider(&ui).range.unwrap().value, 112.);
            key(&mut ui, &mut r, KeyboardKey::Escape);
        } else {
            ui.pointer(&mut r, PointerEvent::Released([315., 20.]))
                .unwrap();
        }
        ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
        r.update(|cx| assert_eq!(e.read(cx).position, SplitPosition::Pixels(900.)));
    }
    ui.pointer(&mut r, PointerEvent::Pressed([315., 20.]))
        .unwrap();
    ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
    ui.pointer(&mut r, PointerEvent::Cancelled).unwrap();
    ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
    r.update(|cx| assert_eq!(e.read(cx).position, SplitPosition::Pixels(900.)));
}
#[test]
fn scroll_track_pages_escape_and_metric_unsubscription_keep_geometry_path() {
    let (mut r, e, mut ui) = scroll_setup(true);
    r.update(|cx| e.update(cx, |s, _| s.metrics = false));
    ui.prepare(&mut r, [200., 120.], &mut Measure).unwrap();
    let id = ui
        .elements()
        .find(|e| e.kind == ElementType::Scrollbar)
        .unwrap()
        .id;
    let bounds = ui.element(id).unwrap().bounds;
    let stats = ui.stats();
    ui.pointer(
        &mut r,
        PointerEvent::Pressed([bounds.x + 6., bounds.y + 80.]),
    )
    .unwrap();
    assert!(ui.element(id).unwrap().range.unwrap().value > 100.);
    key(&mut ui, &mut r, KeyboardKey::Escape);
    assert_eq!(ui.element(id).unwrap().range.unwrap().value, 0.);
    assert!(key(&mut ui, &mut r, KeyboardKey::Home).default_prevented);
    assert!(key(&mut ui, &mut r, KeyboardKey::PageDown).default_prevented);
    assert_eq!(ui.element(id).unwrap().range.unwrap().value, 100.);
    key(&mut ui, &mut r, KeyboardKey::End);
    assert_eq!(ui.element(id).unwrap().range.unwrap().value, 400.);
    ui.prepare(&mut r, [200., 120.], &mut Measure).unwrap();
    assert_eq!(ui.stats(), stats);
    assert!(
        !ui.needs_prepare(&r).unwrap(),
        "former metric reader stayed subscribed"
    );
}
struct Handles {
    handle: ScrollHandle,
    mode: u8,
}
impl View for Handles {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let viewport = |key| {
            column()
                .key(key)
                .size(100., 100.)
                .scroll_y()
                .scroll_handle(self.handle.clone())
                .child(column().height(500.))
        };
        match self.mode {
            0 => column().child(viewport("one")),
            1 => column().child(viewport("one")).child(viewport("two")),
            2 => column().size(100., 100.).scroll_handle(self.handle.clone()),
            3 => column(),
            _ => column().child(viewport("replacement")),
        }
    }
}
#[test]
fn invalid_handle_bindings_retry_and_replaced_placements_reject_queued_commands() {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Handles {
            handle: ScrollHandle::new(),
            mode: 0,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    ui.prepare(&mut r, [100.; 2], &mut Measure).unwrap();
    let mount = ui.mount_ids().next().unwrap();
    assert!(matches!(
        r.update(|cx| e.read(cx).handle.placement(cx)),
        Err(ScrollError::NoPlacement)
    ));
    for (mode, expected) in [(1, "duplicate"), (2, "non-scroll")] {
        r.update(|cx| e.update(cx, |s, _| s.mode = mode));
        let result = ui.prepare(&mut r, [100.; 2], &mut Measure);
        assert!(
            matches!(
                (&result, expected),
                (Err(UiError::DuplicateScrollHandle), "duplicate")
                    | (Err(UiError::InvalidScrollHandle), "non-scroll")
            ),
            "{result:?}"
        );
        assert!(!ui.is_prepared());
        r.update(|cx| e.update(cx, |s, _| s.mode = 0));
        ui.prepare(&mut r, [100.; 2], &mut Measure).unwrap();
    }
    // Capture a fresh slot after error retry, then replace its retained identity.
    let slot = {
        let cx = AppContext {
            runtime: &r.inner,
            dispatch_mount: Some(mount),
        };
        e.read(&cx).handle.placement(&cx).unwrap()
    };
    r.update(|cx| {
        assert_eq!(
            slot.scroll_to(cx, [0., f32::NAN]),
            Err(ScrollError::InvalidOffset)
        );
        slot.scroll_to(cx, [0., 300.]).unwrap();
        e.update(cx, |s, _| s.mode = 4);
    });
    ui.prepare(&mut r, [100.; 2], &mut Measure).unwrap();
    assert_eq!(slot.state(), Err(ScrollError::Disposed));
    assert_eq!(
        ui.elements()
            .find(|n| n.key == Some(&Key::from("replacement")))
            .unwrap()
            .scroll_offset,
        [0.; 2]
    );
    drop(slot);
}
struct Vertical {
    position: SplitPosition,
    small: bool,
    axis: bool,
    phases: Vec<ResizePhase>,
}
impl View for Vertical {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let listener = cx.listener(|s, e: &ResizeEvent, _| {
            s.position = e.position;
            s.phases.push(e.phase);
        });
        let split = if self.axis {
            split_row(column(), column())
        } else {
            split_column(column(), column())
        };
        split
            .size(200., if self.small { 60. } else { 300. })
            .position(self.position)
            .min_first(80.)
            .min_second(80.)
            .on_resize(listener)
    }
}
#[test]
fn vertical_split_keyboard_axis_change_and_insufficient_space_cancel_cleanly() {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Vertical {
            position: SplitPosition::Fraction(0.5),
            small: false,
            axis: false,
            phases: Vec::new(),
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    ui.prepare(&mut r, [200., 300.], &mut Measure).unwrap();
    let b = divider(&ui).bounds;
    assert_eq!(b.y, 146.);
    ui.pointer(&mut r, PointerEvent::Pressed([20., b.y + 2.]))
        .unwrap();
    ui.prepare(&mut r, [200., 300.], &mut Measure).unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([190., b.y + 30.]))
        .unwrap();
    ui.prepare(&mut r, [200., 300.], &mut Measure).unwrap();
    assert_eq!(divider(&ui).bounds.y, 174.);
    ui.pointer(&mut r, PointerEvent::Released([190., b.y + 30.]))
        .unwrap();
    key(&mut ui, &mut r, KeyboardKey::ArrowUp);
    ui.prepare(&mut r, [200., 300.], &mut Measure).unwrap();
    assert_eq!(divider(&ui).bounds.y, 166.);
    ui.pointer(&mut r, PointerEvent::Pressed([20., 168.]))
        .unwrap();
    r.update(|cx| e.update(cx, |s, _| s.axis = true));
    ui.prepare(&mut r, [200., 300.], &mut Measure).unwrap();
    assert_eq!(ui.captured_pointer(), None);
    r.update(|cx| {
        assert!(e.read(cx).phases.contains(&ResizePhase::Cancel));
        e.update(cx, |s, _| {
            s.axis = false;
            s.small = true;
        });
    });
    ui.prepare(&mut r, [200., 300.], &mut Measure).unwrap();
    assert!(divider(&ui).range.unwrap().read_only);
    assert_eq!(divider(&ui).bounds.y, 80.);
    assert!(
        ui.elements()
            .all(|n| n.bounds.width >= 0. && n.bounds.height >= 0.)
    );
}

#[cfg(feature = "accessibility")]
#[test]
fn range_semantics_publish_orientation_and_dispatch_numeric_actions() {
    use accesskit::{Action, ActionData, ActionRequest, Orientation, Role, TreeId};
    let (mut r, e, mut ui) = split_setup(SplitPosition::Pixels(196.));
    let mut tree = AccessKitTree::new();
    let update = tree.update(&ui, "Split", 2.).unwrap().unwrap();
    let (id, node) = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::Splitter)
        .unwrap();
    assert_eq!(node.orientation(), Some(Orientation::Vertical));
    assert_eq!(node.numeric_value(), Some(196.)); // logical, independent of DPR
    assert_eq!(node.min_numeric_value(), Some(80.));
    assert!(node.supports_action(Action::Increment));
    let request = |action, data| ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: *id,
        data,
    };
    let action = tree
        .action(request(
            Action::SetValue,
            Some(ActionData::NumericValue(230.)),
        ))
        .unwrap();
    ui.semantic_action(&mut r, action, &mut Measure).unwrap();
    ui.prepare(&mut r, [400., 200.], &mut Measure).unwrap();
    r.update(|cx| assert_eq!(e.read(cx).position, SplitPosition::Pixels(230.)));
    let (mut r, _, mut ui) = scroll_setup(false);
    let mut tree = AccessKitTree::new();
    let update = tree.update(&ui, "Scroll", 2.).unwrap().unwrap();
    let (id, node) = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::ScrollBar)
        .unwrap();
    assert_eq!(node.orientation(), Some(Orientation::Vertical));
    let action = tree
        .action(ActionRequest {
            action: Action::Increment,
            target_tree: TreeId::ROOT,
            target_node: *id,
            data: None,
        })
        .unwrap();
    ui.semantic_action(&mut r, action, &mut Measure).unwrap();
    assert_eq!(
        ui.elements()
            .find(|e| e.kind == ElementType::Scrollbar)
            .unwrap()
            .range
            .unwrap()
            .value,
        40.
    );
}
struct Both {
    handle: ScrollHandle,
    extent: f32,
}
impl View for Both {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        scroll_area(column().size(self.extent, self.extent))
            .axes(ScrollAxes::Both)
            .handle(self.handle.clone())
            .fill_width()
            .fill_height()
    }
}
#[test]
fn two_axis_bars_wheel_over_track_viewport_resize_and_content_shrink_publish_metrics() {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Both {
            handle: ScrollHandle::new(),
            extent: 500.,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    ui.prepare(&mut r, [200., 150.], &mut Measure).unwrap();
    let ranges: Vec<_> = ui.elements().filter_map(|n| n.range).collect();
    assert_eq!(ranges.len(), 2);
    assert_eq!(
        ranges
            .iter()
            .find(|r| r.axis == Axis::Horizontal)
            .unwrap()
            .max,
        312.
    );
    assert_eq!(
        ranges
            .iter()
            .find(|r| r.axis == Axis::Vertical)
            .unwrap()
            .max,
        362.
    );
    let bar = ui
        .elements()
        .find(|n| n.range.is_some_and(|r| r.axis == Axis::Vertical))
        .unwrap()
        .bounds;
    assert!(ui.scroll([bar.x + 4., bar.y + 4.], [0., 40.]).unwrap());
    ui.prepare(&mut r, [200., 250.], &mut Measure).unwrap();
    let vertical = ui
        .elements()
        .filter_map(|n| n.range)
        .find(|r| r.axis == Axis::Vertical)
        .unwrap();
    assert_eq!(vertical.max, 262.);
    assert_eq!(vertical.value, 40.);
    r.update(|cx| e.update(cx, |s, _| s.extent = 40.));
    ui.prepare(&mut r, [200., 250.], &mut Measure).unwrap();
    assert!(
        ui.elements()
            .filter_map(|n| n.range)
            .all(|r| r.read_only && r.value == 0. && r.max == 0.)
    );
    assert!(
        ui.elements()
            .filter(|n| n.kind == ElementType::Scrollbar)
            .all(|n| !n.focusable && n.disabled)
    );
}
#[test]
fn scroll_metric_layout_cycles_are_diagnosed_and_corrected_views_retry() {
    struct Cycle {
        handle: ScrollHandle,
        cycle: bool,
    }
    impl View for Cycle {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let previous = self.handle.state(cx);
            let height = if self.cycle && previous.is_some_and(|s| s.viewport[1] == 100.) {
                200.
            } else {
                100.
            };
            column()
                .size(100., height)
                .scroll_y()
                .scroll_handle(self.handle.clone())
                .child(column().height(500.))
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Cycle {
            handle: ScrollHandle::new(),
            cycle: true,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    assert!(matches!(
        ui.prepare(&mut r, [100., 300.], &mut Measure),
        Err(UiError::UnstableControlLayout)
    ));
    assert!(!ui.is_prepared());
    r.update(|cx| e.update(cx, |s, _| s.cycle = false));
    ui.prepare(&mut r, [100., 300.], &mut Measure).unwrap();
    assert!(ui.is_prepared());
    assert!(!ui.needs_prepare(&r).unwrap());
}
