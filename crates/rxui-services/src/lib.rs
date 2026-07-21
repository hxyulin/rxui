//! Desktop services for RXUI: file dialogs, launching URLs and paths, and
//! recent-document tracking.
//!
//! # Message-driven dialogs
//!
//! Dialogs never block the UI thread. Each `pick_*`/`save_file` call takes a
//! one-shot delivery closure invoked exactly once with the chosen path
//! (`None` on cancellation). In an application the closure typically posts a
//! message back into [`rxui_app::App::update`] through
//! [`rxui_app::MessageProxy`]:
//!
//! ```ignore
//! use rxui_services::{DesktopServices, FileDialogOptions};
//!
//! enum Message {
//!     FileOpened(Option<std::path::PathBuf>),
//! }
//!
//! // Inside `App::update`, with `cx: &mut AppCx<Message>`:
//! let services = DesktopServices::native();
//! let proxy = cx.proxy();
//! services.pick_file(
//!     FileDialogOptions::new()
//!         .title("Open Project")
//!         .filter("RXUI project", &["rxui"]),
//!     move |path| {
//!         let _ = proxy.post(Message::FileOpened(path));
//!     },
//! );
//! ```
//!
//! Tests use the deterministic [`FakeBackend`] instead of the platform
//! backend, so no operating-system dialogs are shown.

#![warn(missing_docs)]

use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

mod dialogs;
pub mod fake;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod recent;
#[cfg(target_arch = "wasm32")]
mod web;

pub use dialogs::{FileDialogOptions, FileFilter};
pub use fake::FakeBackend;
#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeBackend;
pub use recent::RecentDocuments;
#[cfg(target_arch = "wasm32")]
pub use web::WebBackend;

/// One-shot delivery of a single dialog result; `None` means cancelled.
pub type DeliverFile = Box<dyn FnOnce(Option<PathBuf>) + Send>;

/// One-shot delivery of a multi-selection dialog result; `None` means
/// cancelled.
pub type DeliverFiles = Box<dyn FnOnce(Option<Vec<PathBuf>>) + Send>;

/// Platform implementation behind [`DesktopServices`].
///
/// Dialog methods must invoke `deliver` exactly once, from any thread;
/// launch methods report failure through [`ServiceError`].
pub trait ServiceBackend: Send + Sync {
    /// Shows an open-file dialog for a single file.
    fn pick_file(&self, options: FileDialogOptions, deliver: DeliverFile);

    /// Shows an open-file dialog allowing multiple files.
    fn pick_files(&self, options: FileDialogOptions, deliver: DeliverFiles);

    /// Shows a folder-selection dialog.
    fn pick_folder(&self, options: FileDialogOptions, deliver: DeliverFile);

    /// Shows a save-file dialog.
    fn save_file(&self, options: FileDialogOptions, deliver: DeliverFile);

    /// Opens a URL with the user's default handler (usually the browser).
    fn open_url(&self, url: &str) -> Result<(), ServiceError>;

    /// Opens a file or directory with the default application.
    fn open_path(&self, path: &Path) -> Result<(), ServiceError>;

    /// Reveals a path in the platform file manager.
    fn reveal_path(&self, path: &Path) -> Result<(), ServiceError>;
}

/// Cloneable handle to the platform desktop services.
///
/// All clones share one [`ServiceBackend`]. Applications normally construct
/// it once with [`DesktopServices::native`]; tests inject a
/// [`FakeBackend`] through [`DesktopServices::with_backend`].
#[derive(Clone)]
pub struct DesktopServices {
    backend: Arc<dyn ServiceBackend>,
}

impl fmt::Debug for DesktopServices {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopServices")
            .finish_non_exhaustive()
    }
}

impl DesktopServices {
    /// Creates services backed by the current platform.
    ///
    /// On native targets this uses [`NativeBackend`]; on `wasm32` it uses
    /// `WebBackend`, whose dialog and path operations are largely
    /// unsupported (see the backend type's documentation).
    pub fn native() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self::with_backend(Arc::new(native::NativeBackend::new()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::with_backend(Arc::new(web::WebBackend::new()))
        }
    }

    /// Creates services over an explicit backend (used by tests).
    pub fn with_backend(backend: Arc<dyn ServiceBackend>) -> Self {
        Self { backend }
    }

    /// Shows an open-file dialog; `deliver` receives the chosen path or
    /// `None` on cancellation.
    pub fn pick_file(
        &self,
        options: FileDialogOptions,
        deliver: impl FnOnce(Option<PathBuf>) + Send + 'static,
    ) {
        self.backend.pick_file(options, Box::new(deliver));
    }

    /// Shows a multi-selection open dialog; `deliver` receives the chosen
    /// paths or `None` on cancellation.
    pub fn pick_files(
        &self,
        options: FileDialogOptions,
        deliver: impl FnOnce(Option<Vec<PathBuf>>) + Send + 'static,
    ) {
        self.backend.pick_files(options, Box::new(deliver));
    }

    /// Shows a folder-selection dialog; `deliver` receives the chosen
    /// folder or `None` on cancellation.
    pub fn pick_folder(
        &self,
        options: FileDialogOptions,
        deliver: impl FnOnce(Option<PathBuf>) + Send + 'static,
    ) {
        self.backend.pick_folder(options, Box::new(deliver));
    }

