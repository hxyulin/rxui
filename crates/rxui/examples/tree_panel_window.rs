//! Narrow fixed-height tree rows with long labels. Resize and scroll to either end.
use rxui::prelude::*;
const HEIGHT: f32 = 24.;
struct Tree {
    scroll: ScrollHandle,
    selected: usize,
}
impl View for Tree {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let width = self.scroll.state(cx).map_or(260., |s| s.viewport[0]);
        column()
            .fill_width()
            .fill_height()
            .padding(8.)
            .gap(8.)
            .child(label("Undo history — resize this window"))
            .child(
                virtual_list(160, HEIGHT, &self.scroll, cx, |index| {
                    button(format!(
                        "▾ {index}: restore a deeply nested undo state with a long description"
                    ))
                    .key(index)
                    .font_size(12.5)
                    .height(HEIGHT)
                    .padding_y(0.)
                    .padding_left((12. + (index % 32) as f32 * 14.).min(width * 0.35))
                    .padding_right(12.)
                    .text_overflow(rxui::TextOverflow::Ellipsis)
                    .radius(0.)
                    .variant(if self.selected == index {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Quiet
                    })
                    .on_click(cx.listener(move |s, _, _| s.selected = index))
                })
                .fill_width()
                .fill_height(),
            )
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let root = cx.new(|_| Tree {
            scroll: ScrollHandle::new(),
            selected: 0,
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — narrow tree regression")
                .size(280., 420.),
            root,
        )?;
        Ok(())
    })
}
