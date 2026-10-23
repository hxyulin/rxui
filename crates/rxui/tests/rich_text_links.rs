//! Headless contracts of rich text links: geometry, pointer routing, focus
//! traversal, keyboard and assistive activation, painting state and semantics.
//!
//! These live in their own test binary: each test view type adds layout
//! monomorphizations, and the library's unit-test object is near Mach-O limits.
#![cfg(feature = "layout")]
use rxui::*;

/// Monospaced lines: 10 units per byte, 20 per line, lines split at '\n'.
#[derive(Default)]
struct Measure {
    rects: usize,
}
impl Measure {
    fn position(text: &str, offset: usize) -> [f32; 2] {
        let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
        let line = text[..offset].matches('\n').count();
        [(offset - line_start) as f32 * 10., line as f32 * 20.]
    }
}
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        let lines = request.text.split('\n');
        let width = lines.clone().map(str::len).max().unwrap_or(0);
        Ok([width as f32 * 10., lines.count() as f32 * 20.])
    }
    fn supports_text_geometry(&self) -> bool {
        true
    }
    fn text_hit_test(
        &mut self,
        _: ElementId,
        request: TextRequest<'_>,
        point: [f32; 2],
    ) -> Result<Option<TextPosition>, UiError> {
        let line = (point[1].max(0.) / 20.) as usize;
        let start: usize = request
            .text
            .split('\n')
            .take(line)
            .map(|l| l.len() + 1)
            .sum::<usize>()
            .min(request.text.len());
        let len = request.text[start..]
            .find('\n')
            .unwrap_or(request.text.len() - start);
        let column = ((point[0].max(0.) / 10.).round() as usize).min(len);
        Ok(Some(TextPosition::new(start + column)))
    }
    fn text_caret(
        &mut self,
        _: ElementId,
        request: TextRequest<'_>,
        p: TextPosition,
    ) -> Result<Option<Bounds>, UiError> {
        let [x, y] = Self::position(request.text, p.byte_offset);
        Ok(Some(Bounds {
            x,
            y,
            width: 1.,
            height: 20.,
        }))
    }
    fn text_range_rects(
        &mut self,
        _: ElementId,
        request: TextRequest<'_>,
        range: std::ops::Range<usize>,
    ) -> Result<Option<Vec<Bounds>>, UiError> {
        self.rects += 1;
        let mut rects = Vec::new();
        let mut start = range.start;
        while start < range.end {
            let end = request.text[start..range.end]
                .find('\n')
                .map_or(range.end, |i| start + i);
            let [x, y] = Self::position(request.text, start);
            rects.push(Bounds {
                x,
                y,
                width: (end - start) as f32 * 10.,
                height: 20.,
            });
            start = end + 1;
        }
        Ok(Some(rects))
    }
}
fn keyed<T: View>(ui: &Ui<T>, key: &str) -> ElementId {
    ui.elements()
        .find(|e| e.key == Some(&Key::from(key)))
        .unwrap()
        .id
}
fn press_key<T: View>(
    r: &mut Runtime,
    ui: &mut Ui<T>,
    key: KeyboardKey,
    repeat: bool,
) -> InputResult {
    ui.key(
        r,
        KeyEvent {
            physical_key: Default::default(),
            key,
            pressed: true,
            repeat,
            modifiers: Modifiers::default(),
        },
    )
    .unwrap()
}

/// Hover documentation: "See Vec and  and u8 for details." with three links, one
/// empty, between two buttons. Every text byte is 10 units wide.
struct Doc {
    opened: Vec<usize>,
    text: &'static str,
    selectable: bool,
    links: bool,
}
impl Doc {
    fn new() -> Self {
        Self {
            opened: Vec::new(),
            text: "Vec",
            selectable: false,
            links: true,
        }
    }
}
impl View for Doc {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let open = cx.listener(|this, event: &LinkEvent, _| this.opened.push(event.index));
        let spans = if self.links {
            vec![
                span("See "),
                link(self.text, open.clone())
                    .link_url("https://doc.rust-lang.org/std/vec/struct.Vec.html")
                    .link_description("Opens the Vec documentation"),
                span(" and "),
                link("", open.clone()),
                span(" and "),
                span("u8").monospace().link(open),
                span(" for details."),
            ]
        } else {
            vec![span("See "), span(self.text).bold(), span(" for details.")]
        };
        column()
            .child(button("Before").key("before"))
            .child(
                rich_text(spans)
                    .key("doc")
                    .width(400.)
                    .selectable(self.selectable),
            )
            .child(button("After").key("after"))
    }
}
fn setup(doc: Doc) -> (Runtime, Entity<Doc>, Ui<Doc>, Measure) {
    let mut runtime = Runtime::new();
    let entity = runtime.update(|cx| cx.new(|_| doc));
    let mut ui = Ui::new(&mut runtime, entity.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [600., 300.], &mut measure)
        .unwrap();
    (runtime, entity, ui, measure)
}
/// Window point over the byte at `offset` of the rich text's first line.
fn over<T: View>(ui: &Ui<T>, offset: usize) -> [f32; 2] {
    let content = ui.element(keyed(ui, "doc")).unwrap().content_bounds;
    [content.x + offset as f32 * 10. + 5., content.y + 10.]
}
fn opened(runtime: &mut Runtime, entity: &Entity<Doc>) -> Vec<usize> {
    runtime.update(|cx| entity.read(cx).opened.clone())
}

