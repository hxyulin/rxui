use crate::*;
use std::{cell::Cell, rc::Rc};
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 7., 18.])
    }
}
fn prepare<T: View>(r: &mut Runtime, ui: &mut Ui<T>) {
    ui.prepare(r, [400., 300.], &mut Measure).unwrap();
}
fn id<T: View>(ui: &Ui<T>, key: &str) -> ElementId {
    ui.elements()
        .find(|e| e.key == Some(&Key::from(key)))
        .unwrap()
        .id
}
fn center<T: View>(ui: &Ui<T>, key: &str) -> [f32; 2] {
    let b = ui.element(id(ui, key)).unwrap().bounds;
    [b.x + b.width / 2., b.y + b.height / 2.]
}
fn key(key: KeyboardKey) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}
fn chord() -> KeyEvent {
    let s = Shortcut::primary("k");
    KeyEvent {
        key: s.key,
        modifiers: s.modifiers,
        ..key(KeyboardKey::Escape)
    }
}
struct Run;
impl Command for Run {}
struct Other;
impl Command for Other {}
#[derive(Default)]
struct Commands {
    trace: Vec<&'static str>,
    enabled: bool,
    prevent: bool,
    duplicate: bool,
    repeat: bool,
}
impl View for Commands {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let outer = cx
            .command(Run, |s, _, _| s.trace.push("outer"))
            .label("Outer")
            .shortcut(Shortcut::primary("k"));
        let inner = cx
            .command(Run, |s, _, _| s.trace.push("inner"))
            .label("Inner")
            .enabled(self.enabled)
            .shortcut(Shortcut::primary("k"))
            .repeat(self.repeat);
        let mut scope = column()
            .key("scope")
            .on_command(inner.clone())
            .child(text_input("Edit").key("edit"))
            .child(inner.button().key("invoke"));
        if self.duplicate {
            scope = scope.on_command(inner);
        }
        column()
            .size(400., 300.)
            .on_command(outer)
            .on_key_down_capture(cx.listener(|s, e: &KeyInput, _| {
                if s.prevent {
                    e.prevent_default();
                }
            }))
            .child(scope)
            .child(button("Outside").key("outside"))
    }
}
fn commands() -> (Runtime, Entity<Commands>, Ui<Commands>) {
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Commands {
            enabled: true,
            ..Default::default()
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    (r, e, ui)
}
#[test]
fn typed_commands_shortcuts_and_buttons_share_actions_with_nearest_scope_shadowing() {
    let (mut r, e, mut ui) = commands();
    ui.focus(id(&ui, "edit"));
    assert_eq!(
        ui.dispatch_command::<Run>(&mut r).unwrap(),
        CommandStatus::Handled
    );
    prepare(&mut r, &mut ui);
    assert!(ui.key(&mut r, chord()).unwrap().default_prevented);
    prepare(&mut r, &mut ui);
    ui.semantic_action(
        &mut r,
        SemanticAction::Activate(id(&ui, "invoke")),
        &mut Measure,
    )
    .unwrap();
    prepare(&mut r, &mut ui);
    assert_eq!(
        r.update(|cx| e.read(cx).trace.clone()),
        vec!["inner", "inner", "inner"]
    );
    r.update(|cx| e.update(cx, |s, _| s.enabled = false));
    prepare(&mut r, &mut ui);
    assert_eq!(
        ui.dispatch_command::<Run>(&mut r).unwrap(),
        CommandStatus::Disabled
    );
    assert!(ui.key(&mut r, chord()).unwrap().default_prevented);
    prepare(&mut r, &mut ui);
    assert!(!ui.semantic_node(id(&ui, "invoke")).unwrap().activatable);
    ui.focus(id(&ui, "outside"));
    assert_eq!(
        ui.dispatch_command::<Run>(&mut r).unwrap(),
        CommandStatus::Handled
    );
    assert_eq!(
        r.update(|cx| e.read(cx).trace.clone()),
        vec!["inner", "inner", "inner", "outer"]
    );
}
#[test]
fn key_prevention_repeat_exact_modifiers_and_duplicate_scope_validation() {
    let (mut r, e, mut ui) = commands();
    ui.focus(id(&ui, "edit"));
    let mut event = chord();
    event.repeat = true;
    assert!(ui.key(&mut r, event.clone()).unwrap().default_prevented);
    assert!(r.update(|cx| e.read(cx).trace.is_empty()));
    event.repeat = false;
    event.modifiers.alt = true;
    assert!(!ui.key(&mut r, event).unwrap().default_prevented);
    r.update(|cx| e.update(cx, |s, _| s.repeat = true));
    prepare(&mut r, &mut ui);
    let mut event = chord();
    event.repeat = true;
    ui.key(&mut r, event).unwrap();
    assert_eq!(r.update(|cx| e.read(cx).trace.len()), 1);
    r.update(|cx| e.update(cx, |s, _| s.prevent = true));
    prepare(&mut r, &mut ui);
    ui.key(&mut r, chord()).unwrap();
    assert_eq!(r.update(|cx| e.read(cx).trace.len()), 1);
    r.update(|cx| e.update(cx, |s, _| s.duplicate = true));
    assert!(matches!(
        ui.prepare(&mut r, [400., 300.], &mut Measure),
        Err(UiError::AmbiguousCommand)
    ));
    r.update(|cx| e.update(cx, |s, _| s.duplicate = false));
    prepare(&mut r, &mut ui);
}
#[test]
fn application_fallback_registration_is_explicit_replaceable_and_weak() {
    let (mut r, e, ui) = commands();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let (action, registration) = r.update(|cx| {
        let action = cx
            .command(Other, move |_, _| count.set(count.get() + 1))
            .shortcut(Shortcut::primary("o"));
        let registration = cx.register_command(&action);
        (action, registration)
    });
    assert_eq!(
        ui.dispatch_command::<Other>(&mut r).unwrap(),
        CommandStatus::Handled
    );
    assert_eq!(calls.get(), 1);
    let disabled = action.clone().enabled(false);
    r.update(|cx| registration.replace(&disabled, cx)).unwrap();
    assert_eq!(
        ui.dispatch_command::<Other>(&mut r).unwrap(),
        CommandStatus::Disabled
    );
    let mut foreign = Runtime::new();
    assert_eq!(
        foreign.update(|cx| registration.replace(&action, cx)),
        Err(AccessError::WrongRuntime)
    );
    assert_eq!(
        foreign.update(|cx| action.invoke(cx)),
        Err(AccessError::WrongRuntime)
    );
    drop(registration);
    assert_eq!(
        ui.dispatch_command::<Other>(&mut r).unwrap(),
        CommandStatus::Unhandled
    );
    let mount = r.update(|cx| cx.mount(&e).unwrap());
    let bound = r
        .evaluate(&mount, |_, cx| cx.command(Other, |_, _, _| {}))
        .unwrap();
    drop(mount);
    assert_eq!(
        r.update(|cx| bound.invoke(cx)).unwrap(),
        CommandStatus::Unhandled
    );
}
struct OverlayPage {
    anchor: AnchorHandle,
    open: bool,
    dialog: bool,
    nested: bool,
    reject: bool,
    hidden: bool,
    duplicate: bool,
    point: Option<[f32; 2]>,
    clicked: usize,
    dismiss: Vec<DismissReason>,
    position: PopoverPlacement,
    offset: f32,
}
impl Default for OverlayPage {
    fn default() -> Self {
        Self {
            anchor: AnchorHandle::new(),
            open: false,
            dialog: false,
            nested: false,
            reject: false,
            hidden: false,
            duplicate: false,
            point: None,
            clicked: 0,
            dismiss: Vec::new(),
            position: Default::default(),
            offset: 0.,
        }
    }
}
impl View for OverlayPage {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let run = cx.command(Run, |s, _, _| s.clicked += 1).label("Run");
        let off = cx
            .command(Other, |_, _, _| panic!("disabled menu item"))
            .label("Unavailable")
            .enabled(false);
        let mut clipped = column()
            .key("clipped")
            .size(120., 50.)
            .clip()
            .absolute()
            .left(270.)
            .top(240. - self.offset)
            .child(
                button("Open")
                    .key("anchor")
                    .size(80., 30.)
                    .anchor_handle(self.anchor.clone())
                    .on_click(cx.listener(|s, _, _| s.open = true)),
            );
        if self.hidden {
            clipped = clipped.inert(true);
        }
        if self.open {
            let anchor = self
                .point
                .map(OverlayAnchor::Point)
                .unwrap_or_else(|| OverlayAnchor::Element(self.anchor.clone()));
            let overlay = popover(
                anchor,
                menu()
                    .child(menu_item(&run).key("first"))
                    .child(menu_item(&off).key("disabled"))
                    .child(menu_item(&run).key("last")),
            )
            .key("popup")
            .width(180.)
            .placement(self.position)
            .on_dismiss(cx.listener(|s, e: &DismissEvent, _| {
                s.dismiss.push(e.reason);
                if !s.reject {
                    s.open = false;
                }
            }));
            clipped = clipped.child(overlay);
        }
        let mut root = stack()
            .size(400., 300.)
            .on_command(run.clone())
            .child(clipped)
            .child(
                button("Background")
                    .key("background")
                    .absolute()
                    .left(10.)
                    .top(10.)
                    .on_click(cx.listener(|s, _, _| s.clicked += 100)),
            );
        if self.duplicate {
            root = root.child(label("Duplicate").anchor_handle(self.anchor.clone()));
        }
        if self.dialog {
            let mut content = column()
                .gap(8.)
                .child(
                    button("Confirm")
                        .key("confirm")
                        .on_click(cx.listener(|s, _, _| s.dialog = false)),
                )
                .child(
                    button("Cancel")
                        .key("cancel")
                        .on_click(cx.listener(|s, _, _| s.dialog = false)),
                );
            if self.nested {
                content = content.child(
                    modal(
                        button("Nested close")
                            .key("nested-close")
                            .on_click(cx.listener(|s, _, _| s.nested = false)),
                    )
                    .key("nested")
                    .width(160.)
                    .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.nested = false)),
                );
            }
            root = root.child(
                modal(content)
                    .key("dialog")
                    .width(220.)
                    .accessibility_label("Confirmation")
                    .on_dismiss(cx.listener(|s, e: &DismissEvent, _| {
                        s.dismiss.push(e.reason);
                        if !s.reject {
                            s.dialog = false;
                        }
                    })),
            );
        }
        root
    }
}
fn overlays() -> (Runtime, Entity<OverlayPage>, Ui<OverlayPage>) {
    let mut r = Runtime::new();
    let e = r.update(|cx| cx.new(|_| OverlayPage::default()));
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    (r, e, ui)
}
#[test]
fn popover_escapes_ancestor_clip_flips_constrains_and_restores_focus_without_click_through() {
    let (mut r, e, mut ui) = overlays();
    let opener = id(&ui, "anchor");
    ui.focus(opener);
    r.update(|cx| e.update(cx, |s, _| s.open = true));
    prepare(&mut r, &mut ui);
    let popup = ui.element(id(&ui, "popup")).unwrap();
    let surface = ui
        .element(ui.semantic_node(popup.id).unwrap().children[0])
        .unwrap();
    assert!(surface.bounds.y + surface.bounds.height <= 240.);
    assert!(surface.bounds.x + surface.bounds.width <= 392.);
    assert_eq!(popup.clip_bounds.x, 0.);
    assert_eq!(ui.focused_element(), Some(id(&ui, "first")));
    assert_eq!(ui.hit_test(center(&ui, "first")), Some(id(&ui, "first")));
    let mut event = key(KeyboardKey::ArrowDown);
    assert!(ui.key(&mut r, event.clone()).unwrap().default_prevented);
    assert_eq!(ui.focused_element(), Some(id(&ui, "last")));
    event.key = KeyboardKey::ArrowDown;
    ui.key(&mut r, event).unwrap();
    assert_eq!(ui.focused_element(), Some(id(&ui, "first")));
    let point = center(&ui, "background");
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    prepare(&mut r, &mut ui);
    ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    assert_eq!(r.update(|cx| e.read(cx).clicked), 0);
    assert_eq!(
        r.update(|cx| e.read(cx).dismiss.clone()),
        vec![DismissReason::OutsidePointer]
    );
    assert_eq!(ui.focused_element(), Some(opener));
    r.update(|cx| e.update(cx, |s, _| s.open = true));
    prepare(&mut r, &mut ui);
    assert!(
        ui.key(&mut r, key(KeyboardKey::Escape))
            .unwrap()
            .default_prevented
    );
    prepare(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(opener));
}
#[test]
fn rejected_dismissal_keeps_overlay_and_missing_anchor_notifies_once_until_recovered() {
    let (mut r, e, mut ui) = overlays();
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.open = true;
            s.reject = true;
        })
    });
    prepare(&mut r, &mut ui);
    ui.key(&mut r, key(KeyboardKey::Escape)).unwrap();
    prepare(&mut r, &mut ui);
    assert!(r.update(|cx| e.read(cx).open));
    r.update(|cx| e.update(cx, |s, _| s.hidden = true));
    prepare(&mut r, &mut ui);
    assert!(ui.elements().all(|n| n.key != Some(&Key::from("popup"))));
    prepare(&mut r, &mut ui);
    assert_eq!(
        r.update(|cx| e.read(cx).dismiss.clone()),
        vec![DismissReason::Escape, DismissReason::AnchorUnavailable]
    );
    r.update(|cx| e.update(cx, |s, _| s.hidden = false));
    prepare(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(id(&ui, "first")));
}
#[test]
fn modal_blocks_background_semantics_commands_pointer_and_programmatic_focus_and_restores_nested_focus()
 {
    let (mut r, e, mut ui) = overlays();
    let opener = id(&ui, "background");
    ui.focus(opener);
    r.update(|cx| e.update(cx, |s, _| s.dialog = true));
    prepare(&mut r, &mut ui);
    let confirm = id(&ui, "confirm");
    assert_eq!(ui.focused_element(), Some(confirm));
    assert!(!ui.focus(opener));
    assert!(ui.semantic_node(opener).is_none());
    assert!(
        !ui.semantic_action(&mut r, SemanticAction::Activate(opener), &mut Measure)
            .unwrap()
    );
    assert_eq!(
        ui.dispatch_command::<Run>(&mut r).unwrap(),
        CommandStatus::Unhandled
    );
    assert!(
        ui.semantics()
            .any(|n| n.role == SemanticRole::Dialog && n.modal && n.label == Some("Confirmation"))
    );
    ui.focus_next(false);
    assert_eq!(ui.focused_element(), Some(id(&ui, "cancel")));
    ui.focus_next(false);
    assert_eq!(ui.focused_element(), Some(confirm));
    r.update(|cx| e.update(cx, |s, _| s.nested = true));
    prepare(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(id(&ui, "nested-close")));
    ui.key(&mut r, key(KeyboardKey::Escape)).unwrap();
    prepare(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(confirm));
    ui.key(&mut r, key(KeyboardKey::Escape)).unwrap();
    prepare(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(opener));
}
#[test]
fn anchor_and_overlay_validation_retry_and_point_anchors_are_window_local() {
    let (mut r, e, mut ui) = overlays();
    let mut second = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut second);
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.open = true;
            s.point = Some([398., 298.]);
        })
    });
    prepare(&mut r, &mut ui);
    prepare(&mut r, &mut second);
    let a = ui.element(id(&ui, "popup")).unwrap();
    let b = second.element(id(&second, "popup")).unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(
        ui.anchor_bounds(&r.update(|cx| e.read(cx).anchor.clone())),
        second.anchor_bounds(&r.update(|cx| e.read(cx).anchor.clone()))
    );
    r.update(|cx| e.update(cx, |s, _| s.point = Some([f32::NAN, 0.])));
    assert!(matches!(
        ui.prepare(&mut r, [400., 300.], &mut Measure),
        Err(UiError::InvalidOverlay)
    ));
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.point = None;
            s.duplicate = true;
        })
    });
    assert!(matches!(
        ui.prepare(&mut r, [400., 300.], &mut Measure),
        Err(UiError::DuplicateAnchorHandle)
    ));
    r.update(|cx| e.update(cx, |s, _| s.duplicate = false));
    prepare(&mut r, &mut ui);
}

