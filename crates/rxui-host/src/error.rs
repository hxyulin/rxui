use std::{error::Error, fmt};
use winit::window::WindowId;

/// Window creation, metrics, or presentation configuration failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum WindowError {
    /// The operating system rejected window creation.
    Native(winit::error::OsError),
    /// RXUI drawing rejected initialization or attachment configuration.
    Graphics(rxui_draw::Error),
    /// Native metrics cannot be represented as finite logical dimensions.
    Metrics(crate::InvalidWindowMetrics),
    /// Register windows during an active application lifecycle, after resume.
    Inactive,
    /// Device loss blocks GPU operations until application replacement.
    DeviceUnavailable,
    /// Replacement must use a different device that has not been reported lost.
    InvalidReplacementDevice,
    /// This native window is already registered.
    AlreadyManaged(WindowId),
}
impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(e) => write!(f, "window creation failed: {e}"),
            Self::Graphics(e) => write!(f, "window graphics failed: {e}"),
            Self::Metrics(e) => e.fmt(f),
            Self::Inactive => f.write_str("window registration requires an active application"),
            Self::DeviceUnavailable => f.write_str("device unavailable; replace graphics first"),
            Self::InvalidReplacementDevice => {
                f.write_str("replacement must be a different live device")
            }
            Self::AlreadyManaged(id) => write!(f, "window {id:?} is already managed"),
        }
    }
}
impl Error for WindowError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Native(e) => Some(e),
            Self::Graphics(e) => Some(e),
            Self::Metrics(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rxui_draw::Error> for WindowError {
    fn from(value: rxui_draw::Error) -> Self {
        Self::Graphics(value)
    }
}
impl From<crate::InvalidWindowMetrics> for WindowError {
    fn from(value: crate::InvalidWindowMetrics) -> Self {
        Self::Metrics(value)
    }
}
impl From<winit::error::OsError> for WindowError {
    fn from(value: winit::error::OsError) -> Self {
        Self::Native(value)
    }
}

/// A requested window is absent or has already been queued for close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownWindow(pub WindowId);
impl fmt::Display for UnknownWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown window {:?}", self.0)
    }
}
impl Error for UnknownWindow {}

/// Callback that returned an application error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    /// Application resume.
    Resumed,
    /// Application suspension.
    Suspended,
    /// A newly registered window.
    WindowCreated,
    /// A native window event.
    WindowEvent,
    /// An application message.
    UserEvent,
    /// A device event.
    DeviceEvent,
    /// Preparation before acquisition.
    Prepare,
    /// Recording an acquired frame.
    Render,
    /// Notification after submission.
    Submitted,
    /// An OS close decision.
    CloseRequested,
    /// A window was removed.
    WindowClosed,
    /// Final application shutdown.
    Exiting,
}

/// Native runner failure, with the original error retained where available.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunError {
    /// Invalid retry delay or recovery limit.
    InvalidOptions,
    /// Native event-loop failure.
    EventLoop(winit::error::EventLoopError),
    /// An application callback failed.
    Handler {
        /// Callback that failed.
        callback: Callback,
        /// Window associated with the callback, if any.
        window: Option<WindowId>,
        /// Original application error, boxed only on failure.
        source: Box<dyn Error>,
    },
    /// Window metrics, lifecycle, or surface recreation failed.
    Window {
        /// Associated native window.
        window: WindowId,
        /// Original window error.
        source: WindowError,
    },
    /// Acquisition returned a terminal error.
    Acquisition {
        /// Associated native window.
        window: WindowId,
        /// Original acquisition failure.
        source: rxui_draw::FrameError,
    },
    /// Surface recreation failed; the runner will retry with backoff by default.
    SurfaceRecovery {
        /// Associated native window.
        window: WindowId,
        /// Original window error.
        source: WindowError,
    },
    /// Legacy recovery-limit failure; the runner now backs off instead of emitting it.
    RecoveryLimit {
        /// Associated native window.
        window: WindowId,
    },
    /// Asynchronous GPU diagnostic on the window's shared device.
    Graphics {
        /// Window whose context delivered the diagnostic; other windows may share it.
        window: WindowId,
        /// Original diagnostic. Device loss and out-of-memory are fatal.
        source: rxui_draw::GpuEvent,
    },
    /// Finishing and submitting the recorded frame failed.
    Submission {
        /// Associated native window.
        window: WindowId,
        /// Original rendering failure.
        source: rxui_draw::Error,
    },
}
impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOptions => f.write_str(
                "retry delay must be positive and representable, and recovery limit nonzero",
            ),
            Self::EventLoop(e) => e.fmt(f),
            Self::Handler {
                callback,
                window,
                source,
            } => write!(f, "{callback:?} callback for {window:?} failed: {source}"),
            Self::Window { window, source } | Self::SurfaceRecovery { window, source } => {
                write!(f, "window {window:?}: {source}")
            }
            Self::Acquisition { window, source } => {
                write!(f, "window {window:?} acquisition: {source}")
            }
            Self::RecoveryLimit { window } => write!(
                f,
                "window {window:?} exceeded consecutive surface recovery limit"
            ),
            Self::Graphics { window, source } => write!(f, "window {window:?} GPU: {source}"),
            Self::Submission { window, source } => {
                write!(f, "window {window:?} submission: {source}")
            }
        }
    }
}
impl Error for RunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EventLoop(e) => Some(e),
            Self::Handler { source, .. } => Some(source.as_ref()),
            Self::Window { source, .. } | Self::SurfaceRecovery { source, .. } => Some(source),
            Self::Acquisition { source, .. } => Some(source),
            Self::Submission { source, .. } => Some(source),
            Self::Graphics { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl RunError {
    /// Whether continuing can use the current device and event loop safely.
    /// Continuation skips failed work; it does not undo submitted commands.
    pub fn can_continue(&self) -> bool {
        match self {
            Self::InvalidOptions | Self::EventLoop(_) => false,
            Self::Graphics { source, .. } => matches!(
                source,
                rxui_draw::GpuEvent::Validation(_) | rxui_draw::GpuEvent::Internal(_)
            ),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_failures_can_continue_but_dead_devices_cannot() {
        let window = WindowId::from(1_u64);
        for source in [
            rxui_draw::FrameError::Retry,
            rxui_draw::FrameError::Validation,
            rxui_draw::FrameError::SurfaceLost,
        ] {
            assert!(RunError::Acquisition { window, source }.can_continue());
        }
        for callback in [Callback::Prepare, Callback::Render, Callback::Submitted] {
            assert!(
                RunError::Handler {
                    callback,
                    window: Some(window),
                    source: std::io::Error::other("bad frame").into()
                }
                .can_continue()
            );
        }
        for source in [
            rxui_draw::GpuEvent::Validation("bad frame".into()),
            rxui_draw::GpuEvent::Internal("transient".into()),
        ] {
            assert!(RunError::Graphics { window, source }.can_continue());
        }
        for source in [
            rxui_draw::GpuEvent::OutOfMemory("allocation".into()),
            rxui_draw::GpuEvent::DeviceLost {
                reason: rxui_draw::wgpu::DeviceLostReason::Unknown,
                message: "lost".into(),
            },
        ] {
            assert!(!RunError::Graphics { window, source }.can_continue());
        }
        assert!(!RunError::InvalidOptions.can_continue());
    }
}
