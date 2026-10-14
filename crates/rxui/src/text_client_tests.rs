//! Headless text input client routing: committed text, IME composition and its
//! cancellation, key precedence, Tab and the IME candidate area.
use crate::*;

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}
#[derive(PartialEq)]
struct Surface;
impl CustomElement for Surface {
    type State = ();
    fn measure(&self, _: CustomMeasure) -> [f32; 2] {
        [200., 40.]
    }
}
struct Editor {
    text: String,
    preedit: Option<(String, Option<(usize, usize)>)>,
    log: Vec<String>,
    client: bool,
    removed: bool,
    inert: bool,
    /// Vim-style normal mode: character keys are motions, not text.
    normal_mode: bool,
    capture_tab: bool,
    caret: Option<f32>,
    field: String,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            text: String::new(),
            preedit: None,
            log: Vec::new(),
            client: true,
            removed: false,
            inert: false,
            normal_mode: false,
            capture_tab: false,
            caret: Some(24.),
            field: String::new(),
        }
    }
}
impl View for Editor {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut root = column()
            .size(400., 300.)
            .padding(10.)
            .on_text_insert(cx.listener(|s, e: &TextInsertInput, _| {
                assert_eq!(e.phase, EventPhase::Bubble);
                s.log.push(format!("root insert {}", e.text));
            }))
            .on_ime(cx.listener(|s, e: &ImeInput, _| {
                if let ImeEvent::Cancel(reason) = e.event {
                    s.log.push(format!("root cancel {reason:?}"));
                }
            }));
        if !self.removed {
            let mut editor = custom(Surface)
                .key("editor")
                .padding(4.)
                .text_input_client(self.client)
                .on_key_down(cx.listener(|s, e: &KeyInput, _| {
                    s.log.push(format!("key {:?}", e.event.key));
                    let motion = matches!(e.event.key, KeyboardKey::Character(_)) && s.normal_mode;
                    if motion || (e.event.key == KeyboardKey::Tab && s.capture_tab) {
                        e.prevent_default();
                    }
                }))
                .on_pointer_down(cx.listener(|s, e: &PointerInput, _| {
                    s.log.push("press".into());
                    e.focus();
                    e.prevent_default();
                }))
                .on_text_insert(cx.listener(|s, e: &TextInsertInput, _| {
                    assert_eq!(e.phase, EventPhase::Target);
                    s.log.push(format!("insert {:?} {}", e.source, e.text));
                    if e.source == TextInsertSource::Ime {
                        s.preedit = None;
                    }
                    s.text.push_str(&e.text);
                }))
                .on_ime(cx.listener(|s, e: &ImeInput, _| match &e.event {
                    ImeEvent::Preedit { text, cursor } => {
                        s.log.push(format!("preedit {text}"));
                        s.preedit = Some((text.clone(), *cursor));
                    }
                    ImeEvent::Cancel(reason) => {
                        s.log.push(format!("cancel {reason:?}"));
                        s.preedit = None;
                    }
                }));
            if let Some(x) = self.caret {
                editor = editor.ime_cursor_area(Bounds {
                    x,
                    y: 4.,
                    width: 1.,
                    height: 16.,
                });
            }
            if self.inert {
                editor = editor.inert(true);
            }
            root = root.child(editor);
        }
        root.child(
            custom(Surface)
                .key("plain")
                .focusable(true)
                .on_text_insert(cx.listener(|s, _: &TextInsertInput, _| {
                    s.log.push("plain insert".into());
                })),
        )
        .child(
            text_input(self.field.clone())
                .key("field")
                .on_change(cx.listener(|s, e: &TextChangeEvent, _| {
                    s.field = e.value.clone();
                })),
        )
    }
}
fn setup() -> (Runtime, Entity<Editor>, Ui<Editor>) {
    let mut r = Runtime::new();
    let e = r.update(|cx| cx.new(|_| Editor::default()));
    let mut ui = Ui::new(&mut r, e.clone()).unwrap();
    prepare(&mut r, &mut ui);
    (r, e, ui)
}
fn prepare(r: &mut Runtime, ui: &mut Ui<Editor>) {
    ui.prepare(r, [400., 300.], &mut Measure).unwrap();
}
fn keyed(ui: &Ui<Editor>, key: &str) -> ElementId {
    ui.elements()
        .find(|e| e.key == Some(&Key::from(key)))
        .unwrap()
        .id
}
fn focus(r: &mut Runtime, ui: &mut Ui<Editor>, key: &str) -> ElementId {
    prepare(r, ui);
    let id = keyed(ui, key);
    assert!(ui.focus(id));
    id
}
fn update(r: &mut Runtime, e: &Entity<Editor>, f: impl FnOnce(&mut Editor)) {
    r.update(|cx| e.update(cx, |s, _| f(s)));
}
fn take_log(r: &mut Runtime, e: &Entity<Editor>) -> Vec<String> {
    r.update(|cx| e.update(cx, |s, _| std::mem::take(&mut s.log)))
}
fn text(r: &mut Runtime, e: &Entity<Editor>) -> String {
    r.update(|cx| e.read(cx).text.clone())
}
fn input(r: &mut Runtime, ui: &mut Ui<Editor>, event: TextInputEvent) -> bool {
    ui.text_input(r, event, &mut Measure).unwrap()
}
fn key_down(r: &mut Runtime, ui: &mut Ui<Editor>, key: KeyboardKey) -> InputResult {
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
/// The documented host order: key listeners first, then the key's text unless
/// prevented.
fn type_key(r: &mut Runtime, ui: &mut Ui<Editor>, character: &str) {
    let result = key_down(r, ui, KeyboardKey::Character(character.into()));
    if !result.default_prevented {
        input(r, ui, TextInputEvent::Insert(character.into()));
    }
}
fn preedit(text: &str, cursor: Option<(usize, usize)>) -> TextInputEvent {
    TextInputEvent::Preedit {
        text: text.into(),
        cursor,
    }
}

#[test]
fn committed_text_routes_to_the_focused_client_then_ancestors() {
    let (mut r, e, mut ui) = setup();
    assert!(!ui.accepts_text_input() && !ui.accepts_ime());
    assert!(!input(&mut r, &mut ui, TextInputEvent::Insert("x".into())));
    let id = focus(&mut r, &mut ui, "editor");
    assert_eq!(ui.focused_text_client(), Some(id));
    assert!(ui.accepts_text_input() && ui.accepts_ime());
    // Clients have no built-in selection or caret blink.
    assert!(!ui.has_text_focus());
    assert!(ui.text_input_info(id).is_none());

    // A dead key followed by e produces one committed character.
    assert!(input(&mut r, &mut ui, TextInputEvent::Insert("é".into())));
    assert!(input(&mut r, &mut ui, TextInputEvent::Paste("ß\n".into())));
    assert_eq!(text(&mut r, &e), "éß\n");
    assert_eq!(
        take_log(&mut r, &e),
        [
            "insert Keyboard é",
            "root insert é",
            "insert Paste ß\n",
            "root insert ß\n"
        ]
    );
    // Editing keys reach clients as keys, not text events.
    assert!(!input(&mut r, &mut ui, TextInputEvent::Backspace));
    assert!(!input(&mut r, &mut ui, TextInputEvent::SelectAll));
    assert!(!input(
        &mut r,
        &mut ui,
        TextInputEvent::Insert(String::new())
    ));
    assert!(take_log(&mut r, &e).is_empty());
}

#[test]
fn key_listeners_run_first_and_prevent_default_suppresses_insertion() {
    let (mut r, e, mut ui) = setup();
    focus(&mut r, &mut ui, "editor");
    type_key(&mut r, &mut ui, "j");
    assert_eq!(text(&mut r, &e), "j");
    assert_eq!(
        take_log(&mut r, &e),
        ["key Character(\"j\")", "insert Keyboard j", "root insert j"]
    );

    update(&mut r, &e, |s| s.normal_mode = true);
    prepare(&mut r, &mut ui);
    type_key(&mut r, &mut ui, "j");
    assert_eq!(text(&mut r, &e), "j");
    assert_eq!(take_log(&mut r, &e), ["key Character(\"j\")"]);
}

#[test]
fn preedit_commit_lifecycle_and_cancellation_on_focus_change() {
    let (mut r, e, mut ui) = setup();
    let id = focus(&mut r, &mut ui, "editor");
    let revision = ui.ime_reset_revision();
    // An empty preedit outside composition carries nothing.
    assert!(!input(&mut r, &mut ui, preedit("", None)));
    assert!(input(&mut r, &mut ui, preedit("に", Some((0, 3)))));
    assert_eq!(
        r.update(|cx| e.read(cx).preedit.clone()),
        Some(("に".into(), Some((0, 3))))
    );
    // Keyboard text waits for the composition to end, as in built-in inputs.
    input(&mut r, &mut ui, TextInputEvent::Insert("x".into()));
    assert!(input(&mut r, &mut ui, preedit("にほ", None)));
    assert!(input(&mut r, &mut ui, preedit("", None)));
    assert_eq!(
        r.update(|cx| e.read(cx).preedit.clone()),
        Some((String::new(), None))
    );
    assert!(input(
        &mut r,
        &mut ui,
        TextInputEvent::Commit("日本".into())
    ));
    assert_eq!(text(&mut r, &e), "日本");
    assert_eq!(r.update(|cx| e.read(cx).preedit.clone()), None);
    assert_eq!(
        take_log(&mut r, &e),
        [
            "preedit に",
            "preedit にほ",
            "preedit ",
            "insert Ime 日本",
            "root insert 日本"
        ]
    );
    // A commit does not reset the platform session.
    assert_eq!(ui.ime_reset_revision(), revision);

    // Invalid cursor endpoints are rejected like built-in preedits.
    assert!(matches!(
        ui.text_input(&mut r, preedit("に", Some((0, 1))), &mut Measure),
        Err(UiError::InvalidTextValue)
    ));

    assert!(input(&mut r, &mut ui, preedit("か", None)));
    let field = focus(&mut r, &mut ui, "field");
    assert_ne!(field, id);
    prepare(&mut r, &mut ui);
    assert_eq!(
        take_log(&mut r, &e),
        [
            "preedit か",
            "cancel FocusChanged",
            "root cancel FocusChanged"
        ]
    );
    assert_eq!(r.update(|cx| e.read(cx).preedit.clone()), None);
    assert_eq!(ui.focused_text_client(), None);
}

#[test]
fn host_pointer_deactivation_unavailability_and_removal_cancel_composition() {
    let (mut r, e, mut ui) = setup();
    let id = focus(&mut r, &mut ui, "editor");
    let compose = |r: &mut Runtime, ui: &mut Ui<Editor>| {
        prepare(r, ui);
        assert!(input(r, ui, preedit("あ", None)));
    };

    compose(&mut r, &mut ui);
    let revision = ui.ime_reset_revision();
    assert!(input(&mut r, &mut ui, TextInputEvent::CancelComposition));
    assert_ne!(ui.ime_reset_revision(), revision);
    assert!(!input(&mut r, &mut ui, TextInputEvent::CancelComposition));
    assert_eq!(
        take_log(&mut r, &e),
        ["preedit あ", "cancel Host", "root cancel Host"]
    );

    // A press on the composing client cancels before its listener places the caret,
    // even though that listener prevents the default.
    compose(&mut r, &mut ui);
    let b = ui.element(id).unwrap().bounds;
    ui.pointer(&mut r, PointerEvent::Pressed([b.x + 5., b.y + 5.]))
        .unwrap();
    ui.pointer(&mut r, PointerEvent::Released([b.x + 5., b.y + 5.]))
        .unwrap();
    assert_eq!(
        take_log(&mut r, &e),
        [
            "preedit あ",
            "cancel Pointer",
            "root cancel Pointer",
            "press"
        ]
    );
    assert_eq!(ui.focused_element(), Some(id));

    compose(&mut r, &mut ui);
    let revision = ui.ime_reset_revision();
    ui.set_active(false);
    assert!(!ui.accepts_ime() && ui.focused_text_client().is_none());
    prepare(&mut r, &mut ui);
    assert_eq!(
        take_log(&mut r, &e),
        [
            "preedit あ",
            "cancel Deactivated",
            "root cancel Deactivated"
        ]
    );
    ui.set_active(true);
    assert!(ui.accepts_ime());
    assert_ne!(ui.ime_reset_revision(), revision);

    compose(&mut r, &mut ui);
    update(&mut r, &e, |s| s.client = false);
    prepare(&mut r, &mut ui);
    // Still focusable, but no longer a client: no text, no IME.
    assert_eq!(ui.focused_element(), Some(id));
    assert!(!ui.accepts_ime() && !ui.accepts_text_input());
    assert_eq!(
        take_log(&mut r, &e),
        [
            "preedit あ",
            "cancel Unavailable",
            "root cancel Unavailable"
        ]
    );
    update(&mut r, &e, |s| s.client = true);

    compose(&mut r, &mut ui);
    update(&mut r, &e, |s| s.inert = true);
    prepare(&mut r, &mut ui);
    assert_eq!(
        take_log(&mut r, &e),
        [
            "preedit あ",
            "cancel Unavailable",
            "root cancel Unavailable"
        ]
    );
    assert_eq!(ui.focused_element(), None);
    update(&mut r, &e, |s| s.inert = false);
    focus(&mut r, &mut ui, "editor");

    // The removed client's owner still receives the cancellation.
    compose(&mut r, &mut ui);
    update(&mut r, &e, |s| s.removed = true);
    prepare(&mut r, &mut ui);
    assert_eq!(
        take_log(&mut r, &e),
        ["preedit あ", "cancel Removed", "root cancel Removed"]
    );
    assert_eq!(ui.focused_element(), None);
    assert!(!ui.accepts_ime());
}

#[test]
fn tab_reaches_client_key_listeners_and_prevent_default_keeps_focus() {
    let (mut r, e, mut ui) = setup();
    let id = focus(&mut r, &mut ui, "editor");
    // The host moves focus only when the Tab key's default was not prevented.
    let tab = |r: &mut Runtime, ui: &mut Ui<Editor>| {
        prepare(r, ui);
        if !key_down(r, ui, KeyboardKey::Tab).default_prevented {
            ui.focus_next(false);
        }
    };
    update(&mut r, &e, |s| s.capture_tab = true);
    tab(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(id));
    assert_eq!(take_log(&mut r, &e), ["key Tab"]);

    update(&mut r, &e, |s| s.capture_tab = false);
    tab(&mut r, &mut ui);
    assert_eq!(ui.focused_element(), Some(keyed(&ui, "plain")));
    assert_eq!(take_log(&mut r, &e), ["key Tab"]);
}

#[test]
fn non_clients_and_built_in_inputs_are_unaffected() {
    let (mut r, e, mut ui) = setup();
    // A focusable element with a text listener is not a client.
    let plain = focus(&mut r, &mut ui, "plain");
    assert_eq!(ui.focused_element(), Some(plain));
    assert_eq!(ui.focused_text_client(), None);
    assert!(!ui.accepts_text_input() && !ui.accepts_ime());
    assert!(!input(&mut r, &mut ui, TextInputEvent::Insert("a".into())));
    assert!(!input(&mut r, &mut ui, preedit("あ", None)));
    assert_eq!(ui.ime_cursor_area(&mut Measure).unwrap(), None);
    assert!(take_log(&mut r, &e).is_empty());

    // Built-in inputs keep their own editing; client listeners on ancestors see nothing.
    focus(&mut r, &mut ui, "field");
    assert!(ui.accepts_ime() && ui.has_text_focus());
    assert_eq!(ui.focused_text_client(), None);
    assert!(input(&mut r, &mut ui, TextInputEvent::Insert("ab".into())));
    assert!(input(&mut r, &mut ui, preedit("に", None)));
    assert!(input(&mut r, &mut ui, TextInputEvent::Commit("本".into())));
    assert_eq!(r.update(|cx| e.read(cx).field.clone()), "ab本");
    assert!(take_log(&mut r, &e).is_empty());
}

#[test]
fn ime_cursor_area_follows_the_reported_caret() {
    let (mut r, e, mut ui) = setup();
    let id = focus(&mut r, &mut ui, "editor");
    let element = ui.element(id).unwrap();
    let (bounds, content) = (element.bounds, element.content_bounds);
    assert_eq!(
        ui.ime_cursor_area(&mut Measure).unwrap(),
        Some(Bounds {
            x: bounds.x + 24.,
            y: bounds.y + 4.,
            width: 1.,
            height: 16.,
        })
    );
    update(&mut r, &e, |s| s.caret = Some(40.));
    prepare(&mut r, &mut ui);
    assert_eq!(
        ui.ime_cursor_area(&mut Measure).unwrap().map(|a| a.x),
        Some(bounds.x + 40.)
    );
    update(&mut r, &e, |s| s.caret = None);
    prepare(&mut r, &mut ui);
    assert_eq!(ui.ime_cursor_area(&mut Measure).unwrap(), Some(content));
    update(&mut r, &e, |s| s.caret = Some(f32::NAN));
    prepare(&mut r, &mut ui);
    assert!(matches!(
        ui.ime_cursor_area(&mut Measure),
        Err(UiError::InvalidGeometry)
    ));
}
