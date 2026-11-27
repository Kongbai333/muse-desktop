//! Native caption behavior must not pass through GPUI's focus-prevent-default path.

use gpui::Window;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::ReleaseCapture,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const CAPTION_SUBCLASS: usize = 0x4D55_5345;

// Tabs share the title bar. These logical x positions mark the tab strip so the
// native drag region skips it: [48, TABS_START) and [DRAG_START, width - 138).
// f32::MAX until the first frame keeps the previous whole-bar behaviour.
static TABS_START: AtomicU32 = AtomicU32::new(0x7F7F_FFFF);
static DRAG_START: AtomicU32 = AtomicU32::new(0x7F7F_FFFF);
static DRAG_END: AtomicU32 = AtomicU32::new(0x7F7F_FFFF);

pub fn set_drag_start(drag_start: f32) {
    DRAG_START.store(drag_start.to_bits(), Ordering::Relaxed);
}

pub fn set_drag_end(drag_end: f32) {
    DRAG_END.store(drag_end.to_bits(), Ordering::Relaxed);
}

/// A full-window overlay (image viewer) owns the title bar: its buttons must
/// receive clicks instead of starting a window drag.
static OVERLAY: AtomicBool = AtomicBool::new(false);

pub fn set_overlay(open: bool) {
    OVERLAY.store(open, Ordering::Relaxed);
}

pub fn set_tabs_start(tabs_start: f32) {
    TABS_START.store(tabs_start.to_bits(), Ordering::Relaxed);
}

fn caption_drag_region(
    x: f32,
    y: f32,
    width: f32,
    tabs_start: f32,
    drag_start: f32,
    drag_end: f32,
) -> bool {
    let outside_tabs = x < tabs_start || x >= drag_start;
    x >= 48. && x < (width - 138.).min(drag_end) && y >= 4. && outside_tabs
}

pub struct CaptionBridge(HWND);
pub fn work_area(window: &Window) -> Option<gpui::Bounds<gpui::Pixels>> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    let hwnd = crate::windows_host::hwnd(window).ok()?;
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let scale = window.scale_factor();
    if !unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) }
        .as_bool()
    {
        return None;
    }
    let dip = |value: i32| gpui::px(value as f32 / scale);
    let rect = info.rcWork;
    Some(gpui::Bounds::new(
        gpui::point(dip(rect.left), dip(rect.top)),
        gpui::size(dip(rect.right - rect.left), dip(rect.bottom - rect.top)),
    ))
}

impl CaptionBridge {
    pub fn install(window: &Window) -> Result<Self, String> {
        Self::new(window, false)
    }
    pub fn install_pool(window: &Window) -> Result<Self, String> {
        Self::new(window, true)
    }
    fn new(window: &Window, pool: bool) -> Result<Self, String> {
        let hwnd = crate::windows_host::hwnd(window)?;
        if unsafe { SetWindowSubclass(hwnd, Some(caption_proc), CAPTION_SUBCLASS, pool as usize) }
            .as_bool()
        {
            Ok(Self(hwnd))
        } else {
            Err("无法启用原生标题栏拖动".into())
        }
    }

    pub fn set_pool_border(&self, rgb: u32) {
        use windows::Win32::Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
        };
        let color = rgb.swap_bytes() >> 8;
        let corner = DWMWCP_ROUND.0;
        unsafe {
            let _ = DwmSetWindowAttribute(self.0, DWMWA_BORDER_COLOR, (&raw const color).cast(), 4);
            let _ = DwmSetWindowAttribute(
                self.0,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corner).cast(),
                4,
            );
        }
    }
}
impl Drop for CaptionBridge {
    fn drop(&mut self) {
        unsafe {
            let _ = RemoveWindowSubclass(self.0, Some(caption_proc), CAPTION_SUBCLASS);
        }
    }
}

unsafe extern "system" fn caption_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    pool: usize,
) -> LRESULT {
    // This callback has no GPUI entity borrows and owns no heap state. Windows
    // can run the modal move/size loop here without suppressing its default.
    unsafe {
        if message == WM_NCHITTEST {
            let native = DefWindowProcW(hwnd, message, wparam, lparam);
            if matches!(
                native.0 as u32,
                HTLEFT
                    | HTRIGHT
                    | HTTOP
                    | HTTOPLEFT
                    | HTTOPRIGHT
                    | HTBOTTOM
                    | HTBOTTOMLEFT
                    | HTBOTTOMRIGHT
            ) {
                return native;
            }
            if pool != 0 {
                return DefSubclassProc(hwnd, message, wparam, lparam);
            }
            let mut point = POINT {
                x: lparam.0 as i16 as i32,
                y: ((lparam.0 >> 16) as i16) as i32,
            };
            let mut client = RECT::default();
            if ScreenToClient(hwnd, &mut point).as_bool()
                && GetClientRect(hwnd, &mut client).is_ok()
            {
                let scale = GetDpiForWindow(hwnd) as f32 / 96.;
                if scale > 0. {
                    let (x, y, width) = (
                        point.x as f32 / scale,
                        point.y as f32 / scale,
                        (client.right - client.left) as f32 / scale,
                    );
                    if OVERLAY.load(Ordering::Relaxed) && y >= 0. {
                        return LRESULT(HTCLIENT as isize);
                    }
                    if (0.0..crate::theme::TITLEBAR).contains(&y) {
                        let tabs_start = f32::from_bits(TABS_START.load(Ordering::Relaxed));
                        let drag_start = f32::from_bits(DRAG_START.load(Ordering::Relaxed));
                        return LRESULT(if caption_drag_region(
                            x,
                            y,
                            width,
                            tabs_start,
                            drag_start,
                            f32::from_bits(DRAG_END.load(Ordering::Relaxed)),
                        ) {
                            HTCAPTION
                        } else {
                            HTCLIENT
                        } as isize);
                    }
                }
            }
        }
        if matches!(message, WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK)
            && wparam.0 == HTCAPTION as usize
        {
            let _ = ReleaseCapture();
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(caption_proc), CAPTION_SUBCLASS);
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::caption_drag_region;

    #[test]
    fn theme_switch_and_window_controls_do_not_start_a_native_caption_drag() {
        for x in [1120., 1150., 1200., 1230., 1320.] {
            assert!(!caption_drag_region(x, 18., 1360., 300., 650., 1120.));
        }
        assert!(caption_drag_region(800., 18., 1360., 300., 650., 1120.));
        assert!(!caption_drag_region(400., 18., 1360., 300., 650., 1120.));
        assert!(!caption_drag_region(22., 18., 1360., 300., 650., 1120.));
    }
}
