//! Editable property rows shown when [`InspectorOptions::allow_editing`] is
//! set.
//!
//! Every editor is commit-based (Enter/blur, checkbox toggles, numeric-field
//! steps): the details pane rebuilds wholesale after each applied edit, and a
//! live keystroke-driven editor would fight that rebuild for focus and caret.
//!
//! [`InspectorOptions::allow_editing`]: crate::InspectorOptions::allow_editing

use std::rc::Rc;

use astrelis_core::color::Color;
use astrelis_ui_core::{
    Alignment, Column, Edges, ElementHandle, ElementInspection, EventFilter, Insets, LayoutStyle,
    Length, RoutedEventKind, Ui, UiError, Visibility, WidgetStyle,
};
use astreon_widgets::{ComboBox, ComboBoxItem, NumericField, NumericFieldOptions};

use crate::{
    InspectorAction,
    details::{fmt_edges, fmt_length, number},
};

pub(crate) type Map<Message> = Rc<dyn Fn(InspectorAction) -> Message>;

/// Visibility dropdown and enabled checkbox.
pub(crate) fn edit_state<Message: Clone + 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
    map: &Map<Message>,
) -> Result<(), UiError> {
    let id = node.id;
    let row = editor_row(ui, parent, "visibility")?;
    let choice = |visibility: Visibility| ComboBoxItem {
        label: format!("{visibility:?}"),
        message: (map)(InspectorAction::SetElementVisibility { id, visibility }),
        enabled: true,
    };
    let selected = match node.visibility {
        Visibility::Visible => 0,
        Visibility::Hidden => 1,
        Visibility::Collapsed => 2,
    };
    let combo = ComboBox::new(
        ui,
        row,
        "Visibility",
        vec![
            choice(Visibility::Visible),
            choice(Visibility::Hidden),
            choice(Visibility::Collapsed),
        ],
        Some(selected),
    )?;
    // The dropdown must paint above the panel overlay it lives in.
    ui.set_z_index(combo.popup.popover().content(), crate::INSPECTOR_Z + 3)?;
    let row = editor_row(ui, parent, "enabled")?;
    let checkbox = ui.add_checkbox(row, node.enabled)?;
    let map = map.clone();
    ui.listen(
        checkbox,
        None,
        EventFilter::ValueChanged,
        move |context, event| {
            if let RoutedEventKind::CheckedChanged(enabled) = event.kind {
                context.emit(map(InspectorAction::SetElementEnabled { id, enabled }));
            }
        },
    )?;
    Ok(())
}

/// Width, height, margin, optional padding, and flex-factor editors.
pub(crate) fn edit_layout<Message: Clone + 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
    padding: Option<Insets>,
    map: &Map<Message>,
) -> Result<(), UiError> {
    let id = node.id;
    let layout = node.declared_layout;
    let commit_layout = |map: &Map<Message>, mutate: fn(LayoutStyle, Length) -> LayoutStyle| {
        let map = map.clone();
        move |text: &str| {
            // An unparseable entry re-commits the current layout, which forces
            // the details rebuild that snaps the field back to its old value.
            let layout = parse_length(text).map_or(layout, |length| mutate(layout, length));
            map(InspectorAction::SetElementLayout { id, layout })
        }
    };
    text_editor(
        ui,
        parent,
        "width",
        fmt_length(layout.width),
        commit_layout(map, |mut layout, length| {
            layout.width = length;
            layout
        }),
    )?;
    text_editor(
        ui,
        parent,
        "height",
        fmt_length(layout.height),
        commit_layout(map, |mut layout, length| {
            layout.height = length;
            layout
        }),
    )?;
    let margin_map = map.clone();
    text_editor(
        ui,
        parent,
        "margin",
        fmt_edges(layout.margin),
        move |text| {
            let layout =
                parse_edges(text).map_or(layout, |margin| LayoutStyle { margin, ..layout });
            margin_map(InspectorAction::SetElementLayout { id, layout })
        },
    )?;
    if let Some(current) = padding {
        let padding_map = map.clone();
        text_editor(ui, parent, "padding", fmt_insets(current), move |text| {
            let padding = parse_insets(text).unwrap_or(current);
            padding_map(InspectorAction::SetElementPadding { id, padding })
        })?;
    }
    let factors = NumericFieldOptions {
        min: 0.0,
        max: 1000.0,
        step: 0.1,
        decimals: 1,
    };
    let row = editor_row(ui, parent, "grow")?;
    let grow_map = map.clone();
    NumericField::new(ui, row, f64::from(layout.grow), factors, move |value| {
        grow_map(InspectorAction::SetElementLayout {
            id,
            layout: LayoutStyle {
                grow: value as f32,
                ..layout
            },
        })
    })?;
    let row = editor_row(ui, parent, "shrink")?;
    let shrink_map = map.clone();
    NumericField::new(ui, row, f64::from(layout.shrink), factors, move |value| {
        shrink_map(InspectorAction::SetElementLayout {
            id,
            layout: LayoutStyle {
                shrink: value as f32,
                ..layout
            },
        })
    })?;
    Ok(())
}

