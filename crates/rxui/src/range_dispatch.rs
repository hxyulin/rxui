use super::*;
use crate::{Axis, KeyboardKey, PointerButton, RangeInfo, ResizeEvent, ResizePhase};
impl<T: View> Ui<T> {
    pub(super) fn range_unavailable(&self, id: ElementId) -> bool {
        self.nodes.get(&id).is_some_and(|n| {
            matches!(
                n.element.kind,
                ElementKind::Scrollbar(_) | ElementKind::Splitter(_)
            )
        }) && self.range_info(id).is_none_or(|r| r.read_only)
    }
    pub(crate) fn range_info(&self, id: ElementId) -> Option<RangeInfo> {
        let n = self.nodes.get(&id)?;
        match &n.element.kind {
            ElementKind::Scrollbar(p) => {
                let state = p.handle.state_in(self.tree)?;
                let a = p.axis.index();
                let track = n.content_bounds;
                let size = [track.width, track.height];
                let starts = [track.x, track.y];
                let length = (size[a] - 4.).max(0.);
                let extent = state.viewport[a];
                let total = extent + state.range[a];
                let thumb = if total > 0. {
                    (length * extent / total).max(p.min_thumb).min(length)
                } else {
                    length
                };
                let offset = if state.range[a] > 0. {
                    (length - thumb) * state.offset[a] / state.range[a]
                } else {
                    0.
                };
                let mut bounds = Bounds {
                    x: track.x + 2.,
                    y: track.y + 2.,
                    width: (track.width - 4.).max(0.),
                    height: (track.height - 4.).max(0.),
                };
                if a == 0 {
                    bounds.x = starts[a] + 2. + offset;
                    bounds.width = thumb;
                } else {
                    bounds.y = starts[a] + 2. + offset;
                    bounds.height = thumb;
                }
                Some(RangeInfo {
                    axis: p.axis,
                    value: state.offset[a],
                    min: 0.,
                    max: state.range[a],
                    step: 40.,
                    thumb_bounds: Some(bounds),
                    read_only: state.range[a] <= 0. || length <= thumb,
                })
            }
            ElementKind::Splitter(p) => {
                let parent = self.nodes.get(&n.parent?)?;
                let a = p.axis.index();
                let extent = [parent.content_bounds.width, parent.content_bounds.height][a];
                let available = (extent - p.divider).max(0.);
                let value = ([n.bounds.x, n.bounds.y][a]
                    - [parent.content_bounds.x, parent.content_bounds.y][a])
                    .max(0.);
                Some(RangeInfo {
                    axis: p.axis,
                    value,
                    min: p.min_first,
                    max: (available - p.min_second).max(p.min_first),
                    step: 8.,
                    thumb_bounds: None,
                    read_only: p.resize.is_none() || available <= p.min_first + p.min_second,
                })
            }
            _ => None,
        }
    }
    pub(super) fn resize_event(
        &self,
        id: ElementId,
        pixels: f32,
        phase: ResizePhase,
    ) -> Option<(crate::Listener<ResizeEvent>, ResizeEvent)> {
        let ElementKind::Splitter(p) = &self.nodes.get(&id)?.element.kind else {
            return None;
        };
        let n = &self.nodes[&id];
        let parent = self.nodes.get(&n.parent?)?;
        let a = p.axis.index();
        let available =
            ([parent.content_bounds.width, parent.content_bounds.height][a] - p.divider).max(0.);
        let info = self.range_info(id)?;
        let first_size = pixels.clamp(info.min, info.max);
        Some((
            p.resize.clone()?,
            ResizeEvent {
                position: if matches!(phase, ResizePhase::Begin | ResizePhase::End) {
                    p.position
                } else {
                    p.position.at(first_size, available)
                },
                first_size,
                available,
                phase,
            },
        ))
    }
    pub(super) fn range_value(
        &mut self,
        runtime: &mut Runtime,
        id: ElementId,
        value: f32,
        phase: ResizePhase,
    ) -> Result<bool, UiError> {
        let Some(info) = self.range_info(id).filter(|r| !r.read_only) else {
            return Ok(false);
        };
        if !value.is_finite() {
            return Err(UiError::InvalidGeometry);
        }
        let value = value.clamp(info.min, info.max);
        if value == info.value
            && matches!(
                phase,
                ResizePhase::Drag | ResizePhase::Keyboard | ResizePhase::Accessibility
            )
        {
            return Ok(false);
        }
        if let ElementKind::Scrollbar(p) = &self.nodes[&id].element.kind {
            let handle = p.handle.clone();
            let Some(viewport) = handle.element_in(self.tree) else {
                return Ok(false);
            };
            let mut offset = self.nodes[&viewport].scroll_offset;
            offset[info.axis.index()] = value;
            self.set_scroll_offset(viewport, offset)
        } else if let Some((listener, event)) = self.resize_event(id, value, phase) {
            Ok(runtime.update(|cx| listener.dispatch(&event, cx))? == Dispatch::Handled)
        } else {
            Ok(false)
        }
    }
    pub(super) fn range_pointer(
        &mut self,
        runtime: &mut Runtime,
        id: ElementId,
        kind: usize,
        button: Option<PointerButton>,
    ) -> Result<Option<bool>, UiError> {
        let Some(info) = self.range_info(id) else {
            return Ok(None);
        };
        if info.read_only {
            return Ok(Some(false));
        }
        let a = info.axis.index();
        if kind == 0 && button == Some(PointerButton::Primary) {
            self.change_focus(Some(id));
            let mut start = info.value;
            let initial = info.value;
            if let Some(thumb) = info.thumb_bounds
                && !thumb.contains(self.input.position)
            {
                let n = &self.nodes[&id];
                let track = ([n.content_bounds.width, n.content_bounds.height][a] - 4.).max(0.);
                let len = [thumb.width, thumb.height][a];
                let travel = track - len;
                if travel > 0. {
                    start = ((self.input.position[a]
                        - [n.content_bounds.x, n.content_bounds.y][a]
                        - 2.
                        - len / 2.)
                        / travel
                        * info.max)
                        .clamp(info.min, info.max);
                }
                self.range_value(runtime, id, start, ResizePhase::Begin)?;
            }
            let n = &self.nodes[&id];
            let parent = n
                .parent
                .and_then(|p| self.nodes.get(&p))
                .map(|n| n.content_bounds);
            self.input.capture = Some(input_dispatch::Capture {
                id,
                button: PointerButton::Primary,
                position: self.input.position,
                bounds: n.bounds,
                parent,
                range: Some([initial, start]),
                restore: if let ElementKind::Splitter(p) = &n.element.kind {
                    Some(p.position)
                } else {
                    None
                },
                axis: Some(info.axis),
            });
            if info.thumb_bounds.is_none() {
                self.range_value(runtime, id, start, ResizePhase::Begin)?;
            }
            Ok(Some(true))
        } else if let Some(c) = self.input.capture.filter(|c| c.id == id) {
            if kind == 1 {
                let delta = self.input.position[a] - c.position[a];
                let start = c.range.unwrap()[1];
                let value = if let Some(thumb) = info.thumb_bounds {
                    let n = &self.nodes[&id];
                    let travel = ([n.content_bounds.width, n.content_bounds.height][a]
                        - 4.
                        - [thumb.width, thumb.height][a])
                        .max(0.);
                    if travel > 0. {
                        start + delta / travel * info.max
                    } else {
                        start
                    }
                } else {
                    start + delta
                };
                Ok(Some(self.range_value(
                    runtime,
                    id,
                    value,
                    ResizePhase::Drag,
                )?))
            } else if kind == 2 && button == Some(c.button) {
                Ok(Some(self.range_value(
                    runtime,
                    id,
                    info.value,
                    ResizePhase::End,
                )?))
            } else {
                Ok(Some(false))
            }
        } else {
            Ok(Some(false))
        }
    }
    pub(super) fn range_key(
        &mut self,
        runtime: &mut Runtime,
        id: ElementId,
        key: &KeyboardKey,
    ) -> Result<Option<bool>, UiError> {
        let Some(info) = self.range_info(id) else {
            return Ok(None);
        };
        let negative = match info.axis {
            Axis::Horizontal => KeyboardKey::ArrowLeft,
            Axis::Vertical => KeyboardKey::ArrowUp,
        };
        let positive = match info.axis {
            Axis::Horizontal => KeyboardKey::ArrowRight,
            Axis::Vertical => KeyboardKey::ArrowDown,
        };
        let value = if *key == negative {
            info.value - info.step
        } else if *key == positive {
            info.value + info.step
        } else {
            match key {
                KeyboardKey::Home => info.min,
                KeyboardKey::End => info.max,
                KeyboardKey::PageUp | KeyboardKey::PageDown if info.thumb_bounds.is_some() => {
                    let ElementKind::Scrollbar(p) = &self.nodes[&id].element.kind else {
                        unreachable!()
                    };
                    let page = p
                        .handle
                        .state_in(self.tree)
                        .map_or(0., |s| s.viewport[info.axis.index()]);
                    info.value
                        + if *key == KeyboardKey::PageUp {
                            -page
                        } else {
                            page
                        }
                }
                _ => return Ok(None),
            }
        };
        Ok(Some(self.range_value(
            runtime,
            id,
            value,
            ResizePhase::Keyboard,
        )?))
    }
}