#[test]
fn dock_context_requests_do_not_select_or_drag_and_have_keyboard_equivalents() {
    struct DockPage {
        tree: DockTree,
        requests: Vec<DockContextEvent>,
    }
    impl View for DockPage {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            dock(&self.tree, |key| {
                dock_panel(format!("{key:?}"), label("Content"))
            })
            .draggable(false)
            .on_context_menu(cx.listener(|s, e: &DockContextEvent, _| s.requests.push(e.clone())))
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| DockPage {
            tree: DockTree::from_panels(["a", "b"]).unwrap(),
            requests: Vec::new(),
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    let header = ui
        .semantics()
        .find(|n| n.role == SemanticRole::Tab && n.label == Some("String(\"b\")"))
        .unwrap()
        .id;
    let b = ui.element(header).unwrap().bounds;
    let point = [b.x + b.width / 2., b.y + b.height / 2.];
    ui.pointer(
        &mut r,
        PointerEvent::Down {
            position: point,
            button: PointerButton::Secondary,
            modifiers: Default::default(),
        },
    )
    .unwrap();
    assert_eq!(ui.focused_element(), Some(header));
    assert!(ui.dock_drag().is_none());
    assert!(ui.captured_pointer().is_none());
    prepare(&mut r, &mut ui);
    ui.pointer(
        &mut r,
        PointerEvent::Up {
            position: point,
            button: PointerButton::Secondary,
            modifiers: Default::default(),
        },
    )
    .unwrap();
    ui.key(
        &mut r,
        KeyEvent {
            key: KeyboardKey::Other("F10".into()),
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            ..key(KeyboardKey::Escape)
        },
    )
    .unwrap();
    assert_eq!(r.update(|cx| e.read(cx).requests.len()), 2);
    r.update(|cx| {
        let s = e.read(cx);
        assert_eq!(
            s.tree.root().tabs().unwrap().selected(),
            Some(&Key::from("a"))
        );
        assert_eq!(s.requests[0].panel, Key::from("b"));
        assert_eq!(s.requests[0].position, point);
        assert_eq!(s.requests[1].position, [b.x, b.y + b.height]);
    });
}

#[test]
fn scrolling_repositions_anchors_without_layout_or_owner_updates_and_hidden_anchors_request_prepare()
 {
    struct Scrolling {
        anchor: AnchorHandle,
        dismiss: usize,
    }
    impl View for Scrolling {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column().size(400., 300.).child(
                column()
                    .key("scroll")
                    .size(120., 80.)
                    .scroll_y()
                    .child(
                        button("Anchor")
                            .key("anchor")
                            .size(100., 30.)
                            .anchor_handle(self.anchor.clone()),
                    )
                    .child(label("Long content").height(240.))
                    .child(
                        popover(self.anchor.clone(), button("Popup"))
                            .key("popup")
                            .width(150.)
                            .on_dismiss(cx.listener(|s, e: &DismissEvent, _| {
                                assert_eq!(e.reason, DismissReason::AnchorUnavailable);
                                s.dismiss += 1;
                            })),
                    ),
            )
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Scrolling {
            anchor: AnchorHandle::new(),
            dismiss: 0,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    let surface = ui.semantic_node(id(&ui, "popup")).unwrap().children[0];
    let old = ui.element(surface).unwrap().bounds;
    let revision = r.revision(&e).unwrap();
    let stats = ui.stats();
    let scroll = id(&ui, "scroll");
    ui.semantic_action(
        &mut r,
        SemanticAction::Scroll {
            target: scroll,
            offset: [0., 20.],
        },
        &mut Measure,
    )
    .unwrap();
    assert_eq!(ui.element(surface).unwrap().bounds.y, old.y - 20.);
    assert_eq!(ui.stats().layout_passes, stats.layout_passes);
    assert_eq!(
        ui.stats().component_evaluations,
        stats.component_evaluations
    );
    assert_eq!(r.revision(&e).unwrap(), revision);
    ui.semantic_action(
        &mut r,
        SemanticAction::Scroll {
            target: scroll,
            offset: [0., 100.],
        },
        &mut Measure,
    )
    .unwrap();
    assert!(ui.needs_prepare(&r).unwrap());
    prepare(&mut r, &mut ui);
    assert_eq!(r.update(|cx| e.read(cx).dismiss), 1);
    prepare(&mut r, &mut ui);
    assert_eq!(r.update(|cx| e.read(cx).dismiss), 1);
}

#[cfg(feature = "accessibility")]
#[test]
fn native_semantic_tree_lifts_overlays_and_preserves_bounds_and_modal_state() {
    use accesskit::TreeId;
    let (mut r, e, mut ui) = overlays();
    r.update(|cx| e.update(cx, |s, _| s.open = true));
    prepare(&mut r, &mut ui);
    let expected = ui.element(id(&ui, "first")).unwrap().bounds;
    let mut cache = AccessKitTree::new();
    let update = cache.update(&ui, "Overlays", 2.).unwrap().unwrap();
    let item = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == accesskit::Role::MenuItem && n.label() == Some("Run"))
        .unwrap()
        .0;
    let consumer = accesskit_consumer::Tree::new(update, true);
    let actual = consumer
        .state()
        .node_by_tree_local_id(item, TreeId::ROOT)
        .unwrap()
        .bounding_box()
        .unwrap();
    assert!((actual.x0 - f64::from(expected.x) * 2.).abs() < 0.01);
    assert!((actual.y0 - f64::from(expected.y) * 2.).abs() < 0.01);
    assert_eq!(actual.height(), f64::from(expected.height) * 2.);
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.open = false;
            s.dialog = true;
        })
    });
    prepare(&mut r, &mut ui);
    cache.reset();
    let update = cache.update(&ui, "Overlays", 2.).unwrap().unwrap();
    assert!(
        update
            .nodes
            .iter()
            .any(|(_, n)| n.role() == accesskit::Role::Dialog && n.is_modal())
    );
    accesskit_consumer::Tree::new(update, true);
}

