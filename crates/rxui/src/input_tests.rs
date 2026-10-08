use crate::*;
struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
#[derive(Default)]
struct Demo {
    trace: Vec<&'static str>,
    capture: bool,
    hide: bool,
    clicks: usize,
    moves: usize,
    cancels: Vec<PointerCancelReason>,
    prevent: bool,
}
impl View for Demo {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut root = column()
            .size(200., 100.)
            .on_pointer_down_capture(cx.listener(|s, _, _| s.trace.push("root capture")))
            .on_pointer_down(cx.listener(|s, _, _| s.trace.push("root bubble")))
            .on_key_down(cx.listener(|s, e: &KeyInput, _| {
                s.trace.push("root key");
                if s.prevent {
                    e.prevent_default();
                }
            }));
        if !self.hide {
            root = root.child(
                column()
                    .key("parent")
                    .size(100., 100.)
                    .on_pointer_down_capture(cx.listener(|s, _, _| s.trace.push("parent capture")))
                    .on_pointer_down(cx.listener(|s, _, _| s.trace.push("parent bubble")))
                    .child(
                        button("drag")
                            .key("target")
                            .size(50., 30.)
                            .cursor(Cursor::ResizeHorizontal)
                            .on_click(cx.listener(|s, _, _| s.clicks += 1))
                            .on_pointer_down(cx.listener(|s, e: &PointerInput, _| {
                                s.trace.push("target");
                                if s.capture {
                                    e.capture_pointer();
                                    e.focus();
                                }
                                if s.prevent {
                                    e.prevent_default();
                                }
                            }))
                            .on_pointer_move(cx.listener(|s, e: &PointerInput, _| {
                                s.moves += 1;
                                if e.press_position.is_some() {
                                    assert_eq!(
                                        e.local_position,
                                        [e.position[0] - e.bounds.x, e.position[1] - e.bounds.y]
                                    );
                                }
                            }))
                            .on_pointer_cancel(cx.listener(|s, e: &PointerInput, _| {
                                s.cancels.push(e.cancel_reason.unwrap())
                            })),
                    ),
            );
        }
        root
    }
}
fn setup() -> (Runtime, Entity<Demo>, Ui<Demo>) {
    let mut r = Runtime::new();
    let e = r.update(|cx| cx.new(|_| Demo::default()));
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
    (r, e, ui)
}
fn prepare(r: &mut Runtime, ui: &mut Ui<Demo>) {
    ui.prepare(r, [200., 100.], &mut Measure).unwrap();
}
#[test]
fn input_routes_capture_target_bubble_and_preserves_button_defaults() {
    let (mut r, e, mut ui) = setup();
    ui.pointer(&mut r, PointerEvent::Pressed([10., 10.]))
        .unwrap();
    r.update(|cx| {
        assert_eq!(
            e.read(cx).trace,
            vec![
                "root capture",
                "parent capture",
                "target",
                "parent bubble",
                "root bubble"
            ]
        )
    });
    prepare(&mut r, &mut ui);
    ui.pointer(&mut r, PointerEvent::Released([10., 10.]))
        .unwrap();
    r.update(|cx| assert_eq!(e.read(cx).clicks, 1));
}
#[test]
fn explicit_capture_moves_outside_then_escape_removal_and_deactivation_cancel() {
    let (mut r, e, mut ui) = setup();
    r.update(|cx| e.update(cx, |s, _| s.capture = true));
    prepare(&mut r, &mut ui);
    ui.pointer(&mut r, PointerEvent::Pressed([10., 10.]))
        .unwrap();
    let target = ui.captured_pointer().unwrap();
    prepare(&mut r, &mut ui);
    ui.pointer(&mut r, PointerEvent::Moved([180., 80.]))
        .unwrap();
    assert_eq!(ui.captured_pointer(), Some(target));
    assert_eq!(ui.cursor(), Cursor::ResizeHorizontal);
    prepare(&mut r, &mut ui);
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
    assert_eq!(ui.captured_pointer(), None);
    prepare(&mut r, &mut ui);
    ui.pointer(&mut r, PointerEvent::Pressed([10., 10.]))
        .unwrap();
    r.update(|cx| e.update(cx, |s, _| s.hide = true));
    prepare(&mut r, &mut ui);
    assert_eq!(ui.captured_pointer(), None);
    r.update(|cx| {
        let s = e.read(cx);
        assert_eq!(s.moves, 1);
        assert_eq!(
            s.cancels,
            vec![
                PointerCancelReason::Escape,
                PointerCancelReason::TargetUnavailable
            ]
        );
    });
    r.update(|cx| e.update(cx, |s, _| s.hide = false));
    prepare(&mut r, &mut ui);
    ui.pointer(&mut r, PointerEvent::Pressed([10., 10.]))
        .unwrap();
    ui.set_active(false);
    assert!(ui.needs_prepare(&r).unwrap());
    prepare(&mut r, &mut ui);
    r.update(|cx| assert_eq!(e.read(cx).cancels.last(), Some(&PointerCancelReason::Host)));
}
#[test]
fn prevent_default_does_not_stop_route_and_capture_is_per_placement() {
    let (mut r, e, mut ui) = setup();
    let mut other = Ui::new(&mut r, e.clone()).unwrap();
    r.update(|cx| {
        e.update(cx, |s, _| {
            s.capture = true;
            s.prevent = true;
        })
    });
    prepare(&mut r, &mut ui);
    prepare(&mut r, &mut other);
    ui.pointer(&mut r, PointerEvent::Pressed([10., 10.]))
        .unwrap();
    assert!(ui.captured_pointer().is_some());
    assert_eq!(other.captured_pointer(), None);
    prepare(&mut r, &mut ui);
    let result = ui
        .key(
            &mut r,
            KeyEvent {
                key: KeyboardKey::ArrowRight,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            },
        )
        .unwrap();
    assert!(result.default_prevented);
    prepare(&mut r, &mut ui);
    ui.pointer(&mut r, PointerEvent::Released([10., 10.]))
        .unwrap();
    r.update(|cx| {
        let s = e.read(cx);
        assert_eq!(s.clicks, 0);
        assert!(s.trace.contains(&"root bubble"));
    });
}
#[test]
fn secondary_button_does_not_activate_or_change_control_focus() {
    let (mut r, e, mut ui) = setup();
    ui.pointer(
        &mut r,
        PointerEvent::Down {
            position: [10., 10.],
            button: PointerButton::Secondary,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap();
    assert_eq!(ui.focused_element(), None);
    prepare(&mut r, &mut ui);
    ui.pointer(
        &mut r,
        PointerEvent::Up {
            position: [10., 10.],
            button: PointerButton::Secondary,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap();
    r.update(|cx| assert_eq!(e.read(cx).clicks, 0));
}

#[test]
fn stopping_propagation_keeps_button_default_and_custom_focusable_routes_keys() {
    struct Controls {
        trace: Vec<&'static str>,
        clicks: u32,
        mode: u8,
    }
    impl View for Controls {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let mut target = if self.mode == 0 {
                button("Target").on_click(cx.listener(|s, _, _| s.clicks += 1))
            } else {
                column().focusable(true)
            };
            target = target
                .size(100., 40.)
                .on_pointer_down(cx.listener(|s, e: &PointerInput, _| {
                    s.trace.push("target");
                    e.stop_propagation();
                }))
                .on_key_down(cx.listener(|s, e: &KeyInput, _| {
                    s.trace.push("target key");
                    e.stop_propagation();
                    e.prevent_default();
                }));
            column()
                .size(200., 100.)
                .child(target)
                .on_pointer_down(cx.listener(|s, _, _| s.trace.push("root")))
                .on_key_down(cx.listener(|s, _, _| s.trace.push("root key")))
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Controls {
            trace: Vec::new(),
            clicks: 0,
            mode: 0,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    for mode in [0, 1] {
        r.update(|cx| {
            e.update(cx, |s, _| {
                s.mode = mode;
                s.trace.clear();
            })
        });
        ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
        ui.pointer(&mut r, PointerEvent::Pressed([10., 10.]))
            .unwrap();
        assert!(ui.focused_element().is_some());
        ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
        ui.pointer(&mut r, PointerEvent::Released([10., 10.]))
            .unwrap();
        ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
        let result = ui
            .key(
                &mut r,
                KeyEvent {
                    key: KeyboardKey::Character("x".into()),
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::default(),
                },
            )
            .unwrap();
        assert!(result.default_prevented);
        r.update(|cx| {
            let s = e.read(cx);
            assert_eq!(s.trace, ["target", "target key"]);
            assert_eq!(s.clicks, 1);
        });
    }
}
#[test]
fn hidden_inert_and_pointer_disabled_ancestors_cancel_custom_capture() {
    struct Drag {
        unavailable: u8,
        cancels: usize,
    }
    impl View for Drag {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column()
                .size(200., 100.)
                .inert(self.unavailable == 1)
                .pointer_events(if self.unavailable == 2 {
                    PointerEvents::None
                } else {
                    PointerEvents::Auto
                })
                .layout(|s| {
                    if self.unavailable == 3 {
                        s.display = taffy::Display::None;
                    }
                })
                .child(
                    column()
                        .size(100., 40.)
                        .on_pointer_down(cx.listener(|_, e: &PointerInput, _| e.capture_pointer()))
                        .on_pointer_cancel(cx.listener(|s, e: &PointerInput, _| {
                            assert_eq!(
                                e.cancel_reason,
                                Some(PointerCancelReason::TargetUnavailable)
                            );
                            s.cancels += 1;
                        })),
                )
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| {
        cx.new(|_| Drag {
            unavailable: 0,
            cancels: 0,
        })
    });
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    for mode in [1, 2, 3] {
        r.update(|cx| e.update(cx, |s, _| s.unavailable = 0));
        ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
        ui.pointer(&mut r, PointerEvent::Pressed([10., 10.]))
            .unwrap();
        assert!(ui.captured_pointer().is_some());
        r.update(|cx| e.update(cx, |s, _| s.unavailable = mode));
        ui.prepare(&mut r, [200., 100.], &mut Measure).unwrap();
        assert_eq!(ui.captured_pointer(), None);
    }
    r.update(|cx| assert_eq!(e.read(cx).cancels, 3));
}
#[test]
fn capture_requested_during_secondary_motion_releases_on_secondary_up() {
    struct Drag;
    impl View for Drag {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column()
                .size(100., 100.)
                .on_pointer_move(cx.listener(|_, e: &PointerInput, _| {
                    if e.buttons.contains(PointerButton::Secondary) {
                        e.capture_pointer();
                    }
                }))
        }
    }
    let mut r = Runtime::new();
    let e = r.update(|cx| cx.new(|_| Drag));
    let mut ui = Ui::new(&mut r, e).unwrap();
    ui.prepare(&mut r, [100.; 2], &mut Measure).unwrap();
    ui.pointer(
        &mut r,
        PointerEvent::Down {
            position: [10.; 2],
            button: PointerButton::Secondary,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap();
    ui.pointer(&mut r, PointerEvent::Moved([20.; 2])).unwrap();
    assert!(ui.captured_pointer().is_some());
    ui.pointer(
        &mut r,
        PointerEvent::Up {
            position: [200.; 2],
            button: PointerButton::Secondary,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap();
    assert_eq!(ui.captured_pointer(), None);
}
#[derive(Default)]
struct Hovering {
    log: Vec<(&'static str, bool)>,
    wheel: Vec<(&'static str, [f32; 2])>,
    clicks: Vec<u8>,
    prevent_wheel: bool,
    show_inner: bool,
}
impl View for Hovering {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut outer = column()
            .key("outer")
            .size(100., 100.)
            .scroll_y()
            .on_hover(cx.listener(|s, e: &HoverEvent, _| s.log.push(("outer", e.hovered))))
            .on_wheel(cx.listener(|s, e: &WheelInput, _| {
                s.wheel.push(("outer", e.delta));
            }))
            .on_pointer_down(cx.listener(|s, e: &PointerInput, _| s.clicks.push(e.click_count)))
            .child(column().height(300.));
        if self.show_inner {
            outer = outer.child(
                column()
                    .key("inner")
                    .absolute()
                    .size(40., 40.)
                    .on_hover(cx.listener(|s, e: &HoverEvent, _| s.log.push(("inner", e.hovered))))
                    .on_wheel(cx.listener(|s, e: &WheelInput, _| {
                        s.wheel.push(("inner", e.delta));
                        if s.prevent_wheel {
                            e.prevent_default();
                            e.stop_propagation();
                        }
                    })),
            );
        }
        row().size(200., 200.).child(outer)
    }
}
#[test]
fn hover_enter_and_leave_follow_the_hit_subtree_and_removal() {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Hovering {
            show_inner: true,
            ..Default::default()
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [200., 200.], &mut Measure)
        .unwrap();
    let take = |runtime: &mut Runtime| {
        runtime.update(|cx| root.update(cx, |s, _| std::mem::take(&mut s.log)))
    };
    ui.pointer(&mut runtime, PointerEvent::Moved([10., 10.]))
        .unwrap();
    assert_eq!(take(&mut runtime), [("outer", true), ("inner", true)]);
    ui.pointer(&mut runtime, PointerEvent::Moved([12., 12.]))
        .unwrap();
    assert_eq!(take(&mut runtime), []);
    ui.pointer(&mut runtime, PointerEvent::Moved([60., 60.]))
        .unwrap();
    assert_eq!(take(&mut runtime), [("inner", false)]);
    ui.pointer(&mut runtime, PointerEvent::Moved([150., 150.]))
        .unwrap();
    assert_eq!(take(&mut runtime), [("outer", false)]);
    ui.pointer(&mut runtime, PointerEvent::Moved([10., 10.]))
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Left).unwrap();
    assert_eq!(
        take(&mut runtime),
        [
            ("outer", true),
            ("inner", true),
            ("inner", false),
            ("outer", false)
        ]
    );
    // Removing a hovered element delivers its leave during preparation.
    ui.pointer(&mut runtime, PointerEvent::Moved([10., 10.]))
        .unwrap();
    take(&mut runtime);
    runtime.update(|cx| root.update(cx, |s, _| s.show_inner = false));
    ui.prepare(&mut runtime, [200., 200.], &mut Measure)
        .unwrap();
    assert_eq!(take(&mut runtime), [("inner", false)]);
    // Losing native activation leaves everything.
    ui.set_active(false);
    assert!(ui.needs_prepare(&runtime).unwrap());
    ui.prepare(&mut runtime, [200., 200.], &mut Measure)
        .unwrap();
    assert_eq!(take(&mut runtime), [("outer", false)]);
}
#[test]
fn wheel_listeners_bubble_and_can_prevent_default_scrolling() {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Hovering {
            show_inner: true,
            ..Default::default()
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [200., 200.], &mut Measure)
        .unwrap();
    let offset = |ui: &Ui<Hovering>| {
        ui.elements()
            .find(|e| e.key == Some(&"outer".into()))
            .unwrap()
            .scroll_offset[1]
    };
    let result = ui
        .wheel(&mut runtime, [10., 10.], [0., 30.], Modifiers::default())
        .unwrap();
    assert!(result.changed && !result.default_prevented);
    assert_eq!(offset(&ui), 30.);
    assert_eq!(
        runtime.update(|cx| root.read(cx).wheel.clone()),
        [("inner", [0., 30.]), ("outer", [0., 30.])]
    );
    runtime.update(|cx| {
        root.update(cx, |s, _| {
            s.prevent_wheel = true;
            s.wheel.clear();
        })
    });
    ui.prepare(&mut runtime, [200., 200.], &mut Measure)
        .unwrap();
    // The inner box scrolled up by 30 along with the content.
    let result = ui
        .wheel(&mut runtime, [10., 5.], [0., 30.], Modifiers::default())
        .unwrap();
    assert!(result.default_prevented);
    assert_eq!(offset(&ui), 30.);
    assert_eq!(
        runtime.update(|cx| root.read(cx).wheel.clone()),
        [("inner", [0., 30.])]
    );
    assert!(matches!(
        ui.wheel(&mut runtime, [f32::NAN, 0.], [0., 1.], Modifiers::default()),
        Err(UiError::InvalidGeometry)
    ));
}
#[test]
fn pointer_payloads_carry_the_host_click_count_on_primary_presses() {
    struct Measure2;
    impl TextMeasure for Measure2 {
        fn measure(&mut self, _: ElementId, _: TextRequest<'_>) -> Result<[f32; 2], UiError> {
            Ok([0.; 2])
        }
    }
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| cx.new(|_| Hovering::default()));
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [200., 200.], &mut Measure2)
        .unwrap();
    for count in [1, 2, 3] {
        ui.pointer_with_text_clicks(
            &mut runtime,
            PointerEvent::Pressed([60., 60.]),
            &mut Measure2,
            false,
            count,
        )
        .unwrap();
        ui.pointer(&mut runtime, PointerEvent::Released([60., 60.]))
            .unwrap();
    }
    ui.pointer(&mut runtime, PointerEvent::Pressed([60., 60.]))
        .unwrap();
    assert_eq!(
        runtime.update(|cx| root.read(cx).clicks.clone()),
        [1, 2, 3, 1]
    );
}

#[test]
fn unchanged_pointer_listener_needs_no_evaluation_or_redraw() {
    struct Tracker {
        dragging: bool,
        moves: u32,
    }
    impl View for Tracker {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column()
                .size(100., 100.)
                .on_pointer_move(cx.listener(|this, _: &PointerInput, cx| {
                    if this.dragging {
                        this.moves += 1;
                    } else {
                        cx.unchanged();
                    }
                }))
        }
    }
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Tracker {
            dragging: false,
            moves: 0,
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [100., 100.], &mut Measure)
        .unwrap();
    ui.pointer(&mut runtime, PointerEvent::Moved([10., 10.]))
        .unwrap();
    ui.prepare(&mut runtime, [100., 100.], &mut Measure)
        .unwrap();
    let stats = ui.stats();
    assert!(
        !ui.pointer(&mut runtime, PointerEvent::Moved([20., 10.]))
            .unwrap()
    );
    assert!(!ui.needs_prepare(&runtime).unwrap());
    runtime.update(|cx| root.update(cx, |this, _| this.dragging = true));
    ui.prepare(&mut runtime, [100., 100.], &mut Measure)
        .unwrap();
    assert!(
        ui.pointer(&mut runtime, PointerEvent::Moved([30., 10.]))
            .unwrap()
    );
    assert!(ui.needs_prepare(&runtime).unwrap());
    ui.prepare(&mut runtime, [100., 100.], &mut Measure)
        .unwrap();
    assert_eq!(
        ui.stats().component_evaluations,
        stats.component_evaluations + 2
    );
    assert_eq!(runtime.update(|cx| root.read(cx).moves), 1);
}
