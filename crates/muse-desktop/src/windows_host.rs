//! Windows host checks exercise actual window pixels, not DOM readiness alone.

use gpui::{Bounds, Pixels, Window};
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

/// Let a GPUI hover card cover a child WebView without hiding or resizing the
/// browser. The page remains live outside the small, rounded card footprint.
#[derive(Default)]
pub struct HoverOcclusion {
    target: Option<HWND>,
    applied: Option<[i32; 7]>,
}

impl HoverOcclusion {
    pub fn set_target(&mut self, target: Option<HWND>) {
        if self.target == target {
            return;
        }
        self.clear();
        self.target = target;
    }

    pub fn clear(&mut self) {
        self.restore_region();
        self.target = None;
    }

    fn restore_region(&mut self) {
        if self.applied.take().is_some()
            && let Some(target) = self.target
        {
            // Clearing the region restores the whole child. Windows owns any
            // region successfully installed with SetWindowRgn.
            let _ = unsafe { windows::Win32::Graphics::Gdi::SetWindowRgn(target, None, true) };
        }
    }

    pub fn place(&mut self, window: &Window, bounds: Bounds<Pixels>) -> Result<(), String> {
        use windows::Win32::Graphics::Gdi::{
            CombineRgn, CreateRectRgn, CreateRoundRectRgn, RGN_DIFF, SetWindowRgn,
        };
        let Some(target) = self.target else {
            return Ok(());
        };
        let owner = hwnd(window)?;
        let mut browser = RECT::default();
        let mut origin = POINT::default();
        unsafe {
            GetWindowRect(target, &mut browser).map_err(|_| "无法读取浏览器区域")?;
            if !ClientToScreen(owner, &mut origin).as_bool() {
                return Err("无法定位详情卡片".into());
            }
        }
        let scale = window.scale_factor();
        let left = origin.x + (f32::from(bounds.origin.x) * scale).floor() as i32 - browser.left;
        let top = origin.y + (f32::from(bounds.origin.y) * scale).floor() as i32 - browser.top;
        let right = origin.x
            + (f32::from(bounds.origin.x + bounds.size.width) * scale).ceil() as i32
            - browser.left;
        let bottom = origin.y
            + (f32::from(bounds.origin.y + bounds.size.height) * scale).ceil() as i32
            - browser.top;
        let width = browser.right - browser.left;
        let height = browser.bottom - browser.top;
        if width <= 0 || height <= 0 || right <= 0 || bottom <= 0 || left >= width || top >= height
        {
            self.restore_region();
            return Ok(());
        }
        // `rounded_lg` is an 8px radius, in physical window-region coordinates.
        let diameter = (16. * scale).round().max(1.) as i32;
        let key = [width, height, left, top, right, bottom, diameter];
        if self.applied == Some(key) {
            return Ok(());
        }
        unsafe {
            let visible = CreateRectRgn(0, 0, width, height);
            if visible.0.is_null() {
                return Err("无法创建浏览器显示区域".into());
            }
            let covered = CreateRoundRectRgn(left, top, right, bottom, diameter, diameter);
            if covered.0.is_null() {
                let _ = DeleteObject(HGDIOBJ(visible.0));
                return Err("无法创建详情卡片区域".into());
            }
            let combined = CombineRgn(Some(visible), Some(visible), Some(covered), RGN_DIFF);
            let _ = DeleteObject(HGDIOBJ(covered.0));
            if combined.0 == 0 || SetWindowRgn(target, Some(visible), true) == 0 {
                let _ = DeleteObject(HGDIOBJ(visible.0));
                return Err("无法更新浏览器显示区域".into());
            }
            // Ownership of `visible` has transferred to the window.
        }
        self.applied = Some(key);
        Ok(())
    }
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
