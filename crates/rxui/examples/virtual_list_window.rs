//! A hundred thousand fixed-height rows, with independent scrolling in shared windows.
use rxui::prelude::*;
const ROWS: usize = 100_000;
const HEIGHT: f32 = 32.;
struct Files {
    scroll: ScrollHandle,
    selected: usize,
    jump: String,
}
impl View for Files {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column()
            .fill_width()
            .fill_height()
            .gap(8.)
            .padding(16.)
            .child(label(format!("{ROWS} files — selected {}", self.selected)).font_size(22.))
            .child(
                row()
                    .gap(8.)
                    .child(
                        text_input(self.jump.clone())
                            .width(120.)
                            .accessibility_label("Row to reveal")
                            .on_change(
                                cx.listener(|s, e: &TextChangeEvent, _| s.jump = e.value.clone()),
                            ),
                    )
                    .child(button("Reveal row").on_click(cx.listener(|s, _, cx| {
                        if let Ok(index) = s.jump.parse::<usize>() {
                            s.scroll
                                .reveal_row(cx, index.min(ROWS - 1), HEIGHT)
                                .expect("live list");
                        }
                    })))
                    .child(button("Shared window").on_click(cx.listener(|_, _, cx| {
                        let root = cx.entity().upgrade().expect("live owner");
                        cx.open_window(
                            WindowOptions::new()
                                .title("RXUI — shared virtual list")
                                .size(720., 640.),
                            root,
                        )
                        .expect("valid window");
                    }))),
            )
            .child(
                virtual_list(ROWS, HEIGHT, &self.scroll, cx, |index| {
                    button(format!("File {index:05}.rs"))
                        .key(index)
                        .padding(6.)
                        .variant(if self.selected == index {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Quiet
                        })
                        .on_click(cx.listener(move |s, _, _| s.selected = index))
                })
                .overscan(3),
            )
            .child(
                label("Wheel or drag the scrollbar. Unmounted rows release local UI state.")
                    .color(ThemeColor::TextMuted),
            )
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let root = cx.new(|_| Files {
            scroll: ScrollHandle::new(),
            selected: 0,
            jump: "50000".into(),
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — virtual list")
                .size(720., 640.),
            root,
        )?;
        Ok(())
    })
}