#[test]
fn menu_to_dialog_handoff_restores_the_original_opener_and_global_commands_can_opt_in() {
    let (mut r, e, mut ui) = overlays();
    let opener = id(&ui, "anchor");
    assert!(ui.focused_element().is_none());
    r.update(|cx| e.update(cx, |s, _| s.open = true));
    prepare(&mut r, &mut ui);
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.open = false;
            s.dialog = true;
        })
    });
    prepare(&mut r, &mut ui);
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    let (help, registration) = r.update(|cx| {
        let help = cx.command(Other, move |_, _| calls.set(calls.get() + 1));
        let registration = cx.register_command(&help);
        (help, registration)
    });
    assert_eq!(
        ui.dispatch_command::<Other>(&mut r).unwrap(),
        CommandStatus::Unhandled
    );
    r.update(|cx| registration.replace(&help.allow_in_modal(true), cx))
        .unwrap();
    assert_eq!(
        ui.dispatch_command::<Other>(&mut r).unwrap(),
        CommandStatus::Handled
    );
    assert_eq!(count.get(), 1);
    ui.key(&mut r, key(KeyboardKey::Escape)).unwrap();
    prepare(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(opener));
}

#[test]
fn unrelated_popovers_cannot_paint_over_a_modal_declared_before_them() {
    struct Page;
    impl View for Page {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            stack()
                .size(400., 300.)
                .child(modal(button("Confirm")).key("modal"))
                .child(popover([10., 10.], button("Other")).key("popover"))
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| cx.new(|_| Page));
    let mut ui = Ui::new(&mut r, e).unwrap();
    prepare(&mut r, &mut ui);
    let order: Vec<_> = ui.elements().map(|n| n.id).collect();
    assert!(
        order
            .iter()
            .position(|item| *item == id(&ui, "modal"))
            .unwrap()
            > order
                .iter()
                .position(|item| *item == id(&ui, "popover"))
                .unwrap()
    );
}