#[test]
fn links_are_pointer_targets_with_hover_cursor_and_click_activation() {
    let (mut r, entity, mut ui, mut measure) = setup(Doc::new());
    let doc = keyed(&ui, "doc");
    // "See " is plain text: no target, no link, default cursor.
    assert!(
        !ui.pointer(&mut r, PointerEvent::Moved(over(&ui, 1)))
            .unwrap()
    );
    assert_eq!(ui.hovered_link(), None);
    assert_eq!(ui.cursor(), Cursor::Default);
    assert_eq!(ui.hit_test(over(&ui, 1)), None);
    // "Vec" is link 0; hovering is paint and cursor state only.
    let stats = ui.stats();
    assert!(
        ui.pointer(&mut r, PointerEvent::Moved(over(&ui, 5)))
            .unwrap()
    );
    assert_eq!(ui.hovered_link(), Some((doc, 0)));
    assert_eq!(ui.cursor(), Cursor::Pointer);
    assert_eq!(ui.hit_test(over(&ui, 5)), Some(doc));
    let info = ui.element(doc).unwrap().links.unwrap();
    assert_eq!(info.hovered, Some(0));
    assert_eq!(info.ranges[0], 4..7);
    assert!(info.active_color.is_some(), "accent links recolor on hover");
    assert!(!ui.needs_prepare(&r).unwrap());
    assert_eq!(ui.stats(), stats);
    // Moving within the same link changes nothing.
    assert!(
        !ui.pointer(&mut r, PointerEvent::Moved(over(&ui, 6)))
            .unwrap()
    );
    // "u8" is link 2: the empty link 1 takes no index position on screen.
    let u8_offset = "See Vec and  and ".len();
    ui.pointer(&mut r, PointerEvent::Moved(over(&ui, u8_offset)))
        .unwrap();
    assert_eq!(ui.hovered_link(), Some((doc, 2)));
    // Press and release on the same link activates it; the link takes focus
    // without a visible ring after pointer input.
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Pressed(over(&ui, u8_offset)),
        &mut measure,
        false,
    )
    .unwrap();
    assert_eq!(ui.focused_element(), Some(doc));
    assert_eq!(ui.focused_link(), Some(2));
    assert!(!ui.element(doc).unwrap().focus_visible);
    assert_eq!(ui.element(doc).unwrap().links.unwrap().pressed, Some(2));
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Released(over(&ui, u8_offset + 1)),
        &mut measure,
        false,
    )
    .unwrap();
    assert_eq!(opened(&mut r, &entity), [2]);
    // Releasing over another link or plain text does not activate.
    ui.pointer(&mut r, PointerEvent::Pressed(over(&ui, 5)))
        .unwrap();
    ui.pointer(&mut r, PointerEvent::Released(over(&ui, u8_offset)))
        .unwrap();
    ui.pointer(&mut r, PointerEvent::Pressed(over(&ui, 5)))
        .unwrap();
    ui.pointer(&mut r, PointerEvent::Released(over(&ui, 1)))
        .unwrap();
    assert_eq!(opened(&mut r, &entity), [2]);
    // A press on plain text passes through like a label: focus leaves the link.
    ui.pointer(&mut r, PointerEvent::Pressed(over(&ui, 1)))
        .unwrap();
    assert_eq!(ui.focused_element(), None);
    assert_eq!(ui.focused_link(), None);
    ui.pointer(&mut r, PointerEvent::Released(over(&ui, 1)))
        .unwrap();
    // Leaving the window and cancellation clear hover.
    ui.pointer(&mut r, PointerEvent::Moved(over(&ui, 5)))
        .unwrap();
    assert!(ui.pointer(&mut r, PointerEvent::Left).unwrap());
    assert_eq!(ui.hovered_link(), None);
    assert_eq!(ui.cursor(), Cursor::Default);
    ui.pointer(&mut r, PointerEvent::Moved(over(&ui, 5)))
        .unwrap();
    ui.pointer(&mut r, PointerEvent::Pressed(over(&ui, 5)))
        .unwrap();
    assert!(ui.pointer(&mut r, PointerEvent::Cancelled).unwrap());
    assert_eq!(ui.hovered_link(), None);
    ui.pointer(&mut r, PointerEvent::Released(over(&ui, 5)))
        .unwrap();
    assert_eq!(opened(&mut r, &entity), [2]);
}

