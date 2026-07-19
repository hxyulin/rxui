//! Command toolbars, modal dialogs, and in-application toast notifications.

use std::{
    cell::Cell,
    error::Error,
    fmt,
    rc::Rc,
    time::{Duration, Instant},
};

use astrelis_core::geometry::{LogicalRect, LogicalSize, Point, Size};
use astrelis_paint::{Brush, Painter, Path, StrokeStyle};
use astrelis_platform::{ElementState, Key, NamedKey};
use astrelis_ui::widget_any;
use astrelis_ui_core::{
    Button, Column, ElementHandle, EventFilter, FocusScopeOptions, Insets, LayoutStyle, Length,
    Overlay, OverlayAlignment, OverlayOptions, OverlaySide, RoutedEventKind, SemanticLive,
    SemanticRole, Theme, Ui, UiError, Visibility, Widget, WidgetContainerStyle, WidgetStyle,
};
use astrelis_ui_widgets::{Menu, MenuItem, Tooltip};
use astreon_app::{CommandId, CommandRegistry};

use crate::{CommandButton, Icon};

/// One command-toolbar entry.
#[derive(Clone)]
pub enum ToolbarItem {
    /// Registered command button.
    Command {
        /// Command registry identity.
        id: CommandId,
        /// Optional vector icon.
        icon: Option<Icon>,
    },
    /// Visual separator between command groups.
    Separator,
    /// Space that expands between command groups.
    FlexibleSpace,
}

/// Toolbar presentation policy.
#[derive(Clone, Copy, Debug)]
pub struct ToolbarOptions {
    /// Show command labels beside icons.
    pub show_labels: bool,
}
impl Default for ToolbarOptions {
    fn default() -> Self {
        Self { show_labels: true }
    }
}

/// Responsive command-driven toolbar.
pub struct Toolbar<Message> {
    root: ElementHandle<astrelis_ui_core::Row>,
    buttons: Vec<(CommandId, ElementHandle<CommandButton<Message>>, f32)>,
    overflow_owner: ElementHandle<Button>,
    overflow: Menu,
    slots: Vec<ToolbarSlot>,
    overflow_start: Rc<Cell<usize>>,
}

enum ToolbarSlot {
    Command(usize),
    Separator(ElementHandle<ToolbarSeparator>),
    Flexible,
}

/// Estimated width of a labeled toolbar command before its per-character text.
const LABELED_COMMAND_BASE_WIDTH: f32 = 36.0;
/// Estimated width contributed by each label character.
const LABELED_COMMAND_CHAR_WIDTH: f32 = 7.0;
/// Estimated width of an icon-only toolbar command.
const ICON_COMMAND_WIDTH: f32 = 32.0;
/// Width reserved for the "More" overflow button when commands overflow.
const OVERFLOW_RESERVE_WIDTH: f32 = 44.0;

struct ToolbarSeparator;

impl<Message: 'static> Widget<Message> for ToolbarSeparator {
    widget_any!();

    fn intrinsic_size(&self, _theme: &Theme) -> LogicalSize {
        Size::new(1.0, 20.0)
    }

    fn container_style(&self, _theme: &Theme) -> WidgetContainerStyle {
        WidgetContainerStyle::structural()
    }

    fn paint(
        &self,
        painter: &mut Painter,
        bounds: LogicalRect,
        theme: &Theme,
    ) -> Result<(), UiError> {
        let x = bounds.origin.x + bounds.size.width * 0.5;
        let mut builder = Path::builder();
        builder
            .move_to(Point::new(x, bounds.origin.y))
            .map_err(|error| UiError::from_message(error.to_string()))?;
        builder
            .line_to(Point::new(x, bounds.max_y()))
            .map_err(|error| UiError::from_message(error.to_string()))?;
        painter
            .stroke_path(
                &builder.finish(),
                StrokeStyle {
                    width: theme.border_width.max(1.0),
                    ..Default::default()
                },
                Brush::Solid(theme.border),
            )
            .map_err(|error| UiError::from_message(error.to_string()))
    }

    fn semantics(&self) -> Option<(SemanticRole, String, Option<String>)> {
        Some((SemanticRole::Separator, String::new(), None))
    }
}

