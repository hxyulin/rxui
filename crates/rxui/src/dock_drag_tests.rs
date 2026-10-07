use super::*;

fn middle(b: Bounds) -> [f32; 2] {
    [b.x + b.width * 0.5, b.y + b.height * 0.5]
}
fn group_bounds(ui: &Ui<Workspace>, id: DockNodeId) -> Bounds {
    ui.elements()
        .find(|e| e.key == Some(&id.key()))
        .unwrap()
        .bounds
}
fn body(ui: &Ui<Workspace>, name: &str) -> Bounds {
    let id = ui
        .semantics()
        .find(|n| n.id == header(ui, name))
        .unwrap()
        .controls
        .unwrap();
    ui.element(id).unwrap().bounds
}
fn start(r: &mut Runtime, ui: &mut Ui<Workspace>, name: &str) {
    let point = middle(ui.element(header(ui, name)).unwrap().bounds);
    assert!(ui.pointer(r, PointerEvent::Pressed(point)).unwrap());
    assert!(ui.dock_drag().is_none());
    assert!(ui.captured_pointer().is_some());
}
fn move_to(r: &mut Runtime, ui: &mut Ui<Workspace>, point: [f32; 2]) -> DockDropPreview {
    assert!(ui.pointer(r, PointerEvent::Moved(point)).unwrap());
    ui.dock_drag().unwrap().preview.unwrap()
}
fn drops(r: &mut Runtime, e: &Entity<Workspace>) -> Vec<DockEvent> {
    r.update(|cx| {
        e.read(cx)
            .events
            .iter()
            .filter(|e| matches!(e, DockEvent::Drop { .. }))
            .cloned()
            .collect()
    })
}
#[test]
fn a_small_motion_remains_a_click_and_close_buttons_never_begin_a_drag() {
    let (mut r, e, mut ui, main, _) = setup();
    let point = middle(ui.element(header(&ui, "preview")).unwrap().bounds);
    start(&mut r, &mut ui, "preview");
    ui.pointer(&mut r, PointerEvent::Moved([point[0] + 2., point[1]]))
        .unwrap();
    assert!(ui.dock_drag().is_none());
    ui.pointer(&mut r, PointerEvent::Released([point[0] + 2., point[1]]))
        .unwrap();
    prepare(&mut r, &mut ui);
    assert!(ui.captured_pointer().is_none());
    assert!(drops(&mut r, &e).is_empty());
    assert_eq!(
        r.update(|cx| e
            .read(cx)
            .tree
            .node(main)
            .unwrap()
            .tabs()
            .unwrap()
            .selected()
            .cloned()),
        Some(key("preview"))
    );
    let close = ui
        .semantics()
        .find(|n| n.label == Some("Close String(\"preview\")"))
        .unwrap()
        .id;
    let point = middle(ui.element(close).unwrap().bounds);
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    assert!(ui.captured_pointer().is_none());
    ui.pointer(&mut r, PointerEvent::Moved([point[0], point[1] + 150.]))
        .unwrap();
    assert!(ui.dock_drag().is_none());
    ui.pointer(&mut r, PointerEvent::Released([point[0], point[1] + 150.]))
        .unwrap();
    assert!(r.update(|cx| e.read(cx).tree.group_for(&key("preview")).is_some()));
}
#[test]
fn header_reorder_is_a_single_controlled_drop_without_model_updates_during_motion() {
    let (mut r, e, mut ui, main, _) = setup();
    let editor_id = field(&ui, "editor");
    ui.focus(editor_id);
    ui.text_input(&mut r, TextInputEvent::SelectAll, &mut Measure)
        .unwrap();
    let selection = ui.element(editor_id).unwrap().editing.unwrap().selection;
    let before = r.update(|cx| e.read(cx).tree.clone());
    let revision = r.revision(&e).unwrap();
    let evaluations = ui.stats().component_evaluations;
    let b = ui.element(header(&ui, "editor")).unwrap().bounds;
    let point = [b.x + 1., b.y + b.height * 0.5];
    start(&mut r, &mut ui, "preview");
    for _ in 0..16 {
        let preview = move_to(&mut r, &mut ui, point);
        assert!(preview.insertion);
        assert_eq!(
            preview.target,
            DockDropTarget::Tab {
                group: main,
                index: 0
            }
        );
        prepare(&mut r, &mut ui);
    }
    assert_eq!(r.revision(&e).unwrap(), revision);
    assert_eq!(ui.stats().component_evaluations, evaluations);
    assert_eq!(r.update(|cx| e.read(cx).tree.clone()), before);
    assert!(drops(&mut r, &e).is_empty());
    ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    prepare(&mut r, &mut ui);
    assert_eq!(drops(&mut r, &e).len(), 1);
    assert!(ui.dock_drag().is_none());
    assert!(ui.captured_pointer().is_none());
    assert_eq!(
        r.update(|cx| e
            .read(cx)
            .tree
            .node(main)
            .unwrap()
            .tabs()
            .unwrap()
            .panels()
            .to_vec()),
        vec![key("preview"), key("editor")]
    );
    assert!(ui.contains_element(editor_id));
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.tree.select(main, &key("editor")).unwrap();
        })
    });
    prepare(&mut r, &mut ui);
    assert_eq!(field(&ui, "editor"), editor_id);
    assert_eq!(
        ui.element(editor_id).unwrap().editing.unwrap().selection,
        selection
    );
    // Drop the first tab after the last header: the post-removal index is one.
    let b = ui.element(header(&ui, "editor")).unwrap().bounds;
    let point = [b.x + b.width - 1., b.y + b.height * 0.5];
    start(&mut r, &mut ui, "preview");
    assert_eq!(
        move_to(&mut r, &mut ui, point).target,
        DockDropTarget::Tab {
            group: main,
            index: 1
        }
    );
    ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    prepare(&mut r, &mut ui);
    assert_eq!(
        r.update(|cx| e
            .read(cx)
            .tree
            .node(main)
            .unwrap()
            .tabs()
            .unwrap()
            .panels()
            .to_vec()),
        vec![key("editor"), key("preview")]
    );
}
#[test]
fn center_drop_merges_and_rejecting_it_preserves_the_model() {
    for accept in [false, true] {
        let (mut r, e, mut ui, main, output) = setup();
        r.update(|cx| e.update(cx, |s, _| s.accept = accept));
        prepare(&mut r, &mut ui);
        let before = r.update(|cx| e.read(cx).tree.clone());
        let point = middle(body(&ui, "output"));
        let old = field(&ui, "editor");
        start(&mut r, &mut ui, "editor");
        let preview = move_to(&mut r, &mut ui, point);
        assert_eq!(
            preview.target,
            DockDropTarget::Tab {
                group: output,
                index: 1
            }
        );
        assert!(!preview.insertion);
        ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
        prepare(&mut r, &mut ui);
        assert_eq!(drops(&mut r, &e).len(), 1);
        if accept {
            assert_eq!(
                r.update(|cx| e.read(cx).tree.group_for(&key("editor"))),
                Some(output)
            );
            assert_eq!(
                r.update(|cx| e
                    .read(cx)
                    .tree
                    .node(main)
                    .unwrap()
                    .tabs()
                    .unwrap()
                    .selected()
                    .cloned()),
                Some(key("preview"))
            );
            assert_ne!(field(&ui, "editor"), old);
            assert_eq!(ui.focused_element(), Some(header(&ui, "editor")));
        } else {
            assert_eq!(r.update(|cx| e.read(cx).tree.clone()), before);
            assert_eq!(field(&ui, "editor"), old);
        }
    }
}
#[test]
fn all_edges_preview_half_panes_and_apply_checked_split_proposals() {
    for side in [
        DockSide::Left,
        DockSide::Right,
        DockSide::Top,
        DockSide::Bottom,
    ] {
        let (mut r, e, mut ui, main, _) = setup();
        let b = body(&ui, "editor");
        let pane = group_bounds(&ui, main);
        let point = match side {
            DockSide::Left => [b.x + 2., b.y + b.height * 0.5],
            DockSide::Right => [b.x + b.width - 2., b.y + b.height * 0.5],
            DockSide::Top => [b.x + b.width * 0.5, b.y + 2.],
            DockSide::Bottom => [b.x + b.width * 0.5, b.y + b.height - 2.],
        };
        start(&mut r, &mut ui, "preview");
        let preview = move_to(&mut r, &mut ui, point);
        assert_eq!(
            preview.target,
            DockDropTarget::Split {
                group: main,
                side,
                position: SplitPosition::Fraction(0.5)
            }
        );
        let extent = if matches!(side, DockSide::Left | DockSide::Right) {
            preview.bounds.width
        } else {
            preview.bounds.height
        };
        let expected = if matches!(side, DockSide::Left | DockSide::Right) {
            pane.width
        } else {
            pane.height
        };
        assert!((extent - (expected - 8.) * 0.5).abs() < 0.1);
        ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
        prepare(&mut r, &mut ui);
        assert_ne!(
            r.update(|cx| e.read(cx).tree.group_for(&key("preview"))),
            Some(main)
        );
        invariant(&r.update(|cx| e.read(cx).tree.clone()));
    }
}
#[test]
fn cancellation_outside_noop_and_unavailable_edges_never_commit() {
    for mode in 0..4 {
        let (mut r, e, mut ui, _, _) = setup();
        let before = r.update(|cx| e.read(cx).tree.clone());
        start(&mut r, &mut ui, "preview");
        let point = middle(body(&ui, "output"));
        move_to(&mut r, &mut ui, point);
        match mode {
            0 => assert!(
                ui.key(
                    &mut r,
                    KeyEvent {
                        key: KeyboardKey::Escape,
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::default()
                    }
                )
                .unwrap()
                .changed
            ),
            1 => {
                ui.pointer(&mut r, PointerEvent::Cancelled).unwrap();
            }
            2 => {
                ui.pointer(&mut r, PointerEvent::Moved([900., 700.]))
                    .unwrap();
                assert!(ui.dock_drag().unwrap().preview.is_none());
                ui.pointer(&mut r, PointerEvent::Released([900., 700.]))
                    .unwrap();
            }
            _ => {
                ui.pointer(&mut r, PointerEvent::Left).unwrap();
                assert!(ui.dock_drag().unwrap().preview.is_none());
                ui.pointer(&mut r, PointerEvent::Cancelled).unwrap();
            }
        }
        assert!(ui.dock_drag().is_none());
        assert!(ui.captured_pointer().is_none());
        assert!(drops(&mut r, &e).is_empty());
        assert_eq!(r.update(|cx| e.read(cx).tree.clone()), before);
    }
    let (mut r, e, mut ui, _, _) = setup();
    start(&mut r, &mut ui, "output");
    let b = body(&ui, "output");
    let point = [b.x + 2., b.y + b.height * 0.5];
    ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
    assert!(ui.dock_drag().unwrap().preview.is_none());
    ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    assert!(drops(&mut r, &e).is_empty());
    r.update(|cx| e.update(cx, |s, _| s.minimum = [120., 200.]));
    prepare(&mut r, &mut ui);
    start(&mut r, &mut ui, "preview");
    let b = body(&ui, "editor");
    let point = [b.x + b.width * 0.5, b.y + 2.];
    ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
    assert!(ui.dock_drag().unwrap().preview.is_none());
    ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    assert!(drops(&mut r, &e).is_empty());
}
#[test]
fn release_rechecks_geometry_and_removed_sources_cancel_without_stale_drop() {
    let (mut r, e, mut ui, main, output) = setup();
    start(&mut r, &mut ui, "preview");
    let point = middle(body(&ui, "output"));
    move_to(&mut r, &mut ui, point);
    let final_point = middle(body(&ui, "files"));
    ui.pointer(&mut r, PointerEvent::Released(final_point))
        .unwrap();
    let files = r.update(|cx| e.read(cx).tree.group_for(&key("files")).unwrap());
    assert!(
        matches!(drops(&mut r,&e).last(),Some(DockEvent::Drop {source,target:DockDropTarget::Tab {group,..},..}) if *source==main && *group==files && *group!=output)
    );
    prepare(&mut r, &mut ui);
    start(&mut r, &mut ui, "preview");
    ui.pointer(&mut r, PointerEvent::Moved(middle(body(&ui, "output"))))
        .unwrap();
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.tree.remove(&key("preview"));
        })
    });
    prepare(&mut r, &mut ui);
    assert!(ui.dock_drag().is_none());
    assert!(ui.captured_pointer().is_none());
    ui.pointer(&mut r, PointerEvent::Released(final_point))
        .unwrap();
    assert_eq!(drops(&mut r, &e).len(), 1);
}
#[test]
fn active_drag_is_window_local_and_deferred_drop_checks_original_membership() {
    let (mut r, e, mut a, main, output) = setup();
    let mut b = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut b);
    r.update(|cx| e.update(cx, |s, _| s.accept = false));
    prepare(&mut r, &mut a);
    prepare(&mut r, &mut b);
    start(&mut r, &mut a, "preview");
    let point = middle(body(&a, "output"));
    move_to(&mut r, &mut a, point);
    assert!(b.dock_drag().is_none());
    assert!(b.captured_pointer().is_none());
    a.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    let proposal = drops(&mut r, &e).pop().unwrap();
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.tree.move_panel(&key("preview"), output, 0).unwrap();
        })
    });
    assert_eq!(
        r.update(|cx| e.update(cx, |s, _| s.tree.apply(&proposal))),
        Err(DockError::PanelNotFound)
    );
    assert!(r.update(|cx| e.read(cx).tree.node(main).is_some()));
}

