use crate::{SurfaceSettings, WindowError, WindowMetrics};
use rxui_draw::{Frame, FrameError, GraphicsContext, RenderFormat, RenderTarget};
use std::sync::Arc;
use winit::{
    dpi::PhysicalSize,
    event::WindowEvent,
    window::{Window, WindowId},
};

// Device invalidation is independent of OS suspension. Only explicit successful
// replacement clears it, including when replacement happens while suspended.
#[derive(Default)]
struct DeviceLifecycle {
    active: bool,
    invalidated: bool,
}
impl DeviceLifecycle {
    fn unavailable(&self, observed_loss: bool) -> bool {
        self.invalidated || observed_loss
    }
    fn suspend(&mut self) {
        self.active = false;
    }
    fn resume(&mut self) {
        self.active = true;
    }
    fn invalidate(&mut self) {
        self.invalidated = true;
    }
    fn replaced(&mut self) {
        self.invalidated = false;
    }
}

/// Changes from a native event, useful for invalidating an application-owned loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowUpdate {
    /// Physical size or logical scale changed.
    pub metrics_changed: bool,
    /// Presentation eligibility changed, for example after exposure.
    pub availability_changed: bool,
    /// Native geometry/exposure requests a probe even if cached state is unchanged.
    pub redraw_requested: bool,
}
impl WindowUpdate {
    /// Whether this update warrants a new draw when presentation is available.
    pub const fn needs_redraw(self) -> bool {
        self.metrics_changed || self.availability_changed || self.redraw_requested
    }
}

