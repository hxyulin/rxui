//! Compositor-mediated same-client dragging on an existing winit Wayland connection.
//!
//! Register before the initiating press. Pump this adapter from the application's
//! event loop before beginning a drag and while it is active. It creates its own
//! event queue on the existing connection; it never opens a second client connection
//! or steals winit's event handlers. Targets are compositor-provided surface-local
//! coordinates. Global pointer queries and explicit capture are not available.
use crate::{Capabilities, CaptureState, Error, Target};
use std::{
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    rc::Rc,
    sync::Arc,
};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop,
    protocol::{
        wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_pointer,
        wl_registry, wl_seat, wl_surface,
    },
};
use winit::{
    raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle},
    window::{Window, WindowId},
};

/// A compositor-reported drag update. Positions in targets are logical surface-local units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The compositor entered, moved within, or left a registered destination.
    TargetChanged {
        /// Initiating window.
        source: WindowId,
        /// Current known same-client target, or Unavailable outside it.
        target: Target,
    },
    /// The compositor delivered a successful drop to this registered client surface.
    Dropped {
        /// Initiating window.
        source: WindowId,
        /// Release destination; never inferred from rectangles.
        target: Target,
    },
    /// The source, seat or connection disappeared, or the application cancelled.
    Cancelled {
        /// Initiating window.
        source: WindowId,
    },
}
struct Registration {
    surface: wl_surface::WlSurface,
    window: Arc<Window>,
}
struct Seat {
    proxy: wl_seat::WlSeat,
    pointer: Option<wl_pointer::WlPointer>,
    device: Option<wl_data_device::WlDataDevice>,
    focus: Option<ObjectId>,
    press: Option<(WindowId, u32)>,
}
#[derive(Clone, Copy)]
struct Gesture {
    source: WindowId,
    seat: u32,
    target: Target,
    phase: CaptureState,
}
struct Offer {
    proxy: wl_data_offer::WlDataOffer,
    accepted: bool,
    action: Option<wl_data_device_manager::DndAction>,
}
#[derive(Default)]
struct State {
    source: Option<wl_data_source::WlDataSource>,
    mime: Option<String>,
    offers: HashMap<ObjectId, Offer>,
    active_offer: Option<ObjectId>,
    registrations: HashMap<ObjectId, Registration>,
    seats: HashMap<u32, Seat>,
    manager: Option<wl_data_device_manager::WlDataDeviceManager>,
    manager_name: Option<u32>,
    gesture: Option<Gesture>,
    events: VecDeque<Event>,
}
impl State {
    fn window(&self, surface: &ObjectId) -> Option<WindowId> {
        self.registrations.get(surface).map(|r| r.window.id())
    }
    fn cancel(&mut self) {
        if let Some(source) = self.source.take() {
            source.destroy();
        }
        self.mime = None;
        self.active_offer = None;
        for (_, offer) in self.offers.drain() {
            offer.proxy.destroy();
        }
        if let Some(mut g) = self.gesture.take() {
            if matches!(g.phase, CaptureState::Held(_)) {
                self.events.push_back(Event::Cancelled { source: g.source });
                g.phase = CaptureState::Lost(g.source);
                g.target = Target::Unavailable;
            }
            self.gesture = Some(g);
        }
    }
    fn update_target(&mut self, seat: u32, target: Target) {
        if let Some(g) = &mut self.gesture
            && g.seat == seat
            && matches!(g.phase, CaptureState::Held(_))
        {
            g.target = target;
            self.events.push_back(Event::TargetChanged {
                source: g.source,
                target,
            });
        }
    }
    fn drop_on(&mut self, seat: u32) {
        if let Some(g) = &mut self.gesture
            && g.seat == seat
            && matches!(g.phase, CaptureState::Held(_))
        {
            if matches!(g.target, Target::Window { .. }) {
                g.phase = CaptureState::Released(g.source);
                self.events.push_back(Event::Dropped {
                    source: g.source,
                    target: g.target,
                });
            } else {
                self.cancel();
            }
        }
    }
    fn bind_devices(&mut self, qh: &QueueHandle<Self>) {
        if let Some(manager) = &self.manager {
            for (&name, seat) in &mut self.seats {
                if seat.device.is_none() {
                    seat.device = Some(manager.get_data_device(&seat.proxy, qh, name));
                }
            }
        }
    }
}

