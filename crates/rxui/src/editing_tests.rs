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

/// Line-based geometry: 10 units per grapheme, 20 per `\n`-separated line, no wrapping.
struct LineMeasure;
impl LineMeasure {
    fn line_of(text: &str, offset: usize) -> (usize, usize) {
        let line = text[..offset].matches('\n').count();
        let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
        (line, text[start..offset].graphemes(true).count())
    }
    fn is_boundary(text: &str, offset: usize) -> bool {
        offset == text.len() || text.grapheme_indices(true).any(|(i, _)| i == offset)
    }
}
impl TextMeasure for LineMeasure {
    fn measure(&mut self, _: ElementId, request: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        let lines = request.text.split('\n');
        let width = lines
            .clone()
            .map(|l| l.graphemes(true).count())
            .max()
            .unwrap_or(0);
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
        let lines: Vec<_> = request.text.split('\n').collect();
        let line = ((point[1] / 20.).floor().max(0.) as usize).min(lines.len() - 1);
        let start: usize = lines[..line].iter().map(|l| l.len() + 1).sum();
        let column = (point[0].max(0.) / 10.).round() as usize;
        Ok(Some(TextPosition::new(
            lines[line]
                .grapheme_indices(true)
                .nth(column)
                .map_or(start + lines[line].len(), |(i, _)| start + i),
        )))
    }
    fn text_caret(
        &mut self,
        _: ElementId,
        request: TextRequest<'_>,
        p: TextPosition,
    ) -> Result<Option<Bounds>, UiError> {
        if !Self::is_boundary(request.text, p.byte_offset) {
            return Ok(None);
        }
        let (line, column) = Self::line_of(request.text, p.byte_offset);
        Ok(Some(Bounds {
            x: column as f32 * 10.,
            y: line as f32 * 20.,
            width: 1.,
            height: 20.,
        }))
    }
    fn text_neighbor(
        &mut self,
        _: ElementId,
        request: TextRequest<'_>,
        p: TextPosition,
        right: bool,
    ) -> Result<Option<TextPosition>, UiError> {
        let text = request.text;
        Ok(if right {
            text.grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([text.len()])
                .find(|i| *i > p.byte_offset)
        } else {
            text.grapheme_indices(true)
                .map(|(i, _)| i)
                .take_while(|i| *i < p.byte_offset)
                .last()
        }
        .map(TextPosition::new))
    }
}
struct Doc {
    text: String,
    password: String,
    shortcut: SubmitShortcut,
    submissions: Vec<String>,
    selectable: bool,
}
impl View for Doc {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .child(
                column().key("scroll").height(100.).scroll_y().child(
                    text_area(self.text.clone())
                        .key("area")
                        .width(300.)
                        .padding(0.)
                        .border(0., ThemeColor::InputBorder)
                        .min_height(0.)
                        .placeholder("Notes")
                        .accessibility_label("Notes")
                        .submit_shortcut(self.shortcut)
                        .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                            this.text = edit.value.clone();
                        }))
                        .on_submit(cx.listener(|this, event: &TextSubmitEvent, _| {
                            this.submissions.push(event.value.clone());
                        })),
                ),
            )
            .child(
                text_input(self.password.clone())
                    .key("password")
                    .width(200.)
                    .password(true)
                    .placeholder("Password")
                    .accessibility_label("Password")
                    .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                        this.password = edit.value.clone();
                    })),
            )
            .child(
                label("alpha beta gamma")
                    .key("static")
                    .selectable(self.selectable)
                    .padding(0.),
            )
            .child(button("Other").key("other"))
    }
}
fn doc_setup<M: TextMeasure>(text: &str, mut measure: M) -> (Runtime, Entity<Doc>, Ui<Doc>, M) {
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| {
        cx.new(|_| Doc {
            text: text.into(),
            password: String::new(),
            shortcut: SubmitShortcut::default(),
            submissions: Vec::new(),
            selectable: true,
        })
    });
    let mut ui = Ui::new(&mut runtime, root.clone()).unwrap();
    ui.prepare(&mut runtime, [400., 400.], &mut measure)
        .unwrap();
    (runtime, root, ui, measure)
}
fn keyed<T: View>(ui: &Ui<T>, key: &str) -> ElementId {
    let key = Key::from(key);
    ui.elements().find(|e| e.key == Some(&key)).unwrap().id
}
fn doc_text(runtime: &mut Runtime, root: &Entity<Doc>) -> String {
    runtime.update(|cx| root.read(cx).text.clone())
}
fn movement(movement: TextMovement, extend: bool) -> TextInputEvent {
    TextInputEvent::Move { movement, extend }
}
fn caret<T: View>(ui: &Ui<T>) -> usize {
    ui.text_input_info(ui.focused_element().unwrap())
        .unwrap()
        .selection
        .focus
        .byte_offset
}
fn enter(modifiers: Modifiers, repeat: bool) -> TextInputEvent {
    TextInputEvent::Enter { modifiers, repeat }
}
#[test]
fn text_area_enter_inserts_line_breaks_and_the_submit_shortcut_submits() {
    let (mut r, root, mut ui, mut m) = doc_setup("", Measure::default());
    assert!(ui.focus(keyed(&ui, "area")));
    ui.text_input(&mut r, TextInputEvent::Insert("ab".into()), &mut m)
        .unwrap();
    assert!(
        ui.text_input(&mut r, enter(Modifiers::default(), false), &mut m)
            .unwrap()
    );
    // Held Enter keeps inserting line breaks.
    ui.text_input(&mut r, enter(Modifiers::default(), true), &mut m)
        .unwrap();
    assert_eq!(doc_text(&mut r, &root), "ab\n\n");
    let primary = crate::Shortcut::primary("").modifiers;
    // A repeated submit chord does not submit again.
    assert!(!ui.text_input(&mut r, enter(primary, true), &mut m).unwrap());
    ui.text_input(&mut r, enter(primary, false), &mut m)
        .unwrap();
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    ui.text_input(&mut r, enter(shift, false), &mut m).unwrap();
    assert_eq!(doc_text(&mut r, &root), "ab\n\n\n");
    r.update(|cx| root.update(cx, |d, _| d.shortcut = SubmitShortcut::Enter));
    ui.text_input(&mut r, enter(Modifiers::default(), false), &mut m)
        .unwrap();
    ui.text_input(&mut r, enter(shift, false), &mut m).unwrap();
    r.update(|cx| root.update(cx, |d, _| d.shortcut = SubmitShortcut::Never));
    ui.text_input(&mut r, enter(primary, false), &mut m)
        .unwrap();
    assert_eq!(doc_text(&mut r, &root), "ab\n\n\n\n\n");
    assert_eq!(
        r.update(|cx| root.read(cx).submissions.clone()),
        vec!["ab\n\n", "ab\n\n\n"]
    );
    // Pasted line breaks normalize to LF; tabs remain and other controls go.
    ui.text_input(
        &mut r,
        TextInputEvent::Paste("x\r\ny\rz\tw\u{1}".into()),
        &mut m,
    )
    .unwrap();
    assert_eq!(doc_text(&mut r, &root), "ab\n\n\n\n\nx\ny\nz\tw");
    ui.text_input(&mut r, TextInputEvent::Undo, &mut m).unwrap();
    assert_eq!(doc_text(&mut r, &root), "ab\n\n\n\n\n");
    ui.text_input(&mut r, TextInputEvent::Redo, &mut m).unwrap();
    assert_eq!(doc_text(&mut r, &root), "ab\n\n\n\n\nx\ny\nz\tw");
}
#[test]
fn single_line_inputs_submit_on_enter_but_not_on_repeats() {
    let (mut runtime, root, mut ui, mut measure) = setup("abc", Policy::Accept);
    assert!(
        !ui.text_input(
            &mut runtime,
            enter(Modifiers::default(), true),
            &mut measure
        )
        .unwrap()
    );
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    ui.text_input(&mut runtime, enter(shift, false), &mut measure)
        .unwrap();
    assert_eq!(value(&mut runtime, &root), "abc");
    assert_eq!(
        runtime.update(|cx| root.read(cx).submissions.clone()),
        vec!["abc"]
    );
}
#[test]
fn text_area_values_allow_line_breaks_and_tabs_but_no_other_controls() {
    let (mut r, root, mut ui, mut m) = doc_setup("a\n\tb", Measure::default());
    r.update(|cx| root.update(cx, |d, _| d.text = "a\r\nb".into()));
    assert!(matches!(
        ui.prepare(&mut r, [400.; 2], &mut m),
        Err(UiError::InvalidTextValue)
    ));
    r.update(|cx| root.update(cx, |d, _| d.text = "a\nb".into()));
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
}
#[test]
fn vertical_movement_keeps_the_goal_column_and_line_edges_stay_on_the_line() {
    // Shaped geometry and the logical fallback for hosts without it agree here.
    fn check<M: TextMeasure>(measure: M, geometry: bool) {
        let text = "abcdef\nab\nabcdef";
        let (mut r, _, mut ui, mut m) = doc_setup(text, measure);
        let mut send = |ui: &mut Ui<Doc>, event| ui.text_input(&mut r, event, &mut m).unwrap();
        let range = |ui: &Ui<Doc>| {
            ui.text_input_info(keyed(ui, "area"))
                .unwrap()
                .selection
                .range()
        };
        assert!(ui.focus(keyed(&ui, "area")));
        send(&mut ui, movement(TextMovement::Start, false));
        for _ in 0..5 {
            send(&mut ui, movement(TextMovement::Right, false));
        }
        assert_eq!(caret(&ui), 5, "geometry {geometry}");
        send(&mut ui, movement(TextMovement::Down, false));
        assert_eq!(caret(&ui), 9, "short line clamps, geometry {geometry}");
        send(&mut ui, movement(TextMovement::Down, false));
        assert_eq!(caret(&ui), 15, "goal column returns, geometry {geometry}");
        send(&mut ui, movement(TextMovement::Down, false));
        assert_eq!(caret(&ui), text.len(), "last line reaches the end");
        send(&mut ui, movement(TextMovement::Up, true));
        assert_eq!(range(&ui), 9..16, "geometry {geometry}");
        // Horizontal movement resets the goal.
        send(&mut ui, movement(TextMovement::Left, false));
        send(&mut ui, movement(TextMovement::Up, false));
        assert_eq!(caret(&ui), 2, "geometry {geometry}");
        send(&mut ui, movement(TextMovement::Up, false));
        assert_eq!(caret(&ui), 0, "first line reaches the start");
        send(&mut ui, movement(TextMovement::Down, false));
        send(&mut ui, movement(TextMovement::LineEnd, false));
        assert_eq!(caret(&ui), 9, "geometry {geometry}");
        send(&mut ui, movement(TextMovement::LineStart, true));
        assert_eq!(range(&ui), 7..9, "geometry {geometry}");
    }
    check(Measure::default(), false);
    check(LineMeasure, true);
}
#[test]
fn page_movement_and_edits_reveal_the_caret_in_the_scrolling_ancestor() {
    let text = (0..20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (mut r, root, mut ui, mut m) = doc_setup(&text, LineMeasure);
    let scroll = keyed(&ui, "scroll");
    assert!(ui.focus(keyed(&ui, "area")));
    ui.text_input(&mut r, movement(TextMovement::Start, false), &mut m)
        .unwrap();
    // A 100-unit viewport pages by 80 units: four 20-unit lines.
    ui.text_input(&mut r, movement(TextMovement::PageDown, false), &mut m)
        .unwrap();
    assert_eq!(caret(&ui), text.find("line 4").unwrap());
    assert_eq!(ui.element(scroll).unwrap().scroll_offset, [0., 0.]);
    ui.text_input(&mut r, movement(TextMovement::PageDown, false), &mut m)
        .unwrap();
    assert_eq!(caret(&ui), text.find("line 8").unwrap());
    // Line 8 occupies 160..180, so the viewport scrolls it into view at once.
    assert_eq!(ui.element(scroll).unwrap().scroll_offset, [0., 80.]);
    ui.text_input(&mut r, movement(TextMovement::End, false), &mut m)
        .unwrap();
    assert_eq!(ui.element(scroll).unwrap().scroll_offset, [0., 300.]);
    // An edit that grows the area reveals the new caret after layout.
    ui.text_input(&mut r, enter(Modifiers::default(), false), &mut m)
        .unwrap();
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert_eq!(ui.element(scroll).unwrap().scroll_offset, [0., 320.]);
    assert!(doc_text(&mut r, &root).ends_with("line 19\n"));
    ui.text_input(&mut r, movement(TextMovement::PageUp, false), &mut m)
        .unwrap();
    assert_eq!(caret(&ui), text.find("line 16").unwrap());
    // Wheel scrolling elsewhere is not undone by later preparation.
    ui.scroll([10., 10.], [0., -200.]).unwrap();
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert_eq!(ui.element(scroll).unwrap().scroll_offset, [0., 120.]);
}
#[test]
fn text_area_clicks_select_words_and_paragraphs() {
    let (mut r, root, mut ui, mut m) = doc_setup("one two\nthree four\nfive", LineMeasure);
    let b = ui.element(keyed(&ui, "area")).unwrap().content_bounds;
    let at = |column: f32, line: f32| [b.x + column * 10. + 2., b.y + line * 20. + 10.];
    let mut click = |ui: &mut Ui<Doc>, point, count| {
        ui.pointer_with_text_clicks(&mut r, PointerEvent::Pressed(point), &mut m, false, count)
            .unwrap();
        ui.pointer_with_text_clicks(&mut r, PointerEvent::Released(point), &mut m, false, 1)
            .unwrap();
    };
    click(&mut ui, at(7., 1.), 2);
    assert_eq!(ui.selected_text(), Some("four"));
    click(&mut ui, at(1., 1.), 3);
    assert_eq!(ui.selected_text(), Some("three four\n"));
    click(&mut ui, at(1., 2.), 3);
    assert_eq!(ui.selected_text(), Some("five"));
    // Word movement crosses line breaks.
    ui.text_input(&mut r, movement(TextMovement::Start, false), &mut m)
        .unwrap();
    for _ in 0..3 {
        ui.text_input(&mut r, movement(TextMovement::WordRight, false), &mut m)
            .unwrap();
    }
    assert_eq!(caret(&ui), "one two\nthree".len());
    ui.text_input(&mut r, movement(TextMovement::WordLeft, false), &mut m)
        .unwrap();
    ui.text_input(&mut r, movement(TextMovement::WordLeft, false), &mut m)
        .unwrap();
    assert_eq!(caret(&ui), "one ".len());
    assert!(r.update(|cx| root.read(cx).submissions.is_empty()));
}
#[test]
fn large_documents_keep_deep_history_and_typing_inside_them_restores_exactly() {
    // About 200 KB: whole-value snapshots would allow only five undo steps.
    let text: String = (0..5000)
        .map(|i| format!("line {i:05} of the long document.\n"))
        .collect();
    let (mut r, root, mut ui, mut m) = doc_setup(&text, LineMeasure);
    assert!(ui.focus(keyed(&ui, "area")));
    let b = ui.element(keyed(&ui, "area")).unwrap().content_bounds;
    for i in 0..130 {
        ui.text_input(&mut r, movement(TextMovement::Start, false), &mut m)
            .unwrap();
        for _ in 0..i % 7 {
            ui.text_input(&mut r, movement(TextMovement::Down, false), &mut m)
                .unwrap();
        }
        ui.text_input(&mut r, TextInputEvent::Paste(format!("<{i}>")), &mut m)
            .unwrap();
    }
    for _ in 0..128 {
        assert!(ui.text_input(&mut r, TextInputEvent::Undo, &mut m).unwrap());
    }
    assert!(!ui.text_input(&mut r, TextInputEvent::Undo, &mut m).unwrap());
    let mut expected = text.clone();
    expected.insert_str(0, "<0>");
    expected.insert_str(expected.find("line 00001").unwrap(), "<1>");
    assert_eq!(doc_text(&mut r, &root), expected);
    for _ in 0..128 {
        ui.text_input(&mut r, TextInputEvent::Redo, &mut m).unwrap();
    }
    let redone = doc_text(&mut r, &root);
    assert!(redone.starts_with("<126><119><112>"));
    // Coalesced typing and deletion in the middle of the document undo as one
    // step, restoring the exact previous value and selection.
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Pressed([b.x + 52., b.y + 3. * 20. + 10.]),
        &mut m,
        false,
    )
    .unwrap();
    ui.pointer_with_text(
        &mut r,
        PointerEvent::Released([b.x + 52., b.y + 3. * 20. + 10.]),
        &mut m,
        false,
    )
    .unwrap();
    let before = caret(&ui);
    for s in ["x", "y", "z"] {
        ui.text_input(&mut r, TextInputEvent::Insert(s.into()), &mut m)
            .unwrap();
    }
    ui.text_input(&mut r, TextInputEvent::Backspace, &mut m)
        .unwrap();
    for s in ["q", "r"] {
        ui.text_input(&mut r, TextInputEvent::Insert(s.into()), &mut m)
            .unwrap();
    }
    let edited = doc_text(&mut r, &root);
    assert_eq!(&edited[before..before + 4], "xyqr");
    ui.text_input(&mut r, TextInputEvent::Undo, &mut m).unwrap();
    let undone = doc_text(&mut r, &root);
    assert_eq!(&undone[before..before + 2], "xy");
    assert_eq!(undone[before + 2..], edited[before + 4..]);
    ui.text_input(&mut r, TextInputEvent::Undo, &mut m).unwrap();
    ui.text_input(&mut r, TextInputEvent::Undo, &mut m).unwrap();
    assert_eq!(doc_text(&mut r, &root), redone);
    assert_eq!(caret(&ui), before);
    ui.text_input(&mut r, TextInputEvent::Redo, &mut m).unwrap();
    ui.text_input(&mut r, TextInputEvent::Redo, &mut m).unwrap();
    ui.text_input(&mut r, TextInputEvent::Redo, &mut m).unwrap();
    assert_eq!(doc_text(&mut r, &root), edited);
}
#[test]
fn local_grapheme_boundaries_match_whole_value_segmentation() {
    use crate::editing::{after_boundary, boundary, next, previous};
    let text = "a\r\nb🇯🇵🇫🇷e\u{301}👩‍👩‍👧‍👦\n\nשלום x";
    let starts: Vec<_> = text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    for offset in 0..=text.len() {
        let floor = *starts.iter().rfind(|i| **i <= offset).unwrap();
        assert_eq!(boundary(text, offset), floor.min(text.len()), "{offset}");
        let before = starts.iter().rfind(|i| **i < offset).copied();
        assert_eq!(previous(text, offset), before.unwrap_or(0), "{offset}");
        let after = starts.iter().find(|i| **i > offset).copied();
        assert_eq!(next(text, offset), after.unwrap_or(text.len()), "{offset}");
        let ceiling = starts.iter().find(|i| **i >= offset).copied();
        assert_eq!(
            after_boundary(text, offset),
            ceiling.unwrap_or(text.len()),
            "{offset}"
        );
    }
}
#[test]
fn password_inputs_mask_display_and_hide_words_clipboard_and_composition() {
    let (mut r, root, mut ui, mut m) = doc_setup("", Measure::default());
    let id = keyed(&ui, "password");
    assert!(ui.focus(id));
    assert!(ui.accepts_text_input() && !ui.accepts_ime());
    ui.text_input(&mut r, TextInputEvent::Insert("pa".into()), &mut m)
        .unwrap();
    ui.text_input(&mut r, TextInputEvent::Insert(" e\u{301}".into()), &mut m)
        .unwrap();
    assert_eq!(r.update(|cx| root.read(cx).password.clone()), "pa e\u{301}");
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert_eq!(ui.element(id).unwrap().text, Some("••••"));
    // Composition is refused; committed IME text still arrives.
    assert!(
        !ui.text_input(
            &mut r,
            TextInputEvent::Preedit {
                text: "に".into(),
                cursor: None
            },
            &mut m
        )
        .unwrap()
    );
    assert!(ui.text_input_info(id).unwrap().preedit_range.is_none());
    ui.text_input(&mut r, movement(TextMovement::WordLeft, true), &mut m)
        .unwrap();
    let info = ui.text_input_info(id).unwrap();
    assert_eq!(info.selection.range(), 0.."pa e\u{301}".len());
    assert_eq!(info.display_selection.range(), 0.."••••".len());
    assert!(info.masked);
    assert_eq!(ui.selected_text(), None);
    ui.text_input(&mut r, movement(TextMovement::End, false), &mut m)
        .unwrap();
    ui.text_input(&mut r, TextInputEvent::BackspaceWord, &mut m)
        .unwrap();
    assert_eq!(r.update(|cx| root.read(cx).password.clone()), "");
    ui.text_input(&mut r, TextInputEvent::Commit("xy z".into()), &mut m)
        .unwrap();
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    let b = ui.element(id).unwrap().content_bounds;
    // Double-click selects the whole value rather than a word.
    ui.pointer_with_text_clicks(
        &mut r,
        PointerEvent::Pressed([b.x + 1., b.y + 5.]),
        &mut m,
        false,
        2,
    )
    .unwrap();
    assert_eq!(ui.text_input_info(id).unwrap().selection.range(), 0..4);
    let node = ui.semantic_node(id).unwrap();
    assert_eq!(node.role, SemanticRole::PasswordInput);
    assert_eq!(node.value, Some("••••"));
    assert_eq!(node.selection.unwrap().range(), 0.."••••".len());
    assert_eq!(node.placeholder, Some("Password"));
    // Assistive selection uses offsets into the published mask.
    let revision = node.text_revision;
    ui.semantic_action(
        &mut r,
        SemanticAction::SetSelection {
            target: id,
            text_revision: revision,
            selection: TextSelection {
                anchor: TextPosition::new(3),
                focus: TextPosition::new(9),
            },
        },
        &mut m,
    )
    .unwrap();
    assert_eq!(ui.text_input_info(id).unwrap().selection.range(), 1..3);
}
#[cfg(feature = "native")]
#[test]
fn password_and_selectable_label_clipboard_availability() {
    use crate::standard_commands::{Copy, Cut, Paste};
    let (mut r, _, mut ui, mut m) = doc_setup("", Measure::default());
    let password = keyed(&ui, "password");
    let label = keyed(&ui, "static");
    assert!(ui.focus(password));
    ui.text_input(&mut r, TextInputEvent::Insert("secret".into()), &mut m)
        .unwrap();
    ui.text_input(&mut r, TextInputEvent::SelectAll, &mut m)
        .unwrap();
    assert!(
        !ui.native_command_info(CommandId::of::<Copy>())
            .unwrap()
            .enabled
    );
    assert!(
        !ui.native_command_info(CommandId::of::<Cut>())
            .unwrap()
            .enabled
    );
    assert!(
        ui.native_command_info(CommandId::of::<Paste>())
            .unwrap()
            .enabled
    );
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert!(ui.focus(label));
    ui.text_input(&mut r, TextInputEvent::SelectAll, &mut m)
        .unwrap();
    assert!(
        ui.native_command_info(CommandId::of::<Copy>())
            .unwrap()
            .enabled
    );
    assert!(
        !ui.native_command_info(CommandId::of::<Cut>())
            .unwrap()
            .enabled
    );
    assert!(
        !ui.native_command_info(CommandId::of::<Paste>())
            .unwrap()
            .enabled
    );
}
#[test]
fn placeholders_show_only_while_empty_and_not_composing() {
    let (mut r, root, mut ui, mut m) = doc_setup("", Measure::default());
    let area = keyed(&ui, "area");
    assert_eq!(ui.element(area).unwrap().placeholder, Some("Notes"));
    assert_eq!(ui.element(area).unwrap().text, Some(""));
    let muted = ui.theme().palette().text_muted;
    assert_eq!(ui.text_input_info(area).unwrap().placeholder_color, muted);
    assert!(ui.focus(area));
    ui.text_input(
        &mut r,
        TextInputEvent::Preedit {
            text: "に".into(),
            cursor: None,
        },
        &mut m,
    )
    .unwrap();
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert_eq!(ui.element(area).unwrap().placeholder, None);
    ui.text_input(&mut r, TextInputEvent::Commit("に".into()), &mut m)
        .unwrap();
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert_eq!(ui.element(area).unwrap().placeholder, None);
    // The accessible placeholder stays published, separately from name and value.
    let node = ui.semantic_node(area).unwrap();
    assert_eq!(node.placeholder, Some("Notes"));
    assert_eq!(node.label, Some("Notes"));
    assert_eq!(node.role, SemanticRole::MultilineTextInput);
    r.update(|cx| root.update(cx, |d, _| d.text.clear()));
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert_eq!(ui.element(area).unwrap().placeholder, Some("Notes"));
}
#[test]
fn selectable_labels_select_and_copy_without_tab_stops_or_edits() {
    let (mut r, root, mut ui, mut m) = doc_setup("", Measure::default());
    let id = keyed(&ui, "static");
    // Tab traversal skips the label.
    for _ in 0..4 {
        ui.focus_next(false);
        assert_ne!(ui.focused_element(), Some(id));
    }
    let b = ui.element(id).unwrap().content_bounds;
    let at = |i: f32| [b.x + i * 10. + 1., b.y + 5.];
    ui.pointer(&mut r, PointerEvent::Moved(at(1.))).unwrap();
    assert_eq!(ui.cursor(), Cursor::Text);
    ui.pointer_with_text(&mut r, PointerEvent::Pressed(at(6.)), &mut m, false)
        .unwrap();
    assert_eq!(ui.focused_element(), Some(id));
    ui.pointer_with_text(&mut r, PointerEvent::Moved(at(10.)), &mut m, false)
        .unwrap();
    ui.pointer_with_text(&mut r, PointerEvent::Released(at(10.)), &mut m, false)
        .unwrap();
    assert_eq!(ui.selected_text(), Some("beta"));
    let info = ui.text_input_info(id).unwrap();
    assert!(info.read_only && !info.caret_visible && !info.can_undo);
    assert!(!ui.accepts_text_input() && ui.has_text_focus());
    assert!(
        !ui.text_input(&mut r, TextInputEvent::Insert("x".into()), &mut m)
            .unwrap()
    );
    ui.pointer_with_text_clicks(&mut r, PointerEvent::Pressed(at(12.)), &mut m, false, 2)
        .unwrap();
    assert_eq!(ui.selected_text(), Some("gamma"));
    ui.text_input(&mut r, movement(TextMovement::Left, true), &mut m)
        .unwrap();
    assert_eq!(ui.selected_text(), Some("gamm"));
    ui.text_input(&mut r, TextInputEvent::SelectAll, &mut m)
        .unwrap();
    assert_eq!(ui.selected_text(), Some("alpha beta gamma"));
    let node = ui.semantic_node(id).unwrap();
    assert_eq!(node.role, SemanticRole::Label);
    assert!(!node.editable && node.selection.is_some());
    // Focus elsewhere collapses the label's selection.
    ui.focus(keyed(&ui, "other"));
    assert!(ui.text_input_info(id).unwrap().selection.is_collapsed());
    // Turning selection off removes the editing state.
    r.update(|cx| root.update(cx, |d, _| d.selectable = false));
    ui.prepare(&mut r, [400.; 2], &mut m).unwrap();
    assert!(ui.text_input_info(id).is_none());
    assert!(!ui.focus(id));
}
#[test]
fn control_character_validation_scans_blocks_and_allows_text_area_breaks() {
    use crate::editing::has_control;
    assert!(!has_control("plain é © ° text", false));
    assert!(has_control("tab\there", false));
    assert!(!has_control("tab\there\nline", true));
    assert!(has_control("cr\r", true));
    assert!(has_control("del\u{7f}", true));
    // A C1 control (0xC2 0x85) at every alignment, including across 64-byte blocks.
    for pad in 0..70 {
        let text = format!("{}\u{85}", "x".repeat(pad));
        assert!(has_control(&text, true), "{pad}");
        let text = format!("{}\u{a0}{}", "x".repeat(pad), "y".repeat(70));
        assert!(!has_control(&text, true), "{pad}");
    }
}
