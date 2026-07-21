//! Bounded most-recently-used document list.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Bounded, most-recent-first list of document paths.
///
/// The list serializes with serde and round-trips through
/// [`rxui_app::JsonStateStore`], so applications can persist it alongside
/// their other state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentDocuments {
    entries: Vec<PathBuf>,
    capacity: usize,
}

impl RecentDocuments {
    /// Creates an empty list that keeps at most `capacity` entries.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity,
        }
    }

    /// Records a use of `path`: deduplicates, moves it to the front, and
    /// truncates the list to capacity.
    pub fn touch(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.entries.retain(|entry| entry != &path);
        self.entries.insert(0, path);
        self.entries.truncate(self.capacity);
    }

    /// Removes `path`, returning whether it was present.
    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry != path);
        self.entries.len() != before
    }

    /// Removes every entry.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Drops entries whose paths no longer exist on the filesystem.
    ///
    /// The check requires native filesystem access; on `wasm32` this is a
    /// no-op.
    pub fn retain_existing(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.entries.retain(|entry| entry.exists());
    }

    /// Iterates entries from most to least recently used.
    pub fn iter(&self) -> impl Iterator<Item = &Path> {
        self.entries.iter().map(PathBuf::as_path)
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the list has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the maximum number of retained entries.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_deduplicates_moves_to_front_and_truncates() {
        let mut recent = RecentDocuments::new(3);
        recent.touch("/a");
        recent.touch("/b");
        recent.touch("/c");
        recent.touch("/a");
        assert_eq!(
            recent.iter().collect::<Vec<_>>(),
            [Path::new("/a"), Path::new("/c"), Path::new("/b")]
        );
        recent.touch("/d");
        assert_eq!(recent.len(), 3);
        assert_eq!(
            recent.iter().collect::<Vec<_>>(),
            [Path::new("/d"), Path::new("/a"), Path::new("/c")]
        );
    }

    #[test]
    fn remove_and_clear_report_membership() {
        let mut recent = RecentDocuments::new(4);
        recent.touch("/a");
        recent.touch("/b");
        assert!(recent.remove(Path::new("/a")));
        assert!(!recent.remove(Path::new("/a")));
        assert_eq!(recent.len(), 1);
        recent.clear();
        assert!(recent.is_empty());
    }

    #[test]
    fn retain_existing_drops_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let kept = dir.path().join("kept.txt");
        std::fs::write(&kept, b"x").unwrap();
        let mut recent = RecentDocuments::new(4);
        recent.touch(dir.path().join("missing.txt"));
        recent.touch(&kept);
        recent.retain_existing();
        assert_eq!(recent.iter().collect::<Vec<_>>(), [kept.as_path()]);
    }

    #[test]
    fn json_state_store_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = rxui_app::JsonStateStore::at(dir.path().join("recent.json"));
        let mut recent = RecentDocuments::new(2);
        recent.touch("/a");
        recent.touch("/b");
        store.save(1, &recent).unwrap();
        let restored: RecentDocuments = store.load(1).unwrap().unwrap();
        assert_eq!(restored, recent);
        assert_eq!(restored.capacity(), 2);
    }
}
