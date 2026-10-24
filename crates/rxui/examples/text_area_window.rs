//! Standalone multiline editing example: a long controlled document in a scrolling
//! text area, a chat-style composer that submits on Enter, selectable read-only and
//! rich text, and runtime font registration.
//! Run: cargo run -p rxui --example text_area_window --features native
use rxui::prelude::*;

struct Notes {
    document: String,
    draft: String,
    messages: Vec<String>,
    monospace: bool,
    status: String,
}
impl View for Notes {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let lines = self.document.split('\n').count();
        let words = self.document.split_whitespace().count();
        let family = if self.monospace {
            FontFamily::Monospace
        } else {
            FontFamily::SansSerif
        };
        column().fill_width().fill_height().padding(24.).gap(12.)
            .child(label("Text areas").text_role(TextRole::Title).accessibility_role(SemanticRole::Heading))
            .child(label("Enter inserts a line break; Up/Down keep the column, Home/End stay on the visual line, \
                Page Up/Down move by the visible height. The caret stays visible in the scrolling column.")
                .selectable(true))
            .child(row().gap(12.)
                .child(rich_text([span(lines.to_string()).bold(), span(" lines, "), span(words.to_string()).bold(), span(" words")])
                    .color(ThemeColor::TextMuted).selectable(true))
                .child(button(if self.monospace { "Proportional" } else { "Monospace" })
                    .on_click(cx.listener(|this, _, _| this.monospace = !this.monospace)))
                .child(button("Load a long document").on_click(cx.listener(|this, _, _| {
                    this.document = (1..=5000)
                        .map(|i| format!("Line {i}: edits re-lay out only nearby paragraphs."))
                        .collect::<Vec<_>>()
                        .join("\n");
                })))
                .child(button("Register a font file…").on_click(cx.listener(|this, _, cx| {
                    // Any TTF/OTF on disk; its family names become usable at once.
                    let path = std::env::var("RXUI_FONT").unwrap_or_default();
                    this.status = match std::fs::read(&path) {
                        Ok(bytes) => match cx.register_font(bytes) {
                            Ok(faces) => format!("Registered {:?}", faces.first().map(|f| &f.families)),
                            Err(error) => format!("Not a usable font: {error}"),
                        },
                        Err(_) => "Set RXUI_FONT to the path of a font file.".into(),
                    };
                }))))
            .child(column().fill_width().flex_grow(1.).flex_basis(0.).min_height(120.).scroll_y()
                .child(text_area(self.document.clone()).key("document").fill_width()
                    .font_family(family)
                    .placeholder("Write or paste a document")
                    .accessibility_label("Document")
                    .on_change(cx.listener(|this, edit: &TextChangeEvent, _| this.document = edit.value.clone()))))
            .child(label("Composer: Enter sends, Shift+Enter adds a line").color(ThemeColor::TextMuted))
            .child(column().gap(4.).children(self.messages.iter().enumerate()
                .map(|(i, m)| label(m.clone()).key(i).selectable(true))))
            .child(text_area(self.draft.clone()).key("draft").fill_width().min_height(0.)
                .submit_shortcut(SubmitShortcut::Enter)
                .placeholder("Message")
                .accessibility_label("Message")
                .on_change(cx.listener(|this, edit: &TextChangeEvent, _| this.draft = edit.value.clone()))
                .on_submit(cx.listener(|this, event: &TextSubmitEvent, _| {
                    if !event.value.trim().is_empty() {
                        this.messages.push(event.value.trim().to_owned());
                        this.draft.clear();
                    }
                })))
            .child(label(self.status.clone()).color(ThemeColor::TextMuted))
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let notes = cx.new(|_| Notes {
            document:
                "Shopping\n\t- bread\n\t- coffee\n\nMixed scripts wrap and select by grapheme: \
                e\u{301}, 👩‍👩‍👧‍👦, שלום, مرحبا."
                    .into(),
            draft: String::new(),
            messages: Vec::new(),
            monospace: false,
            status: String::new(),
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — text areas")
                .size(900., 760.),
            notes,
        )?;
        Ok(())
    })
}