#[test]
fn prevented_press_and_secondary_buttons_do_not_activate() {
    struct Guarded(Vec<usize>);
    impl View for Guarded {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column()
                .on_pointer_down(cx.listener(|_, event: &PointerInput, _| {
                    if event.modifiers.alt {
                        event.prevent_default();
                    }
                }))
                .child(
                    rich_text([link(
                        "docs",
                        cx.listener(|this: &mut Self, event: &LinkEvent, _| {
                            this.0.push(event.index)
                        }),
                    )])
                    .key("doc"),
                )
        }
    }
    let mut r = Runtime::new();
    let entity = r.update(|cx| cx.new(|_| Guarded(Vec::new())));
    let mut ui = Ui::new(&mut r, entity.clone()).unwrap();
    ui.prepare(&mut r, [200., 100.], &mut Measure::default())
        .unwrap();
    let point = over(&ui, 1);
    let alt = Modifiers {
        alt: true,
        ..Default::default()
    };
    for (button, modifiers) in [
        (PointerButton::Primary, alt),
        (PointerButton::Secondary, Modifiers::default()),
    ] {
        ui.pointer(
            &mut r,
            PointerEvent::Down {
                position: point,
                button,
                modifiers,
            },
        )
        .unwrap();
        ui.pointer(
            &mut r,
            PointerEvent::Up {
                position: point,
                button,
                modifiers: Modifiers::default(),
            },
        )
        .unwrap();
    }
    assert!(r.update(|cx| entity.read(cx).0.is_empty()));
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    assert_eq!(r.update(|cx| entity.read(cx).0.clone()), [0]);
}

#[test]
fn tab_visits_each_link_in_order_and_enter_activates_the_focused_one() {
    let (mut r, entity, mut ui, _) = setup(Doc::new());
    let doc = keyed(&ui, "doc");
    let before = keyed(&ui, "before");
    let after = keyed(&ui, "after");
    let mut stops = Vec::new();
    for _ in 0..5 {
        assert!(ui.focus_next(false));
        stops.push((ui.focused_element(), ui.focused_link()));
    }
    // The empty link is skipped; the rich text is one element with two stops.
    assert_eq!(
        stops,
        [
            (Some(before), None),
            (Some(doc), Some(0)),
            (Some(doc), Some(2)),
            (Some(after), None),
            (Some(before), None),
        ]
    );
    // Shift+Tab enters at the last link and walks back.
    ui.focus(after);
    assert!(ui.focus_next(true));
    assert_eq!(
        (ui.focused_element(), ui.focused_link()),
        (Some(doc), Some(2))
    );
    let info = ui.element(doc).unwrap();
    assert!(info.focus_visible);
    assert_eq!(info.links.unwrap().focused, Some(2));
    assert!(ui.focus_next(true));
    assert_eq!(ui.focused_link(), Some(0));
    assert!(ui.focus_next(true));
    assert_eq!(ui.focused_element(), Some(before));
    assert_eq!(ui.focused_link(), None);
    // Enter activates the focused link and prevents the host's own default.
    ui.focus_next(false);
    ui.focus_next(false);
    let result = press_key(&mut r, &mut ui, KeyboardKey::Enter, false);
    assert!(result.default_prevented);
    assert!(result.changed);
    assert_eq!(opened(&mut r, &entity), [2]);
    // Holding Enter activates once; Space is left to the host and does nothing.
    let held = press_key(&mut r, &mut ui, KeyboardKey::Enter, true);
    assert!(held.default_prevented);
    assert!(!press_key(&mut r, &mut ui, KeyboardKey::Space, false).default_prevented);
    assert!(!ui.activate_focused(&mut r).unwrap());
    assert_eq!(opened(&mut r, &entity), [2]);
    // Enter elsewhere is not a link default.
    ui.focus(after);
    assert!(!press_key(&mut r, &mut ui, KeyboardKey::Enter, false).default_prevented);
}

