//! Structured detail sections for the selected element.

use astrelis_core::color::Color;
use astrelis_ui_core::{
    Alignment, Column, Edges, ElementHandle, ElementInspection, ElementKind, FlexStyle, Insets,
    Justification, LayoutStyle, Length, Ui, UiError, WidgetStyle,
};

use crate::{
    edit::{Map, edit_layout, edit_state, edit_style, edit_text},
    model::{RowMeta, kind_color, label_color},
};

/// Builds every detail section for one element into `parent`.
///
/// With `editors` present the state, layout, style, and text properties render
/// as commit-based editing controls instead of read-only rows.
pub(crate) fn build_details<Message: Clone + 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
    meta: Option<&RowMeta>,
    editors: Option<&Map<Message>>,
) -> Result<(), UiError> {
    element_header(ui, parent, node, meta)?;
    state_chips(ui, parent, node)?;
    if let Some(map) = editors {
        section_header(ui, parent, "State")?;
        edit_state(ui, parent, node, map)?;
    }
    section_header(ui, parent, "Box model")?;
    box_model(ui, parent, node)?;
    section_header(ui, parent, "Layout")?;
    if let Some(map) = editors {
        // The resolved padding equals the declared padding for a padding
        // container, so it seeds the editor directly.
        let padding =
            (node.kind == ElementKind::Padding).then_some(node.resolved_padding);
        edit_layout(ui, parent, node, padding, map)?;
        declared_layout(ui, parent, node, true)?;
    } else {
        declared_layout(ui, parent, node, false)?;
    }
    section_header(ui, parent, "Computed")?;
    computed(ui, parent, node)?;
    section_header(ui, parent, "Paint")?;
    kv_row(ui, parent, "z-index", node.z_index.to_string())?;
    kv_row(ui, parent, "paint rank", node.paint_rank.to_string())?;
    if editors.is_none() {
        kv_row(ui, parent, "visibility", format!("{:?}", node.visibility))?;
    }
    if let Some(map) = editors {
        section_header(ui, parent, "Style")?;
        edit_style(ui, parent, node, map)?;
        if let Some(seed) = text_seed(ui, node, meta) {
            section_header(ui, parent, "Text")?;
            edit_text(ui, parent, node, seed, map)?;
        }
    }
    if let Some(meta) = meta.filter(|meta| meta.role.is_some() || !meta.label.is_empty()) {
        section_header(ui, parent, "Semantics")?;
        if let Some(role) = meta.role {
            kv_row(ui, parent, "role", format!("{role:?}"))?;
        }
        if !meta.label.is_empty() {
            kv_row(ui, parent, "label", format!("\"{}\"", meta.label))?;
        }
    }
    Ok(())
}

/// The editable text content of a label, button, or text field.
fn text_seed<Message: 'static>(
    ui: &Ui<Message>,
    node: &ElementInspection,
    meta: Option<&RowMeta>,
) -> Option<String> {
    match node.kind {
        ElementKind::TextField => ui
            .typed_handle::<astrelis_ui_core::TextField>(node.id)
            .and_then(|handle| ui.text(handle).ok())
            .map(str::to_string),
        ElementKind::Label | ElementKind::Button => {
            Some(meta.map(|meta| meta.label.clone()).unwrap_or_default())
        }
        _ => None,
    }
}

