//! Owner-thread Win32 subclass integration. No application callbacks are stored.
use crate::{CaptureState, Error, Signal, Target};
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::{Rc, Weak},
    sync::{Arc, Weak as SyncWeak},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
    UI::{
        HiDpi::{
            DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            DPI_AWARENESS_PER_MONITOR_AWARE, GetAwarenessFromDpiAwarenessContext, GetDpiForWindow,
            GetWindowDpiAwarenessContext, SetThreadDpiAwarenessContext,
        },
        Input::KeyboardAndMouse::GetCapture,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetClientRect, GetPhysicalCursorPos, GetWindowThreadProcessId, WM_ACTIVATEAPP,
            WM_CANCELMODE, WM_CAPTURECHANGED, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
            WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WindowFromPhysicalPoint,
        },
    },
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Window, WindowId},
};

#[derive(Clone, Copy)]
struct Gesture {
    state: CaptureState,
    registration: u64,
    press: u64,
    target: Target,
    target_registration: u64,
}

impl Default for Gesture {
    fn default() -> Self {
        Self {
            state: CaptureState::Idle,
            registration: 0,
            press: 0,
            target: Target::Unavailable,
            target_registration: 0,
        }
    }
}

struct Shared {
    thread: u32,
    process: u32,
    hooks: RefCell<Vec<Rc<Hook>>>,
    gesture: Cell<Gesture>,
    signals: RefCell<Vec<Signal>>,
    next_registration: Cell<u64>,
}

struct Hook {
    shared: Weak<Shared>,
    window: SyncWeak<Window>,
    id: WindowId,
    hwnd: HWND,
    registration: u64,
    installed: Cell<bool>,
    down: Cell<bool>,
    press: Cell<u64>,
    consumed_press: Cell<u64>,
}

pub(crate) struct Adapter {
    shared: Rc<Shared>,
}

impl Adapter {
    pub(crate) fn new() -> Result<Self, Error> {
        // SAFETY: These getters have no preconditions and do not dispatch messages.
        let (thread, process) = unsafe { (GetCurrentThreadId(), GetCurrentProcessId()) };
        Ok(Self {
            shared: Rc::new(Shared {
                thread,
                process,
                hooks: RefCell::new(Vec::new()),
                gesture: Cell::new(Gesture::default()),
                signals: RefCell::new(Vec::new()),
                next_registration: Cell::new(1),
            }),
        })
    }

    pub(crate) fn register(&mut self, window: &Arc<Window>) -> Result<(), Error> {
        self.shared.check_thread()?;
        self.prune();
        let hwnd = match window
            .window_handle()
            .map_err(|e| Error(e.to_string()))?
            .as_raw()
        {
            RawWindowHandle::Win32(handle) => handle.hwnd.get() as HWND,
            _ => return Err(Error("expected a Win32 window handle".into())),
        };
        if !self.shared.owns(hwnd) {
            return Err(Error(
                "window must belong to this process and thread".into(),
            ));
        }
        // SAFETY: hwnd was obtained from a live winit Window and owner-thread validated.
        let awareness =
            unsafe { GetAwarenessFromDpiAwarenessContext(GetWindowDpiAwarenessContext(hwnd)) };
        if awareness != DPI_AWARENESS_PER_MONITOR_AWARE {
            return Err(Error(
                "native drag requires a per-monitor DPI-aware window".into(),
            ));
        }
        let existing = self.shared.hook(window.id());
        if let Some(hook) = existing {
            return if hook.hwnd == hwnd && hook.window.ptr_eq(&Arc::downgrade(window)) {
                Ok(())
            } else {
                Err(Error("window identity is already registered".into()))
            };
        }
        let registration = self.shared.next_registration.get();
        let next = registration
            .checked_add(1)
            .ok_or_else(|| Error("registration counter exhausted".into()))?;
        self.shared.next_registration.set(next);
        let hook = Rc::new(Hook {
            shared: Rc::downgrade(&self.shared),
            window: Arc::downgrade(window),
            id: window.id(),
            hwnd,
            registration,
            installed: Cell::new(true),
            down: Cell::new(false),
            press: Cell::new(0),
            consumed_press: Cell::new(0),
        });
        let raw = Rc::into_raw(hook.clone());
        // SAFETY: Owner-thread live HWND; the unique pointer is both subclass ID and
        // reference data. This extra Rc remains owned by the installed subclass until
        // successful removal or WM_NCDESTROY. The registry owns a separate Rc.
        if unsafe { SetWindowSubclass(hwnd, Some(subclass), raw as usize, raw as usize) } == 0 {
            hook.installed.set(false);
            // SAFETY: Installation failed, so no subclass owns the transferred Rc.
            unsafe {
                drop(Rc::from_raw(raw));
            }
            return Err(Error("SetWindowSubclass failed".into()));
        }
        self.shared.hooks.borrow_mut().push(hook);
        Ok(())
    }

