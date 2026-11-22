//! Platform conventions shared by the native window and studio.
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into()));
    #[cfg(target_os = "macos")]
    let root = PathBuf::from(std::env::var_os("HOME").expect("Cannot locate the user home"))
        .join("Library/Application Support");
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
    #[cfg(target_os = "macos")]
    {
        let seconds: libc::time_t = (milliseconds / 1000).try_into().ok()?;
        let mut output = std::mem::MaybeUninit::<libc::tm>::uninit();
        // localtime_r writes a caller-owned tm and is safe on background tasks.
        if unsafe { libc::localtime_r(&seconds, output.as_mut_ptr()) }.is_null() {
            return None;
        }
        let output = unsafe { output.assume_init() };
        Some(LocalTime {
            month: (output.tm_mon + 1) as u16,
            day: output.tm_mday as u16,
            hour: output.tm_hour as u16,
            minute: output.tm_min as u16,
            second: output.tm_sec as u16,
        })
    }
}

pub fn key_binding<A: gpui::Action>(
    key: &str,
    action: A,
    context: Option<&str>,
) -> gpui::KeyBinding {
    #[cfg(target_os = "macos")]
    let key = key.replace("ctrl-", "cmd-");
    gpui::KeyBinding::new(&key, action, context)
}