fn element_header<Message: 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
    meta: Option<&RowMeta>,
) -> Result<(), UiError> {
    let heading = ui.theme().type_scale.heading;
    let heading_weight = ui.theme().type_scale.heading_weight;
    let caption = ui.theme().type_scale.caption;
    let muted = ui.theme().muted_foreground;
    let row = ui.add_row(parent)?;
    ui.set_flex(row, 6.0, Alignment::Center)?;
    ui.set_layout(
        row,
        LayoutStyle {
            width: Length::Percent(1.0),
            ..LayoutStyle::default()
        },
    )?;
    let kind = ui.add_label(row, format!("{:?}", node.kind))?;
    ui.set_widget_style(
        kind,
        WidgetStyle {
            foreground: Some(kind_color(node.kind)),
            font_size: Some(heading),
            font_weight: Some(heading_weight),
            ..WidgetStyle::default()
        },
    )?;
    if let Some(meta) = meta.filter(|meta| !meta.label.is_empty()) {
        let label = ui.add_label(row, format!("\"{}\"", truncated(&meta.label, 24)))?;
        ui.set_widget_style(
            label,
            WidgetStyle {
                foreground: Some(label_color()),
                ..WidgetStyle::default()
            },
        )?;
    }
    let spacer = ui.add_label(row, "")?;
    ui.set_layout(
        spacer,
        LayoutStyle {
            grow: 1.0,
            ..LayoutStyle::default()
        },
    )?;
    let bounds = node.world_bounds;
    let size = ui.add_label(
        row,
        format!("{} × {}", number(bounds.size.width), number(bounds.size.height)),
    )?;
    ui.set_widget_style(
        size,
        WidgetStyle {
            foreground: Some(muted),
            font_size: Some(caption),
            ..WidgetStyle::default()
        },
    )?;
    Ok(())
}

fn state_chips<Message: 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
) -> Result<(), UiError> {
    let caption = ui.theme().type_scale.caption;
    let overlay = ui.theme().overlay;
    let accent = ui.theme().accent;
    let disabled = ui.theme().disabled_foreground;
    let row = ui.add_row(parent)?;
    ui.set_flex_style(
        row,
        FlexStyle {
            column_gap: 4.0,
            row_gap: 4.0,
            align_items: Alignment::Center,
            wrap: astrelis_ui_core::FlexWrap::Wrap,
            ..FlexStyle::default()
        },
    )?;
    ui.set_layout(
        row,
        LayoutStyle {
            width: Length::Percent(1.0),
            ..LayoutStyle::default()
        },
    )?;
    let states = [
        ("enabled", node.enabled),
        ("visible", node.effectively_visible),
        ("interactive", node.interactive),
        ("focusable", node.focusable),
        ("focused", node.focused),
        ("hovered", node.hovered),
        ("hit-testable", node.hit_testable),
    ];
    for (name, on) in states {
        let chip = ui.add_label(row, name)?;
        ui.set_widget_style(
            chip,
            WidgetStyle {
                foreground: Some(if on { accent } else { disabled }),
                background: Some(overlay),
                font_size: Some(caption),
                ..WidgetStyle::default()
            },
        )?;
        ui.set_layout(
            chip,
            LayoutStyle {
                margin: Edges::all(Length::Px(1.0)),
                ..LayoutStyle::default()
            },
        )?;
    }
    Ok(())
}

/// Uppercase muted caption heading separating detail sections.
pub(crate) fn section_header<Message: 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    title: &str,
) -> Result<(), UiError> {
    let caption = ui.theme().type_scale.caption;
    let heading_weight = ui.theme().type_scale.heading_weight;
    let muted = ui.theme().muted_foreground;
    let spacing = ui.theme().spacing.md;
    let label = ui.add_label(parent, title.to_uppercase())?;
    ui.set_widget_style(
        label,
        WidgetStyle {
            foreground: Some(muted),
            font_size: Some(caption),
            font_weight: Some(heading_weight),
            ..WidgetStyle::default()
        },
    )?;
    ui.set_layout(
        label,
        LayoutStyle {
            margin: Edges {
                top: Length::Px(spacing),
                ..Edges::all(Length::Px(0.0))
            },
            ..LayoutStyle::default()
        },
    )?;
    Ok(())
}

/// One muted-key, plain-value detail row.
pub(crate) fn kv_row<Message: 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    key: &str,
    value: String,
) -> Result<(), UiError> {
    let caption = ui.theme().type_scale.caption;
    let muted = ui.theme().muted_foreground;
    let row = ui.add_row(parent)?;
    ui.set_flex(row, 8.0, Alignment::Start)?;
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
    let value = ui.add_label(row, value)?;
    ui.set_widget_style(
        value,
        WidgetStyle {
            font_size: Some(caption),
            ..WidgetStyle::default()
        },
    )?;
    ui.set_wrap(value, true)?;
    ui.set_layout(
        value,
        LayoutStyle {
            grow: 1.0,
            ..LayoutStyle::default()
        },
    )?;
    Ok(())
}

