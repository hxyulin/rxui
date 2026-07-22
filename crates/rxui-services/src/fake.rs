//! Deterministic scripted backend for tests.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    DeliverFile, DeliverFiles, DeliverSavedFile, DeliverSelectedFile, DeliverSelectedFiles,
    DeliverWatch, FileDialogOptions, FileWatchEvent, FileWatchOptions, FileWatcher, SavedFile,
    SelectedFile, ServiceBackend, ServiceError,
};

struct WatchSink {
    active: Arc<AtomicBool>,
    deliver: DeliverWatch,
}

struct FakeWatchGuard(Arc<AtomicBool>);

impl Drop for FakeWatchGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Deterministic [`ServiceBackend`] for tests; never shows OS dialogs.
///
/// Single-path dialog results (`pick_file`, `pick_folder`, `save_file`) are
/// scripted with [`push_file`](Self::push_file) and multi-selection results
/// with [`push_files`](Self::push_files); each dialog call pops the next
/// scripted result and delivers it synchronously on the calling thread. An
/// empty queue delivers `None` (cancellation). Every dialog request and
/// launch call is recorded for assertions via
/// [`requests`](Self::requests) and [`launches`](Self::launches).
#[derive(Default)]
pub struct FakeBackend {
    files: Mutex<VecDeque<Option<PathBuf>>>,
    file_lists: Mutex<VecDeque<Option<Vec<PathBuf>>>>,
    requests: Mutex<Vec<FileDialogOptions>>,
    launches: Mutex<Vec<String>>,
    selected: Mutex<VecDeque<Result<Option<SelectedFile>, ServiceError>>>,
    selected_lists: Mutex<VecDeque<Result<Option<Vec<SelectedFile>>, ServiceError>>>,
    saves: Mutex<Vec<(FileDialogOptions, Arc<[u8]>)>>,
    watchers: Mutex<Vec<WatchSink>>,
}

impl std::fmt::Debug for FakeBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FakeBackend")
            .finish_non_exhaustive()
    }
}

impl FakeBackend {
    /// Creates a backend with no scripted results.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues the next single-path dialog result; `None` scripts a
    /// cancellation.
    pub fn push_file(&self, result: Option<PathBuf>) {
        self.files
            .lock()
            .expect("FakeBackend poisoned")
            .push_back(result);
    }

    /// Queues the next multi-selection dialog result; `None` scripts a
    /// cancellation.
    pub fn push_files(&self, result: Option<Vec<PathBuf>>) {
        self.file_lists
            .lock()
            .expect("FakeBackend poisoned")
            .push_back(result);
    }

    /// Queues a portable file-content result.
    pub fn push_selected_file(&self, result: Result<Option<SelectedFile>, ServiceError>) {
        self.selected
            .lock()
            .expect("FakeBackend poisoned")
            .push_back(result);
    }

    /// Queues a portable multi-file content result.
    pub fn push_selected_files(&self, result: Result<Option<Vec<SelectedFile>>, ServiceError>) {
        self.selected_lists
            .lock()
            .expect("FakeBackend poisoned")
            .push_back(result);
    }

    /// Returns byte-oriented save requests in call order.
    pub fn saves(&self) -> Vec<(FileDialogOptions, Arc<[u8]>)> {
        self.saves.lock().expect("FakeBackend poisoned").clone()
    }

    /// Delivers one event to every active fake watcher.
    pub fn emit_watch(&self, event: FileWatchEvent) {
        for sink in self
            .watchers
            .lock()
            .expect("FakeBackend poisoned")
            .iter_mut()
        {
            if sink.active.load(Ordering::Acquire) {
                (sink.deliver)(Ok(event.clone()));
            }
        }
    }

    /// Returns every dialog request received so far, in call order.
    pub fn requests(&self) -> Vec<FileDialogOptions> {
        self.requests.lock().expect("FakeBackend poisoned").clone()
    }

    /// Returns every launch call so far as `"<method>:<argument>"` strings
    /// (for example `"open_url:https://example.com"`), in call order.
    pub fn launches(&self) -> Vec<String> {
        self.launches.lock().expect("FakeBackend poisoned").clone()
    }

    fn record_request(&self, options: FileDialogOptions) {
        self.requests
            .lock()
            .expect("FakeBackend poisoned")
            .push(options);
    }

    fn next_file(&self) -> Option<PathBuf> {
        self.files
            .lock()
            .expect("FakeBackend poisoned")
            .pop_front()
            .unwrap_or(None)
    }
}

impl ServiceBackend for FakeBackend {
    fn pick_file(&self, options: FileDialogOptions, deliver: DeliverFile) {
        self.record_request(options);
        deliver(self.next_file());
    }

    fn pick_files(&self, options: FileDialogOptions, deliver: DeliverFiles) {
        self.record_request(options);
        let next = self
            .file_lists
            .lock()
            .expect("FakeBackend poisoned")
            .pop_front()
            .unwrap_or(None);
        deliver(next);
    }

    fn pick_folder(&self, options: FileDialogOptions, deliver: DeliverFile) {
        self.record_request(options);
        deliver(self.next_file());
    }

    fn save_file(&self, options: FileDialogOptions, deliver: DeliverFile) {
        self.record_request(options);
        deliver(self.next_file());
    }

    fn pick_file_contents(&self, options: FileDialogOptions, deliver: DeliverSelectedFile) {
        self.record_request(options);
        let result = self
            .selected
            .lock()
            .expect("FakeBackend poisoned")
            .pop_front()
            .unwrap_or(Ok(None));
        deliver(result);
    }

    fn pick_files_contents(&self, options: FileDialogOptions, deliver: DeliverSelectedFiles) {
        self.record_request(options);
        let result = self
            .selected_lists
            .lock()
            .expect("FakeBackend poisoned")
            .pop_front()
            .unwrap_or(Ok(None));
        deliver(result);
    }

    fn save_bytes(&self, options: FileDialogOptions, bytes: Arc<[u8]>, deliver: DeliverSavedFile) {
        self.record_request(options.clone());
        self.saves
            .lock()
            .expect("FakeBackend poisoned")
            .push((options.clone(), bytes));
        deliver(Ok(Some(SavedFile {
            name: options.file_name_ref().unwrap_or("download").to_owned(),
            path: None,
        })));
    }

    fn watch(
        &self,
        _options: FileWatchOptions,
        deliver: DeliverWatch,
    ) -> Result<FileWatcher, ServiceError> {
        let active = Arc::new(AtomicBool::new(true));
        self.watchers
            .lock()
            .expect("FakeBackend poisoned")
            .push(WatchSink {
                active: active.clone(),
                deliver,
            });
        Ok(FileWatcher::from_guard(FakeWatchGuard(active)))
    }

    fn open_url(&self, url: &str) -> Result<(), ServiceError> {
        self.launches
            .lock()
            .expect("FakeBackend poisoned")
            .push(format!("open_url:{url}"));
        Ok(())
    }

    fn open_path(&self, path: &Path) -> Result<(), ServiceError> {
        self.launches
            .lock()
            .expect("FakeBackend poisoned")
            .push(format!("open_path:{}", path.display()));
        Ok(())
    }

    fn reveal_path(&self, path: &Path) -> Result<(), ServiceError> {
        self.launches
            .lock()
            .expect("FakeBackend poisoned")
            .push(format!("reveal_path:{}", path.display()));
        Ok(())
    }
}
