use super::*;

fn fixture() -> (Adapter, Rc<Hook>) {
    let adapter = Adapter::new().unwrap();
    let hook = Rc::new(Hook {
        shared: Rc::downgrade(&adapter.shared),
        window: SyncWeak::new(),
        id: WindowId::from(1_u64),
        hwnd: 1_usize as HWND,
        registration: 1,
        installed: Cell::new(false),
        down: Cell::new(true),
        press: Cell::new(1),
        consumed_press: Cell::new(1),
    });
    adapter.shared.gesture.set(Gesture {
        state: CaptureState::Held(hook.id),
        registration: 1,
        press: 1,
        ..Gesture::default()
    });
    (adapter, hook)
}

#[test]
fn signed_message_coordinates_include_outside_source() {
    let packed = ((-300_i16 as u16 as usize) << 16) | (-125_i16 as u16 as usize);
    assert_eq!(message_position(packed as LPARAM), [-125.0, -300.0]);
}

#[test]
fn capture_theft_is_synchronous_and_sticky_across_same_window_recapture() {
    let (adapter, hook) = fixture();
    adapter.shared.observe(&hook, WM_CAPTURECHANGED, 0, 2);
    assert_eq!(
        adapter.shared.gesture.get().state,
        CaptureState::Lost(hook.id)
    );
    adapter
        .shared
        .observe(&hook, WM_CAPTURECHANGED, 0, hook.hwnd as LPARAM);
    assert_eq!(
        adapter.shared.gesture.get().state,
        CaptureState::Lost(hook.id)
    );
    assert_eq!(
        *adapter.shared.signals.borrow(),
        [Signal::Lost { source: hook.id }]
    );
    assert!(!hook.down.get());
}

#[test]
fn expected_capture_release_preserves_native_up_snapshot() {
    let (adapter, hook) = fixture();
    let mut gesture = adapter.shared.gesture.get();
    gesture.state = CaptureState::Released(hook.id);
    gesture.target = Target::Foreign;
    adapter.shared.gesture.set(gesture);
    adapter.shared.observe(&hook, WM_CAPTURECHANGED, 0, 0);
    assert_eq!(
        adapter.shared.gesture.get().state,
        CaptureState::Released(hook.id)
    );
    assert_eq!(adapter.shared.gesture.get().target, Target::Foreign);
    assert!(adapter.shared.signals.borrow().is_empty());
    // Destruction invalidates even a released source; it cannot become a drop.
    adapter.shared.observe(&hook, WM_NCDESTROY, 0, 0);
    assert_eq!(
        adapter.shared.gesture.get().state,
        CaptureState::Lost(hook.id)
    );
}

#[test]
fn new_primary_press_and_cancel_invalidate_previous_generation_once() {
    let (adapter, hook) = fixture();
    adapter.shared.observe(&hook, WM_LBUTTONDOWN, 0, 0);
    assert_eq!(hook.press.get(), 2);
    assert_eq!(
        adapter.shared.gesture.get().state,
        CaptureState::Lost(hook.id)
    );
    adapter.shared.observe(&hook, WM_CANCELMODE, 0, 0);
    assert_eq!(adapter.shared.signals.borrow().len(), 1);
    assert!(!hook.down.get());
}

#[test]
fn foreign_registration_destruction_does_not_cancel_source() {
    let (adapter, hook) = fixture();
    adapter.shared.lose(hook.registration + 1, true);
    assert_eq!(
        adapter.shared.gesture.get().state,
        CaptureState::Held(hook.id)
    );
    assert!(adapter.shared.signals.borrow().is_empty());
}

#[test]
fn untracked_release_cannot_reuse_an_old_observed_press() {
    let (adapter, hook) = fixture();
    adapter.shared.gesture.set(Gesture::default());
    adapter.shared.observe(&hook, WM_LBUTTONUP, 0, 0);
    assert!(!hook.down.get());
    assert!(adapter.shared.signals.borrow().is_empty());
}

#[test]
fn cancelled_lease_cannot_readopt_the_same_held_press() {
    let (mut adapter, hook) = fixture();
    adapter.end_capture(hook.id);
    assert_eq!(adapter.shared.gesture.get().state, CaptureState::Idle);
    assert!(hook.down.get());
    assert!(hook.consume_press().is_err());
}

#[test]
fn new_down_before_old_guard_cleanup_remains_eligible() {
    let (mut adapter, hook) = fixture();
    adapter.shared.observe(&hook, WM_LBUTTONDOWN, 0, 0);
    assert_eq!(
        adapter.shared.gesture.get().state,
        CaptureState::Lost(hook.id)
    );
    adapter.end_capture(hook.id);
    assert_eq!(hook.consumed_press.get(), 1);
    assert_eq!(hook.consume_press().unwrap(), 2);
    assert!(hook.consume_press().is_err());
}

