//! Disclosures: a header button that shows and hides its content.
use super::{ToggleEvent, callback, local, propose, quiet_root};
use crate::{
    AccessibilityHandle, Animated, ClickEvent, Dispatch, Element, Entity, Icon, IntoElement,
    Listener, ScrollHandle, ScrollState, SemanticRole, Transition, ViewContext, button, column,
    label, row,
};
use std::cell::RefCell;
use std::rc::Rc;
use taffy::prelude::{AlignItems, JustifyContent};

/// Placement-local expanded state and reveal motion.
#[derive(Default)]
pub(crate) struct DisclosureState {
    /// Expanded state of an uncontrolled disclosure.
    expanded: bool,
    /// Binds the content region, whose scroll range reports its natural height.
    scroll: ScrollHandle,
    motion: Rc<Motion>,
}

/// Height and opacity of the content region.
struct Motion {
    height: RefCell<Animated<f32>>,
    opacity: RefCell<Animated<f32>>,
}
impl Default for Motion {
    fn default() -> Self {
        Self {
            height: RefCell::new(Animated::new(0.)),
            opacity: RefCell::new(Animated::new(0.)),
        }
    }
}

/// A header button that shows or hides content below it; see [`disclosure`].
#[must_use = "attach the disclosure to a parent or return it from View::view"]
pub struct Disclosure {
    root: Element,
    title: String,
    content: Element,
    state: Entity<DisclosureState>,
    local: bool,
    expanded: Option<bool>,
    toggle: Option<Listener<ToggleEvent>>,
    scroll: ScrollHandle,
    scroll_state: Option<ScrollState>,
    motion: Rc<Motion>,
    clock: crate::context::FrameClock,
    transition: Transition,
}

/// Creates a disclosure titled `title` over `content`, collapsed at first.
///
/// The header is a quiet button with a chevron, right when collapsed and down
/// when expanded. Enter, Space or a click toggles it. Without
/// [`Disclosure::expanded`] the disclosure keeps its own state at this call site
/// in the current placement, and [`Disclosure::on_toggle`] only reports changes;
/// disclosures built in a loop need [`disclosure_keyed`]. With it, the
/// application owns the state and accepts the proposals.
///
/// Expanding reveals the content: its height grows from zero to its natural
/// height as it fades in, over the theme's `overlay_enter` duration, and
/// collapsing reverses that. With reduced motion, or without a host frame
/// clock, it shows and hides at once. Collapsing content is inert at once: it
/// leaves Tab order and the accessibility tree before its motion ends, and is
/// removed when the motion ends.
///
/// The header publishes its expanded state and, while the content shows,
/// controls the content's Group, which the title names.
///
/// ```
/// use rxui::prelude::*;
/// struct Settings { trim: bool }
/// impl View for Settings {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         disclosure(cx, "Advanced", checkbox(self.trim, "Trim trailing whitespace")
///             .on_change(cx.listener(|s, e: &ToggleEvent, _| s.trim = e.checked)))
///     }
/// }
/// ```
#[track_caller]
pub fn disclosure<T>(
    cx: &ViewContext<'_, T>,
    title: impl Into<String>,
    content: impl IntoElement,
) -> Disclosure {
    disclosure_keyed(cx, (), title, content)
}

/// Like [`disclosure`], with separate state per key at one call site.
#[track_caller]
pub fn disclosure_keyed<T>(
    cx: &ViewContext<'_, T>,
    key: impl std::hash::Hash,
    title: impl Into<String>,
    content: impl IntoElement,
) -> Disclosure {
    let state: Entity<DisclosureState> = local(cx, key);
    let read = state.read(cx);
    let scroll_state = read.scroll.state(cx);
    Disclosure {
        root: column().fill_width(),
        title: title.into(),
        content: content.into_element(),
        local: read.expanded,
        state: state.clone(),
        expanded: None,
        toggle: None,
        scroll: read.scroll.clone(),
        scroll_state,
        motion: read.motion.clone(),
        clock: cx.frame_clock(),
        transition: Transition::ease(cx.theme().sizes().motion.overlay_enter),
    }
}

impl Disclosure {
    /// Makes the disclosure controlled: it shows `expanded` and proposes
    /// changes through [`Self::on_toggle`].
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }
    /// Receives the proposed expanded state as [`ToggleEvent::checked`].
    pub fn on_toggle(mut self, listener: Listener<ToggleEvent>) -> Self {
        self.toggle = Some(listener);
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
}

impl IntoElement for Disclosure {
    fn into_element(self) -> Element {
        let controlled = self.expanded.is_some();
        let expanded = self.expanded.unwrap_or(self.local);
        let motion = &self.motion;
        // The region scrolls by nothing; its range is how far the content
        // overflows, so viewport plus range is the content's height.
        let natural = self.scroll_state.map(|s| s.viewport[1] + s.range[1]);
        let target = if expanded { natural.unwrap_or(0.) } else { 0. };
        let height = self
            .clock
            .drive(&mut motion.height.borrow_mut(), target, self.transition);
        let opacity = self.clock.drive(
            &mut motion.opacity.borrow_mut(),
            if expanded { 1. } else { 0. },
            self.transition,
        );
        let settled = natural.is_some() && height == target;
        let shown = expanded || height > 0.;

        let region = AccessibilityHandle::new();
        let title = AccessibilityHandle::new();
        let chevron = if expanded {
            Icon::CHEVRON_DOWN
        } else {
            Icon::CHEVRON_RIGHT
        };
        let mut header = quiet_root(button(
            row()
                .gap(6.)
                .align_items(AlignItems::CENTER)
                .child(crate::icon(chevron))
                .child(label(self.title).accessibility_handle(title.clone())),
        ))
        .key("header")
        .justify_content(JustifyContent::START)
        .accessibility_expanded(expanded);
        if expanded {
            header = header.accessibility_controls(region.clone());
        }
        let (local, toggle) = (self.state.clone(), self.toggle);
        header = header.on_click(callback(move |_: &ClickEvent, cx| {
            if !controlled {
                local.try_update(cx, |s, _| s.expanded = !expanded)?;
            }
            propose(toggle.as_ref(), &ToggleEvent { checked: !expanded }, cx)?;
            Ok(Dispatch::Handled)
        }));
        let mut root = self.root.child(header);
        if !shown {
            return root;
        }
        let mut content = column()
            .key("content")
            .fill_width()
            .scroll_y()
            .scroll_handle(self.scroll)
            .opacity(opacity.clamp(0., 1.))
            .accessibility_role(SemanticRole::Group)
            .accessibility_handle(region)
            .accessibility_labelled_by(title)
            .inert(!expanded)
            .child(self.content);
        // While it moves, or before its height is known, the region is as tall
        // as the motion; afterwards it follows its content.
        if !(expanded && settled) {
            content = content.height(height.max(0.));
        }
        root = root.child(content);
        root
    }
}
