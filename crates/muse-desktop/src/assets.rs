use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

pub struct Assets;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$((concat!("icons/", $name, ".svg"),
            include_bytes!(concat!("../../../assets/icons/", $name, ".svg")).as_slice())),*]
    };
}

const ICONS: &[(&str, &[u8])] = icons![
    "chevron-left",
    "chevron-right",
    "arrow-left",
    "arrow-right",
    "reload",
    "home",
    "plus",
    "x",
    "search",
    "settings",
    "layout-sidebar",
    "external-link",
    "chart-donut",
    "shield-check",
    "folder",
    "device-desktop",
    "command",
    "sun",
    "flask",
    "circle-check",
    "copy",
    "pencil",
    "trash",
    "help-circle",
    "world",
    "chevrons-right",
    "dots",
    "chevron-down",
    "arrow-up-right",
    "user",
    "layout-grid",
    "window-minimize",
    "window-maximize",
    "window-restore",
    "message",
    "image",
    "video",
    "send",
    "at-sign",
    "arrow-up",
    "cookie",
    "users",
    "ratio-auto",
    "ratio-square",
    "ratio-wide",
    "ratio-phone",
    "ratio-classic",
    "ratio-portrait",
    "duration",
    "play",
    "stop",
    "maximize",
    "download",
    "clipboard-text",
    "zoom-in",
    "zoom-out",
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
