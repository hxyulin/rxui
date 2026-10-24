//! Feedback: inline callouts and corner toasts.
use super::{Part, Tone, callback, local, part, propose};
use crate::{
    ClickEvent, Dispatch, Element, Elevation, Entity, FocusEvent, FontWeight, HoverEvent, Icon,
    IntoElement, Listener, LivePoliteness, Overlay, SemanticRole, TextOverflow, ThemeColor,
    ViewContext, button, column, icon_button, label, row, stack,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;
use taffy::prelude::AlignItems;

/// The icon of a tone: information for neutral and accent, and the status
/// icons for the others.
pub(super) fn tone_icon(tone: Tone) -> Icon {
    match tone {
        Tone::Neutral | Tone::Accent => Icon::INFO,
        Tone::Danger => Icon::CIRCLE_ALERT,
        Tone::Warning => Icon::TRIANGLE_ALERT,
        Tone::Success => Icon::CIRCLE_CHECK,
    }
}

/// Alert for problems, Status for everything else.
fn tone_role(tone: Tone) -> SemanticRole {
    match tone {
        Tone::Danger | Tone::Warning => SemanticRole::Alert,
        _ => SemanticRole::Status,
    }
}

/// The tone icon, title and description of a callout or toast.
fn message(tone: Tone, title: String, description: Option<String>) -> [Element; 2] {
    // The icon's 16 units center on the title's first line.
    let icon = crate::icon(tone_icon(tone))
        .color(tone.foreground())
        .flex_shrink(0.)
        .layout(|s| s.margin.top = taffy::prelude::length(2.));
    let mut text = column()
        .gap(2.)
        .flex_grow(1.)
        .flex_shrink(1.)
        .min_width(0.)
        .child(label(title).key("title").font_weight(FontWeight::MEDIUM));
    if let Some(description) = description {
        text = text.child(
            label(description)
                .key("description")
                .color(ThemeColor::TextMuted)
                .text_overflow(TextOverflow::Wrap),
        );
    }
    [icon, text]
}

/// A close button for a callout or toast.
fn close(listener: Listener<ClickEvent>) -> Element {
    icon_button(Icon::X)
        .key("dismiss")
        .accessibility_label("Dismiss")
        .flex_shrink(0.)
        // The button's square overlaps the padding instead of growing the row.
        .margin(-4.)
        .on_click(listener)
}

/// An inline banner with a tone; see [`callout`].
#[must_use = "add the callout to a parent element"]
pub struct Callout {
    root: Element,
    tone: Tone,
    title: String,
    description: Option<String>,
    actions: Vec<Element>,
    dismiss: Option<Listener<ClickEvent>>,
}

/// An inline banner: the tone's icon, a medium-weight title, optional muted
/// description, and optional actions and close button at its end.
///
/// It is drawn on the tone's subtle fill inside a 1-unit border of the tone's
/// foreground at half alpha, with the theme's `radius`. The icon is
/// [`Icon::INFO`] for [`Tone::Neutral`] and [`Tone::Accent`],
/// [`Icon::CIRCLE_ALERT`] for danger, [`Icon::TRIANGLE_ALERT`] for warnings and
/// [`Icon::CIRCLE_CHECK`] for success. Danger and warning callouts have the
/// Alert role and the others the Status role; the title names them.
///
/// ```
/// use rxui::prelude::*;
/// struct Status;
/// impl View for Status {
///     fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         callout(Tone::Warning, "rust-analyzer is out of date")
///             .description("Some features may not work until it is updated.")
///             .action(button("Update"))
///     }
/// }
/// ```
pub fn callout(tone: Tone, title: impl Into<String>) -> Callout {
    Callout {
        root: row().fill_width(),
        tone,
        title: title.into(),
        description: None,
        actions: Vec::new(),
        dismiss: None,
    }
}

impl Callout {
    /// Muted text below the title.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }
    /// Adds an element, usually a button, to the actions at the callout's end.
    /// Actions keep the order they are added in.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_element());
        self
    }
    /// Adds a close button that sends a [`ClickEvent`] to `listener`. The
    /// application removes the callout.
    pub fn on_dismiss(mut self, listener: Listener<ClickEvent>) -> Self {
        self.dismiss = Some(listener);
        self
    }
    /// Stable identity within the parent's sibling scope.
    pub fn key(mut self, key: impl Into<crate::Key>) -> Self {
        self.root.key = Some(key.into());
        self
    }
    /// Fixed logical width, instead of the parent's width.
    pub fn width(mut self, width: f32) -> Self {
        self.root = self.root.width(width);
        self
    }
}