    pub(crate) fn unregister(&mut self, id: WindowId) -> Result<(), Error> {
        self.shared.check_thread()?;
        if let Some(hook) = self.shared.hook(id) {
            self.shared.lose(hook.registration, true);
            if !hook.detach() {
                return Err(Error(
                    "RemoveWindowSubclass failed; registration retained for retry".into(),
                ));
            }
            self.shared
                .hooks
                .borrow_mut()
                .retain(|h| !Rc::ptr_eq(h, &hook));
        }
        Ok(())
    }

    pub(crate) fn begin_capture(&mut self, id: WindowId) -> Result<(), Error> {
        self.shared.check_thread()?;
        self.prune();
        let hook = self
            .shared
            .hook(id)
            .ok_or_else(|| Error("source is not registered".into()))?;
        // We observe winit's capture rather than replacing it: winit already called
        // SetCapture before delivering its public primary press.
        // SAFETY: GetCapture is a thread-local query without pointer arguments.
        if !hook.live(&self.shared) || !hook.down.get() || unsafe { GetCapture() } != hook.hwnd {
            return Err(Error(
                "no observed native primary press with source capture".into(),
            ));
        }
        let old = self.shared.gesture.get();
        if let CaptureState::Held(_) = old.state {
            return if old.registration == hook.registration && old.press == hook.press.get() {
                Ok(())
            } else {
                Err(Error("another native gesture is active".into()))
            };
        }
        let press = hook.consume_press()?;
        self.shared.signals.borrow_mut().clear();
        self.shared.gesture.set(Gesture {
            state: CaptureState::Held(id),
            registration: hook.registration,
            press,
            ..Gesture::default()
        });
        Ok(())
    }

    pub(crate) fn end_capture(&mut self, id: WindowId) {
        if self.shared.check_thread().is_err() {
            return;
        }
        let gesture = self.shared.gesture.get();
        if !matches!(gesture.state, CaptureState::Held(source) | CaptureState::Released(source)
            | CaptureState::Lost(source) if source == id)
        {
            return;
        }
        // Winit owns native capture and shares it among all pressed buttons.
        // End only our primary lease: releasing the HWND here could cancel a
        // right/middle/X-button gesture, including one started during this drag.
        // Winit releases capture when its own button count reaches zero.
        self.shared.gesture.set(Gesture::default());
    }

    pub(crate) fn capture_state(&mut self) -> CaptureState {
        self.refresh();
        self.shared.gesture.get().state
    }

    pub(crate) fn take_signals(&mut self) -> Vec<Signal> {
        self.refresh();
        std::mem::take(&mut *self.shared.signals.borrow_mut())
    }

    pub(crate) fn target_at_pointer(&mut self) -> Target {
        self.refresh();
        let gesture = self.shared.gesture.get();
        match gesture.state {
            CaptureState::Lost(_) => Target::Unavailable,
            CaptureState::Released(_) => {
                if let Target::Window { window, .. } = gesture.target
                    && !self.shared.hook(window).is_some_and(|h| {
                        h.registration == gesture.target_registration && h.live(&self.shared)
                    })
                {
                    return Target::Unavailable;
                }
                gesture.target
            }
            _ => self.shared.hit().0,
        }
    }

