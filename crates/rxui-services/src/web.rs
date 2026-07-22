//! Browser backend for `wasm32` targets.

use std::{path::Path, sync::Arc};

use crate::{
    DeliverFile, DeliverFiles, DeliverSavedFile, DeliverSelectedFile, DeliverSelectedFiles,
    FileDialogOptions, SavedFile, SelectedFile, ServiceBackend, ServiceError,
};

/// Browser [`ServiceBackend`] for `wasm32` targets.
///
/// Byte-oriented file-content opening and downloads use `rfd`'s local browser
/// futures. Native-path dialog methods still deliver `None`, because browsers
/// cannot expose a `PathBuf`; folder selection, path launching, revealing, and
/// filesystem watching remain unsupported. URLs open in a new browsing
/// context through `web_sys::Window::open_with_url`.
#[derive(Clone, Copy, Debug, Default)]
pub struct WebBackend;

impl WebBackend {
    /// Creates the browser backend.
    pub fn new() -> Self {
        Self
    }
}

fn build_dialog(options: &FileDialogOptions) -> rfd::AsyncFileDialog {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(title) = options.title_ref() {
        dialog = dialog.set_title(title);
    }
    if let Some(file_name) = options.file_name_ref() {
        dialog = dialog.set_file_name(file_name);
    }
    for filter in options.filters() {
        dialog = dialog.add_filter(&filter.name, &filter.extensions);
    }
    dialog
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

    fn pick_file_contents(&self, options: FileDialogOptions, deliver: DeliverSelectedFile) {
        wasm_bindgen_futures::spawn_local(async move {
            let result = if let Some(handle) = build_dialog(&options).pick_file().await {
                let name = handle.file_name();
                let bytes = handle.read().await;
                Some(SelectedFile {
                    name,
                    bytes: bytes.into(),
                    path: None,
                })
            } else {
                None
            };
            deliver(Ok(result));
        });
    }

    fn pick_files_contents(&self, options: FileDialogOptions, deliver: DeliverSelectedFiles) {
        wasm_bindgen_futures::spawn_local(async move {
            let Some(handles) = build_dialog(&options).pick_files().await else {
                deliver(Ok(None));
                return;
            };
            let mut files = Vec::with_capacity(handles.len());
            for handle in handles {
                let name = handle.file_name();
                let bytes = handle.read().await;
                files.push(SelectedFile {
                    name,
                    bytes: bytes.into(),
                    path: None,
                });
            }
            deliver(Ok(Some(files)));
        });
    }

    fn save_bytes(&self, options: FileDialogOptions, bytes: Arc<[u8]>, deliver: DeliverSavedFile) {
        wasm_bindgen_futures::spawn_local(async move {
            let Some(handle) = build_dialog(&options).save_file().await else {
                deliver(Ok(None));
                return;
            };
            let name = handle.file_name();
            let result = handle
                .write(bytes.as_ref())
                .await
                .map(|()| Some(SavedFile { name, path: None }))
                .map_err(ServiceError::from_display);
            deliver(result);
        });
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
