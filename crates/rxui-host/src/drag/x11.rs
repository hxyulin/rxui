//! Opt-in X11 core-pointer host using a separate safe Rust protocol connection.
//!
//! Call [`X11DragHost::arm`] **before** the initiating press. It installs a passive
//! primary-button grab: primary input then arrives on this host's connection,
//! not winit's. Route [`X11Input`] into your UI, and pump the host while armed.
//! Starting after an ordinary winit press cannot take over winit's implicit grab.
//! Wayland and AppKit/Win32 windows are rejected. See `docs/native-drag-x11.md`.
use super::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Weak},
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xproto::{self, ConnectionExt, EventMask, GrabMode, GrabStatus, MapState},
    },
    rust_connection::RustConnection,
};

/// Failed X11 connection, protocol request, registration, or capture operation.
#[derive(Debug)]
pub struct X11DragError(String);
impl fmt::Display for X11DragError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Error for X11DragError {}
fn error(e: impl fmt::Display) -> X11DragError {
    X11DragError(e.to_string())
}

/// Primary-button input delivered by the host's native connection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum X11Input {
    /// An armed window received a real server button press.
    Pressed {
        /// Registered source.
        window: WindowId,
        /// X server timestamp for validating acquisition.
        timestamp: u32,
        /// Source physical client coordinates.
        position: PhysicalPosition<f64>,
    },
    /// Captured motion, including outside the source.
    Moved {
        /// Registered source.
        window: WindowId,
        /// Source physical client coordinates.
        position: PhysicalPosition<f64>,
    },
    /// Primary release; query the native destination before releasing capture.
    Released {
        /// Registered source.
        window: WindowId,
        /// Source physical client coordinates.
        position: PhysicalPosition<f64>,
    },
    /// Source became unviewable/destroyed, or its grab ended unexpectedly.
    Cancelled {
        /// Registered source.
        window: WindowId,
    },
}
// Multiple native queries must observe one stacking/input-shape tree. Holding
// only our Rust mutex cannot stop another X client from restacking between them.
struct ServerSnapshot<'a>(&'a RustConnection);
impl<'a> ServerSnapshot<'a> {
    fn new(connection: &'a RustConnection) -> Result<Self, X11DragError> {
        connection
            .grab_server()
            .map_err(error)?
            .check()
            .map_err(error)?;
        Ok(Self(connection))
    }
}
impl Drop for ServerSnapshot<'_> {
    fn drop(&mut self) {
        let _ = self.0.ungrab_server();
        let _ = self.0.flush();
    }
}
struct Registration {
    id: WindowId,
    scale: f64,
    window: Option<Weak<Window>>,
}

