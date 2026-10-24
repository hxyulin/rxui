//! Standalone text input client example: a minimal application-owned editor that takes
//! typed text (any keyboard layout and dead keys), IME composition and paste, handles
//! its own editing keys, and has a Vim-like normal mode where letters are motions.
//! Insert mode captures Tab for indentation; press Escape for normal mode, where Tab
//! moves focus again and `i` returns to insert mode.
//! Run: cargo run -p rxui --example custom_text_client_window --features native
use rxui::{Bounds, prelude::*};

/// Monospace layout estimate for the IME candidate window. A real editor reports the
/// caret rectangle from its own shaped layout.
const FONT_SIZE: f32 = 16.;
const ADVANCE: f32 = FONT_SIZE * 0.6;
const LINE_HEIGHT: f32 = FONT_SIZE * 1.25;
const PADDING: f32 = 12.;

struct Scratch {
    text: String,
    /// Caret as a UTF-8 byte offset into `text`.
    caret: usize,
    /// Composition shown at the caret, not yet part of `text`.
    preedit: Option<String>,
    normal_mode: bool,
    notes: String,
}
impl Scratch {
    fn insert(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.text.insert_str(self.caret, &text);
        self.caret += text.len();
    }
    fn left(&self) -> usize {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }
    fn right(&self) -> usize {
        self.text[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |c| self.caret + c.len_utf8())
    }
    fn key(&mut self, event: &KeyInput, _: &mut Context<'_, Self>) {
        let key = &event.event;
        if !key.pressed || key.modifiers.primary() {
            return;
        }
        let handled = match &key.key {
            KeyboardKey::Escape if !self.normal_mode => {
                self.normal_mode = true;
                true
            }
            KeyboardKey::ArrowLeft => {
                self.caret = self.left();
                true
            }
            KeyboardKey::ArrowRight => {
                self.caret = self.right();
                true
            }
            KeyboardKey::Backspace if !self.normal_mode => {
                let start = self.left();
                self.text.replace_range(start..self.caret, "");
                self.caret = start;
                true
            }
            KeyboardKey::Enter if !self.normal_mode => {
                self.insert("\n");
                true
            }
            // Insert mode keeps Tab for indentation instead of focus traversal.
            KeyboardKey::Tab if !self.normal_mode => {
                self.insert("    ");
                true
            }
            // Normal mode: letters are commands, and preventing the default keeps
            // them out of the document.
            KeyboardKey::Character(c) if self.normal_mode => {
                match c.as_str() {
                    "h" => self.caret = self.left(),
                    "l" => self.caret = self.right(),
                    "i" => self.normal_mode = false,
                    _ => {}
                }
                true
            }
            KeyboardKey::Space if self.normal_mode => true,
            _ => false,
        };
        if handled {
            event.prevent_default();
        }
    }
    fn text_insert(&mut self, event: &TextInsertInput, _: &mut Context<'_, Self>) {
        if event.source == TextInsertSource::Ime {
            self.preedit = None;
        }
        self.insert(&event.text);
    }
    fn ime(&mut self, event: &ImeInput, _: &mut Context<'_, Self>) {
        match &event.event {
            ImeEvent::Preedit { text, .. } => self.preedit = Some(text.clone()),
            ImeEvent::Cancel(reason) => {
                self.preedit = None;
                self.notes = format!("Composition cancelled: {reason:?}");
            }
        }
    }
    /// Caret rectangle relative to the editor's border box.
    fn caret_area(&self) -> Bounds {
        let before = &self.text[..self.caret];
        let line = before.matches('\n').count();
        let column = before.rsplit('\n').next().unwrap_or("").chars().count()
            + self.preedit.as_ref().map_or(0, |p| p.chars().count());
        Bounds {
            x: PADDING + column as f32 * ADVANCE,
            y: PADDING + line as f32 * LINE_HEIGHT,
            width: 1.,
            height: LINE_HEIGHT,
        }
    }
}
impl View for Scratch {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let (before, after) = self.text.split_at(self.caret);
        let mode = if self.normal_mode {
            "NORMAL — h/l move, i inserts, Tab moves focus"
        } else {
            "INSERT — Escape for normal mode, Tab indents"
        };
        column()
            .fill_width()
            .padding(24.)
            .gap(12.)
            .child(label("Custom text input client").text_role(TextRole::Title))
            .child(label(mode).color(ThemeColor::TextMuted))
            .child(
                column()
                    .key("editor")
                    .fill_width()
                    .min_height(200.)
                    .padding(PADDING)
                    .background(ThemeColor::Input)
                    .border(1., ThemeColor::InputBorder)
                    .radius(6.)
                    .cursor(Cursor::Text)
                    .accessibility_label("Scratch editor")
                    .text_input_client(true)
                    .ime_cursor_area(self.caret_area())
                    .on_pointer_down(cx.listener(|_, event: &PointerInput, _| event.focus()))
                    .on_key_down(cx.listener(Self::key))
                    .on_text_insert(cx.listener(Self::text_insert))
                    .on_ime(cx.listener(Self::ime))
                    .child(
                        rich_text([
                            span(before).monospace(),
                            span(self.preedit.clone().unwrap_or_default())
                                .monospace()
                                .underline(),
                            span("|").monospace().color(ThemeColor::TextMuted),
                            span(after).monospace(),
                        ])
                        .font_size(FONT_SIZE),
                    ),
            )
            .child(label(self.notes.clone()).color(ThemeColor::TextMuted))
            .child(text_input(String::new()).accessibility_label("Next field"))
    }
}

fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let scratch = cx.new(|_| Scratch {
            text: String::new(),
            caret: 0,
            preedit: None,
            normal_mode: false,
            notes: String::new(),
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — custom text input client")
                .size(720., 480.),
            scratch,
        )?;
        Ok(())
    })
}
