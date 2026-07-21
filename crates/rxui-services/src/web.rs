//! Browser backend for `wasm32` targets.

use std::path::Path;

use crate::{DeliverFile, DeliverFiles, FileDialogOptions, ServiceBackend, ServiceError};

/// Browser [`ServiceBackend`] for `wasm32` targets.
///
/// v1 semantics are intentionally minimal: only
/// [`open_url`](ServiceBackend::open_url) is functional, opening the URL in
/// a new browsing context via `web_sys::Window::open_with_url`. File
/// dialogs are not yet supported in the browser — `rfd`'s wasm dialog
/// futures are not `Send` and cannot satisfy the [`DeliverFile`] contract —
/// so every dialog delivers `None` (indistinguishable from cancellation),
/// and [`open_path`](ServiceBackend::open_path) /
/// [`reveal_path`](ServiceBackend::reveal_path) return
/// [`ServiceError::UnsupportedPlatform`].
#[derive(Clone, Copy, Debug, Default)]
pub struct WebBackend;

impl WebBackend {
    /// Creates the browser backend.
    pub fn new() -> Self {
        Self
    }
}

impl ServiceBackend for WebBackend {
    fn pick_file(&self, _options: FileDialogOptions, deliver: DeliverFile) {
        deliver(None);
    }

    fn pick_files(&self, _options: FileDialogOptions, deliver: DeliverFiles) {
        deliver(None);
    }

    fn pick_folder(&self, _options: FileDialogOptions, deliver: DeliverFile) {
        deliver(None);
    }

    fn save_file(&self, _options: FileDialogOptions, deliver: DeliverFile) {
        deliver(None);
    }

    fn open_url(&self, url: &str) -> Result<(), ServiceError> {
        let window = web_sys::window().ok_or(ServiceError::UnsupportedPlatform)?;
        window
            .open_with_url(url)
            .map_err(|error| ServiceError::Backend(format!("window.open failed: {error:?}")))?;
        Ok(())
    }

    fn open_path(&self, _path: &Path) -> Result<(), ServiceError> {
        Err(ServiceError::UnsupportedPlatform)
    }

    fn reveal_path(&self, _path: &Path) -> Result<(), ServiceError> {
        Err(ServiceError::UnsupportedPlatform)
    }
}