/// X11 core-pointer capture and live server hit testing for one screen.
///
/// Registrations must be on this connection's X server/screen. An armed primary
/// press is delivered here, so applications must bridge normal primary input as
/// well as drag input. Button chords and wheel input during a primary gesture
/// are outside this host subset; those events are not bridged. Outside a primary
/// grab, other-button delivery stays with the application. Keep the connection private while a drag
/// is active: no other code may change its grabs/event masks.
pub struct X11DragHost {
    connection: Arc<RustConnection>,
    root: xproto::Window,
    windows: HashMap<xproto::Window, Registration>,
    pending: VecDeque<X11Input>,
    press: Option<(xproto::Window, u32)>,
    wire_press: Option<(xproto::Window, u32)>,
    captured: Option<xproto::Window>,
    lost: bool,
    healthy: bool,
}
impl X11DragHost {
    /// Opens the named display (or `DISPLAY`) and selects its default screen.
    pub fn connect(display: Option<&str>) -> Result<Self, X11DragError> {
        if display.is_none() && std::env::var_os("WAYLAND_DISPLAY").is_some_and(|s| !s.is_empty()) {
            return Err(error(
                "a native Wayland session is not an X11 desktop; use compositor DnD",
            ));
        }
        let (connection, screen) = x11rb::connect(display).map_err(error)?;
        let root = connection.setup().roots[screen].root;
        Self::from_connection(Arc::new(connection), root)
    }
    /// Uses an application-owned connection, useful for custom X11 native hosts.
    /// Do not share capture/event-queue operations with another event consumer.
    /// Requires a standalone X11 desktop/server. Known Xwayland/XQuartz servers
    /// are rejected; their tree cannot describe foreign compositor windows.
    pub fn from_connection(
        connection: Arc<RustConnection>,
        root: xproto::Window,
    ) -> Result<Self, X11DragError> {
        if !connection
            .setup()
            .roots
            .iter()
            .any(|screen| screen.root == root)
        {
            return Err(error("invalid X screen root"));
        }
        for extension in [b"XWAYLAND".as_slice(), b"Apple-WM".as_slice()] {
            if connection
                .query_extension(extension)
                .map_err(error)?
                .reply()
                .map_err(error)?
                .present
            {
                return Err(error(
                    "nested X11 server cannot provide compositor-wide targeting",
                ));
            }
        }
        Ok(Self {
            connection,
            root,
            windows: HashMap::new(),
            pending: VecDeque::new(),
            press: None,
            wire_press: None,
            captured: None,
            lost: false,
            healthy: true,
        })
    }
    /// Registers a live winit Xlib/XCB client. No raw pointer is dereferenced.
    /// The display must match the one supplied to `connect`.
    pub fn register(&mut self, window: &Arc<Window>) -> Result<(), X11DragError> {
        let xid = match window.window_handle().map_err(error)?.as_raw() {
            RawWindowHandle::Xlib(h) => u32::try_from(h.window).map_err(error)?,
            RawWindowHandle::Xcb(h) => h.window.get(),
            _ => return Err(error("window is not an X11 client")),
        };
        self.register_xid(window.id(), xid, window.scale_factor())?;
        self.windows.get_mut(&xid).expect("registered").window = Some(Arc::downgrade(window));
        Ok(())
    }
    /// Registers an application-owned X11 client identity and logical scale.
    /// The caller owns this XID and must unregister before destroying/reusing it.
    /// Winit users should prefer `register`, which also tracks live window scale.
    pub fn register_xid(
        &mut self,
        id: WindowId,
        xid: xproto::Window,
        scale: f64,
    ) -> Result<(), X11DragError> {
        if !scale.is_finite() || scale <= 0. {
            return Err(error("invalid logical scale"));
        }
        if self.captured.is_some() || self.press.is_some() {
            return Err(error("cannot register during pointer gesture"));
        }
        if self.windows.contains_key(&xid) || self.windows.values().any(|r| r.id == id) {
            return Err(error("duplicate registration"));
        }
        let geometry = self
            .connection
            .get_geometry(xid)
            .map_err(error)?
            .reply()
            .map_err(error)?;
        if geometry.root != self.root {
            return Err(error("window belongs to another X screen"));
        }
        let attributes = self
            .connection
            .get_window_attributes(xid)
            .map_err(error)?
            .reply()
            .map_err(error)?;
        self.connection
            .change_window_attributes(
                xid,
                &xproto::ChangeWindowAttributesAux::new().event_mask(
                    attributes.your_event_mask
                        | EventMask::STRUCTURE_NOTIFY
                        | EventMask::LEAVE_WINDOW,
                ),
            )
            .map_err(error)?
            .check()
            .map_err(error)?;
        self.windows.insert(
            xid,
            Registration {
                id,
                scale,
                window: None,
            },
        );
        Ok(())
    }
    /// Updates raw registrations after an application DPI change.
    pub fn set_scale(&mut self, id: WindowId, scale: f64) -> Result<(), X11DragError> {
        if !scale.is_finite() || scale <= 0. {
            return Err(error("invalid logical scale"));
        }
        let registration = self
            .windows
            .values_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| error("unregistered window"))?;
        registration.scale = scale;
        Ok(())
    }
    /// Installs an unconstrained passive primary-button grab before a press.
    /// Normal primary presses/releases must now be routed from `poll_input`.
    /// A conflicting passive grab is an error, never a supported fallback.
    pub fn arm(&mut self, id: WindowId) -> Result<(), X11DragError> {
        let xid = self.xid(id)?;
        self.connection
            .grab_button(
                false,
                xid,
                EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::LEAVE_WINDOW,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                x11rb::NONE,
                x11rb::NONE,
                xproto::ButtonIndex::M1,
                xproto::ModMask::ANY,
            )
            .map_err(error)?
            .check()
            .map_err(error)?;
        self.connection.flush().map_err(error)
    }
    /// Removes a registration/passive grab. Cancel any active UI drag first.
    pub fn unregister(&mut self, id: WindowId) -> Result<(), X11DragError> {
        let xid = self.xid(id)?;
        // Stop future passive activation first, then synchronize all input that
        // may already have activated the grab but has not reached poll_input.
        self.connection
            .ungrab_button(xproto::ButtonIndex::M1, xid, xproto::ModMask::ANY)
            .map_err(error)?
            .check()
            .map_err(error)?;
        self.pump()?;
        if self.captured == Some(xid) {
            self.release_capture(id);
        }
        if let Some((pressed, time)) = self.wire_press
            && pressed == xid
        {
            self.connection
                .ungrab_pointer(time)
                .map_err(error)?
                .check()
                .map_err(error)?;
            self.wire_press = None;
        }
        self.windows.remove(&xid);
        self.pending.retain(|e| match e {
            X11Input::Pressed { window, .. }
            | X11Input::Moved { window, .. }
            | X11Input::Released { window, .. }
            | X11Input::Cancelled { window } => *window != id,
        });
        if self.press.is_some_and(|p| p.0 == xid) {
            self.press = None;
        }
        Ok(())
    }
    fn xid(&self, id: WindowId) -> Result<xproto::Window, X11DragError> {
        self.windows
            .iter()
            .find(|(_, r)| r.id == id)
            .map(|(&xid, _)| xid)
            .ok_or_else(|| error("unregistered window"))
    }
    fn ingest(&mut self, event: Event) {
        match event {
            Event::ButtonPress(e) if e.detail == 1 => {
                if let Some(r) = self.windows.get(&e.event) {
                    self.wire_press = Some((e.event, e.time));
                    self.pending.push_back(X11Input::Pressed {
                        window: r.id,
                        timestamp: e.time,
                        position: PhysicalPosition::new(e.event_x.into(), e.event_y.into()),
                    });
                }
            }
            Event::MotionNotify(e) => {
                if let Some(r) = self.windows.get(&e.event) {
                    self.pending.push_back(X11Input::Moved {
                        window: r.id,
                        position: PhysicalPosition::new(e.event_x.into(), e.event_y.into()),
                    });
                }
            }
            Event::ButtonRelease(e) if e.detail == 1 => {
                self.wire_press = None;
                if let Some(r) = self.windows.get(&e.event) {
                    self.pending.push_back(X11Input::Released {
                        window: r.id,
                        position: PhysicalPosition::new(e.event_x.into(), e.event_y.into()),
                    });
                }
            }
            Event::LeaveNotify(e)
                if e.mode == xproto::NotifyMode::UNGRAB && self.captured == Some(e.event) =>
            {
                self.mark_lost(e.event);
            }
            Event::UnmapNotify(e) => self.mark_lost(e.window),
            Event::DestroyNotify(e) => {
                self.mark_lost(e.window);
                self.windows.remove(&e.window);
            }
            _ => {}
        }
    }
    fn mark_lost(&mut self, xid: xproto::Window) {
        if self.wire_press.is_some_and(|p| p.0 == xid) {
            self.wire_press = None;
        }
        let pending_press = self.windows.get(&xid).is_some_and(|r| {
            self.pending
                .iter()
                .any(|e| matches!(e,X11Input::Pressed {window,..} if *window==r.id))
        });
        if self.captured == Some(xid) || self.press.is_some_and(|p| p.0 == xid) || pending_press {
            self.lost = true;
            if let Some(r) = self.windows.get(&xid) {
                self.pending.retain(|e| match e {
                    X11Input::Pressed { window, .. }
                    | X11Input::Moved { window, .. }
                    | X11Input::Released { window, .. }
                    | X11Input::Cancelled { window } => *window != r.id,
                });
                self.pending
                    .push_front(X11Input::Cancelled { window: r.id });
            }
        }
    }
    fn pump(&mut self) -> Result<(), X11DragError> {
        loop {
            match self.connection.poll_for_event().map_err(error) {
                Ok(Some(event)) => self.ingest(event),
                Ok(None) => return Ok(()),
                Err(e) => {
                    self.healthy = false;
                    self.lost = true;
                    return Err(e);
                }
            }
        }
    }
    /// Reads pending native primary input without blocking. Poll while armed,
    /// including during idle UI frames; winit does not read this connection.
    pub fn poll_input(&mut self) -> Result<Option<X11Input>, X11DragError> {
        self.pump()?;
        let next = self.pending.pop_front();
        if let Some(X11Input::Pressed {
            window, timestamp, ..
        }) = next
        {
            self.press = Some((self.xid(window)?, timestamp));
            if self.captured.is_none() {
                self.lost = false;
            }
        }
        if matches!(
            next,
            Some(X11Input::Released { .. } | X11Input::Cancelled { .. })
        ) && self.captured.is_none()
        {
            self.press = None;
        }
        Ok(next)
    }
    fn query_target(&mut self) -> Result<NativeDragTarget, X11DragError> {
        self.pump()?;
        if self.lost {
            return Ok(NativeDragTarget::Unavailable);
        }
        // Resolve cached winit metrics before the server grab. No window API,
        // UI callback or another client's round trip may run under the grab.
        let scales: HashMap<_, _> = self
            .windows
            .iter()
            .filter_map(|(&xid, r)| {
                let scale = if let Some(window) = &r.window {
                    window.upgrade()?.scale_factor()
                } else {
                    r.scale
                };
                (scale.is_finite() && scale > 0.).then_some((xid, scale))
            })
            .collect();
        let _snapshot = ServerSnapshot::new(&self.connection)?;
        if let Some(source) = self.captured {
            if !scales.contains_key(&source) {
                return Ok(NativeDragTarget::Unavailable);
            }
            let a = self
                .connection
                .get_window_attributes(source)
                .map_err(error)?
                .reply()
                .map_err(error)?;
            if a.map_state != MapState::VIEWABLE {
                return Ok(NativeDragTarget::Unavailable);
            }
        }
        let root = self
            .connection
            .query_pointer(self.root)
            .map_err(error)?
            .reply()
            .map_err(error)?;
        if !root.same_screen {
            return Ok(NativeDragTarget::Unavailable);
        }
        let mut child = root.child;
        if child == x11rb::NONE {
            return Ok(NativeDragTarget::NoWindow);
        }
        // Native input-shaped child traversal, never application rectangles.
        for _ in 0..128 {
            let point = self
                .connection
                .query_pointer(child)
                .map_err(error)?
                .reply()
                .map_err(error)?;
            if !point.same_screen
                || point.root != root.root
                || point.root_x != root.root_x
                || point.root_y != root.root_y
            {
                return Ok(NativeDragTarget::Unavailable);
            }
            if point.child != x11rb::NONE {
                child = point.child;
                continue;
            }
            let Some(r) = self.windows.get(&child) else {
                return Ok(NativeDragTarget::Foreign);
            };
            let Some(&scale) = scales.get(&child) else {
                return Ok(NativeDragTarget::Unavailable);
            };
            return Ok(NativeDragTarget::Window {
                window: r.id,
                position: LogicalPosition::new(
                    f64::from(point.win_x) / scale,
                    f64::from(point.win_y) / scale,
                ),
            });
        }
        Ok(NativeDragTarget::Unavailable)
    }
}