/// Thread-bound Wayland adapter using the application's existing display and surfaces.
/// It retains registered windows until unregister/drop to keep borrowed native
/// handles alive. Unregister before application-controlled window removal.
///
/// This backend supports same-client compositor drag routing, not OS data transfer
/// or unrestricted pointer capture. Winit's connection must use libwayland (its
/// normal system backend), and remain owned by the application event-loop thread.
pub struct WaylandAdapter {
    queue: EventQueue<State>,
    state: State,
    connection: Connection,
    // Keep the borrowed wl_display alive until every guest proxy/queue has dropped.
    display_owner: winit::event_loop::OwnedDisplayHandle,
    display: std::ptr::NonNull<std::ffi::c_void>,
    _thread: PhantomData<Rc<()>>,
}
fn error(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}
impl WaylandAdapter {
    /// Attaches to the application's owned Wayland display and discovers globals.
    /// Pass `event_loop.owned_display_handle()`, then register participating windows.
    /// Registry discovery performs two initial round trips. Subsequent polling is nonblocking.
    pub fn new(display_owner: winit::event_loop::OwnedDisplayHandle) -> Result<Self, Error> {
        let display = match display_owner.display_handle().map_err(error)?.as_raw() {
            RawDisplayHandle::Wayland(h) => h.display,
            _ => return Err(error("display does not use Wayland")),
        };
        // SAFETY: the owned winit display handle retains the connection until after
        // all guest queues and proxies drop. It is stored last in this adapter.
        // Guest mode does not close the application's connection.
        let backend = unsafe { Backend::from_foreign_display(display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        let mut queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        let mut state = State::default();
        queue.roundtrip(&mut state).map_err(error)?;
        queue.roundtrip(&mut state).map_err(error)?;
        if state.manager.as_ref().is_none_or(|m| m.version() < 3)
            || !state
                .seats
                .values()
                .any(|s| s.pointer.is_some() && s.device.is_some())
        {
            return Err(error(
                "Wayland compositor offers no pointer seat and version-3 data device manager",
            ));
        }
        Ok(Self {
            queue,
            state,
            connection,
            display_owner,
            display,
            _thread: PhantomData,
        })
    }
    /// Registers a window on this exact display, retaining its surface lifetime.
    pub fn register(&mut self, window: &Arc<Window>) -> Result<(), Error> {
        if self.state.gesture.is_some() {
            return Err(error("cannot register during a drag session"));
        }
        match window.display_handle().map_err(error)?.as_raw() {
            RawDisplayHandle::Wayland(h) if h.display == self.display => {}
            _ => return Err(error("window belongs to another Wayland display")),
        }
        let ptr = match window.window_handle().map_err(error)?.as_raw() {
            RawWindowHandle::Wayland(h) => h.surface,
            _ => return Err(error("window is not a Wayland surface")),
        };
        // SAFETY: winit guarantees a live wl_surface for the borrowed window handle.
        // Registration retains the Arc<Window>; the imported proxy is never destroyed
        // or assigned a handler by this adapter, and drops before that retained window.
        let id =
            unsafe { ObjectId::from_ptr(wl_surface::WlSurface::interface(), ptr.as_ptr().cast()) }
                .map_err(error)?;
        let surface =
            wl_surface::WlSurface::from_id(&self.connection, id.clone()).map_err(error)?;
        if self.state.registrations.contains_key(&id) {
            return Err(error("surface already registered"));
        }
        self.state.registrations.insert(
            id,
            Registration {
                window: window.clone(),
                surface,
            },
        );
        Ok(())
    }
    /// Cancels a source gesture and removes stale destination identity before window removal.
    pub fn unregister(&mut self, window: WindowId) {
        if self.state.gesture.is_some_and(|g| {
            g.source == window
                || matches!(g.target, Target::Window {window: id, ..} if id == window)
        }) {
            self.state.cancel();
        }
        if let Some(g) = &mut self.state.gesture
            && (g.source == window
                || matches!(g.target, Target::Window {window:id,..} if id==window))
        {
            g.target = Target::Unavailable;
            g.phase = CaptureState::Lost(g.source);
        }
        self.state.events.retain(|event| !matches!(event,
            Event::Dropped { source, target } | Event::TargetChanged { source, target }
                if *source == window || matches!(target, Target::Window {window:id,..} if *id == window)));
        self.state
            .registrations
            .retain(|_, r| r.window.id() != window);
        for seat in self.state.seats.values_mut() {
            if seat.press.is_some_and(|(id, _)| id == window) {
                seat.press = None;
            }
        }
    }
    /// Dispatches this adapter's pending events and flushes its requests without blocking.
    /// Call from winit input handling and `about_to_wait`; use periodic wakes while active.
    pub fn pump(&mut self) -> Result<(), Error> {
        if let Err(e) = self.queue.dispatch_pending(&mut self.state) {
            self.state.cancel();
            return Err(error(e));
        }
        if let Err(e) = self.connection.flush() {
            self.state.cancel();
            return Err(error(e));
        }
        Ok(())
    }
    /// Starts a same-client drag using a native serial observed for this source press.
    /// The serial is consumed once, and is never synthesized from winit events.
    pub fn begin_capture(&mut self, source: WindowId) -> Result<(), Error> {
        self.pump()?;
        if self.state.gesture.is_some() {
            return Err(error("drag session already active; end it first"));
        }
        let (&seat_name, seat) = self
            .state
            .seats
            .iter_mut()
            .find(|(_, s)| s.press.is_some_and(|(id, _)| id == source))
            .ok_or_else(|| error("no current native primary press for this registered source"))?;
        let (_, serial) = seat.press.take().unwrap();
        let surface = self
            .state
            .registrations
            .values()
            .find(|r| r.window.id() == source)
            .map(|r| &r.surface)
            .ok_or_else(|| error("source is not registered"))?;
        let device = seat
            .device
            .as_ref()
            .ok_or_else(|| error("seat has no drag device"))?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let mime = format!(
            "application/x-draglet-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let data_source = self
            .state
            .manager
            .as_ref()
            .ok_or_else(|| error("data device manager removed"))?
            .create_data_source(&self.queue.handle(), seat_name);
        data_source.offer(mime.clone());
        data_source.set_actions(wl_data_device_manager::DndAction::Move);
        device.start_drag(Some(&data_source), surface, None, serial);
        self.state.source = Some(data_source);
        self.state.mime = Some(mime);
        self.state.gesture = Some(Gesture {
            source,
            seat: seat_name,
            target: Target::Unavailable,
            phase: CaptureState::Held(source),
        });
        if let Err(e) = self.connection.flush() {
            self.state.cancel();
            return Err(error(e));
        }
        Ok(())
    }
    /// Ends application participation. Wayland exposes no explicit pointer-ungrab request.
    /// Destroying the private data source cancels the compositor drag. New drags
    /// still require a fresh native press serial.
    pub fn end_capture(&mut self, source: WindowId) {
        if self.state.gesture.is_some_and(|g| g.source == source) {
            self.state.cancel();
            self.state.gesture = None;
        }
    }
    /// Takes compositor-routed target, drop and cancellation events.
    pub fn take_events(&mut self) -> Vec<Event> {
        self.state.events.drain(..).collect()
    }
    /// Current lifecycle as observed from compositor events.
    pub fn capture_state(&self) -> CaptureState {
        self.state.gesture.map_or(CaptureState::Idle, |g| g.phase)
    }
    /// Latest compositor-provided target, never a global-coordinate query.
    pub fn target_at_pointer(&self) -> Target {
        self.state.gesture.map_or(Target::Unavailable, |g| g.target)
    }
    /// Same-client targeting and cancellation are supported; explicit capture is not.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            authoritative_targeting: true,
            pointer_capture: false,
            capture_loss_notification: true,
        }
    }
}
impl Drop for WaylandAdapter {
    fn drop(&mut self) {
        self.state.cancel();
        for (_, offer) in self.state.offers.drain() {
            offer.proxy.destroy();
        }
        for seat in self.state.seats.values() {
            if let Some(device) = &seat.device
                && device.version() >= 2
            {
                device.release();
            }
            if let Some(pointer) = &seat.pointer
                && pointer.version() >= 3
            {
                pointer.release();
            }
            if seat.proxy.version() >= 5 {
                seat.proxy.release();
            }
        }
        // Imported surfaces are owned by winit and must not receive destroy requests.
        let _ = self.connection.flush();
        let _ = &self.display_owner;
    }
}
impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == "wl_seat" => {
                let proxy = registry.bind(name, version.min(7), qh, name);
                state.seats.insert(
                    name,
                    Seat {
                        proxy,
                        pointer: None,
                        device: None,
                        focus: None,
                        press: None,
                    },
                );
                state.bind_devices(qh);
            }
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == "wl_data_device_manager" => {
                state.manager = Some(registry.bind(name, version.min(3), qh, ()));
                state.manager_name = Some(name);
                state.bind_devices(qh);
            }
            wl_registry::Event::GlobalRemove { name } => {
                if state.gesture.is_some_and(|g| g.seat == name) || state.manager_name == Some(name)
                {
                    state.cancel();
                }
                if let Some(seat) = state.seats.remove(&name) {
                    if let Some(device) = seat.device
                        && device.version() >= 2
                    {
                        device.release();
                    }
                    if let Some(pointer) = seat.pointer
                        && pointer.version() >= 3
                    {
                        pointer.release();
                    }
                    if seat.proxy.version() >= 5 {
                        seat.proxy.release();
                    }
                }
                if state.manager_name == Some(name) {
                    state.manager = None;
                    state.manager_name = None;
                }
            }
            _ => {}
        }
    }
}
impl Dispatch<wl_seat::WlSeat, u32> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        name: &u32,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            let pointer_present = capabilities.contains(wl_seat::Capability::Pointer);
            if !pointer_present && state.gesture.is_some_and(|g| g.seat == *name) {
                state.cancel();
            }
            if let Some(s) = state.seats.get_mut(name) {
                if pointer_present && s.pointer.is_none() {
                    s.pointer = Some(seat.get_pointer(qh, *name));
                } else if !pointer_present {
                    s.press = None;
                    s.focus = None;
                    if let Some(pointer) = s.pointer.take()
                        && pointer.version() >= 3
                    {
                        pointer.release();
                    }
                }
            }
        }
    }
}
impl Dispatch<wl_pointer::WlPointer, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { surface, .. } => {
                if let Some(s) = state.seats.get_mut(name) {
                    s.focus = Some(surface.id());
                }
            }
            wl_pointer::Event::Leave { .. } => {
                if let Some(s) = state.seats.get_mut(name) {
                    s.focus = None;
                    s.press = None;
                }
            }
            wl_pointer::Event::Button {
                serial,
                button: 0x110,
                state: WEnum::Value(button_state),
                ..
            } => {
                let source = state
                    .seats
                    .get(name)
                    .and_then(|s| s.focus.as_ref())
                    .and_then(|id| state.window(id));
                if let Some(s) = state.seats.get_mut(name) {
                    s.press = if button_state == wl_pointer::ButtonState::Pressed {
                        source.map(|source| (source, serial))
                    } else {
                        None
                    };
                }
            }
            _ => {}
        }
    }
}
impl Dispatch<wl_data_device::WlDataDevice, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_data_device::WlDataDevice,
        event: wl_data_device::Event,
        name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::DataOffer { id } => {
                state.offers.insert(
                    id.id(),
                    Offer {
                        proxy: id,
                        accepted: false,
                        action: None,
                    },
                );
            }
            wl_data_device::Event::Selection { id } => {
                if let Some(offer) = id.and_then(|o| state.offers.remove(&o.id())) {
                    offer.proxy.destroy();
                }
            }
            wl_data_device::Event::Enter {
                serial,
                surface,
                x,
                y,
                id,
            } => {
                let own_session = state
                    .gesture
                    .is_some_and(|g| g.seat == *name && matches!(g.phase, CaptureState::Held(_)));
                let target = if own_session {
                    state
                        .window(&surface.id())
                        .map_or(Target::Unavailable, |window| Target::Window {
                            window,
                            position: [x, y],
                        })
                } else {
                    Target::Unavailable
                };
                let mut accepted = false;
                if let Some(offer) = id.as_ref().and_then(|o| state.offers.get_mut(&o.id())) {
                    accepted =
                        own_session && matches!(target, Target::Window { .. }) && offer.accepted;
                    offer
                        .proxy
                        .accept(serial, if accepted { state.mime.clone() } else { None });
                    if accepted {
                        offer.proxy.set_actions(
                            wl_data_device_manager::DndAction::Move,
                            wl_data_device_manager::DndAction::Move,
                        );
                    }
                }
                state.active_offer = if accepted {
                    id.map(|o| o.id())
                } else {
                    if let Some(offer) = id.and_then(|o| state.offers.remove(&o.id())) {
                        offer.proxy.destroy();
                    }
                    None
                };
                state.update_target(
                    *name,
                    if accepted {
                        target
                    } else {
                        Target::Unavailable
                    },
                );
            }
            wl_data_device::Event::Motion { x, y, .. } => {
                let target = state
                    .gesture
                    .map_or(Target::Unavailable, |g| match g.target {
                        Target::Window { window, .. } => Target::Window {
                            window,
                            position: [x, y],
                        },
                        _ => Target::Unavailable,
                    });
                state.update_target(*name, target);
            }
            wl_data_device::Event::Leave => {
                if let Some(offer) = state
                    .active_offer
                    .take()
                    .and_then(|id| state.offers.remove(&id))
                {
                    offer.proxy.destroy();
                }
                state.update_target(*name, Target::Unavailable);
            }
            wl_data_device::Event::Drop => {
                let offer = state
                    .active_offer
                    .take()
                    .and_then(|id| state.offers.remove(&id));
                if let Some(offer) = offer {
                    if offer.accepted
                        && offer.action == Some(wl_data_device_manager::DndAction::Move)
                    {
                        offer.proxy.finish();
                        state.drop_on(*name);
                    } else {
                        state.cancel();
                    }
                    offer.proxy.destroy();
                } else {
                    state.cancel();
                }
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, wl_data_device::WlDataDevice, [0 => (wl_data_offer::WlDataOffer, ())]);
}
impl Dispatch<wl_data_offer::WlDataOffer, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_data_offer::WlDataOffer,
        event: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(offer) = state.offers.get_mut(&proxy.id()) {
            match event {
                wl_data_offer::Event::Offer { mime_type } => {
                    if state.mime.as_ref() == Some(&mime_type) {
                        offer.accepted = true;
                    }
                }
                wl_data_offer::Event::Action {
                    dnd_action: WEnum::Value(action),
                } => offer.action = Some(action),
                _ => {}
            }
        }
    }
}
impl Dispatch<wl_data_source::WlDataSource, u32> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        _: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.source.as_ref().is_none_or(|s| s.id() != proxy.id()) {
            return;
        }
        match event {
            wl_data_source::Event::Cancelled => state.cancel(),
            wl_data_source::Event::DndFinished => {
                if state
                    .gesture
                    .is_some_and(|g| matches!(g.phase, CaptureState::Held(_)))
                {
                    state.cancel();
                } else {
                    if let Some(source) = state.source.take() {
                        source.destroy();
                    }
                    state.mime = None;
                }
            }
            // Data is held inside the application. This private MIME type is a
            // compositor handshake, not an exported application payload.
            wl_data_source::Event::Send { fd, .. } => drop(fd),
            _ => {}
        }
    }
}
delegate_noop!(State: ignore wl_data_device_manager::WlDataDeviceManager);

