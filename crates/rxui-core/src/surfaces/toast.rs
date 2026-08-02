//! Controlled toast notifications.

use astrelis_core::geometry::LogicalSize;
use astrelis_ui_next::Alignment;

use crate::{
    ColorRole, ContainerStyle, FrameStyle, Space, View, button, column_with, label, views,
};

/// Toast urgency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastLevel {
    /// Informational status.
    #[default]
    Info,
    /// Successful operation.
    Success,
    /// Recoverable warning.
    Warning,
    /// Failed operation.
    Error,
}

/// One controlled toast notification.
#[derive(Clone)]
pub struct Toast<Action> {
    /// Stable identity.
    pub id: u64,
    /// User-visible message.
    pub message: String,
    /// Urgency.
    pub level: ToastLevel,
    /// Optional action label and payload.
    pub action: Option<(String, Action)>,
}

/// Builds keyed toast surfaces.
pub fn toasts<Action: Clone + 'static>(items: &[Toast<Action>]) -> View<Action> {
    column_with(
        ContainerStyle::new().gap(Space::Sm).padding(Space::Sm),
        views(items.iter().map(|toast| {
            let role = match toast.level {
                ToastLevel::Info | ToastLevel::Success => ColorRole::Surface,
                ToastLevel::Warning => ColorRole::Accent,
                ToastLevel::Error => ColorRole::Danger,
            };
            let action = toast
                .action
                .as_ref()
                .map(|(label, action)| button(label.clone(), action.clone()));
            let children = std::iter::once(label(toast.message.clone()))
                .chain(action)
                .collect::<Vec<_>>();
            column_with(
                ContainerStyle::new()
                    .gap(Space::Xs)
                    .padding(Space::Md)
                    .background(role),
                children,
            )
            .key(toast.id)
        })),
    )
    .frame(FrameStyle::new().max(LogicalSize::new(360.0, 640.0)))
    .aligned(Alignment::TopTrailing, Space::Md)
}