impl<Message: Clone + 'static> Toolbar<Message> {
    /// Builds a toolbar from a command snapshot.
    pub fn new<T>(
        ui: &mut Ui<Message>,
        parent: ElementHandle<T>,
        items: Vec<ToolbarItem>,
        commands: &CommandRegistry<Message>,
        options: ToolbarOptions,
    ) -> Result<Self, UiError> {
        let root = ui.add_row(parent)?;
        ui.set_semantic_role(root, SemanticRole::Toolbar)?;
        let mut buttons = Vec::new();
        let mut overflow_entries = Vec::new();
        let mut slots = Vec::new();
        for item in items {
            match item {
                ToolbarItem::Command { id, icon } => {
                    let command = commands.get(&id).ok_or_else(|| {
                        UiError::from_message(format!("toolbar command `{id}` is not registered"))
                    })?;
                    let button = ui.add_widget(
                        root,
                        CommandButton::new(command.label.clone(), command.message.clone())
                            .icon(icon)
                            .show_label(options.show_labels)
                            .checked(command.checked == Some(true))
                            .enabled(command.enabled),
                    )?;
                    ui.set_enabled(button, command.enabled)?;
                    if let Some(label) = ui.widget(button)?.label_handle() {
                        ui.set_enabled(label, command.enabled)?;
                        let checked_foreground = (command.checked == Some(true) && command.enabled)
                            .then(|| ui.theme().accent_foreground);
                        ui.set_widget_style(
                            label,
                            WidgetStyle {
                                foreground: checked_foreground,
                                ..Default::default()
                            },
                        )?;
                    }
                    let tip = command.shortcut.as_ref().map_or_else(
                        || command.label.clone(),
                        |key| format!("{} ({})", command.label, key.display_label()),
                    );
                    if !options.show_labels {
                        Tooltip::new(ui, button, tip)?;
                    }
                    let estimated = if options.show_labels {
                        LABELED_COMMAND_BASE_WIDTH
                            + command.label.chars().count() as f32 * LABELED_COMMAND_CHAR_WIDTH
                    } else {
                        ICON_COMMAND_WIDTH
                    };
                    buttons.push((id, button, estimated));
                    slots.push(ToolbarSlot::Command(buttons.len() - 1));
                    overflow_entries.push(MenuItem {
                        label: command.label.clone(),
                        message: command.message.clone(),
                        enabled: command.enabled,
                    });
                }
                ToolbarItem::Separator => {
                    let separator = ui.add_widget(root, ToolbarSeparator)?;
                    ui.set_semantic_role(separator, SemanticRole::Separator)?;
                    slots.push(ToolbarSlot::Separator(separator));
                }
                ToolbarItem::FlexibleSpace => {
                    let spacer = ui.add_label(root, "")?;
                    ui.set_layout(
                        spacer,
                        LayoutStyle {
                            grow: 1.0,
                            ..Default::default()
                        },
                    )?;
                    slots.push(ToolbarSlot::Flexible);
                }
            }
        }
        install_horizontal_navigation(
            ui,
            &buttons
                .iter()
                .map(|(_, button, _)| *button)
                .collect::<Vec<_>>(),
        )?;
        let overflow_owner = ui.add_button(root, "More")?;
        let overflow = Menu::new(ui, overflow_owner, overflow_entries)?;
        let overflow_start = Rc::new(Cell::new(buttons.len()));
        let start_for_open = overflow_start.clone();
        let overflow_buttons = overflow.items().to_vec();
        ui.listen(
            overflow_owner,
            None,
            EventFilter::Activate,
            move |context, _| {
                if let Some(first) = overflow_buttons.get(start_for_open.get()) {
                    context.request_focus_for(*first);
                }
            },
        )?;
        for (index, item) in overflow.items().iter().copied().enumerate() {
            let all = overflow.items().to_vec();
            let start = overflow_start.clone();
            ui.listen(item, None, EventFilter::Keyboard, move |context, event| {
                let RoutedEventKind::Keyboard(input) = &event.kind else {
                    return;
                };
                if input.state != ElementState::Pressed {
                    return;
                }
                let Key::Named(NamedKey::Other(key)) = &input.logical_key else {
                    return;
                };
                let first = start.get();
                let count = all.len().saturating_sub(first);
                if count == 0 {
                    return;
                }
                let next = match key.as_str() {
                    "ArrowDown" | "ArrowRight" => first + (index + 1 - first) % count,
                    "ArrowUp" | "ArrowLeft" => first + (index + count - 1 - first) % count,
                    "Home" => first,
                    "End" => all.len() - 1,
                    _ => return,
                };
                context.request_focus_for(all[next]);
                context.prevent_default();
            })?;
        }
        ui.set_visibility(overflow_owner, Visibility::Hidden)?;
        for item in overflow.items() {
            ui.set_visibility(*item, Visibility::Hidden)?;
        }
        Ok(Self {
            root,
            buttons,
            overflow_owner,
            overflow,
            slots,
            overflow_start,
        })
    }
    /// Synchronizes labels and enablement from the live registry.
    pub fn sync(
        &self,
        ui: &mut Ui<Message>,
        commands: &CommandRegistry<Message>,
    ) -> Result<(), UiError> {
        for (index, (id, button, _)) in self.buttons.iter().enumerate() {
            if let Some(command) = commands.get(id) {
                ui.update_widget(*button, |button| {
                    button.sync(
                        command.label.clone(),
                        command.enabled,
                        command.checked == Some(true),
                    );
                })?;
                if let Some(label) = ui.widget(*button)?.label_handle() {
                    ui.set_label_text(label, &command.label)?;
                    ui.set_enabled(label, command.enabled)?;
                    let checked_foreground = (command.checked == Some(true) && command.enabled)
                        .then(|| ui.theme().accent_foreground);
                    ui.set_widget_style(
                        label,
                        WidgetStyle {
                            foreground: checked_foreground,
                            ..Default::default()
                        },
                    )?;
                }
                ui.set_enabled(*button, command.enabled)?;
                ui.set_button_text(self.overflow.items()[index], toolbar_label(command))?;
                ui.set_enabled(self.overflow.items()[index], command.enabled)?;
            }
        }
        Ok(())
    }
    /// Recomputes trailing overflow for an available logical width.
    pub fn update_overflow(
        &self,
        ui: &mut Ui<Message>,
        available_width: f32,
    ) -> Result<(), UiError> {
        let required = self.buttons.iter().map(|(_, _, width)| *width).sum::<f32>();
        let mut remaining = required;
        let mut overflowed = vec![false; self.buttons.len()];
        for index in (0..self.buttons.len()).rev() {
            if remaining + OVERFLOW_RESERVE_WIDTH <= available_width {
                break;
            }
            overflowed[index] = true;
            remaining -= self.buttons[index].2;
        }
        let any = overflowed.iter().any(|value| *value);
        self.overflow_start.set(
            overflowed
                .iter()
                .position(|value| *value)
                .unwrap_or(self.buttons.len()),
        );
        ui.set_visibility(
            self.overflow_owner,
            if any {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        )?;
        for (index, (_, button, _)) in self.buttons.iter().enumerate() {
            ui.set_visibility(
                *button,
                if overflowed[index] {
                    Visibility::Hidden
                } else {
                    Visibility::Visible
                },
            )?;
            ui.set_visibility(
                self.overflow.items()[index],
                if overflowed[index] {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                },
            )?;
        }
        for (slot_index, slot) in self.slots.iter().enumerate() {
            let ToolbarSlot::Separator(separator) = slot else {
                continue;
            };
            let before = self.slots[..slot_index]
                .iter()
                .rev()
                .find_map(|slot| match slot {
                    ToolbarSlot::Command(index) => Some(!overflowed[*index]),
                    ToolbarSlot::Separator(_) => None,
                    ToolbarSlot::Flexible => None,
                })
                .unwrap_or(false);
            let after = self.slots[slot_index + 1..]
                .iter()
                .find_map(|slot| match slot {
                    ToolbarSlot::Command(index) => Some(!overflowed[*index]),
                    ToolbarSlot::Separator(_) => None,
                    ToolbarSlot::Flexible => None,
                })
                .unwrap_or(false);
            ui.set_visibility(
                *separator,
                if before && after {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                },
            )?;
        }
        Ok(())
    }
    /// Returns the toolbar root.
    pub const fn root(&self) -> ElementHandle<astrelis_ui_core::Row> {
        self.root
    }
}

