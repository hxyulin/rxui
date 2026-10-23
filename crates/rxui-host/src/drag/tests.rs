use super::*;
use winit::{dpi::PhysicalSize, event::DeviceId};

fn id(value: u64) -> WindowId {
    WindowId::from(value)
}
fn geometry(x: i32, y: i32, scale: f64, space: DesktopSpace) -> DragGeometry {
    DragGeometry::from_winit_parts(
        PhysicalPosition::new(x, y),
        WindowMetrics::new(PhysicalSize::new(1000, 800), scale).unwrap(),
        space,
    )
}
fn mouse(button: MouseButton, state: ElementState) -> WindowEvent {
    WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state,
        button,
    }
}
fn moved(x: f64, y: f64) -> WindowEvent {
    WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position: PhysicalPosition::new(x, y),
    }
}

#[test]
fn macos_origins_use_each_windows_scale_before_mapping() {
    // Cocoa origins (-800, 100), (-400, 50); winit scales each independently.
    let source = geometry(-1600, 200, 2., DesktopSpace::MacOsPoints);
    let destination = geometry(-400, 50, 1., DesktopSpace::MacOsPoints);
    let point = source
        .to_desktop(PhysicalPosition::new(1000., 100.))
        .unwrap();
    assert_eq!(point.position, PhysicalPosition::new(-300., 150.));
    assert_eq!(
        destination.to_logical(point),
        Some(LogicalPosition::new(100., 100.))
    );
    let back = destination
        .to_desktop(PhysicalPosition::new(100., 100.))
        .unwrap();
    assert_eq!(
        source.to_logical(back),
        Some(LogicalPosition::new(500., 50.))
    );
}

#[test]
fn physical_desktop_preserves_origin_and_uses_destination_density() {
    let source = geometry(-1200, -100, 1.25, DesktopSpace::PhysicalPixels);
    let destination = geometry(0, 0, 2., DesktopSpace::PhysicalPixels);
    let point = source
        .to_desktop(PhysicalPosition::new(1450.5, 300.))
        .unwrap();
    assert_eq!(
        destination.to_logical(point),
        Some(LogicalPosition::new(125.25, 100.))
    );
    // Out-of-content mapping is legal and makes no destination claim.
    assert_eq!(
        source.to_logical(point),
        Some(LogicalPosition::new(1160.4, 240.))
    );
    assert!(
        geometry(0, 0, 1., DesktopSpace::MacOsPoints)
            .to_logical(point)
            .is_none()
    );
    assert!(
        source
            .to_desktop(PhysicalPosition::new(f64::NAN, 0.))
            .is_none()
    );
}

#[test]
fn geometry_refresh_handles_move_and_scale_change() {
    let point = geometry(100, 200, 1., DesktopSpace::MacOsPoints)
        .to_desktop(PhysicalPosition::new(80., 40.))
        .unwrap();
    let changed = geometry(300, 300, 1.5, DesktopSpace::MacOsPoints);
    assert_eq!(
        changed.to_logical(point),
        Some(LogicalPosition::new(-20., 40.))
    );
}

#[test]
fn tracker_ignores_other_windows_buttons_and_cursor_exit() {
    let mut tracker = DragTracker::new(id(1), MouseButton::Left);
    assert_eq!(
        tracker.observe(id(2), &mouse(MouseButton::Left, ElementState::Released)),
        None
    );
    assert_eq!(
        tracker.observe(id(1), &mouse(MouseButton::Right, ElementState::Released)),
        None
    );
    assert_eq!(
        tracker.observe(
            id(1),
            &WindowEvent::CursorLeft {
                device_id: DeviceId::dummy()
            }
        ),
        None
    );
    assert_eq!(
        tracker.observe(id(1), &moved(-50., 100.)),
        Some(DragUpdate::Moved(PhysicalPosition::new(-50., 100.)))
    );
    assert_eq!(
        tracker.observe(id(1), &mouse(MouseButton::Left, ElementState::Released)),
        Some(DragUpdate::Released)
    );
    assert!(!tracker.is_active());
    assert_eq!(tracker.cancel(DragCancelReason::Explicit), None);
    assert_eq!(tracker.observe(id(1), &moved(0., 0.)), None);
}

#[test]
fn focus_and_close_cancel_once_and_never_accept_later_release() {
    for (event, reason) in [
        (WindowEvent::Focused(false), DragCancelReason::FocusLost),
        (WindowEvent::CloseRequested, DragCancelReason::SourceClosed),
        (WindowEvent::Destroyed, DragCancelReason::SourceClosed),
    ] {
        let mut tracker = DragTracker::new(id(1), MouseButton::Left);
        assert_eq!(
            tracker.observe(id(1), &event),
            Some(DragUpdate::Cancelled(reason))
        );
        assert_eq!(
            tracker.observe(id(1), &mouse(MouseButton::Left, ElementState::Released)),
            None
        );
    }
}