#[test]
fn programmatic_focus_selects_the_first_link_and_survives_description_changes() {
    let (mut r, entity, mut ui, mut measure) = setup(Doc::new());
    let doc = keyed(&ui, "doc");
    assert!(ui.focus(doc));
    assert_eq!(ui.focused_link(), Some(0));
    ui.focus_next(false);
    assert_eq!(ui.focused_link(), Some(2));
    // Renaming a link keeps the focused index while it still exists.
    r.update(|cx| entity.update(cx, |d, _| d.text = "Vector"));
    ui.prepare(&mut r, [600., 300.], &mut measure).unwrap();
    assert_eq!(
        (ui.focused_element(), ui.focused_link()),
        (Some(doc), Some(2))
    );
    let ranges = ui.element(doc).unwrap().links.unwrap().ranges.to_vec();
    assert_eq!(ranges, [4..10, 15..15, 20..22]);
    // Removing every link removes the stop and its focus.
    r.update(|cx| entity.update(cx, |d, _| d.links = false));
    ui.prepare(&mut r, [600., 300.], &mut measure).unwrap();
    assert_eq!(ui.focused_element(), None);
    assert_eq!(ui.focused_link(), None);
    assert!(ui.element(doc).unwrap().links.is_none());
    assert!(!ui.focus(doc));
}

#[test]
fn link_geometry_is_measured_once_per_text_or_width_change() {
    let (mut r, entity, mut ui, mut measure) = setup(Doc::new());
    assert_eq!(ui.stats().link_measurements, 1);
    // Two non-empty links, measured once each.
    assert_eq!(measure.rects, 2);
    for offset in [1, 5, 18, 5] {
        ui.pointer(&mut r, PointerEvent::Moved(over(&ui, offset)))
            .unwrap();
        ui.prepare(&mut r, [600., 300.], &mut measure).unwrap();
    }
    ui.focus_next(false);
    ui.focus_next(false);
    ui.prepare(&mut r, [600., 300.], &mut measure).unwrap();
    assert_eq!(measure.rects, 2);
    assert_eq!(ui.stats().link_measurements, 1);
    r.update(|cx| entity.update(cx, |d, _| d.text = "Vector"));
    ui.prepare(&mut r, [600., 300.], &mut measure).unwrap();
    assert_eq!(ui.stats().link_measurements, 2);
    // Rich text without links never asks for geometry.
    let (_, _, ui, measure) = setup(Doc {
        links: false,
        ..Doc::new()
    });
    assert_eq!(ui.stats().link_measurements, 0);
    assert_eq!(measure.rects, 0);
}

#[test]
fn wrapped_links_hit_test_every_line_fragment() {
    struct Wrapped(Vec<usize>);
    impl View for Wrapped {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            rich_text([
                span("ab "),
                link(
                    "cd\nef",
                    cx.listener(|this: &mut Self, e: &LinkEvent, _| this.0.push(e.index)),
                ),
            ])
            .key("doc")
        }
    }
    let mut r = Runtime::new();
    let entity = r.update(|cx| cx.new(|_| Wrapped(Vec::new())));
    let mut ui = Ui::new(&mut r, entity.clone()).unwrap();
    ui.prepare(&mut r, [200., 100.], &mut Measure::default())
        .unwrap();
    let doc = keyed(&ui, "doc");
    let content = ui.element(doc).unwrap().content_bounds;
    // "ef" starts the second line; right of "cd" on the first line is not a link.
    let second = [content.x + 5., content.y + 30.];
    ui.pointer(
        &mut r,
        PointerEvent::Moved([content.x + 65., content.y + 10.]),
    )
    .unwrap();
    assert_eq!(ui.hovered_link(), None);
    ui.pointer(&mut r, PointerEvent::Moved(second)).unwrap();
    assert_eq!(ui.hovered_link(), Some((doc, 0)));
    ui.pointer(&mut r, PointerEvent::Pressed(second)).unwrap();
    ui.pointer(&mut r, PointerEvent::Released(second)).unwrap();
    assert_eq!(r.update(|cx| entity.read(cx).0.clone()), [0]);
    let link = ui.semantic_links(doc).next().unwrap();
    assert_eq!(
        link.bounds,
        Bounds {
            x: content.x,
            y: content.y,
            width: 50.,
            height: 40.,
        }
    );
}

