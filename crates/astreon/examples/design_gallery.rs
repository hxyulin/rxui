//! Astreon 0.3 design-system gallery.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

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
    Pressed(&'static str),
    Text(String),
    Toggle(bool),
    Slide(f32),
    ShowDialog,
    ConfirmDialog,
    CancelDialog,
    PushToast,
    Dismiss(ToastId),
}

/// Labels and buttons whose [`WidgetStyle`] carries a theme color.
///
/// Color overrides snapshot the color at the time of the `set_widget_style`
/// call, so [`apply_styles`] must run again after every `set_theme`.
#[derive(Clone, Copy)]
struct StyledText {
    title: ElementHandle<Label>,
    heading: ElementHandle<Label>,
    caption: ElementHandle<Label>,
    muted: ElementHandle<Label>,
    disabled: ElementHandle<Label>,
    destructive: ElementHandle<Button>,
    sections: [FormSection; 5],
}

fn apply_styles(ui: &mut Ui<Message>, styled: &StyledText) -> Result<(), UiError> {
    for section in &styled.sections {
        section.restyle(ui)?;
    }
    let theme = ui.theme();
    let type_scale = theme.type_scale;
    let muted = theme.muted_foreground;
    let disabled = theme.disabled_foreground;
    let danger = theme.danger;
    let heading = WidgetStyle {
        font_size: Some(type_scale.heading),
        font_weight: Some(type_scale.heading_weight),
        ..Default::default()
    };
    ui.set_widget_style(styled.title, heading)?;
    ui.set_widget_style(styled.heading, heading)?;
    ui.set_widget_style(
        styled.caption,
        WidgetStyle {
            font_size: Some(type_scale.caption),
            ..Default::default()
        },
    )?;
    ui.set_widget_style(
        styled.muted,
        WidgetStyle {
            foreground: Some(muted),
            ..Default::default()
        },
    )?;
    ui.set_widget_style(
        styled.disabled,
        WidgetStyle {
            foreground: Some(disabled),
            ..Default::default()
        },
    )?;
    ui.set_widget_style(
        styled.destructive,
        WidgetStyle {
            foreground: Some(danger),
            ..Default::default()
        },
    )?;
    Ok(())
}