fn toolbar_label<Message>(command: &astreon_app::Command<Message>) -> String {
    if command.checked == Some(true) {
        format!("[x] {}", command.label)
    } else {
        command.label.clone()
    }
}

fn install_horizontal_navigation<Message: Clone + 'static>(
    ui: &mut Ui<Message>,
    buttons: &[ElementHandle<CommandButton<Message>>],
) -> Result<(), UiError> {
    for (index, button) in buttons.iter().copied().enumerate() {
        let all = buttons.to_vec();
        ui.listen(
            button,
            None,
            EventFilter::Keyboard,
            move |context, event| {
                let RoutedEventKind::Keyboard(input) = &event.kind else {
                    return;
                };
                if input.state != ElementState::Pressed {
                    return;
                }
                let Key::Named(NamedKey::Other(key)) = &input.logical_key else {
                    return;
                };
                let next = match key.as_str() {
                    "ArrowRight" => (index + 1) % all.len(),
                    "ArrowLeft" => (index + all.len() - 1) % all.len(),
                    "Home" => 0,
                    "End" => all.len() - 1,
                    _ => return,
                };
                context.request_focus_for(all[next]);
                context.prevent_default();
            },
        )?;
    }
    Ok(())
}

/// Dialog action visual and keyboard role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogActionRole {
    /// Default affirmative action.
    Primary,
    /// Ordinary non-default action.
    Secondary,
    /// Escape-key cancellation action.
    Cancel,
    /// Irreversible or dangerous action.
    Destructive,
}
/// One typed dialog action.
pub struct DialogAction<Message> {
    /// Button label.
    pub label: String,
    /// Typed message emitted by activation.
    pub message: Message,
    /// Presentation and keyboard role.
    pub role: DialogActionRole,
    /// Whether the action accepts activation.
    pub enabled: bool,
}
/// Modal dialog presentation.
#[derive(Clone, Debug)]
pub struct DialogOptions {
    /// Dialog title.
    pub title: String,
    /// Optional explanatory text and semantic description.
    pub description: Option<String>,
}
/// Modal host failure.
#[derive(Debug)]
pub enum DialogError {
    /// The host already contains an active modal.
    AlreadyOpen,
    /// Retained-tree operation failed.
    Ui(UiError),
}
impl fmt::Display for DialogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyOpen => f.write_str("a dialog is already open"),
            Self::Ui(error) => error.fmt(f),
        }
    }
}
impl Error for DialogError {}
impl From<UiError> for DialogError {
    fn from(value: UiError) -> Self {
        Self::Ui(value)
    }
}