#[test]
fn selectable_text_selects_from_a_link_drag_and_still_tabs_to_links() {
    let (mut r, entity, mut ui, mut measure) = setup(Doc {
        selectable: true,
        ..Doc::new()
    });
    let doc = keyed(&ui, "doc");
    // Selectable text stays a Tab stop through its links.
    ui.focus_next(false);
    ui.focus_next(false);
    assert_eq!(
        (ui.focused_element(), ui.focused_link()),
        (Some(doc), Some(0))
    );
    // A press between links focuses the text itself, with the text cursor.
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Pressed(over(&ui, 1)),
        &mut measure,
        false,
    )
    .unwrap();
    assert_eq!((ui.focused_element(), ui.focused_link()), (Some(doc), None));
    assert_eq!(ui.cursor(), Cursor::Text);
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Released(over(&ui, 1)),
        &mut measure,
        false,
    )
    .unwrap();
    // Tab from there enters the first link; Shift+Tab leaves the text.
    ui.focus_next(false);
    assert_eq!(ui.focused_link(), Some(0));
    // The pointer cursor wins over the selectable text cursor on a link.
    ui.pointer(&mut r, PointerEvent::Moved(over(&ui, 5)))
        .unwrap();
    assert_eq!(ui.cursor(), Cursor::Pointer);
    // A drag that starts on a link selects instead of activating.
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Pressed(over(&ui, 4)),
        &mut measure,
        false,
    )
    .unwrap();
    assert_eq!(ui.focused_link(), Some(0));
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Moved(over(&ui, 9)),
        &mut measure,
        false,
    )
    .unwrap();
    assert_eq!(ui.cursor(), Cursor::Text);
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Moved(over(&ui, 5)),
        &mut measure,
        false,
    )
    .unwrap();
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Released(over(&ui, 5)),
        &mut measure,
        false,
    )
    .unwrap();
    assert!(opened(&mut r, &entity).is_empty());
    // Points sit mid-glyph, so the press and release carets round to bytes 5 and 6.
    assert_eq!(ui.selected_text(), Some("e"));
    // A click within the slop activates.
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Pressed(over(&ui, 5)),
        &mut measure,
        false,
    )
    .unwrap();
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Moved(over(&ui, 5)),
        &mut measure,
        false,
    )
    .unwrap();
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Released(over(&ui, 5)),
        &mut measure,
        false,
    )
    .unwrap();
    assert_eq!(opened(&mut r, &entity), [0]);
    // Enter on a focused link activates it, not text submission.
    ui.focus_next(false);
    assert_eq!(ui.focused_link(), Some(2));
    assert!(press_key(&mut r, &mut ui, KeyboardKey::Enter, false).default_prevented);
    assert_eq!(opened(&mut r, &entity), [0, 2]);
}

#[test]
fn semantics_publish_named_links_and_route_link_actions() {
    let (mut r, entity, mut ui, mut measure) = setup(Doc::new());
    let doc = keyed(&ui, "doc");
    let node = ui.semantic_node(doc).unwrap();
    assert_eq!(node.role, SemanticRole::Label);
    assert_eq!(node.value, Some("See Vec and  and u8 for details."));
    assert_eq!(node.links, 2);
    // Focus belongs to the link nodes, not the label.
    assert!(!node.focusable);
    let content = node.content_bounds;
    let links: Vec<_> = ui.semantic_links(doc).collect();
    assert_eq!(links.len(), 2);
    assert_eq!((links[0].index, links[0].name), (0, "Vec"));
    assert_eq!(
        links[0].url,
        Some("https://doc.rust-lang.org/std/vec/struct.Vec.html")
    );
    assert_eq!(links[0].description, Some("Opens the Vec documentation"));
    assert_eq!(
        links[0].bounds,
        Bounds {
            x: content.x + 40.,
            y: content.y,
            width: 30.,
            height: 20.,
        }
    );
    assert_eq!(
        (links[1].index, links[1].name, links[1].url),
        (2, "u8", None)
    );
    assert!(links.iter().all(|l| l.available && !l.focused));
    // Assistive focus and activation use the same paths as keyboard input.
    assert!(
        ui.semantic_action(
            &mut r,
            SemanticAction::FocusLink {
                target: doc,
                link: 2
            },
            &mut measure
        )
        .unwrap()
    );
    assert_eq!(
        (ui.semantic_focus(), ui.focused_link()),
        (Some(doc), Some(2))
    );
    assert!(ui.semantic_links(doc).nth(1).unwrap().focused);
    assert!(
        ui.semantic_action(
            &mut r,
            SemanticAction::ActivateLink {
                target: doc,
                link: 0
            },
            &mut measure
        )
        .unwrap()
    );
    assert_eq!(opened(&mut r, &entity), [0]);
    // The empty link and unknown indices are rejected.
    for link in [1, 3] {
        for action in [
            SemanticAction::FocusLink { target: doc, link },
            SemanticAction::ActivateLink { target: doc, link },
        ] {
            assert!(!ui.semantic_action(&mut r, action, &mut measure).unwrap());
        }
    }
    // Ordinary focus of the label lands on its first link.
    ui.focus(keyed(&ui, "before"));
    ui.semantic_action(&mut r, SemanticAction::Focus(doc), &mut measure)
        .unwrap();
    assert_eq!(ui.focused_link(), Some(0));
    assert_eq!(opened(&mut r, &entity), [0]);
}

