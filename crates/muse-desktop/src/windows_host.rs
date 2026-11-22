//! Windows host checks exercise actual window pixels, not DOM readiness alone.

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{
        ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
        GetPixel, GetWindowDC, HGDIOBJ, ReleaseDC, SelectObject,
    },
    Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
    UI::WindowsAndMessaging::{GetWindowRect, IsIconic},
};

pub fn hwnd(window: &Window) -> Result<HWND, String> {
    match HasWindowHandle::window_handle(window)
        .map_err(|_| "无法获取应用窗口")?
        .as_raw()
    {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut _)),
        _ => Err("需要 Windows 原生窗口".into()),
    }
}

pub fn is_minimized(window: &Window) -> bool {
    hwnd(window).is_ok_and(|hwnd| unsafe { IsIconic(hwnd).as_bool() })
}

pub fn toggle_maximize(window: &Window) {
    use windows::Win32::UI::WindowsAndMessaging::{
        IsZoomed, SW_MAXIMIZE, SW_RESTORE, ShowWindowAsync,
    };
    if let Ok(handle) = hwnd(window) {
        unsafe {
            let command = if IsZoomed(handle).as_bool() {
                SW_RESTORE
            } else {
                SW_MAXIMIZE
            };
            let _ = ShowWindowAsync(handle, command);
        }
    }
}

pub fn capture_pointer(window: &Window, capture: bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
    unsafe {
        if capture {
            if let Ok(handle) = hwnd(window) {
                let _ = SetCapture(handle);
            }
        } else {
            let _ = ReleaseCapture();
        }
    }
}

/// Capture this window only. Occluding windows and their contents are excluded.
pub fn pixel_matches(window: &Window, x: f32, y: f32, rgb: u32) -> Result<bool, String> {
    let hwnd = hwnd(window)?;
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(|_| "无法读取窗口大小")?;
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width <= 0 || height <= 0 || width > 16000 || height > 16000 {
            return Err("窗口大小无法用于画面检查".into());
        }
        let mut client = POINT::default();
        if !ClientToScreen(hwnd, &mut client).as_bool() {
            return Err("无法定位网页画面".into());
        }
        let scale = window.scale_factor();
        let x = client.x - rect.left + (x * scale).round() as i32;
        let y = client.y - rect.top + (y * scale).round() as i32;
        let source = GetWindowDC(Some(hwnd));
        if source.is_invalid() {
            return Err("无法访问应用画面".into());
        }
        let target = CreateCompatibleDC(Some(source));
        let bitmap = CreateCompatibleBitmap(source, width, height);
        if target.is_invalid() || bitmap.is_invalid() {
            if !target.is_invalid() {
                let _ = DeleteDC(target);
            }
            if !bitmap.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
            ReleaseDC(Some(hwnd), source);
            return Err("无法分配画面检查缓冲区".into());
        }
        let previous = SelectObject(target, HGDIOBJ(bitmap.0));
        let captured = PrintWindow(hwnd, target, PRINT_WINDOW_FLAGS(2)).as_bool();
        let color = GetPixel(target, x, y).0;
        let _ = SelectObject(target, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(target);
        ReleaseDC(Some(hwnd), source);
        if !captured || color == u32::MAX {
            return Err("应用画面检查失败".into());
        }
        let expected = ((rgb >> 16) & 255) | (rgb & 0xFF00) | ((rgb & 255) << 16);
        Ok(color == expected)
    }
}