/// Single-modal retained dialog controller.
#[derive(Default)]
pub struct DialogHost {
    overlay: Option<ElementHandle<Overlay>>,
}
impl DialogHost {
    /// Creates an empty dialog host.
    pub const fn new() -> Self {
        Self { overlay: None }
    }
    /// Opens a dialog and lets the caller populate its retained content column.
    pub fn show<Message: Clone + 'static, F>(
        &mut self,
        ui: &mut Ui<Message>,
        options: DialogOptions,
        actions: Vec<DialogAction<Message>>,
        build: F,
    ) -> Result<(), DialogError>
    where
        F: FnOnce(&mut Ui<Message>, ElementHandle<Column>) -> Result<(), UiError>,
    {
        if self.overlay.is_some() {
            return Err(DialogError::AlreadyOpen);
        }
        let overlay = ui.add_overlay(
            ui.root(),
            OverlayOptions {
                side: OverlaySide::Center,
                alignment: OverlayAlignment::Center,
                z_index: 1000,
                focus: FocusScopeOptions {
                    trapped: true,
                    autofocus: true,
                    restore_focus: true,
                },
                ..Default::default()
            },
        )?;
        ui.set_semantic_role(overlay, SemanticRole::Dialog)?;
        ui.set_semantic_description(overlay, options.description.clone())?;
        ui.set_layout(
            overlay,
            LayoutStyle {
                min_width: Length::Px(320.0),
                max_width: Length::Px(560.0),
                ..Default::default()
            },
        )?;
        let padding = ui.add_padding(overlay, Insets::all(ui.theme().spacing.lg))?;
        let column = ui.add_column(padding)?;
        let title = ui.add_label(column, options.title)?;
        ui.set_widget_style(
            title,
            WidgetStyle {
                font_size: Some(ui.theme().type_scale.heading),
                font_weight: Some(ui.theme().type_scale.heading_weight),
                ..Default::default()
            },
        )?;
        if let Some(description) = options.description {
            let description = ui.add_label(column, description)?;
            ui.set_widget_style(
                description,
                WidgetStyle {
                    foreground: Some(ui.theme().muted_foreground),
                    ..Default::default()
                },
            )?;
        }
        let content = ui.add_column(column)?;
        build(ui, content)?;
        let buttons = ui.add_row(column)?;
        let mut default = None;
        let mut cancel = None;
        for action in actions {
            let button = ui.add_button(buttons, action.label)?;
            ui.set_enabled(button, action.enabled)?;
            if action.role == DialogActionRole::Destructive {
                ui.set_widget_style(
                    button,
                    WidgetStyle {
                        foreground: Some(ui.theme().danger),
                        ..Default::default()
                    },
                )?;
            }
            let message = action.message.clone();
            ui.listen(button, None, EventFilter::Activate, move |context, _| {
                context.emit(message.clone())
            })?;
            if action.role == DialogActionRole::Primary && action.enabled {
                default = Some((button, action.message.clone()));
            }
            if action.role == DialogActionRole::Cancel && action.enabled {
                cancel = Some(action.message);
            }
        }
        if let Some((button, _)) = &default {
            ui.focus(*button)?;
        }
        ui.listen(
            overlay,
            None,
            EventFilter::Keyboard,
            move |context, event| {
                let RoutedEventKind::Keyboard(input) = &event.kind else {
                    return;
                };
                if input.state != ElementState::Pressed {
                    return;
                }
                match &input.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        if let Some((_, message)) = &default {
                            context.emit(message.clone());
                            context.prevent_default();
                        }
                    }
                    Key::Named(NamedKey::Escape) => {
                        if let Some(message) = &cancel {
                            context.emit(message.clone());
                            context.prevent_default();
                        }
                    }
                    _ => {}
                }
            },
        )?;
        self.overlay = Some(overlay);
        Ok(())
    }
    /// Closes the active dialog and restores previous focus.
    pub fn close<Message: 'static>(&mut self, ui: &mut Ui<Message>) -> Result<bool, UiError> {
        let Some(overlay) = self.overlay.take() else {
            return Ok(false);
        };
        ui.remove(overlay)?;
        Ok(true)
    }
    /// Returns whether a dialog is active.
    pub const fn is_open(&self) -> bool {
        self.overlay.is_some()
    }
}