    fn refresh(&mut self) {
        if self.shared.check_thread().is_err() {
            return;
        }
        self.prune();
        let gesture = self.shared.gesture.get();
        if let CaptureState::Held(id) = gesture.state {
            let valid = self.shared.hook(id).is_some_and(|h| {
                // SAFETY: Thread-local query; the adapter is on its creation thread.
                h.live(&self.shared)
                    && h.down.get()
                    && h.press.get() == gesture.press
                    && h.registration == gesture.registration
                    && unsafe { GetCapture() } == h.hwnd
            });
            if !valid {
                self.shared.lose(gesture.registration, false);
            }
        }
    }

    fn prune(&self) {
        // Clone before liveness checks: dropping a temporary Arc<Window> can
        // synchronously destroy a native window and reenter this subclass.
        let hooks = self.shared.hooks.borrow().clone();
        for hook in hooks {
            if !hook.live(&self.shared) {
                self.shared.lose(hook.registration, true);
                if hook.detach() {
                    self.shared
                        .hooks
                        .borrow_mut()
                        .retain(|h| !Rc::ptr_eq(h, &hook));
                }
            }
        }
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        if let CaptureState::Held(id) = self.shared.gesture.get().state {
            self.end_capture(id);
        }
        let hooks = std::mem::take(&mut *self.shared.hooks.borrow_mut());
        for hook in hooks {
            // On an unexpected removal failure, keep the subclass's own Rc alive.
            // Its Weak<Shared>/Weak<Window> retain no UI resources. NCDESTROY will
            // retire it; freeing it here would leave a dangling OS callback pointer.
            hook.detach();
        }
    }
}

impl Shared {
    fn check_thread(&self) -> Result<(), Error> {
        // SAFETY: GetCurrentThreadId has no preconditions.
        if unsafe { GetCurrentThreadId() } == self.thread {
            Ok(())
        } else {
            Err(Error("native drag called off its owner thread".into()))
        }
    }

