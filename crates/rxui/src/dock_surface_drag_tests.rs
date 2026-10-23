use crate::*;

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
struct Surfaces {
    workspace: DockWorkspace,
    source: DockSurfaceId,
    target: DockSurfaceId,
    bind: bool,
    same_scope: bool,
    deny_source: bool,
    deny_target: bool,
    block: bool,
    accept: bool,
    releases: usize,
    prevent_release: bool,
    fail_release: bool,
    transfers: Vec<DockTransferEvent>,
    detaches: Vec<DockDetachEvent>,
}
impl View for Surfaces {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let deny_source = self.deny_source;
        let deny_target = self.deny_target;
        let mut source = dock(self.workspace.surface(self.source).unwrap(), |key| {
            dock_panel(format!("{key:?}"), label("Source content"))
        })
        .size(300., 300.)
        .min_pane_size(30., 30.)
        .surface(self.source)
        .transfer_scope("workspace")
        .on_transfer(cx.listener(|s, e: &DockTransferEvent, _| {
            s.transfers.push(e.clone());
            if s.accept {
                s.workspace
                    .transfer(
                        e.source_surface,
                        e.source_group,
                        &e.panel,
                        e.target_surface,
                        e.target,
                    )
                    .unwrap();
            }
        }))
        .on_detach(cx.listener(|s, e: &DockDetachEvent, _| s.detaches.push(e.clone())))
        .drop_policy(move |_| !deny_source);
        // A transfer-only source must not require an ordinary on_event listener.
        if !self.bind {
            source = dock(self.workspace.surface(self.source).unwrap(), |key| {
                dock_panel(format!("{key:?}"), label("Source content"))
            })
            .size(300., 300.)
            .on_event(cx.listener(|_, _: &DockEvent, _| {}));
        }
        let target = dock(self.workspace.surface(self.target).unwrap(), |key| {
            dock_panel(format!("{key:?}"), label("Target content"))
        })
        .size(300., 300.)
        .min_pane_size(30., 30.)
        .surface(self.target)
        .transfer_scope(if self.same_scope {
            "workspace"
        } else {
            "other"
        })
        .drop_policy(move |_| !deny_target);
        let mut root = stack().size(800., 400.).child(
            row()
                .size(600., 300.)
                .child(source.key("source"))
                .child(target.key("target")),
        );
        if self.block {
            root = root.child(
                stack()
                    .absolute()
                    .left(600.)
                    .top(0.)
                    .size(200., 400.)
                    .pointer_events(PointerEvents::Block),
            );
        }
        let root = root.on_pointer_up(cx.listener(|s, e: &PointerInput, _| {
            s.releases += 1;
            if s.prevent_release {
                e.prevent_default();
            }
        }));
        if self.fail_release {
            root.on_pointer_up(Listener::from_callback(|_, _| {
                Err(AccessError::WrongRuntime)
            }))
        } else {
            root
        }
    }
}
fn setup() -> (Runtime, Entity<Surfaces>, Ui<Surfaces>) {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| {
            let mut workspace = DockWorkspace::new();
            let source = workspace
                .create_surface(DockTree::from_panels(["a", "b"]).unwrap())
                .unwrap();
            let target = workspace
                .create_surface(DockTree::from_panels(["c"]).unwrap())
                .unwrap();
            Surfaces {
                workspace,
                source,
                target,
                bind: true,
                same_scope: true,
                deny_source: false,
                deny_target: false,
                block: false,
                accept: false,
                releases: 0,
                prevent_release: false,
                fail_release: false,
                transfers: Vec::new(),
                detaches: Vec::new(),
            }
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    (r, e, ui)
}
fn prepare(r: &mut Runtime, ui: &mut Ui<Surfaces>) {
    ui.prepare(r, [800., 400.], &mut Measure).unwrap();
}
fn header(ui: &Ui<Surfaces>, name: &str) -> ElementId {
    let label = format!("String(\"{name}\")");
    ui.semantics()
        .find(|n| n.role == SemanticRole::Tab && n.label == Some(label.as_str()))
        .unwrap()
        .id
}
fn start(r: &mut Runtime, ui: &mut Ui<Surfaces>) {
    let b = ui.element(header(ui, "a")).unwrap().bounds;
    ui.pointer(
        r,
        PointerEvent::Pressed([b.x + b.width * 0.5, b.y + b.height * 0.5]),
    )
    .unwrap();
    assert!(ui.captured_pointer().is_some());
}
#[test]
fn cross_region_transfer_is_controlled_and_focus_follows_an_accepted_transfer() {
    for accept in [false, true] {
        let (mut r, e, mut ui) = setup();
        r.update(|cx| e.update(cx, |s, _| s.accept = accept));
        prepare(&mut r, &mut ui);
        let before = r.update(|cx| e.read(cx).workspace.clone());
        start(&mut r, &mut ui);
        ui.pointer(&mut r, PointerEvent::Moved([450., 170.]))
            .unwrap();
        assert!(ui.dock_drag().unwrap().preview.is_some());
        assert!(r.update(|cx| e.read(cx).transfers.is_empty()));
        assert_eq!(r.update(|cx| e.read(cx).workspace.clone()), before);
        ui.pointer(&mut r, PointerEvent::Released([450., 170.]))
            .unwrap();
        assert!(ui.captured_pointer().is_none());
        prepare(&mut r, &mut ui);
        r.update(|cx| {
            let s = e.read(cx);
            assert_eq!(s.transfers.len(), 1);
            assert_eq!(s.transfers[0].source_surface, s.source);
            assert_eq!(s.transfers[0].target_surface, s.target);
            assert!(s.detaches.is_empty());
            assert_eq!(
                s.workspace.surface_for(&Key::from("a")),
                Some(if accept { s.target } else { s.source })
            );
        });
        if accept {
            assert_eq!(ui.focused_element(), Some(header(&ui, "a")));
        }
    }
}
#[test]
fn absent_bindings_scope_mismatch_and_both_policies_reject_without_detaching() {
    for mode in 0..4 {
        let (mut r, e, mut ui) = setup();
        r.update(|cx| {
            e.update(cx, |s, _| match mode {
                0 => s.bind = false,
                1 => s.same_scope = false,
                2 => s.deny_source = true,
                _ => s.deny_target = true,
            })
        });
        prepare(&mut r, &mut ui);
        start(&mut r, &mut ui);
        ui.pointer(&mut r, PointerEvent::Moved([450., 170.]))
            .unwrap();
        assert!(ui.dock_drag().unwrap().preview.is_none());
        ui.pointer(&mut r, PointerEvent::Released([450., 170.]))
            .unwrap();
        assert!(r.update(|cx| {
            let s = e.read(cx);
            s.transfers.is_empty() && s.detaches.is_empty()
        }));
    }
}
#[test]
fn detach_requires_unblocked_outside_release_and_cancellation_never_detaches() {
    for mode in 0..4 {
        let (mut r, e, mut ui) = setup();
        if mode == 1 {
            r.update(|cx| e.update(cx, |s, _| s.block = true));
            prepare(&mut r, &mut ui);
        }
        start(&mut r, &mut ui);
        ui.pointer(&mut r, PointerEvent::Moved([700., 200.]))
            .unwrap();
        assert!(r.update(|cx| e.read(cx).detaches.is_empty()));
        if mode == 2 {
            ui.pointer(&mut r, PointerEvent::Cancelled).unwrap();
        } else if mode == 3 {
            ui.key(
                &mut r,
                KeyEvent {
                    key: KeyboardKey::Escape,
                    physical_key: Default::default(),
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
            )
            .unwrap();
        }
        ui.pointer(&mut r, PointerEvent::Released([700., 200.]))
            .unwrap();
        assert!(ui.captured_pointer().is_none());
        r.update(|cx| {
            let s = e.read(cx);
            assert_eq!(s.detaches.len(), usize::from(mode == 0));
            if let Some(event) = s.detaches.first() {
                assert_eq!(event.position, [700., 200.]);
            }
            assert!(s.transfers.is_empty());
        });
    }
}
#[test]
fn release_rechecks_destination_policy_and_changed_source_binding_cancels() {
    for rebind in [false, true] {
        let (mut r, e, mut ui) = setup();
        start(&mut r, &mut ui);
        ui.pointer(&mut r, PointerEvent::Moved([450., 170.]))
            .unwrap();
        assert!(ui.dock_drag().unwrap().preview.is_some());
        r.update(|cx| {
            e.update(cx, |s, _| {
                if rebind {
                    s.bind = false
                } else {
                    s.deny_target = true
                }
            })
        });
        prepare(&mut r, &mut ui);
        if rebind {
            assert!(ui.dock_drag().is_none());
        }
        ui.pointer(&mut r, PointerEvent::Released([450., 170.]))
            .unwrap();
        assert!(r.update(|cx| e.read(cx).transfers.is_empty()));
    }
}

#[test]
fn explicit_host_bridge_queries_another_ui_and_consumes_release_before_callback() {
    let (mut r, e, mut source) = setup();
    r.update(|cx| e.update(cx, |s, _| s.accept = true));
    prepare(&mut r, &mut source);
    let mut target = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut target);
    start(&mut r, &mut source);
    source
        .pointer(&mut r, PointerEvent::Moved([700., 200.]))
        .unwrap();
    source.pointer(&mut r, PointerEvent::Left).unwrap();
    let snapshot = source.dock_drag_session().unwrap();
    assert_eq!(snapshot.panel(), &Key::from("a"));
    assert!(
        target
            .external_dock_preview(&snapshot, [450., 170.])
            .is_some()
    );
    assert!(
        !target
            .drop_external_dock(&mut r, snapshot, [450., 170.])
            .unwrap()
            .changed
    );
    assert!(r.update(|cx| e.read(cx).transfers.is_empty()));
    assert!(source.take_dock_drag_session().is_none());
    assert!(source.prepare_external_dock_release());
    source
        .pointer(&mut r, PointerEvent::Released([700., 200.]))
        .unwrap();
    let release = source.take_dock_drag_session().unwrap();
    assert!(source.dock_drag().is_none());
    assert!(source.captured_pointer().is_none());
    assert!(
        target
            .drop_external_dock(&mut r, release, [450., 170.])
            .unwrap()
            .changed
    );
    prepare(&mut r, &mut target);
    assert_eq!(target.focused_element(), Some(header(&target, "a")));
    r.update(|cx| {
        let s = e.read(cx);
        assert_eq!(s.transfers.len(), 1);
        assert!(s.detaches.is_empty());
        assert_eq!(s.workspace.surface_for(&Key::from("a")), Some(s.target));
    });
}

#[test]
fn external_release_rechecks_target_and_source_cancellation_cannot_yield_a_token() {
    let (mut r, e, mut source) = setup();
    let mut target = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut target);
    start(&mut r, &mut source);
    source
        .pointer(&mut r, PointerEvent::Moved([700., 200.]))
        .unwrap();
    let snapshot = source.dock_drag_session().unwrap();
    assert!(
        target
            .external_dock_preview(&snapshot, [450., 170.])
            .is_some()
    );
    assert!(
        target
            .external_dock_preview(&snapshot, [f32::NAN, 170.])
            .is_none()
    );
    assert!(source.take_dock_drag_session().is_none());
    assert!(source.prepare_external_dock_release());
    source
        .pointer(&mut r, PointerEvent::Released([700., 200.]))
        .unwrap();
    let release = source.take_dock_drag_session().unwrap();
    r.update(|cx| e.update(cx, |s, _| s.deny_target = true));
    prepare(&mut r, &mut target);
    assert!(
        !target
            .drop_external_dock(&mut r, release, [450., 170.])
            .unwrap()
            .changed
    );
    assert!(r.update(|cx| e.read(cx).transfers.is_empty()));
    source
        .pointer(&mut r, PointerEvent::Released([700., 200.]))
        .unwrap();
    prepare(&mut r, &mut source);
    start(&mut r, &mut source);
    source
        .pointer(&mut r, PointerEvent::Moved([700., 200.]))
        .unwrap();
    source.pointer(&mut r, PointerEvent::Cancelled).unwrap();
    assert!(source.take_dock_drag_session().is_none());
}