#[test]
fn pointer_transparent_overlays_preserve_background_input_and_idle_layout_is_cached() {
    struct Page(usize);
    impl View for Page {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            stack()
                .size(400., 300.)
                .child(
                    button("Background")
                        .key("background")
                        .size(400., 300.)
                        .on_click(cx.listener(|s, _, _| s.0 += 1)),
                )
                .child(
                    popover([10., 10.], label("Passive"))
                        .key("popup")
                        .autofocus(false)
                        .into_element()
                        .pointer_events(PointerEvents::None),
                )
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| cx.new(|_| Page(0)));
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    let stats = ui.stats();
    for _ in 0..8 {
        prepare(&mut r, &mut ui);
    }
    assert_eq!(ui.stats(), stats);
    ui.pointer(&mut r, PointerEvent::Pressed([300., 250.]))
        .unwrap();
    ui.pointer(&mut r, PointerEvent::Released([300., 250.]))
        .unwrap();
    assert_eq!(r.update(|cx| e.read(cx).0), 1);
    prepare(&mut r, &mut ui);
    assert!(ui.elements().any(|n| n.key == Some(&Key::from("popup"))));
}

#[test]
fn command_queries_follow_focus_state_registration_replacement_and_disposal() {
    let (mut r, e, mut ui) = commands();
    ui.focus(id(&ui, "edit"));
    assert_eq!(ui.query_command::<Run>().unwrap().label, "Inner");
    r.update(|cx| e.update(cx, |s, _| s.enabled = false));
    prepare(&mut r, &mut ui);
    assert!(!ui.query_command_id(CommandId::of::<Run>()).unwrap().enabled);
    ui.focus(id(&ui, "outside"));
    assert_eq!(ui.query_command::<Run>().unwrap().label, "Outer");
    let registration = r.update(|cx| {
        let action = cx.command(Other, |_, _| {}).label("Application");
        cx.register_command(&action)
    });
    assert_eq!(
        r.update(|cx| cx.query_command::<Other>().unwrap().label),
        "Application"
    );
    r.update(|cx| {
        let action = cx.command(Other, |_, _| {}).label("Updated").enabled(false);
        registration.replace(&action, cx).unwrap();
        assert_eq!(
            cx.dispatch_command::<Other>().unwrap(),
            CommandStatus::Disabled
        );
    });
    assert_eq!(ui.query_command::<Other>().unwrap().label, "Updated");
    let mount = r.update(|cx| cx.mount(&e).unwrap());
    let bound = r
        .evaluate(&mount, |_, cx| {
            cx.command(Other, |_, _, _| {}).label("Temporary")
        })
        .unwrap();
    let temporary = r.update(|cx| cx.register_command(&bound));
    assert_eq!(ui.query_command::<Other>().unwrap().label, "Temporary");
    drop(mount);
    assert_eq!(ui.query_command::<Other>().unwrap().label, "Updated");
    drop((temporary, registration));
    assert!(ui.query_command::<Other>().is_none());
}