struct Host {
    capabilities: NativeDragCapabilities,
    fail: bool,
    acquired: usize,
    released: usize,
    queries: usize,
    target: NativeDragTarget,
    downgrade_after_query: bool,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            capabilities: NativeDragCapabilities {
                authoritative_targeting: true,
                pointer_capture: true,
                capture_loss_notification: true,
            },
            fail: false,
            acquired: 0,
            released: 0,
            queries: 0,
            target: NativeDragTarget::Window {
                window: id(2),
                position: LogicalPosition::new(20., 30.),
            },
            downgrade_after_query: false,
        }
    }
}
impl NativeDragHost for Host {
    type Error = &'static str;
    fn capabilities(&self) -> NativeDragCapabilities {
        if self.downgrade_after_query && self.queries > 0 {
            NativeDragCapabilities::WINIT_ONLY
        } else {
            self.capabilities
        }
    }
    fn acquire_capture(&mut self, _source: WindowId) -> Result<(), Self::Error> {
        self.acquired += 1;
        if self.fail {
            Err("capture rejected")
        } else {
            Ok(())
        }
    }
    fn release_capture(&mut self, _source: WindowId) {
        self.released += 1;
    }
    fn target_at_pointer(&mut self) -> NativeDragTarget {
        self.queries += 1;
        // Foreign window appeared between hover and release.
        if self.queries > 1 {
            NativeDragTarget::Foreign
        } else {
            self.target
        }
    }
}

#[test]
fn reliable_start_requires_all_capabilities_and_successful_capture() {
    assert!(!NativeDragCapabilities::WINIT_ONLY.reliable_routing());
    for capabilities in [
        NativeDragCapabilities::WINIT_ONLY,
        NativeDragCapabilities {
            authoritative_targeting: false,
            ..Host::default().capabilities
        },
        NativeDragCapabilities {
            pointer_capture: false,
            ..Host::default().capabilities
        },
        NativeDragCapabilities {
            capture_loss_notification: false,
            ..Host::default().capabilities
        },
    ] {
        let mut host = Host {
            capabilities,
            ..Host::default()
        };
        assert!(matches!(
            CapturedDrag::begin(&mut host, id(1), MouseButton::Left),
            Err(DragStartError::Unsupported)
        ));
        assert_eq!(host.acquired, 0);
    }
    let mut host = Host {
        fail: true,
        ..Host::default()
    };
    assert!(matches!(
        CapturedDrag::begin(&mut host, id(1), MouseButton::Left),
        Err(DragStartError::Capture("capture rejected"))
    ));
    assert_eq!(host.released, 0);
}

#[test]
fn release_requeries_native_target_before_releasing_capture() {
    let mut host = Host::default();
    {
        let mut drag = CapturedDrag::begin(&mut host, id(1), MouseButton::Left).unwrap();
        assert!(matches!(
            drag.observe(id(1), &moved(100., 200.)).unwrap().target,
            NativeDragTarget::Window { .. }
        ));
        let release = drag
            .observe(id(1), &mouse(MouseButton::Left, ElementState::Released))
            .unwrap();
        assert_eq!(release.update, DragUpdate::Released);
        assert_eq!(release.target, NativeDragTarget::Foreign);
        assert_eq!(drag.cancel(DragCancelReason::Explicit), None);
    }
    assert_eq!((host.acquired, host.queries, host.released), (1, 2, 1));
}

#[test]
fn unavailable_is_not_a_cached_hover_or_an_empty_desktop() {
    let mut host = Host {
        target: NativeDragTarget::Unavailable,
        ..Host::default()
    };
    let mut drag = CapturedDrag::begin(&mut host, id(1), MouseButton::Left).unwrap();
    assert_eq!(
        drag.observe(id(1), &mouse(MouseButton::Left, ElementState::Released))
            .unwrap()
            .target,
        NativeDragTarget::Unavailable
    );
}

#[test]
fn all_cancellation_paths_and_drop_release_capture_once() {
    for reason in [
        DragCancelReason::Explicit,
        DragCancelReason::CaptureLost,
        DragCancelReason::HostUnavailable,
        DragCancelReason::Suspended,
        DragCancelReason::SourceClosed,
    ] {
        let mut host = Host::default();
        {
            let mut drag = CapturedDrag::begin(&mut host, id(1), MouseButton::Left).unwrap();
            assert_eq!(
                drag.cancel(reason).unwrap().target,
                NativeDragTarget::Unavailable
            );
            assert_eq!(drag.cancel(reason), None);
        }
        assert_eq!((host.released, host.queries), (1, 0));
    }
    let mut host = Host::default();
    drop(CapturedDrag::begin(&mut host, id(1), MouseButton::Left).unwrap());
    assert_eq!(host.released, 1);
    let mut host = Host::default();
    let mut drag = CapturedDrag::begin(&mut host, id(1), MouseButton::Left).unwrap();
    assert_eq!(
        drag.observe(id(1), &WindowEvent::Focused(false))
            .unwrap()
            .update,
        DragUpdate::Cancelled(DragCancelReason::FocusLost)
    );
    drop(drag);
    assert_eq!((host.released, host.queries), (1, 0));
}

#[test]
fn capability_loss_cancels_and_host_can_be_recovered_for_next_drag() {
    let mut host = Host {
        downgrade_after_query: true,
        ..Host::default()
    };
    let mut drag = CapturedDrag::begin(&mut host, id(1), MouseButton::Left).unwrap();
    assert_eq!(drag.source(), id(1));
    assert!(drag.is_active());
    drag.observe(id(1), &moved(1., 2.)).unwrap();
    let terminal = drag
        .observe(id(1), &mouse(MouseButton::Left, ElementState::Released))
        .unwrap();
    assert_eq!(
        terminal.update,
        DragUpdate::Cancelled(DragCancelReason::HostUnavailable)
    );
    assert_eq!(terminal.target, NativeDragTarget::Unavailable);
    assert!(!drag.is_active());
    let recovered = drag.into_host();
    assert_eq!((recovered.released, recovered.queries), (1, 1));
    recovered.downgrade_after_query = false;
    let drag = CapturedDrag::begin(recovered, id(2), MouseButton::Right).unwrap();
    assert!(drag.is_active());
    let recovered = drag.into_host();
    assert_eq!(recovered.released, 2);
}
