//! Determinate and indeterminate progress indicators.
use super::glyph::{Glyph, SPOKES, glyph};
use super::root_builders;
use crate::{
    Element, IntoElement, NumericValue, SemanticRole, ThemeColor, ViewContext, column, row,
};

/// Seconds for one sweep of an indeterminate bar.
const SWEEP: f32 = 1.6;
/// Fraction of the track covered by the indeterminate segment.
const SEGMENT: f32 = 0.3;

/// Progress bar builder. A value shows determinate progress through the range
/// and publishes it as a numeric ProgressIndicator; `None` shows an indeterminate
/// segment and publishes no value. Values outside the range are clamped.
///
/// ```
/// use rxui::prelude::*;
/// struct Upload { sent: u64, total: u64, connecting: bool }
/// impl View for Upload {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         let value = (!self.connecting).then_some(self.sent as f32);
///         progress_bar(value).range(0., self.total as f32)
///             .accessibility_label("Upload")
///             .animate(cx) // Only indeterminate bars request frames.
///     }
/// }
/// ```
#[must_use = "attach the progress bar to a parent or return it from View::view"]
pub struct ProgressBar {
    root: Element,
    value: Option<f32>,
    min: f32,
    max: f32,
    phase: Option<f32>,
}

/// Creates a 0..=1 progress bar, 200 logical units wide and 8 tall.
pub fn progress_bar(value: impl Into<Option<f32>>) -> ProgressBar {
    ProgressBar {
        root: column()
            .width(200.)
            .height(8.)
            .radius(4.)
            .background(ThemeColor::Input)
            .border(1., ThemeColor::InputBorder)
            .clip()
            .accessibility_role(SemanticRole::ProgressIndicator),
        value: value.into(),
        min: 0.,
        max: 1.,
        phase: None,
    }
}

impl ProgressBar {
    /// Inclusive range for the value; must be finite with `min <= max`.
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }
    /// Animates an indeterminate bar from the view's frame clock. It requests the
    /// next animation frame on every evaluation while indeterminate, so the owning
    /// view is evaluated each frame. With reduced motion it shows a centered static
    /// segment; a determinate bar requests nothing.
    pub fn animate<T>(mut self, cx: &ViewContext<'_, T>) -> Self {
        if self.value.is_none() && !cx.reduce_motion() {
            self.phase = Some((cx.frame_time().as_secs_f32() / SWEEP).fract());
            cx.request_animation_frame();
        }
        self
    }
    /// Fixed logical height.
    pub fn height(mut self, height: f32) -> Self {
        self.root = self.root.height(height);
        self
    }
}
root_builders!(ProgressBar);

impl IntoElement for ProgressBar {
    fn into_element(self) -> Element {
        let mut root = self.root;
        let segment = row()
            .key("fill")
            .fill_height()
            .background(ThemeColor::Accent);
        match self.value {
            Some(value) => {
                let valid = [value, self.min, self.max].iter().all(|v| v.is_finite())
                    && self.min <= self.max;
                let value = if valid {
                    value.clamp(self.min, self.max)
                } else {
                    value
                };
                let fraction = if valid && self.max > self.min {
                    (value - self.min) / (self.max - self.min)
                } else {
                    0.
                };
                root = root
                    .accessibility_numeric_value(NumericValue {
                        value,
                        min: self.min,
                        max: self.max,
                        step: None,
                    })
                    .child(segment.width_percent(fraction));
            }
            None => {
                // The segment enters from the left edge and leaves past the right.
                let start = self
                    .phase
                    .map_or(0.5 - SEGMENT / 2., |p| p * (1. + SEGMENT) - SEGMENT);
                root = root.accessibility_value("Indeterminate").child(
                    segment
                        .absolute()
                        .top(0.)
                        .bottom(0.)
                        .width_percent(SEGMENT)
                        .layout(|s| s.inset.left = taffy::LengthPercentageAuto::percent(start)),
                );
            }
        }
        root
    }
}

/// Spinning activity indicator, 20 logical units square, painted in the accent
/// color and named "Loading". It requests an animation frame on every evaluation,
/// so the owning view is evaluated each frame while the spinner is shown; remove
/// it when the work finishes. Reduced motion keeps a static indicator and requests
/// no frames. Element builders restyle or rename it.
///
/// ```
/// use rxui::prelude::*;
/// struct Search { busy: bool }
/// impl View for Search {
///     fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
///         let mut status = row().gap(8.).child(label("Searching"));
///         if self.busy {
///             status = status.child(spinner(cx).accessibility_label("Searching"));
///         }
///         status
///     }
/// }
/// ```
pub fn spinner<T>(cx: &ViewContext<'_, T>) -> Element {
    if !cx.reduce_motion() {
        cx.request_animation_frame();
    }
    // One spoke per step, a full turn per second.
    let head = if cx.reduce_motion() {
        0
    } else {
        (cx.frame_time().as_secs_f32() * f32::from(SPOKES)) as u64 % u64::from(SPOKES)
    };
    glyph(Glyph::Spinner { head: head as u8 }, 20.)
        .color(ThemeColor::Accent)
        .accessibility_hidden(false)
        .accessibility_role(SemanticRole::ProgressIndicator)
        .accessibility_label("Loading")
}