    /// Shows a save-file dialog; `deliver` receives the chosen destination
    /// or `None` on cancellation.
    pub fn save_file(
        &self,
        options: FileDialogOptions,
        deliver: impl FnOnce(Option<PathBuf>) + Send + 'static,
    ) {
        self.backend.save_file(options, Box::new(deliver));
    }

    /// Opens a URL with the user's default handler.
    pub fn open_url(&self, url: &str) -> Result<(), ServiceError> {
        self.backend.open_url(url)
    }

    /// Opens a file or directory with the default application.
    pub fn open_path(&self, path: impl AsRef<Path>) -> Result<(), ServiceError> {
        self.backend.open_path(path.as_ref())
    }

    /// Reveals a path in the platform file manager.
    pub fn reveal_path(&self, path: impl AsRef<Path>) -> Result<(), ServiceError> {
        self.backend.reveal_path(path.as_ref())
    }
}

/// Desktop service failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceError {
    /// The current target has no RXUI backend for the requested service.
    UnsupportedPlatform,
    /// The platform backend reported an error.
    Backend(String),
}

impl ServiceError {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn from_display(error: impl fmt::Display) -> Self {
        Self::Backend(error.to_string())
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("this desktop service is not supported on the current platform")
            }
            Self::Backend(message) => formatter.write_str(message),
        }
    }
}

impl Error for ServiceError {}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn services() -> (DesktopServices, Arc<FakeBackend>) {
        let backend = Arc::new(FakeBackend::new());
        (DesktopServices::with_backend(backend.clone()), backend)
    }

    #[test]
    fn pick_file_forwards_options_and_delivers_scripted_result() {
        let (services, backend) = services();
        backend.push_file(Some(PathBuf::from("/tmp/a.rxui")));
        let options = FileDialogOptions::new()
            .title("Open")
            .filter("Projects", &[".rxui"]);
        let delivered = Arc::new(std::sync::Mutex::new(None));
        let sink = delivered.clone();
        services.pick_file(options.clone(), move |path| {
            *sink.lock().unwrap() = Some(path);
        });
        assert_eq!(
            *delivered.lock().unwrap(),
            Some(Some(PathBuf::from("/tmp/a.rxui")))
        );
        assert_eq!(backend.requests(), vec![options]);
    }

    #[test]
    fn dialogs_deliver_exactly_once_in_queue_order() {
        let (services, backend) = services();
        backend.push_file(Some(PathBuf::from("/one")));
        backend.push_file(Some(PathBuf::from("/two")));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        for _ in 0..2 {
            let calls = calls.clone();
            let seen = seen.clone();
            services.pick_file(FileDialogOptions::new(), move |path| {
                calls.fetch_add(1, Ordering::SeqCst);
                seen.lock().unwrap().push(path);
            });
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![Some(PathBuf::from("/one")), Some(PathBuf::from("/two"))]
        );
    }

    #[test]
    fn empty_queue_and_scripted_none_both_deliver_cancellation() {
        let (services, backend) = services();
        backend.push_file(None);
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        for _ in 0..2 {
            let seen = seen.clone();
            services.save_file(FileDialogOptions::new(), move |path| {
                seen.lock().unwrap().push(path);
            });
        }
        assert_eq!(*seen.lock().unwrap(), vec![None, None]);
    }

    #[test]
    fn pick_files_delivers_scripted_lists() {
        let (services, backend) = services();
        backend.push_files(Some(vec![PathBuf::from("/a"), PathBuf::from("/b")]));
        let seen = Arc::new(std::sync::Mutex::new(None));
        let sink = seen.clone();
        services.pick_files(FileDialogOptions::new(), move |paths| {
            *sink.lock().unwrap() = Some(paths);
        });
        assert_eq!(
            *seen.lock().unwrap(),
            Some(Some(vec![PathBuf::from("/a"), PathBuf::from("/b")]))
        );
    }

    #[test]
    fn launch_calls_are_recorded() {
        let (services, backend) = services();
        services.open_url("https://example.com").unwrap();
        services.open_path("/tmp/doc.txt").unwrap();
        services.reveal_path("/tmp/doc.txt").unwrap();
        assert_eq!(
            backend.launches(),
            vec![
                "open_url:https://example.com",
                "open_path:/tmp/doc.txt",
                "reveal_path:/tmp/doc.txt",
            ]
        );
    }

    #[test]
    fn clones_share_one_backend() {
        let (services, backend) = services();
        backend.push_file(Some(PathBuf::from("/shared")));
        let clone = services.clone();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let sink = seen.clone();
        clone.pick_file(FileDialogOptions::new(), move |path| {
            *sink.lock().unwrap() = Some(path);
        });
        assert_eq!(*seen.lock().unwrap(), Some(Some(PathBuf::from("/shared"))));
        assert_eq!(backend.requests().len(), 1);
    }

    #[test]
    fn service_error_display_is_stable() {
        assert_eq!(
            ServiceError::UnsupportedPlatform.to_string(),
            "this desktop service is not supported on the current platform"
        );
        assert_eq!(
            ServiceError::Backend("dialog exploded".into()).to_string(),
            "dialog exploded"
        );
        assert_eq!(
            ServiceError::from_display("wrapped"),
            ServiceError::Backend("wrapped".into())
        );
    }
}