    fn owns(&self, hwnd: HWND) -> bool {
        let mut process = 0;
        // SAFETY: The API accepts any HWND (returns zero for invalid ones), and
        // process is a valid writable output pointer for the duration of the call.
        let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut process) };
        thread == self.thread && process == self.process
    }

    fn hook(&self, id: WindowId) -> Option<Rc<Hook>> {
        self.hooks.borrow().iter().find(|h| h.id == id).cloned()
    }

    fn lose(&self, registration: u64, include_released: bool) {
        let mut gesture = self.gesture.get();
        if gesture.registration != registration {
            return;
        }
        let source = match gesture.state {
            CaptureState::Held(id) => id,
            CaptureState::Released(id) if include_released => id,
            _ => return,
        };
        gesture.state = CaptureState::Lost(source);
        gesture.target = Target::Unavailable;
        self.gesture.set(gesture);
        self.signals.borrow_mut().push(Signal::Lost { source });
    }

    fn hit(&self) -> (Target, u64) {
        let Some(_dpi) = PhysicalDpi::enter() else {
            return (Target::Unavailable, 0);
        };
        let mut point = POINT::default();
        // SAFETY: Writable point output and a valid physical point passed by value.
        let hwnd = unsafe {
            if GetPhysicalCursorPos(&mut point) == 0 {
                return (Target::Unavailable, 0);
            }
            WindowFromPhysicalPoint(point)
        };
        if hwnd.is_null() {
            return (Target::NoWindow, 0);
        }
        // The native hit is authoritative: never search registered rectangles
        // behind a foreign HWND or climb from an unregistered child HWND.
        let hook = self.hooks.borrow().iter().find(|h| h.hwnd == hwnd).cloned();
        let Some(hook) = hook else {
            return (Target::Foreign, 0);
        };
        if !hook.live(self) {
            return (Target::Unavailable, 0);
        }
        let mut client = point;
        let mut bounds = RECT::default();
        // SAFETY: A validated registered HWND; output pointers are live locals.
        // The temporary PMv2 context makes both coordinates physical pixels.
        let dpi = unsafe {
            if ScreenToClient(hwnd, &mut client) == 0 || GetClientRect(hwnd, &mut bounds) == 0 {
                return (Target::Unavailable, 0);
            }
            GetDpiForWindow(hwnd)
        };
        if dpi == 0 {
            return (Target::Unavailable, 0);
        }
        if client.x < bounds.left
            || client.y < bounds.top
            || client.x >= bounds.right
            || client.y >= bounds.bottom
        {
            return (Target::Foreign, 0);
        }
        (
            Target::Window {
                window: hook.id,
                position: [
                    f64::from(client.x) * 96.0 / f64::from(dpi),
                    f64::from(client.y) * 96.0 / f64::from(dpi),
                ],
            },
            hook.registration,
        )
    }

    fn observe(&self, hook: &Hook, message: u32, wparam: WPARAM, lparam: LPARAM) {
        let was_down = hook.down.get();
        // Even an untracked Up retires the native press. Otherwise a later
        // unrelated SetCapture could make that old press appear eligible again.
        if message == WM_LBUTTONUP {
            hook.down.set(false);
        }
        match message {
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                // A second press makes the previous lease stale, even on this HWND.
                let old = self.gesture.get();
                self.lose(old.registration, true);
                if let Some(press) = hook.press.get().checked_add(1) {
                    hook.press.set(press);
                    hook.down.set(true);
                } else {
                    hook.down.set(false);
                }
            }
            WM_CAPTURECHANGED if lparam as HWND != hook.hwnd => {
                hook.down.set(false);
                self.lose(hook.registration, false);
            }
            WM_CANCELMODE | WM_KILLFOCUS => {
                hook.down.set(false);
                self.lose(hook.registration, false);
            }
            WM_ACTIVATEAPP if wparam == 0 => {
                hook.down.set(false);
                self.lose(hook.registration, false);
            }
            WM_NCDESTROY => {
                hook.down.set(false);
                self.lose(hook.registration, true);
            }
            WM_MOUSEMOVE | WM_LBUTTONUP => {
                let gesture = self.gesture.get();
                if gesture.state != CaptureState::Held(hook.id)
                    || gesture.registration != hook.registration
                    || gesture.press != hook.press.get()
                    || !was_down
                {
                    return;
                }
                // SAFETY: GetCapture has no preconditions; callbacks run on owner thread.
                if unsafe { GetCapture() } != hook.hwnd {
                    self.lose(hook.registration, false);
                    return;
                }
                let position = message_position(lparam);
                if message == WM_MOUSEMOVE {
                    self.signals.borrow_mut().push(Signal::Motion {
                        source: hook.id,
                        position,
                    });
                } else {
                    // This must precede DefSubclassProc: winit releases capture
                    // before publishing MouseInput::Released. No borrows span hit().
                    let (target, target_registration) = self.hit();
                    let current = self.gesture.get();
                    // Native queries may reenter: never resurrect a cancelled lease.
                    if current.state == gesture.state
                        && current.registration == gesture.registration
                        && current.press == gesture.press
                    {
                        self.gesture.set(Gesture {
                            state: CaptureState::Released(hook.id),
                            target,
                            target_registration,
                            ..gesture
                        });
                        self.signals.borrow_mut().push(Signal::Released {
                            source: hook.id,
                            position,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

impl Hook {
    fn consume_press(&self) -> Result<u64, Error> {
        let press = self.press.get();
        if !self.down.get() || press == self.consumed_press.get() {
            return Err(Error(
                "native primary press has already been consumed or released".into(),
            ));
        }
        // Adoption consumes only this generation. Cancellation must not consume
        // the latest observed generation: a new Down may precede old guard cleanup.
        self.consumed_press.set(press);
        Ok(press)
    }

    fn live(&self, shared: &Shared) -> bool {
        if !self.installed.get() || !shared.owns(self.hwnd) {
            return false;
        }
        let Some(window) = self.window.upgrade() else {
            return false;
        };
        let matches = window.id() == self.id && window.window_handle().is_ok_and(|handle|
            matches!(handle.as_raw(), RawWindowHandle::Win32(raw) if raw.hwnd.get() as HWND == self.hwnd));
        // Dropping the temporary strong reference can itself destroy the window.
        // Observe NCDESTROY before reporting liveness to the caller.
        drop(window);
        matches && self.installed.get() && shared.owns(self.hwnd)
    }

    fn detach(&self) -> bool {
        if !self.installed.get() {
            return true;
        }
        let raw = self as *const Self;
        // SAFETY: Only called on owner thread; this pointer uniquely identifies the
        // installed subclass and its retained Rc is still alive during removal.
        if unsafe { RemoveWindowSubclass(self.hwnd, Some(subclass), raw as usize) } == 0 {
            return false;
        }
        if self.installed.replace(false) {
            // SAFETY: Successful removal transfers back exactly the Rc retained by
            // SetWindowSubclass. Callers and active callbacks hold independent Rcs.
            unsafe {
                drop(Rc::from_raw(raw));
            }
        }
        true
    }
}

/// A short scoped override; never spans forwarding to winit or application code.
struct PhysicalDpi(DPI_AWARENESS_CONTEXT);
impl PhysicalDpi {
    fn enter() -> Option<Self> {
        // SAFETY: Valid predefined context; saves the previous thread context.
        let old =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        (!old.is_null()).then(|| Self(old))
    }
}
impl Drop for PhysicalDpi {
    fn drop(&mut self) {
        // SAFETY: Restore the context returned by the successful matching enter().
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}

fn message_position(lparam: LPARAM) -> [f64; 2] {
    // GET_X/Y_LPARAM signed extraction, including negative outside-client positions.
    [
        f64::from(lparam as u16 as i16),
        f64::from((lparam >> 16) as u16 as i16),
    ]
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    reference: usize,
) -> LRESULT {
    let raw = reference as *const Hook;
    // SAFETY: The installed subclass owns a retained Rc at this exact pointer.
    // Clone it immediately so nested destruction/removal cannot free our callback.
    let hook = unsafe {
        Rc::increment_strong_count(raw);
        Rc::from_raw(raw)
    };
    let shared = hook.shared.upgrade();
    if let Some(shared) = &shared {
        // No RefCell borrow crosses an OS call. Also contain unexpected Rust
        // panics: unwinding across the system ABI is never permitted.
        if catch_unwind(AssertUnwindSafe(|| {
            shared.observe(&hook, message, wparam, lparam)
        }))
        .is_err()
        {
            let mut gesture = shared.gesture.get();
            if gesture.registration == hook.registration {
                gesture.state = CaptureState::Lost(hook.id);
                gesture.target = Target::Unavailable;
                shared.gesture.set(gesture);
                if let Ok(mut signals) = shared.signals.try_borrow_mut() {
                    signals.push(Signal::Lost { source: hook.id });
                }
            }
        }
    }
    if message == WM_NCDESTROY && hook.installed.replace(false) {
        // SAFETY: This is the final native destruction notification. Remove this
        // callback before forwarding, and retire the OS-owned Rc exactly once.
        // Even if removal fails, the destroyed HWND cannot issue future messages.
        unsafe {
            RemoveWindowSubclass(hwnd, Some(subclass), raw as usize);
            drop(Rc::from_raw(raw));
        }
    }
    // SAFETY: Forward the original parameters exactly once through comctl32's
    // active subclass chain. Our callback-local Rc survives synchronous reentry.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests;