impl IntoElement for Callout {
    fn into_element(self) -> Element {
        let [icon, text] = message(self.tone, self.title, self.description);
        let mut root = part(self.root, Part::Banner(self.tone))
            .gap(8.)
            .padding_x(12.)
            .padding_y(10.)
            .align_items(AlignItems::START)
            .accessibility_role(tone_role(self.tone))
            .child(icon)
            .child(text);
        if !self.actions.is_empty() {
            root = root.child(
                row()
                    .key("actions")
                    .gap(8.)
                    .flex_shrink(0.)
                    .align_items(AlignItems::CENTER)
                    .children(self.actions),
            );
        }
        if let Some(dismiss) = self.dismiss {
            root = root.child(close(dismiss));
        }
        root
    }
}

/// A corner of the window that [`toasts`] stack in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastPosition {
    /// Top left in left-to-right layouts.
    TopStart,
    /// Top right.
    TopEnd,
    /// Bottom left.
    BottomStart,
    /// Bottom right.
    #[default]
    BottomEnd,
}

/// One application-owned notification shown by [`toasts`].
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    /// Identity, unique within the list, that dismissals and actions report.
    pub id: u64,
    /// Status color and icon.
    pub tone: Tone,
    /// Medium-weight first line.
    pub title: String,
    /// Muted text below the title.
    pub description: Option<String>,
    /// Caption of an action button, reported through [`Toasts::on_action`].
    pub action: Option<String>,
    /// Time shown before it proposes its dismissal; `None` keeps it until it
    /// is closed.
    pub timeout: Option<Duration>,
}

impl Toast {
    /// A neutral toast that proposes its dismissal after 5 seconds.
    pub fn new(id: u64, title: impl Into<String>) -> Self {
        Self {
            id,
            tone: Tone::Neutral,
            title: title.into(),
            description: None,
            action: None,
            timeout: Some(Duration::from_secs(5)),
        }
    }
    /// Sets the tone.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
    /// Sets the muted text below the title.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }
    /// Adds an action button with `caption`.
    pub fn action(mut self, caption: impl Into<String>) -> Self {
        self.action = Some(caption.into());
        self
    }
    /// Sets the timeout; `None` keeps the toast until it is closed.
    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }
}

/// A proposal to remove a toast, after its timeout or from its close button.
/// Remove the toast from the list to accept it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToastDismiss {
    /// The toast's [`Toast::id`].
    pub id: u64,
}

/// Time already shown and the run in progress for one toast.
#[derive(Default)]
struct ToastClock {
    /// Time shown before the current run.
    elapsed: Duration,
    /// Frame time the current run started, while not paused.
    start: Option<Duration>,
    /// Frame time of the evaluation that saw the toast waiting to run; the run
    /// starts on a later, fresh frame.
    waiting: Option<Duration>,
    /// Whether the expiry has been proposed.
    proposed: bool,
}

/// Placement-local pause state, clocks and announcements.
#[derive(Default)]
pub(crate) struct ToastState {
    /// Toasts under the pointer.
    hovered: HashSet<u64>,
    /// Toasts containing focus.
    focused: HashSet<u64>,
    clocks: Rc<RefCell<HashMap<u64, ToastClock>>>,
    /// Toasts already announced, and the latest announcement.
    announced: Rc<RefCell<(HashSet<u64>, String)>>,
}

