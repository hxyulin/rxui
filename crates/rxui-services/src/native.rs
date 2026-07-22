//! Native desktop backend backed by `rfd` dialogs and the `open` crate.

use std::{path::Path, sync::Arc, thread};

use notify_debouncer_full::{
    new_debouncer,
    notify::{RecursiveMode, event::ModifyKind},
};

use crate::{
    DeliverFile, DeliverFiles, DeliverSavedFile, DeliverSelectedFile, DeliverSelectedFiles,
    DeliverWatch, FileDialogOptions, FileWatchEvent, FileWatchKind, FileWatchOptions, FileWatcher,
    SavedFile, SelectedFile, ServiceBackend, ServiceError,
};

/// Native [`ServiceBackend`] for desktop targets.
///
/// Dialogs run through [`rfd::AsyncFileDialog`] on a dedicated spawned
/// thread (blocked on with `pollster`), so the UI thread never waits; each
/// delivery closure is invoked exactly once. URL and path launching uses
/// [`open::that_detached`]. Revealing a path uses `open -R` on macOS and
/// `explorer /select,` on Windows; other Unix targets have no portable
/// "reveal" verb, so the parent directory is opened instead.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeBackend;

impl NativeBackend {
    /// Creates the native backend.
    pub fn new() -> Self {
        Self
    }
}

fn build_dialog(options: &FileDialogOptions) -> rfd::AsyncFileDialog {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(title) = options.title_ref() {
        dialog = dialog.set_title(title);
    }
    if let Some(directory) = options.directory_ref() {
        dialog = dialog.set_directory(directory);
    }
    if let Some(file_name) = options.file_name_ref() {
        dialog = dialog.set_file_name(file_name);
    }
    for filter in options.filters() {
        dialog = dialog.add_filter(&filter.name, &filter.extensions);
    }
    dialog
}

impl ServiceBackend for NativeBackend {
    fn pick_file(&self, options: FileDialogOptions, deliver: DeliverFile) {
        thread::spawn(move || {
            let handle = pollster::block_on(build_dialog(&options).pick_file());
            deliver(handle.map(|file| file.path().to_path_buf()));
        });
    }

    fn pick_files(&self, options: FileDialogOptions, deliver: DeliverFiles) {
        thread::spawn(move || {
            let handles = pollster::block_on(build_dialog(&options).pick_files());
            deliver(handles.map(|files| {
                files
                    .into_iter()
                    .map(|file| file.path().to_path_buf())
                    .collect()
            }));
        });
    }

    fn pick_folder(&self, options: FileDialogOptions, deliver: DeliverFile) {
        thread::spawn(move || {
            let handle = pollster::block_on(build_dialog(&options).pick_folder());
            deliver(handle.map(|folder| folder.path().to_path_buf()));
        });
    }

    fn save_file(&self, options: FileDialogOptions, deliver: DeliverFile) {
        thread::spawn(move || {
            let handle = pollster::block_on(build_dialog(&options).save_file());
            deliver(handle.map(|file| file.path().to_path_buf()));
        });
    }

    fn pick_file_contents(&self, options: FileDialogOptions, deliver: DeliverSelectedFile) {
        thread::spawn(move || {
            let Some(handle) = pollster::block_on(build_dialog(&options).pick_file()) else {
                deliver(Ok(None));
                return;
            };
            let path = handle.path().to_path_buf();
            let result = std::fs::read(&path)
                .map(|bytes| {
                    Some(SelectedFile {
                        name: handle.file_name(),
                        bytes: bytes.into(),
                        path: Some(path),
                    })
                })
                .map_err(ServiceError::from_display);
            deliver(result);
        });
    }

    fn pick_files_contents(&self, options: FileDialogOptions, deliver: DeliverSelectedFiles) {
        thread::spawn(move || {
            let Some(handles) = pollster::block_on(build_dialog(&options).pick_files()) else {
                deliver(Ok(None));
                return;
            };
            let result = handles
                .into_iter()
                .map(|handle| {
                    let path = handle.path().to_path_buf();
                    std::fs::read(&path)
                        .map(|bytes| SelectedFile {
                            name: handle.file_name(),
                            bytes: bytes.into(),
                            path: Some(path),
                        })
                        .map_err(ServiceError::from_display)
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some);
            deliver(result);
        });
    }

    fn save_bytes(&self, options: FileDialogOptions, bytes: Arc<[u8]>, deliver: DeliverSavedFile) {
        thread::spawn(move || {
            let Some(handle) = pollster::block_on(build_dialog(&options).save_file()) else {
                deliver(Ok(None));
                return;
            };
            let path = handle.path().to_path_buf();
            let result = std::fs::write(&path, bytes.as_ref())
                .map(|()| {
                    Some(SavedFile {
                        name: handle.file_name(),
                        path: Some(path),
                    })
                })
                .map_err(ServiceError::from_display);
            deliver(result);
        });
    }

    fn watch(
        &self,
        options: FileWatchOptions,
        mut deliver: DeliverWatch,
    ) -> Result<FileWatcher, ServiceError> {
        let mut debouncer = new_debouncer(
            options.debounce,
            None,
            move |result: notify_debouncer_full::DebounceEventResult| match result {
                Ok(events) => {
                    for event in events {
                        let kind = if event.need_rescan() {
                            FileWatchKind::Rescan
                        } else {
                            match event.kind {
                                notify_debouncer_full::notify::EventKind::Create(_) => {
                                    FileWatchKind::Created
                                }
                                notify_debouncer_full::notify::EventKind::Modify(
                                    ModifyKind::Name(_),
                                ) => FileWatchKind::Renamed,
                                notify_debouncer_full::notify::EventKind::Modify(_) => {
                                    FileWatchKind::Modified
                                }
                                notify_debouncer_full::notify::EventKind::Remove(_) => {
                                    FileWatchKind::Removed
                                }
                                _ => FileWatchKind::Other,
                            }
                        };
                        deliver(Ok(FileWatchEvent {
                            paths: event.paths.clone(),
                            kind,
                        }));
                    }
                }
                Err(errors) => {
                    let message = errors
                        .into_iter()
                        .map(|error| error.to_string())
                        .collect::<Vec<_>>()
                        .join("; ");
                    deliver(Err(ServiceError::Backend(message)));
                }
            },
        )
        .map_err(ServiceError::from_display)?;
        debouncer
            .watch(
                &options.path,
                if options.recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                },
            )
            .map_err(ServiceError::from_display)?;
        Ok(FileWatcher::from_guard(debouncer))
    }

    fn open_url(&self, url: &str) -> Result<(), ServiceError> {
        open::that_detached(url).map_err(ServiceError::from_display)
    }

    fn open_path(&self, path: &Path) -> Result<(), ServiceError> {
        open::that_detached(path).map_err(ServiceError::from_display)
    }

    fn reveal_path(&self, path: &Path) -> Result<(), ServiceError> {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg("-R")
                .arg(path)
                .spawn()
                .map(|_child| ())
                .map_err(ServiceError::from_display)
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn()
                .map(|_child| ())
                .map_err(ServiceError::from_display)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            // No portable "reveal in file manager" verb exists here; opening
            // the parent directory is the closest supported behavior.
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
            open::that_detached(parent.unwrap_or(path)).map_err(ServiceError::from_display)
        }
    }
}
