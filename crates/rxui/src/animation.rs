//! Deterministic animation primitives driven by the host's monotonic frame time.
//!
//! Nothing here reads a clock. Every value is a pure function of the frame time
//! passed to `Runtime::begin_frame` (exposed to views as `ViewContext::frame_time`),
//! so animations are reproducible in headless tests. See `docs/animation.md`.
//!
//! ```
//! use rxui::{Animated, Easing, Transition};
//! use std::time::Duration;
//! let ms = Duration::from_millis;
//! let mut width = Animated::new(100_f32);
//! width.set(200., Transition::tween(ms(100), Easing::Linear), ms(1000));
//! assert_eq!(width.value(ms(1050)), 150.);
//! assert!(width.is_animating(ms(1050)));
//! assert_eq!(width.value(ms(1100)), 200.);
//! assert!(!width.is_animating(ms(1100)));
//! ```
use std::time::Duration;

/// Linear RGBA, as [`crate::Color`] in the layout feature.
type Color = [f32; 4];

/// Timing curve mapping linear progress in `0..=1` to eased progress. Curves start
/// at zero and end at one; cubic Bézier curves may overshoot in between.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Easing {
    /// Constant rate.
    Linear,
    /// Slow start, `cubic-bezier(0.42, 0, 1, 1)`.
    EaseIn,
    /// Slow end, `cubic-bezier(0, 0, 0.58, 1)`.
    EaseOut,
    /// Slow start and end, `cubic-bezier(0.42, 0, 0.58, 1)`.
    EaseInOut,
    /// Quick response that settles gently, `cubic-bezier(0.2, 0, 0, 1)`. The default
    /// for state transitions and overlays.
    #[default]
    Standard,
    /// CSS-style `cubic-bezier(x1, y1, x2, y2)`. X control points are clamped to
    /// `0..=1` so the curve stays a function of time; Y values may overshoot.
    CubicBezier(f32, f32, f32, f32),
}
impl Easing {
    /// Eased progress at linear progress `t`, which is clamped to `0..=1`.
    pub fn apply(self, t: f32) -> f32 {
        let t = if t.is_nan() { 1. } else { t.clamp(0., 1.) };
        let (x1, y1, x2, y2) = match self {
            Self::Linear => return t,
            Self::EaseIn => (0.42, 0., 1., 1.),
            Self::EaseOut => (0., 0., 0.58, 1.),
            Self::EaseInOut => (0.42, 0., 0.58, 1.),
            Self::Standard => (0.2, 0., 0., 1.),
            Self::CubicBezier(x1, y1, x2, y2) => (x1, y1, x2, y2),
        };
        if t == 0. || t == 1. {
            return t;
        }
        let finite = |v: f32, default: f32| if v.is_finite() { v } else { default };
        let x1 = finite(x1, 0.).clamp(0., 1.);
        let x2 = finite(x2, 1.).clamp(0., 1.);
        bezier(bezier_parameter(t, x1, x2), finite(y1, 0.), finite(y2, 1.))
    }
}
/// One coordinate of a cubic Bézier from 0 to 1 with control values `a` and `b`.
fn bezier(s: f32, a: f32, b: f32) -> f32 {
    let r = 1. - s;
    3. * r * r * s * a + 3. * r * s * s * b + s * s * s
}
/// Curve parameter whose x coordinate is `x`: Newton's method, then bisection.
fn bezier_parameter(x: f32, x1: f32, x2: f32) -> f32 {
    let mut s = x;
    for _ in 0..8 {
        let error = bezier(s, x1, x2) - x;
        if error.abs() < 1e-6 {
            return s;
        }
        let r = 1. - s;
        let slope = 3. * r * r * x1 + 6. * r * s * (x2 - x1) + 3. * s * s * (1. - x2);
        if slope.abs() < 1e-6 {
            break;
        }
        s -= error / slope;
    }
    let (mut low, mut high) = (0_f32, 1_f32);
    s = x;
    for _ in 0..32 {
        let value = bezier(s, x1, x2);
        if (value - x).abs() < 1e-6 {
            break;
        }
        if value < x {
            low = s;
        } else {
            high = s;
        }
        s = (low + high) * 0.5;
    }
    s
}

