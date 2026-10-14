//! Link spans in rich text: retained span geometry, per-link hover, press and
//! focus, and pointer, keyboard and assistive activation.
//!
//! A rich text element with links stays one text leaf. Focus remains the element;
//! the focused link is an index beside it. Link rectangles come from the text
//! measurer's shaped selection geometry and are cached per element until its text,
//! spans, content width or font generation change, so pointer hit testing,
//! semantics and reveal need no text work on input.
use super::*;
use crate::input::{KeyEvent, KeyboardKey};

/// Logical distance a press on a link in selectable text may travel before the
/// gesture becomes a selection drag instead of a click, as for click counting.
const CLICK_SLOP: f32 = 4.;

/// Shaped rectangles of each link, in text-local logical units.
#[derive(Default)]
struct Geometry {
    /// Text revision, content width bits and measurement generation measured.
    key: Option<(u64, u32, u64)>,
    rects: Vec<Vec<Bounds>>,
}
struct Press {
    id: ElementId,
    index: usize,
    origin: [f32; 2],
    moved: bool,
}
/// Hovered link and pressed link with its drag flag, compared around pointer input.
pub(super) type Snapshot = (Option<(ElementId, usize)>, Option<(usize, bool)>);
#[derive(Default)]
pub(super) struct State {
    geometry: IdMap<ElementId, Geometry>,
    hovered: Option<(ElementId, usize)>,
    press: Option<Press>,
    /// Link of the focused element that holds focus; None leaves focus on the
    /// element itself, as on selectable text focused by a press between links.
    focused: Option<usize>,
}

/// Link spans of a rich text element and their interaction state, for painting.
#[derive(Clone, Copy, Debug)]
pub struct LinkInfo<'a> {
    /// Byte range of each link span in the element's text, in link index order.
    /// Empty links have no geometry and never take hover or focus.
    pub ranges: &'a [std::ops::Range<usize>],
    /// Link under the pointer.
    pub hovered: Option<usize>,
    /// Link pressed by the primary button and still eligible to activate.
    pub pressed: Option<usize>,
    /// Link holding keyboard focus when the element is focused. With
    /// [`ElementInfo::focus_visible`], the focus ring surrounds this link's text
    /// rather than the element.
    pub focused: Option<usize>,
    /// Color the pressed, else hovered, link is redrawn in: the theme's accent
    /// hover or pressed color for links using the default accent. None keeps the
    /// span's own color.
    pub active_color: Option<Color>,
}

