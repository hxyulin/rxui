use rxui_draw::{SurfaceOptions, wgpu};
use std::{error::Error, fmt};
use winit::{
    dpi::{LogicalSize, PhysicalSize},
    window::Window,
};

/// Physical and logical inner-window dimensions captured at one scale factor.
///
/// Capture on window creation, resize, or scale-factor changes. Zero dimensions
/// are valid and represent a window that cannot currently present. Outer-window
/// decorations are excluded. Construction checks that logical dimensions remain
/// finite; getters return cached values without native window queries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowMetrics {
    physical: PhysicalSize<u32>,
    logical: LogicalSize<f64>,
    scale_factor: f64,
}

impl WindowMetrics {
    /// Validates a finite positive scale factor and derives logical dimensions.
    pub fn new(
        physical: PhysicalSize<u32>,
        scale_factor: f64,
    ) -> Result<Self, InvalidWindowMetrics> {
        if !scale_factor.is_finite() || scale_factor <= 0. {
            return Err(InvalidWindowMetrics);
        }
        let logical = physical.to_logical::<f64>(scale_factor);
        if !logical.width.is_finite() || !logical.height.is_finite() {
            return Err(InvalidWindowMetrics);
        }
        Ok(Self {
            physical,
            logical,
            scale_factor,
        })
    }

    /// Captures the native window's current inner size and scale factor.
    /// These native queries are intended for lifecycle updates, rather than draws.
    pub fn from_window(window: &Window) -> Result<Self, InvalidWindowMetrics> {
        Self::new(window.inner_size(), window.scale_factor())
    }

    /// Inner size in physical pixels, including zero while minimized/suspended.
    pub const fn physical_size(&self) -> PhysicalSize<u32> {
        self.physical
    }

    /// Inner size in logical units derived from the captured scale factor.
    pub const fn logical_size(&self) -> LogicalSize<f64> {
        self.logical
    }

    /// Physical pixels per logical unit. Text raster density is chosen separately.
    pub const fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    /// Whether both physical dimensions are nonzero; this does not query occlusion.
    pub const fn has_area(&self) -> bool {
        self.physical.width != 0 && self.physical.height != 0
    }
}

/// The scale factor is invalid or deriving logical dimensions overflowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidWindowMetrics;
impl fmt::Display for InvalidWindowMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("window scale must be finite and positive, with finite logical dimensions")
    }
}
impl Error for InvalidWindowMetrics {}

/// Desired attachment settings independent of the native window's physical size.
///
/// Window dimensions come from WindowMetrics; these settings survive surface
/// replacement and suspension. Describing settings performs no GPU work.
/// RXUI drawing validates format/sample support when a target is created or changed;
/// unsupported requests are errors rather than implicit fallback choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceSettings {
    sample_count: u32,
    depth_stencil_format: Option<wgpu::TextureFormat>,
    depth_stencil_usage: wgpu::TextureUsages,
    present_mode: wgpu::PresentMode,
    desired_maximum_frame_latency: u32,
    alpha_mode: wgpu::CompositeAlphaMode,
}
impl Default for SurfaceSettings {
    fn default() -> Self {
        Self::new()
    }
}
impl SurfaceSettings {
    /// Describes single-sampled color without depth/stencil attachments.
    pub const fn new() -> Self {
        Self {
            sample_count: 1,
            depth_stencil_format: None,
            depth_stencil_usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
        }
    }

    /// Selects presentation pacing; FIFO (vsync) by default. See [`SurfaceOptions`].
    #[must_use]
    pub const fn present_mode(mut self, mode: wgpu::PresentMode) -> Self {
        self.present_mode = mode;
        self
    }

    /// Selects desired queued frames, retained across recovery. Default 2.
    /// See [`SurfaceOptions::desired_maximum_frame_latency`].
    #[must_use]
    pub const fn desired_maximum_frame_latency(mut self, latency: u32) -> Self {
        self.desired_maximum_frame_latency = latency;
        self
    }

    /// Selects window compositing alpha; automatic by default. Transparent windows
    /// also need a transparent native window. See [`SurfaceOptions`].
    #[must_use]
    pub const fn alpha_mode(mut self, mode: wgpu::CompositeAlphaMode) -> Self {
        self.alpha_mode = mode;
        self
    }

    /// Selects the requested MSAA count; one disables MSAA.
    #[must_use]
    pub const fn sample_count(mut self, count: u32) -> Self {
        self.sample_count = count;
        self
    }

    /// Requests a depth-only, stencil-only, or combined attachment format.
    #[must_use]
    pub const fn depth_stencil(mut self, format: wgpu::TextureFormat) -> Self {
        self.depth_stencil_format = Some(format);
        self
    }

    /// Sets depth/stencil usages, including RENDER_ATTACHMENT for rendering.
    #[must_use]
    pub const fn depth_stencil_usage(mut self, usage: wgpu::TextureUsages) -> Self {
        self.depth_stencil_usage = usage;
        self
    }

    /// Supplies physical dimensions to RXUI drawing target creation/recreation.
    /// Zero sizes are supported; capabilities are validated by target creation.
    pub const fn surface_options(self, size: PhysicalSize<u32>) -> SurfaceOptions {
        SurfaceOptions {
            size: [size.width, size.height],
            sample_count: self.sample_count,
            depth_stencil_format: self.depth_stencil_format,
            depth_stencil_usage: self.depth_stencil_usage,
            present_mode: self.present_mode,
            desired_maximum_frame_latency: self.desired_maximum_frame_latency,
            alpha_mode: self.alpha_mode,
        }
    }
}

/// Desired redraw behavior for one managed window in the desktop runner.
/// Explicit redraw/deadline requests are also available in either mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RedrawMode {
    /// Draw on invalidation or OS redraw requests; sleep while idle.
    #[default]
    OnDemand,
    /// Request subsequent frames while the window can present.
    Continuous,
}

/// Application decision for an OS close request in the handler API.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CloseResponse {
    /// Remove the managed window and its presentation resources.
    #[default]
    Close,
    /// Keep the window open, for example while the application resolves unsaved data.
    KeepOpen,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_validate_scale_overflow_and_zero_sized_windows() {
        let size = PhysicalSize::new(1600, 1200);
        for scale in [0., -1., f64::NAN, f64::INFINITY, f64::MIN_POSITIVE] {
            assert!(WindowMetrics::new(size, scale).is_err());
        }
        let metrics = WindowMetrics::new(size, 2.).unwrap();
        assert_eq!(metrics.logical_size(), LogicalSize::new(800., 600.));
        assert!(metrics.has_area());
        assert!(
            !WindowMetrics::new(PhysicalSize::new(0, 1200), 2.)
                .unwrap()
                .has_area()
        );
    }

    #[test]
    fn surface_settings_carry_presentation_modes_into_options() {
        let defaults = SurfaceSettings::new().surface_options(PhysicalSize::new(4, 3));
        assert_eq!(defaults, SurfaceOptions::new(4, 3));
        let options = SurfaceSettings::new()
            .present_mode(wgpu::PresentMode::Mailbox)
            .desired_maximum_frame_latency(1)
            .alpha_mode(wgpu::CompositeAlphaMode::PreMultiplied)
            .surface_options(PhysicalSize::new(4, 3));
        assert_eq!(options.present_mode, wgpu::PresentMode::Mailbox);
        assert_eq!(defaults.desired_maximum_frame_latency, 2);
        assert_eq!(options.desired_maximum_frame_latency, 1);
        assert_eq!(options.alpha_mode, wgpu::CompositeAlphaMode::PreMultiplied);
    }
}