/// Executes real winit/Win32 subclass chains and capture transitions. This uses
/// desktop-wide mouse capture and must be run explicitly on an interactive host.
#[test]
#[ignore = "requires an interactive Windows desktop; takes real native capture"]
#[allow(deprecated)]
fn native_winit_release_theft_and_destruction() {
    use windows_sys::Win32::UI::{
        Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
        WindowsAndMessaging::{DestroyWindow, SendMessageW, WM_RBUTTONDOWN, WM_RBUTTONUP},
    };
    use winit::{event_loop::EventLoop, platform::windows::EventLoopBuilderExtWindows};

    let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
    let first = Arc::new(
        event_loop
            .create_window(Window::default_attributes().with_title("native drag test source"))
            .unwrap(),
    );
    let second = Arc::new(
        event_loop
            .create_window(Window::default_attributes().with_title("native drag test destination"))
            .unwrap(),
    );
    let mut adapter = Adapter::new().unwrap();
    adapter.register(&first).unwrap();
    adapter.register(&second).unwrap();
    let a = adapter.shared.hook(first.id()).unwrap();
    let b = adapter.shared.hook(second.id()).unwrap();

    // SAFETY: Live owner-thread HWNDs from the registered winit windows. Sending
    // native mouse messages synchronously exercises winit's capture behavior.
    unsafe {
        SendMessageW(a.hwnd, WM_LBUTTONDOWN, 1, 0);
    }
    adapter.begin_capture(first.id()).unwrap();
    let release_position = ((-300_i16 as u16 as usize) << 16) | (-125_i16 as u16 as usize);
    // SAFETY: Same valid owner-thread source; winit releases before public Up.
    unsafe {
        SendMessageW(a.hwnd, WM_LBUTTONUP, 0, release_position as LPARAM);
    }
    assert_eq!(adapter.capture_state(), CaptureState::Released(first.id()));
    assert!(
        adapter
            .take_signals()
            .iter()
            .any(|s| matches!(s, Signal::Released { source, position }
                if *source == first.id() && *position == [-125.0, -300.0]))
    );

    // Capture belonging to a later window must survive cleanup of the release.
    // SAFETY: Live owner-thread destination; this intentionally takes test capture.
    unsafe {
        SetCapture(b.hwnd);
    }
    adapter.end_capture(first.id());
    // SAFETY: Query has no preconditions.
    assert_eq!(unsafe { GetCapture() }, b.hwnd);

    // Winit shares capture across buttons. Cancelling our primary drag must
    // preserve both its HWND capture and its internal pressed-button count.
    // SAFETY: Synchronous button messages target the live owner-thread source.
    unsafe {
        SendMessageW(a.hwnd, WM_LBUTTONDOWN, 1, 0);
    }
    adapter.begin_capture(first.id()).unwrap();
    // SAFETY: Right joins the same native capture while left remains pressed.
    unsafe {
        SendMessageW(a.hwnd, WM_RBUTTONDOWN, 3, 0);
    }
    adapter.end_capture(first.id());
    assert_eq!(adapter.capture_state(), CaptureState::Idle);
    assert!(adapter.begin_capture(first.id()).is_err());
    // SAFETY: Query has no preconditions; cancellation must preserve capture.
    assert_eq!(unsafe { GetCapture() }, a.hwnd);
    // SAFETY: Releasing left must leave winit's right-button capture intact.
    unsafe {
        SendMessageW(a.hwnd, WM_LBUTTONUP, 2, 0);
    }
    // SAFETY: Query has no preconditions; winit still tracks one pressed button.
    assert_eq!(unsafe { GetCapture() }, a.hwnd);
    // SAFETY: The last button release belongs to winit, which now releases capture.
    unsafe {
        SendMessageW(a.hwnd, WM_RBUTTONUP, 0, 0);
    }
    // SAFETY: Query has no preconditions.
    assert!(unsafe { GetCapture() }.is_null());

    // SAFETY: Begin another native primary gesture on the live test source.
    unsafe {
        SendMessageW(a.hwnd, WM_LBUTTONDOWN, 1, 0);
    }
    adapter.begin_capture(first.id()).unwrap();
    // SAFETY: A real new primary sequence can be observed before the old drag's
    // guard is cleaned up. Cleanup must leave the newer press eligible to adopt.
    unsafe {
        SendMessageW(a.hwnd, WM_LBUTTONUP, 0, 0);
        SendMessageW(a.hwnd, WM_LBUTTONDOWN, 1, 0);
    }
    adapter.end_capture(first.id());
    adapter.begin_capture(first.id()).unwrap();
    // SAFETY: Force synchronous theft then same-HWND recapture without a public
    // event/poll between them. The loss must remain latched across both calls.
    unsafe {
        SetCapture(b.hwnd);
        SetCapture(a.hwnd);
    }
    assert_eq!(adapter.capture_state(), CaptureState::Lost(first.id()));
    adapter.end_capture(first.id());
    // SAFETY: Query has no preconditions; adapter must not release newer capture.
    assert_eq!(unsafe { GetCapture() }, a.hwnd);

    // SAFETY: Live source gets a fresh primary press before destruction.
    unsafe {
        SendMessageW(a.hwnd, WM_LBUTTONDOWN, 1, 0);
    }
    adapter.begin_capture(first.id()).unwrap();
    // SAFETY: Destroy on its native owner thread; this also checks our subclass's
    // WM_NCDESTROY cleanup while callbacks are synchronously on the stack.
    unsafe {
        DestroyWindow(a.hwnd);
    }
    assert_eq!(adapter.capture_state(), CaptureState::Lost(first.id()));
    assert!(!a.installed.get());
    assert!(
        adapter
            .take_signals()
            .iter()
            .any(|s| matches!(s, Signal::Lost { source } if *source == first.id()))
    );
    adapter.unregister(first.id()).unwrap();
    // SAFETY: Finish with no test capture on this thread.
    unsafe {
        ReleaseCapture();
    }
}