#[test]
fn command_queries_respect_modal_scopes_and_explicit_global_permissions() {
    let (mut r, e, mut ui) = overlays();
    let registration = r.update(|cx| {
        let action = cx.command(Other, |_, _| {}).label("Global");
        cx.register_command(&action)
    });
    r.update(|cx| e.update(cx, |s, _| s.dialog = true));
    prepare(&mut r, &mut ui);
    assert!(ui.query_command::<Run>().is_none());
    assert!(ui.query_command::<Other>().is_none());
    r.update(|cx| {
        let action = cx
            .command(Other, |_, _| {})
            .label("Help")
            .allow_in_modal(true);
        registration.replace(&action, cx).unwrap();
    });
    assert_eq!(ui.query_command::<Other>().unwrap().label, "Help");
}

#[cfg(feature = "native")]
#[test]
fn native_commands_use_retained_focus_during_menu_tracking_without_reactivating_ui() {
    let (mut r, e, mut ui) = commands();
    ui.focus(id(&ui, "edit"));
    ui.set_active(false);
    assert_eq!(
        ui.dispatch_command::<Run>(&mut r).unwrap(),
        CommandStatus::Unhandled
    );
    assert_eq!(
        ui.dispatch_command_id(&mut r, CommandId::of::<Run>())
            .unwrap(),
        CommandStatus::Handled
    );
    assert_eq!(r.update(|cx| e.read(cx).trace.clone()), vec!["inner"]);
    prepare(&mut r, &mut ui);
    use standard_commands::*;
    assert!(
        ui.native_command_info(CommandId::of::<SelectAll>())
            .unwrap()
            .enabled
    );
    assert!(
        !ui.native_command_info(CommandId::of::<Copy>())
            .unwrap()
            .enabled
    );
    // This example input has no on_change, so it is selectable but not editable.
    assert!(
        !ui.native_command_info(CommandId::of::<Paste>())
            .unwrap()
            .enabled
    );
    ui.native_text_input(&mut r, TextInputEvent::SelectAll, &mut Measure)
        .unwrap();
    assert_eq!(ui.selected_text(), Some("Edit"));
    assert!(
        ui.native_command_info(CommandId::of::<Copy>())
            .unwrap()
            .enabled
    );
    assert!(!ui.has_text_focus()); // No persistent native reactivation.
    assert!(
        !ui.native_command_info(CommandId::of::<Cut>())
            .unwrap()
            .enabled
    );
    ui.focus(id(&ui, "outside"));
    assert!(
        !ui.native_command_info(CommandId::of::<SelectAll>())
            .unwrap()
            .enabled
    );
    let (mut r, e, mut ui) = overlays();
    r.update(|cx| e.update(cx, |s, _| s.dialog = true));
    prepare(&mut r, &mut ui);
    assert!(
        !ui.native_command_info(CommandId::of::<CloseWindow>())
            .unwrap()
            .enabled
    );
    assert!(
        ui.native_command_info(CommandId::of::<Quit>())
            .unwrap()
            .enabled
    );
}