fn computed<Message: 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
) -> Result<(), UiError> {
    kv_row(ui, parent, "world", rect(node.world_bounds))?;
    kv_row(ui, parent, "layout", rect(node.layout_bounds))?;
    kv_row(
        ui,
        parent,
        "physical",
        format!("{} px", rect_physical(node)),
    )?;
    kv_row(
        ui,
        parent,
        "clip",
        node.clip.map_or_else(|| "none".to_string(), rect),
    )?;
    if node.world_transform != astrelis_core::math::Affine2::IDENTITY {
        kv_row(ui, parent, "transform", format!("{:?}", node.world_transform))?;
    }
    Ok(())
}

fn declared_layout<Message: 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
    skip_edited: bool,
) -> Result<(), UiError> {
    let declared = &node.declared_layout;
    let defaults = LayoutStyle::default();
    let mut rows: Vec<(&str, String)> = Vec::new();
    let mut length = |key: &'static str, value: Length, default: Length| {
        if value != default {
            rows.push((key, fmt_length(value)));
        }
    };
    if !skip_edited {
        length("width", declared.width, defaults.width);
        length("height", declared.height, defaults.height);
    }
    length("min-width", declared.min_width, defaults.min_width);
    length("min-height", declared.min_height, defaults.min_height);
    length("max-width", declared.max_width, defaults.max_width);
    length("max-height", declared.max_height, defaults.max_height);
    length("basis", declared.basis, defaults.basis);
    if !skip_edited && declared.margin != defaults.margin {
        rows.push(("margin", fmt_edges(declared.margin)));
    }
    if !skip_edited {
        if declared.grow != defaults.grow {
            rows.push(("grow", number(declared.grow)));
        }
        if declared.shrink != defaults.shrink {
            rows.push(("shrink", number(declared.shrink)));
        }
    }
    if let Some(align) = declared.align_self {
        rows.push(("align-self", format!("{align:?}")));
    }
    if declared.positioning != defaults.positioning {
        rows.push(("position", format!("{:?}", declared.positioning)));
    }
    if declared.inset != defaults.inset {
        rows.push(("inset", fmt_edges(declared.inset)));
    }
    if let Some(ratio) = declared.aspect_ratio {
        rows.push(("aspect-ratio", number(ratio)));
    }
    if rows.is_empty() && !skip_edited {
        kv_row(ui, parent, "declared", "defaults".to_string())?;
    }
    for (key, value) in rows {
        kv_row(ui, parent, key, value)?;
    }
    Ok(())
}