/// Native window and its recoverable RXUI drawing surface, independent of a runner.
///
/// GPU resources and requested attachments survive suspension. A surface is
/// recreated only after resume or loss; resize updates the existing target.
/// The target is exposed read-only so configuration cannot diverge from recovery
/// settings. Frame acquisition borrows this context until recording ends.
pub struct WindowContext {
    pub(crate) window: Arc<Window>,
    pub(crate) graphics: GraphicsContext,
    pub(crate) target: Option<RenderTarget<'static>>,
    pub(crate) format: RenderFormat,
    metrics: WindowMetrics,
    settings: SurfaceSettings,
    supported_samples: Vec<u32>,
    generation: u64,
    lifecycle: DeviceLifecycle,
    visible: bool,
    occluded: bool,
    pub(crate) defer_show: bool,
    pub(crate) invalidated: bool,
    pub(crate) availability_invalidated: bool,
}
impl WindowContext {
    /// Selects a compatible adapter/device asynchronously for this first surface.
    /// Metrics are reconciled after GPU initialization, which may take time.
    pub async fn new(window: Arc<Window>, settings: SurfaceSettings) -> Result<Self, WindowError> {
        let metrics = WindowMetrics::from_window(&window)?;
        let (graphics, target) = GraphicsContext::with_surface(
            window.clone(),
            settings.surface_options(metrics.physical_size()),
        )
        .await?;
        Self::from_target(graphics, window, settings, target)
    }
    /// Selects graphics using a native display connection, required for GLES presentation.
    pub async fn with_display(
        window: Arc<Window>,
        settings: SurfaceSettings,
        display: winit::event_loop::OwnedDisplayHandle,
    ) -> Result<Self, WindowError> {
        let metrics = WindowMetrics::from_window(&window)?;
        let instance = rxui_draw::wgpu::Instance::new(
            rxui_draw::wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display))
                .with_env(),
        );
        let (graphics, target) = GraphicsContext::with_surface_instance(
            &instance,
            window.clone(),
            settings.surface_options(metrics.physical_size()),
        )
        .await?;
        Self::from_target(graphics, window, settings, target)
    }

    /// Creates another window surface on an explicitly selected/shared device.
    /// Incompatible surfaces fail rather than silently selecting another device.
    pub fn with_graphics(
        graphics: &GraphicsContext,
        window: Arc<Window>,
        settings: SurfaceSettings,
    ) -> Result<Self, WindowError> {
        if graphics.is_device_lost() {
            return Err(WindowError::DeviceUnavailable);
        }
        let metrics = WindowMetrics::from_window(&window)?;
        let target = graphics.create_surface(
            window.clone(),
            settings.surface_options(metrics.physical_size()),
        )?;
        Self::from_target(graphics.clone(), window, settings, target)
    }
    fn from_target(
        graphics: GraphicsContext,
        window: Arc<Window>,
        settings: SurfaceSettings,
        mut target: RenderTarget<'static>,
    ) -> Result<Self, WindowError> {
        let metrics = WindowMetrics::from_window(&window)?;
        let size = metrics.physical_size();
        target.resize(size.width, size.height)?;
        let format = target.render_format();
        let supported_samples = target.supported_sample_counts().to_vec();
        let visible = window.is_visible().unwrap_or(true);
        Ok(Self {
            window,
            graphics,
            target: Some(target),
            format,
            metrics,
            settings,
            supported_samples,
            generation: 1,
            lifecycle: DeviceLifecycle {
                active: true,
                invalidated: false,
            },
            visible,
            occluded: false,
            defer_show: false,
            invalidated: true,
            availability_invalidated: false,
        })
    }
    /// Native handle for title, cursor, IME, accessibility, and platform APIs.
    pub fn window(&self) -> &Arc<Window> {
        &self.window
    }
    /// Device shared by this window's attachments and application resources.
    pub fn graphics(&self) -> &GraphicsContext {
        &self.graphics
    }
    /// Cached inner dimensions and density; performs no native query.
    pub fn metrics(&self) -> WindowMetrics {
        self.metrics
    }
    /// Requested settings retained across surface replacement.
    pub fn settings(&self) -> SurfaceSettings {
        self.settings
    }
    /// Live target, absent during suspension or failed recreation.
    pub fn target(&self) -> Option<&RenderTarget<'static>> {
        self.target.as_ref()
    }
    /// Cached live attachment compatibility; also available for zero-sized targets.
    pub fn render_format(&self) -> Option<&RenderFormat> {
        self.target.as_ref().map(|_| &self.format)
    }
    /// Increments on successful surface replacement, not resize or MSAA changes.
    pub fn surface_generation(&self) -> u64 {
        self.generation
    }
    /// Cached native eligibility, without attempting swapchain acquisition.
    pub fn can_present(&self) -> bool {
        self.can_recover() && self.target.is_some()
    }
    pub(crate) fn can_recover(&self) -> bool {
        self.lifecycle.active
            && !self.device_unavailable()
            && self.visible
            && !self.occluded
            && self.metrics.has_area()
    }
    /// Updates cached resize/DPI/occlusion state before forwarding a native event.
    /// Unchanged size notifications do not replace attachments.
    pub fn process_event(&mut self, event: &WindowEvent) -> Result<WindowUpdate, WindowError> {
        let before = self.can_present();
        let next = match event {
            WindowEvent::Resized(size) => {
                Some(WindowMetrics::new(*size, self.metrics.scale_factor())?)
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // winit sends this before applying its suggested size, so inner_size()
                // still reports the old size on Windows, and the writer cannot be read.
                // Predict the platforms' suggestion: logical size is preserved unless
                // maximized or fullscreen. A differing final size arrives as Resized.
                let size = if self.window.is_maximized() || self.window.fullscreen().is_some() {
                    self.metrics.physical_size()
                } else {
                    rescaled_size(
                        self.metrics.physical_size(),
                        self.metrics.scale_factor(),
                        *scale_factor,
                    )
                };
                Some(WindowMetrics::new(size, *scale_factor)?)
            }
            _ => None,
        };
        let metrics_changed = next.is_some_and(|m| m != self.metrics);
        if let Some(metrics) = next
            && metrics_changed
        {
            if !self.device_unavailable()
                && let Some(target) = &mut self.target
            {
                let size = metrics.physical_size();
                target.resize(size.width, size.height)?;
            }
            self.metrics = metrics;
        }
        if let WindowEvent::Occluded(value) = event {
            self.occluded = *value;
        }
        let update = WindowUpdate {
            metrics_changed,
            availability_changed: before != self.can_present(),
            redraw_requested: matches!(
                event,
                WindowEvent::Occluded(false)
                    | WindowEvent::Resized(_)
                    | WindowEvent::ScaleFactorChanged { .. }
            ),
        };
        self.invalidated |= update.needs_redraw();
        self.availability_invalidated |= update.availability_changed;
        Ok(update)
    }
    /// Releases presentation resources while preserving device and settings.
    /// Repeated calls are harmless.
    pub fn suspend(&mut self) {
        self.lifecycle.suspend();
        self.target = None;
        self.invalidated = true;
        self.availability_invalidated = true;
    }
    /// Restores presentation after suspension; repeated active calls are harmless.
    /// A failed recreation leaves the lifecycle active, with no presentation target.
    pub fn resume(&mut self) -> Result<(), WindowError> {
        self.lifecycle.resume();
        if !self.device_unavailable() && self.target.is_none() {
            self.recreate()?;
        }
        Ok(())
    }
    pub(crate) fn recreate(&mut self) -> Result<(), WindowError> {
        if self.device_unavailable() {
            self.invalidate_device();
            return Ok(());
        }
        // Drop first: some backends cannot create two swapchains for one window.
        self.target = None;
        let metrics = WindowMetrics::from_window(&self.window)?;
        let target = self.graphics.create_surface(
            self.window.clone(),
            self.settings.surface_options(metrics.physical_size()),
        )?;
        self.format = target.render_format();
        self.supported_samples = target.supported_sample_counts().to_vec();
        self.target = Some(target);
        self.metrics = metrics;
        self.generation = self.generation.saturating_add(1);
        self.invalidated = true;
        self.availability_invalidated = true;
        Ok(())
    }
    /// Whether presentation is blocked until an explicit device replacement.
    /// OS resume, resize, exposure and redraw requests cannot clear this state.
    pub fn device_unavailable(&self) -> bool {
        self.lifecycle.unavailable(self.graphics.is_device_lost())
    }

    /// Drops the surface and blocks recovery on this device, preserving settings.
    /// Call for every window sharing a lost device before rebuilding resources.
    /// Repeated calls are harmless; [`Self::resume`] cannot undo this operation.
    pub fn invalidate_device(&mut self) {
        self.lifecycle.invalidate();
        self.target = None;
        self.invalidated = true;
        self.availability_invalidated = true;
    }

    /// Installs an application-created replacement and recreates presentation.
    ///
    /// Invalidates first. Failure leaves the old device blocked with no target;
    /// the caller chooses whether and when to retry. Suspended windows retain the
    /// replacement but defer surface creation until resume. Rebuild application
    /// resources before allowing another render callback. No hook is invoked.
    /// A replacement must be a different device and must not be reported lost.
    pub fn replace_graphics(&mut self, graphics: &GraphicsContext) -> Result<(), WindowError> {
        self.invalidate_device();
        let replacement = self.prepare_graphics(graphics)?;
        self.install_graphics(graphics, replacement);
        Ok(())
    }

    pub(crate) fn prepare_graphics(
        &self,
        graphics: &GraphicsContext,
    ) -> Result<Option<(RenderTarget<'static>, WindowMetrics)>, WindowError> {
        if graphics.is_device_lost() || graphics.same_device(&self.graphics) {
            return Err(WindowError::InvalidReplacementDevice);
        }
        if !self.lifecycle.active {
            return Ok(None);
        }
        let metrics = WindowMetrics::from_window(&self.window)?;
        let target = graphics.create_surface(
            self.window.clone(),
            self.settings.surface_options(metrics.physical_size()),
        )?;
        Ok(Some((target, metrics)))
    }

    pub(crate) fn install_graphics(
        &mut self,
        graphics: &GraphicsContext,
        replacement: Option<(RenderTarget<'static>, WindowMetrics)>,
    ) {
        self.graphics = graphics.clone();
        self.lifecycle.replaced();
        // Counts from the old adapter must never validate pending settings.
        self.supported_samples.clear();
        if let Some((target, metrics)) = replacement {
            self.format = target.render_format();
            self.supported_samples = target.supported_sample_counts().to_vec();
            self.target = Some(target);
            self.metrics = metrics;
            self.generation = self.generation.saturating_add(1);
        }
        self.invalidated = true;
        self.availability_invalidated = true;
    }

    /// Changes MSAA now or queues a cached-supported setting while suspended/lost.
    /// Single sampling can always be queued; replacement revalidates other counts.
    /// Rejection leaves both live attachments and recovery settings unchanged.
    pub fn set_sample_count(&mut self, count: u32) -> Result<(), WindowError> {
        if self.device_unavailable() {
            self.invalidate_device();
        }
        if let Some(target) = &mut self.target {
            target.set_sample_count(count)?;
        } else if count != 1 && !self.supported_samples.contains(&count) {
            return Err(rxui_draw::Error::UnsupportedSampleCount {
                format: self.format.colors[0].expect("surface color"),
                count,
            }
            .into());
        }
        if self
            .settings
            .surface_options(self.metrics.physical_size())
            .sample_count
            != count
        {
            self.settings = self.settings.sample_count(count);
            if let Some(target) = &self.target {
                self.format = target.render_format();
            }
            self.invalidated = true;
        }
        Ok(())
    }
    /// Sets managed visibility and invalidates on changes.
    /// In window_created, showing is deferred until that callback returns.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            self.invalidated = true;
            self.availability_invalidated = true;
        }
        if !self.defer_show {
            self.window.set_visible(visible);
        }
    }
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    pub(crate) fn finish_creation(&mut self) {
        if self.defer_show {
            self.defer_show = false;
            self.window.set_visible(self.visible);
        }
    }
    /// Acquires an independently configured core frame; no pass is opened.
    /// Custom loops must notify the native window before consuming Frame::finish.
    pub fn begin_frame(&mut self) -> Result<Frame<'_, 'static>, FrameError> {
        if !self.can_present() {
            return Err(FrameError::Suspended);
        }
        self.target.as_mut().expect("eligible target").begin_frame()
    }
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    pub(crate) fn info_and_frame(
        &mut self,
    ) -> Result<(WindowInfo<'_>, Frame<'_, 'static>), FrameError> {
        if !self.can_present() {
            return Err(FrameError::Suspended);
        }
        let info = WindowInfo {
            window: &self.window,
            graphics: &self.graphics,
            metrics: self.metrics,
            format: &self.format,
            generation: self.generation,
        };
        let frame = self
            .target
            .as_mut()
            .expect("eligible target")
            .begin_frame()?;
        Ok((info, frame))
    }
}

