//! Headless controlled settings form with validation and composed layout.

use astrelis_core::geometry::LogicalPoint;
use rxui_core::{
    Axis, Context, Element, EntityHarness, Render, button, checkbox, column,
    forms::{FormModel, ValidationIssue, ValidationResult},
    label, scroll, slider, split_pane, text_field,
};
use rxui_tree::SemanticAction;

const NAME: &str = "name";
const EMAIL: &str = "email";

struct Settings {
    name: String,
    email: String,
    notifications: bool,
    volume: f32,
    split: f32,
    scroll: LogicalPoint,
    form: FormModel<&'static str>,
}

impl Settings {
    fn validate_name(value: &str) -> ValidationResult {
        if value.trim().is_empty() {
            ValidationResult::issue(ValidationIssue::error("Name is required"))
        } else {
            ValidationResult::default()
        }
    }

    fn validate_email(value: &str) -> ValidationResult {
        if value.contains('@') {
            ValidationResult::default()
        } else {
            ValidationResult::issue(ValidationIssue::error("Email must contain @"))
        }
    }

    fn initial() -> Self {
        let mut settings = Self {
            name: "Ada".into(),
            email: "ada@example.com".into(),
            notifications: true,
            volume: 40.0,
            split: 0.3,
            scroll: LogicalPoint::ZERO,
            form: FormModel::default(),
        };
        settings
            .form
            .set_result(NAME, Self::validate_name(&settings.name));
        settings
            .form
            .set_result(EMAIL, Self::validate_email(&settings.email));
        settings
    }
}

impl Render for Settings {
    fn render(&mut self, cx: &mut Context<Self>) -> Element {
        let name_error = self
            .form
            .visible_result(&NAME)
            .and_then(ValidationResult::message)
            .map(str::to_owned);
        let email_error = self
            .form
            .visible_result(&EMAIL)
            .and_then(ValidationResult::message)
            .map(str::to_owned);

        let mut name = text_field("Name", self.name.clone())
            .on_input(cx.listener_value(|this, value: String, cx| {
                this.name = value;
                this.form.mark_dirty(NAME);
                this.form.set_result(NAME, Self::validate_name(&this.name));
                cx.notify();
            }))
            .on_commit(cx.listener_value(|this, value: String, cx| {
                this.name = value;
                this.form.mark_dirty(NAME);
                this.form.set_result(NAME, Self::validate_name(&this.name));
                cx.notify();
            }));
        if let Some(error) = name_error {
            name = name.error(error);
        }

        let mut email = text_field("Email", self.email.clone()).on_input(cx.listener_value(
            |this, value: String, cx| {
                this.email = value;
                this.form.mark_dirty(EMAIL);
                this.form
                    .set_result(EMAIL, Self::validate_email(&this.email));
                cx.notify();
            },
        ));
        if let Some(error) = email_error {
            email = email.error(error);
        }

        let form = column()
            .gap(8.0)
            .child(name)
            .child(email)
            .child(
                checkbox("Notifications", self.notifications).on_toggle(cx.listener_value(
                    |this, value, cx| {
                        this.notifications = value;
                        cx.notify();
                    },
                )),
            )
            .child(
                slider("Volume", self.volume, 0.0..=100.0)
                    .step(5.0)
                    .on_change(cx.listener_value(|this, value, cx| {
                        this.volume = value;
                        cx.notify();
                    })),
            )
            .child(button("Validate").on_click(cx.listener(|this, _, cx| {
                this.form.set_result(NAME, Self::validate_name(&this.name));
                this.form
                    .set_result(EMAIL, Self::validate_email(&this.email));
                this.form.validate_all();
                cx.notify();
            })));

        let content = scroll()
            .offset(self.scroll)
            .on_scroll(cx.listener_value(|this, value, cx| {
                this.scroll = value;
                cx.notify();
            }))
            .child(form);

        split_pane(Axis::Horizontal, self.split)
            .on_change(cx.listener_value(|this, value, cx| {
                this.split = value;
                cx.notify();
            }))
            .child(
                column()
                    .gap(8.0)
                    .child(label("Settings"))
                    .child(label("Profile")),
            )
            .child(content)
    }
}

fn main() {
    let mut harness = EntityHarness::new(|cx| cx.new(|_| Settings::initial()));
    harness.semantic_action("Email", SemanticAction::SetText("invalid".into()));
    harness.activate("Validate");
    println!("settings after validation failure:\n{}", harness.snapshot());

    harness.semantic_action("Email", SemanticAction::SetText("fixed@example.com".into()));
    println!("settings after fix:\n{}", harness.snapshot());
}
