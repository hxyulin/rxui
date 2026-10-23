use super::*;
use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use x11rb::protocol::{
    shape::{ConnectionExt as _, SK, SO},
    xtest::ConnectionExt as _,
};
static SERVER: Mutex<()> = Mutex::new(());
static KEEPALIVE: OnceLock<Arc<RustConnection>> = OnceLock::new();
struct Fixture {
    owner: Arc<RustConnection>,
    host: X11DragHost,
    root: u32,
    source: u32,
    destination: u32,
}
fn sync(c: &RustConnection) {
    c.get_input_focus().unwrap().reply().unwrap();
}
fn window(c: &RustConnection, root: u32, x: i16, y: i16, w: u16, h: u16) -> u32 {
    let id = c.generate_id().unwrap();
    c.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        id,
        root,
        x,
        y,
        w,
        h,
        0,
        xproto::WindowClass::INPUT_OUTPUT,
        0,
        &xproto::CreateWindowAux::new()
            .override_redirect(1)
            .event_mask(
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            ),
    )
    .unwrap()
    .check()
    .unwrap();
    c.map_window(id).unwrap().check().unwrap();
    sync(c);
    id
}
fn fixture() -> Fixture {
    let display = std::env::var("ASTRELIS_X11_TEST_DISPLAY")
        .expect("set ASTRELIS_X11_TEST_DISPLAY to an isolated Xvfb server");
    KEEPALIVE.get_or_init(|| Arc::new(x11rb::connect(Some(&display)).unwrap().0));
    let (owner, screen) = x11rb::connect(Some(&display)).unwrap();
    let owner = Arc::new(owner);
    let root = owner.setup().roots[screen].root;
    let source = window(&owner, root, 30, 30, 200, 180);
    let destination = window(&owner, root, 330, 30, 200, 180);
    let mut host = X11DragHost::connect(Some(&display)).unwrap();
    host.register_xid(WindowId::from(1), source, 1.).unwrap();
    host.register_xid(WindowId::from(2), destination, 2.)
        .unwrap();
    Fixture {
        owner,
        host,
        root,
        source,
        destination,
    }
}
impl Fixture {
    fn motion(&self, x: i16, y: i16) {
        self.owner
            .xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, self.root, x, y, 0)
            .unwrap()
            .check()
            .unwrap();
        sync(&self.owner);
    }
    fn button(&self, down: bool) {
        self.owner
            .xtest_fake_input(
                if down {
                    xproto::BUTTON_PRESS_EVENT
                } else {
                    xproto::BUTTON_RELEASE_EVENT
                },
                1,
                0,
                self.root,
                0,
                0,
                0,
            )
            .unwrap()
            .check()
            .unwrap();
        sync(&self.owner);
    }
    fn press(&mut self) {
        self.host.arm(WindowId::from(1)).unwrap();
        self.motion(50, 50);
        self.button(true);
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            if matches!(self.host.poll_input().unwrap(),Some(X11Input::Pressed{window,..}) if window==WindowId::from(1))
            {
                break;
            }
            assert!(Instant::now() < end, "missing armed press");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.button(false);
        let _ = self.owner.destroy_window(self.source);
        let _ = self.owner.destroy_window(self.destination);
        let _ = self.owner.flush();
    }
}
fn wait_drag(
    drag: &mut CapturedDrag<'_, X11DragHost>,
    test: impl Fn(&RoutedDragUpdate) -> bool,
) -> RoutedDragUpdate {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(event) = drag.poll_x11().unwrap()
            && test(&event)
        {
            return event;
        }
        assert!(Instant::now() < end, "missing native drag event");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_stacking_foreign_occlusion_and_destination_scale() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.motion(350, 70);
    assert_eq!(
        f.host.target_at_pointer(),
        NativeDragTarget::Window {
            window: WindowId::from(2),
            position: LogicalPosition::new(10., 20.)
        }
    );
    let (foreign_client, _) =
        x11rb::connect(Some(&std::env::var("ASTRELIS_X11_TEST_DISPLAY").unwrap())).unwrap();
    let foreign = window(&foreign_client, f.root, 340, 50, 100, 100);
    assert_eq!(f.host.target_at_pointer(), NativeDragTarget::Foreign);
    f.owner
        .configure_window(
            f.destination,
            &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
        )
        .unwrap()
        .check()
        .unwrap();
    sync(&f.owner);
    assert!(
        matches!(f.host.target_at_pointer(),NativeDragTarget::Window{window,..} if window==WindowId::from(2))
    );
    f.owner.destroy_window(foreign).unwrap().check().unwrap();
    f.host.set_scale(WindowId::from(2), 1.25).unwrap();
    assert_eq!(
        f.host.target_at_pointer(),
        NativeDragTarget::Window {
            window: WindowId::from(2),
            position: LogicalPosition::new(16., 32.)
        }
    );
}
#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_input_shapes_and_unregistered_decorations_block_correctly() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    let (foreign_client, _) =
        x11rb::connect(Some(&std::env::var("ASTRELIS_X11_TEST_DISPLAY").unwrap())).unwrap();
    let foreign = window(&foreign_client, f.root, 330, 30, 100, 100);
    f.owner
        .shape_rectangles(
            SO::SET,
            SK::INPUT,
            xproto::ClipOrdering::UNSORTED,
            foreign,
            0,
            0,
            &[xproto::Rectangle {
                x: 0,
                y: 0,
                width: 20,
                height: 100,
            }],
        )
        .unwrap()
        .check()
        .unwrap();
    sync(&f.owner);
    f.motion(340, 70);
    assert_eq!(f.host.target_at_pointer(), NativeDragTarget::Foreign);
    f.motion(370, 70);
    assert!(
        matches!(f.host.target_at_pointer(),NativeDragTarget::Window{window,..} if window==WindowId::from(2))
    );
    f.owner.destroy_window(foreign).unwrap().check().unwrap();
    let frame = window(&f.owner, f.root, 600, 40, 200, 200);
    let content = window(&f.owner, frame, 0, 30, 200, 170);
    f.host.register_xid(WindowId::from(3), content, 1.).unwrap();
    f.motion(620, 50);
    assert_eq!(f.host.target_at_pointer(), NativeDragTarget::Foreign);
    f.motion(620, 100);
    assert_eq!(
        f.host.target_at_pointer(),
        NativeDragTarget::Window {
            window: WindowId::from(3),
            position: LogicalPosition::new(20., 30.)
        }
    );
    f.owner.destroy_window(frame).unwrap().check().unwrap();
}
#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_capture_delivers_outside_release_and_raii_releases_grab() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.press();
    assert!(matches!(
        CapturedDrag::begin(&mut f.host, WindowId::from(1), MouseButton::Right),
        Err(DragStartError::Unsupported)
    ));
    let mut drag = CapturedDrag::begin(&mut f.host, WindowId::from(1), MouseButton::Left).unwrap();
    let reply = f
        .owner
        .grab_pointer(
            false,
            f.source,
            EventMask::BUTTON_RELEASE,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            x11rb::CURRENT_TIME,
        )
        .unwrap()
        .reply()
        .unwrap();
    assert_eq!(reply.status, GrabStatus::ALREADY_GRABBED);
    f.owner
        .xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, f.root, 370, 90, 0)
        .unwrap()
        .check()
        .unwrap();
    sync(&f.owner);
    let moved = wait_drag(&mut drag, |e| matches!(e.update, DragUpdate::Moved(_)));
    assert_eq!(
        moved.update,
        DragUpdate::Moved(PhysicalPosition::new(340., 60.))
    );
    f.owner
        .xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, 1, 0, f.root, 0, 0, 0)
        .unwrap()
        .check()
        .unwrap();
    sync(&f.owner);
    let released = wait_drag(&mut drag, |e| e.update == DragUpdate::Released);
    assert_eq!(
        released.target,
        NativeDragTarget::Window {
            window: WindowId::from(2),
            position: LogicalPosition::new(20., 30.)
        }
    );
    drop(drag);
    let reply = f
        .owner
        .grab_pointer(
            false,
            f.source,
            EventMask::BUTTON_RELEASE,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            x11rb::CURRENT_TIME,
        )
        .unwrap()
        .reply()
        .unwrap();
    assert_eq!(reply.status, GrabStatus::SUCCESS);
    f.owner
        .ungrab_pointer(x11rb::CURRENT_TIME)
        .unwrap()
        .check()
        .unwrap();
}
#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_unmap_cancels_and_never_commits_later_release() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.press();
    let mut drag = CapturedDrag::begin(&mut f.host, WindowId::from(1), MouseButton::Left).unwrap();
    f.owner.unmap_window(f.source).unwrap().check().unwrap();
    sync(&f.owner);
    let cancelled = wait_drag(&mut drag, |e| matches!(e.update, DragUpdate::Cancelled(_)));
    assert_eq!(
        cancelled.update,
        DragUpdate::Cancelled(DragCancelReason::CaptureLost)
    );
    assert_eq!(cancelled.target, NativeDragTarget::Unavailable);
    assert!(!drag.is_active());
    drop(drag);
}
#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_cannot_take_over_winit_like_implicit_grab_after_press() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.motion(50, 50);
    f.button(true); // normal native owner receives press, no passive host grab
    let reply = f
        .host
        .connection
        .grab_pointer(
            false,
            f.source,
            EventMask::BUTTON_RELEASE,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            x11rb::CURRENT_TIME,
        )
        .unwrap()
        .reply()
        .unwrap();
    assert_eq!(reply.status, GrabStatus::ALREADY_GRABBED);
    assert!(f.host.acquire_capture(WindowId::from(1)).is_err());
}
#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_rejects_ended_press_and_does_not_reuse_hover() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.press();
    f.button(false);
    assert!(f.host.acquire_capture(WindowId::from(1)).is_err());
    f.host.poll_input().unwrap();
    f.motion(350, 70);
    assert!(matches!(
        f.host.target_at_pointer(),
        NativeDragTarget::Window { .. }
    ));
    f.owner
        .destroy_window(f.destination)
        .unwrap()
        .check()
        .unwrap();
    sync(&f.owner);
    assert!(
        !matches!(f.host.target_at_pointer(),NativeDragTarget::Window{window,..} if window==WindowId::from(2))
    );
}

