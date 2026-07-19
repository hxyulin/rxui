//! Astreon 0.2 design-system gallery.

use std::io;

use astrelis_app::{App, AppContext, Runtime, RuntimeConfig};
use astrelis_core::geometry::Size;
use astrelis_platform::{WindowAttributes, WindowEvent, WindowId};
use astrelis_text::FontDatabase;
use astreon::prelude::*;

#[derive(Clone, Debug)]
enum Message {
    Theme(bool),
    Radio(usize),
    Combo(usize),
    Number(f64),
    Save,
}

struct Gallery {
    graphics: GraphicsContext,
    host: Option<WindowHost<Message>>,
    themes: ThemeSet,
    radio: Option<RadioGroup<Message>>,
    combo: Option<ComboBox<Message>>,
    number: Option<NumericField<Message>>,
    status: Option<ElementHandle<Label>>,
}

impl Gallery {
    fn new() -> Self {
        Self {
            graphics: GraphicsContext::new(),
            host: None,
            themes: ThemeSet::default(),
            radio: None,
            combo: None,
            number: None,
            status: None,
        }
    }

    fn apply(&mut self, message: Message) -> Result<(), io::Error> {
        let host = self.host.as_mut().expect("gallery host exists");
        let status = self.status.expect("status exists");
        match message {
            Message::Theme(dark) => {
                host.ui_mut().set_theme(if dark {
                    self.themes.dark.clone()
                } else {
                    self.themes.light.clone()
                });
                host.ui_mut()
                    .set_label_text(status, if dark { "Dark theme" } else { "Light theme" })
                    .map_err(io::Error::other)?;
            }
            Message::Radio(index) => {
                self.radio
                    .as_ref()
                    .expect("radio exists")
                    .set_selected(host.ui_mut(), Some(index))
                    .map_err(io::Error::other)?;
                host.ui_mut()
                    .set_label_text(status, format!("Radio option {} selected", index + 1))
                    .map_err(io::Error::other)?;
            }
            Message::Combo(index) => {
                self.combo
                    .as_mut()
                    .expect("combo exists")
                    .set_selected(host.ui_mut(), Some(index))
                    .map_err(io::Error::other)?;
                host.ui_mut()
                    .set_label_text(status, format!("Combo option {} selected", index + 1))
                    .map_err(io::Error::other)?;
            }
            Message::Number(value) => {
                self.number
                    .as_ref()
                    .expect("number exists")
                    .set_value(host.ui_mut(), value)
                    .map_err(io::Error::other)?;
                host.ui_mut()
                    .set_label_text(status, format!("Numeric value: {value:.2}"))
                    .map_err(io::Error::other)?;
            }
            Message::Save => host
                .ui_mut()
                .set_label_text(status, "Save icon button activated")
                .map_err(io::Error::other)?,
        }
        Ok(())
    }
}

impl App for Gallery {
    type Error = io::Error;

    fn resumed(&mut self, context: &mut AppContext<'_, '_, Self>) -> Result<(), Self::Error> {
        if self.host.is_some() {
            return Ok(());
        }
        let mut ui = Ui::new(FontDatabase::default(), self.themes.dark.clone());
        let content = ui
            .padding(ui.root(), Insets::all(24.0))
            .grow(1.0)
            .column()
            .finish();
        ui.label(content, "Astreon 0.2 design gallery").finish();

        let themes = ui.row(content).finish();
        let light = ui.button(themes, "Light").finish();
        let dark = ui.button(themes, "Dark").finish();
        ui.on_click(light, |event| event.emit(Message::Theme(false)));
        ui.on_click(dark, |event| event.emit(Message::Theme(true)));

        let icons_row = ui.row(content).finish();
        ui.add_widget(icons_row, IconView::new(icons::folder(), 24.0))
            .map_err(io::Error::other)?;
        ui.add_widget(icons_row, IconView::new(icons::settings(), 24.0))
            .map_err(io::Error::other)?;
        ui.add_widget(
            icons_row,
            IconButton::labelled(icons::save(), "Save", Message::Save),
        )
        .map_err(io::Error::other)?;

        let form = FormSection::new(
            &mut ui,
            content,
            "Editor essentials",
            Some("Keyboard-accessible retained controls"),
        )
        .map_err(io::Error::other)?;
        let radio = RadioGroup::new(
            &mut ui,
            form.content,
            vec![
                RadioOption::new("Perspective"),
                RadioOption::new("Orthographic"),
                RadioOption {
                    label: "Unavailable mode".into(),
                    enabled: false,
                },
            ],
            Some(0),
            Message::Radio,
        )
        .map_err(io::Error::other)?;
        let combo = ComboBox::new(
            &mut ui,
            form.content,
            "Select quality…",
            vec![
                ComboBoxItem {
                    label: "Low".into(),
                    message: Message::Combo(0),
                    enabled: true,
                },
                ComboBoxItem {
                    label: "Medium".into(),
                    message: Message::Combo(1),
                    enabled: true,
                },
                ComboBoxItem {
                    label: "High".into(),
                    message: Message::Combo(2),
                    enabled: true,
                },
            ],
            Some(1),
        )
        .map_err(io::Error::other)?;
        let number = NumericField::new(
            &mut ui,
            form.content,
            1.0,
            NumericFieldOptions {
                min: 0.25,
                max: 4.0,
                step: 0.25,
                decimals: 2,
            },
            Message::Number,
        )
        .map_err(io::Error::other)?;
        let status = ui.label(content, "Ready").finish();

        let host = WindowHost::open(
            context,
            &self.graphics,
            ui,
            WindowHostOptions {
                window: WindowAttributes {
                    title: "Astreon design gallery".into(),
                    inner_size: Some(Size::new(720.0, 760.0)),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
        self.radio = Some(radio);
        self.combo = Some(combo);
        self.number = Some(number);
        self.status = Some(status);
        self.host = Some(host);
        Ok(())
    }

    fn window_event(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        id: WindowId,
        event: WindowEvent,
    ) -> Result<(), Self::Error> {
        let Some(host) = &mut self.host else {
            return Ok(());
        };
        let update = host
            .handle_event(&context.clipboard(), &event)
            .map_err(io::Error::other)?;
        if update.close_requested {
            context.unregister_window(id);
            self.host = None;
            context.exit();
            return Ok(());
        }
        let messages = host.drain_messages().collect::<Vec<_>>();
        for message in messages {
            self.apply(message)?;
        }
        if update.redraw
            || self
                .host
                .as_ref()
                .is_some_and(|host| host.ui().needs_redraw())
        {
            context.invalidate_window(id);
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        _context: &mut AppContext<'_, '_, Self>,
        _window: WindowId,
    ) -> Result<(), Self::Error> {
        if let Some(host) = &mut self.host {
            host.redraw().map_err(io::Error::other)?;
        }
        Ok(())
    }
}

fn main() -> Result<(), astrelis_app::RuntimeError<io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        Gallery::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}
