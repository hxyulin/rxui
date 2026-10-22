//! Hover tooltips: delayed, non-focusable popovers that never intercept input.
use super::{callback, local};
use crate::{
    AnchorHandle, DismissEvent, Dispatch, Element, Entity, FocusEvent, HoverEvent, IntoElement,
    KeyInput, PointerEvents, PointerInput, ThemeColor, ViewContext, label, popover, stack,
};
use std::{cell::Cell, time::Duration};
use taffy::prelude::{AlignItems, Dimension};

/// Hover time before a tooltip appears.
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(600);

/// Placement-local hover state for one tooltip.
#[derive(Default)]
pub(crate) struct TooltipState {
    hovered: bool,
    keyboard_focused: bool,
    focused: bool,
    /// Hidden by a press, key or Escape until the pointer leaves and returns.
    suppressed: bool,
    /// Frame time when the pointer entered, possibly from an earlier frame.
    entered: Duration,
    /// First fresh frame time after entering, recorded during evaluation.
    since: Cell<Option<Duration>>,
}

fn update(
    state: &Entity<TooltipState>,
    cx: &mut crate::AppContext<'_>,
    f: impl FnOnce(&mut TooltipState) -> bool,
) -> Result<Dispatch, crate::AccessError> {
    state.try_update(cx, |s, cx| {
        if f(s) {
            Dispatch::Handled
        } else {
            cx.unchanged();
            Dispatch::Unchanged
        }
    })
}