/// Font-size and hex-color editors for the declared widget style.
pub(crate) fn edit_style<Message: Clone + 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
    map: &Map<Message>,
) -> Result<(), UiError> {
    let id = node.id;
    let style = node.widget_style;
    let row = editor_row(ui, parent, "font-size")?;
    let size_map = map.clone();
    NumericField::new(
        ui,
        row,
        f64::from(style.font_size.unwrap_or(0.0)),
        NumericFieldOptions {
            min: 0.0,
            max: 128.0,
            step: 1.0,
            decimals: 0,
        },
        move |value| {
            // Zero clears the override so the theme's scale applies again.
            size_map(InspectorAction::SetElementStyle {
                id,
                style: WidgetStyle {
                    font_size: (value > 0.0).then_some(value as f32),
                    ..style
                },
            })
        },
    )?;
    let foreground_map = map.clone();
    text_editor(
        ui,
        parent,
        "foreground",
        fmt_color(style.foreground),
        move |text| {
            let style = parse_color(text).map_or(style, |foreground| WidgetStyle {
                foreground,
                ..style
            });
            foreground_map(InspectorAction::SetElementStyle { id, style })
        },
    )?;
    let background_map = map.clone();
    text_editor(
        ui,
        parent,
        "background",
        fmt_color(style.background),
        move |text| {
            let style = parse_color(text).map_or(style, |background| WidgetStyle {
                background,
                ..style
            });
            background_map(InspectorAction::SetElementStyle { id, style })
        },
    )?;
    Ok(())
}

/// Free-text editor replacing a label's, button's, or text field's content.
pub(crate) fn edit_text<Message: Clone + 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
    seed: String,
    map: &Map<Message>,
) -> Result<(), UiError> {
    let id = node.id;
    let map = map.clone();
    text_editor(ui, parent, "text", seed, move |text| {
        map(InspectorAction::SetElementText {
            id,
            text: text.to_string(),
        })
    })?;
    Ok(())
}

/// A muted-key row shell that an editor control fills.
fn editor_row<Message: 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    key: &str,
) -> Result<ElementHandle<astrelis_ui_core::Row>, UiError> {
    let caption = ui.theme().type_scale.caption;
    let muted = ui.theme().muted_foreground;
    let row = ui.add_row(parent)?;
    ui.set_flex(row, 8.0, Alignment::Center)?;
    ui.set_layout(
        row,
        LayoutStyle {
            width: Length::Percent(1.0),
            ..LayoutStyle::default()
        },
    )?;
    let key = ui.add_label(row, key)?;
    ui.set_widget_style(
        key,
        WidgetStyle {
            foreground: Some(muted),
            font_size: Some(caption),
            ..WidgetStyle::default()
        },
    )?;
    ui.set_layout(
        key,
        LayoutStyle {
            width: Length::Px(96.0),
            shrink: 0.0,
            ..LayoutStyle::default()
        },
    )?;
    Ok(row)
}

/// A commit-on-submit text field editor in a keyed row.
fn text_editor<Message: 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    key: &str,
    value: String,
    commit: impl Fn(&str) -> Message + 'static,
) -> Result<(), UiError> {
    let row = editor_row(ui, parent, key)?;
    let field = ui.add_text_field(row, value)?;
    ui.set_layout(
        field,
        LayoutStyle {
            grow: 1.0,
            ..LayoutStyle::default()
        },
    )?;
    ui.listen(
        field,
        None,
        EventFilter::ValueChanged,
        move |context, event| {
            if let RoutedEventKind::TextSubmitted(text) = &event.kind {
                context.emit(commit(text));
            }
        },
    )?;
    Ok(())
}

/// Parses `auto`, `120`, `120px`, or `50%` into a [`Length`].
pub(crate) fn parse_length(text: &str) -> Option<Length> {
    let text = text.trim().to_ascii_lowercase();
    if text == "auto" {
        return Some(Length::Auto);
    }
    if let Some(percent) = text.strip_suffix('%') {
        let value: f32 = percent.trim().parse().ok()?;
        return value.is_finite().then_some(Length::Percent(value / 100.0));
    }
    let value: f32 = text
        .strip_suffix("px")
        .unwrap_or(&text)
        .trim()
        .parse()
        .ok()?;
    value.is_finite().then_some(Length::Px(value))
}