/// A stack of application-owned toasts in a window corner; see [`toasts`].
#[must_use = "attach the toasts to the window's root element"]
pub struct Toasts {
    state: Entity<ToastState>,
    toasts: Vec<Toast>,
    announcement: String,
    clock: crate::context::FrameClock,
    position: ToastPosition,
    dismiss: Option<Listener<ToastDismiss>>,
    action: Option<Listener<ToastAction>>,
    expired: Vec<u64>,
}

/// An action button of a toast was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToastAction {
    /// The toast's [`Toast::id`].
    pub id: u64,
}

/// Shows `list` as a stack of toasts in a window corner, the last nearest the
/// corner. Attach the result anywhere in the window; it takes no space.
///
/// Each toast is a raised card at most 360 units wide, with the tone's icon,
/// title, description, an optional action button and a close button. Toasts
/// enter and exit with the theme's overlay motion, rising from the window edge,
/// and appear at once with reduced motion. They never take focus or block the
/// window behind them; Tab reaches their buttons after the content before
/// [`toasts`] in the view.
///
/// The application owns the list. When a toast's timeout passes, or its close
/// button is pressed, the stack proposes [`ToastDismiss`] through
/// [`Toasts::on_dismiss`]; remove the toast to accept it. Timeouts run on the
/// frame clock through view deadlines, from the first frame that shows the
/// toast, and pause while the pointer is over a toast or focus is in one. The
/// expiry proposal is deferred to the runtime's next flush, since a view cannot
/// dispatch while it is evaluated.
///
/// A polite live region, which is not drawn, announces each new toast's title
/// and description once. Toast clocks and announcements belong to this call
/// site in the current placement.
///
/// ```
/// use rxui::prelude::*;
/// struct App { toasts: Vec<Toast> }
/// impl View for App {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         column()
///             .child(button("Save").on_click(cx.listener(|s, _, _| {
///                 s.toasts.push(Toast::new(1, "Saved").tone(Tone::Success));
///             })))
///             .child(toasts(cx, &self.toasts).on_dismiss(cx.listener(
///                 |s, e: &ToastDismiss, _| s.toasts.retain(|t| t.id != e.id),
///             )))
///     }
/// }
/// ```
#[track_caller]
pub fn toasts<T>(cx: &ViewContext<'_, T>, list: &[Toast]) -> Toasts {
    let state: Entity<ToastState> = local(cx, ());
    let read = state.read(cx);
    let ids: HashSet<u64> = list.iter().map(|t| t.id).collect();
    let paused = read
        .hovered
        .iter()
        .chain(&read.focused)
        .any(|id| ids.contains(id));
    let now = cx.frame_time();
    let mut expired = Vec::new();
    {
        let mut clocks = read.clocks.borrow_mut();
        clocks.retain(|id, _| ids.contains(id));
        for toast in list {
            let Some(timeout) = toast.timeout else {
                continue;
            };
            let clock = clocks.entry(toast.id).or_default();
            if clock.proposed {
                continue;
            }
            match (paused, clock.start) {
                (true, Some(start)) => {
                    clock.elapsed += now.saturating_sub(start);
                    clock.start = None;
                }
                (true, None) => clock.waiting = None,
                (false, None) => {
                    // The frame time may be stale between frames; start on a fresh one.
                    if clock.waiting.is_some_and(|at| at != now) {
                        clock.start = Some(now);
                        clock.waiting = None;
                    } else {
                        clock.waiting = Some(now);
                        cx.request_animation_frame();
                    }
                }
                (false, Some(_)) => {}
            }
            if let Some(start) = clock.start {
                let left = timeout.saturating_sub(clock.elapsed);
                if now.saturating_sub(start) >= left {
                    clock.proposed = true;
                    expired.push(toast.id);
                } else {
                    cx.request_wakeup_at(start + left);
                }
            }
        }
    }
    let announcement = {
        let mut announced = read.announced.borrow_mut();
        let (seen, text) = &mut *announced;
        seen.retain(|id| ids.contains(id));
        for toast in list {
            if seen.insert(toast.id) {
                *text = match &toast.description {
                    Some(d) => format!("{}. {d}", toast.title),
                    None => toast.title.clone(),
                };
            }
        }
        text.clone()
    };
    drop(read);
    Toasts {
        state,
        toasts: list.to_vec(),
        announcement,
        clock: cx.frame_clock(),
        position: ToastPosition::default(),
        dismiss: None,
        action: None,
        expired,
    }
}