/// Wraps `target` with a hover and keyboard-focus tooltip showing `text`.
///
/// The tooltip appears after the pointer rests on the target for
/// [`TOOLTIP_DELAY`] (also after keyboard focus enters its subtree). It hides
/// when neither hover nor keyboard focus remains, on any press or key
/// press inside the target, on Escape, and when the target is hidden or scrolled
/// away. It never takes focus, is transparent to pointer input and does not
/// consume outside presses, so it cannot block the control it describes. The text
/// is also published as the target's accessible description (unless it already
/// has one), which is how keyboard and assistive users receive it; the popover
/// itself is hidden from assistive technology.
///
/// The wrapper takes over the target's flex-item properties (grow, shrink,
/// basis, alignment, margins, insets, grid placement and percentage sizes), so the
/// target keeps its place in the parent layout. While the delay runs the owning
/// view requests one deadline after a fresh entry frame. Hover and focus state is
/// placement-local and keyed by call site and the target's key, so tooltips built
/// in a loop should have keyed targets. Updating text preserves the current delay
/// and visibility.
///
/// ```
/// use rxui::prelude::*;
/// struct Toolbar;
/// impl View for Toolbar {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         row().gap(4.)
///             .child(tooltip(cx, button("Save").key("save"), "Save the document (Ctrl+S)"))
///             .child(tooltip(cx, button("Open").key("open"), "Open a document"))
///     }
/// }
/// ```
#[track_caller]
pub fn tooltip<T>(
    cx: &ViewContext<'_, T>,
    target: impl IntoElement,
    text: impl Into<String>,
) -> Element {
    let mut target = target.into_element();
    let text = text.into();
    let state: Entity<TooltipState> = local(cx, target.key.clone());
    let visible = {
        let s = state.read(cx);
        let now = cx.frame_time();
        if (!s.hovered && !s.keyboard_focused) || s.suppressed {
            false
        } else if s.since.get().is_none() && now == s.entered {
            // The hover listener saw the previous frame's clock; wait for a fresh one.
            cx.request_animation_frame();
            false
        } else {
            let since = s.since.get().unwrap_or(now);
            s.since.set(Some(since));
            let shown = now.saturating_sub(since) >= TOOLTIP_DELAY;
            if !shown {
                cx.request_wakeup_at(since.saturating_add(TOOLTIP_DELAY));
            }
            shown
        }
    };
    if target
        .semantics
        .as_ref()
        .is_none_or(|s| s.description.is_none())
    {
        target = target.accessibility_description(text.clone());
    }
    let anchor = AnchorHandle::new();
    let mut wrapper = stack()
        .anchor_handle(anchor.clone())
        .align_items(AlignItems::STRETCH)
        .justify_items(AlignItems::STRETCH);
    adopt_item_layout(&mut wrapper, &mut target);
    let hover = state.clone();
    wrapper = wrapper.on_hover(callback(move |event: &HoverEvent, cx| {
        let now = cx.runtime.frames.borrow().1;
        let hovered = event.hovered;
        update(&hover, cx, |s| {
            let active = s.hovered || s.keyboard_focused;
            let rearm = hovered && s.suppressed;
            let changed = s.hovered != hovered || rearm;
            s.hovered = hovered;
            if active != (s.hovered || s.keyboard_focused) || rearm {
                s.suppressed = false;
                s.entered = now;
                s.since.set(None);
            }
            changed
        })
    }));
    let focus = state.clone();
    wrapper = wrapper.on_focus_change(callback(move |event: &FocusEvent, cx| {
        let now = cx.runtime.frames.borrow().1;
        update(&focus, cx, |s| {
            let active = s.hovered || s.keyboard_focused;
            let fresh = event.focused && !s.focused;
            // A key on a pointer-focused control dismisses its hover tooltip;
            // changing modality alone must not turn that into keyboard entry.
            let focused = event.focused && event.keyboard && (fresh || s.keyboard_focused);
            let changed = s.focused != event.focused || s.keyboard_focused != focused;
            s.focused = event.focused;
            s.keyboard_focused = focused;
            if !changed {
                return false;
            }
            let next_active = s.hovered || s.keyboard_focused;
            if active != next_active || (fresh && event.keyboard && s.suppressed) {
                if !next_active || fresh {
                    s.suppressed = false;
                }
                s.entered = now;
                s.since.set(None);
            }
            true
        })
    }));
    let press = state.clone();
    wrapper = wrapper.on_pointer_down_capture(callback(move |_: &PointerInput, cx| {
        update(&press, cx, |s| {
            !std::mem::replace(&mut s.suppressed, s.hovered || s.keyboard_focused)
                && (s.hovered || s.keyboard_focused)
        })
    }));
    let key = state.clone();
    wrapper = wrapper.on_key_down_capture(callback(move |_: &KeyInput, cx| {
        update(&key, cx, |s| !std::mem::replace(&mut s.suppressed, true))
    }));
    wrapper = wrapper.child(target);
    if visible {
        let dismiss = state.clone();
        let mut overlay = popover(anchor, label(text).color(ThemeColor::Text).font_size(12.))
            .key("tooltip")
            .padding(6.)
            .autofocus(false)
            .on_dismiss(callback(move |_: &DismissEvent, cx| {
                update(&dismiss, cx, |s| {
                    !std::mem::replace(&mut s.suppressed, true)
                })
            }))
            .into_element()
            .pointer_events(PointerEvents::None)
            .accessibility_hidden(true);
        if let Some(surface) = overlay.children.first_mut() {
            surface.input.get_or_insert_with(Default::default).focusable = Some(false);
        }
        wrapper = wrapper.child(overlay);
    }
    wrapper
}

/// Moves the target's flex/grid item properties to the wrapper that replaces it
/// in its parent, leaving the target to fill the wrapper.
fn adopt_item_layout(wrapper: &mut Element, target: &mut Element) {
    let (w, t) = (&mut wrapper.style, &mut target.style);
    w.flex_grow = t.flex_grow;
    w.flex_shrink = t.flex_shrink;
    w.flex_basis = std::mem::replace(&mut t.flex_basis, Dimension::auto());
    w.align_self = t.align_self.take();
    w.justify_self = t.justify_self.take();
    w.margin = std::mem::replace(&mut t.margin, taffy::Rect::zero());
    w.position = std::mem::replace(&mut t.position, taffy::Position::Relative);
    w.inset = std::mem::replace(&mut t.inset, taffy::Rect::auto());
    w.grid_row = std::mem::take(&mut t.grid_row);
    w.grid_column = std::mem::take(&mut t.grid_column);
    for (outer, inner) in [
        (&mut w.size.width, &mut t.size.width),
        (&mut w.size.height, &mut t.size.height),
    ] {
        if inner.into_raw().tag() == taffy::CompactLength::PERCENT_TAG {
            *outer = *inner;
            *inner = Dimension::percent(1.);
        }
    }
}