/// Parses CSS-style shorthand (`8`, `8 12`, `8 12 4`, `8 12 4 0`) into edges.
pub(crate) fn parse_edges(text: &str) -> Option<Edges<Length>> {
    let values = text
        .split_whitespace()
        .map(parse_length)
        .collect::<Option<Vec<_>>>()?;
    let (top, right, bottom, left) = match values.as_slice() {
        [all] => (*all, *all, *all, *all),
        [vertical, horizontal] => (*vertical, *horizontal, *vertical, *horizontal),
        [top, horizontal, bottom] => (*top, *horizontal, *bottom, *horizontal),
        [top, right, bottom, left] => (*top, *right, *bottom, *left),
        _ => return None,
    };
    Some(Edges {
        top,
        right,
        bottom,
        left,
    })
}

/// Parses the same shorthand as [`parse_edges`] into pixel insets.
pub(crate) fn parse_insets(text: &str) -> Option<Insets> {
    let edges = parse_edges(text)?;
    let pixels = |length: Length| match length {
        Length::Px(value) => Some(value),
        _ => None,
    };
    Some(Insets {
        top: pixels(edges.top)?,
        right: pixels(edges.right)?,
        bottom: pixels(edges.bottom)?,
        left: pixels(edges.left)?,
    })
}

/// Formats pixel insets in the CSS `top right bottom left` order.
pub(crate) fn fmt_insets(insets: Insets) -> String {
    format!(
        "{} {} {} {}",
        number(insets.top),
        number(insets.right),
        number(insets.bottom),
        number(insets.left)
    )
}

/// Parses `#rrggbb` / `#rrggbbaa` (leading `#` optional); blank clears the
/// override so the theme resolves the color again.
pub(crate) fn parse_color(text: &str) -> Option<Option<Color>> {
    let text = text.trim().trim_start_matches('#');
    if text.is_empty() {
        return Some(None);
    }
    let value = u32::from_str_radix(text, 16).ok()?;
    match text.len() {
        6 => Some(Some(Color::from_hex(value))),
        8 => Some(Some(Color::from_hex_alpha(value))),
        _ => None,
    }
}

/// Formats a color override as lowercase hex, empty when unset.
pub(crate) fn fmt_color(color: Option<Color>) -> String {
    let Some(color) = color else {
        return String::new();
    };
    let srgb = color.to_srgb8();
    if srgb.a == 0xff {
        format!("#{:02x}{:02x}{:02x}", srgb.r, srgb.g, srgb.b)
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", srgb.r, srgb.g, srgb.b, srgb.a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_parse_auto_pixels_and_percentages() {
        assert_eq!(parse_length(" AUTO "), Some(Length::Auto));
        assert_eq!(parse_length("120"), Some(Length::Px(120.0)));
        assert_eq!(parse_length("16px"), Some(Length::Px(16.0)));
        assert_eq!(parse_length("50%"), Some(Length::Percent(0.5)));
        assert_eq!(parse_length("-4"), Some(Length::Px(-4.0)));
        assert_eq!(parse_length("wide"), None);
        assert_eq!(parse_length(""), None);
    }

    #[test]
    fn edge_shorthand_expands_in_css_order() {
        let edges = parse_edges("8").unwrap();
        assert_eq!(edges, Edges::all(Length::Px(8.0)));
        let edges = parse_edges("8 12").unwrap();
        assert_eq!(edges.top, Length::Px(8.0));
        assert_eq!(edges.right, Length::Px(12.0));
        assert_eq!(edges.bottom, Length::Px(8.0));
        assert_eq!(edges.left, Length::Px(12.0));
        let edges = parse_edges("1 2 3 4").unwrap();
        assert_eq!(edges.left, Length::Px(4.0));
        assert!(parse_edges("1 2 3 4 5").is_none());
        assert!(parse_edges("1 wide").is_none());
    }

    #[test]
    fn insets_reject_non_pixel_lengths() {
        assert_eq!(
            parse_insets("4 8"),
            Some(Insets {
                top: 4.0,
                right: 8.0,
                bottom: 4.0,
                left: 8.0,
            })
        );
        assert!(parse_insets("50%").is_none());
        assert!(parse_insets("auto").is_none());
    }

    #[test]
    fn colors_round_trip_through_hex() {
        assert_eq!(parse_color(""), Some(None));
        assert_eq!(parse_color("  "), Some(None));
        let color = parse_color("#4c8dff").unwrap().unwrap();
        assert_eq!(fmt_color(Some(color)), "#4c8dff");
        let translucent = parse_color("4c8dff80").unwrap().unwrap();
        assert_eq!(fmt_color(Some(translucent)), "#4c8dff80");
        assert_eq!(parse_color("#4c8d"), None);
        assert_eq!(parse_color("nope"), None);
    }
}