fn box_model<Message: 'static>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<Column>,
    node: &ElementInspection,
) -> Result<(), UiError> {
    let margin_tint = Color::from_hex_alpha(0xf3ad6b59);
    let padding_tint = Color::from_hex_alpha(0x93c76359);
    let content_tint = Color::from_hex_alpha(0x6aa9d873);

    let wrapper = ui.add_row(parent)?;
    ui.set_flex_style(
        wrapper,
        FlexStyle {
            justify_content: Justification::Center,
            align_items: Alignment::Center,
            ..FlexStyle::default()
        },
    )?;
    ui.set_layout(
        wrapper,
        LayoutStyle {
            width: Length::Percent(1.0),
            ..LayoutStyle::default()
        },
    )?;

    let margin = node.resolved_margin;
    let padding = combined(node.resolved_padding, node.resolved_border);
    let content = crate::highlight::BandSet::of(node).content.size;

    let margin_box = ui.add_padding(wrapper, Insets::all(4.0))?;
    ui.set_widget_style(
        margin_box,
        WidgetStyle {
            background: Some(margin_tint),
            ..WidgetStyle::default()
        },
    )?;
    let margin_col = ui.add_column(margin_box)?;
    ui.set_flex(margin_col, 2.0, Alignment::Center)?;
    inset_label(ui, margin_col, margin.top)?;
    let margin_mid = ui.add_row(margin_col)?;
    ui.set_flex(margin_mid, 4.0, Alignment::Center)?;
    inset_label(ui, margin_mid, margin.left)?;

    let padding_box = ui.add_padding(margin_mid, Insets::all(4.0))?;
    ui.set_widget_style(
        padding_box,
        WidgetStyle {
            background: Some(padding_tint),
            ..WidgetStyle::default()
        },
    )?;
    let padding_col = ui.add_column(padding_box)?;
    ui.set_flex(padding_col, 2.0, Alignment::Center)?;
    inset_label(ui, padding_col, padding.top)?;
    let padding_mid = ui.add_row(padding_col)?;
    ui.set_flex(padding_mid, 4.0, Alignment::Center)?;
    inset_label(ui, padding_mid, padding.left)?;

    let content_box = ui.add_padding(
        padding_mid,
        Insets {
            left: 12.0,
            top: 5.0,
            right: 12.0,
            bottom: 5.0,
        },
    )?;
    ui.set_widget_style(
        content_box,
        WidgetStyle {
            background: Some(content_tint),
            ..WidgetStyle::default()
        },
    )?;
    let caption = ui.theme().type_scale.caption;
    let size = ui.add_label(
        content_box,
        format!("{} × {}", number(content.width), number(content.height)),
    )?;
    ui.set_widget_style(
        size,
        WidgetStyle {
            font_size: Some(caption),
            ..WidgetStyle::default()
        },
    )?;

    inset_label(ui, padding_mid, padding.right)?;
    inset_label(ui, padding_col, padding.bottom)?;
    inset_label(ui, margin_mid, margin.right)?;
    inset_label(ui, margin_col, margin.bottom)?;
    Ok(())
}

fn inset_label<Message: 'static, T>(
    ui: &mut Ui<Message>,
    parent: ElementHandle<T>,
    value: f32,
) -> Result<(), UiError> {
    let caption = ui.theme().type_scale.caption;
    let label = ui.add_label(
        parent,
        if value == 0.0 {
            "–".to_string()
        } else {
            number(value)
        },
    )?;
    ui.set_widget_style(
        label,
        WidgetStyle {
            font_size: Some(caption),
            ..WidgetStyle::default()
        },
    )?;
    Ok(())
}

fn combined(a: Insets, b: Insets) -> Insets {
    Insets {
        left: a.left + b.left,
        top: a.top + b.top,
        right: a.right + b.right,
        bottom: a.bottom + b.bottom,
    }
}

/// Formats a float without a trailing `.0` and with one decimal otherwise.
pub(crate) fn number(value: f32) -> String {
    if (value - value.round()).abs() < 0.05 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

fn rect(rect: astrelis_core::geometry::LogicalRect) -> String {
    format!(
        "({}, {}) {} × {}",
        number(rect.origin.x),
        number(rect.origin.y),
        number(rect.size.width),
        number(rect.size.height)
    )
}

fn rect_physical(node: &ElementInspection) -> String {
    let rect = node.physical_bounds;
    format!(
        "({}, {}) {} × {}",
        number(rect.origin.x),
        number(rect.origin.y),
        number(rect.size.width),
        number(rect.size.height)
    )
}

pub(crate) fn fmt_length(length: Length) -> String {
    match length {
        Length::Auto => "auto".to_string(),
        Length::Px(value) => format!("{}px", number(value)),
        Length::Percent(value) => format!("{}%", number(value * 100.0)),
    }
}

pub(crate) fn fmt_edges(edges: Edges<Length>) -> String {
    format!(
        "{} {} {} {}",
        fmt_length(edges.top),
        fmt_length(edges.right),
        fmt_length(edges.bottom),
        fmt_length(edges.left)
    )
}

fn truncated(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut = text.chars().take(max.saturating_sub(1)).collect::<String>();
        format!("{cut}…")
    }
}
