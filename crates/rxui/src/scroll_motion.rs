//! Smooth scrolling for one placement. Discrete wheel steps and programmatic
//! commands on a container whose behavior is `ScrollBehavior::Smooth` move its
//! offset over `ThemeMotion::scroll` instead of jumping. Each preparation first
//! writes the offsets at the current frame time, so scroll metrics, virtual lists
//! and hit testing see them before views are described. Any other change of the
//! offset (trackpad deltas, scrollbar drags, focus and caret reveal, assistive
//! actions) ends the move, keeping the new offset. Nothing is allocated while no
//! smooth scroll is in flight.
use super::*;
use crate::{Easing, ScrollBehavior, animation::Timing, scrolling::CommandMode};
use animation_dispatch::FrameRequest;
use std::time::Duration;

/// Smooth scrolls in flight; allocated only while one moves.
pub(super) struct ScrollMotion {
    moves: IdMap<ElementId, ScrollMove>,
    _frame: FrameRequest,
}
struct ScrollMove {
    from: [f32; 2],
    to: [f32; 2],
    /// Frame time the move started; `None` until the next fresh frame.
    start: Option<Duration>,
    timing: Timing,
    /// Offset the move last wrote. Any other offset means something else moved the
    /// container, which ends the move.
    applied: [f32; 2],
    /// Pointer position of the wheel input that started the move, for hover.
    pointer: Option<[f32; 2]>,
}
impl ScrollMove {
    /// Offset at frame time `now` and whether the move has ended.
    fn at(&self, now: Duration) -> ([f32; 2], bool) {
        let Some(start) = self.start else {
            return (self.from, false);
        };
        let (t, done) = self.timing.progress(now.saturating_sub(start), 0.);
        if done {
            return (self.to, true);
        }
        let blend = |axis: usize| self.from[axis] + (self.to[axis] - self.from[axis]) * t;
        ([blend(0), blend(1)], false)
    }
}
fn clamp(offset: [f32; 2], range: [f32; 2]) -> [f32; 2] {
    [offset[0].clamp(0., range[0]), offset[1].clamp(0., range[1])]
}
impl<T: View> Ui<T> {
    /// Timing of a smooth scroll of container `id`, when its behavior, the theme and
    /// the frame clock allow one.
    fn smooth_timing(&self, id: ElementId) -> Option<Timing> {
        let (animates, ..) = self.motion_clock();
        let node = self.nodes.get(&id)?;
        if !animates || node.element.scroll_behavior != ScrollBehavior::Smooth {
            return None;
        }
        let duration = node.resolved_theme.sizes().motion.scroll;
        crate::Transition::tween(duration, Easing::EaseOut).timing(duration)
    }
    /// The live move of `id`, unless something else has moved the container since.
    fn live_move(&self, id: ElementId) -> Option<&ScrollMove> {
        let node = self.nodes.get(&id)?;
        self.scroll_motion
            .as_ref()?
            .moves
            .get(&id)
            .filter(|m| m.applied == node.scroll_offset)
    }
    /// Where container `id` is heading: a live move's target, or its offset.
    fn scroll_target(&self, id: ElementId) -> [f32; 2] {
        self.live_move(id)
            .map_or(self.nodes[&id].scroll_offset, |m| m.to)
    }
    /// Starts or retargets a smooth move of `id` toward `target`, clamped to its
    /// range, from the offset displayed now. The move starts at `start`, or at the
    /// next fresh frame.
    fn animate_scroll(
        &mut self,
        id: ElementId,
        target: [f32; 2],
        timing: Timing,
        start: Option<Duration>,
        pointer: Option<[f32; 2]>,
    ) {
        let node = &self.nodes[&id];
        let (from, to) = (node.scroll_offset, clamp(target, node.scroll_range));
        if from == to {
            self.cancel_scroll_motion(id);
            return;
        }
        let runtime = &self.runtime;
        let motion = self.scroll_motion.get_or_insert_with(|| {
            Box::new(ScrollMotion {
                moves: IdMap::default(),
                _frame: FrameRequest::new(runtime),
            })
        });
        motion.moves.insert(
            id,
            ScrollMove {
                from,
                to,
                start,
                timing,
                applied: from,
                pointer,
            },
        );
    }
    fn cancel_scroll_motion(&mut self, id: ElementId) {
        if let Some(motion) = &mut self.scroll_motion {
            motion.moves.remove(&id);
            if motion.moves.is_empty() {
                self.scroll_motion = None;
            }
        }
    }
    /// Applies one queued scroll command. Returns whether the offset changed now;
    /// a smooth command starts a move instead, at this frame when it is fresh.
    pub(crate) fn apply_scroll_command(
        &mut self,
        target: ElementId,
        offset: [f32; 2],
        mode: CommandMode,
    ) -> bool {
        let Some(node) = self.nodes.get(&target) else {
            return false;
        };
        let (current, range) = (node.scroll_offset, node.scroll_range);
        let next = match mode {
            CommandMode::To | CommandMode::By => {
                let base = if mode == CommandMode::By {
                    self.scroll_target(target)
                } else {
                    [0.; 2]
                };
                let next = clamp([base[0] + offset[0], base[1] + offset[1]], range);
                if let Some(timing) = self.smooth_timing(target) {
                    let (_, fresh, now) = self.motion_clock();
                    self.animate_scroll(target, next, timing, fresh.then_some(now), None);
                    return false;
                }
                self.cancel_scroll_motion(target);
                next
            }
            CommandMode::Jump => {
                self.cancel_scroll_motion(target);
                clamp(offset, range)
            }
            CommandMode::Anchor(shift) => {
                let next = clamp(offset, range);
                // A move in flight keeps heading to the same content.
                if let Some(motion) = &mut self.scroll_motion
                    && let Some(m) = motion.moves.get_mut(&target)
                    && m.applied == current
                {
                    m.from = [m.from[0] + shift[0], m.from[1] + shift[1]];
                    m.to = clamp([m.to[0] + shift[0], m.to[1] + shift[1]], range);
                    m.applied = next;
                }
                next
            }
        };
        let node = self.nodes.get_mut(&target).unwrap();
        let changed = node.scroll_offset != next;
        node.scroll_offset = next;
        changed
    }
    /// Writes the offsets of smooth scrolls in flight at the current frame time and
    /// retires finished or interrupted moves. Without a running animation clock, or
    /// with reduce motion, moves complete immediately.
    pub(super) fn advance_scroll_motion(&mut self) -> Result<(), UiError> {
        if self.scroll_motion.is_none() {
            return Ok(());
        }
        let (animates, fresh, now) = self.motion_clock();
        let nodes = &mut self.nodes;
        let motion = self.scroll_motion.as_mut().unwrap();
        let mut changed = false;
        let mut pointer = None;
        motion.moves.retain(|id, m| {
            let Some(node) = nodes.get_mut(id).filter(|n| n.element.scroll.is_some()) else {
                return false;
            };
            if node.scroll_offset != m.applied {
                return false;
            }
            if fresh {
                m.start.get_or_insert(now);
            }
            let (value, done) = if animates { m.at(now) } else { (m.to, true) };
            let next = clamp(value, node.scroll_range);
            if next != node.scroll_offset {
                node.scroll_offset = next;
                changed = true;
                pointer = m.pointer.or(pointer);
            }
            m.applied = next;
            !done
        });
        if motion.moves.is_empty() {
            self.scroll_motion = None;
        }
        if changed && self.viewport.is_some() && self.root.is_some() {
            self.refresh_geometry()?;
            // Hover paint follows the content under a still pointer; hover listeners
            // update with the next pointer motion, as after `Ui::scroll`.
            if let Some(point) = pointer {
                self.hovered = self
                    .pointer_target(point)
                    .filter(|id| self.input_available(*id));
            }
        }
        Ok(())
    }
    /// Scrolls by discrete wheel steps converted to logical pixels: smooth containers
    /// animate toward their target plus the delta, others move directly, and unused
    /// motion chains to scrolling ancestors as in [`Ui::scroll`].
    pub(super) fn scroll_lines(
        &mut self,
        point: [f32; 2],
        mut delta: [f32; 2],
    ) -> Result<bool, UiError> {
        if point.iter().chain(delta.iter()).any(|v| !v.is_finite()) {
            return Err(UiError::InvalidGeometry);
        }
        if !self.geometry_ready {
            return Ok(false);
        }
        let mut current = self.wheel_target(point);
        let (mut moved, mut animated) = (false, false);
        while let Some(id) = current {
            let node = &self.nodes[&id];
            let parent = node.parent;
            if node.element.scroll.is_some() {
                let base = self.scroll_target(id);
                let next = clamp([base[0] + delta[0], base[1] + delta[1]], node.scroll_range);
                delta = [
                    delta[0] - (next[0] - base[0]),
                    delta[1] - (next[1] - base[1]),
                ];
                if let Some(timing) = self.smooth_timing(id) {
                    if next != base {
                        self.animate_scroll(id, next, timing, None, Some(point));
                        animated = true;
                    }
                } else if next != self.nodes[&id].scroll_offset {
                    self.cancel_scroll_motion(id);
                    self.nodes.get_mut(&id).unwrap().scroll_offset = next;
                    moved = true;
                }
            }
            if self.is_overlay(id) {
                break;
            }
            current = parent;
        }
        if moved {
            self.refresh_geometry()?;
            self.hovered = self
                .pointer_target(point)
                .filter(|id| self.input_available(*id));
        }
        Ok(moved || animated)
    }
}
