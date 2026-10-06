//! Standalone flex and stacked-overlay example. Layout and sibling paint order are
//! explicit; an inert background keeps overlay input and semantic navigation coherent.
use rxui::{
    Element,
    prelude::*,
    taffy::prelude::{AlignItems, AlignSelf},
};
struct Workspace {
    overlay: bool,
    reverse: bool,
    clicks: u32,
}
fn tile(name: &str, color: [f32; 4]) -> Element {
    column()
        .size(180., 120.)
        .background(color)
        .border(1., ThemeColor::Border)
        .padding(16.)
        .child(label(name).color(rgb8(250, 250, 250)))
}
impl View for Workspace {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let background = row().fill_width().fill_height().gap(16.).padding(24.).inert(self.overlay)
            .child(column().width(180.).fill_height().padding(12.).gap(12.).background(ThemeColor::Surface)
                .child(label("Sidebar").font_size(22.))
                .child(button("Open overlay").variant(ButtonVariant::Primary).fill_width()
                    .on_click(cx.listener(|s, _, _| s.overlay = true)))
                .child(button("Count click").fill_width().on_click(cx.listener(|s, _, _| s.clicks += 1)))
                .child(label(format!("Clicks: {}", self.clicks))))
            .child(column().flex_grow(1.).flex_basis(0.).min_width(0.).fill_height().gap(16.).scroll_y()
                .child(label("Layout & paint ordering").font_size(26.).accessibility_role(SemanticRole::Heading))
                .child(label("The sidebar keeps its width; this pane takes the remaining space. Overlapping cards use sibling z order."))
                .child(stack().height(240.).fill_width().background(ThemeColor::Surface)
                    .child(tile("First in description", rgb8(50, 65, 90)).key("first").absolute().left(16.).top(16.)
                        .z_index(if self.reverse { 2 } else { 0 }))
                    .child(tile("Second in description", rgb8(75, 60, 75)).key("second").absolute().left(100.).top(80.).z_index(1))
                    .child(button("Swap card order").absolute().right(12.).bottom(12.).z_index(3)
                        .on_click(cx.listener(|s, _, _| s.reverse = !s.reverse))))
                .child(label("Natural stack size comes from the largest in-flow child; absolute overlays do not expand it."))
                .child(stack().align_self(AlignSelf::START).padding(12.).border(1., ThemeColor::Border)
                    .child(column().size(280., 80.).background(ThemeColor::Control))
                    .child(label("Centered over content").align_self(AlignSelf::CENTER).justify_self(AlignSelf::CENTER))));
        let mut root = stack().fill_width().fill_height().child(background);
        if self.overlay {
            root = root.child(stack().absolute().inset(0.).z_index(100).pointer_events(PointerEvents::Block)
                .background([0., 0., 0., 0.65]).align_items(AlignItems::CENTER).justify_items(AlignItems::CENTER)
                .child(column().width(360.).max_width(500.).padding(24.).gap(16.)
                    .background(ThemeColor::Surface).border(1., ThemeColor::Border).radius(8.)
                    .accessibility_role(SemanticRole::Group).accessibility_label("Overlay controls")
                    .child(label("Overlay owns interaction").font_size(22.))
                    .child(label("Clicks cannot reach the content underneath. The background remains painted but is excluded from Tab and accessibility. Use Tab to focus Close."))
                    .child(button("Close overlay").variant(ButtonVariant::Primary)
                        .on_click(cx.listener(|s, _, _| s.overlay = false)))));
        }
        root
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let root = cx.new(|_| Workspace {
            overlay: false,
            reverse: false,
            clicks: 0,
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — layout & overlays")
                .size(860., 600.),
            root,
        )?;
        Ok(())
    })
}
