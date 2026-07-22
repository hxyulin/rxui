//! Reusable decoding from routed pointer input to viewport navigation intents.

use astrelis_core::geometry::{LogicalPoint, Point};
use astrelis_platform::{DeviceId, Modifiers, TouchPhase};
use astrelis_ui_core::{RoutedEventKind, ScrollGranularity};

/// Behavior assigned to one class of scrolling input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollNavigation {
    /// Move the viewport on both reported axes.
    Pan,
    /// Change magnification from the vertical component.
    Zoom,
    /// Leave the input unhandled.
    Ignore,
}

/// Platform-independent viewport navigation requested by pointer input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewportNavigationIntent {
    /// Scroll the viewport by a logical displacement.
    Pan {
        /// Input device.
        device_id: DeviceId,
        /// Logical window position under the gesture.
        position: LogicalPoint,
        /// Logical scroll displacement on both axes.
        delta: LogicalPoint,
        /// Gesture lifecycle phase.
        phase: TouchPhase,
    },
    /// Change magnification around the gesture position.
    Zoom {
        /// Input device.
        device_id: DeviceId,
        /// Logical window position under the gesture.
        position: LogicalPoint,
        /// Multiplicative magnification; values above one zoom in.
        factor: f64,
        /// Gesture lifecycle phase.
        phase: TouchPhase,
    },
}

/// Configurable mappings from wheels and gestures to viewport navigation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportNavigationBindings {
    /// Discrete line-wheel behavior.
    pub line_scroll: ScrollNavigation,
    /// Precision pixel-scroll behavior.
    pub pixel_scroll: ScrollNavigation,
    /// Convert Shift plus vertical scrolling into horizontal panning.
    pub shift_scrolls_horizontally: bool,
    /// Magnification response per logical wheel pixel.
    pub wheel_zoom_sensitivity: f64,
    /// Magnification response to native pinch deltas.
    pub pinch_zoom_sensitivity: f64,
    /// Whether native pan gestures move the viewport.
    pub pan_gestures: bool,
    /// Whether native pinch gestures change magnification.
    pub pinch_gestures: bool,
}

impl Default for ViewportNavigationBindings {
    fn default() -> Self {
        Self {
            line_scroll: ScrollNavigation::Zoom,
            pixel_scroll: ScrollNavigation::Pan,
            shift_scrolls_horizontally: true,
            wheel_zoom_sensitivity: 0.002,
            pinch_zoom_sensitivity: 1.0,
            pan_gestures: true,
            pinch_gestures: true,
        }
    }
}

impl ViewportNavigationBindings {
    /// Decodes one routed input event according to these bindings.
    pub fn decode(
        self,
        event: &RoutedEventKind,
        modifiers: Modifiers,
    ) -> Option<ViewportNavigationIntent> {
        match event {
            RoutedEventKind::Scroll {
                device_id,
                position,
                delta,
                granularity,
                phase,
            } => {
                let behavior = match granularity {
                    ScrollGranularity::Line => self.line_scroll,
                    ScrollGranularity::Pixel => self.pixel_scroll,
                };
                let horizontal = self.shift_scrolls_horizontally
                    && modifiers.shift
                    && delta.x.abs() <= f32::EPSILON;
                if horizontal {
                    return Some(ViewportNavigationIntent::Pan {
                        device_id: *device_id,
                        position: *position,
                        delta: Point::new(delta.y, 0.0),
                        phase: *phase,
                    });
                }
                match behavior {
                    ScrollNavigation::Pan => Some(ViewportNavigationIntent::Pan {
                        device_id: *device_id,
                        position: *position,
                        delta: *delta,
                        phase: *phase,
                    }),
                    ScrollNavigation::Zoom if delta.y.abs() > f32::EPSILON => {
                        Some(ViewportNavigationIntent::Zoom {
                            device_id: *device_id,
                            position: *position,
                            factor: (-f64::from(delta.y) * self.wheel_zoom_sensitivity).exp(),
                            phase: *phase,
                        })
                    }
                    ScrollNavigation::Zoom | ScrollNavigation::Ignore => None,
                }
            }
            RoutedEventKind::PinchGesture {
                device_id,
                position,
                delta,
                phase,
            } if self.pinch_gestures => Some(ViewportNavigationIntent::Zoom {
                device_id: *device_id,
                position: *position,
                factor: (*delta * self.pinch_zoom_sensitivity).exp(),
                phase: *phase,
            }),
            RoutedEventKind::PanGesture {
                device_id,
                position,
                delta,
                phase,
            } if self.pan_gestures => Some(ViewportNavigationIntent::Pan {
                device_id: *device_id,
                position: *position,
                delta: Point::new(-delta.x, -delta.y),
                phase: *phase,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll(granularity: ScrollGranularity, delta: LogicalPoint) -> RoutedEventKind {
        RoutedEventKind::Scroll {
            device_id: DeviceId(7),
            position: Point::new(30.0, 40.0),
            delta,
            granularity,
            phase: TouchPhase::Moved,
        }
    }

    #[test]
    fn precision_scroll_pans_on_both_axes() {
        let intent = ViewportNavigationBindings::default()
            .decode(
                &scroll(ScrollGranularity::Pixel, Point::new(12.0, -8.0)),
                Modifiers::default(),
            )
            .unwrap();
        assert!(matches!(
            intent,
            ViewportNavigationIntent::Pan { delta, .. }
                if delta == Point::new(12.0, -8.0)
        ));
    }

    #[test]
    fn line_wheel_zooms_and_shift_remaps_it_to_horizontal_pan() {
        let bindings = ViewportNavigationBindings::default();
        let event = scroll(ScrollGranularity::Line, Point::new(0.0, -40.0));
        assert!(matches!(
            bindings.decode(&event, Modifiers::default()).unwrap(),
            ViewportNavigationIntent::Zoom { factor, .. } if factor > 1.0
        ));
        assert!(matches!(
            bindings.decode(&event, Modifiers { shift: true, ..Modifiers::default() }).unwrap(),
            ViewportNavigationIntent::Pan { delta, .. }
                if delta == Point::new(-40.0, 0.0)
        ));
    }

    #[test]
    fn pinch_magnifies_and_native_pan_follows_the_fingers() {
        let bindings = ViewportNavigationBindings::default();
        let pinch = RoutedEventKind::PinchGesture {
            device_id: DeviceId(7),
            position: LogicalPoint::ZERO,
            delta: 0.25,
            phase: TouchPhase::Moved,
        };
        assert!(matches!(
            bindings.decode(&pinch, Modifiers::default()).unwrap(),
            ViewportNavigationIntent::Zoom { factor, .. } if factor > 1.0
        ));

        let pan = RoutedEventKind::PanGesture {
            device_id: DeviceId(7),
            position: LogicalPoint::ZERO,
            delta: Point::new(4.0, -6.0),
            phase: TouchPhase::Moved,
        };
        assert!(matches!(
            bindings.decode(&pan, Modifiers::default()).unwrap(),
            ViewportNavigationIntent::Pan { delta, .. }
                if delta == Point::new(-4.0, 6.0)
        ));
    }
}
