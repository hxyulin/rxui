//! Versioned JSON state and safe native-window placement.

use std::{
    error::Error,
    fmt, fs, io,
    io::Write,
    path::{Path, PathBuf},
};

use astrelis_core::geometry::{Logical, Point, Size};
use astrelis_platform::{Monitor, Window, WindowAttributes, WindowEvent};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Versioned state file payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateEnvelope<T> {
    /// Application-controlled schema version.
    pub version: u32,
    /// Serialized application state.
    pub data: T,
}

/// Atomic JSON state stored in the platform configuration directory.
#[derive(Clone, Debug)]
pub struct JsonStateStore {
    path: PathBuf,
}

impl JsonStateStore {
    /// Creates a store at an explicit path.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    /// Creates a platform-conventional store for an application identity.
    pub fn for_app(
        qualifier: &str,
        organization: &str,
        application: &str,
    ) -> Result<Self, PersistError> {
        let dirs = ProjectDirs::from(qualifier, organization, application).ok_or_else(|| {
            PersistError::Identity("application identity has no platform config directory".into())
        })?;
        Ok(Self::at(dirs.config_dir().join("state.json")))
    }
    /// Returns the state file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Loads a matching version, returning `None` when the file does not exist.
    pub fn load<T: DeserializeOwned>(&self, version: u32) -> Result<Option<T>, PersistError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let envelope: StateEnvelope<T> = serde_json::from_slice(&bytes)?;
        if envelope.version != version {
            return Err(PersistError::Version {
                expected: version,
                found: envelope.version,
            });
        }
        Ok(Some(envelope.data))
    }
    /// Atomically replaces the state file with a serialized value.
    pub fn save<T: Serialize>(&self, version: u32, data: &T) -> Result<(), PersistError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| PersistError::Identity("state path has no parent".into()))?;
        fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec_pretty(&StateEnvelope { version, data })?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(&bytes)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}

/// State persistence failure.
#[derive(Debug)]
pub enum PersistError {
    /// The application identity or explicit path is unusable.
    Identity(String),
    /// Filesystem operation failed.
    Io(io::Error),
    /// JSON encoding or decoding failed.
    Json(serde_json::Error),
    /// Stored and requested schemas differ.
    Version {
        /// Requested schema version.
        expected: u32,
        /// Stored schema version.
        found: u32,
    },
}
impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(v) => f.write_str(v),
            Self::Io(e) => write!(f, "state I/O failed: {e}"),
            Self::Json(e) => write!(f, "state JSON is invalid: {e}"),
            Self::Version { expected, found } => {
                write!(f, "state schema {found} does not match expected {expected}")
            }
        }
    }
}
impl Error for PersistError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for PersistError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<serde_json::Error> for PersistError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// Restorable physical window geometry and state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlacement {
    /// Physical outer X position.
    pub x: i32,
    /// Physical outer Y position.
    pub y: i32,
    /// Physical client width.
    pub width: u32,
    /// Physical client height.
    pub height: u32,
    /// DPI scale at capture time.
    pub scale_factor: f64,
    /// Human-readable monitor hint.
    pub monitor: Option<String>,
    /// Whether the window should reopen maximized.
    pub maximized: bool,
}

impl WindowPlacement {
    /// Applies safe logical geometry to native creation attributes.
    pub fn apply(
        &self,
        attributes: &mut WindowAttributes,
        monitors: &[Monitor],
        primary: Option<&Monitor>,
    ) {
        let monitor = choose_monitor(self, monitors)
            .or(primary)
            .or_else(|| monitors.first());
        let Some(monitor) = monitor else {
            attributes.maximized = self.maximized;
            return;
        };
        let scale = monitor.scale_factor.max(f64::EPSILON);
        let max_width = monitor.size.width.max(320);
        let max_height = monitor.size.height.max(240);
        let width = self.width.clamp(320, max_width);
        let height = self.height.clamp(240, max_height);
        let visible = 48_i32;
        let min_x = monitor.position.x - i32::try_from(width).unwrap_or(i32::MAX) + visible;
        let max_x =
            monitor.position.x + i32::try_from(monitor.size.width).unwrap_or(i32::MAX) - visible;
        let min_y = monitor.position.y;
        let max_y =
            monitor.position.y + i32::try_from(monitor.size.height).unwrap_or(i32::MAX) - visible;
        let saved_is_visible = monitors
            .iter()
            .any(|candidate| overlap(self, candidate) > 0);
        let (x, y) = if saved_is_visible {
            (self.x.clamp(min_x, max_x), self.y.clamp(min_y, max_y))
        } else {
            (
                monitor.position.x + (monitor.size.width.saturating_sub(width) / 2) as i32,
                monitor.position.y + (monitor.size.height.saturating_sub(height) / 2) as i32,
            )
        };
        attributes.position = Some(Point::<Logical, f64>::new(
            x as f64 / scale,
            y as f64 / scale,
        ));
        attributes.inner_size = Some(Size::<Logical, f64>::new(
            width as f64 / scale,
            height as f64 / scale,
        ));
        attributes.maximized = self.maximized;
    }
}

