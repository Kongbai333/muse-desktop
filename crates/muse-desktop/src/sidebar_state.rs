//! Sidebar preferences are independent of account and browser data.

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DIVIDER: f32 = 6.;
pub const COMPACT: f32 = 72.;
const MIN: f32 = 288.;
const MAX: f32 = 560.;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SidebarState {
    pub width: f32,
    pub collapsed: bool,
}
impl Default for SidebarState {
    fn default() -> Self {
        Self {
            width: 300.,
            collapsed: false,
        }
    }
}
impl SidebarState {
    fn limit(viewport: f32, inspector: bool) -> f32 {
        (viewport
            - DIVIDER
            - if inspector {
                crate::theme::INSPECTOR
            } else {
                0.
            }
            - crate::theme::MIN_BROWSER_WIDTH)
            .clamp(1., MAX)
    }
    pub fn preferred_extent(self) -> f32 {
        if self.collapsed {
            COMPACT
        } else {
            self.width + DIVIDER
        }
    }
    #[cfg(test)]
    pub fn extent(self, viewport: f32, inspector: bool) -> f32 {
        if self.collapsed {
            COMPACT
        } else {
            self.width.min(Self::limit(viewport, inspector)) + DIVIDER
        }
    }
    pub fn resize(&mut self, width: f32, viewport: f32, inspector: bool) {
        if width.is_finite() {
            let max = Self::limit(viewport, inspector);
            self.width = width.clamp(MIN.min(max), max);
        }
    }
    pub fn load(root: &Path) -> Self {
        let path = root.join("sidebar.json");
        let load = || -> Option<Self> {
            if std::fs::metadata(&path).ok()?.len() > 1024 {
                return None;
            }
            let value: Self = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
            (value.width.is_finite() && (MIN..=MAX).contains(&value.width)).then_some(value)
        };
        load().unwrap_or_default()
    }
    pub fn save(self, root: &Path) -> Result<(), String> {
        let temporary = root.join(format!(".sidebar-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(root)?;
            std::fs::write(&temporary, serde_json::to_vec(&self)?)?;
            std::fs::rename(&temporary, root.join("sidebar.json"))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|_| "无法保存账号栏宽度".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_toggle_and_restore_preserve_dragged_width() {
        let mut state = SidebarState::default();
        state.resize(452., 1360., false);
        assert_eq!(state.extent(1360., false), 458.);
        state.collapsed = true;
        assert_eq!(state.extent(1360., false), 72.);
        state.collapsed = false;
        assert_eq!(state.extent(1360., false), 458.);
        assert_eq!(state.extent(1030., true), 406.);
        state.resize(-1., 1360., false);
        assert_eq!(state.extent(1360., false), 294.);
        state.resize(f32::NAN, 1360., false);
        assert_eq!(state.extent(1360., false), 294.);
    }
    #[test]
    fn preferences_roundtrip_and_corruption_cannot_touch_accounts() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sidebar-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("workspace.json"), "unchanged").unwrap();
        let state = SidebarState {
            width: 444.,
            collapsed: true,
        };
        state.save(&root).unwrap();
        assert_eq!(SidebarState::load(&root), state);
        std::fs::write(
            root.join("sidebar.json"),
            r#"{"width":-50,"collapsed":false}"#,
        )
        .unwrap();
        assert_eq!(SidebarState::load(&root), SidebarState::default());
        std::fs::write(root.join("sidebar.json"), "{truncated").unwrap();
        assert_eq!(SidebarState::load(&root), SidebarState::default());
        assert_eq!(
            std::fs::read_to_string(root.join("workspace.json")).unwrap(),
            "unchanged"
        );
    }
}
