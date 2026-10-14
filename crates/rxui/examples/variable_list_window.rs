//! A file tree with variable row heights: folders expand and collapse in place,
//! and changes above the viewport keep the visible rows still. Mouse-wheel steps
//! and reveals scroll smoothly until a button turns that off.
use rxui::prelude::*;
const FOLDERS: usize = 2_000;
const FOLDER_HEIGHT: f32 = 34.;
const FILE_HEIGHT: f32 = 24.;
const DIAGNOSTIC_HEIGHT: f32 = 46.;
#[derive(Clone, Copy)]
enum Item {
    Folder(usize),
    File(usize, usize),
}
fn file_count(folder: usize) -> usize {
    (folder * 7919) % 40 + 1
}
/// Every seventh file shows a diagnostic line under its name.
fn has_diagnostic(folder: usize, file: usize) -> bool {
    (folder + file).is_multiple_of(7)
}
fn entry(item: Item) -> (String, f32) {
    match item {
        Item::Folder(f) => (format!("src{f}"), FOLDER_HEIGHT),
        Item::File(f, i) => (
            format!("src{f}/file{i}.rs"),
            if has_diagnostic(f, i) {
                DIAGNOSTIC_HEIGHT
            } else {
                FILE_HEIGHT
            },
        ),
    }
}
struct Tree {
    scroll: ScrollHandle,
    rows: VirtualRows,
    items: Vec<Item>,
    expanded: Vec<bool>,
    jump: String,
    /// Wheel steps and reveals animate; trackpads always scroll directly.
    smooth: bool,
}
impl Tree {
    fn new() -> Self {
        // Every third folder starts expanded.
        let expanded: Vec<bool> = (0..FOLDERS).map(|f| f.is_multiple_of(3)).collect();
        let items: Vec<Item> = (0..FOLDERS)
            .flat_map(|f| {
                let files = if expanded[f] { file_count(f) } else { 0 };
                std::iter::once(Item::Folder(f)).chain((0..files).map(move |i| Item::File(f, i)))
            })
            .collect();
        Self {
            scroll: ScrollHandle::new(),
            rows: items.iter().map(|&item| entry(item)).collect(),
            items,
            expanded,
            jump: "src1500".into(),
            smooth: true,
        }
    }
    /// Inserts or removes a folder's files after its row.
    fn toggle(&mut self, folder: usize) {
        let Some(index) = self.rows.index_of(&format!("src{folder}").into()) else {
            return;
        };
        let start = index + 1;
        if self.expanded[folder] {
            let end = start + file_count(folder);
            self.items.drain(start..end);
            self.rows
                .splice(start..end, std::iter::empty::<(String, f32)>());
        } else {
            let files: Vec<_> = (0..file_count(folder))
                .map(|i| Item::File(folder, i))
                .collect();
            self.rows
                .splice(start..start, files.iter().map(|&item| entry(item)));
            self.items.splice(start..start, files);
        }
        self.expanded[folder] = !self.expanded[folder];
    }
}
impl View for Tree {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .fill_width()
            .fill_height()
            .gap(8.)
            .padding(16.)
            .child(label(format!("{} rows", self.rows.len())).font_size(22.))
            .child(
                row()
                    .gap(8.)
                    .child(
                        text_input(self.jump.clone())
                            .width(140.)
                            .accessibility_label("Row to reveal")
                            .on_change(
                                cx.listener(|s, e: &TextChangeEvent, _| s.jump = e.value.clone()),
                            ),
                    )
                    .child(button("Reveal").on_click(cx.listener(|s, _, cx| {
                        if let Some(index) = s.rows.index_of(&s.jump.as_str().into()) {
                            s.scroll
                                .reveal_list_row(cx, &s.rows, index)
                                .expect("live list");
                        }
                    })))
                    .child(button("Toggle src0").on_click(cx.listener(|s, _, _| s.toggle(0))))
                    .child(
                        button(if self.smooth {
                            "Smooth scrolling: on"
                        } else {
                            "Smooth scrolling: off"
                        })
                        .on_click(cx.listener(|s, _, _| s.smooth = !s.smooth)),
                    ),
            )
            .child(
                variable_list(&self.rows, &self.scroll, cx, |index| {
                    match self.items[index] {
                        Item::Folder(f) => button(format!(
                            "{} src{f}",
                            if self.expanded[f] { "▾" } else { "▸" }
                        ))
                        .variant(ButtonVariant::Quiet)
                        .padding(6.)
                        .on_click(cx.listener(move |s, _, _| s.toggle(f))),
                        Item::File(f, i) => {
                            let name = label(format!("file{i}.rs"));
                            let content = if has_diagnostic(f, i) {
                                column().child(name).child(
                                    label("warning: unused variable")
                                        .color(ThemeColor::TextMuted)
                                        .font_size(12.),
                                )
                            } else {
                                column().child(name)
                            };
                            content.padding_y(3.).padding_x(28.)
                        }
                    }
                })
                .scroll_behavior(if self.smooth {
                    ScrollBehavior::Smooth
                } else {
                    ScrollBehavior::Instant
                })
                .overscan(3),
            )
            .child(
                label("Scroll down, then toggle src0: the rows on screen stay where they are.")
                    .color(ThemeColor::TextMuted),
            )
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let root = cx.new(|_| Tree::new());
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — variable-height list")
                .size(720., 640.),
            root,
        )?;
        Ok(())
    })
}