#[test]
fn inert_rich_text_links_take_no_input() {
    struct Inert(Vec<usize>);
    impl View for Inert {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column().inert(true).child(
                rich_text([link(
                    "docs",
                    cx.listener(|this: &mut Self, e: &LinkEvent, _| this.0.push(e.index)),
                )])
                .key("doc"),
            )
        }
    }
    let mut r = Runtime::new();
    let entity = r.update(|cx| cx.new(|_| Inert(Vec::new())));
    let mut ui = Ui::new(&mut r, entity.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut r, [200., 100.], &mut measure).unwrap();
    let doc = keyed(&ui, "doc");
    let point = over(&ui, 1);
    ui.pointer(&mut r, PointerEvent::Moved(point)).unwrap();
    assert_eq!(ui.hovered_link(), None);
    ui.pointer(&mut r, PointerEvent::Pressed(point)).unwrap();
    ui.pointer(&mut r, PointerEvent::Released(point)).unwrap();
    assert!(!ui.focus_next(false));
    assert!(!ui.focus(doc));
    let action = SemanticAction::ActivateLink {
        target: doc,
        link: 0,
    };
    assert!(!ui.semantic_action(&mut r, action, &mut measure).unwrap());
    assert!(r.update(|cx| entity.read(cx).0.is_empty()));
}

#[test]
fn link_spans_default_to_accent_underline_and_stay_overridable() {
    /// Records the resolved spans that reach text measurement.
    #[derive(Default)]
    struct Spans(Vec<ResolvedSpan>, Measure);
    impl TextMeasure for Spans {
        fn measure(
            &mut self,
            id: ElementId,
            request: TextRequest<'_>,
        ) -> Result<[f32; 2], UiError> {
            self.0 = request.spans.to_vec();
            self.1.measure(id, request)
        }
        fn text_range_rects(
            &mut self,
            id: ElementId,
            request: TextRequest<'_>,
            range: std::ops::Range<usize>,
        ) -> Result<Option<Vec<Bounds>>, UiError> {
            self.1.text_range_rects(id, request, range)
        }
    }
    struct Styled;
    impl View for Styled {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let a = cx.listener(|_, _: &LinkEvent, _| {});
            let b = cx.listener(|_, _: &LinkEvent, _| {});
            // Spans sharing a listener compare equal; another binding does not.
            assert_eq!(link("x", a.clone()), link("x", a.clone()));
            assert_ne!(link("x", a.clone()), link("x", b.clone()));
            rich_text([
                link("a", a),
                link("b", b).color(ThemeColor::TextMuted).underlined(false),
                span("c").underline(),
            ])
            .key("doc")
        }
    }
    let mut r = Runtime::new();
    let entity = r.update(|cx| cx.new(|_| Styled));
    let mut ui = Ui::new(&mut r, entity).unwrap();
    let mut measure = Spans::default();
    ui.prepare(&mut r, [200., 100.], &mut measure).unwrap();
    let theme = ui.theme().clone();
    let spans = &measure.0;
    assert_eq!(spans[0].color, Some(theme.color(ThemeColor::Accent)));
    assert!(spans[0].underline);
    assert_eq!(spans[1].color, Some(theme.color(ThemeColor::TextMuted)));
    assert!(!spans[1].underline);
    assert!(spans[2].underline && spans[2].color.is_none());
    // Hover recolors the accent link only; an explicit color stays.
    let doc = keyed(&ui, "doc");
    ui.pointer(&mut r, PointerEvent::Moved(over(&ui, 0)))
        .unwrap();
    let info = ui.element(doc).unwrap().links.unwrap();
    assert_eq!(
        info.active_color,
        Some(theme.color(ThemeColor::AccentHover))
    );
    ui.pointer(&mut r, PointerEvent::Pressed(over(&ui, 0)))
        .unwrap();
    let info = ui.element(doc).unwrap().links.unwrap();
    assert_eq!(info.pressed, Some(0));
    assert_eq!(
        info.active_color,
        Some(theme.color(ThemeColor::AccentPressed))
    );
    ui.pointer(&mut r, PointerEvent::Released(over(&ui, 0)))
        .unwrap();
    ui.pointer(&mut r, PointerEvent::Moved(over(&ui, 1)))
        .unwrap();
    let info = ui.element(doc).unwrap().links.unwrap();
    assert_eq!((info.hovered, info.active_color), (Some(1), None));
}

