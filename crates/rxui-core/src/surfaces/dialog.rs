//! Controlled modal surfaces.

use std::sync::Arc;

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::Alignment;

use crate::{
    ButtonStyle, ButtonVariant, ColorRole, ContainerStyle, FrameStyle, Space, StackStyle, View,
    ViewKey, button_with, column_with, label, row, stack_with, views,
};

/// One modal-dialog action.
#[derive(Clone)]
pub struct DialogAction<Action> {
    /// Identity for keyed reconciliation, stable across label changes.
    pub id: ViewKey,
    /// User-visible label.
    pub label: String,
    /// Typed action.
    pub action: Action,
    /// Semantic presentation.
    pub variant: ButtonVariant,
}

/// Builds a controlled modal surface over a background view.
pub fn dialog<Action: Clone + 'static>(
    open: bool,
    title: impl Into<Arc<str>>,
    background: View<Action>,
    content: View<Action>,
    on_dismiss: Action,
    actions: &[DialogAction<Action>],
) -> View<Action> {
    let modal = column_with(
        ContainerStyle::new()
            .gap(Space::Md)
            .padding(Space::Lg)
            .background(ColorRole::Surface),
        (
            label(title),
            content,
            row(views(actions.iter().map(|action| {
                button_with(
                    action.label.clone(),
                    action.action.clone(),
                    ButtonStyle::standard().variant(action.variant),
                )
                .key(action.id.clone())
            }))),
        ),
    )
    .frame(
        FrameStyle::new()
            .max(LogicalSize::new(560.0, 640.0))
            .min(LogicalSize::new(360.0, 0.0)),
    )
    .visible(open)
    .focus_scope(open)
    .dismiss_on_escape(on_dismiss)
    .aligned(Alignment::Center, Space::Xl);
    stack_with(
        StackStyle::new().background(ColorRole::Background),
        (
            background.enabled(!open).frame(FrameStyle::new().grow(1.0)),
            modal,
        ),
    )
}