struct Policy {
    tree: DockTree,
    events: Vec<DockEvent>,
    listen: bool,
    draggable: bool,
    threshold: f32,
    prevent: usize,
    block: bool,
    two: bool,
}
fn policy_panel(key: &Key) -> DockPanel {
    let content = if key == &Key::from("outer") {
        tabs()
            .selected("inner 1")
            .tab(tab("inner 1", "Inner 1", label("Nested content")))
            .tab(tab("inner 2", "Inner 2", label("Second nested content")))
            .into_element()
    } else {
        label("Other panel").into_element()
    };
    dock_panel(format!("{key:?}"), content)
}
impl View for Policy {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut first = dock(&self.tree, policy_panel)
            .min_pane_size(40., 40.)
            .draggable(self.draggable)
            .drag_threshold(self.threshold);
        if self.listen {
            first = first.on_event(cx.listener(|s, e: &DockEvent, _| s.events.push(e.clone())));
        }
        let first = first
            .into_element()
            .on_pointer_down_capture(cx.listener(|s, e: &PointerInput, _| {
                if s.prevent == 1 {
                    e.prevent_default();
                }
            }))
            .on_pointer_move_capture(cx.listener(|s, e: &PointerInput, _| {
                if s.prevent == 2 {
                    e.prevent_default();
                }
            }));
        if self.two {
            let second = dock(&self.tree, policy_panel)
                .size(300., 400.)
                .min_pane_size(40., 40.)
                .on_event(cx.listener(|s, e: &DockEvent, _| s.events.push(e.clone())));
            row()
                .size(600., 400.)
                .child(first.size(300., 400.).key("left"))
                .child(second.key("right"))
        } else {
            let mut root = stack().size(600., 400.).child(first.key("dock"));
            if self.block {
                root = root.child(
                    stack()
                        .key("blocker")
                        .absolute()
                        .left(0.)
                        .top(60.)
                        .size(600., 340.)
                        .pointer_events(PointerEvents::Block),
                );
            }
            root
        }
    }
}
fn policy_setup(
    listen: bool,
    draggable: bool,
    prevent: usize,
    two: bool,
) -> (Runtime, Entity<Policy>, Ui<Policy>) {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Policy {
            tree: DockTree::from_panels(["outer", "other"]).unwrap(),
            events: Vec::new(),
            listen,
            draggable,
            threshold: 6.,
            prevent,
            block: false,
            two,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    ui.prepare(&mut r, [600., 400.], &mut Measure).unwrap();
    (r, e, ui)
}
fn policy_header(ui: &Ui<Policy>) -> ElementId {
    ui.semantics()
        .find(|n| n.role == SemanticRole::Tab && n.label == Some("String(\"outer\")"))
        .unwrap()
        .id
}
#[test]
fn disabled_unhandled_or_prevented_gestures_and_nested_tabs_do_not_start_dock_drags() {
    for (listen, draggable, prevent) in [(true, false, 0), (false, true, 0), (true, true, 1)] {
        let (mut r, e, mut ui) = policy_setup(listen, draggable, prevent, false);
        let point = middle(ui.element(policy_header(&ui)).unwrap().bounds);
        ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
        assert!(ui.captured_pointer().is_none());
        ui.pointer(&mut r, PointerEvent::Moved([2., 200.])).unwrap();
        assert!(ui.dock_drag().is_none());
        assert!(r.update(|cx| e.read(cx).events.is_empty()));
    }
    let (mut r, e, mut ui) = policy_setup(true, true, 0, false);
    let nested = ui
        .semantics()
        .find(|n| n.label == Some("Inner 1") && n.role == SemanticRole::Tab)
        .unwrap()
        .id;
    let point = middle(ui.element(nested).unwrap().bounds);
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([2., 200.])).unwrap();
    assert!(ui.captured_pointer().is_none());
    assert!(ui.dock_drag().is_none());
    assert!(r.update(|cx| e.read(cx).events.is_empty()));
    r.update(|cx| e.update(cx, |s, _| s.prevent = 2));
    ui.prepare(&mut r, [600., 400.], &mut Measure).unwrap();
    let point = middle(ui.element(policy_header(&ui)).unwrap().bounds);
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    assert!(ui.captured_pointer().is_some());
    ui.pointer(&mut r, PointerEvent::Moved([2., 200.])).unwrap();
    assert!(ui.dock_drag().is_none());
    assert!(ui.captured_pointer().is_none());
}
#[test]
fn blockers_scope_boundaries_and_disabling_an_active_drag_clear_feedback() {
    let (mut r, e, mut ui) = policy_setup(true, true, 0, false);
    let point = middle(ui.element(policy_header(&ui)).unwrap().bounds);
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([2., 200.])).unwrap();
    assert!(ui.dock_drag().unwrap().preview.is_some());
    r.update(|cx| e.update(cx, |s, _| s.block = true));
    ui.prepare(&mut r, [600., 400.], &mut Measure).unwrap();
    assert!(ui.dock_drag().unwrap().preview.is_none());
    ui.pointer(&mut r, PointerEvent::Released([2., 200.]))
        .unwrap();
    assert!(r.update(|cx| e.read(cx).events.is_empty()));
    r.update(|cx| e.update(cx, |s, _| s.block = false));
    ui.prepare(&mut r, [600., 400.], &mut Measure).unwrap();
    let point = middle(ui.element(policy_header(&ui)).unwrap().bounds);
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([2., 200.])).unwrap();
    r.update(|cx| e.update(cx, |s, _| s.draggable = false));
    ui.prepare(&mut r, [600., 400.], &mut Measure).unwrap();
    assert!(ui.dock_drag().is_none());
    assert!(ui.captured_pointer().is_none());
    let (mut r, e, mut ui) = policy_setup(true, true, 0, true);
    let point = middle(ui.element(policy_header(&ui)).unwrap().bounds);
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([450., 220.]))
        .unwrap();
    assert!(ui.dock_drag().unwrap().preview.is_none());
    ui.pointer(&mut r, PointerEvent::Released([450., 220.]))
        .unwrap();
    assert!(r.update(|cx| e.read(cx).events.is_empty()));
}
#[test]
fn invalid_drag_threshold_is_diagnosed_and_corrected_without_losing_the_tree() {
    let (mut r, e, mut ui) = policy_setup(true, true, 0, false);
    for threshold in [f32::NAN, -1.] {
        r.update(|cx| e.update(cx, |s, _| s.threshold = threshold));
        assert!(matches!(
            ui.prepare(&mut r, [600., 400.], &mut Measure),
            Err(UiError::InvalidDockConfiguration)
        ));
    }
    r.update(|cx| e.update(cx, |s, _| s.threshold = 0.));
    ui.prepare(&mut r, [600., 400.], &mut Measure).unwrap();
    let point = middle(ui.element(policy_header(&ui)).unwrap().bounds);
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([2., 200.])).unwrap();
    assert!(ui.dock_drag().unwrap().preview.is_some());
}

