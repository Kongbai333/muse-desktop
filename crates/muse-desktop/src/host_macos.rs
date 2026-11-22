//! Cocoa host operations. All native view access stays on GPUI's UI thread.
use gpui::Window;
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub fn native_window(window: &Window) -> Result<Retained<AnyObject>, String> {
    let handle = window
        .window_handle()
        .map_err(|_| "无法获取应用窗口")?
        .as_raw();
    let RawWindowHandle::AppKit(handle) = handle else {
        return Err("需要 macOS 原生窗口".into());
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<AnyObject>() };
    let result: Option<Retained<AnyObject>> = unsafe { msg_send![view, window] };
    result.ok_or_else(|| "应用窗口尚未就绪".into())
}

pub fn is_minimized(window: &Window) -> bool {
    native_window(window).is_ok_and(|native| {
        let result: Bool = unsafe { msg_send![&*native, isMiniaturized] };
        result.as_bool()
    })
}

pub fn toggle_maximize(window: &Window) {
    window.zoom_window();
}

// AppKit continues delivering a mouse drag to the originating view until mouse-up.
pub fn capture_pointer(_: &Window, _: bool) {}

pub fn pixel_matches(_: &Window, _: f32, _: f32, _: u32) -> Result<bool, String> {
    Err("Windows 像素冒烟检查不适用于 macOS；请检查原生应用画面".into())
}

pub fn hide_standard_controls(window: &Window) {
    if let Ok(native) = native_window(window) {
        for kind in 0..3isize {
            let button: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*native, standardWindowButton: kind] };
            if let Some(button) = button {
                let _: () = unsafe { msg_send![&*button, setHidden: Bool::YES] };
            }
        }
    }
}

pub fn reduce_motion() -> bool {
    unsafe {
        let workspace: Retained<AnyObject> = msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        let reduced: Bool = msg_send![&*workspace, accessibilityDisplayShouldReduceMotion];
        reduced.as_bool()
    }
}

pub fn write_image_clipboard(png: &[u8]) -> Result<(), String> {
    use objc2_foundation::{NSData, NSString};
    let data = NSData::with_bytes(png);
    let kind = NSString::from_str("public.png");
    unsafe {
        let pasteboard: Retained<AnyObject> =
            msg_send![objc2::class!(NSPasteboard), generalPasteboard];
        let _: isize = msg_send![&*pasteboard, clearContents];
        let written: Bool = msg_send![&*pasteboard, setData: &*data, forType: &*kind];
        if written.as_bool() {
            Ok(())
        } else {
            Err("无法写入图片剪贴板".into())
        }
    }
}