#[test]
fn cross_region_header_split_and_empty_surface_destinations_are_checked() {
    for mode in 0..3 {
        let (mut r, e, mut ui) = setup();
        r.update(|cx| {
            e.update(cx, |s, _| {
                s.accept = true;
                if mode == 2 {
                    let group = s.workspace.surface(s.target).unwrap().root().id();
                    s.workspace
                        .apply(
                            s.target,
                            &DockEvent::Close {
                                group,
                                panel: Key::from("c"),
                            },
                        )
                        .unwrap();
                }
            })
        });
        prepare(&mut r, &mut ui);
        let target_group = r.update(|cx| {
            let s = e.read(cx);
            s.workspace.surface(s.target).unwrap().root().id()
        });
        let point = match mode {
            0 => {
                let b = ui.element(header(&ui, "c")).unwrap().bounds;
                [b.x + 1., b.y + b.height * 0.5]
            }
            1 => [302., 170.],
            _ => [450., 170.],
        };
        start(&mut r, &mut ui);
        ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
        let preview = ui.dock_drag().unwrap().preview.unwrap();
        match mode {
            0 | 2 => assert_eq!(
                preview.target,
                DockDropTarget::Tab {
                    group: target_group,
                    index: 0
                }
            ),
            _ => assert!(
                matches!(preview.target, DockDropTarget::Split { group, side: DockSide::Left, .. } if group == target_group)
            ),
        }
        ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
        r.update(|cx| {
            let s = e.read(cx);
            assert_eq!(s.workspace.surface_for(&Key::from("a")), Some(s.target));
        });
    }
}