#[test]
fn failing_application_callbacks_cannot_leave_header_capture_stuck() {
    struct Foreign {
        tree: DockTree,
        listener: Listener<DockEvent>,
    }
    impl View for Foreign {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            dock(&self.tree, policy_panel)
                .size(600., 400.)
                .min_pane_size(40., 40.)
                .on_event(self.listener.clone())
        }
    }
    let mut other = Runtime::new();
    let (_owner, mount) = other.update(|cx| {
        let e = cx.new(|_| 0_u32);
        let mount = cx.mount(&e).unwrap();
        (e, mount)
    });
    let listener = other
        .evaluate(&mount, |_, cx| cx.listener(|_, _: &DockEvent, _| {}))
        .unwrap();
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Foreign {
            tree: DockTree::from_panels(["outer", "other"]).unwrap(),
            listener,
        })
    });
    let mut ui = Ui::new(&mut r, e).unwrap();
    ui.prepare(&mut r, [600., 400.], &mut Measure).unwrap();
    for active in [false, true] {
        let id = ui
            .semantics()
            .find(|n| n.role == SemanticRole::Tab && n.label == Some("String(\"outer\")"))
            .unwrap()
            .id;
        let point = middle(ui.element(id).unwrap().bounds);
        ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
        let release = if active { [2., 200.] } else { point };
        if active {
            ui.pointer(&mut r, PointerEvent::Moved(release)).unwrap();
            assert!(ui.dock_drag().unwrap().preview.is_some());
        }
        assert!(matches!(
            ui.pointer(&mut r, PointerEvent::Released(release)),
            Err(UiError::Access(AccessError::WrongRuntime))
        ));
        assert!(ui.dock_drag().is_none());
        assert!(ui.captured_pointer().is_none());
    }
}
