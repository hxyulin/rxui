//! RXUI design-system gallery.
//!
//! Runs on the high-level [`rxui::app`] runner: [`App::build`] assembles the
//! gallery window and [`App::update`] applies each typed message. Toast
//! deadlines arrive as [`Message::ExpireToasts`] timer messages.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use rxui::prelude::*;

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
    ExpireToasts,
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

    fn set_status(&self, cx: &mut AppCx<'_, Message>, text: impl Into<String>) -> rxui::Result<()> {
        cx.source_ui()?
            .set_label_text(self.status.expect("status exists"), text)?;
        Ok(())
    }

    fn sync_toasts(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let ui = cx.source_ui()?;
        self.toast_host.as_mut().expect("toast host exists").sync(
            ui,
            &self.toast_queue,
            Message::Dismiss,
        )?;
        Ok(())
    }

    fn notify(&mut self, cx: &mut AppCx<'_, Message>, toast: Toast<Message>) -> rxui::Result<()> {
        let now = cx.now();
        self.toast_queue.push(toast, now);
        self.sync_toasts(cx)?;
        if let Some(deadline) = self.toast_queue.next_deadline() {
            cx.set_timeout(
                deadline.saturating_duration_since(now),
                Message::ExpireToasts,
            );
        }
        Ok(())
    }

    fn open_dialog(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let ui = cx.source_ui()?;
        self.dialogs.show(
            ui,
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
        )?;
        Ok(())
    }
}

impl App for Gallery {
    type Message = Message;

    fn build(&mut self, cx: &mut AppCx<'_, Message>) -> rxui::Result<()> {
        let mut ui = cx.new_ui();
        let content = ui
            .padding(ui.root(), Insets::all(24.0))
            .grow(1.0)
            .scroll_view()
            .grow(1.0)
            .column()
            .finish();
        let title = ui.label(content, "RXUI design gallery").finish();

        let themes = ui.row(content).finish();
        let light = ui.button(themes, "Light").finish();
        let dark = ui.button(themes, "Dark").finish();
        ui.on_click(light, |event| event.emit(Message::Theme(false)));
        ui.on_click(dark, |event| event.emit(Message::Theme(true)));

        let icons_row = ui.row(content).finish();
        ui.add_widget(icons_row, IconView::new(icons::folder(), 24.0))?;
        ui.add_widget(icons_row, IconView::new(icons::settings(), 24.0))?;
        ui.add_widget(
            icons_row,
            IconButton::labelled(icons::save(), "Save", Message::Save),
        )?;

        let typography = FormSection::new(
            &mut ui,
            content,
            "Typography",
            Some("Type scale and foreground tokens"),
        )?;
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
        )?;
        let button_row = ui.row(buttons.content).finish();
        ui.add_widget(
            button_row,
            CommandButton::new("Normal", Message::Pressed("Normal")),
        )?;
        ui.add_widget(
            button_row,
            CommandButton::new("Disabled", Message::Pressed("Disabled")).enabled(false),
        )?;
        ui.add_widget(
            button_row,
            CommandButton::new("Checked", Message::Pressed("Checked")).checked(true),
        )?;
        let destructive = ui.button(button_row, "Delete").finish();
        ui.on_click(destructive, |event| event.emit(Message::Pressed("Delete")));

        let form = FormSection::new(
            &mut ui,
            content,
            "Editor essentials",
            Some("Keyboard-accessible retained controls"),
        )?;
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
        )?;
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
        )?;
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
        )?;

        let inputs = FormSection::new(
            &mut ui,
            content,
            "Inputs",
            Some("Field, checkbox, and slider tokens"),
        )?;
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
        )?;
        let overlay_row = ui.row(overlays.content).finish();
        let show_dialog = ui.button(overlay_row, "Show dialog").finish();
        ui.on_click(show_dialog, |event| event.emit(Message::ShowDialog));
        let push_toast = ui.button(overlay_row, "Push toast").finish();
        ui.on_click(push_toast, |event| event.emit(Message::PushToast));
        let toast_host = ToastHost::new(&mut ui)?;

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
        apply_styles(&mut ui, &styled)?;

        self.radio = Some(radio);
        self.combo = Some(combo);
        self.number = Some(number);
        self.status = Some(status);
        self.styled = Some(styled);
        self.toast_host = Some(toast_host);
        cx.open_window(
            WindowConfig::new("RXUI design gallery").size(720.0, 760.0),
            ui,
        )?;
        Ok(())
    }

    fn update(&mut self, cx: &mut AppCx<'_, Message>, message: Message) -> rxui::Result<()> {
        match message {
            Message::Theme(dark) => {
                let styled = self.styled.expect("styled exists");
                let theme = if dark {
                    self.themes.dark.clone()
                } else {
                    self.themes.light.clone()
                };
                let ui = cx.source_ui()?;
                ui.set_theme(theme);
                // Color overrides snapshot the theme, so re-apply them.
                apply_styles(ui, &styled)?;
                self.set_status(cx, if dark { "Dark theme" } else { "Light theme" })?;
            }
            Message::Radio(index) => {
                let ui = cx.source_ui()?;
                self.radio
                    .as_ref()
                    .expect("radio exists")
                    .set_selected(ui, Some(index))?;
                self.set_status(cx, format!("Radio option {} selected", index + 1))?;
            }
            Message::Combo(index) => {
                let ui = cx.source_ui()?;
                self.combo
                    .as_mut()
                    .expect("combo exists")
                    .set_selected(ui, Some(index))?;
                self.set_status(cx, format!("Combo option {} selected", index + 1))?;
            }
            Message::Number(value) => {
                let ui = cx.source_ui()?;
                self.number
                    .as_ref()
                    .expect("number exists")
                    .set_value(ui, value)?;
                self.set_status(cx, format!("Numeric value: {value:.2}"))?;
            }
            Message::Save => self.set_status(cx, "Save icon button activated")?,
            Message::Pressed(name) => self.set_status(cx, format!("{name} button activated"))?,
            Message::Text(value) => self.set_status(cx, format!("Text: {value}"))?,
            Message::Toggle(on) => {
                self.set_status(cx, if on { "Snapping on" } else { "Snapping off" })?;
            }
            Message::Slide(value) => self.set_status(cx, format!("Slider value: {value:.0}"))?,
            Message::ShowDialog => self.open_dialog(cx)?,
            Message::ConfirmDialog => {
                self.dialogs.close(cx.source_ui()?)?;
                self.set_status(cx, "Dialog confirmed")?;
            }
            Message::CancelDialog => {
                self.dialogs.close(cx.source_ui()?)?;
                self.set_status(cx, "Dialog cancelled")?;
            }
            Message::PushToast => self.notify(
                cx,
                Toast::new(
                    ToastLevel::Success,
                    "Toast pushed",
                    "Toasts float on the overlay surface with the theme shadow.",
                ),
            )?,
            Message::Dismiss(id) => {
                self.toast_queue.dismiss(id, cx.now());
                self.sync_toasts(cx)?;
            }
            Message::ExpireToasts => {
                self.toast_queue.expire(cx.now());
                self.sync_toasts(cx)?;
            }
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> MainResult {
    let gallery = Gallery::new();
    let theme = gallery.themes.dark.clone();
    run_with(gallery, AppConfig::default().theme(theme))
}

#[cfg(target_arch = "wasm32")]
fn main() {}