#[test]
fn external_release_routes_to_source_capture_and_can_be_prevented() {
    for prevent in [false, true] {
        let (mut r, e, mut source) = setup();
        r.update(|cx| e.update(cx, |s, _| s.prevent_release = prevent));
        prepare(&mut r, &mut source);
        start(&mut r, &mut source);
        source
            .pointer(&mut r, PointerEvent::Moved([700., 200.]))
            .unwrap();
        assert!(source.prepare_external_dock_release());
        assert!(source.take_dock_drag_session().is_none());
        source
            .pointer(&mut r, PointerEvent::Released([900., 500.]))
            .unwrap();
        assert_eq!(r.update(|cx| e.read(cx).releases), 1);
        assert!(source.captured_pointer().is_none());
        assert!(source.dock_drag().is_none());
        assert_eq!(source.take_dock_drag_session().is_some(), !prevent);
        assert!(r.update(|cx| e.read(cx).detaches.is_empty()));
    }
}

// Keep nested-content regression coverage separate from the host bridge tests.
mod nested_target_blockers {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    enum Content {
        Shield,
        ShieldWithButton,
        DisabledButton,
        DisabledAncestor,
        Button,
        Plain,
    }
    impl Content {
        fn blocked(self) -> bool {
            matches!(
                self,
                Self::Shield
                    | Self::ShieldWithButton
                    | Self::DisabledButton
                    | Self::DisabledAncestor
            )
        }
        fn element(self) -> Element {
            let child = button("Interactive child").size(160., 80.).key("hit");
            match self {
                Self::Shield => stack()
                    .size(160., 80.)
                    .key("hit")
                    .pointer_events(PointerEvents::Block),
                Self::ShieldWithButton => stack()
                    .size(160., 80.)
                    .pointer_events(PointerEvents::Block)
                    .child(child),
                Self::DisabledButton => child.disabled(true),
                Self::DisabledAncestor => {
                    button(stack().size(160., 80.).key("hit").focusable(true))
                        .size(200., 110.)
                        .disabled(true)
                }
                Self::Button => child,
                Self::Plain => stack().size(160., 80.).key("hit"),
            }
        }
    }
    struct Nested {
        source: DockTree,
        target: DockTree,
        source_id: DockSurfaceId,
        target_id: DockSurfaceId,
        content: Content,
        transfers: usize,
        detaches: usize,
    }
    impl View for Nested {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let source = dock(&self.source, |key| {
                dock_panel(format!("{key:?}"), label("Source"))
            })
            .size(300., 300.)
            .surface(self.source_id)
            .transfer_scope("nested blockers")
            .on_transfer(cx.listener(|s, _: &DockTransferEvent, _| s.transfers += 1))
            .on_detach(cx.listener(|s, _: &DockDetachEvent, _| s.detaches += 1));
            let target = dock(&self.target, |key| {
                dock_panel(format!("{key:?}"), stack().child(self.content.element())).closable(true)
            })
            .size(300., 300.)
            .surface(self.target_id)
            .transfer_scope("nested blockers")
            .on_event(cx.listener(|_, _: &DockEvent, _| {}));
            row().size(600., 300.).child(source).child(target)
        }
    }
    fn setup(content: Content) -> (Runtime, Entity<Nested>, Ui<Nested>) {
        let mut r = Runtime::new();
        let e = r.update(|cx| {
            cx.new(|_| {
                let mut workspace = DockWorkspace::new();
                let source = DockTree::from_panels(["a", "b"]).unwrap();
                let target = DockTree::from_panels(["c"]).unwrap();
                Nested {
                    source_id: workspace.create_surface(source.clone()).unwrap(),
                    target_id: workspace.create_surface(target.clone()).unwrap(),
                    source,
                    target,
                    content,
                    transfers: 0,
                    detaches: 0,
                }
            })
        });
        let mut ui = Ui::new(&mut r, e.clone()).unwrap();
        prepare(&mut r, &mut ui);
        (r, e, ui)
    }
    fn prepare(r: &mut Runtime, ui: &mut Ui<Nested>) {
        ui.prepare(r, [600., 300.], &mut Measure).unwrap();
    }
    fn center(b: Bounds) -> [f32; 2] {
        [b.x + b.width * 0.5, b.y + b.height * 0.5]
    }
    fn start(r: &mut Runtime, ui: &mut Ui<Nested>) {
        let header = ui
            .semantics()
            .find(|n| n.role == SemanticRole::Tab && n.label == Some("String(\"a\")"))
            .unwrap()
            .id;
        let point = center(ui.element(header).unwrap().bounds);
        ui.pointer(r, PointerEvent::Pressed(point)).unwrap();
        ui.pointer(r, PointerEvent::Moved([point[0] + 12., point[1]]))
            .unwrap();
        assert!(ui.dock_drag_session().is_some());
    }
    fn target_point(ui: &Ui<Nested>, hit: usize) -> [f32; 2] {
        let id = match hit {
            0 => {
                ui.elements()
                    .find(|e| e.key == Some(&Key::from("hit")))
                    .unwrap()
                    .id
            }
            1 => {
                ui.semantics()
                    .find(|n| n.role == SemanticRole::Tab && n.label == Some("String(\"c\")"))
                    .unwrap()
                    .id
            }
            _ => {
                ui.semantics()
                    .find(|n| {
                        n.role == SemanticRole::Button
                            && n.label.is_some_and(|label| label.contains("Close"))
                    })
                    .unwrap()
                    .id
            }
        };
        center(ui.element(id).unwrap().bounds)
    }
    #[test]
    fn release_rechecks_a_new_blocker_nested_in_previously_eligible_content() {
        for content in [Content::ShieldWithButton, Content::DisabledAncestor] {
            for external in [false, true] {
                let (mut r, e, mut source) = setup(Content::Button);
                let mut destination = Ui::new(&mut r, e.clone()).unwrap();
                prepare(&mut r, &mut destination);
                let point = target_point(&source, 0);
                start(&mut r, &mut source);
                if external {
                    let snapshot = source.dock_drag_session().unwrap();
                    assert!(
                        destination
                            .external_dock_preview(&snapshot, point)
                            .is_some()
                    );
                    assert!(source.prepare_external_dock_release());
                    source
                        .pointer(&mut r, PointerEvent::Released(point))
                        .unwrap();
                    let release = source.take_dock_drag_session().unwrap();
                    r.update(|cx| e.update(cx, |s, _| s.content = content));
                    prepare(&mut r, &mut destination);
                    assert!(
                        !destination
                            .drop_external_dock(&mut r, release, point)
                            .unwrap()
                            .changed
                    );
                } else {
                    source.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
                    assert!(source.dock_drag().unwrap().preview.is_some());
                    r.update(|cx| e.update(cx, |s, _| s.content = content));
                    prepare(&mut r, &mut source);
                    source
                        .pointer(&mut r, PointerEvent::Released(point))
                        .unwrap();
                }
                r.update(|cx| {
                    let s = e.read(cx);
                    assert_eq!(s.transfers, 0, "{content:?}, external {external}");
                    assert_eq!(s.detaches, 0);
                });
            }
        }
    }
    #[test]
    fn nested_content_blocks_same_ui_and_cross_ui_targets_but_controls_remain_eligible() {
        for content in [
            Content::Shield,
            Content::ShieldWithButton,
            Content::DisabledButton,
            Content::DisabledAncestor,
            Content::Button,
            Content::Plain,
        ] {
            // Also exercise headers and their close buttons above blocked content.
            for hit in 0..3 {
                for external in [false, true] {
                    let (mut r, e, mut source) = setup(content);
                    let mut destination = Ui::new(&mut r, e.clone()).unwrap();
                    prepare(&mut r, &mut destination);
                    let point = target_point(if external { &destination } else { &source }, hit);
                    let allowed = hit != 0 || !content.blocked();
                    start(&mut r, &mut source);
                    if external {
                        let snapshot = source.dock_drag_session().unwrap();
                        assert_eq!(
                            destination
                                .external_dock_preview(&snapshot, point)
                                .is_some(),
                            allowed,
                            "{content:?}, hit {hit}"
                        );
                        assert!(source.prepare_external_dock_release());
                        source
                            .pointer(&mut r, PointerEvent::Released(point))
                            .unwrap();
                        let release = source.take_dock_drag_session().unwrap();
                        assert_eq!(
                            destination
                                .drop_external_dock(&mut r, release, point)
                                .unwrap()
                                .changed,
                            allowed,
                            "{content:?}, hit {hit}"
                        );
                    } else {
                        source.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
                        assert_eq!(
                            source.dock_drag().unwrap().preview.is_some(),
                            allowed,
                            "{content:?}, hit {hit}"
                        );
                        source
                            .pointer(&mut r, PointerEvent::Released(point))
                            .unwrap();
                    }
                    r.update(|cx| {
                        let s = e.read(cx);
                        assert_eq!(
                            s.transfers,
                            usize::from(allowed),
                            "{content:?}, hit {hit}, external {external}"
                        );
                        assert_eq!(s.detaches, 0);
                        // Listeners deliberately ignore the controlled proposal.
                        assert!(s.source.group_for(&Key::from("a")).is_some());
                        assert!(s.source.group_for(&Key::from("b")).is_some());
                        assert!(s.target.group_for(&Key::from("a")).is_none());
                    });
                }
            }
        }
    }
}

