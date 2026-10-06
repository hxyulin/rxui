//! Standalone image fitting and composed-button example. Each placement shares one
//! Image handle; tint/fit/filter changes reuse its uploaded pixels. No external assets.
use rxui::prelude::*;

struct Gallery {
    image: Image,
    nearest: bool,
    tinted: bool,
    clicks: u32,
}
impl View for Gallery {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let filter = if self.nearest {
            ImageFilter::Nearest
        } else {
            ImageFilter::Linear
        };
        let tint = if self.tinted {
            rgb8(150, 200, 255)
        } else {
            [1.; 4]
        };
        column()
            .padding(24.)
            .gap(16.)
            .fill_width()
            .fill_height()
            .scroll_y()
            .child(
                label("Shared images & composed controls")
                    .font_size(26.)
                    .accessibility_role(SemanticRole::Heading),
            )
            .child(
                label("Contain keeps the whole image; Cover crops; Stretch fills the box.")
                    .color(ThemeColor::TextMuted),
            )
            .child(
                row().gap(16.).children(
                    [
                        (ImageFit::Contain, "Contain"),
                        (ImageFit::Cover, "Cover"),
                        (ImageFit::Stretch, "Stretch"),
                    ]
                    .into_iter()
                    .map(|(fit, name)| {
                        column().gap(8.).child(label(name)).child(
                            image(self.image.clone())
                                .key(name)
                                .width(220.)
                                .height(180.)
                                .fit(fit)
                                .filter(filter)
                                .tint(tint)
                                .background(ThemeColor::Surface)
                                .border(1., ThemeColor::Border)
                                .accessibility_label(format!("{name} color grid")),
                        )
                    }),
                ),
            )
            .child(
                row()
                    .gap(12.)
                    .child(
                        button(
                            row()
                                .gap(8.)
                                .child(
                                    image(self.image.clone())
                                        .width(24.)
                                        .height(24.)
                                        .accessibility_hidden(true),
                                )
                                .child(label("Primary action")),
                        )
                        .variant(ButtonVariant::Primary)
                        .on_click(cx.listener(|s, _, _| s.clicks += 1)),
                    )
                    .child(
                        button("Toggle nearest")
                            .on_click(cx.listener(|s, _, _| s.nearest = !s.nearest)),
                    )
                    .child(
                        button("Toggle tint")
                            .variant(ButtonVariant::Quiet)
                            .on_click(cx.listener(|s, _, _| s.tinted = !s.tinted)),
                    ),
            )
            .child(
                button(
                    row()
                        .gap(8.)
                        .child(
                            image(self.image.clone())
                                .width(24.)
                                .height(24.)
                                .accessibility_hidden(true),
                        )
                        .child(label("Disabled composed action")),
                )
                .disabled(true),
            )
            .child(label(format!(
                "Clicks: {} · Filter: {:?} · Tint: {}",
                self.clicks, filter, self.tinted
            )))
            .child(
                button("Open another placement").on_click(cx.listener(|_, _, cx| {
                    let root = cx.entity().upgrade().unwrap();
                    cx.open_window(
                        WindowOptions::new()
                            .title("RXUI — shared images")
                            .size(780., 500.)
                            .theme(Theme::light()),
                        root,
                    )
                    .unwrap();
                })),
            )
    }
}
fn main() -> Result<(), ApplicationError> {
    let mut pixels = Vec::with_capacity(32 * 16 * 4);
    for y in 0..16 {
        for x in 0..32 {
            let cell = ((x / 4) + (y / 4)) % 2 == 0;
            pixels.extend_from_slice(&[
                if cell { 235 } else { 30 },
                (x * 8) as u8,
                (y * 16) as u8,
                if x < 4 { 100 } else { 255 },
            ]);
        }
    }
    let image = Image::from_rgba8(32, 16, pixels)?;
    Application::new().run(|cx| {
        let root = cx.new(|_| Gallery {
            image,
            nearest: false,
            tinted: false,
            clicks: 0,
        });
        cx.open_window(
            WindowOptions::new().title("RXUI — images").size(780., 500.),
            root,
        )?;
        Ok(())
    })
}
