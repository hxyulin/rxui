//! Resize either window to reflow measured rows independently. Load belongs to
//! the window's component mount; closing it suppresses even a queued completion.
use rxui::prelude::*;
use std::time::Duration;

struct Results {
    rows: VirtualRows,
    scroll: ScrollHandle,
    completed: usize,
}
impl View for Results {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let scope = cx.task_scope();
        column().fill_width().fill_height().padding(16.).gap(8.)
            .child(row().child(tooltip(cx, button("Load (takes three seconds)").on_click(cx.listener(move |_, _, cx| {
                let owner = cx.entity();
                scope.spawn(cx, async {
                    rxui::sleep(Duration::from_secs(3)).await;
                }, move |result, cx| {
                    if result.is_ok() {
                        let _ = owner.update(cx, |s, _| s.completed += 1);
                    }
                }).unwrap().detach();
            })), "Closing this window cancels its load completion")))
            .child(label(format!("Completed loads: {}", self.completed)).color(ThemeColor::TextMuted))
            .child(variable_list(&self.rows, &self.scroll, cx, |index| {
                column().padding_y(8.).gap(4.)
                    .child(label(format!("Result {index}")).text_role(TextRole::Subheading))
                    .child(label("A wrapped description measured at the width of this window. Resize the window to see row heights and the scroll extent adjust. ".repeat(index % 4 + 1)))
            }).measured().fill_width().fill_height())
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let results = cx.new(|_| Results {
            rows: (0..10_000).map(|index| (index, 100.)).collect(),
            scroll: ScrollHandle::new(),
            completed: 0,
        });
        cx.open_window(
            WindowOptions::new().title("Measured rows — first window"),
            results.clone(),
        )?;
        cx.open_window(
            WindowOptions::new().title("Measured rows — second window"),
            results,
        )?;
        Ok(())
    })
}
