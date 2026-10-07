//! Persistable normal window bounds and monitor-aware restoration.
use super::*;
use astrelis_winit::winit::dpi::{LogicalSize, PhysicalPosition};

/// Persistable window placement. The application chooses storage and window keys.
/// `inner_size` is the normal (unmaximized) content size in logical pixels;
/// `outer_position` is the normal frame's top-left in physical desktop coordinates.
/// Fullscreen/minimized bounds never replace normal bounds. Position is unavailable
/// on platforms such as Wayland. Restoring clamps against current monitor bounds;
/// winit exposes full monitor bounds, not the OS usable work area.
///
/// ```no_run
/// use rxui::{Application, FileDropEvent, WindowGeometry, WindowOptions};
/// let saved = WindowGeometry {
///     inner_size: [900., 600.],
///     outer_position: Some([120, 80]),
///     maximized: false,
/// };
/// let options = WindowOptions::new().title("Workspace").geometry(saved);
/// let app = Application::new()
///     .window_geometry_changed(|window, geometry, _cx| {
///         // Retain under your own stable window key; debounce disk writes.
///         println!("{:?}: {:?}", window.id(), geometry);
///     })
///     .file_drop(|window, event, _cx| {
///         if let FileDropEvent::Dropped(path) = event {
///             println!("{:?} received {}", window.id(), path.display());
///         }
///     });
/// // Open a view with options inside app.run(...).
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowGeometry {
    /// Normal logical content width and height; both must be finite and positive.
    pub inner_size: [f64; 2],
    /// Normal physical desktop frame origin, or None to let the compositor place it.
    pub outer_position: Option<[i32; 2]>,
    /// Whether to reopen maximized, retaining normal bounds for later restoration.
    pub maximized: bool,
}
impl WindowGeometry {
    /// Creates normal bounds with compositor-selected position.
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            inner_size: [width, height],
            outer_position: None,
            maximized: false,
        }
    }
    pub(super) fn attributes(self, mut attributes: WindowAttributes) -> WindowAttributes {
        attributes.inner_size =
            Some(LogicalSize::new(self.inner_size[0], self.inner_size[1]).into());
        attributes.position = self
            .outer_position
            .map(|[x, y]| PhysicalPosition::new(x, y).into());
        attributes.maximized = self.maximized;
        attributes
    }
}
/// Remember raw DPI types until the native window supplies its actual scale.
#[derive(Clone, Copy)]
pub(super) struct Request {
    size: Option<Size>,
    position: Option<astrelis_winit::winit::dpi::Position>,
    maximized: bool,
}
impl Request {
    pub(super) fn new(attributes: &WindowAttributes) -> Self {
        Self {
            size: attributes.inner_size,
            position: attributes.position,
            maximized: attributes.maximized,
        }
    }
    pub(super) fn pending(self) -> Option<WindowGeometry> {
        if !matches!(self.size, Some(Size::Logical(_))) {
            return None;
        }
        let mut value = self.resolve(1.)?;
        if matches!(
            self.position,
            Some(astrelis_winit::winit::dpi::Position::Logical(_))
        ) {
            value.outer_position = None;
        }
        Some(value)
    }
    pub(super) fn resolve(self, scale: f64) -> Option<WindowGeometry> {
        let size = self.size?.to_logical::<f64>(scale);
        Some(WindowGeometry {
            inner_size: [size.width, size.height],
            outer_position: self.position.map(|p| {
                let p = p.to_physical::<i32>(scale);
                [p.x, p.y]
            }),
            maximized: self.maximized,
        })
    }
}
#[derive(Clone, Copy)]
pub(super) struct Monitor {
    pub position: [i32; 2],
    pub size: [u32; 2],
    pub scale: f64,
}
impl Monitor {
    fn valid(self) -> bool {
        self.size.into_iter().all(|v| v > 0) && self.scale.is_finite() && self.scale > 0.
    }
    fn contains(self, position: [i32; 2]) -> bool {
        (0..2).all(|axis| {
            let origin = i64::from(self.position[axis]);
            let point = i64::from(position[axis]);
            point >= origin && point < origin + i64::from(self.size[axis])
        })
    }
    fn distance(self, position: [i32; 2]) -> f64 {
        (0..2)
            .map(|axis| {
                let min = f64::from(self.position[axis]);
                let max = min + f64::from(self.size[axis]);
                let p = f64::from(position[axis]);
                (p - p.clamp(min, max)).powi(2)
            })
            .sum()
    }
}
/// Keeps the requested origin's nearest monitor; missing origins use host-preferred
/// order. A small margin reserves room for decorations without claiming work-area
/// knowledge. No monitor information means the OS must enforce placement itself.
pub(super) fn restore(mut geometry: WindowGeometry, monitors: &[Monitor]) -> WindowGeometry {
    let monitors = monitors.iter().copied().filter(|m| m.valid());
    let monitor = if let Some(position) = geometry.outer_position {
        monitors.min_by(|a, b| {
            a.distance(position)
                .total_cmp(&b.distance(position))
                .then_with(|| b.contains(position).cmp(&a.contains(position)))
        })
    } else {
        monitors.into_iter().next()
    };
    let Some(monitor) = monitor else {
        return geometry;
    };
    for axis in 0..2 {
        let decoration = if axis == 0 { 32. } else { 64. };
        let max = (f64::from(monitor.size[axis]) / monitor.scale - decoration).max(1.);
        geometry.inner_size[axis] = geometry.inner_size[axis].min(max);
    }
    if let Some(mut position) = geometry.outer_position {
        for (axis, coordinate) in position.iter_mut().enumerate() {
            let min = f64::from(monitor.position[axis]);
            let extent = geometry.inner_size[axis] * monitor.scale
                + (if axis == 0 { 32. } else { 64. }) * monitor.scale;
            let max = (min + f64::from(monitor.size[axis]) - extent).max(min);
            *coordinate = f64::from(*coordinate).clamp(min, max).round() as i32;
        }
        geometry.outer_position = Some(position);
    }
    geometry
}
pub(super) fn restore_attributes(
    attributes: WindowAttributes,
    monitors: &[Monitor],
) -> WindowAttributes {
    let request = Request::new(&attributes);
    let preferred = monitors
        .iter()
        .copied()
        .filter(|m| m.valid())
        .min_by(|a, b| {
            let score = |m: Monitor| {
                request.position.map_or((0., false), |p| {
                    let p = p.to_physical::<i32>(m.scale);
                    let point = [p.x, p.y];
                    (m.distance(point), !m.contains(point))
                })
            };
            let a = score(*a);
            let b = score(*b);
            a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1))
        });
    let scale = preferred.map_or(1., |m| m.scale);
    match request.resolve(scale) {
        Some(g) => restore(g, monitors).attributes(attributes),
        None => attributes,
    }
}
pub(super) fn monitors(
    event_loop: &astrelis_winit::winit::event_loop::ActiveEventLoop,
) -> Vec<Monitor> {
    event_loop
        .primary_monitor()
        .into_iter()
        .chain(event_loop.available_monitors())
        .map(|m| Monitor {
            position: [m.position().x, m.position().y],
            size: [m.size().width, m.size().height],
            scale: m.scale_factor(),
        })
        .collect()
}
pub(super) struct Sample {
    inner_size: [f64; 2],
    outer_position: Option<[i32; 2]>,
    maximized: bool,
    transient: bool,
}
impl Sample {
    fn read(window: &Window) -> Self {
        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        Self {
            inner_size: [size.width, size.height],
            outer_position: window.outer_position().ok().map(|p| [p.x, p.y]),
            maximized: window.is_maximized(),
            transient: window.is_minimized() == Some(true) || window.fullscreen().is_some(),
        }
    }
    fn update(self, previous: Option<WindowGeometry>) -> Option<WindowGeometry> {
        if self.transient
            || self
                .inner_size
                .into_iter()
                .any(|v| !v.is_finite() || v <= 0.)
        {
            return previous;
        }
        let mut geometry = match previous {
            Some(geometry) => geometry,
            None if self.maximized => return None, // Normal bounds have never been observed.
            None => WindowGeometry::new(self.inner_size[0], self.inner_size[1]),
        };
        geometry.maximized = self.maximized;
        if !self.maximized {
            geometry.inner_size = self.inner_size;
            geometry.outer_position = self.outer_position;
        }
        Some(geometry)
    }
}
pub(super) fn initial(
    window: &Window,
    requested: Option<WindowGeometry>,
) -> Option<WindowGeometry> {
    Sample::read(window).update(requested)
}
impl<F> Host<F> {
    pub(super) fn queue_geometry(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: NativeWindowId,
    ) -> Result<(), ApplicationError> {
        let Some(window) = self.windows.get_mut(&id) else {
            return Ok(());
        };
        if !std::mem::replace(&mut window.geometry_pending, true) {
            // macOS sends resize/move notifications inside zoom before isZoomed
            // settles. A proxy wake captures after the native operation returns.
            cx.proxy().send_event(Wake::Geometry(id)).map_err(|_| {
                ApplicationError::Native(Box::new(std::io::Error::other(
                    "native geometry wake failed",
                )))
            })?;
        }
        Ok(())
    }
    pub(super) fn observe_geometry(&mut self, id: NativeWindowId, initial: bool) {
        let Some(window) = self.windows.get(&id) else {
            return;
        };
        let Some(native) = window.life.native.borrow().clone() else {
            return;
        };
        let previous = window.life.geometry.get();
        let next = Sample::read(&native).update(previous);
        window.life.geometry.set(next);
        if !window.life.closing.get()
            && !self.commands.exited.get()
            && (initial || next != previous)
            && let Some(next) = next
            && let Some(hook) = &mut self.geometry_changed
        {
            let handle = WindowHandle {
                life: window.life.clone(),
            };
            self.runtime.update(|cx| hook(&handle, next, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_attributes_wait_for_actual_dpi_and_restore_converts_once() {
        let attrs = Window::default_attributes()
            .with_inner_size(astrelis_winit::winit::dpi::PhysicalSize::new(1600, 1200))
            .with_position(astrelis_winit::winit::dpi::PhysicalPosition::new(80, 120));
        let request = Request::new(&attrs);
        assert_eq!(request.pending(), None);
        assert_eq!(request.resolve(2.).unwrap().inner_size, [800., 600.]);
        let monitor = [Monitor {
            position: [0, 0],
            size: [2560, 1440],
            scale: 2.,
        }];
        let restored = restore_attributes(attrs, &monitor);
        assert_eq!(
            Request::new(&restored).resolve(2.).unwrap().inner_size,
            [800., 600.]
        );
        let logical = Window::default_attributes()
            .with_inner_size(LogicalSize::new(800., 600.))
            .with_position(astrelis_winit::winit::dpi::LogicalPosition::new(40., 60.));
        let request = Request::new(&logical);
        assert_eq!(request.pending().unwrap().outer_position, None);
        assert_eq!(request.resolve(2.).unwrap().outer_position, Some([80, 120]));
    }
    #[test]
    fn restore_handles_removed_monitors_negative_origins_dpi_and_oversized_bounds() {
        let monitors = [
            Monitor {
                position: [-1920, 0],
                size: [1920, 1080],
                scale: 1.,
            },
            Monitor {
                position: [0, 0],
                size: [2560, 1440],
                scale: 2.,
            },
        ];
        let normal = WindowGeometry {
            inner_size: [800., 600.],
            outer_position: Some([-1800, 80]),
            maximized: true,
        };
        assert_eq!(restore(normal, &monitors), normal);
        // Shared monitor edges belong to the display starting at that origin,
        // rather than an adjacent display whose bounds end there.
        let edge = WindowGeometry {
            outer_position: Some([0, 0]),
            ..normal
        };
        assert_eq!(restore(edge, &monitors), edge);
        let removed = WindowGeometry {
            outer_position: Some([9000, -9000]),
            inner_size: [5000., 5000.],
            ..normal
        };
        let restored = restore(removed, &monitors);
        assert_eq!(restored.outer_position, Some([0, 0]));
        assert_eq!(restored.inner_size, [1248., 656.]);
        assert!(restored.maximized);
        assert_eq!(restore(normal, &[]), normal);
        assert_eq!(
            restore(WindowGeometry::new(800., 600.), &monitors).outer_position,
            None
        );
        let tiny = [Monitor {
            position: [i32::MAX - 1, i32::MIN],
            size: [1, 1],
            scale: 2.,
        }];
        let result = restore(removed, &tiny);
        assert_eq!(result.inner_size, [1., 1.]);
        assert_eq!(result.outer_position, Some([i32::MAX - 1, i32::MIN]));
    }
    #[test]
    fn tracking_preserves_normal_bounds_across_maximize_fullscreen_and_minimize() {
        let normal = WindowGeometry {
            outer_position: Some([80, 100]),
            ..WindowGeometry::new(800., 600.)
        };
        let maximized = Sample {
            inner_size: [1920., 1080.],
            outer_position: Some([0, 0]),
            maximized: true,
            transient: false,
        }
        .update(Some(normal))
        .unwrap();
        assert_eq!(
            Sample {
                inner_size: [1920., 1080.],
                outer_position: Some([0, 0]),
                maximized: true,
                transient: false
            }
            .update(None),
            None
        );
        assert_eq!(maximized.inner_size, normal.inner_size);
        assert_eq!(maximized.outer_position, normal.outer_position);
        assert!(maximized.maximized);
        for (size, transient) in [
            ([0., 0.], false),
            ([1920., 1080.], true),
            ([f64::NAN, 600.], false),
        ] {
            assert_eq!(
                Sample {
                    inner_size: size,
                    outer_position: None,
                    maximized: false,
                    transient
                }
                .update(Some(maximized)),
                Some(maximized)
            );
        }
        assert_eq!(
            Sample {
                inner_size: normal.inner_size,
                outer_position: normal.outer_position,
                maximized: false,
                transient: false
            }
            .update(Some(maximized)),
            Some(normal)
        );
        assert_eq!(
            Sample {
                inner_size: normal.inner_size,
                outer_position: None,
                maximized: false,
                transient: false
            }
            .update(Some(normal))
            .unwrap()
            .outer_position,
            None
        );
    }
}