/// Toast severity and presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    /// Neutral information.
    Info,
    /// Successful operation.
    Success,
    /// Non-fatal warning.
    Warning,
    /// Failure requiring attention.
    Error,
}
/// Optional actionable toast command.
#[derive(Clone, Debug)]
pub struct ToastAction<Message> {
    /// Action button label.
    pub label: String,
    /// Typed action message.
    pub message: Message,
}
/// One queued notification.
#[derive(Clone, Debug)]
pub struct Toast<Message> {
    /// Concise notification heading.
    pub title: String,
    /// Supporting notification text.
    pub body: String,
    /// Severity and announcement policy.
    pub level: ToastLevel,
    /// Optional typed action.
    pub action: Option<ToastAction<Message>>,
    /// Visible lifetime; `None` remains until dismissed.
    pub duration: Option<Duration>,
}
impl<Message> Toast<Message> {
    /// Creates a toast with severity-default duration.
    pub fn new(level: ToastLevel, title: impl Into<String>, body: impl Into<String>) -> Self {
        let duration = match level {
            ToastLevel::Info | ToastLevel::Success => Some(Duration::from_secs(5)),
            ToastLevel::Warning => Some(Duration::from_secs(8)),
            ToastLevel::Error => None,
        };
        Self {
            title: title.into(),
            body: body.into(),
            level,
            action: None,
            duration,
        }
    }
}
/// Stable notification identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ToastId(u64);
#[derive(Clone, Debug)]
struct QueuedToast<Message> {
    id: ToastId,
    toast: Toast<Message>,
    visible_since: Option<Instant>,
}
/// FIFO notification queue with deterministic expiry.
#[derive(Clone, Debug)]
pub struct ToastQueue<Message> {
    next: u64,
    entries: Vec<QueuedToast<Message>>,
    visible_limit: usize,
}
impl<Message> Default for ToastQueue<Message> {
    fn default() -> Self {
        Self {
            next: 0,
            entries: Vec::new(),
            visible_limit: 3,
        }
    }
}
impl<Message> ToastQueue<Message> {
    /// Enqueues a toast and returns its identity.
    pub fn push(&mut self, toast: Toast<Message>, now: Instant) -> ToastId {
        let id = ToastId(self.next);
        self.next += 1;
        let visible_since = (self.entries.len() < self.visible_limit).then_some(now);
        self.entries.push(QueuedToast {
            id,
            toast,
            visible_since,
        });
        id
    }
    /// Dismisses a toast and promotes queued entries.
    pub fn dismiss(&mut self, id: ToastId, now: Instant) -> bool {
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return false;
        };
        self.entries.remove(index);
        self.promote(now);
        true
    }
    /// Removes elapsed visible toasts and promotes queued entries.
    pub fn expire(&mut self, now: Instant) -> Vec<ToastId> {
        let expired =
            self.entries
                .iter()
                .filter(|entry| {
                    entry.visible_since.zip(entry.toast.duration).is_some_and(
                        |(start, duration)| now.saturating_duration_since(start) >= duration,
                    )
                })
                .map(|entry| entry.id)
                .collect::<Vec<_>>();
        self.entries.retain(|entry| !expired.contains(&entry.id));
        self.promote(now);
        expired
    }
    /// Returns the next visible expiry deadline.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.entries
            .iter()
            .filter_map(|entry| {
                entry
                    .visible_since
                    .zip(entry.toast.duration)
                    .map(|(start, duration)| start + duration)
            })
            .min()
    }
    fn promote(&mut self, now: Instant) {
        for entry in self.entries.iter_mut().take(self.visible_limit) {
            if entry.visible_since.is_none() {
                entry.visible_since = Some(now);
            }
        }
    }
}

