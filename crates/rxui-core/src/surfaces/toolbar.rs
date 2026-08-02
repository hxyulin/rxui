//! Keyed command toolbars.

use astrelis_core::geometry::LogicalSize;

use crate::{
    ButtonStyle, ButtonVariant, ColorRole, ContainerStyle, Icon, IconButtonStyle, Space, View,
    ViewKey, button_with, icon_button_with, panel, row_with, spacer, views,
};

/// One toolbar entry.
#[derive(Clone)]
pub enum ToolbarItem<Action> {
    /// Activatable command.
    Command {
        /// Stable command identity, independent of toolbar position.
        id: ViewKey,
        /// User-visible label.
        label: String,
        /// Typed action.
        action: Action,
        /// Whether the command accepts interaction.
        enabled: bool,
        /// Semantic presentation.
        variant: ButtonVariant,
    },
    /// Vector-icon command with compact or labeled presentation.
    IconCommand {
        /// Stable command identity, independent of toolbar position.
        id: ViewKey,
        /// Monochrome vector glyph.
        icon: Icon,
        /// Accessible label, optionally also painted.
        label: String,
        /// Typed action.
        action: Action,
        /// Whether the command accepts interaction.
        enabled: bool,
        /// Icon-button presentation.
        style: IconButtonStyle,
    },
    /// Visual separator.
    Separator,
    /// Fixed logical spacing.
    Space(f32),
}

/// Builds a keyed command toolbar.
pub fn toolbar<Action: Clone + 'static>(items: &[ToolbarItem<Action>]) -> View<Action> {
    // Separators and spacers are interchangeable and retain no interaction
    // state, so numbering them among themselves is a canonical identity: a
    // command inserted anywhere still leaves every decoration key alone.
    let mut decorations = 0usize;
    let keys = items
        .iter()
        .map(|item| match item {
            ToolbarItem::Command { id, .. } | ToolbarItem::IconCommand { id, .. } => {
                ViewKey::new(format!("command-{id}"))
            }
            ToolbarItem::Separator | ToolbarItem::Space(_) => {
                decorations += 1;
                ViewKey::new(format!("decoration-{}", decorations - 1))
            }
        })
        .collect::<Vec<_>>();
    row_with(
        ContainerStyle::new()
            .gap(Space::Xs)
            .padding(Space::Xs)
            .background(ColorRole::Surface),
        views(items.iter().zip(keys).map(|(item, key)| {
            let view = match item {
                ToolbarItem::Command {
                    label,
                    action,
                    enabled,
                    variant,
                    ..
                } => button_with(
                    label.clone(),
                    action.clone(),
                    ButtonStyle::standard().variant(*variant),
                )
                .enabled(*enabled),
                ToolbarItem::IconCommand {
                    icon,
                    label,
                    action,
                    enabled,
                    style,
                    ..
                } => icon_button_with(icon.clone(), label.clone(), action.clone(), *style)
                    .enabled(*enabled),
                ToolbarItem::Separator => panel(LogicalSize::new(1.0, 24.0), ColorRole::Muted),
                ToolbarItem::Space(width) => spacer(LogicalSize::new((*width).max(0.0), 1.0)),
            };
            view.key(key)
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Component, ComponentContext, ComponentHost, Theme,
        semantic_probe::{focused, node_labeled},
    };

    struct Toolbar {
        commands: Vec<&'static str>,
    }

    impl Component for Toolbar {
        type Action = &'static str;
        type Effect = ();

        fn update(&mut self, _action: Self::Action, _context: &mut ComponentContext<'_, ()>) {}

        fn view(&self, _theme: &Theme) -> View<Self::Action> {
            let items = self
                .commands
                .iter()
                .flat_map(|label| {
                    [
                        ToolbarItem::Command {
                            id: (*label).into(),
                            label: (*label).into(),
                            action: *label,
                            enabled: true,
                            variant: ButtonVariant::Standard,
                        },
                        ToolbarItem::Separator,
                    ]
                })
                .collect::<Vec<_>>();
            toolbar(&items)
        }
    }

    #[test]
    fn toolbar_commands_keep_their_retained_identity_across_an_insertion() {
        let mut host = ComponentHost::new(
            Toolbar {
                commands: vec!["Save", "Close"],
            },
            LogicalSize::new(480.0, 120.0),
            Theme::dark(),
        )
        .unwrap();
        let save = node_labeled(&host, "Save");
        host.ui_mut().set_focus(Some(save)).unwrap();
        host.refresh().unwrap();

        host.component_mut().commands.insert(0, "Open");
        host.refresh().unwrap();

        let focused = focused(&host);
        assert_eq!(focused.id, save);
        assert_eq!(focused.data.label, "Save");
    }
}