impl<T: View> Ui<T> {
    /// Rich text content of an element with at least one non-empty link.
    pub(super) fn rich_links(&self, id: ElementId) -> Option<&crate::typography::RichText> {
        match &self.nodes.get(&id)?.element.kind {
            ElementKind::RichText(rich) if rich.has_links() => Some(rich),
            _ => None,
        }
    }
    /// Starts tracking a reconciled element's link geometry when it has links.
    pub(super) fn register_link_node(&mut self, id: ElementId) {
        if self.rich_links(id).is_some() {
            self.links
                .get_or_insert_with(Default::default)
                .geometry
                .entry(id)
                .or_default();
        }
    }
    /// Remeasures link rectangles whose text, spans, width or fonts changed, and
    /// drops interaction state that no longer names a live link.
    pub(super) fn refresh_links(&mut self, measure: &mut impl TextMeasure) -> Result<(), UiError> {
        let Some(mut state) = self.links.take() else {
            return Ok(());
        };
        let mut measured = 0;
        let result = self.measure_links(&mut state, measure, &mut measured);
        self.stats.link_measurements += measured;
        let live = |ui: &Self, (id, index): (ElementId, usize)| {
            ui.rich_links(id)
                .is_some_and(|rich| rich.link_ranges.get(index).is_some_and(|r| !r.is_empty()))
        };
        if state.hovered.is_some_and(|link| !live(self, link)) {
            state.hovered = None;
        }
        if state
            .press
            .as_ref()
            .is_some_and(|p| !live(self, (p.id, p.index)))
        {
            state.press = None;
        }
        state.focused = match (self.focused, state.focused) {
            (Some(id), Some(index)) if live(self, (id, index)) => Some(index),
            // A focused link that disappeared moves focus to the first remaining one.
            (Some(id), Some(_)) => self.first_link(id, false),
            _ => None,
        };
        self.links = Some(state);
        result
    }
    fn measure_links(
        &self,
        state: &mut State,
        measure: &mut impl TextMeasure,
        measured: &mut u64,
    ) -> Result<(), UiError> {
        state
            .geometry
            .retain(|id, _| self.rich_links(*id).is_some());
        for (id, geometry) in &mut state.geometry {
            let node = &self.nodes[id];
            if !node.visible {
                continue;
            }
            let key = (
                node.text_revision,
                node.content_bounds.width.to_bits(),
                self.measurement_generation,
            );
            if geometry.key == Some(key) {
                continue;
            }
            let ElementKind::RichText(rich) = &node.element.kind else {
                continue;
            };
            geometry.rects.clear();
            for range in &rich.link_ranges {
                let rects = if range.is_empty() {
                    Vec::new()
                } else {
                    measure
                        .text_range_rects(*id, node.text_request(), range.clone())?
                        .unwrap_or_default()
                };
                geometry.rects.push(rects);
            }
            geometry.key = Some(key);
            *measured += 1;
        }
        Ok(())
    }
    /// First, or with `reverse` last, non-empty link of an element.
    fn first_link(&self, id: ElementId, reverse: bool) -> Option<usize> {
        let ranges = &self.rich_links(id)?.link_ranges;
        let mut links = ranges.iter().enumerate().filter(|(_, r)| !r.is_empty());
        if reverse {
            links.next_back()
        } else {
            links.next()
        }
        .map(|(index, _)| index)
    }
    /// Link of a visible element under a logical window point, from cached geometry.
    pub(super) fn link_at(&self, id: ElementId, point: [f32; 2]) -> Option<usize> {
        let geometry = self.links.as_ref()?.geometry.get(&id)?;
        let node = &self.nodes[&id];
        if !node.visible || !node.clip_bounds.contains(point) {
            return None;
        }
        let local = [
            point[0] - node.content_bounds.x + node.editor.as_ref().map_or(0., |e| e.scroll_x),
            point[1] - node.content_bounds.y,
        ];
        geometry
            .rects
            .iter()
            .position(|rects| rects.iter().any(|r| r.contains(local)))
    }
    /// Whether the element takes focus only through its links: rich text with links
    /// that is neither selectable nor explicitly focusable.
    pub(super) fn link_only_focus(&self, id: ElementId) -> bool {
        self.rich_links(id).is_some()
            && self.nodes[&id].editor.is_none()
            && self.nodes[&id]
                .element
                .input
                .as_ref()
                .and_then(|p| p.focusable)
                .is_none()
    }
    /// Index of the focused link within the focused rich text element. None when
    /// focus is elsewhere or on selectable text between its links.
    pub fn focused_link(&self) -> Option<usize> {
        self.links
            .as_ref()?
            .focused
            .filter(|_| self.focused.is_some())
    }
    /// The rich text element and link under the pointer, if any.
    pub fn hovered_link(&self) -> Option<(ElementId, usize)> {
        self.links.as_ref()?.hovered
    }
    /// Sets the link-focus default after focus moved to `next`: the first link of
    /// rich text that takes focus only through links, otherwise none.
    pub(super) fn reset_link_focus(&mut self) {
        let next = self
            .focused
            .filter(|id| self.link_only_focus(*id))
            .and_then(|id| self.first_link(id, false));
        if let Some(state) = &mut self.links {
            state.focused = next;
        }
    }
    fn set_link_focus(&mut self, index: Option<usize>) -> bool {
        let state = self.links.get_or_insert_with(Default::default);
        std::mem::replace(&mut state.focused, index) != index
    }
    /// Reveals the focused link's first rectangle through scroll ancestors.
    fn reveal_link(&mut self, id: ElementId, index: usize) -> bool {
        let area = self
            .links
            .as_ref()
            .and_then(|s| s.geometry.get(&id))
            .and_then(|g| g.rects.get(index))
            .and_then(|rects| rects.first().copied());
        match area {
            Some(area) => self.reveal_area(id, Some(area)),
            None => self.reveal(id),
        }
    }
    /// Tab within the focused rich text element: the next or previous link, if
    /// one remains in that direction. Selectable text focused between links moves
    /// forward to its first link. None lets traversal leave the element.
    pub(super) fn step_link_focus(&mut self, reverse: bool) -> Option<bool> {
        let id = self.focused?;
        let ranges = &self.rich_links(id)?.link_ranges;
        let next = match self.focused_link() {
            Some(current) if reverse => (0..current).rev().find(|i| !ranges[*i].is_empty()),
            Some(current) => (current + 1..ranges.len()).find(|i| !ranges[*i].is_empty()),
            None if reverse => None,
            None => self.first_link(id, false),
        }?;
        self.set_link_focus(Some(next));
        self.reveal_link(id, next);
        Some(true)
    }
    /// After Tab moved focus to `id`, focuses its first link, or its last when
    /// moving backward. Returns whether anything changed.
    pub(super) fn enter_links(&mut self, reverse: bool) -> bool {
        let Some(id) = self.focused else {
            return false;
        };
        let Some(index) = self.first_link(id, reverse) else {
            return false;
        };
        self.set_link_focus(Some(index)) | self.reveal_link(id, index)
    }
    /// Interaction state compared before and after pointer input.
    pub(super) fn link_snapshot(&self) -> Snapshot {
        let Some(state) = &self.links else {
            return (None, None);
        };
        (
            state.hovered,
            state.press.as_ref().map(|p| (p.index, p.moved)),
        )
    }
    /// Updates the hovered link from the pointer's hit target, and marks a press
    /// in selectable text as a drag once it travels beyond the click slop.
    pub(super) fn update_link_hover(&mut self, hit: Option<ElementId>, point: [f32; 2]) {
        if self.links.is_none() {
            return;
        }
        let hovered = hit
            .filter(|id| self.input_available(*id))
            .and_then(|id| Some((id, self.link_at(id, point)?)));
        let state = self.links.as_mut().unwrap();
        state.hovered = hovered;
        if let Some(press) = &mut state.press {
            let [dx, dy] = [point[0] - press.origin[0], point[1] - press.origin[1]];
            press.moved |= dx.hypot(dy) > CLICK_SLOP;
        }
    }
    /// The pointer left the window: no link is hovered, a press stays pending.
    pub(super) fn leave_links(&mut self) {
        if let Some(state) = &mut self.links {
            state.hovered = None;
        }
    }
    /// Forgets hover and any press, for pointer cancellation and deactivation.
    pub(super) fn clear_link_pointer(&mut self, hover: bool) {
        if let Some(state) = &mut self.links {
            state.press = None;
            if hover {
                state.hovered = None;
            }
        }
    }
    /// A primary press on `target`: starts a link press when the pointer is over
    /// one of its links, returning that link.
    pub(super) fn press_link(
        &mut self,
        target: Option<ElementId>,
        point: [f32; 2],
    ) -> Option<usize> {
        let state = self.links.as_mut()?;
        state.press = None;
        let (id, index) = state.hovered.filter(|(id, _)| target == Some(*id))?;
        state.press = Some(Press {
            id,
            index,
            origin: point,
            moved: false,
        });
        Some(index)
    }
    /// After a primary press focused rich text: the pressed link takes focus, and
    /// a press between links leaves focus on the text itself.
    pub(super) fn focus_pressed_link(&mut self, target: Option<ElementId>, link: Option<usize>) {
        if target.is_some_and(|id| self.focused == Some(id) && self.rich_links(id).is_some()) {
            self.set_link_focus(link);
        }
    }
    /// A primary release: activates the pressed link when the pointer is still
    /// over it and, in selectable text, the gesture did not become a drag.
    pub(super) fn release_link(&mut self, runtime: &mut Runtime) -> Result<bool, UiError> {
        let Some(press) = self.links.as_mut().and_then(|s| s.press.take()) else {
            return Ok(false);
        };
        let selecting = press.moved
            && self
                .nodes
                .get(&press.id)
                .is_some_and(|n| n.editor.is_some());
        if selecting || self.hovered_link() != Some((press.id, press.index)) {
            return Ok(true);
        }
        self.activate_link(runtime, press.id, press.index)?;
        Ok(true)
    }
    /// Dispatches one link's listener if its element accepts input.
    pub(super) fn activate_link(
        &self,
        runtime: &mut Runtime,
        id: ElementId,
        index: usize,
    ) -> Result<bool, UiError> {
        if !self.geometry_ready || !self.input_available(id) {
            return Ok(false);
        }
        let Some(rich) = self.rich_links(id) else {
            return Ok(false);
        };
        if rich.link_ranges.get(index).is_none_or(|r| r.is_empty()) {
            return Ok(false);
        }
        let listener = rich.links[index].target.listener.clone();
        Ok(
            runtime.update(|cx| listener.dispatch(&crate::LinkEvent { index }, cx))?
                == Dispatch::Handled,
        )
    }
    /// Enter on a focused link activates it and prevents the host's default.
    /// Repeats are swallowed so holding Enter activates once.
    pub(super) fn link_key(
        &mut self,
        runtime: &mut Runtime,
        event: &KeyEvent,
    ) -> Result<Option<bool>, UiError> {
        let modifiers = event.modifiers;
        if event.key != KeyboardKey::Enter || modifiers.control || modifiers.alt || modifiers.meta {
            return Ok(None);
        }
        let (Some(id), Some(index)) = (self.focused, self.focused_link()) else {
            return Ok(None);
        };
        if event.repeat {
            return Ok(Some(false));
        }
        Ok(Some(self.activate_link(runtime, id, index)?))
    }
    /// Whether the pointer cursor shows over a hovered link: not during another
    /// element's capture or a selection drag.
    pub(super) fn link_cursor(&self) -> bool {
        let Some(state) = &self.links else {
            return false;
        };
        let Some((id, _)) = state.hovered else {
            return false;
        };
        if self.captured_pointer().is_some() || self.hovered != Some(id) {
            return false;
        }
        match &state.press {
            Some(press) => !press.moved || self.nodes[&press.id].editor.is_none(),
            None => self.pressed.is_none(),
        }
    }
    /// Painting snapshot of an element's links.
    pub(super) fn link_info(&self, id: ElementId) -> Option<LinkInfo<'_>> {
        let rich = self.rich_links(id)?;
        let state = self.links.as_deref();
        let hovered = state
            .and_then(|s| s.hovered)
            .filter(|(link, _)| *link == id)
            .map(|(_, index)| index);
        let pressed = state
            .and_then(|s| s.press.as_ref())
            .filter(|p| p.id == id && !p.moved && hovered == Some(p.index))
            .map(|p| p.index);
        let focused = (self.focused == Some(id))
            .then(|| self.focused_link())
            .flatten();
        let node = &self.nodes[&id];
        let active_color = pressed.or(hovered).and_then(|index| {
            rich.links[index].accent.then(|| {
                StyleColor::Theme(if pressed.is_some() {
                    ThemeColor::AccentPressed
                } else {
                    ThemeColor::AccentHover
                })
                .resolve(&node.resolved_theme)
            })
        });
        Some(LinkInfo {
            ranges: &rich.link_ranges,
            hovered,
            pressed,
            focused,
            active_color,
        })
    }
    /// Assistive Link nodes of one visible element, in link order, skipping empty
    /// links. Bounds are logical window coordinates: the union of the link's
    /// shaped rectangles, or the element's content box without text geometry.
    pub fn semantic_links(&self, id: ElementId) -> impl Iterator<Item = crate::SemanticLink<'_>> {
        let rich = self
            .rich_links(id)
            .filter(|_| self.is_prepared() && self.semantic_visible(id));
        let geometry = self.links.as_ref().and_then(|s| s.geometry.get(&id));
        let focused = (self.semantic_focus() == Some(id))
            .then(|| self.focused_link())
            .flatten();
        let available = self.input_available(id);
        rich.into_iter().flat_map(move |rich| {
            let node = &self.nodes[&id];
            rich.link_ranges
                .iter()
                .enumerate()
                .filter(|(_, range)| !range.is_empty())
                .map(move |(index, range)| {
                    let rects = geometry.and_then(|g| g.rects.get(index));
                    let bounds = rects
                        .and_then(|rects| {
                            rects.iter().copied().reduce(|a, b| {
                                let x = a.x.min(b.x);
                                let y = a.y.min(b.y);
                                Bounds {
                                    x,
                                    y,
                                    width: (a.x + a.width).max(b.x + b.width) - x,
                                    height: (a.y + a.height).max(b.y + b.height) - y,
                                }
                            })
                        })
                        .map_or(node.content_bounds, |r| Bounds {
                            x: node.content_bounds.x + r.x,
                            y: node.content_bounds.y + r.y,
                            ..r
                        });
                    let target = &rich.links[index].target;
                    crate::SemanticLink {
                        element: id,
                        index,
                        name: &rich.text[range.clone()],
                        range: range.clone(),
                        url: target.url.as_deref(),
                        description: target.description.as_deref(),
                        bounds,
                        focused: focused == Some(index),
                        available,
                    }
                })
        })
    }
    /// Number of assistive Link nodes of an element.
    pub(super) fn semantic_link_count(&self, id: ElementId) -> usize {
        self.rich_links(id).map_or(0, |rich| {
            rich.link_ranges.iter().filter(|r| !r.is_empty()).count()
        })
    }
    /// Assistive focus or activation of one link.
    pub(super) fn semantic_link_action(
        &mut self,
        runtime: &mut Runtime,
        id: ElementId,
        index: usize,
        activate: bool,
    ) -> Result<bool, UiError> {
        let valid = self
            .rich_links(id)
            .is_some_and(|rich| rich.link_ranges.get(index).is_some_and(|r| !r.is_empty()));
        if !valid || !self.geometry_ready {
            return Ok(false);
        }
        if activate {
            return self.activate_link(runtime, id, index);
        }
        if !self.enabled(id) {
            return Ok(false);
        }
        let changed = self.focused != Some(id);
        self.change_focus(Some(id));
        let changed = self.set_link_focus(Some(index)) | changed;
        Ok(self.reveal_link(id, index) | changed)
    }
}
