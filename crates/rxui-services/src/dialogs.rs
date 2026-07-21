//! Pure data model describing native file dialog requests.

use std::path::{Path, PathBuf};

/// Named extension filter offered by a file dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    /// Human-readable filter name shown by the dialog (for example
    /// `"Images"`).
    pub name: String,
    /// File extensions without leading dots, case preserved, as `rfd`
    /// expects (for example `["png", "jpg"]`).
    pub extensions: Vec<String>,
}

/// Owned-`self` builder describing one file dialog request.
///
/// The options are a pure, platform-independent value; backends translate
/// them into their native dialog configuration.
///
/// ```
/// use rxui_services::FileDialogOptions;
///
/// let options = FileDialogOptions::new()
///     .title("Open Image")
///     .filter("Images", &["png", ".jpg"]);
/// assert_eq!(options.filters()[0].extensions, ["png", "jpg"]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDialogOptions {
    title: Option<String>,
    directory: Option<PathBuf>,
    file_name: Option<String>,
    filters: Vec<FileFilter>,
}

impl FileDialogOptions {
    /// Creates empty options; every unset field uses the platform default.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the dialog window title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the directory the dialog initially shows.
    pub fn directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }

    /// Sets the initially suggested file name (most useful for save dialogs).
    pub fn file_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    /// Appends a named extension filter.
    ///
    /// Extensions are normalized by stripping leading dots (`".png"`
    /// becomes `"png"`); case is preserved because `rfd` matches extensions
    /// as given.
    pub fn filter(mut self, name: impl Into<String>, extensions: &[&str]) -> Self {
        self.filters.push(FileFilter {
            name: name.into(),
            extensions: extensions
                .iter()
                .map(|extension| extension.trim_start_matches('.').to_string())
                .collect(),
        });
        self
    }

    /// Returns the configured dialog title.
    pub fn title_ref(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the configured starting directory.
    pub fn directory_ref(&self) -> Option<&Path> {
        self.directory.as_deref()
    }

    /// Returns the configured suggested file name.
    pub fn file_name_ref(&self) -> Option<&str> {
        self.file_name.as_deref()
    }

    /// Returns the accumulated extension filters in insertion order.
    pub fn filters(&self) -> &[FileFilter] {
        &self.filters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_accumulates_fields_and_filters() {
        let options = FileDialogOptions::new()
            .title("Open Project")
            .directory("/tmp/projects")
            .file_name("untitled.rxui")
            .filter("Projects", &["rxui"])
            .filter("All", &["*"]);
        assert_eq!(options.title_ref(), Some("Open Project"));
        assert_eq!(options.directory_ref(), Some(Path::new("/tmp/projects")));
        assert_eq!(options.file_name_ref(), Some("untitled.rxui"));
        assert_eq!(options.filters().len(), 2);
        assert_eq!(options.filters()[0].name, "Projects");
        assert_eq!(options.filters()[0].extensions, ["rxui"]);
        assert_eq!(options.filters()[1].name, "All");
    }

    #[test]
    fn filter_extensions_strip_leading_dots_and_preserve_case() {
        let options = FileDialogOptions::new().filter("Images", &[".png", "..jpeg", "PNG", "TifF"]);
        assert_eq!(
            options.filters()[0].extensions,
            ["png", "jpeg", "PNG", "TifF"]
        );
    }

    #[test]
    fn unset_fields_stay_empty() {
        let options = FileDialogOptions::new();
        assert_eq!(options.title_ref(), None);
        assert_eq!(options.directory_ref(), None);
        assert_eq!(options.file_name_ref(), None);
        assert!(options.filters().is_empty());
    }
}