#[test]
fn external_release_errors_and_cancelled_releases_cannot_leave_an_armed_gesture() {
    for cancelled in [false, true] {
        let (mut r, e, mut ui) = setup();
        r.update(|cx| e.update(cx, |s, _| s.fail_release = true));
        prepare(&mut r, &mut ui);
        start(&mut r, &mut ui);
        ui.pointer(&mut r, PointerEvent::Moved([700., 200.]))
            .unwrap();
        assert!(ui.prepare_external_dock_release());
        let event = if cancelled {
            PointerEvent::CancelledUp {
                position: [700., 200.],
                button: PointerButton::Primary,
                modifiers: Modifiers::default(),
            }
        } else {
            PointerEvent::Released([700., 200.])
        };
        assert!(matches!(
            ui.pointer(&mut r, event),
            Err(UiError::Access(AccessError::WrongRuntime))
        ));
        assert!(ui.dock_drag().is_none());
        assert!(ui.captured_pointer().is_none());
        assert!(ui.take_dock_drag_session().is_none());
        r.update(|cx| e.update(cx, |s, _| s.fail_release = false));
        prepare(&mut r, &mut ui);
        ui.pointer(&mut r, PointerEvent::Released([700., 200.]))
            .unwrap();
        assert!(ui.take_dock_drag_session().is_none());
    }
}

#[test]
fn untaken_external_release_expires_on_source_input_and_deactivation() {
    for mode in 0..5 {
        let (mut r, _, mut ui) = setup();
        start(&mut r, &mut ui);
        ui.pointer(&mut r, PointerEvent::Moved([700., 200.]))
            .unwrap();
        assert!(ui.prepare_external_dock_release());
        ui.pointer(&mut r, PointerEvent::Released([700., 200.]))
            .unwrap();
        match mode {
            0 => {
                ui.key(
                    &mut r,
                    KeyEvent {
                        key: KeyboardKey::Escape,
                        physical_key: PhysicalKey::Unidentified,
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::default(),
                    },
                )
                .unwrap();
            }
            1 => {
                ui.wheel(&mut r, [700., 200.], [0., 1.], Modifiers::default())
                    .unwrap();
            }
            2 => {
                ui.set_active(false);
            }
            3 => {
                ui.pointer(&mut r, PointerEvent::Moved([700., 201.]))
                    .unwrap();
            }
            _ => {
                ui.pointer(&mut r, PointerEvent::Cancelled).unwrap();
            }
        }
        assert!(ui.take_dock_drag_session().is_none(), "mode {mode}");
    }
}