impl NativeDragHost for X11DragHost {
    type Error = X11DragError;
    fn supports_button(&self, button: MouseButton) -> bool {
        button == MouseButton::Left
    }
    fn capabilities(&self) -> NativeDragCapabilities {
        NativeDragCapabilities {
            authoritative_targeting: self.healthy,
            pointer_capture: self.healthy,
            capture_loss_notification: self.healthy,
        }
    }
    fn acquire_capture(&mut self, source: WindowId) -> Result<(), Self::Error> {
        self.pump()?;
        if self.lost {
            return Err(error("source was cancelled before acquisition"));
        }
        if self.captured.is_some() {
            return Err(error("capture already active"));
        }
        let xid = self.xid(source)?;
        let (pressed, time) = self
            .press
            .ok_or_else(|| error("pump the armed primary press before capture"))?;
        if pressed != xid {
            return Err(error("press belongs to another source"));
        }
        if self
            .pending
            .iter()
            .any(|e| matches!(e,X11Input::Released { window, .. } if *window==source))
        {
            return Err(error("initiating press already has a queued release"));
        }
        let point = self
            .connection
            .query_pointer(xid)
            .map_err(error)?
            .reply()
            .map_err(error)?;
        if !point.mask.contains(xproto::KeyButMask::BUTTON1) {
            return Err(error("primary press has already ended"));
        }
        let reply = self
            .connection
            .grab_pointer(
                false,
                xid,
                EventMask::BUTTON_RELEASE
                    | EventMask::BUTTON_PRESS
                    | EventMask::POINTER_MOTION
                    | EventMask::LEAVE_WINDOW,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                x11rb::NONE,
                x11rb::NONE,
                time,
            )
            .map_err(error)?
            .reply()
            .map_err(error)?;
        if reply.status != GrabStatus::SUCCESS {
            return Err(error(format!("X11 capture rejected: {:?}", reply.status)));
        }
        self.captured = Some(xid);
        self.lost = false;
        Ok(())
    }
    fn release_capture(&mut self, source: WindowId) {
        // Drop/cancellation can occur before the application polls native loss.
        // Observe it before deciding whether this connection still owns the old
        // gesture, rather than releasing a newer passive grab blindly.
        let _ = self.pump();
        if self
            .captured
            .is_some_and(|xid| self.windows.get(&xid).is_none_or(|r| r.id == source))
        {
            self.captured = None;
            if self
                .press
                .is_some_and(|(xid, _)| self.windows.get(&xid).is_none_or(|r| r.id == source))
            {
                self.press = None;
            }
            // Unmap/destroy/ungrab already ended the old native grab. A newer
            // passive gesture may now belong to another source on this same
            // connection; releasing CURRENT_TIME would steal that gesture.
            if self.lost {
                return;
            }
            self.wire_press = None;
            if let Err(e) = self
                .connection
                .ungrab_pointer(x11rb::CURRENT_TIME)
                .map_err(error)
                .and_then(|c| c.check().map_err(error))
                .and_then(|()| self.connection.flush().map_err(error))
            {
                self.healthy = false;
                log::error!("X11 capture release: {e}");
            }
        }
    }
    fn target_at_pointer(&mut self) -> NativeDragTarget {
        self.query_target().unwrap_or_else(|e| {
            log::error!("X11 native targeting: {e}");
            NativeDragTarget::Unavailable
        })
    }
}
impl Drop for X11DragHost {
    fn drop(&mut self) {
        // A passive grab may already be active even if its press is still in
        // the socket queue. This dedicated connection never owns unrelated grabs.
        self.captured = None;
        let _ = self.connection.ungrab_pointer(x11rb::CURRENT_TIME);
        for &xid in self.windows.keys() {
            let _ =
                self.connection
                    .ungrab_button(xproto::ButtonIndex::M1, xid, xproto::ModMask::ANY);
        }
        let _ = self.connection.flush();
    }
}
impl CapturedDrag<'_, X11DragHost> {
    /// Pumps the host's independent X connection during capture. Use an event
    /// loop wake/deadline while active; native winit motion is not the event source.
    /// A connection failure cancels the lease before returning an error.
    pub fn poll_x11(&mut self) -> Result<Option<RoutedDragUpdate>, X11DragError> {
        if !self.is_active() {
            return Ok(None);
        }
        let event = match self.host.as_mut().expect("lease host").poll_input() {
            Ok(e) => e,
            Err(e) => {
                self.cancel(DragCancelReason::CaptureLost);
                return Err(e);
            }
        };
        let Some(event) = event else {
            return Ok(None);
        };
        let (window, event) = match event {
            X11Input::Moved { window, position } => (
                window,
                WindowEvent::CursorMoved {
                    device_id: winit::event::DeviceId::dummy(),
                    position,
                },
            ),
            X11Input::Released { window, .. } => (
                window,
                WindowEvent::MouseInput {
                    device_id: winit::event::DeviceId::dummy(),
                    state: ElementState::Released,
                    button: MouseButton::Left,
                },
            ),
            X11Input::Cancelled { window } if window == self.source() => {
                return Ok(self.cancel(DragCancelReason::CaptureLost));
            }
            _ => return Ok(None),
        };
        Ok(self.observe(window, &event))
    }
}
#[cfg(test)]
mod tests;
