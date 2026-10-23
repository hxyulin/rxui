use super::*;
pub(crate) struct Adapter;
impl Adapter {
    pub(crate) fn new() -> Result<Self, Error> {
        Err(Error(
            "desktop adapter is only available on macOS and Windows".into(),
        ))
    }
    pub(crate) fn register(&mut self, _: &Arc<Window>) -> Result<(), Error> {
        Err(Error("unsupported platform".into()))
    }
    pub(crate) fn unregister(&mut self, _: WindowId) -> Result<(), Error> {
        Err(Error("unsupported platform".into()))
    }
    pub(crate) fn begin_capture(&mut self, _: WindowId) -> Result<(), Error> {
        Err(Error("unsupported platform".into()))
    }
    pub(crate) fn end_capture(&mut self, _: WindowId) {}
    pub(crate) fn capture_state(&mut self) -> CaptureState {
        CaptureState::Idle
    }
    pub(crate) fn take_signals(&mut self) -> Vec<Signal> {
        Vec::new()
    }
    pub(crate) fn target_at_pointer(&mut self) -> Target {
        Target::Unavailable
    }
}
