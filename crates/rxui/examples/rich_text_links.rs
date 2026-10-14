//! Standalone rich text links example, shaped like an IDE: a hover documentation
//! card whose links navigate between symbols, and diagnostics whose locations and
//! error codes are links. Hover a link for the pointer cursor, click it, or press
//! Tab to move between links and Enter to follow one.
//! Run: cargo run -p rxui --example rich_text_links --features native
use rxui::prelude::*;

/// A documented symbol and the symbols its documentation links to.
struct Symbol {
    name: &'static str,
    signature: &'static str,
    summary: &'static str,
    see_also: &'static [usize],
}
const SYMBOLS: &[Symbol] = &[
    Symbol {
        name: "Vec",
        signature: "pub struct Vec<T>",
        summary: "A contiguous growable array type, written as Vec<T>.",
        see_also: &[1, 2],
    },
    Symbol {
        name: "push",
        signature: "pub fn push(&mut self, value: T)",
        summary: "Appends an element to the back of a collection.",
        see_also: &[0, 2],
    },
    Symbol {
        name: "with_capacity",
        signature: "pub fn with_capacity(capacity: usize) -> Vec<T>",
        summary: "Constructs a new, empty Vec<T> with at least the given capacity.",
        see_also: &[0],
    },
];
/// A compiler diagnostic with a source location and an error code.
struct Diagnostic {
    code: &'static str,
    message: &'static str,
    file: &'static str,
    line: u32,
}
const DIAGNOSTICS: &[Diagnostic] = &[
    Diagnostic {
        code: "E0308",
        message: "mismatched types: expected Vec<u8>, found &str",
        file: "src/main.rs",
        line: 12,
    },
    Diagnostic {
        code: "E0599",
        message: "no method named push_front found for Vec<u8>",
        file: "src/buffer.rs",
        line: 48,
    },
];

struct Workspace {
    symbol: usize,
    history: Vec<usize>,
    status: String,
}
impl View for Workspace {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let symbol = &SYMBOLS[self.symbol];
        // One listener serves every generated link: LinkEvent::index names it.
        let see_also = symbol.see_also;
        let navigate = cx.listener(move |this, event: &LinkEvent, _| {
            // Link 0 is "Back"; the rest follow the see-also list.
            if event.index == 0 {
                if let Some(previous) = this.history.pop() {
                    this.symbol = previous;
                }
            } else {
                this.history.push(this.symbol);
                this.symbol = see_also[event.index - 1];
            }
            this.status = format!("Showing {}", SYMBOLS[this.symbol].name);
        });
        let mut docs = vec![
            link("Back", navigate.clone()).link_description("Previous symbol"),
            span(format!("  {}\n", symbol.signature)).monospace().bold(),
            span(format!("{}\n", symbol.summary)),
            span("See also: "),
        ];
        for (i, &target) in symbol.see_also.iter().enumerate() {
            if i > 0 {
                docs.push(span(", "));
            }
            docs.push(
                link(SYMBOLS[target].name, navigate.clone())
                    .monospace()
                    .link_description(format!(
                        "Show the documentation of {}",
                        SYMBOLS[target].name
                    )),
            );
        }
        let diagnostics = DIAGNOSTICS.iter().enumerate().map(|(i, d)| {
            let open = cx.listener(move |this, event: &LinkEvent, _| {
                let d = &DIAGNOSTICS[i];
                this.status = if event.index == 0 {
                    format!("Opened {}:{}", d.file, d.line)
                } else {
                    format!("Explained {}", d.code)
                };
            });
            rich_text([
                span("error").bold().color(rgb8(0xe5, 0x48, 0x4d)),
                span(": "),
                span(d.message),
                span("\n  --> "),
                link(format!("{}:{}", d.file, d.line), open.clone()).monospace(),
                span("  "),
                link(d.code, open)
                    .link_url(format!(
                        "https://doc.rust-lang.org/error_codes/{}.html",
                        d.code
                    ))
                    .underlined(false),
            ])
            .key(i)
            .fill_width()
            .selectable(true)
        });
        column()
            .fill_width()
            .fill_height()
            .padding(24.)
            .gap(16.)
            .child(
                label("Hover documentation")
                    .font_size(22.)
                    .accessibility_role(SemanticRole::Heading),
            )
            .child(
                column()
                    .fill_width()
                    .padding(14.)
                    .radius(8.)
                    .background(ThemeColor::Raised)
                    .border(1., ThemeColor::Border)
                    .child(rich_text(docs).key("docs").fill_width()),
            )
            .child(
                label("Diagnostics")
                    .font_size(22.)
                    .accessibility_role(SemanticRole::Heading),
            )
            .child(column().fill_width().gap(10.).children(diagnostics))
            .child(label(self.status.clone()).color(ThemeColor::TextMuted))
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let workspace = cx.new(|_| Workspace {
            symbol: 0,
            history: Vec::new(),
            status: "Tab moves between links; Enter follows one.".into(),
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — rich text links")
                .size(720., 520.),
            workspace,
        )?;
        Ok(())
    })
}