/// Retained toast overlay synchronized from a [`ToastQueue`].
pub struct ToastHost {
    overlay: ElementHandle<Overlay>,
    content: ElementHandle<Column>,
    rows: Vec<ElementHandle<Column>>,
}
impl ToastHost {
    /// Attaches an initially empty top-right notification overlay.
    pub fn new<Message: 'static>(ui: &mut Ui<Message>) -> Result<Self, UiError> {
        let overlay = ui.add_overlay(
            ui.root(),
            OverlayOptions {
                side: OverlaySide::Above,
                alignment: OverlayAlignment::End,
                z_index: 900,
                ..Default::default()
            },
        )?;
        ui.set_semantic_role(overlay, SemanticRole::Status)?;
        ui.set_layout(
            overlay,
            LayoutStyle {
                min_width: Length::Px(280.0),
                max_width: Length::Px(380.0),
                ..Default::default()
            },
        )?;
        ui.set_visibility(overlay, Visibility::Collapsed)?;
        let padding = ui.add_padding(overlay, Insets::all(12.0))?;
        let content = ui.add_column(padding)?;
        Ok(Self {
            overlay,
            content,
            rows: Vec::new(),
        })
    }

    /// Returns the viewport-hosted toast surface.
    pub const fn overlay(&self) -> ElementHandle<Overlay> {
        self.overlay
    }

    /// Rebuilds visible toast presentation. Dismiss buttons emit `on_dismiss(id)`.
    pub fn sync<Message: Clone + 'static, F>(
        &mut self,
        ui: &mut Ui<Message>,
        queue: &ToastQueue<Message>,
        on_dismiss: F,
    ) -> Result<(), UiError>
    where
        F: Fn(ToastId) -> Message + Clone + 'static,
    {
        for row in self.rows.drain(..) {
            ui.remove(row)?;
        }
        for entry in queue
            .entries
            .iter()
            .filter(|entry| entry.visible_since.is_some())
            .take(queue.visible_limit)
        {
            let row = ui.add_column(self.content)?;
            ui.set_semantic_role(
                row,
                if entry.toast.level == ToastLevel::Error {
                    SemanticRole::Alert
                } else {
                    SemanticRole::Status
                },
            )?;
            ui.set_semantic_live(
                row,
                if entry.toast.level == ToastLevel::Error {
                    SemanticLive::Assertive
                } else {
                    SemanticLive::Polite
                },
            )?;
            let title = ui.add_label(row, &entry.toast.title)?;
            ui.set_widget_style(
                title,
                WidgetStyle {
                    foreground: Some(match entry.toast.level {
                        ToastLevel::Info => ui.theme().accent,
                        ToastLevel::Success => ui.theme().success,
                        ToastLevel::Warning => ui.theme().warning,
                        ToastLevel::Error => ui.theme().danger,
                    }),
                    font_weight: Some(ui.theme().type_scale.heading_weight),
                    ..Default::default()
                },
            )?;
            let body = ui.add_label(row, &entry.toast.body)?;
            ui.set_widget_style(
                body,
                WidgetStyle {
                    foreground: Some(ui.theme().muted_foreground),
                    font_size: Some(ui.theme().type_scale.caption),
                    ..Default::default()
                },
            )?;
            if let Some(action) = &entry.toast.action {
                let button = ui.add_button(row, &action.label)?;
                let message = action.message.clone();
                ui.listen(button, None, EventFilter::Activate, move |context, _| {
                    context.emit(message.clone())
                })?;
            }
            let dismiss = ui.add_button(row, "Dismiss")?;
            let id = entry.id;
            let callback = on_dismiss.clone();
            ui.listen(dismiss, None, EventFilter::Activate, move |context, _| {
                context.emit(callback(id))
            })?;
            self.rows.push(row);
        }
        ui.set_visibility(
            self.overlay,
            if self.rows.is_empty() {
                Visibility::Collapsed
            } else {
                Visibility::Visible
            },
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astrelis_core::geometry::Size;
    use astrelis_text::FontDatabase;
    use astrelis_ui_core::{SemanticAction, Theme};

    #[test]
    fn queued_toasts_start_expiry_when_promoted() {
        let start = Instant::now();
        let mut queue = ToastQueue::default();
        for n in 0..4 {
            queue.push(Toast::<()>::new(ToastLevel::Info, n.to_string(), ""), start);
        }
        assert_eq!(queue.entries[3].visible_since, None);
        queue.dismiss(queue.entries[0].id, start + Duration::from_secs(2));
        assert_eq!(
            queue.entries[2].visible_since,
            Some(start + Duration::from_secs(2))
        );
    }

    #[test]
    fn dialog_is_single_modal_and_emits_typed_actions() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        ui.set_viewport(Size::new(640.0, 480.0), 1.0);
        let mut host = DialogHost::new();
        host.show(
            &mut ui,
            DialogOptions {
                title: "Confirm".into(),
                description: None,
            },
            vec![DialogAction {
                label: "Continue".into(),
                message: 7,
                role: DialogActionRole::Primary,
                enabled: true,
            }],
            |_, _| Ok(()),
        )
        .unwrap();
        assert!(matches!(
            host.show(
                &mut ui,
                DialogOptions {
                    title: "Other".into(),
                    description: None
                },
                vec![],
                |_, _| Ok(())
            ),
            Err(DialogError::AlreadyOpen)
        ));
        let tree = ui.semantic_tree().unwrap();
        let button = find_semantic(&tree, SemanticRole::Button, "Continue").unwrap();
        ui.perform_semantic_action(button, SemanticAction::Activate)
            .unwrap();
        assert_eq!(ui.drain_messages().collect::<Vec<_>>(), vec![7]);
        assert!(host.close(&mut ui).unwrap());
    }

    #[test]
    fn toast_content_is_inset_from_its_surface() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        let mut queue = ToastQueue::default();
        queue.push(
            Toast::<i32>::new(ToastLevel::Error, "Settings saved", "Hello"),
            Instant::now(),
        );
        let mut host = ToastHost::new(&mut ui).unwrap();
        host.sync(&mut ui, &queue, |_| 0).unwrap();
        ui.set_viewport(Size::new(640.0, 480.0), 1.0);
        ui.semantic_tree().unwrap();
        let surface = ui.layout_bounds(host.overlay).unwrap();
        let row = ui.layout_bounds(host.rows[0]).unwrap();
        assert!(row.origin.x >= surface.origin.x + 11.9);
        assert!(row.origin.y >= surface.origin.y + 11.9);
    }

    #[test]
    fn empty_toast_host_is_collapsed() {
        let mut ui = Ui::<i32>::new(FontDatabase::default(), Theme::default());
        let queue = ToastQueue::default();
        let mut host = ToastHost::new(&mut ui).unwrap();
        host.sync(&mut ui, &queue, |_| 0).unwrap();
        ui.set_viewport(Size::new(640.0, 480.0), 1.0);
        ui.semantic_tree().unwrap();

        assert_eq!(ui.layout_bounds(host.overlay()).unwrap().size, Size::ZERO);
    }

    #[test]
    fn toolbar_overflows_trailing_commands_and_exposes_semantics() {
        let mut ui = Ui::new(FontDatabase::default(), Theme::default());
        let mut commands = CommandRegistry::new();
        for (id, label, message) in [("a", "Alpha", 1), ("b", "Beta", 2), ("c", "Gamma", 3)] {
            commands
                .register(astreon_app::Command::new(
                    CommandId::new(id).unwrap(),
                    label,
                    message,
                ))
                .unwrap();
        }
        let items = commands
            .iter()
            .map(|command| ToolbarItem::Command {
                id: command.id.clone(),
                icon: Some(crate::icons::add()),
            })
            .collect();
        let root = ui.root();
        let toolbar =
            Toolbar::new(&mut ui, root, items, &commands, ToolbarOptions::default()).unwrap();
        // Wide enough for the first labeled command ("Alpha") plus the
        // overflow reserve, so the trailing two commands must overflow.
        let available =
            LABELED_COMMAND_BASE_WIDTH + 5.0 * LABELED_COMMAND_CHAR_WIDTH + OVERFLOW_RESERVE_WIDTH;
        toolbar.update_overflow(&mut ui, available).unwrap();
        ui.set_viewport(Size::new(640.0, 200.0), 1.0);
        let tree = ui.semantic_tree().unwrap();
        let first = toolbar.buttons[0].1;
        let label = ui.widget(first).unwrap().label_handle().unwrap();
        let button_bounds = ui.layout_bounds(first).unwrap();
        let label_bounds = ui.layout_bounds(label).unwrap();
        assert!(label_bounds.origin.x >= button_bounds.origin.x + 24.0);
        assert!(find_semantic(&tree, SemanticRole::Toolbar, "").is_some());
        assert!(find_semantic(&tree, SemanticRole::Button, "More").is_some());
    }

    fn find_semantic(
        node: &astrelis_ui_core::SemanticNode,
        role: SemanticRole,
        label: &str,
    ) -> Option<astrelis_ui_core::ElementId> {
        if node.role == role && node.label == label {
            return Some(node.id);
        }
        node.children
            .iter()
            .find_map(|child| find_semantic(child, role, label))
    }
}
