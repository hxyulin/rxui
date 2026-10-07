//! Standalone theme/styling gallery. Themes belong to placements; the two previews
//! and separately opened windows can share a model while keeping their own theme.
use rxui::Element;
use rxui::prelude::*;

struct Gallery {
    name: String,
    clicks: u32,
    read_only: bool,
}
fn preview(theme: Theme, title: &str, value: &str, cx: &mut ViewContext<'_, Gallery>) -> Element {
    column()
        .theme(theme)
        .width(360.)
        .padding(20.)
        .gap(12.)
        .background(ThemeColor::Surface)
        .border(1., ThemeColor::Border)
        .radius(8.)
        .accessibility_role(SemanticRole::Group)
        .accessibility_label(title)
        .child(
            label(title)
                .font_size(22.)
                .accessibility_role(SemanticRole::Heading),
        )
        .child(label("Secondary text inherits the local palette").color(ThemeColor::TextMuted))
        .child(
            row()
                .gap(8.)
                .child(
                    button("Primary")
                        .variant(ButtonVariant::Primary)
                        .on_click(cx.listener(|this, _, _| this.clicks += 1)),
                )
                .child(button("Default").on_click(cx.listener(|this, _, _| this.clicks += 1)))
                .child(button("Quiet").variant(ButtonVariant::Quiet)),
        )
        .child(button("Disabled button").fill_width().disabled(true))
        .child(
            text_input(value)
                .fill_width()
                .accessibility_label(format!("{title} name"))
                .on_change(
                    cx.listener(|this, edit: &TextChangeEvent, _| this.name = edit.value.clone()),
                ),
        )
        .child(
            text_input("Selectable read-only value")
                .fill_width()
                .read_only(true)
                .accessibility_label(format!("{title} read-only value")),
        )
}
/// Cycles dark → light → high contrast.
fn next_theme(current: &Theme) -> Theme {
    if *current == Theme::dark() {
        Theme::light()
    } else if *current == Theme::light() {
        Theme::high_contrast()
    } else {
        Theme::dark()
    }
}
impl View for Gallery {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().fill_width().fill_height().padding(24.).gap(16.).scroll_y()
            .child(label("RXUI theme gallery").font_size(28.).accessibility_role(SemanticRole::Heading))
            .child(label("Hover, press, use Tab, and select text. Focus stays visible over pointer states.")
                .color(ThemeColor::TextMuted))
            .child(row().gap(12.)
                .child(button("Toggle application theme").on_click(cx.listener(|_, _, cx| {
                    let next = next_theme(&cx.theme().unwrap());
                    cx.set_theme(next).unwrap();
                })))
                .child(button("Toggle this window").on_click(cx.listener(|_, _, cx| {
                    let window = cx.window().unwrap();
                    cx.set_window_theme(&window, next_theme(&window.theme())).unwrap();
                })))
                .child(button("Follow application theme").on_click(cx.listener(|_, _, cx| {
                    let window = cx.window().unwrap();
                    cx.use_application_theme(&window).unwrap();
                }))))
            .child(row().gap(16.)
                .child(preview(Theme::dark(), "Dark", &self.name, cx))
                .child(preview(Theme::light(), "Light", &self.name, cx))
                .child(preview(Theme::high_contrast(), "High contrast", &self.name, cx)))
            .child(row().gap(16.)
                .child(preview(Theme::dark().compact(), "Dark compact", &self.name, cx))
                .child(preview(Theme::light().compact(), "Light compact", &self.name, cx))
                .child(preview(Theme::high_contrast().compact(), "High contrast compact", &self.name, cx)))
            .child(label("Local overrides").font_size(22.))
            .child(row().gap(12.)
                .child(button("Custom interaction paints")
                    .background(ThemeColor::Control)
                    .hover_style(PaintStyle::new().background(ThemeColor::ControlHover)
                        .border_color(ThemeColor::Focus))
                    .pressed_style(PaintStyle::new().background(ThemeColor::ControlPressed))
                    .radius(8.)
                    .on_click(cx.listener(|this, _, _| this.clicks += 1)))
                .child(button("Literal color remains fixed").background(rgb8(28, 28, 28))
                    .color(rgb8(245, 245, 245)).border(1., rgb8(133, 133, 133))
                    .paint_style(PaintStyle::new().focus_color(rgb8(245, 245, 245)))
                    .on_click(cx.listener(|this, _, _| this.clicks += 1))))
            .child(text_input(self.name.clone()).key("name").fill_width().read_only(self.read_only)
                .accessibility_label("Shared name")
                .on_change(cx.listener(|this, edit: &TextChangeEvent, _| this.name = edit.value.clone())))
            .child(row().gap(12.)
                .child(button("Toggle read-only").on_click(cx.listener(|this, _, _| this.read_only = !this.read_only)))
                .child(button("Open shared light window").on_click(cx.listener(|_, _, cx| {
                    let root = cx.entity().upgrade().unwrap();
                    cx.open_window(WindowOptions::new().title("RXUI — light gallery")
                        .size(1200., 900.).theme(Theme::light()), root).unwrap();
                }))))
            .child(label(format!("Clicks: {} · shared values, independent theme/focus/selection", self.clicks)))
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().theme(Theme::dark()).run(|cx| {
        let root = cx.new(|_| Gallery {
            name: "Select text — שלום — 👋".into(),
            clicks: 0,
            read_only: false,
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — theme gallery")
                .size(1200., 900.),
            root,
        )?;
        Ok(())
    })
}