#[cfg(feature = "accessibility")]
mod access_kit {
    use super::*;
    use rxui::accesskit::{Action, ActionRequest, NodeId, Role, TreeId, TreeUpdate};

    fn request(target: NodeId, action: Action) -> ActionRequest {
        ActionRequest {
            target_tree: TreeId::ROOT,
            target_node: target,
            action,
            data: None,
        }
    }
    #[test]
    fn links_publish_as_link_children_with_focus_and_click() {
        let (mut r, entity, mut ui, mut measure) = setup(Doc::new());
        let doc = keyed(&ui, "doc");
        let mut cache = AccessKitTree::new();
        let first = cache.update(&ui, "Docs", 2.).unwrap().unwrap();
        let find = |update: &TreeUpdate, role: Role| {
            update
                .nodes
                .iter()
                .filter(|(_, n)| n.role() == role)
                .map(|(id, n)| (*id, n.clone()))
                .collect::<Vec<_>>()
        };
        let links = find(&first, Role::Link);
        assert_eq!(links.len(), 2);
        let (vec_id, vec_node) = &links[0];
        assert_eq!(vec_node.label(), Some("Vec"));
        assert_eq!(
            vec_node.url(),
            Some("https://doc.rust-lang.org/std/vec/struct.Vec.html")
        );
        assert_eq!(vec_node.description(), Some("Opens the Vec documentation"));
        assert!(vec_node.supports_action(Action::Focus));
        assert!(vec_node.supports_action(Action::Click));
        // Bounds are physical, relative to the label, whose transform places them.
        let label_bounds = ui.semantic_node(doc).unwrap();
        let offset = [
            label_bounds.content_bounds.x - label_bounds.bounds.x,
            label_bounds.content_bounds.y - label_bounds.bounds.y,
        ];
        let bounds = vec_node.bounds().unwrap();
        assert_eq!(
            [bounds.x0, bounds.y0, bounds.x1, bounds.y1],
            [
                f64::from(offset[0] + 40.) * 2.,
                f64::from(offset[1]) * 2.,
                f64::from(offset[0] + 70.) * 2.,
                f64::from(offset[1] + 20.) * 2.
            ]
        );
        let labels = find(&first, Role::Label);
        let (_, label) = labels
            .iter()
            .find(|(_, n)| n.value() == Some("See Vec and  and u8 for details."))
            .unwrap();
        assert_eq!(label.children(), [*vec_id, links[1].0]);
        assert!(!label.supports_action(Action::Focus));
        // Decoded actions reach the link through the portable action path.
        let focus = cache.action(request(links[1].0, Action::Focus)).unwrap();
        assert_eq!(
            focus,
            SemanticAction::FocusLink {
                target: doc,
                link: 2
            }
        );
        assert!(cache.action(request(*vec_id, Action::SetValue)).is_none());
        ui.semantic_action(&mut r, focus, &mut measure).unwrap();
        let update = cache.update(&ui, "Docs", 2.).unwrap().unwrap();
        assert_eq!(update.focus, links[1].0);
        // Hovering a link is not a semantic change.
        ui.pointer(&mut r, PointerEvent::Moved(over(&ui, 5)))
            .unwrap();
        assert!(cache.update(&ui, "Docs", 2.).unwrap().is_none());
        // Tab between links moves accessibility focus between link nodes.
        ui.focus_next(true);
        assert_eq!(
            cache.update(&ui, "Docs", 2.).unwrap().unwrap().focus,
            *vec_id
        );
        let click = cache.action(request(*vec_id, Action::Click)).unwrap();
        assert!(ui.semantic_action(&mut r, click, &mut measure).unwrap());
        assert_eq!(opened(&mut r, &entity), [0]);
        // Link identities are stable while the element lives.
        r.update(|cx| entity.update(cx, |d, _| d.text = "Vector"));
        ui.prepare(&mut r, [600., 300.], &mut measure).unwrap();
        let renamed = cache.update(&ui, "Docs", 2.).unwrap().unwrap();
        let (id, node) = &find(&renamed, Role::Link)[0];
        assert_eq!((*id, node.label()), (*vec_id, Some("Vector")));
    }
}

