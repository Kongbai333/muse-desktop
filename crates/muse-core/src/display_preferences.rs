//! Personal display settings, kept separate from account and browser profiles.

use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayPreferences {
    pub theme: ThemeMode,
    page_zoom: f64,
}

impl Default for DisplayPreferences {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Light,
            page_zoom: 1.0,
        }
    }
}

impl DisplayPreferences {
    pub const MIN_ZOOM: f64 = 0.25;
    pub const MAX_ZOOM: f64 = 3.0;

    pub fn page_zoom(self) -> f64 {
        self.page_zoom
    }

    /// Reject non-numbers and clamp positive values to the browser's menu range.
    pub fn set_page_zoom(&mut self, factor: f64) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let factor = factor.clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        if (self.page_zoom - factor).abs() < 0.0001 {
            return false;
        }
        self.page_zoom = factor;
        true
    }

    pub fn load(root: &Path) -> Self {
        let path = root.join("display.json");
        let read = || -> Option<Self> {
            if std::fs::metadata(&path).ok()?.len() > 4096 {
                return None;
            }
            let mut value: Self = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            if !value.page_zoom.is_finite()
                || !(Self::MIN_ZOOM..=Self::MAX_ZOOM).contains(&value.page_zoom)
            {
                value.page_zoom = 1.0;
            }
            Some(value)
        };
        read().unwrap_or_default()
    }

    pub fn save(self, root: &Path) -> Result<(), String> {
        let temporary = root.join(format!(".display-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(root)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec(&self)?)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, root.join("display.json"))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|_| "无法保存页面缩放与主题设置".into())
    }
}
