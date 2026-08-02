//! Controlled numeric text entry.

use std::{fmt::Display, str::FromStr};

use crate::{View, text_field};

/// Builds a controlled numeric field and reports parsing failures explicitly.
pub fn numeric_field<Action, Number>(
    label_text: impl Into<String>,
    value: Number,
    on_changed: impl Fn(Result<Number, String>) -> Action + 'static,
) -> View<Action>
where
    Action: 'static,
    Number: Display + FromStr + 'static,
    Number::Err: Display,
{
    text_field(label_text, value.to_string(), move |text| {
        on_changed(text.parse().map_err(|error: Number::Err| error.to_string()))
    })
}
