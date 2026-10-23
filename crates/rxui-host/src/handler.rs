use crate::{AppContext, CloseResponse, RunError, WindowInfo};
use rxui_draw::{Frame, wgpu};
use winit::{
    event::{DeviceEvent, DeviceId, WindowEvent},
    window::WindowId,
};

/// Decision after preparation, before any swapchain image is acquired.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PrepareAction {
    /// Acquire a frame and invoke render.
    #[default]
    Render,
    /// Acquire nothing; wait for the next invalidation, including in Continuous.
    Skip,
}

/// Decision after a reported runner error.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ErrorAction {
    /// Stop the loop and return the original error.
    #[default]
    Exit,
    /// Abandon failed work and continue. Fatal device/loop failures still exit.
    Continue,
}

/// Application hooks for the native runner, with static dispatch and owned events.
///
/// Applications own renderers, layout, and models. Prepare uploads and pipeline
/// variants before acquisition; render opens application-selected passes in a
/// borrowed core frame. Successful recording is submitted once by the runner.
/// Errors are reported to [`Self::error`]; the original error is boxed only on failure.
/// A handler is borrowed by run and remains available after native shutdown.
pub trait Handler {
    /// Actual winit user-event type, also accepted by the native EventLoopProxy.
    type Message: 'static;
    /// Convertible application error; `Box<dyn Error>` is supported directly.
    type Error: Into<Box<dyn std::error::Error>>;
    /// Reports failed work after frame borrows end, before deciding whether to exit.
    /// Default policy retries transient acquisition/loss and exits on other errors.
    /// Return Continue for per-frame errors to skip and retry with runner pacing.
    /// Queued device loss and out-of-memory remain fatal even if Continue is returned.
    /// An explicit GraphicsContext loss bridge instead hands recovery to the application.
    /// The context permits queuing closes, redraws, or application messages.
    fn error(&mut self, _cx: &mut AppContext<'_, Self::Message>, error: &RunError) -> ErrorAction {
        match error {
            RunError::SurfaceRecovery { .. } => ErrorAction::Continue,
            RunError::Acquisition {
                source: rxui_draw::FrameError::Retry | rxui_draw::FrameError::SurfaceLost,
                ..
            } => ErrorAction::Continue,
            _ => ErrorAction::Exit,
        }
    }
    /// Called after existing presentation targets are restored. May repeat.
    fn resumed(&mut self, _cx: &mut AppContext<'_, Self::Message>) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Called after all presentation targets are released. May repeat.
    fn suspended(&mut self, _cx: &mut AppContext<'_, Self::Message>) -> Result<(), Self::Error> {
        Ok(())
    }
    /// A registration completed, before a newly created managed window is shown.
    /// Native adapters can be installed here. Adoption preserves caller visibility.
    fn window_created(
        &mut self,
        _cx: &mut AppContext<'_, Self::Message>,
        _id: WindowId,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Original native event after lifecycle bookkeeping, including redraw/close.
    /// Unmanaged IDs are forwarded too. No implicit UI interpretation occurs.
    fn window_event(
        &mut self,
        _cx: &mut AppContext<'_, Self::Message>,
        _id: WindowId,
        _event: WindowEvent,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Original typed user message without a runner-specific envelope.
    fn user_event(
        &mut self,
        _cx: &mut AppContext<'_, Self::Message>,
        _message: Self::Message,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Original raw device event.
    fn device_event(
        &mut self,
        _cx: &mut AppContext<'_, Self::Message>,
        _id: DeviceId,
        _event: DeviceEvent,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Update resources/layout/settings before acquisition. Retries may repeat it.
    fn prepare(
        &mut self,
        _cx: &mut AppContext<'_, Self::Message>,
        _id: WindowId,
    ) -> Result<PrepareAction, Self::Error> {
        Ok(PrepareAction::Render)
    }
    /// Records any number of application-selected passes or custom GPU commands.
    /// The runner notifies, submits and presents after success. Error abandons the
    /// frame. Registry mutation is available in the other context-bearing hooks.
    ///
    /// A successful render must initialize the surface, normally with a clearing
    /// pass from `frame.render_pass()`. Returning `Ok` without one reports an error
    /// with [`crate::RunError::Handler`] for [`crate::Callback::Render`] carrying
    /// [`rxui_draw::Error::UninitializedFrame`], because presenting would show undefined
    /// contents. To draw nothing this time, return [`PrepareAction::Skip`] from
    /// [`Self::prepare`] instead.
    fn render(
        &mut self,
        window: WindowInfo<'_>,
        frame: &mut Frame<'_, 'static>,
    ) -> Result<(), Self::Error>;
    /// Submission queued successfully; this does not promise display completion.
    /// Frame borrows have ended, so redraw/deadline/registry operations are allowed.
    fn submitted(
        &mut self,
        _cx: &mut AppContext<'_, Self::Message>,
        _id: WindowId,
        _submission: wgpu::SubmissionIndex,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Veto or accept an OS close request. Explicit close_window bypasses this hook.
    fn close_requested(
        &mut self,
        _cx: &mut AppContext<'_, Self::Message>,
        _id: WindowId,
    ) -> Result<CloseResponse, Self::Error> {
        Ok(CloseResponse::Close)
    }
    /// Managed ownership was released. Called before exiting, also during failure.
    fn window_closed(
        &mut self,
        _cx: &mut AppContext<'_, Self::Message>,
        _id: WindowId,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Final shutdown hook after managed windows were closed.
    fn exiting(&mut self, _cx: &mut AppContext<'_, Self::Message>) -> Result<(), Self::Error> {
        Ok(())
    }
}
