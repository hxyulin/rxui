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
fn value(runtime: &mut Runtime, root: &Entity<Form>) -> String {
    runtime.update(|cx| root.read(cx).value.clone())
}
fn edit(
    ui: &mut Ui<Form>,
    runtime: &mut Runtime,
    measure: &mut Measure,
    event: TextInputEvent,
) -> bool {
    ui.text_input(runtime, event, measure).unwrap()
}
fn info(ui: &Ui<Form>) -> TextInputInfo {
    ui.text_input_info(ui.focused_element().unwrap()).unwrap()
}
#[test]
fn undo_redo_coalesce_typing_and_deletions_but_split_paste_navigation_and_new_edits() {
    let (mut r, root, mut ui, mut m) = setup("", Policy::Accept);
    for s in ["a", "b", "c"] {
        insert(&mut ui, &mut r, &mut m, s);
    }
    assert!(info(&ui).can_undo);
    assert!(edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo));
    assert_eq!(value(&mut r, &root), "");
    assert_eq!(info(&ui).selection, TextSelection::caret(0));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Redo);
    assert_eq!(value(&mut r, &root), "abc");
    assert_eq!(info(&ui).selection, TextSelection::caret(3));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Backspace);
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Backspace);
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "abc");
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Paste("x".into()));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "abc");
    edit(
        &mut ui,
        &mut r,
        &mut m,
        TextInputEvent::Move {
            movement: TextMovement::Start,
            extend: false,
        },
    );
    insert(&mut ui, &mut r, &mut m, "q");
    assert!(!info(&ui).can_redo);
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "abc");
    assert_eq!(info(&ui).selection, TextSelection::caret(0));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "");
}
#[test]
fn normalized_typing_coalesces_using_the_accepted_selection_and_actual_values() {
    let (mut r, root, mut ui, mut m) = setup("BASE", Policy::Uppercase);
    for s in ["a", "b", "c"] {
        insert(&mut ui, &mut r, &mut m, s);
    }
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "BASE");
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Redo);
    assert_eq!(value(&mut r, &root), "BASEABC");
}
#[test]
fn controlled_history_preserves_rejected_replays_and_records_actual_normalized_values() {
    let (mut r, root, mut ui, mut m) = setup("A", Policy::Accept);
    insert(&mut ui, &mut r, &mut m, "1");
    r.update(|cx| root.update(cx, |s, _| s.policy = Policy::Digits));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "A1");
    assert!(info(&ui).can_undo && !info(&ui).can_redo);
    assert_eq!(info(&ui).selection, TextSelection::caret(2));
    r.update(|cx| root.update(cx, |s, _| s.policy = Policy::Accept));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "A");
    r.update(|cx| root.update(cx, |s, _| s.policy = Policy::Digits));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Redo);
    assert_eq!(value(&mut r, &root), "A");
    assert!(info(&ui).can_redo);
    r.update(|cx| root.update(cx, |s, _| s.policy = Policy::Uppercase));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Redo);
    insert(&mut ui, &mut r, &mut m, "b");
    assert_eq!(value(&mut r, &root), "A1B");
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "A1");
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Redo);
    assert_eq!(value(&mut r, &root), "A1B");
    // A normalizer which changes a replay cannot retain a valid previous chain.
    r.update(|cx| {
        root.update(cx, |s, _| {
            s.value = "lower".into();
        })
    });
    insert(&mut ui, &mut r, &mut m, "x");
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "LOWER");
    assert!(!info(&ui).can_undo && !info(&ui).can_redo);
}
#[test]
fn ime_commit_is_one_transaction_readonly_preserves_history_external_changes_reset_it() {
    let (mut r, root, mut ui, mut m) = setup("abc", Policy::Accept);
    edit(&mut ui, &mut r, &mut m, TextInputEvent::SelectAll);
    edit(
        &mut ui,
        &mut r,
        &mut m,
        TextInputEvent::Preedit {
            text: "に".into(),
            cursor: Some((3, 3)),
        },
    );
    assert!(!info(&ui).can_undo);
    assert!(!edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo));
    edit(
        &mut ui,
        &mut r,
        &mut m,
        TextInputEvent::Preedit {
            text: "日本".into(),
            cursor: Some((6, 6)),
        },
    );
    edit(
        &mut ui,
        &mut r,
        &mut m,
        TextInputEvent::Commit("日本".into()),
    );
    r.update(|cx| root.update(cx, |s, _| s.read_only = true));
    assert!(!edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo));
    assert_eq!(value(&mut r, &root), "日本");
    r.update(|cx| root.update(cx, |s, _| s.read_only = false));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "abc");
    assert_eq!(info(&ui).selection.range(), 0..3);
    r.update(|cx| root.update(cx, |s, _| s.value = "external".into()));
    assert!(!edit(&mut ui, &mut r, &mut m, TextInputEvent::Redo));
    assert!(!info(&ui).can_undo && !info(&ui).can_redo);
}
#[test]
fn shared_inputs_do_not_undo_another_placements_edits() {
    let (mut r, root, mut ui, mut m) = setup("", Policy::Accept);
    let mut second = Ui::new(&mut r, root.clone()).unwrap();
    second.prepare(&mut r, [400.; 2], &mut m).unwrap();
    second.focus_next(false);
    insert(&mut ui, &mut r, &mut m, "a");
    assert!(!edit(&mut second, &mut r, &mut m, TextInputEvent::Undo));
    edit(
        &mut second,
        &mut r,
        &mut m,
        TextInputEvent::Move {
            movement: TextMovement::End,
            extend: false,
        },
    );
    insert(&mut second, &mut r, &mut m, "b");
    assert!(!edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo));
    assert_eq!(value(&mut r, &root), "ab");
    edit(&mut second, &mut r, &mut m, TextInputEvent::Undo);
    assert_eq!(value(&mut r, &root), "a");
}
#[test]
fn word_and_line_deletions_respect_unicode_and_restore_directional_selections() {
    let (mut r, root, mut ui, mut m) = setup("one e\u{301} שלום 👩‍👩‍👧‍👦", Policy::Accept);
    edit(&mut ui, &mut r, &mut m, TextInputEvent::BackspaceWord);
    assert_eq!(value(&mut r, &root), "one e\u{301} ");
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    edit(
        &mut ui,
        &mut r,
        &mut m,
        TextInputEvent::Move {
            movement: TextMovement::Start,
            extend: false,
        },
    );
    edit(&mut ui, &mut r, &mut m, TextInputEvent::DeleteWord);
    assert_eq!(value(&mut r, &root), " e\u{301} שלום 👩‍👩‍👧‍👦");
    edit(&mut ui, &mut r, &mut m, TextInputEvent::DeleteToEnd);
    assert_eq!(value(&mut r, &root), "");
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    edit(
        &mut ui,
        &mut r,
        &mut m,
        TextInputEvent::Move {
            movement: TextMovement::End,
            extend: false,
        },
    );
    edit(&mut ui, &mut r, &mut m, TextInputEvent::BackspaceToStart);
    assert_eq!(value(&mut r, &root), "");
}
#[test]
fn double_click_and_drag_select_words_triple_click_keeps_the_single_line_selected() {
    let (mut r, root, mut ui, mut m) = setup("one two three", Policy::Accept);
    let b = ui.element(input_id(&ui)).unwrap().content_bounds;
    let p = |i: f32| [b.x + i * 10., b.y + 10.];
    // Initial field scroll follows the end caret. Reset it before hit testing.
    edit(
        &mut ui,
        &mut r,
        &mut m,
        TextInputEvent::Move {
            movement: TextMovement::Start,
            extend: false,
        },
    );
    ui.pointer_with_text_clicks(&mut r, PointerEvent::Pressed(p(5.)), &mut m, false, 2)
        .unwrap();
    assert_eq!(info(&ui).selection.range(), 4..7);
    ui.pointer_with_text_clicks(&mut r, PointerEvent::Moved(p(1.)), &mut m, false, 1)
        .unwrap();
    assert_eq!(info(&ui).selection.range(), 0..7);
    assert_eq!(info(&ui).selection.focus.byte_offset, 0);
    ui.pointer_with_text_clicks(&mut r, PointerEvent::Released(p(1.)), &mut m, false, 1)
        .unwrap();
    ui.pointer_with_text_clicks(&mut r, PointerEvent::Pressed(p(2.)), &mut m, false, 3)
        .unwrap();
    assert_eq!(info(&ui).selection.range(), 0..13);
    ui.pointer_with_text_clicks(&mut r, PointerEvent::Moved(p(1.)), &mut m, false, 1)
        .unwrap();
    assert_eq!(info(&ui).selection.range(), 0..13);
    assert!(r.update(|cx| root.read(cx).edits.is_empty()));
}
#[test]
fn word_selection_handles_punctuation_spaces_combining_marks_and_emoji_boundaries() {
    use crate::editing::word_selection;
    let text = "e\u{301},  👩‍👩‍👧‍👦!";
    for position in text
        .grapheme_indices(true)
        .map(|(i, _)| TextPosition::new(i))
        .chain([TextPosition::new(text.len())])
    {
        let range = word_selection(text, position).range();
        assert!(range.start <= range.end && range.end <= text.len());
        assert_eq!(crate::editing::boundary(text, range.start), range.start);
        assert_eq!(crate::editing::boundary(text, range.end), range.end);
    }
    assert_eq!(word_selection(text, TextPosition::new(0)).range(), 0..3);
    assert_eq!(word_selection(text, TextPosition::new(4)).range(), 4..6);
}
#[test]
fn history_snapshot_count_and_utf8_memory_are_bounded_without_changing_edit_acceptance() {
    let (mut r, root, mut ui, mut m) = setup("", Policy::Accept);
    for _ in 0..140 {
        edit(&mut ui, &mut r, &mut m, TextInputEvent::Paste("x".into()));
    }
    for _ in 0..128 {
        assert!(edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo));
    }
    assert!(!edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo));
    assert_eq!(value(&mut r, &root).len(), 12);
    let big = "x".repeat(1_048_577);
    r.update(|cx| root.update(cx, |s, _| s.value = big.clone()));
    edit(&mut ui, &mut r, &mut m, TextInputEvent::SelectAll);
    edit(
        &mut ui,
        &mut r,
        &mut m,
        TextInputEvent::Paste("small".into()),
    );
    assert_eq!(value(&mut r, &root), "small");
    assert!(!info(&ui).can_undo);
}
#[cfg(feature = "native")]
#[test]
fn native_replay_commands_publish_current_availability_and_respect_readonly_and_external_reset() {
    use crate::standard_commands::{Redo, Undo};
    let (mut r, root, mut ui, mut m) = setup("", Policy::Accept);
    let undo = CommandId::of::<Undo>();
    let redo = CommandId::of::<Redo>();
    assert!(!ui.native_command_info(undo).unwrap().enabled);
    insert(&mut ui, &mut r, &mut m, "x");
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert!(ui.native_command_info(undo).unwrap().enabled);
    assert!(!ui.native_command_info(redo).unwrap().enabled);
    edit(&mut ui, &mut r, &mut m, TextInputEvent::Undo);
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert!(ui.native_command_info(redo).unwrap().enabled);
    r.update(|cx| root.update(cx, |s, _| s.read_only = true));
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert!(!ui.native_command_info(redo).unwrap().enabled);
    r.update(|cx| {
        root.update(cx, |s, _| {
            s.read_only = false;
            s.value = "external".into();
        })
    });
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert!(
        !ui.native_command_info(undo).unwrap().enabled
            && !ui.native_command_info(redo).unwrap().enabled
    );
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
