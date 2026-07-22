//! Debounced native filesystem watching.

use std::{any::Any, path::PathBuf, time::Duration};

/// Broad, portable classification of one filesystem change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileWatchKind {
    /// A file or directory was created.
    Created,
    /// File metadata or contents changed.
    Modified,
    /// A file or directory was removed.
    Removed,
    /// A path was renamed; `paths` normally contains the old and new names.
    Renamed,
    /// The backend could not express the change more precisely.
    Other,
    /// The backend reports that callers should rescan the watched tree.
    Rescan,
}

/// One debounced filesystem notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileWatchEvent {
    /// Normalized paths associated with the change.
    pub paths: Vec<PathBuf>,
    /// Portable event classification.
    pub kind: FileWatchKind,
}

/// Configuration for one watched path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileWatchOptions {
    /// File or directory to watch.
    pub path: PathBuf,
    /// Whether directory descendants are included.
    pub recursive: bool,
    /// Quiet period used to coalesce platform event bursts.
    pub debounce: Duration,
}

impl FileWatchOptions {
    /// Watches one path with a 100 ms debounce interval.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            recursive: false,
            debounce: Duration::from_millis(100),
        }
    }

    /// Enables or disables recursive directory watching.
    #[must_use]
    pub const fn recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }

    /// Changes the debounce interval. Zero is repaired to one millisecond.
    #[must_use]
    pub fn debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce.max(Duration::from_millis(1));
        self
    }
}

/// Delivery callback used by filesystem watcher backends.
pub type DeliverWatch = Box<dyn FnMut(Result<FileWatchEvent, crate::ServiceError>) + Send>;

/// Live filesystem subscription. Dropping it stops event delivery.
pub struct FileWatcher {
    guard: Box<dyn Any + Send>,
}

impl std::fmt::Debug for FileWatcher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FileWatcher")
            .finish_non_exhaustive()
    }
}

impl FileWatcher {
    /// Wraps a backend guard whose drop terminates the subscription.
    #[doc(hidden)]
    pub fn from_guard(guard: impl Any + Send) -> Self {
        Self {
            guard: Box::new(guard),
        }
    }

    /// Keeps the backend guard observably live for diagnostics.
    pub fn is_active(&self) -> bool {
        let _ = &self.guard;
        true
    }
}