#[cfg(test)]
mod tests {
    use super::*;
    fn held() -> State {
        State {
            gesture: Some(Gesture {
                source: WindowId::from(1_u64),
                seat: 7,
                target: Target::Unavailable,
                phase: CaptureState::Held(WindowId::from(1_u64)),
            }),
            ..Default::default()
        }
    }
    #[test]
    fn only_active_seat_can_route_and_commit_a_compositor_drop() {
        let mut state = held();
        let target = Target::Window {
            window: WindowId::from(2_u64),
            position: [12.5, 24.],
        };
        state.update_target(8, target);
        state.drop_on(8);
        assert!(state.events.is_empty());
        state.update_target(7, target);
        state.drop_on(7);
        state.drop_on(7);
        assert_eq!(state.events.len(), 2);
        assert!(matches!(state.events.back(),Some(Event::Dropped { target:t,..}) if *t==target));
        state.update_target(7, Target::Unavailable);
        assert_eq!(state.gesture.unwrap().target, target);
    }
    #[test]
    fn leaving_client_surfaces_never_authorizes_a_drop() {
        let mut state = held();
        state.update_target(7, Target::Unavailable);
        state.drop_on(7);
        assert!(matches!(
            state.gesture.unwrap().phase,
            CaptureState::Lost(_)
        ));
        assert!(
            !state
                .events
                .iter()
                .any(|e| matches!(e, Event::Dropped { .. }))
        );
    }
    #[test]
    fn cancellation_is_sticky_and_delivered_once() {
        let mut state = held();
        state.cancel();
        state.cancel();
        state.update_target(
            7,
            Target::Window {
                window: WindowId::from(2_u64),
                position: [0., 0.],
            },
        );
        state.drop_on(7);
        assert_eq!(state.events.len(), 1);
        assert!(matches!(state.events[0], Event::Cancelled { .. }));
        assert!(matches!(
            state.gesture.unwrap().phase,
            CaptureState::Lost(_)
        ));
    }
}