/// Damped spring for transitions with physical feel. It is evaluated analytically
/// from the elapsed time, so results do not depend on the frame rate.
///
/// ```
/// use rxui::Spring;
/// use std::time::Duration;
/// let spring = Spring::critically_damped(Duration::from_millis(300));
/// assert_eq!(spring.damping_ratio(), 1.);
/// let (position, _) = spring.sample(10., 0.);
/// assert!((position - 1.).abs() < 1e-4);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    /// Undamped angular frequency, radians per second.
    omega: f32,
    /// Damping ratio: 1 is critical, lower values overshoot.
    zeta: f32,
}
impl Default for Spring {
    /// A critically damped spring with a 300 ms response.
    fn default() -> Self {
        Self::critically_damped(Duration::from_millis(300))
    }
}
impl Spring {
    /// Smallest damping ratio; lower values would oscillate for a long time.
    pub const MIN_DAMPING_RATIO: f32 = 0.1;
    /// A spring whose undamped period is `response`, with a damping ratio (1 is
    /// critically damped, lower values overshoot, higher values approach slowly).
    /// The ratio is clamped to at least [`Self::MIN_DAMPING_RATIO`]; a zero or
    /// invalid response gives an instant spring.
    pub fn new(response: Duration, damping_ratio: f32) -> Self {
        let seconds = response.as_secs_f32();
        let omega = if seconds > 0. {
            std::f32::consts::TAU / seconds
        } else {
            f32::INFINITY
        };
        let zeta = if damping_ratio.is_finite() {
            damping_ratio.max(Self::MIN_DAMPING_RATIO)
        } else {
            1.
        };
        Self { omega, zeta }
    }
    /// The fastest spring that does not overshoot, with the given response.
    pub fn critically_damped(response: Duration) -> Self {
        Self::new(response, 1.)
    }
    /// A spring from physical parameters: stiffness `k`, damping coefficient `c`
    /// and mass `m`, all positive.
    pub fn from_physics(stiffness: f32, damping: f32, mass: f32) -> Self {
        if !(stiffness > 0. && mass > 0. && damping.is_finite()) {
            return Self::new(Duration::ZERO, 1.);
        }
        let omega = (stiffness / mass).sqrt();
        let zeta = damping / (2. * (stiffness * mass).sqrt());
        Self {
            omega,
            zeta: zeta.max(Self::MIN_DAMPING_RATIO),
        }
    }
    /// Undamped period.
    pub fn response(&self) -> Duration {
        Duration::from_secs_f32(std::f32::consts::TAU / self.omega)
    }
    /// Damping ratio.
    pub fn damping_ratio(&self) -> f32 {
        self.zeta
    }
    /// Position and velocity, in progress units, `elapsed` seconds into a move from
    /// 0 to 1 that started with `velocity` progress units per second.
    pub fn sample(&self, elapsed: f32, velocity: f32) -> (f32, f32) {
        if !self.omega.is_finite() || !elapsed.is_finite() {
            return (1., 0.);
        }
        let t = elapsed.max(0.);
        let (w, z) = (self.omega, self.zeta);
        let d0 = -1_f32;
        // Displacement from the target and its derivative.
        let (d, v) = if (z - 1.).abs() < 1e-4 {
            let b = velocity + w * d0;
            let e = (-w * t).exp();
            ((d0 + b * t) * e, (b - w * (d0 + b * t)) * e)
        } else if z < 1. {
            let wd = w * (1. - z * z).sqrt();
            let b = (velocity + z * w * d0) / wd;
            let e = (-z * w * t).exp();
            let (sin, cos) = (wd * t).sin_cos();
            let d = e * (d0 * cos + b * sin);
            (d, -z * w * d + e * (-d0 * wd * sin + b * wd * cos))
        } else {
            let root = (z * z - 1.).sqrt();
            let (r1, r2) = (-w * (z - root), -w * (z + root));
            let c2 = (velocity - r1 * d0) / (r2 - r1);
            let c1 = d0 - c2;
            let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
            (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
        };
        (1. + d, v)
    }
    /// Whether the move has settled within 0.1% of its distance, nearly at rest.
    fn settled(&self, elapsed: f32, velocity: f32) -> bool {
        let (position, velocity) = self.sample(elapsed, velocity);
        (position - 1.).abs() < 1e-3 && velocity.abs() < 1e-2
    }
}

/// How a value moves to a new target. Elements and overlays default to
/// [`Transition::Theme`]; `Transition::None` snaps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Transition {
    /// Change immediately.
    None,
    /// The theme's duration for the context with [`Easing::Standard`]: state changes
    /// use [`ThemeMotion::state`], overlays their enter/exit durations. Outside a
    /// theme, [`ThemeMotion::default`] supplies the duration.
    #[default]
    Theme,
    /// A fixed duration and curve.
    Tween {
        /// Total time.
        duration: Duration,
        /// Timing curve.
        easing: Easing,
    },
    /// A damped spring; it ends when settled rather than after a fixed time.
    Spring(Spring),
}
impl Transition {
    /// A fixed duration with [`Easing::Standard`].
    pub fn ease(duration: Duration) -> Self {
        Self::tween(duration, Easing::Standard)
    }
    /// A fixed duration at constant rate.
    pub fn linear(duration: Duration) -> Self {
        Self::tween(duration, Easing::Linear)
    }
    /// A fixed duration and curve.
    pub fn tween(duration: Duration, easing: Easing) -> Self {
        Self::Tween { duration, easing }
    }
    /// A damped spring.
    pub fn spring(spring: Spring) -> Self {
        Self::Spring(spring)
    }
    /// The timing this transition uses, with `theme` supplying the duration of
    /// [`Self::Theme`]. `None` when the change is immediate.
    pub(crate) fn timing(self, theme: Duration) -> Option<Timing> {
        match self {
            Self::None => None,
            Self::Theme => (!theme.is_zero()).then_some(Timing::Tween(theme, Easing::Standard)),
            Self::Tween { duration, easing } => {
                (!duration.is_zero()).then_some(Timing::Tween(duration, easing))
            }
            Self::Spring(spring) => spring.omega.is_finite().then_some(Timing::Spring(spring)),
        }
    }
}
/// A resolved, non-instant transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Timing {
    Tween(Duration, Easing),
    Spring(Spring),
}
impl Timing {
    /// Progress (possibly overshooting for springs) and whether the move has ended,
    /// `elapsed` after it started with `velocity` progress units per second.
    pub(crate) fn progress(&self, elapsed: Duration, velocity: f32) -> (f32, bool) {
        match *self {
            Self::Tween(duration, easing) => {
                if elapsed >= duration {
                    (1., true)
                } else {
                    let t = elapsed.as_secs_f32() / duration.as_secs_f32();
                    (easing.apply(t), false)
                }
            }
            Self::Spring(spring) => {
                let seconds = elapsed.as_secs_f32();
                if spring.settled(seconds, velocity) {
                    (1., true)
                } else {
                    (spring.sample(seconds, velocity).0, false)
                }
            }
        }
    }
    /// Progress velocity, per second; only springs carry it across retargets.
    fn velocity(&self, elapsed: Duration, velocity: f32) -> f32 {
        match self {
            Self::Tween(..) => 0.,
            Self::Spring(spring) => spring.sample(elapsed.as_secs_f32(), velocity).1,
        }
    }
}