#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_release_rejects_new_foreign_occlusion_and_drop_releases_active_capture() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.press();
    let mut drag = CapturedDrag::begin(&mut f.host, WindowId::from(1), MouseButton::Left).unwrap();
    f.owner
        .xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, f.root, 370, 90, 0)
        .unwrap()
        .check()
        .unwrap();
    sync(&f.owner);
    assert!(matches!(
        wait_drag(&mut drag, |e| matches!(e.update, DragUpdate::Moved(_))).target,
        NativeDragTarget::Window { .. }
    ));
    let (foreign_client, _) =
        x11rb::connect(Some(&std::env::var("ASTRELIS_X11_TEST_DISPLAY").unwrap())).unwrap();
    let foreign = window(&foreign_client, f.root, 340, 50, 100, 100);
    f.owner
        .xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, 1, 0, f.root, 0, 0, 0)
        .unwrap()
        .check()
        .unwrap();
    sync(&f.owner);
    assert_eq!(
        wait_drag(&mut drag, |e| e.update == DragUpdate::Released).target,
        NativeDragTarget::Foreign
    );
    drop(drag);
    f.owner.destroy_window(foreign).unwrap().check().unwrap();
    f.press();
    drop(CapturedDrag::begin(&mut f.host, WindowId::from(1), MouseButton::Left).unwrap());
    let reply = f
        .owner
        .grab_pointer(
            false,
            f.destination,
            EventMask::BUTTON_RELEASE,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            x11rb::CURRENT_TIME,
        )
        .unwrap()
        .reply()
        .unwrap();
    assert_eq!(reply.status, GrabStatus::SUCCESS);
    f.owner
        .ungrab_pointer(x11rb::CURRENT_TIME)
        .unwrap()
        .check()
        .unwrap();
}
#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_destroy_and_disconnect_cancel_without_drop() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.press();
    let mut drag = CapturedDrag::begin(&mut f.host, WindowId::from(1), MouseButton::Left).unwrap();
    f.owner.destroy_window(f.source).unwrap().check().unwrap();
    sync(&f.owner);
    assert!(matches!(
        wait_drag(&mut drag, |e| matches!(e.update, DragUpdate::Cancelled(_))).update,
        DragUpdate::Cancelled(DragCancelReason::CaptureLost)
    ));
    drop(drag);
    drop(f);
    let mut f = fixture();
    f.press();
    let host_resource = window(&f.host.connection, f.root, 800, 500, 10, 10);
    let mut drag = CapturedDrag::begin(&mut f.host, WindowId::from(1), MouseButton::Left).unwrap();
    f.owner.kill_client(host_resource).unwrap().check().unwrap();
    sync(&f.owner);
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        if drag.poll_x11().is_err() {
            break;
        }
        assert!(Instant::now() < end, "disconnect not observed");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!drag.is_active());
}

