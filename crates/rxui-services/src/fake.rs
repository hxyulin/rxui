//! Deterministic scripted backend for tests.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::{DeliverFile, DeliverFiles, FileDialogOptions, ServiceBackend, ServiceError};

/// Deterministic [`ServiceBackend`] for tests; never shows OS dialogs.
///
/// Single-path dialog results (`pick_file`, `pick_folder`, `save_file`) are
/// scripted with [`push_file`](Self::push_file) and multi-selection results
/// with [`push_files`](Self::push_files); each dialog call pops the next
/// scripted result and delivers it synchronously on the calling thread. An
/// empty queue delivers `None` (cancellation). Every dialog request and
/// launch call is recorded for assertions via
/// [`requests`](Self::requests) and [`launches`](Self::launches).
#[derive(Debug, Default)]
pub struct FakeBackend {
    files: Mutex<VecDeque<Option<PathBuf>>>,
    file_lists: Mutex<VecDeque<Option<Vec<PathBuf>>>>,
    requests: Mutex<Vec<FileDialogOptions>>,
    launches: Mutex<Vec<String>>,
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
