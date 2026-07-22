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

use rxui_app::{Subscription, SubscriptionId, SubscriptionKind};

mod dialogs;
pub mod fake;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod recent;
mod watch;
#[cfg(target_arch = "wasm32")]
mod web;

pub use dialogs::{FileDialogOptions, FileFilter};
pub use fake::FakeBackend;
#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeBackend;
pub use recent::RecentDocuments;
pub use watch::{DeliverWatch, FileWatchEvent, FileWatchKind, FileWatchOptions, FileWatcher};
#[cfg(target_arch = "wasm32")]
pub use web::WebBackend;

/// One-shot delivery of a single dialog result; `None` means cancelled.
pub type DeliverFile = Box<dyn FnOnce(Option<PathBuf>) + Send>;

/// One-shot delivery of a multi-selection dialog result; `None` means
/// cancelled.
pub type DeliverFiles = Box<dyn FnOnce(Option<Vec<PathBuf>>) + Send>;

/// A selected file whose contents are portable across native and browser targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedFile {
    /// User-visible file name.
    pub name: String,
    /// Entire selected file contents.
    pub bytes: Arc<[u8]>,
    /// Native path when the platform exposes one; browsers return `None`.
    pub path: Option<PathBuf>,
}

/// Result of a completed byte-oriented save operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedFile {
    /// User-visible saved file name.
    pub name: String,
    /// Native destination when the platform exposes one; browsers return `None`.
    pub path: Option<PathBuf>,
}

/// Delivery of one portable selected file; `Ok(None)` means cancellation.
pub type DeliverSelectedFile = Box<dyn FnOnce(Result<Option<SelectedFile>, ServiceError>) + Send>;

/// Delivery of portable selected files; `Ok(None)` means cancellation.
pub type DeliverSelectedFiles =
    Box<dyn FnOnce(Result<Option<Vec<SelectedFile>>, ServiceError>) + Send>;

/// Delivery of a byte-oriented save; `Ok(None)` means cancellation.
pub type DeliverSavedFile = Box<dyn FnOnce(Result<Option<SavedFile>, ServiceError>) + Send>;

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

    /// Selects and reads one file on any supported target.
    fn pick_file_contents(&self, _options: FileDialogOptions, deliver: DeliverSelectedFile) {
        deliver(Err(ServiceError::UnsupportedPlatform));
    }

    /// Selects and reads multiple files on any supported target.
    fn pick_files_contents(&self, _options: FileDialogOptions, deliver: DeliverSelectedFiles) {
        deliver(Err(ServiceError::UnsupportedPlatform));
    }

    /// Chooses a destination and writes the supplied bytes.
    fn save_bytes(
        &self,
        _options: FileDialogOptions,
        _bytes: Arc<[u8]>,
        deliver: DeliverSavedFile,
    ) {
        deliver(Err(ServiceError::UnsupportedPlatform));
    }

    /// Starts watching one native path.
    fn watch(
        &self,
        _options: FileWatchOptions,
        _deliver: DeliverWatch,
    ) -> Result<FileWatcher, ServiceError> {
        Err(ServiceError::UnsupportedPlatform)
    }

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

#[derive(Clone)]
struct WatchSubscriptionConfig {
    backend: Arc<dyn ServiceBackend>,
    options: FileWatchOptions,
}

impl PartialEq for WatchSubscriptionConfig {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.backend, &other.backend) && self.options == other.options
    }
}

impl Eq for WatchSubscriptionConfig {}