/// Theme durations for implicit motion, part of [`crate::ThemeMetrics`].
/// A zero duration disables that kind of motion.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeMotion {
    /// Hover, pressed, focus, disabled, selection and theme changes of retained paint.
    pub state: Duration,
    /// Overlay fade-in after opening.
    pub overlay_enter: Duration,
    /// Overlay fade-out after closing; the visual lingers this long.
    pub overlay_exit: Duration,
    /// Logical distance an anchored popover slides toward its anchor side while
    /// entering and exiting. Modal dialogs only fade.
    pub overlay_offset: f32,
    /// Smooth scrolling of discrete wheel steps and programmatic scroll commands,
    /// with [`Easing::EaseOut`]; see `ScrollBehavior`.
    pub scroll: Duration,
}
impl Default for ThemeMotion {
    /// 120 ms state changes, 150 ms overlay entrance, 100 ms exit, a 4-unit slide and
    /// 150 ms smooth scrolling.
    fn default() -> Self {
        Self {
            state: Duration::from_millis(120),
            overlay_enter: Duration::from_millis(150),
            overlay_exit: Duration::from_millis(100),
            overlay_offset: 4.,
            scroll: Duration::from_millis(150),
        }
    }
}

/// Values that can be blended for animation. `t` is eased progress; springs can
/// pass values slightly outside `0..=1`.
///
/// Implementations exist for `f32`, `[f32; 2]` offsets, [`crate::Color`] (each channel is
/// clamped to `0..=1`, so overshoot stays a valid color), `Option<Color>` (absence
/// fades through transparent), [`crate::Bounds`], [`crate::ResolvedShadow`] and
/// [`crate::ResolvedPaint`].
pub trait Interpolate: Clone + PartialEq {
    /// The value `t` of the way from `self` to `to`.
    fn interpolate(&self, to: &Self, t: f32) -> Self;
    /// Up to four numeric components, used to carry a spring's velocity into a new
    /// target. Values that return `None` restart from rest when retargeted.
    fn components(&self) -> Option<[f32; 4]> {
        None
    }
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
impl Interpolate for f32 {
    fn interpolate(&self, to: &Self, t: f32) -> Self {
        lerp(*self, *to, t)
    }
    fn components(&self) -> Option<[f32; 4]> {
        Some([*self, 0., 0., 0.])
    }
}
impl Interpolate for [f32; 2] {
    fn interpolate(&self, to: &Self, t: f32) -> Self {
        [lerp(self[0], to[0], t), lerp(self[1], to[1], t)]
    }
    fn components(&self) -> Option<[f32; 4]> {
        Some([self[0], self[1], 0., 0.])
    }
}
impl Interpolate for Color {
    fn interpolate(&self, to: &Self, t: f32) -> Self {
        std::array::from_fn(|i| lerp(self[i], to[i], t).clamp(0., 1.))
    }
    fn components(&self) -> Option<[f32; 4]> {
        Some(*self)
    }
}
impl Interpolate for Option<Color> {
    fn interpolate(&self, to: &Self, t: f32) -> Self {
        let clear = |c: &Color| [c[0], c[1], c[2], 0.];
        match (self, to) {
            (Some(a), Some(b)) => Some(a.interpolate(b, t)),
            (Some(a), None) => (t < 1.).then(|| a.interpolate(&clear(a), t)),
            (None, Some(b)) => Some(clear(b).interpolate(b, t)),
            (None, None) => None,
        }
    }
}

#[cfg(feature = "layout")]
impl Interpolate for crate::Bounds {
    fn interpolate(&self, to: &Self, t: f32) -> Self {
        Self {
            x: lerp(self.x, to.x, t),
            y: lerp(self.y, to.y, t),
            width: lerp(self.width, to.width, t).max(0.),
            height: lerp(self.height, to.height, t).max(0.),
        }
    }
    fn components(&self) -> Option<[f32; 4]> {
        Some([self.x, self.y, self.width, self.height])
    }
}
#[cfg(feature = "layout")]
impl Interpolate for crate::ResolvedShadow {
    fn interpolate(&self, to: &Self, t: f32) -> Self {
        Self {
            offset: self.offset.interpolate(&to.offset, t),
            blur: lerp(self.blur, to.blur, t).max(0.),
            spread: lerp(self.spread, to.spread, t),
            color: self.color.interpolate(&to.color, t),
        }
    }
}
#[cfg(feature = "layout")]
impl Interpolate for Option<crate::ResolvedShadow> {
    fn interpolate(&self, to: &Self, t: f32) -> Self {
        let clear = |s: &crate::ResolvedShadow| crate::ResolvedShadow {
            color: [s.color[0], s.color[1], s.color[2], 0.],
            ..*s
        };
        match (self, to) {
            (Some(a), Some(b)) => Some(a.interpolate(b, t)),
            (Some(a), None) => (t < 1.).then(|| a.interpolate(&clear(a), t)),
            (None, Some(b)) => Some(clear(b).interpolate(b, t)),
            (None, None) => None,
        }
    }
}
/// Blends every color, the radii, the focus width and the shadow. Progress is
/// clamped to `0..=1`, so paint never overshoots.
#[cfg(feature = "layout")]
impl Interpolate for crate::ResolvedPaint {
    fn interpolate(&self, to: &Self, t: f32) -> Self {
        let t = t.clamp(0., 1.);
        Self {
            color: self.color.interpolate(&to.color, t),
            background: self.background.interpolate(&to.background, t),
            border_color: self.border_color.interpolate(&to.border_color, t),
            radii: std::array::from_fn(|i| lerp(self.radii[i], to.radii[i], t).max(0.)),
            focus_color: self.focus_color.interpolate(&to.focus_color, t),
            focus_width: lerp(self.focus_width, to.focus_width, t).max(0.),
            focus_offset: lerp(self.focus_offset, to.focus_offset, t),
            selection_color: self.selection_color.interpolate(&to.selection_color, t),
            selection_text_color: self
                .selection_text_color
                .interpolate(&to.selection_text_color, t),
            caret_color: self.caret_color.interpolate(&to.caret_color, t),
            preedit_color: self.preedit_color.interpolate(&to.preedit_color, t),
            shadow: self.shadow.interpolate(&to.shadow, t),
        }
    }
}

/// A value that moves toward its latest target over time. Store one in a view's
/// state, or let `ViewContext::animate` keep one per placement. All methods take
/// the current frame time; no clock is read.
#[derive(Clone, Debug, PartialEq)]
pub struct Animated<T> {
    from: T,
    to: T,
    start: Duration,
    /// The move waits for the next frame time to start; see [`Self::defer`].
    pending: bool,
    timing: Option<Timing>,
    /// Initial progress velocity of the current move, per second.
    velocity: f32,
}
impl<T: Interpolate> Animated<T> {
    /// A settled value.
    pub fn new(value: T) -> Self {
        Self {
            from: value.clone(),
            to: value,
            start: Duration::ZERO,
            pending: false,
            timing: None,
            velocity: 0.,
        }
    }
    /// Time into the current move; zero while it waits for a frame.
    fn elapsed(&self, now: Duration) -> Duration {
        if self.pending {
            Duration::ZERO
        } else {
            now.saturating_sub(self.start)
        }
    }
    /// Makes the latest move wait at its starting value until [`Self::resume`],
    /// for changes observed between frames at a stale frame time.
    pub(crate) fn defer(&mut self) {
        self.pending = self.timing.is_some();
    }
    /// Starts a deferred move at frame time `now`.
    pub(crate) fn resume(&mut self, now: Duration) {
        if self.pending {
            self.pending = false;
            self.start = now;
        }
    }
    /// The latest target.
    pub fn target(&self) -> &T {
        &self.to
    }
    /// The value at frame time `now`.
    pub fn value(&self, now: Duration) -> T {
        match &self.timing {
            Some(timing) => {
                let (t, done) = timing.progress(self.elapsed(now), self.velocity);
                if done {
                    self.to.clone()
                } else {
                    self.from.interpolate(&self.to, t)
                }
            }
            None => self.to.clone(),
        }
    }
    /// Whether the value is still moving at frame time `now`. Hosts need further
    /// frames only while this holds.
    pub fn is_animating(&self, now: Duration) -> bool {
        self.timing
            .is_some_and(|timing| !timing.progress(self.elapsed(now), self.velocity).1)
    }
    /// Moves toward `target` from the value at `now`, keeping the motion continuous
    /// when retargeted mid-flight. A spring also keeps its velocity along the new
    /// direction for values with [`Interpolate::components`]. Setting the current
    /// target again changes nothing. [`Transition::Theme`] uses
    /// [`ThemeMotion::default`]'s state duration.
    pub fn set(&mut self, target: T, transition: Transition, now: Duration) {
        if target == self.to {
            return;
        }
        let Some(timing) = transition.timing(ThemeMotion::default().state) else {
            return self.jump_to(target);
        };
        let current = self.value(now);
        let mut velocity = 0.;
        if let (Some(old), true) = (self.timing, self.is_animating(now)) {
            let progress = old.velocity(self.elapsed(now), self.velocity);
            if let (Some(a), Some(b), Some(c), Some(d)) = (
                self.from.components(),
                self.to.components(),
                current.components(),
                target.components(),
            ) {
                // Value-space velocity projected onto the new direction.
                let old: [f32; 4] = std::array::from_fn(|i| (b[i] - a[i]) * progress);
                let new: [f32; 4] = std::array::from_fn(|i| d[i] - c[i]);
                let length = new.iter().map(|v| v * v).sum::<f32>();
                if length > 0. {
                    velocity = old.iter().zip(&new).map(|(a, b)| a * b).sum::<f32>() / length;
                }
            }
        }
        self.from = current;
        self.to = target;
        self.start = now;
        self.pending = false;
        self.timing = Some(timing);
        self.velocity = if velocity.is_finite() { velocity } else { 0. };
    }
    /// Jumps to `value` without animating.
    pub fn jump_to(&mut self, value: T) {
        *self = Self::new(value);
    }
}

/// Runtime-wide motion state: whether a host drives the frame clock, and the
/// reduce-motion preference.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Clock {
    /// `Runtime::begin_frame` has been called at least once.
    pub(crate) started: bool,
    /// Transitions complete immediately.
    pub(crate) reduce_motion: bool,
    /// Serial of the latest `Runtime::begin_frame`.
    pub(crate) frame: u64,
    /// Frame whose first `Ui::prepare` has completed. Preparations after it, such
    /// as input handling between frames, see a stale frame time.
    pub(crate) consumed: u64,
}
impl Clock {
    /// Whether the current frame time is fresh: no preparation has completed since
    /// the latest `Runtime::begin_frame`. Motion created otherwise starts at the
    /// next frame instead of a stale time.
    pub(crate) fn fresh(self) -> bool {
        self.frame != self.consumed
    }
    /// Whether transitions animate: a frame clock runs and motion is not reduced.
    pub(crate) fn animates(self) -> bool {
        self.started && !self.reduce_motion
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }
    #[test]
    fn easing_curves_are_monotonic_and_pinned_at_the_ends() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
            Easing::Standard,
            Easing::CubicBezier(0.3, 0.1, 0.7, 0.9),
        ] {
            assert_eq!(easing.apply(0.), 0.);
            assert_eq!(easing.apply(1.), 1.);
            assert_eq!(easing.apply(-1.), 0.);
            assert_eq!(easing.apply(2.), 1.);
            let mut last = 0.;
            for i in 1..=100 {
                let v = easing.apply(i as f32 / 100.);
                assert!(v >= last - 1e-5, "{easing:?} at {i}");
                last = v;
            }
        }
        // Known values: ease-in-out is symmetric, and a linear Bézier is the identity.
        assert!((Easing::EaseInOut.apply(0.5) - 0.5).abs() < 1e-4);
        assert!((Easing::CubicBezier(0.25, 0.25, 0.75, 0.75).apply(0.3) - 0.3).abs() < 1e-4);
        assert!(Easing::EaseIn.apply(0.25) < 0.25 && Easing::EaseOut.apply(0.25) > 0.25);
        // CSS `ease` at x = 0.5 is about 0.8024.
        assert!((Easing::CubicBezier(0.25, 0.1, 0.25, 1.).apply(0.5) - 0.8024).abs() < 1e-3);
        // Y overshoot is allowed; invalid X control points are clamped.
        assert!(Easing::CubicBezier(0.3, 1.6, 0.7, 1.4).apply(0.5) > 1.);
        assert!(Easing::CubicBezier(-3., 0., 9., 1.).apply(0.5).is_finite());
        assert_eq!(Easing::Standard.apply(f32::NAN), 1.);
    }
    #[test]
    fn springs_settle_without_overshoot_when_critically_damped() {
        let spring = Spring::critically_damped(ms(300));
        assert_eq!(spring.sample(0., 0.), (0., 0.));
        let mut last = 0.;
        for i in 1..200 {
            let (p, _) = spring.sample(i as f32 * 0.005, 0.);
            assert!(p >= last && p <= 1. + 1e-6);
            last = p;
        }
        let timing = Timing::Spring(spring);
        assert!(!timing.progress(ms(100), 0.).1);
        assert_eq!(timing.progress(ms(2000), 0.), (1., true));
    }
    #[test]
    fn underdamped_and_overdamped_springs_match_their_physics() {
        let bouncy = Spring::new(ms(400), 0.3);
        let peak = (0..400)
            .map(|i| bouncy.sample(i as f32 * 0.002, 0.).0)
            .fold(0_f32, f32::max);
        assert!(peak > 1.2, "overshoot {peak}");
        let slow = Spring::new(ms(300), 2.);
        let critical = Spring::critically_damped(ms(300));
        assert!(slow.sample(0.1, 0.).0 < critical.sample(0.1, 0.).0);
        // Numerical derivative agrees with the analytic velocity in every regime.
        for spring in [bouncy, slow, critical] {
            for t in [0.01_f32, 0.05, 0.2] {
                let h = 1e-3;
                let numeric = (spring.sample(t + h, 2.).0 - spring.sample(t - h, 2.).0) / (2. * h);
                let analytic = spring.sample(t, 2.).1;
                assert!((numeric - analytic).abs() < 0.05, "{spring:?} {t}");
            }
        }
        let physics = Spring::from_physics(100., 20., 1.);
        assert!((physics.damping_ratio() - 1.).abs() < 1e-6);
        assert!((physics.response().as_secs_f32() - std::f32::consts::TAU / 10.).abs() < 1e-4);
        assert_eq!(
            Spring::new(ms(300), 0.).damping_ratio(),
            Spring::MIN_DAMPING_RATIO
        );
        assert_eq!(
            Transition::spring(Spring::new(Duration::ZERO, 1.)).timing(ms(1)),
            None
        );
    }
    #[test]
    fn animated_values_settle_and_retarget_continuously() {
        let mut value = Animated::new(0_f32);
        assert!(!value.is_animating(ms(0)));
        value.set(10., Transition::linear(ms(100)), ms(1000));
        assert_eq!(value.value(ms(1000)), 0.);
        assert_eq!(value.value(ms(1025)), 2.5);
        // Retargeting starts from the current value, without a jump.
        value.set(0., Transition::linear(ms(100)), ms(1050));
        assert_eq!(value.value(ms(1050)), 5.);
        assert_eq!(value.value(ms(1100)), 2.5);
        assert!(value.is_animating(ms(1149)));
        assert!(!value.is_animating(ms(1150)));
        assert_eq!(value.value(ms(5000)), 0.);
        // The same target is a no-op; Transition::None jumps.
        value.set(0., Transition::linear(ms(100)), ms(6000));
        assert!(!value.is_animating(ms(6000)));
        value.set(3., Transition::None, ms(6000));
        assert_eq!(
            (value.value(ms(6000)), value.is_animating(ms(6000))),
            (3., false)
        );
        value.set(4., Transition::Theme, ms(7000));
        assert!(value.is_animating(ms(7000) + ThemeMotion::default().state / 2));
        assert!(!value.is_animating(ms(7000) + ThemeMotion::default().state));
    }
    #[test]
    fn retargeted_springs_keep_their_velocity() {
        let spring = Transition::spring(Spring::critically_damped(ms(400)));
        let mut value = Animated::new(0_f32);
        value.set(100., spring, ms(0));
        let before = (value.value(ms(80)) - value.value(ms(79))) * 1000.;
        // Extend the target in the same direction mid-flight.
        value.set(200., spring, ms(80));
        let after = (value.value(ms(81)) - value.value(ms(80))) * 1000.;
        assert!((after - before).abs() / before < 0.05, "{before} {after}");
        let mut settled = ms(80);
        while value.is_animating(settled) {
            settled += ms(16);
        }
        assert!((value.value(settled) - 200.).abs() < 1e-3);
        assert!(settled < ms(3000));
    }
    #[test]
    fn colors_clamp_and_absent_colors_fade_through_transparent() {
        let a = [0., 0.5, 1., 1.];
        let b = [1., 0.5, 0., 0.];
        assert_eq!(a.interpolate(&b, 0.5), [0.5, 0.5, 0.5, 0.5]);
        assert_eq!(a.interpolate(&b, 1.5), [1., 0.5, 0., 0.]);
        assert_eq!(Some(a).interpolate(&None, 0.5), Some([0., 0.5, 1., 0.5]));
        assert_eq!(Some(a).interpolate(&None, 1.), None);
        assert_eq!(None.interpolate(&Some(a), 0.25), Some([0., 0.5, 1., 0.25]));
        assert_eq!([0., 4.].interpolate(&[2., 0.], 0.5), [1., 2.]);
    }
}