struct Gallery {
    graphics: GraphicsContext,
    host: Option<WindowHost<Message>>,
    themes: ThemeSet,
    radio: Option<RadioGroup<Message>>,
    combo: Option<ComboBox<Message>>,
    number: Option<NumericField<Message>>,
    status: Option<ElementHandle<Label>>,
    styled: Option<StyledText>,
    dialogs: DialogHost,
    toast_queue: ToastQueue<Message>,
    toast_host: Option<ToastHost>,
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
            styled: None,
            dialogs: DialogHost::new(),
            toast_queue: ToastQueue::default(),
            toast_host: None,
        }
    }

    fn set_status(&mut self, text: impl Into<String>) -> Result<(), io::Error> {
        let host = self.host.as_mut().expect("gallery host exists");
        host.ui_mut()
            .set_label_text(self.status.expect("status exists"), text)
            .map_err(io::Error::other)
    }

    fn sync_toasts(&mut self) -> Result<(), io::Error> {
        let host = self.host.as_mut().expect("gallery host exists");
        self.toast_host
            .as_mut()
            .expect("toast host exists")
            .sync(host.ui_mut(), &self.toast_queue, Message::Dismiss)
            .map_err(io::Error::other)
    }

    fn notify(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        toast: Toast<Message>,
    ) -> Result<(), io::Error> {
        let now = context.now();
        self.toast_queue.push(toast, now);
        self.sync_toasts()?;
        if let Some(deadline) = self.toast_queue.next_deadline() {
            context.set_timeout(deadline.saturating_duration_since(now), |app, context| {
                app.toast_queue.expire(context.now());
                app.sync_toasts()?;
                if let Some(host) = &app.host {
                    context.invalidate_window(host.id());
                }
                Ok(())
            });
        }
        Ok(())
    }

    fn open_dialog(&mut self) -> Result<(), io::Error> {
        let host = self.host.as_mut().expect("gallery host exists");
        self.dialogs
            .show(
                host.ui_mut(),
                DialogOptions {
                    title: "Remove asset?".into(),
                    description: Some(
                        "Dialogs float on the overlay surface and cast the theme shadow.".into(),
                    ),
                },
                vec![
                    DialogAction {
                        label: "Cancel".into(),
                        message: Message::CancelDialog,
                        role: DialogActionRole::Cancel,
                        enabled: true,
                    },
                    DialogAction {
                        label: "Remove".into(),
                        message: Message::ConfirmDialog,
                        role: DialogActionRole::Destructive,
                        enabled: true,
                    },
                ],
                |ui, content| {
                    ui.add_label(
                        content,
                        "The asset stays on disk and can be re-imported later.",
                    )?;
                    Ok(())
                },
            )
            .map_err(io::Error::other)
    }

    fn apply(
        &mut self,
        context: &mut AppContext<'_, '_, Self>,
        message: Message,
    ) -> Result<(), io::Error> {
        match message {
            Message::Theme(dark) => {
                let styled = self.styled.expect("styled exists");
                let host = self.host.as_mut().expect("gallery host exists");
                host.ui_mut().set_theme(if dark {
                    self.themes.dark.clone()
                } else {
                    self.themes.light.clone()
                });
                // Color overrides snapshot the theme, so re-apply them.
                apply_styles(host.ui_mut(), &styled).map_err(io::Error::other)?;
                self.set_status(if dark { "Dark theme" } else { "Light theme" })?;
            }
            Message::Radio(index) => {
                let host = self.host.as_mut().expect("gallery host exists");
                self.radio
                    .as_ref()
                    .expect("radio exists")
                    .set_selected(host.ui_mut(), Some(index))
                    .map_err(io::Error::other)?;
                self.set_status(format!("Radio option {} selected", index + 1))?;
            }
            Message::Combo(index) => {
                let host = self.host.as_mut().expect("gallery host exists");
                self.combo
                    .as_mut()
                    .expect("combo exists")
                    .set_selected(host.ui_mut(), Some(index))
                    .map_err(io::Error::other)?;
                self.set_status(format!("Combo option {} selected", index + 1))?;
            }
            Message::Number(value) => {
                let host = self.host.as_mut().expect("gallery host exists");
                self.number
                    .as_ref()
                    .expect("number exists")
                    .set_value(host.ui_mut(), value)
                    .map_err(io::Error::other)?;
                self.set_status(format!("Numeric value: {value:.2}"))?;
            }
            Message::Save => self.set_status("Save icon button activated")?,
            Message::Pressed(name) => self.set_status(format!("{name} button activated"))?,
            Message::Text(value) => self.set_status(format!("Text: {value}"))?,
            Message::Toggle(on) => {
                self.set_status(if on { "Snapping on" } else { "Snapping off" })?;
            }
            Message::Slide(value) => self.set_status(format!("Slider value: {value:.0}"))?,
            Message::ShowDialog => self.open_dialog()?,
            Message::ConfirmDialog => {
                let host = self.host.as_mut().expect("gallery host exists");
                self.dialogs
                    .close(host.ui_mut())
                    .map_err(io::Error::other)?;
                self.set_status("Dialog confirmed")?;
            }
            Message::CancelDialog => {
                let host = self.host.as_mut().expect("gallery host exists");
                self.dialogs
                    .close(host.ui_mut())
                    .map_err(io::Error::other)?;
                self.set_status("Dialog cancelled")?;
            }
            Message::PushToast => self.notify(
                context,
                Toast::new(
                    ToastLevel::Success,
                    "Toast pushed",
                    "Toasts float on the overlay surface with the theme shadow.",
                ),
            )?,
            Message::Dismiss(id) => {
                self.toast_queue.dismiss(id, context.now());
                self.sync_toasts()?;
            }
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
            .scroll_view()
            .grow(1.0)
            .column()
            .finish();
        let title = ui.label(content, "Astreon 0.3 design gallery").finish();

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

        let typography = FormSection::new(
            &mut ui,
            content,
            "Typography",
            Some("Type scale and foreground tokens"),
        )
        .map_err(io::Error::other)?;
        let heading = ui.label(typography.content, "Heading 15 semibold").finish();
        ui.label(typography.content, "Body 13 regular").finish();
        let caption = ui.label(typography.content, "Caption 11").finish();
        let muted = ui
            .label(typography.content, "Muted foreground for secondary copy")
            .finish();
        let disabled = ui
            .label(typography.content, "Disabled foreground for inactive text")
            .finish();

        let buttons = FormSection::new(
            &mut ui,
            content,
            "Buttons",
            Some("Command-button states and a destructive action"),
        )
        .map_err(io::Error::other)?;
        let button_row = ui.row(buttons.content).finish();
        ui.add_widget(
            button_row,
            CommandButton::new("Normal", Message::Pressed("Normal")),
        )
        .map_err(io::Error::other)?;
        ui.add_widget(
            button_row,
            CommandButton::new("Disabled", Message::Pressed("Disabled")).enabled(false),
        )
        .map_err(io::Error::other)?;
        ui.add_widget(
            button_row,
            CommandButton::new("Checked", Message::Pressed("Checked")).checked(true),
        )
        .map_err(io::Error::other)?;
        let destructive = ui.button(button_row, "Delete").finish();
        ui.on_click(destructive, |event| event.emit(Message::Pressed("Delete")));

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

        let inputs = FormSection::new(
            &mut ui,
            content,
            "Inputs",
            Some("Field, checkbox, and slider tokens"),
        )
        .map_err(io::Error::other)?;
        let field = ui
            .text_field(inputs.content, "Editable text")
            .width(px(240.0))
            .finish();
        ui.on_text_changed(field, |event, text| {
            event.emit(Message::Text(text.to_string()));
        });
        let toggle_row = ui.row(inputs.content).finish();
        let snap = ui.checkbox(toggle_row, true).finish();
        ui.label(toggle_row, "Snap to grid").finish();
        ui.on_checked(snap, |event, on| event.emit(Message::Toggle(on)));
        let slider = ui
            .slider(inputs.content, 0.0, 100.0, 5.0, 40.0)
            .width(px(240.0))
            .finish();
        ui.on_slider(slider, |event, value| event.emit(Message::Slide(value)));

        let overlays = FormSection::new(
            &mut ui,
            content,
            "Overlays",
            Some("Floating surfaces with real gaussian shadows"),
        )
        .map_err(io::Error::other)?;
        let overlay_row = ui.row(overlays.content).finish();
        let show_dialog = ui.button(overlay_row, "Show dialog").finish();
        ui.on_click(show_dialog, |event| event.emit(Message::ShowDialog));
        let push_toast = ui.button(overlay_row, "Push toast").finish();
        ui.on_click(push_toast, |event| event.emit(Message::PushToast));
        let toast_host = ToastHost::new(&mut ui).map_err(io::Error::other)?;

        let status = ui.label(content, "Ready").finish();

        let styled = StyledText {
            title,
            heading,
            caption,
            muted,
            disabled,
            destructive,
            sections: [typography, buttons, form, inputs, overlays],
        };
        apply_styles(&mut ui, &styled).map_err(io::Error::other)?;

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
        self.styled = Some(styled);
        self.toast_host = Some(toast_host);
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
            self.apply(context, message)?;
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

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), astrelis_app::RuntimeError<io::Error>> {
    Runtime::finish(astrelis_platform_winit::run_return(Runtime::new(
        Gallery::new(),
        RuntimeConfig::default(),
    )))
    .map(|_| ())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
