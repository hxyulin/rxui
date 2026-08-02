//! Explicitly reviewed text-golden workflow.

use std::{ffi::OsStr, path::Path};

/// Compares normalized output with a checked-in golden.
///
/// Setting `RXUI_UPDATE_GOLDENS=1` writes `actual` to `path` instead. This is
/// intentionally opt-in so ordinary tests and CI never bless regressions.
pub fn assert_text_golden(actual: &str, expected: &str, path: impl AsRef<Path>) {
    if std::env::var_os("RXUI_UPDATE_GOLDENS").as_deref() == Some(OsStr::new("1")) {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create golden directory");
        }
        std::fs::write(path, actual).expect("write reviewed golden candidate");
    } else {
        assert_eq!(actual, expected);
    }
}