#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_queued_release_and_unmap_remap_cannot_revive_an_old_press() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.press();
    f.button(false);
    f.button(true);
    assert!(f.host.acquire_capture(WindowId::from(1)).is_err());
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        if matches!(f.host.poll_input().unwrap(), Some(X11Input::Pressed { .. })) {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(CapturedDrag::begin(&mut f.host, WindowId::from(1), MouseButton::Left).unwrap());
    f.button(false);
    f.press();
    f.owner.unmap_window(f.source).unwrap().check().unwrap();
    f.owner.map_window(f.source).unwrap().check().unwrap();
    sync(&f.owner);
    assert!(f.host.acquire_capture(WindowId::from(1)).is_err());
    assert!(matches!(
        f.host.poll_input().unwrap(),
        Some(X11Input::Cancelled { .. })
    ));
}

#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_unregister_handles_unpolled_press_and_preserves_another_sources_grab() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.host.arm(WindowId::from(1)).unwrap();
    f.motion(50, 50);
    f.button(true);
    f.host.unregister(WindowId::from(1)).unwrap();
    assert!(
        !matches!(f.host.poll_input().unwrap(),Some(X11Input::Pressed{window,..}) if window==WindowId::from(1))
    );
    let reply = f
        .owner
        .grab_pointer(
            false,
            f.destination,
            EventMask::BUTTON_RELEASE,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            x11rb::CURRENT_TIME,
        )
        .unwrap()
        .reply()
        .unwrap();
    assert_eq!(reply.status, GrabStatus::SUCCESS);
    f.owner
        .ungrab_pointer(x11rb::CURRENT_TIME)
        .unwrap()
        .check()
        .unwrap();
    f.button(false);
    drop(f);
    let mut f = fixture();
    f.host.arm(WindowId::from(2)).unwrap();
    f.motion(350, 70);
    f.button(true);
    f.host.unregister(WindowId::from(1)).unwrap();
    let reply = f
        .owner
        .grab_pointer(
            false,
            f.destination,
            EventMask::BUTTON_RELEASE,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            x11rb::CURRENT_TIME,
        )
        .unwrap()
        .reply()
        .unwrap();
    assert_eq!(reply.status, GrabStatus::ALREADY_GRABBED);
    assert!(
        matches!(f.host.poll_input().unwrap(),Some(X11Input::Pressed{window,..}) if window==WindowId::from(2))
    );
    drop(CapturedDrag::begin(&mut f.host, WindowId::from(2), MouseButton::Left).unwrap());
}