/// Tracks the last non-maximized placement for one native window.
#[derive(Clone, Debug, Default)]
pub struct WindowPlacementTracker {
    placement: Option<WindowPlacement>,
}
impl WindowPlacementTracker {
    /// Starts a tracker from a restored placement.
    pub fn from_placement(placement: WindowPlacement) -> Self {
        Self {
            placement: Some(placement),
        }
    }

    /// Observes move, resize, and scale changes and refreshes normal geometry.
    pub fn handle_event(&mut self, window: &Window, event: &WindowEvent) {
        if matches!(
            event,
            WindowEvent::Moved(_)
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
        ) {
            self.capture(window);
        }
    }
    /// Queries and stores the current placement, preserving normal bounds while maximized.
    pub fn capture(&mut self, window: &Window) {
        let maximized = window.is_maximized();
        if maximized {
            if let Some(value) = &mut self.placement {
                value.maximized = true;
            }
            return;
        }
        let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
            return;
        };
        self.placement = Some(WindowPlacement {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            scale_factor: window.scale_factor(),
            monitor: window.current_monitor().and_then(|m| m.name),
            maximized: false,
        });
    }
    /// Returns the latest placement.
    pub const fn placement(&self) -> Option<&WindowPlacement> {
        self.placement.as_ref()
    }
}

fn overlap(value: &WindowPlacement, monitor: &Monitor) -> u64 {
    let left = value.x.max(monitor.position.x) as i64;
    let top = value.y.max(monitor.position.y) as i64;
    let right = (value.x as i64 + value.width as i64)
        .min(monitor.position.x as i64 + monitor.size.width as i64);
    let bottom = (value.y as i64 + value.height as i64)
        .min(monitor.position.y as i64 + monitor.size.height as i64);
    right.saturating_sub(left).max(0) as u64 * bottom.saturating_sub(top).max(0) as u64
}
fn choose_monitor<'a>(value: &WindowPlacement, monitors: &'a [Monitor]) -> Option<&'a Monitor> {
    monitors.iter().max_by_key(|monitor| {
        (
            overlap(value, monitor),
            monitor.name.as_ref() == value.monitor.as_ref(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use astrelis_platform::MonitorId;
    #[test]
    fn json_store_versions_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonStateStore::at(dir.path().join("state.json"));
        store.save(2, &vec![1, 2, 3]).unwrap();
        assert_eq!(store.load::<Vec<i32>>(2).unwrap(), Some(vec![1, 2, 3]));
        assert!(matches!(
            store.load::<Vec<i32>>(3),
            Err(PersistError::Version { .. })
        ));
    }
    #[test]
    fn removed_monitor_centers_on_primary() {
        let monitor = Monitor {
            id: MonitorId(1),
            name: Some("main".into()),
            position: Point::new(0, 0),
            size: Size::new(1920, 1080),
            scale_factor: 2.0,
        };
        let placement = WindowPlacement {
            x: 5000,
            y: 5000,
            width: 800,
            height: 600,
            scale_factor: 1.0,
            monitor: Some("gone".into()),
            maximized: false,
        };
        let mut attrs = WindowAttributes::default();
        placement.apply(&mut attrs, std::slice::from_ref(&monitor), Some(&monitor));
        assert_eq!(attrs.position, Some(Point::new(280.0, 120.0)));
    }
}
