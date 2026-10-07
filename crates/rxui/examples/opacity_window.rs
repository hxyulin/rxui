//! Complete native group-opacity example. Fading overlapping content once differs
//! from fading each child; native hosting records isolated layers automatically.
use rxui::{Element, prelude::*};
struct Demo {
    alpha: f32,
    nested: bool,
    icon: Image,
    input: String,
}
fn cards(alpha: f32, nested: bool, per_child: bool, icon: &Image) -> Element {
    let card = |name: &str, color, left, top, opacity| {
        column()
            .absolute()
            .left(left)
            .top(top)
            .size(180., 120.)
            .padding(16.)
            .gap(8.)
            .background(color)
            .radius(8.)
            .opacity(opacity)
            .child(
                row()
                    .gap(8.)
                    .child(image(icon.clone()).size(20., 20.))
                    .child(label(name).color([1.; 4])),
            )
            .child(
                label("Text fades with its card")
                    .font_size(14.)
                    .color([1.; 4]),
            )
    };
    stack()
        .size(320., 220.)
        .opacity(if per_child { 1. } else { alpha })
        .child(
            card(
                "First card",
                rgb8(190, 55, 55),
                16.,
                16.,
                if per_child { alpha } else { 1. },
            )
            .key("first"),
        )
        .child(
            card(
                "Second card",
                rgb8(40, 140, 100),
                100.,
                80.,
                if per_child {
                    alpha
                } else if nested {
                    0.5
                } else {
                    1.
                },
            )
            .key("second"),
        )
}
impl View for Demo {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().fill_width().fill_height().padding(24.).gap(16.)
            .child(label("True group opacity").font_size(28.).accessibility_role(SemanticRole::Heading))
            .child(label("The left subtree is flattened, then faded once. The right cards fade separately; their overlap blends differently."))
            .child(row().gap(12.)
                .child(button(format!("Opacity: {:.0}% — cycle",self.alpha*100.))
                    .on_click(cx.listener(|s,_,_|s.alpha = if s.alpha < 1. {s.alpha+0.25} else {0.})))
                .child(button(if self.nested {"Nested 50%: on"} else {"Nested 50%: off"})
                    .on_click(cx.listener(|s,_,_|s.nested = !s.nested))))
            .child(row().gap(24.)
                .child(column().gap(8.).child(label("One group"))
                    .child(cards(self.alpha,self.nested,false,&self.icon)))
                .child(column().gap(8.).child(label("Separate card groups"))
                    .child(cards(self.alpha,false,true,&self.icon))))
            .child(label("Opacity affects painting only. Use inert or pointer policy separately when fading out interactive content."))
            .child(column().padding(12.).background(ThemeColor::Surface).opacity(self.alpha)
                .child(label("Controls, focus rings and editing content share the layer"))
                .child(text_input(self.input.clone()).width(360.).accessibility_label("Faded text input").on_change(cx.listener(|s, edit: &TextChangeEvent, _| s.input = edit.value.clone()))))
    }
}
fn main() -> Result<(), ApplicationError> {
    Application::new().run(|cx| {
        let icon = Image::from_rgba8(2, 2, vec![255; 16])?;
        let root = cx.new(|_| Demo {
            alpha: 0.5,
            nested: false,
            icon,
            input: "Try typing here".into(),
        });
        cx.open_window(
            WindowOptions::new()
                .title("RXUI — group opacity")
                .size(760., 680.),
            root,
        )?;
        Ok(())
    })
}
