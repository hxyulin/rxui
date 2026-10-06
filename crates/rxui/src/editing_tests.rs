use crate::*;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
struct Measure {
    calls: usize,
}
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        self.calls += 1;
        Ok([request.text.graphemes(true).count() as f32 * 10., 20.])
    }
    fn text_hit_test(
        &mut self,
        _: ElementId,
        request: TextRequest<'_>,
        point: [f32; 2],
    ) -> Result<Option<TextPosition>, UiError> {
        let index = (point[0].max(0.) / 10.).round() as usize;
        Ok(Some(TextPosition::new(
            request
                .text
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .nth(index)
                .unwrap_or(request.text.len()),
        )))
    }
    fn text_caret(
        &mut self,
        _: ElementId,
        request: TextRequest<'_>,
        p: TextPosition,
    ) -> Result<Option<Bounds>, UiError> {
        if !request.text.is_char_boundary(p.byte_offset) {
            return Ok(None);
        }
        Ok(Some(Bounds {
            x: request.text[..p.byte_offset].graphemes(true).count() as f32 * 10.,
            y: 0.,
            width: 1.,
            height: 20.,
        }))
    }
}
#[derive(Clone, Copy)]
enum Policy {
    Accept,
    Uppercase,
    Digits,
}
struct Form {
    value: String,
    policy: Policy,
    edits: Vec<String>,
    submissions: Vec<String>,
    reverse: bool,
    show: bool,
    disabled: bool,
    read_only: bool,
}
impl View for Form {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut fields = Vec::new();
        if self.show {
            fields.push(
                text_input(self.value.clone())
                    .key("input")
                    .width(100.)
                    .disabled(self.disabled)
                    .read_only(self.read_only)
                    .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                        this.edits.push(edit.value.clone());
                        match this.policy {
                            Policy::Accept => this.value = edit.value.clone(),
                            Policy::Uppercase => this.value = edit.value.to_uppercase(),
                            Policy::Digits if edit.value.bytes().all(|b| b.is_ascii_digit()) => {
                                this.value = edit.value.clone()
                            }
                            _ => {}
                        }
                    }))
                    .on_submit(cx.listener(|this, event: &TextSubmitEvent, _| {
                        this.submissions.push(event.value.clone())
                    })),
            );
        }
        fields.push(button("Other").key("other"));
        if self.reverse {
            fields.reverse();
        }
        column().children(fields)
    }
}
fn setup(value: &str, policy: Policy) -> (Runtime, Entity<Form>, Ui<Form>, Measure) {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Form {
            value: value.into(),
            policy,
            edits: Vec::new(),
            submissions: Vec::new(),
            reverse: false,
            show: true,
            disabled: false,
            read_only: false,
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    let mut measure = Measure::default();
    ui.prepare(&mut runtime, [400., 400.], &mut measure)
        .unwrap();
    ui.focus_next(false);
    (runtime, root, ui, measure)
}
fn input_id(ui: &Ui<Form>) -> ElementId {
    ui.elements()
        .find(|e| e.kind == ElementType::TextInput)
        .unwrap()
        .id
}
fn insert(ui: &mut Ui<Form>, runtime: &mut Runtime, measure: &mut Measure, text: &str) {
    ui.text_input(runtime, TextInputEvent::Insert(text.into()), measure)
        .unwrap();
}
#[test]
fn successive_edits_follow_live_controlled_values_without_prepare_or_effect_flush() {
    for (policy, initial, edits, expected, proposed) in [
        (
            Policy::Accept,
            "",
            ["a", "b", "c"],
            "abc",
            vec!["a", "ab", "abc"],
        ),
        (
            Policy::Uppercase,
            "",
            ["a", "b", "c"],
            "ABC",
            vec!["a", "Ab", "ABc"],
        ),
        (
            Policy::Digits,
            "12",
            ["x", "3", "4"],
            "1234",
            vec!["12x", "123", "1234"],
        ),
    ] {
        let (mut runtime, root, mut ui, mut measure) = setup(initial, policy);
        let before = ui.stats();
        let calls = measure.calls;
        for text in edits {
            insert(&mut ui, &mut runtime, &mut measure, text);
        }
        runtime.update(|cx| {
            let model = root.read(cx);
            assert_eq!(model.value, expected);
            assert_eq!(model.edits, proposed);
        });
        assert_eq!(ui.stats().layout_passes, before.layout_passes);
        assert_eq!(measure.calls, calls);
        ui.prepare(&mut runtime, [400., 400.], &mut measure)
            .unwrap();
        assert_eq!(
            ui.elements()
                .find(|e| e.kind == ElementType::TextInput)
                .unwrap()
                .text,
            Some(expected)
        );
    }
}
#[test]
fn deletion_and_movement_use_extended_graphemes_and_valid_byte_selection() {
    let (mut runtime, root, mut ui, mut measure) = setup("e\u{301}👩‍👩‍👧‍👦Z", Policy::Accept);
    ui.text_input(&mut runtime, TextInputEvent::Backspace, &mut measure)
        .unwrap();
    ui.text_input(&mut runtime, TextInputEvent::Backspace, &mut measure)
        .unwrap();
    assert_eq!(runtime.update(|cx| root.read(cx).value.clone()), "e\u{301}");
    ui.text_input(
        &mut runtime,
        TextInputEvent::Move {
            movement: TextMovement::Start,
            extend: false,
        },
        &mut measure,
    )
    .unwrap();
    ui.text_input(&mut runtime, TextInputEvent::Delete, &mut measure)
        .unwrap();
    assert_eq!(runtime.update(|cx| root.read(cx).value.clone()), "");
    insert(&mut ui, &mut runtime, &mut measure, "a👋b");
    ui.text_input(
        &mut runtime,
        TextInputEvent::Move {
            movement: TextMovement::Left,
            extend: true,
        },
        &mut measure,
    )
    .unwrap();
    ui.text_input(
        &mut runtime,
        TextInputEvent::Move {
            movement: TextMovement::Left,
            extend: true,
        },
        &mut measure,
    )
    .unwrap();
    assert_eq!(ui.selected_text(), Some("👋b"));
    insert(&mut ui, &mut runtime, &mut measure, "X");
    assert_eq!(runtime.update(|cx| root.read(cx).value.clone()), "aX");
}
#[test]
fn pointer_drag_and_shift_click_extend_selection_without_model_changes() {
    let (mut runtime, root, mut ui, mut measure) = setup("abcdef", Policy::Accept);
    let before = ui.stats();
    ui.pointer_with_text(
        &mut runtime,
        PointerEvent::Pressed([20., 20.]),
        &mut measure,
        false,
    )
    .unwrap();
    ui.pointer_with_text(
        &mut runtime,
        PointerEvent::Moved([50., 20.]),
        &mut measure,
        false,
    )
    .unwrap();
    ui.pointer_with_text(
        &mut runtime,
        PointerEvent::Released([50., 20.]),
        &mut measure,
        false,
    )
    .unwrap();
    assert_eq!(ui.selected_text(), Some("bcd"));
    ui.pointer_with_text(
        &mut runtime,
        PointerEvent::Pressed([70., 20.]),
        &mut measure,
        true,
    )
    .unwrap();
    assert_eq!(ui.selected_text(), Some("bcdef"));
    assert_eq!(ui.stats(), before);
    assert!(runtime.update(|cx| root.read(cx).edits.is_empty()));
}
#[test]
fn ime_preedit_is_transient_empty_clear_preserves_replacement_and_commit_applies_once() {
    let (mut runtime, root, mut ui, mut measure) = setup("Hello", Policy::Accept);
    ui.text_input(&mut runtime, TextInputEvent::SelectAll, &mut measure)
        .unwrap();
    for (text, cursor) in [("你", Some((3, 3))), ("你好", Some((6, 6))), ("", None)] {
        ui.text_input(
            &mut runtime,
            TextInputEvent::Preedit {
                text: text.into(),
                cursor,
            },
            &mut measure,
        )
        .unwrap();
        assert_eq!(runtime.update(|cx| root.read(cx).value.clone()), "Hello");
        assert!(runtime.update(|cx| root.read(cx).edits.is_empty()));
    }
    ui.text_input(
        &mut runtime,
        TextInputEvent::Commit("你好".into()),
        &mut measure,
    )
    .unwrap();
    assert_eq!(
        runtime.update(|cx| root.read(cx).edits.clone()),
        vec!["你好"]
    );
    assert_eq!(runtime.update(|cx| root.read(cx).value.clone()), "你好");
}
#[test]
fn external_changes_cancel_composition_and_clamp_selection_on_shared_placement() {
    let (mut runtime, root, mut ui, mut measure) = setup("abcdef", Policy::Accept);
    let mut other = Ui::new(&mut runtime, root.clone()).unwrap();
    other
        .prepare(&mut runtime, [400., 400.], &mut measure)
        .unwrap();
    other.focus_next(false);
    ui.text_input(
        &mut runtime,
        TextInputEvent::Preedit {
            text: "你".into(),
            cursor: Some((3, 3)),
        },
        &mut measure,
    )
    .unwrap();
    let ime_reset_before = ui.ime_reset_revision();
    insert(&mut other, &mut runtime, &mut measure, "X");
    ui.prepare(&mut runtime, [400., 400.], &mut measure)
        .unwrap();
    assert!(ui.ime_reset_revision() > ime_reset_before);
    let snapshot = ui.element(input_id(&ui)).unwrap();
    assert_eq!(snapshot.text, Some("abcdefX"));
    assert!(snapshot.editing.unwrap().preedit_range.is_none());
    runtime.update(|cx| root.update(cx, |this, _| this.value = "e\u{301}".into()));
    ui.text_input(&mut runtime, TextInputEvent::Backspace, &mut measure)
        .unwrap();
    assert_eq!(runtime.update(|cx| root.read(cx).value.clone()), "");
}
#[test]
fn selection_is_independent_between_placements_and_survives_keyed_reorder() {
    let (mut runtime, root, mut ui, mut measure) = setup("abc", Policy::Accept);
    let mut other = Ui::new(&mut runtime, root.clone()).unwrap();
    other
        .prepare(&mut runtime, [400., 400.], &mut measure)
        .unwrap();
    other.focus_next(false);
    let id = input_id(&ui);
    ui.text_input(&mut runtime, TextInputEvent::SelectAll, &mut measure)
        .unwrap();
    assert_eq!(ui.selected_text(), Some("abc"));
    assert!(other.selected_text().is_none());
    runtime.update(|cx| root.update(cx, |this, _| this.reverse = true));
    ui.prepare(&mut runtime, [400., 400.], &mut measure)
        .unwrap();
    assert_eq!(input_id(&ui), id);
    assert_eq!(ui.selected_text(), Some("abc"));
    runtime.update(|cx| root.update(cx, |this, _| this.show = false));
    assert!(
        !ui.text_input(
            &mut runtime,
            TextInputEvent::Insert("X".into()),
            &mut measure
        )
        .unwrap()
    );
    assert!(ui.focused_element().is_none());
}
#[test]
fn readonly_disabled_and_focus_loss_prevent_edits_and_cancel_preedit() {
    let (mut runtime, root, mut ui, mut measure) = setup("abc", Policy::Accept);
    ui.text_input(
        &mut runtime,
        TextInputEvent::Preedit {
            text: "你".into(),
            cursor: Some((3, 3)),
        },
        &mut measure,
    )
    .unwrap();
    ui.set_active(false);
    assert!(
        !ui.text_input(
            &mut runtime,
            TextInputEvent::Commit("X".into()),
            &mut measure
        )
        .unwrap()
    );
    ui.set_active(true);
    runtime.update(|cx| root.update(cx, |this, _| this.read_only = true));
    assert!(
        !ui.text_input(
            &mut runtime,
            TextInputEvent::Insert("X".into()),
            &mut measure
        )
        .unwrap()
    );
    ui.text_input(&mut runtime, TextInputEvent::SelectAll, &mut measure)
        .unwrap();
    assert_eq!(ui.selected_text(), Some("abc"));
    assert!(!ui.accepts_text_input());
    runtime.update(|cx| root.update(cx, |this, _| this.disabled = true));
    assert!(
        !ui.text_input(&mut runtime, TextInputEvent::Backspace, &mut measure)
            .unwrap()
    );
    assert!(ui.focused_element().is_none());
    assert_eq!(runtime.update(|cx| root.read(cx).value.clone()), "abc");
}
#[test]
fn submission_ignores_composition_and_user_paste_normalizes_line_breaks() {
    let (mut runtime, root, mut ui, mut measure) = setup("", Policy::Accept);
    insert(&mut ui, &mut runtime, &mut measure, "a\r\nb\tc\u{1}");
    assert_eq!(runtime.update(|cx| root.read(cx).value.clone()), "a b c");
    ui.text_input(
        &mut runtime,
        TextInputEvent::Preedit {
            text: "你".into(),
            cursor: None,
        },
        &mut measure,
    )
    .unwrap();
    assert!(
        !ui.text_input(&mut runtime, TextInputEvent::Submit, &mut measure)
            .unwrap()
    );
    ui.text_input(
        &mut runtime,
        TextInputEvent::CancelComposition,
        &mut measure,
    )
    .unwrap();
    ui.text_input(&mut runtime, TextInputEvent::Submit, &mut measure)
        .unwrap();
    assert_eq!(
        runtime.update(|cx| root.read(cx).submissions.clone()),
        vec!["a b c"]
    );
}
#[test]
fn invalid_ime_offsets_are_errors_and_no_pending_edit_is_applied() {
    let (mut runtime, root, mut ui, mut measure) = setup("", Policy::Accept);
    assert!(matches!(
        ui.text_input(
            &mut runtime,
            TextInputEvent::Preedit {
                text: "你".into(),
                cursor: Some((1, 2))
            },
            &mut measure
        ),
        Err(UiError::InvalidTextValue)
    ));
    assert!(runtime.update(|cx| root.read(cx).edits.is_empty()));
}
