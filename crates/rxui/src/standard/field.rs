//! Form fields: a label, a control, and help or error text.
use super::Tone;
use crate::element::ElementKind;
use crate::{
    AccessibilityHandle, Element, FontWeight, Icon, IntoElement, TextRole, ThemeColor, column,
    label, row,
};
use taffy::prelude::AlignItems;

/// A labelled control with optional help or error text; see [`field`].
#[must_use = "add the field to a form"]
pub struct Field {
    root: Element,
    label: String,
    control: Element,
    help: Option<String>,
    error: Option<String>,
    inline: Option<f32>,
}

/// Stacks a medium-weight label, `control`, and help or error text.
///
/// The field names its control: it points the control's
/// `accessibility_labelled_by` at the label and publishes the help or error
/// text as the control's accessible description. The control is the first
/// text input or button in `control`, depth first, so the field finds the input
/// of a [`crate::number_field`] or [`crate::combobox`] and the trigger of a
/// [`crate::select`].
///
/// An error replaces the help text. It shows in the [`Tone::Danger`] color
/// beside a small [`Icon::CIRCLE_ALERT`], and marks the control with
/// [`Element::accessibility_invalid`], which draws its border in
/// [`ThemeColor::Danger`].
///
/// ```
/// use rxui::prelude::*;
/// struct Settings { font: String, error: Option<String> }
/// impl View for Settings {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         field("Font family", text_input(&self.font)
///                 .on_change(cx.listener(|s, e: &TextChangeEvent, _| s.font = e.value.clone())))
///             .help("A monospaced family installed on this system.")
///             .error(self.error.as_deref())
///     }
/// }
/// ```
pub fn field(label: impl Into<String>, control: impl IntoElement) -> Field {
    Field {
        root: column().gap(4.),
        label: label.into(),
        control: control.into_element(),
        help: None,
        error: None,
        inline: None,
    }
}

impl Field {
    /// Muted caption text below the control, such as the accepted range.
    pub fn help(mut self, text: impl Into<String>) -> Self {
        self.help = Some(text.into());
        self
    }
    /// Shows `error` instead of the help text and marks the control invalid.
    /// `None` clears it, so a validation result can be passed directly.
    pub fn error(mut self, error: Option<impl Into<String>>) -> Self {
        self.error = error.map(Into::into);
        self
    }
    /// Puts the label to the left of the control, `label_width` units wide, for
    /// settings pages whose controls line up in one column.
    pub fn inline(mut self, label_width: f32) -> Self {
        self.inline = Some(label_width);
        self
    }
    /// Stable identity within the parent's sibling scope.
    pub fn key(mut self, key: impl Into<crate::Key>) -> Self {
        self.root.key = Some(key.into());
        self
    }
    /// Fixed logical width.
    pub fn width(mut self, width: f32) -> Self {
        self.root = self.root.width(width);
        self
    }
    /// Fills the parent's content width.
    pub fn fill_width(mut self) -> Self {
        self.root = self.root.fill_width();
        self
    }
}

/// The first text input or button in `element`, depth first, outside overlays.
fn control_of(element: &mut Element) -> Option<&mut Element> {
    if matches!(
        element.kind,
        ElementKind::TextInput { .. } | ElementKind::Button { .. }
    ) {
        return Some(element);
    }
    element
        .children
        .iter_mut()
        .filter(|c| c.input.as_ref().is_none_or(|p| p.overlay.is_none()))
        .find_map(control_of)
}

impl IntoElement for Field {
    fn into_element(self) -> Element {
        let handle = AccessibilityHandle::new();
        let caption = label(self.label)
            .font_weight(FontWeight::MEDIUM)
            .accessibility_handle(handle.clone());
        let invalid = self.error.is_some();
        let message = self.error.or(self.help);
        let mut control = self.control;
        if let Some(target) = control_of(&mut control) {
            let semantics = target.semantics.get_or_insert_with(Default::default);
            semantics.labelled_by = Some(handle);
            if let Some(message) = &message {
                semantics.description = Some(message.as_str().into());
            }
            semantics.invalid = invalid;
        }
        let message = message.map(|text| {
            let text = label(text).text_role(TextRole::Caption);
            if invalid {
                let color = Tone::Danger.foreground();
                row()
                    .key("error")
                    .gap(4.)
                    .align_items(AlignItems::CENTER)
                    .child(crate::icon(Icon::CIRCLE_ALERT).size(12., 12.).color(color))
                    .child(text.color(color))
            } else {
                text.key("help").color(ThemeColor::TextMuted)
            }
        });
        let mut root = self.root;
        let Some(width) = self.inline else {
            return root.child(caption).child(control).children(message);
        };
        root = root.child(
            row()
                .gap(12.)
                .align_items(AlignItems::CENTER)
                .child(caption.width(width).flex_shrink(0.))
                .child(control),
        );
        root.children(
            message.map(|m| m.layout(|s| s.margin.left = taffy::prelude::length(width + 12.))),
        )
    }
}
