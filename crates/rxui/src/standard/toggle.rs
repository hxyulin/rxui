//! Controlled check boxes and switches.
use super::glyph::{Glyph, mark_glyph};
use super::{Part, ToggleEvent, caption, indicator_colors, part, quiet_root, root_builders};
use crate::{
    Checked, ClickEvent, Element, IntoElement, Listener, SemanticRole, button, row, stack,
};
use taffy::prelude::{AlignItems, JustifyContent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    CheckBox,
    Switch,
}

/// Controlled check box or switch builder. Both are buttons: pointer clicks,
/// Enter/Space and assistive activation all propose the opposite state through
/// [`Toggle::on_change`]. The caption names the control; a control without a
/// caption needs [`Toggle::accessibility_label`].
#[must_use = "attach the control to a parent or return it from View::view"]
pub struct Toggle {
    root: Element,
    kind: Kind,
    checked: Checked,
    label: String,
    disabled: bool,
    change: Option<Listener<ToggleEvent>>,
}

/// Check box with an optional caption. `Checked::Mixed` paints a bar and is
/// published as mixed; activating it proposes `true`.
///
/// ```
/// use rxui::prelude::*;
/// struct Prefs { autosave: bool }
/// impl View for Prefs {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         checkbox(self.autosave, "Save automatically").key("autosave")
///             .on_change(cx.listener(|s, e: &ToggleEvent, _| s.autosave = e.checked))
///     }
/// }
/// ```
pub fn checkbox(checked: impl Into<Checked>, label: impl Into<String>) -> Toggle {
    Toggle::new(Kind::CheckBox, checked.into(), label.into())
}

/// On/off switch with an optional caption, published with the Switch role.
pub fn switch(on: bool, label: impl Into<String>) -> Toggle {
    Toggle::new(Kind::Switch, on.into(), label.into())
}

impl Toggle {
    fn new(kind: Kind, checked: Checked, label: String) -> Self {
        Self {
            root: quiet_root(button(row())).padding_x(4.),
            kind,
            checked,
            label,
            disabled: false,
            change: None,
        }
    }
    /// Receives the proposed next state. Without a listener the control still
    /// focuses and activates but cannot change.
    pub fn on_change(mut self, listener: Listener<ToggleEvent>) -> Self {
        self.change = Some(listener);
        self
    }
    /// Disables activation and removes the control from Tab traversal.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
root_builders!(Toggle);

impl IntoElement for Toggle {
    fn into_element(self) -> Element {
        let on = self.checked != Checked::False;
        let [fill, border, mark] = indicator_colors(on, self.disabled);
        let indicator = match self.kind {
            Kind::CheckBox => {
                let mut indicator = part(stack(), Part::CheckBox)
                    .background(fill)
                    .border(1., border)
                    .align_items(AlignItems::CENTER)
                    .justify_items(AlignItems::CENTER);
                if on {
                    let kind = if self.checked == Checked::Mixed {
                        Glyph::Dash
                    } else {
                        Glyph::Check
                    };
                    indicator =
                        indicator.child(part(mark_glyph(kind), Part::Mark).key("mark").color(mark));
                }
                indicator
            }
            Kind::Switch => part(row(), Part::Switch)
                .background(fill)
                .border(1., border)
                .align_items(AlignItems::CENTER)
                .justify_content(if on {
                    JustifyContent::END
                } else {
                    JustifyContent::START
                })
                .child(part(row(), Part::Knob).key("knob").background(mark)),
        };
        let mut content = row()
            .gap(8.)
            .align_items(AlignItems::CENTER)
            .child(indicator.key("indicator").accessibility_hidden(true));
        if let Some(text) = caption(&self.label) {
            content = content.child(text.key("caption"));
        }
        let next = ToggleEvent {
            checked: self.checked != Checked::True,
        };
        let mut root = self.root;
        root.children = vec![content];
        root = root
            .disabled(self.disabled)
            .accessibility_role(match self.kind {
                Kind::CheckBox => SemanticRole::CheckBox,
                Kind::Switch => SemanticRole::Switch,
            })
            .accessibility_checked(self.checked);
        if let Some(listener) = self.change {
            root = root.on_click(listener.map_event(move |_: &ClickEvent| next));
        }
        root
    }
}