/// Immutable window information borrowed independently of its recording frame.
/// No Arc or attachment-format vector is cloned for a render callback.
#[derive(Clone, Copy)]
pub struct WindowInfo<'a> {
    window: &'a Window,
    graphics: &'a GraphicsContext,
    metrics: WindowMetrics,
    format: &'a RenderFormat,
    generation: u64,
}
impl WindowInfo<'_> {
    /// Native identity, also used by AppContext.
    pub fn id(&self) -> WindowId {
        self.window.id()
    }
    /// Native window operations remain available while recording.
    pub fn window(&self) -> &Window {
        self.window
    }
    /// Graphics device used by this window.
    pub fn graphics(&self) -> &GraphicsContext {
        self.graphics
    }
    /// Cached physical/logical dimensions and scale.
    pub fn metrics(&self) -> WindowMetrics {
        self.metrics
    }
    /// Cached attachment compatibility for preparing application renderers.
    pub fn render_format(&self) -> &RenderFormat {
        self.format
    }
    /// Surface replacement generation.
    pub fn surface_generation(&self) -> u64 {
        self.generation
    }
}

/// Physical size preserving logical dimensions across a scale change, rounded
/// like winit's default `ScaleFactorChanged` suggestion.
fn rescaled_size(size: PhysicalSize<u32>, from: f64, to: f64) -> PhysicalSize<u32> {
    size.to_logical::<f64>(from).to_physical(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_loss_survives_os_resume_and_failed_replacement() {
        let mut state = DeviceLifecycle {
            active: true,
            invalidated: false,
        };
        assert!(!state.unavailable(false));
        // The backend flag blocks before the event-loop message is processed.
        assert!(state.unavailable(true));
        state.invalidate();
        state.invalidate();
        for _ in 0..3 {
            state.suspend();
            state.resume();
            assert!(state.unavailable(false));
        }
        // Failed staging never calls replaced, so the loss remains sticky.
        state.suspend();
        assert!(state.unavailable(false));
        state.replaced();
        assert!(!state.active);
        assert!(!state.unavailable(false));
        state.resume();
        assert!(state.active);
        assert!(!state.unavailable(false));
        assert!(state.unavailable(true)); // replacement can itself be lost
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    #[test]
    fn shared_loss_blocks_continuous_windows_and_expired_retries() {
        use crate::{RedrawMode, scheduler::Schedule};
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let mut windows = [DeviceLifecycle::default(), DeviceLifecycle::default()];
        for state in &mut windows {
            state.resume();
            let mut schedule = Schedule::default();
            schedule.mode(RedrawMode::Continuous);
            // Surface recovery may already be deep into backoff when the device dies.
            for _ in 0..10 {
                schedule.lost(3);
            }
            let delay = schedule.recovery_delay(Duration::from_millis(16), 3);
            assert_eq!(delay, Duration::from_secs(1));
            schedule.retry(now, delay);
            schedule.at(now);
            state.invalidate();
            for tick in 0..5 {
                state.suspend();
                state.resume();
                schedule.availability_changed();
                schedule.invalidate(); // OS exposure/resize cannot bypass loss
                let at = now + Duration::from_secs(tick);
                schedule.advance(at);
                let eligible = state.active && !state.unavailable(false);
                assert!(!schedule.request_native(at, eligible));
                assert_eq!(schedule.wake(eligible), None);
            }
            state.replaced();
            schedule.recoveries = 0;
            schedule.availability_changed();
            assert_eq!(
                schedule.recovery_delay(Duration::from_millis(16), 3),
                Duration::from_millis(16)
            );
            assert!(schedule.request_native(now + Duration::from_secs(6), true));
        }
    }

    #[test]
    fn scale_changes_preserve_logical_size() {
        assert_eq!(
            rescaled_size(PhysicalSize::new(1600, 1200), 2., 1.5),
            PhysicalSize::new(1200, 900)
        );
        assert_eq!(
            rescaled_size(PhysicalSize::new(801, 601), 1., 1.25),
            PhysicalSize::new(1001, 751)
        );
        assert_eq!(
            rescaled_size(PhysicalSize::new(0, 600), 1., 2.),
            PhysicalSize::new(0, 1200)
        );
    }
}