#[test]
#[ignore = "requires an isolated X server; set ASTRELIS_X11_TEST_DISPLAY"]
fn server_lost_capture_cleanup_preserves_a_newer_passive_gesture_and_its_release() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    for drop_without_poll in [false, true] {
        let mut f = fixture();
        f.host.arm(WindowId::from(2)).unwrap();
        f.press();
        let mut drag =
            CapturedDrag::begin(&mut f.host, WindowId::from(1), MouseButton::Left).unwrap();
        f.owner.unmap_window(f.source).unwrap().check().unwrap();
        sync(&f.owner);
        f.owner
            .xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, 1, 0, f.root, 0, 0, 0)
            .unwrap()
            .check()
            .unwrap();
        f.owner
            .xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, f.root, 350, 70, 0)
            .unwrap()
            .check()
            .unwrap();
        f.owner
            .xtest_fake_input(xproto::BUTTON_PRESS_EVENT, 1, 0, f.root, 0, 0, 0)
            .unwrap()
            .check()
            .unwrap();
        sync(&f.owner);
        if !drop_without_poll {
            assert!(matches!(
                wait_drag(&mut drag, |e| matches!(e.update, DragUpdate::Cancelled(_))).update,
                DragUpdate::Cancelled(DragCancelReason::CaptureLost)
            ));
            assert!(drag.poll_x11().unwrap().is_none());
        }
        drop(drag);
        let reply = f
            .owner
            .grab_pointer(
                false,
                f.destination,
                EventMask::BUTTON_RELEASE,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                x11rb::NONE,
                x11rb::NONE,
                x11rb::CURRENT_TIME,
            )
            .unwrap()
            .reply()
            .unwrap();
        assert_eq!(reply.status, GrabStatus::ALREADY_GRABBED);
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            if matches!(f.host.poll_input().unwrap(), Some(X11Input::Pressed {window,..}) if window==WindowId::from(2))
            {
                break;
            }
            assert!(Instant::now() < end, "new press disappeared");
            std::thread::sleep(Duration::from_millis(1));
        }
        f.button(false);
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            if matches!(f.host.poll_input().unwrap(),Some(X11Input::Released{window,..}) if window==WindowId::from(2))
            {
                break;
            }
            assert!(Instant::now() < end, "new source lost its release");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
