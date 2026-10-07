//! Standalone scrolling and controlled splits. Shared model data has separate
//! scroll/capture/focus in each window. Resize listeners retain the requested sizes.
use rxui::{Element, prelude::*};

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
                    .child(label(format!("Selected file {:03}", self.selected)).font_size(20.))
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
        let files = column()
            .fill_width()
            .fill_height()
            .min_width(0.)
            .min_height(0.)
            .background(ThemeColor::Surface)
            .child(label("Files").font_size(20.).padding(12.))
            .child(
                scroll_area(column().fill_width().gap(2.).children((0..160).map(|i| {
                    button(format!("File {i:03}"))
                        .key(i)
                        .fill_width()
                        .variant(if self.selected == i {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Quiet
                        })
                        .on_click(cx.listener(move |s, _, _| s.selected = i))
                })))
                .handle(self.files.clone()),
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
                    .child(label("Output").font_size(20.))
                    .child(button("Clear scroll").on_click(cx.listener(|s, _, cx| {
                        s.output
                            .scroll_to(cx, [0.; 2])
                            .expect("live output viewport");
                    }))),
            )
            .child(
                scroll_area(
                    column()
                        .fill_width()
                        .padding(12.)
                        .gap(4.)
                        .children((0..120).map(|i| {
                            label(format!(
                                "{i:03}  Retained output line — scrolling keeps text resources."
                            ))
                            .key(i)
                        })),
                )
                .handle(self.output.clone()),
            );
        column()
            .fill_width()
            .fill_height()
            .min_height(0.)
            .child(
                row()
                    .fill_width()
                    .padding(12.)
                    .gap(12.)
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
fn main() {
    use rxui::{
        AccessKitTree, Ui, UiPainter,
        astrelis::{FramebufferOptions, GraphicsContext, wgpu},
    };
    use std::time::{Duration, Instant};
    let graphics = pollster::block_on(GraphicsContext::headless()).unwrap();
    let mut target = graphics
        .create_framebuffer(FramebufferOptions::new(2080, 1440))
        .unwrap();
    let mut runtime = Runtime::new();
    let root = runtime.update(|cx| cx.new(|_| Workspace::new()));
    let mut ui = Ui::new(&mut runtime, root).unwrap();
    let mut painter = UiPainter::new(&graphics);
    painter
        .fonts_mut()
        .load_font_shared(std::sync::Arc::<[u8]>::from(
            include_bytes!("SOURCE_FONT_PATH").as_slice(),
        ))
        .unwrap();
    ui.prepare(&mut runtime, [1040., 720.], &mut painter)
        .unwrap();
    painter.prepare(&ui, &target.render_format(), 2.).unwrap();
    let mut accesskit = AccessKitTree::new();
    accesskit.update(&ui, "Workspace", 2.).unwrap();
    let stats = ui.stats();
    let text_stats = painter.painter().text().stats();
    println!(
        "frame,scroll_us,prepare_us,record_us,submit_us,wait_us,text_draws,accessibility_us,accessibility_nodes"
    );
    for i in 0..180 {
        let start = Instant::now();
        ui.scroll([100., 180.], [0., if i % 2 == 0 { 10. } else { -10. }])
            .unwrap();
        let scrolled = start.elapsed();
        ui.prepare(&mut runtime, [1040., 720.], &mut painter)
            .unwrap();
        painter.prepare(&ui, &target.render_format(), 2.).unwrap();
        let prepared = start.elapsed();
        let draws = painter.painter().text().stats().draw_calls;
        let mut frame = target.begin_frame().unwrap();
        painter
            .compose(&ui, &mut frame, 2., |frame, composed| {
                let mut pass = frame
                    .render_pass()
                    .clear_color(wgpu::Color::BLACK)
                    .begin()?;
                composed.paint(&mut pass)
            })
            .unwrap();
        let recorded = start.elapsed();
        let submission = frame.finish().unwrap();
        let submitted = start.elapsed();
        graphics
            .device()
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(10)),
            })
            .unwrap();
        let waited = start.elapsed();
        let ax_nodes = accesskit
            .update(&ui, "Workspace", 2.)
            .unwrap()
            .map_or(0, |u| u.nodes.len());
        let accessibility = start.elapsed() - waited;
        println!(
            "{i},{:.3},{:.3},{:.3},{:.3},{:.3},{},{:.3},{}",
            scrolled.as_secs_f64() * 1e6,
            (prepared - scrolled).as_secs_f64() * 1e6,
            (recorded - prepared).as_secs_f64() * 1e6,
            (submitted - recorded).as_secs_f64() * 1e6,
            (waited - submitted).as_secs_f64() * 1e6,
            painter.painter().text().stats().draw_calls - draws,
            accessibility.as_secs_f64() * 1e6,
            ax_nodes
        );
    }
    assert_eq!(ui.stats(), stats);
    assert_eq!(
        painter.painter().text().stats().geometry_bytes,
        text_stats.geometry_bytes
    );
    assert_eq!(
        painter.painter().text().stats().uploaded_bytes,
        text_stats.uploaded_bytes
    );
}
