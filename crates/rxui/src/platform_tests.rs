//! Headless coverage for platform conventions: focus-visible rings, window drag
//! regions and scrollbar styles.
use crate::*;

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
fn center<T: View>(ui: &Ui<T>, key: &str) -> [f32; 2] {
    let b = ui
        .elements()
        .find(|e| e.key == Some(&Key::from(key)))
        .unwrap()
        .bounds;
    [b.x + b.width / 2., b.y + b.height / 2.]
}
fn ring<T: View>(ui: &Ui<T>, key: &str) -> bool {
    ui.elements()
        .find(|e| e.key == Some(&Key::from(key)))
        .unwrap()
        .focus_visible
}
fn press(key: KeyboardKey, modifiers: Modifiers) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

struct Form;
impl View for Form {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .child(button("First").key("first"))
            .child(button("Second").key("second"))
            .child(text_input("Ada").key("name"))
    }
}

#[test]
fn focus_rings_follow_keyboard_modality_like_focus_visible() {
    let mut r = Runtime::new();
    let root = r.update(|cx| cx.new(|_| Form));
    let mut ui = Ui::new(&mut r, root).unwrap();
    ui.prepare(&mut r, [400., 300.], &mut Measure).unwrap();

    assert!(ui.focus_next(false));
    assert!(ring(&ui, "first") && ui.focus_ring_visible());

    // A pointer press focuses without a ring and reports the change.
    let second = center(&ui, "second");
    assert!(ui.pointer(&mut r, PointerEvent::Pressed(second)).unwrap());
    ui.pointer(&mut r, PointerEvent::Released(second)).unwrap();
    assert!(
        ui.elements()
            .any(|e| e.focused && e.key == Some(&Key::from("second")))
    );
    assert!(!ring(&ui, "second") && !ui.focus_ring_visible());

    // Shortcuts and bare modifiers keep it hidden; navigation keys show it.
    let command = Modifiers {
        meta: true,
        control: true,
        ..Default::default()
    };
    ui.key(&mut r, press(KeyboardKey::Character("c".into()), command))
        .unwrap();
    ui.key(
        &mut r,
        press(KeyboardKey::Other("Shift".into()), Modifiers::default()),
    )
    .unwrap();
    assert!(!ring(&ui, "second"));
    let shown = ui
        .key(&mut r, press(KeyboardKey::ArrowDown, Modifiers::default()))
        .unwrap();
    assert!(shown.changed && ring(&ui, "second"));

    // Text inputs show the ring even when clicked.
    ui.pointer(&mut r, PointerEvent::Pressed(center(&ui, "name")))
        .unwrap();
    assert!(ring(&ui, "name"));

    // The Always policy shows it after pointer focus too.
    ui.pointer(&mut r, PointerEvent::Pressed(center(&ui, "first")))
        .unwrap();
    assert!(!ring(&ui, "first"));
    assert!(ui.set_focus_ring_policy(FocusRingPolicy::Always));
    assert!(!ui.set_focus_ring_policy(FocusRingPolicy::Always));
    assert!(ring(&ui, "first"));
    assert!(!ring(&ui, "second"));
}

struct TitleBar;
impl View for TitleBar {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .fill_width()
            .child(
                row()
                    .key("bar")
                    .height(32.)
                    .fill_width()
                    .window_drag_region()
                    .child(label("Title").key("title"))
                    .child(button("Close").key("close")),
            )
            .child(column().key("body").height(100.).fill_width())
    }
}

#[test]
fn window_drag_regions_route_presses_outside_interactive_descendants() {
    let mut r = Runtime::new();
    let root = r.update(|cx| cx.new(|_| TitleBar));
    let mut ui = Ui::new(&mut r, root).unwrap();
    assert!(!ui.window_drag_region_at([1., 1.])); // Not prepared yet.
    ui.prepare(&mut r, [400., 300.], &mut Measure).unwrap();
    assert!(ui.window_drag_region_at(center(&ui, "title")));
    assert!(ui.window_drag_region_at([390., 16.]));
    assert!(!ui.window_drag_region_at(center(&ui, "close")));
    assert!(!ui.window_drag_region_at(center(&ui, "body")));
    assert!(!ui.window_drag_region_at([-1., 16.]));
    assert!(!ui.window_drag_region_at([400., 16.]));
}

struct Scrolling(ScrollbarStyle);
impl View for Scrolling {
    fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().size(300., 200.).child(
            scroll_area(column().key("content").height(1000.).fill_width()).scrollbar_style(self.0),
        )
    }
}

#[test]
fn overlay_scrollbars_reserve_no_gutter_and_classic_bars_do() {
    for (style, bar_width, content_width) in [
        (ScrollbarStyle::Classic, 12., 288.),
        (ScrollbarStyle::Overlay, 8., 300.),
    ] {
        let mut r = Runtime::new();
        let root = r.update(|cx| cx.new(|_| Scrolling(style)));
        let mut ui = Ui::new(&mut r, root).unwrap();
        ui.prepare(&mut r, [300., 200.], &mut Measure).unwrap();
        let bar = ui
            .elements()
            .find(|e| e.kind == ElementType::Scrollbar)
            .unwrap();
        assert_eq!(bar.bounds.width, bar_width, "{style:?}");
        assert_eq!(bar.bounds.x + bar.bounds.width, 300., "{style:?}");
        assert_eq!(bar.bounds.height, 200., "{style:?}");
        assert_eq!(
            bar.paint.background.is_none(),
            style == ScrollbarStyle::Overlay
        );
        let content = ui
            .elements()
            .find(|e| e.key == Some(&Key::from("content")))
            .unwrap();
        assert_eq!(content.bounds.width, content_width, "{style:?}");
    }
    assert_eq!(
        ScrollbarStyle::platform() == ScrollbarStyle::Overlay,
        cfg!(target_os = "macos")
    );
}
