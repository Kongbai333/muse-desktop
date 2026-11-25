//! Platform conventions shared by the native window and studio.
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into()));
    root.join("MuseDesktopNext")
}

pub struct LocalTime {
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

pub fn local_time(milliseconds: u64) -> Option<LocalTime> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::{
            Foundation::{FILETIME, SYSTEMTIME},
            System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
        };
        let ticks = milliseconds
            .checked_add(11_644_473_600_000)?
            .checked_mul(10_000)?;
        let file = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let mut utc = SYSTEMTIME::default();
        let mut local = SYSTEMTIME::default();
        unsafe {
            FileTimeToSystemTime(&file, &mut utc).ok()?;
            SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
        }
        Some(LocalTime {
            month: local.wMonth,
            day: local.wDay,
            hour: local.wHour,
            minute: local.wMinute,
            second: local.wSecond,
        })
    }
}

pub fn key_binding<A: gpui::Action>(
    key: &str,
    action: A,
    context: Option<&str>,
) -> gpui::KeyBinding {
    gpui::KeyBinding::new(&key, action, context)
}
