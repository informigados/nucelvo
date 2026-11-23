use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Graphics::Gdi::InvalidateRect;

pub fn repaint_restored_window(window: &slint::Window) {
    let native = window.window_handle();
    if let Ok(handle) = native.window_handle()
        && let RawWindowHandle::Win32(handle) = handle.as_raw()
    {
        // SAFETY: Slint owns the live HWND. A null rectangle invalidates the
        // whole client area so its software renderer can repaint the window.
        unsafe { InvalidateRect(handle.hwnd.get() as _, std::ptr::null(), 0) };
    }
    window.request_redraw();
}
