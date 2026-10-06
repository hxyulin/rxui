//! Standalone controlled-input example. Values are shared; selection, horizontal
//! scrolling and composition are independent in each window placement.
use rxui::prelude::*;

struct Form {
    name: String,
    normalized: String,
    digits: String,
    submitted: String,
    read_only: bool,
}
impl View for Form {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().fill_width().padding(24.).gap(12.).accessibility_role(SemanticRole::Form).accessibility_label("Shared form")
            .child(label("Controlled text input").font_size(28.).accessibility_role(SemanticRole::Heading))
            .child(label("Click/drag to select. Shift + arrows extends selection. Copy/paste and IME are supported."))
            .child(label("Shared value, independent caret/selection in each window"))
            .child(text_input(self.name.clone()).key("name").fill_width().accessibility_label("Name")
                .read_only(self.read_only)
                .on_change(cx.listener(|this, edit: &TextChangeEvent, _| this.name = edit.value.clone()))
                .on_submit(cx.listener(|this, event: &TextSubmitEvent, _| this.submitted = event.value.clone())))
            .child(label("Application normalization: uppercase"))
            .child(text_input(self.normalized.clone()).key("normalized").fill_width().accessibility_label("Uppercase value").accessibility_description("Text is converted to uppercase")
                .on_change(cx.listener(|this, edit: &TextChangeEvent, _| this.normalized = edit.value.to_uppercase())))
            .child(label("Application rejection: only ASCII digits, up to eight"))
            .child(text_input(self.digits.clone()).key("digits").fill_width().accessibility_label("Digits").accessibility_description("Only ASCII digits, up to eight")
                .on_change(cx.listener(|this, edit: &TextChangeEvent, _| {
                    if edit.value.len() <= 8 && edit.value.bytes().all(|b| b.is_ascii_digit()) {
                        this.digits = edit.value.clone();
                    }
                })))
            .child(row().gap(12.)
                .child(button("Open shared window").on_click(cx.listener(|_, _, cx| {
                    let root = cx.entity().upgrade().unwrap();
                    cx.open_window(WindowOptions::new().title("RXUI — shared form").size(980., 660.), root).unwrap();
                })))
                .child(button("Toggle read-only name").on_click(cx.listener(|this, _, _| this.read_only = !this.read_only)))
                .child(button("External reset").on_click(cx.listener(|this, _, _| {
                    this.name = "Reset from application state".into();
                    this.normalized.clear(); this.digits.clear();
                }))))
            .child(label(format!("Submitted: {}", self.submitted)))
            .child(label("Try combining accents, emoji, mixed RTL/LTR text, or a native input method."))
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let form = cx.new(|_| Form {
            name: "Hello, RXUI 👋 — שלום — مرحبا".into(),
            normalized: "EDIT ME".into(),
            digits: "123".into(),
            submitted: String::new(),
            read_only: false,
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — controlled editing and IME")
                .size(980., 660.),
            form,
        )?;
        Ok(())
    })
}