impl fmt::Debug for WatchSubscriptionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WatchSubscriptionConfig")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
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

    /// Selects and reads one file, returning bytes on native and browser targets.
    pub fn pick_file_contents(
        &self,
        options: FileDialogOptions,
        deliver: impl FnOnce(Result<Option<SelectedFile>, ServiceError>) + Send + 'static,
    ) {
        self.backend.pick_file_contents(options, Box::new(deliver));
    }

    /// Selects and reads multiple files, returning bytes on native and browser targets.
    pub fn pick_files_contents(
        &self,
        options: FileDialogOptions,
        deliver: impl FnOnce(Result<Option<Vec<SelectedFile>>, ServiceError>) + Send + 'static,
    ) {
        self.backend.pick_files_contents(options, Box::new(deliver));
    }

    /// Chooses a destination and writes bytes without exposing a browser path.
    pub fn save_bytes(
        &self,
        options: FileDialogOptions,
        bytes: impl Into<Arc<[u8]>>,
        deliver: impl FnOnce(Result<Option<SavedFile>, ServiceError>) + Send + 'static,
    ) {
        self.backend
            .save_bytes(options, bytes.into(), Box::new(deliver));
    }

    /// Starts a debounced filesystem watcher. Native backends support this;
    /// browser backends return [`ServiceError::UnsupportedPlatform`].
    pub fn watch(
        &self,
        options: FileWatchOptions,
        deliver: impl FnMut(Result<FileWatchEvent, ServiceError>) + Send + 'static,
    ) -> Result<FileWatcher, ServiceError> {
        self.backend.watch(options, Box::new(deliver))
    }

    /// Describes a lifecycle-managed, debounced filesystem subscription.
    ///
    /// Events and startup failures are converted into application messages on
    /// the UI thread. Filesystem subscriptions retain only the latest pending
    /// event; [`rxui_app::DeliveryPolicy::Every`] is not supported for this
    /// invalidation-oriented source.
    pub fn watch_subscription<M: 'static>(
        &self,
        id: SubscriptionId,
        options: FileWatchOptions,
        map: impl FnMut(Result<FileWatchEvent, ServiceError>) -> M + 'static,
    ) -> Subscription<M> {
        let config = WatchSubscriptionConfig {
            backend: Arc::clone(&self.backend),
            options: options.clone(),
        };
        let services = self.clone();
        Subscription::service(
            id,
            SubscriptionKind::FileWatch,
            config,
            move |sink| {
                services
                    .watch(options, move |event| sink.emit(event))
                    .map_err(Err)
            },
            map,
        )
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
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use rxui_app::{
        App, AppCx, DeliveryPolicy, RuntimeInstrumentationConfig, RuntimeLifecycleEvent,
        SubscriptionId, SubscriptionKind, SubscriptionStatus, Subscriptions,
    };
    use rxui_testing::AppHarness;

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

    enum WatchMessage {
        SetPath(Option<PathBuf>),
        SetLabel(Rc<String>),
        Event(Rc<String>, Result<FileWatchEvent, ServiceError>),
        Noop,
    }

    struct WatchApp {
        services: DesktopServices,
        path: Option<PathBuf>,
        label: Rc<String>,
        events: Vec<(Rc<String>, Result<FileWatchEvent, ServiceError>)>,
    }

    impl App for WatchApp {
        type Message = WatchMessage;

        fn build(&mut self, _cx: &mut AppCx<'_, Self::Message>) -> rxui_app::Result<()> {
            Ok(())
        }

        fn subscriptions(&self) -> Subscriptions<Self::Message> {
            let Some(path) = self.path.clone() else {
                return Subscriptions::none();
            };
            let label = Rc::clone(&self.label);
            Subscriptions::one(self.services.watch_subscription(
                SubscriptionId::singleton("test.watch"),
                FileWatchOptions::new(path),
                move |event| WatchMessage::Event(Rc::clone(&label), event),
            ))
        }

        fn update(
            &mut self,
            _cx: &mut AppCx<'_, Self::Message>,
            message: Self::Message,
        ) -> rxui_app::Result<()> {
            match message {
                WatchMessage::SetPath(path) => self.path = path,
                WatchMessage::SetLabel(label) => self.label = label,
                WatchMessage::Event(label, event) => self.events.push((label, event)),
                WatchMessage::Noop => {}
            }
            Ok(())
        }
    }

    #[test]
    fn filesystem_subscription_reconciles_and_coalesces_latest_events() {
        let (services, backend) = services();
        let mut harness = AppHarness::new(WatchApp {
            services,
            path: Some(PathBuf::from("/one")),
            label: Rc::new("first".into()),
            events: Vec::new(),
        })
        .unwrap();
        assert_eq!(backend.watch_start_count(), 1);
        assert_eq!(backend.active_watch_count(), 1);

        harness
            .post(WatchMessage::SetLabel(Rc::new("newest".into())))
            .unwrap();
        harness.post(WatchMessage::Noop).unwrap();
        assert_eq!(backend.watch_start_count(), 1);

        backend.emit_watch(FileWatchEvent {
            paths: vec![PathBuf::from("/one/old")],
            kind: FileWatchKind::Modified,
        });
        backend.emit_watch(FileWatchEvent {
            paths: vec![PathBuf::from("/one/latest")],
            kind: FileWatchKind::Modified,
        });
        harness.advance(Duration::ZERO).unwrap();
        assert_eq!(harness.app().events.len(), 1);
        assert_eq!(harness.app().events[0].0.as_str(), "newest");
        assert_eq!(
            harness.app().events[0].1.as_ref().unwrap().paths,
            [PathBuf::from("/one/latest")]
        );
        backend.emit_watch_error(ServiceError::Backend("watch event failed".into()));
        harness.advance(Duration::ZERO).unwrap();
        assert_eq!(
            harness.app().events[1].1,
            Err(ServiceError::Backend("watch event failed".into()))
        );

        harness
            .post(WatchMessage::SetPath(Some(PathBuf::from("/two"))))
            .unwrap();
        assert_eq!(backend.watch_start_count(), 2);
        assert_eq!(backend.active_watch_count(), 1);
        assert!(backend.emit_stale_watch(
            0,
            FileWatchEvent {
                paths: vec![PathBuf::from("/one/stale")],
                kind: FileWatchKind::Modified,
            }
        ));
        harness.advance(Duration::ZERO).unwrap();
        assert_eq!(harness.app().events.len(), 2);
        let runtime = harness.runtime_snapshot();
        let snapshot = &runtime.active_subscriptions()[0];
        assert_eq!(snapshot.kind(), SubscriptionKind::FileWatch);
        assert_eq!(snapshot.interval(), None);
        assert_eq!(snapshot.status(), SubscriptionStatus::Running);

        harness.post(WatchMessage::SetPath(None)).unwrap();
        assert_eq!(backend.active_watch_count(), 0);
    }

    #[test]
    fn filesystem_subscription_reports_startup_failure_once_without_retrying() {
        let (services, backend) = services();
        backend.fail_next_watch(ServiceError::Backend("watch failed".into()));
        let mut harness = AppHarness::new_with_instrumentation(
            WatchApp {
                services,
                path: Some(PathBuf::from("/missing")),
                label: Rc::new("failed".into()),
                events: Vec::new(),
            },
            RuntimeInstrumentationConfig::default().lifecycle_history(4),
        )
        .unwrap();
        assert_eq!(backend.watch_start_count(), 1);
        assert_eq!(harness.app().events.len(), 1);
        assert_eq!(
            harness.app().events[0].1,
            Err(ServiceError::Backend("watch failed".into()))
        );
        assert_eq!(
            harness.runtime_snapshot().active_subscriptions()[0].status(),
            SubscriptionStatus::Failed
        );
        assert_eq!(
            harness.runtime_snapshot().lifecycle_traces()[0].event(),
            RuntimeLifecycleEvent::Failed
        );

        harness.post(WatchMessage::Noop).unwrap();
        assert_eq!(backend.watch_start_count(), 1);
        assert_eq!(harness.app().events.len(), 1);
    }

    #[test]
    fn filesystem_subscription_rejects_every_delivery() {
        let (services, _backend) = services();
        let subscriptions = Subscriptions::one(
            services
                .watch_subscription(
                    SubscriptionId::singleton("test.every-watch"),
                    FileWatchOptions::new("/tmp"),
                    |_| (),
                )
                .delivery(DeliveryPolicy::Every),
        );
        assert!(
            subscriptions
                .into_unique()
                .unwrap_err()
                .to_string()
                .contains("latest-value delivery only")
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
    fn portable_content_dialogs_preserve_bytes_and_errors() {
        let (services, backend) = services();
        backend.push_selected_file(Ok(Some(SelectedFile {
            name: "scene.json".into(),
            bytes: Arc::from(&b"{}"[..]),
            path: None,
        })));
        let selected = Arc::new(std::sync::Mutex::new(None));
        let sink = selected.clone();
        services.pick_file_contents(FileDialogOptions::new(), move |result| {
            *sink.lock().unwrap() = Some(result);
        });
        let selected = selected.lock().unwrap().take().unwrap().unwrap().unwrap();
        assert_eq!(selected.name, "scene.json");
        assert_eq!(selected.bytes.as_ref(), b"{}");

        let saved = Arc::new(std::sync::Mutex::new(None));
        let sink = saved.clone();
        services.save_bytes(
            FileDialogOptions::new().file_name("scene.json"),
            &b"payload"[..],
            move |result| *sink.lock().unwrap() = Some(result),
        );
        assert_eq!(backend.saves()[0].1.as_ref(), b"payload");
        assert_eq!(
            saved.lock().unwrap().take().unwrap().unwrap().unwrap().name,
            "scene.json"
        );
    }

    #[test]
    fn fake_watcher_stops_delivering_after_drop() {
        let (services, backend) = services();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let watcher = services
            .watch(FileWatchOptions::new("/tmp"), move |event| {
                sink.lock().unwrap().push(event.unwrap());
            })
            .unwrap();
        let event = FileWatchEvent {
            paths: vec![PathBuf::from("/tmp/a")],
            kind: FileWatchKind::Modified,
        };
        backend.emit_watch(event.clone());
        assert_eq!(*events.lock().unwrap(), vec![event]);
        drop(watcher);
        backend.emit_watch(FileWatchEvent {
            paths: vec![PathBuf::from("/tmp/b")],
            kind: FileWatchKind::Created,
        });
        assert_eq!(events.lock().unwrap().len(), 1);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_watcher_observes_a_temporary_file_change() {
        use std::sync::mpsc;
        use std::time::Duration;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("watched.txt");
        std::fs::write(&path, b"before").unwrap();
        let services = DesktopServices::native();
        let (send, receive) = mpsc::channel();
        let _watcher = services
            .watch(
                FileWatchOptions::new(&path).debounce(Duration::from_millis(20)),
                move |event| {
                    let _ = send.send(event);
                },
            )
            .unwrap();
        std::fs::write(&path, b"after").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let observed = loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let event = receive.recv_timeout(remaining).unwrap().unwrap();
            if event
                .paths
                .iter()
                .any(|changed| changed.file_name() == path.file_name())
            {
                break event;
            }
        };
        assert!(!observed.paths.is_empty());
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
