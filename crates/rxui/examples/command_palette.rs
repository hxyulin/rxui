//! A command palette: Primary+Shift+P opens a dialog with an inline combobox over
//! the application's commands. Enter or a click runs one; Escape closes it.
use rxui::prelude::*;

struct ShowCommands;
impl Command for ShowCommands {}

/// A palette entry. An application builds these from its command registry.
struct Entry {
    name: &'static str,
    shortcut: Option<Shortcut>,
}
fn entry(name: &'static str, shortcut: Option<Shortcut>) -> Entry {
    Entry { name, shortcut }
}

struct App {
    palette: bool,
    query: String,
    commands: Vec<Entry>,
    ran: Option<&'static str>,
}

impl View for App {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let shortcut = Shortcut::primary("p").shift();
        let show = cx
            .command(ShowCommands, |s, _, _| {
                s.palette = true;
                s.query.clear();
            })
            .label("Show all commands")
            .shortcut(shortcut.clone());
        let mut root = column()
            .fill_width()
            .fill_height()
            .align_items(rxui::taffy::AlignItems::CENTER)
            .justify_content(rxui::taffy::JustifyContent::CENTER)
            .gap(8.)
            .on_command(show)
            .child(
                row()
                    .gap(8.)
                    .child(label("Show all commands").color(ThemeColor::TextMuted))
                    .child(kbd(shortcut)),
            );
        if let Some(name) = self.ran {
            root = root.child(label(name));
        }
        if !self.palette {
            return root;
        }
        let palette = combobox(cx)
            .inline()
            .fill_width()
            .accessibility_label("Command palette")
            .placeholder("Type a command")
            .query(&self.query)
            .on_query(cx.listener(|s, e: &QueryEvent, _| s.query = e.text.clone()))
            .items(self.commands.len(), |i| {
                let entry = &self.commands[i];
                let item = ComboItem::new(entry.name);
                match &entry.shortcut {
                    Some(shortcut) => item.trailing(kbd(shortcut.clone())),
                    None => item,
                }
            })
            .on_choose(cx.listener(|s, e: &ChooseEvent, _| {
                s.ran = Some(s.commands[e.index].name);
                s.palette = false;
            }));
        root.child(
            modal(palette)
                .key("palette")
                .width(560.)
                .padding(8.)
                .accessibility_label("Command palette")
                .on_dismiss(cx.listener(|s, _: &DismissEvent, _| s.palette = false)),
        )
    }
}

fn main() -> Result<(), ApplicationError> {
    let primary = |key: &str| Some(Shortcut::primary(key));
    let commands = vec![
        entry("File: New File", primary("n")),
        entry("File: Open File…", primary("o")),
        entry("File: Save", primary("s")),
        entry("File: Save As…", Some(Shortcut::primary("s").shift())),
        entry("File: Close Editor", primary("w")),
        entry("Edit: Find", primary("f")),
        entry("Edit: Replace", Some(Shortcut::primary("f").alt())),
        entry("Edit: Toggle Line Comment", primary("/")),
        entry("Go to File…", primary("p")),
        entry("Go to Line…", Some(Shortcut::primary("g").shift())),
        entry("Go to Symbol in Workspace…", primary("t")),
        entry("View: Toggle Sidebar", primary("b")),
        entry("View: Toggle Terminal", primary("j")),
        entry("View: Zoom In", primary("=")),
        entry("View: Zoom Out", primary("-")),
        entry("Format Document", Some(Shortcut::primary("i").shift())),
        entry("Rename Symbol", None),
        entry("Restart Language Server", None),
        entry("Run Tests", None),
        entry("Preferences: Color Theme", None),
    ];
    Application::new().theme(Theme::dark()).run(|cx| {
        let root = cx.new(|_| App {
            palette: false,
            query: String::new(),
            commands,
            ran: None,
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — command palette")
                .size(800., 560.),
            root,
        )?;
        Ok(())
    })
}
