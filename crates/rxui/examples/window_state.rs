//! Standalone geometry persistence and native file-drop example.
//! Run: cargo run -p rxui --example window_state --features native -- [state-file]
//! Geometry is retained in memory while running and written after orderly exit.
//! The default file is in the temporary directory; pass a path for durable storage.
use rxui::{WindowId, prelude::*};
use std::{
    cell::{Cell, RefCell},
    error::Error,
    path::{Path, PathBuf},
    rc::Rc,
};

struct Workspace {
    window: Option<WindowId>,
    geometry: Option<WindowGeometry>,
    hovered: Vec<PathBuf>,
    dropped: Vec<PathBuf>,
    storage: PathBuf,
    status: String,
}
impl View for Workspace {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let placement = self.geometry.map_or_else(
            || "Waiting for native geometry…".into(),
            |g| {
                format!(
                    "Normal content: {:.0} × {:.0} logical pixels · Position: {:?} · Maximized: {}",
                    g.inner_size[0], g.inner_size[1], g.outer_position, g.maximized
                )
            },
        );
        let hover = if self.hovered.is_empty() {
            "Drop files anywhere in this window.".into()
        } else {
            format!(
                "Hovering {} file(s). Drop to accept their paths.",
                self.hovered.len()
            )
        };
        let paths = column().gap(8.).children(
            self.dropped
                .iter()
                .enumerate()
                .map(|(index, path)| label(path.display().to_string()).key(index as u64)),
        );
        let area = scroll_area(paths)
            .fill_width()
            .fill_height()
            .into_element()
            .background(if self.hovered.is_empty() {
                ThemeColor::Surface
            } else {
                ThemeColor::ControlHover
            });
        column()
            .fill_width()
            .fill_height()
            .padding(24.)
            .gap(14.)
            .child(label("Window placement and file drops").font_size(26.))
            .child(label(placement))
            .child(label(format!(
                "Placement is saved on exit to {}",
                self.storage.display()
            )))
            .child(label(
                "Move/resize this window, close it, then rerun to restore placement.",
            ))
            .child(label(hover))
            .child(area)
            .child(
                row()
                    .gap(10.)
                    .child(button("Clear paths").on_click(cx.listener(|this, _, _| {
                        this.dropped.clear();
                        this.status = "Cleared accepted paths.".into();
                    })))
                    .child(button("Close").on_click(cx.listener(|_, _, cx| {
                        if let Some(window) = cx.window() {
                            cx.request_close(&window).unwrap();
                        }
                    }))),
            )
            .child(label(self.status.clone()))
    }
}
fn encode(geometry: WindowGeometry) -> String {
    let position = geometry
        .outer_position
        .map_or_else(|| "none".into(), |[x, y]| format!("{x} {y}"));
    format!(
        "rxui-window-geometry-v1\n{} {}\n{}\n{}\n",
        geometry.inner_size[0], geometry.inner_size[1], position, geometry.maximized
    )
}
fn decode(text: &str) -> Option<WindowGeometry> {
    let mut lines = text.lines();
    if lines.next()? != "rxui-window-geometry-v1" {
        return None;
    }
    let mut size = lines.next()?.split_whitespace();
    let width: f64 = size.next()?.parse().ok()?;
    let height: f64 = size.next()?.parse().ok()?;
    if size.next().is_some()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.
        || height <= 0.
    {
        return None;
    }
    let position = lines.next()?;
    let outer_position = if position == "none" {
        None
    } else {
        let mut parts = position.split_whitespace();
        let result = [parts.next()?.parse().ok()?, parts.next()?.parse().ok()?];
        if parts.next().is_some() {
            return None;
        }
        Some(result)
    };
    let maximized = lines.next()?.parse().ok()?;
    if lines.next().is_some() {
        return None;
    }
    Some(WindowGeometry {
        inner_size: [width, height],
        outer_position,
        maximized,
    })
}
fn load(path: &Path) -> (Option<WindowGeometry>, String) {
    match std::fs::read_to_string(path) {
        Ok(text) => match decode(&text) {
            Some(geometry) => (Some(geometry), "Restoring saved placement.".into()),
            None => (
                None,
                "Ignoring malformed saved placement; using defaults.".into(),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (None, "No saved placement yet.".into())
        }
        Err(error) => (None, format!("Could not read placement: {error}")),
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let storage = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("rxui-window-state-v1.txt"));
    let (saved, status) = load(&storage);
    let latest = Rc::new(Cell::new(saved));
    let changes = latest.clone();
    let view: Rc<RefCell<Option<rxui::WeakEntity<Workspace>>>> = Rc::default();
    let geometry_view = view.clone();
    let drop_view = view.clone();
    Application::new()
        .window_geometry_changed(move |window, geometry, cx| {
            if let Some(view) = geometry_view
                .borrow()
                .as_ref()
                .and_then(|weak| weak.upgrade())
            {
                view.update(cx, |this, _| {
                    if this.window == Some(window.id()) {
                        this.geometry = Some(geometry);
                        changes.set(Some(geometry));
                    }
                });
            }
        })
        .file_drop(move |window, event, cx| {
            if let Some(view) = drop_view.borrow().as_ref().and_then(|weak| weak.upgrade()) {
                view.update(cx, |this, _| {
                    if this.window != Some(window.id()) {
                        return;
                    }
                    match event {
                        FileDropEvent::Hovered(path) => {
                            if !this.hovered.contains(path) {
                                this.hovered.push(path.clone());
                            }
                        }
                        FileDropEvent::Cancelled => {
                            if !this.hovered.is_empty() {
                                this.status = "File drag cancelled.".into();
                            }
                            this.hovered.clear();
                        }
                        FileDropEvent::Dropped(path) => {
                            this.hovered.clear();
                            this.dropped.push(path.clone());
                            this.status = format!(
                                "Accepted {} path(s); no file contents have been read.",
                                this.dropped.len()
                            );
                        }
                    }
                });
            }
        })
        .run(|cx| {
            let root = cx.new(|_| Workspace {
                window: None,
                geometry: None,
                hovered: Vec::new(),
                dropped: Vec::new(),
                storage: storage.clone(),
                status,
            });
            *view.borrow_mut() = Some(root.downgrade());
            let mut options = WindowOptions::new()
                .title("RXUI window state")
                .size(900., 520.);
            if let Some(geometry) = saved {
                options = options.geometry(geometry);
            }
            let window = cx.open_window(options, root.clone())?;
            root.update(cx, |this, _| this.window = Some(window.id()));
            Ok(())
        })?;
    // The event loop is gone: no resize callback or rendered frame waits on disk.
    if let Some(geometry) = latest.get() {
        std::fs::write(storage, encode(geometry))?;
    }
    Ok(())
}
