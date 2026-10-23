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
    host: X11Adapter,
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
    let display = std::env::var("DRAGLET_X11_TEST_DISPLAY")
        .expect("set DRAGLET_X11_TEST_DISPLAY to an isolated Xvfb server");
    KEEPALIVE.get_or_init(|| Arc::new(x11rb::connect(Some(&display)).unwrap().0));
    let (owner, screen) = x11rb::connect(Some(&display)).unwrap();
    let owner = Arc::new(owner);
    let root = owner.setup().roots[screen].root;
    let source = window(&owner, root, 30, 30, 200, 180);
    let destination = window(&owner, root, 330, 30, 200, 180);
    let mut host = X11Adapter::connect(Some(&display)).unwrap();
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
#[test]
#[ignore = "requires an isolated X server; set DRAGLET_X11_TEST_DISPLAY"]
fn server_stacking_foreign_occlusion_and_destination_scale() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.motion(350, 70);
    assert_eq!(
        f.host.target_at_pointer(),
        Target::Window {
            window: WindowId::from(2),
            position: [10., 20.]
        }
    );
    let (foreign_client, _) =
        x11rb::connect(Some(&std::env::var("DRAGLET_X11_TEST_DISPLAY").unwrap())).unwrap();
    let foreign = window(&foreign_client, f.root, 340, 50, 100, 100);
    assert_eq!(f.host.target_at_pointer(), Target::Foreign);
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
        matches!(f.host.target_at_pointer(),Target::Window{window,..} if window==WindowId::from(2))
    );
    f.owner.destroy_window(foreign).unwrap().check().unwrap();
    f.host.set_scale(WindowId::from(2), 1.25).unwrap();
    assert_eq!(
        f.host.target_at_pointer(),
        Target::Window {
            window: WindowId::from(2),
            position: [16., 32.]
        }
    );
}
#[test]
#[ignore = "requires an isolated X server; set DRAGLET_X11_TEST_DISPLAY"]
fn server_input_shapes_and_unregistered_decorations_block_correctly() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    let (foreign_client, _) =
        x11rb::connect(Some(&std::env::var("DRAGLET_X11_TEST_DISPLAY").unwrap())).unwrap();
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
    assert_eq!(f.host.target_at_pointer(), Target::Foreign);
    f.motion(370, 70);
    assert!(
        matches!(f.host.target_at_pointer(),Target::Window{window,..} if window==WindowId::from(2))
    );
    f.owner.destroy_window(foreign).unwrap().check().unwrap();
    let frame = window(&f.owner, f.root, 600, 40, 200, 200);
    let content = window(&f.owner, frame, 0, 30, 200, 170);
    f.host.register_xid(WindowId::from(3), content, 1.).unwrap();
    f.motion(620, 50);
    assert_eq!(f.host.target_at_pointer(), Target::Foreign);
    f.motion(620, 100);
    assert_eq!(
        f.host.target_at_pointer(),
        Target::Window {
            window: WindowId::from(3),
            position: [20., 30.]
        }
    );
    f.owner.destroy_window(frame).unwrap().check().unwrap();
}

#[test]
#[ignore = "requires an isolated X server; set DRAGLET_X11_TEST_DISPLAY"]
fn source_unmap_cancels_standalone_capture() {
    let _serial = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    f.press();
    f.host.acquire_capture(WindowId::from(1)).unwrap();
    f.owner.unmap_window(f.source).unwrap().check().unwrap();
    sync(&f.owner);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(
            f.host.poll_input().unwrap(),
            Some(X11Input::Cancelled { .. })
        ) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native cancellation was not delivered"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    f.host.release_capture(WindowId::from(1));
}