impl Toasts {
    /// The corner the toasts stack in; [`ToastPosition::BottomEnd`] by default.
    pub fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }
    /// Receives dismissal proposals from timeouts and close buttons.
    pub fn on_dismiss(mut self, listener: Listener<ToastDismiss>) -> Self {
        self.dismiss = Some(listener);
        self
    }
    /// Receives presses of a toast's action button. The toast stays until the
    /// application removes it.
    pub fn on_action(mut self, listener: Listener<ToastAction>) -> Self {
        self.action = Some(listener);
        self
    }
}

/// Records the pointer or focus over toast `id`.
fn track(
    state: &Entity<ToastState>,
    cx: &mut crate::AppContext<'_>,
    id: u64,
    focus: bool,
    on: bool,
) -> Result<Dispatch, crate::AccessError> {
    state.try_update(cx, |s, cx| {
        let set = if focus {
            &mut s.focused
        } else {
            &mut s.hovered
        };
        let changed = if on { set.insert(id) } else { set.remove(&id) };
        if changed {
            Dispatch::Handled
        } else {
            cx.unchanged();
            Dispatch::Unchanged
        }
    })
}

impl IntoElement for Toasts {
    fn into_element(self) -> Element {
        // Expiry is decided while the view is evaluated, where nothing can be
        // dispatched, so it is proposed at the next flush.
        if let Some(dismiss) = &self.dismiss {
            for id in self.expired {
                let dismiss = dismiss.clone();
                self.clock.defer(move |cx| {
                    let _ = dismiss.dispatch(&ToastDismiss { id }, cx);
                });
            }
        }
        // Not drawn: a zero-size, clipped live region.
        let live = row().key("announcer").absolute().size(0., 0.).clip().child(
            label(self.announcement)
                .key("announcement")
                .accessibility_role(SemanticRole::Status)
                .accessibility_live(LivePoliteness::Polite),
        );
        let mut root = stack().key("toasts").absolute().size(0., 0.).child(live);
        for toast in self.toasts {
            let id = toast.id;
            let [icon, text] = message(toast.tone, toast.title, toast.description);
            let mut card = row()
                .key("toast")
                .width(360.)
                .padding(12.)
                .gap(10.)
                .align_items(AlignItems::START)
                .raised(Elevation::Popover)
                .accessibility_role(tone_role(toast.tone))
                .child(icon)
                .child(text);
            if let Some(caption) = toast.action {
                let action = self.action.clone();
                card = card.child(button(caption).key("action").flex_shrink(0.).on_click(
                    callback(move |_: &ClickEvent, cx| {
                        propose(action.as_ref(), &ToastAction { id }, cx)?;
                        Ok(Dispatch::Handled)
                    }),
                ));
            }
            let dismiss = self.dismiss.clone();
            card = card.child(close(callback(move |_: &ClickEvent, cx| {
                propose(dismiss.as_ref(), &ToastDismiss { id }, cx)?;
                Ok(Dispatch::Handled)
            })));
            let (hover, focus) = (self.state.clone(), self.state.clone());
            card = card
                .on_hover(callback(move |event: &HoverEvent, cx| {
                    track(&hover, cx, id, false, event.hovered)
                }))
                .on_focus_change(callback(move |event: &FocusEvent, cx| {
                    track(&focus, cx, id, true, event.focused)
                }));
            // The overlay takes the surface's key, so the card sits inside it.
            root = root.child(Overlay::corner(self.position, column().child(card)).key(id));
        }
        root
    }
}