/// Real shaping and painting of links: geometry from Astrelis layout, the hover
/// recolor and the focus ring around the link's text.
#[cfg(feature = "rendering")]
mod gpu {
    use super::*;
    use rxui::drawing::GraphicsContext;

    struct Page(Vec<usize>);
    impl View for Page {
        fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            column().padding(10.).child(
                rich_text([
                    span("Read the "),
                    link(
                        "guide",
                        cx.listener(|this: &mut Self, e: &LinkEvent, _| this.0.push(e.index)),
                    ),
                    span(" first."),
                ])
                .font_size(24.)
                .key("doc"),
            )
        }
    }
    /// Pixels inside `b` (logical units, scale one) that differ between frames.
    fn changed(a: &[u8], b: &[u8], width: u32, area: Bounds) -> usize {
        let mut count = 0;
        for y in area.y.max(0.) as u32..(area.y + area.height) as u32 {
            for x in area.x.max(0.) as u32..(area.x + area.width) as u32 {
                let i = ((y * width + x) * 4) as usize;
                count += usize::from(a[i..i + 4] != b[i..i + 4]);
            }
        }
        count
    }
    #[test]
    #[ignore = "requires a native GPU; run with --features rendering -- --ignored"]
    fn shaped_links_hit_test_recolor_on_hover_and_ring_their_text() {
        let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
        let size = [320, 64];
        let mut test = TestUi::new(&graphics, size, Page(Vec::new())).unwrap();
        test.painter_mut()
            .fonts_mut()
            .load_font(include_bytes!("fonts/SourceSans3-Regular.otf"))
            .unwrap();
        let plain = test.render().unwrap();
        let doc = test.element("doc").unwrap().unwrap();
        let (id, content) = (doc.id, doc.content_bounds);
        let link = test.ui().semantic_links(id).next().unwrap().bounds;
        // Shaped geometry places "guide" after "Read the ", on the first line.
        assert!(link.x > content.x + 40., "{link:?}");
        assert!(link.width > 20. && link.height >= 24.);
        let center = [link.x + link.width / 2., link.y + link.height / 2.];
        let before_link = Bounds {
            x: content.x,
            width: link.x - content.x - 2.,
            ..link
        };
        // Hover redraws only the link's glyphs.
        test.pointer(PointerEvent::Moved(center)).unwrap();
        assert_eq!(test.ui().cursor(), Cursor::Pointer);
        let hovered = test.render().unwrap();
        assert!(changed(&plain, &hovered, size[0], link) > 0);
        assert_eq!(changed(&plain, &hovered, size[0], before_link), 0);
        // Plain text left of the link is not a link.
        test.pointer(PointerEvent::Moved([content.x + 5., center[1]]))
            .unwrap();
        assert_eq!(test.ui().hovered_link(), None);
        assert_eq!(changed(&plain, &test.render().unwrap(), size[0], link), 0);
        // A click on the shaped link activates it.
        test.pointer(PointerEvent::Pressed(center)).unwrap();
        test.pointer(PointerEvent::Released(center)).unwrap();
        assert_eq!(test.read(|p| p.0.clone()), [0]);
        // Keyboard focus rings the link, not the element.
        test.pointer(PointerEvent::Left).unwrap();
        let (_, ui, _) = test.parts();
        ui.focus(id);
        ui.focus_next(true);
        ui.focus_next(false);
        assert_eq!(ui.focused_link(), Some(0));
        let focused = test.render().unwrap();
        let ring = Bounds {
            x: link.x - 3.,
            y: link.y - 3.,
            width: link.width + 6.,
            height: 3.,
        };
        assert!(changed(&plain, &focused, size[0], ring) > 0);
        assert_eq!(changed(&plain, &focused, size[0], before_link), 0);
    }
}
