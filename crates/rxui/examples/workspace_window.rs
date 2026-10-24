//! Standalone scrolling and controlled splits. Shared model data has separate
//! scroll/capture/focus in each window. Resize listeners retain the requested sizes.
use rxui::{Element, prelude::*, taffy::prelude::AlignItems};

struct Workspace {
    sidebar: SplitPosition,
    console: SplitPosition,
    files: ScrollHandle,
    output: ScrollHandle,
    selected: usize,
    name: String,
    drag: [f32; 2],
    drag_origin: [f32; 2],
    dragging: bool,
}
impl Workspace {
    fn new() -> Self {
        Self {
            sidebar: SplitPosition::Pixels(240.),
            console: SplitPosition::Fraction(0.65),
            files: ScrollHandle::new(),
            output: ScrollHandle::new(),
            selected: 0,
            name: "Workspace".into(),
            drag: [24., 24.],
            drag_origin: [24., 24.],
            dragging: false,
        }
    }
    fn drag_canvas(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        stack()
            .fill_width()
            .fill_height()
            .clip()
            .background(ThemeColor::Background)
            .child(
                label("Drag the card beyond this pane; Escape restores its position.")
                    .absolute()
                    .left(16.)
                    .bottom(16.)
                    .right(16.)
                    .color(ThemeColor::TextMuted),
            )
            .child(
                column()
                    .key("card")
                    .absolute()
                    .left(self.drag[0])
                    .top(self.drag[1])
                    .size(200., 104.)
                    .padding(16.)
                    .gap(8.)
                    .radius(8.)
                    .background(ThemeColor::Control)
                    .border(1., ThemeColor::Border)
                    .focusable(true)
                    .cursor(if self.dragging {
                        Cursor::Grabbing
                    } else {
                        Cursor::Grab
                    })
                    .accessibility_role(SemanticRole::Group)
                    .accessibility_label("Draggable card")
                    .child(
                        label(format!("Selected file {:03}", self.selected))
                            .text_role(TextRole::Heading),
                    )
                    .child(label("Pointer capture demo").color(ThemeColor::TextMuted))
                    .on_pointer_down(cx.listener(|s, e: &PointerInput, _| {
                        if e.button == Some(PointerButton::Primary) {
                            s.drag_origin = s.drag;
                            s.dragging = true;
                            e.capture_pointer();
                            e.focus();
                            e.prevent_default();
                        }
                    }))
                    .on_pointer_move(cx.listener(|s, e: &PointerInput, _| {
                        if s.dragging
                            && let Some(delta) = e.drag_delta()
                        {
                            s.drag = [s.drag_origin[0] + delta[0], s.drag_origin[1] + delta[1]];
                        }
                    }))
                    .on_pointer_up(cx.listener(|s, e: &PointerInput, _| {
                        if e.button == Some(PointerButton::Primary) {
                            s.dragging = false;
                        }
                    }))
                    .on_pointer_cancel(cx.listener(|s, e: &PointerInput, _| {
                        if e.cancel_reason == Some(rxui::PointerCancelReason::Escape) {
                            s.drag = s.drag_origin;
                        }
                        s.dragging = false;
                    })),
            )
    }
}
impl View for Workspace {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        // Virtual list slots match the theme's list row height.
        let row_height = cx.theme().sizes().row_height;
        let files = column()
            .fill_width()
            .fill_height()
            .min_width(0.)
            .min_height(0.)
            .background(ThemeColor::Surface)
            .child(label("Files").text_role(TextRole::Heading).padding(12.))
            .child(
                // Rows sit 6 units in from the pane edges, as in desktop sidebars.
                column()
                    .fill_width()
                    .flex_grow(1.)
                    .flex_basis(0.)
                    .min_height(0.)
                    .padding_x(6.)
                    .child(virtual_list(160, row_height, &self.files, cx, |i| {
                        button(format!("File {i:03}"))
                            .key(i)
                            .list_row(self.selected == i)
                            .on_click(cx.listener(move |s, _, _| s.selected = i))
                    })),
            )
            .child(
                row()
                    .padding(8.)
                    .gap(8.)
                    .child(button("Top").on_click(cx.listener(|s, _, cx| {
                        s.files
                            .scroll_to(cx, [0., 0.])
                            .expect("live files viewport");
                    })))
                    .child(button("Bottom").on_click(cx.listener(|s, _, cx| {
                        if let Some(state) = s.files.state(cx) {
                            s.files
                                .scroll_to(cx, state.range)
                                .expect("live files viewport");
                        }
                    }))),
            );
        let output = column()
            .fill_width()
            .fill_height()
            .min_width(0.)
            .min_height(0.)
            .background(ThemeColor::Surface)
            .child(
                row()
                    .padding(8.)
                    .gap(12.)
                    .child(label("Output").text_role(TextRole::Heading))
                    .child(button("Clear scroll").on_click(cx.listener(|s, _, cx| {
                        s.output
                            .scroll_to(cx, [0.; 2])
                            .expect("live output viewport");
                    }))),
            )
            .child(virtual_list(120, row_height, &self.output, cx, |i| {
                label(format!(
                    "{i:03}  Virtual output line — only nearby rows are mounted."
                ))
                .key(i)
                .padding_x(8.)
                .padding_y(3.)
            }));
        column()
            .fill_width()
            .fill_height()
            .min_height(0.)
            .child(
                row()
                    .fill_width()
                    .padding(12.)
                    .gap(12.)
                    .align_items(AlignItems::CENTER)
                    .child(
                        text_input(self.name.clone())
                            .width(200.)
                            .accessibility_label("Workspace name")
                            .on_change(
                                cx.listener(|s, e: &TextChangeEvent, _| s.name = e.value.clone()),
                            ),
                    )
                    .child(button("Shared window").on_click(cx.listener(|_, _, cx| {
                        let root = cx.entity().upgrade().expect("live view owner");
                        cx.open_window(
                            WindowOptions::new()
                                .title("RXUI — shared workspace")
                                .size(960., 680.),
                            root,
                        )
                        .expect("valid window options");
                    })))
                    .child(
                        label("Tab to dividers/scrollbars; arrows and Home/End adjust them.")
                            .color(ThemeColor::TextMuted),
                    ),
            )
            .child(
                split_row(
                    files,
                    split_column(self.drag_canvas(cx), output)
                        .position(self.console)
                        .min_first(160.)
                        .min_second(120.)
                        .on_resize(cx.listener(|s, e: &ResizeEvent, _| s.console = e.position)),
                )
                .position(self.sidebar)
                .min_first(160.)
                .min_second(400.)
                .on_resize(cx.listener(|s, e: &ResizeEvent, _| s.sidebar = e.position)),
            )
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let root = cx.new(|_| Workspace::new());
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — workspace controls")
                .size(1040., 720.),
            root,
        )?;
        Ok(())
    })
}
