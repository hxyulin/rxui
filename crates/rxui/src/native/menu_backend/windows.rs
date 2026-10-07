//! The only unsafe boundary in RXUI: borrowed live HWNDs and winit-owned MSGs.
use super::*;
use astrelis_winit::winit::{
    platform::windows::EventLoopBuilderExtWindows,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, TranslateAcceleratorW};
fn hwnd(window: &Window) -> Result<isize, ApplicationError> {
    match window
        .window_handle()
        .map_err(|e| ApplicationError::Native(Box::new(e)))?
        .as_raw()
    {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
        _ => Err(ApplicationError::InvalidWindowOptions),
    }
}
pub(super) fn attach(menu: &Menu, window: &Arc<Window>) -> Result<(), ApplicationError> {
    let handle = hwnd(window)?;
    // SAFETY: called on the winit UI thread; the Arc keeps this HWND live until
    // detach, and the menu is retained for the complete attachment lifetime.
    unsafe { menu.init_for_hwnd(handle) }.map_err(native_error)
}
pub(super) fn detach(menu: &Menu, window: &Arc<Window>) {
    if let Ok(handle) = hwnd(window) {
        // SAFETY: the same owned window/menu pair used for attach is still alive;
        // detachment runs on the UI thread before either resource is released.
        let _ = unsafe { menu.remove_for_hwnd(handle) };
    }
}
pub(super) fn hook(
    builder: &mut astrelis_winit::winit::event_loop::EventLoopBuilder<Wake>,
    menus: WindowMenus,
) {
    builder.with_msg_hook(move |message| {
        // SAFETY: winit guarantees a valid borrowed MSG during this callback. Only
        // translate using a menu attached to that HWND; the borrow retains both
        // the window and accelerator table. Returning true consumes the message,
        // so the same shortcut cannot also reach RXUI's KeyboardInput path.
        unsafe {
            let message = message.cast::<MSG>();
            if message.is_null() {
                return false;
            }
            let menus = menus.borrow();
            menus.values().any(|(window, menu)| {
                hwnd(window).ok().is_some_and(|handle| {
                    handle == (*message).hwnd as isize
                        && TranslateAcceleratorW((*message).hwnd, menu.menu.haccel() as _, message)
                            != 0
                })
            })
        }
    });
}
