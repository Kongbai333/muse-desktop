//! Window preferences are independent of accounts and browser profiles.

use gpui::{Bounds, Pixels, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl SavedWindow {
    pub fn capture(bounds: WindowBounds) -> Option<Self> {
        let rect = bounds.get_bounds();
        let saved = Self {
            x: rect.origin.x.into(),
            y: rect.origin.y.into(),
            width: rect.size.width.into(),
            height: rect.size.height.into(),
            maximized: matches!(
                bounds,
                WindowBounds::Maximized(_) | WindowBounds::Fullscreen(_)
            ),
        };
        saved.valid().then_some(saved)
    }
    fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.x.abs() <= 100_000.
            && self.y.abs() <= 100_000.
            && (200.0..=16000.0).contains(&self.width)
            && (150.0..=16000.0).contains(&self.height)
    }
    pub fn restore(self, displays: &[Bounds<Pixels>]) -> Option<WindowBounds> {
        if !self.valid() {
            return None;
        }
        let display = displays
            .iter()
            .filter(|d| d.size.width > px(0.) && d.size.height > px(0.))
            .max_by(|a, b| {
                let overlap = |display: &&Bounds<Pixels>| {
                    let x = f32::from(display.origin.x);
                    let y = f32::from(display.origin.y);
                    let w = (self.x + self.width).min(x + f32::from(display.size.width))
                        - self.x.max(x);
                    let h = (self.y + self.height).min(y + f32::from(display.size.height))
                        - self.y.max(y);
                    w.max(0.) * h.max(0.)
                };
                overlap(a).total_cmp(&overlap(b))
            })?;
        let display_width = f32::from(display.size.width);
        let display_height = f32::from(display.size.height);
        let width = self.width.max(1030.).min(display_width);
        let height = self.height.max(680.).min(display_height);
        let left = f32::from(display.origin.x);
        let top = f32::from(display.origin.y);
        let bounds = Bounds::new(
            point(
                px(self.x.clamp(left, left + display_width - width)),
                px(self.y.clamp(top, top + display_height - height)),
            ),
            size(px(width), px(height)),
        );
        Some(if self.maximized {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        })
    }
    pub fn load(root: &Path) -> Option<Self> {
        let path = root.join("window.json");
        if std::fs::metadata(&path).ok()?.len() > 4096 {
            return None;
        }
        let saved: Self = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        saved.valid().then_some(saved)
    }
    pub fn save(self, root: &Path) -> Result<(), String> {
        if !self.valid() {
            return Err("窗口位置或大小无效".into());
        }
        let temporary = root.join(format!(".window-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            use std::io::Write;
            std::fs::create_dir_all(root)?;
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(serde_json::to_string_pretty(&self)?.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, root.join("window.json"))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|_| "无法保存窗口位置和大小".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }
    fn sample() -> SavedWindow {
        SavedWindow {
            x: 120.,
            y: 80.,
            width: 1180.,
            height: 760.,
            maximized: false,
        }
    }

    #[test]
    fn normal_and_maximized_preferences_roundtrip_without_touching_accounts() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/window-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("workspace.json"), "account-fixture-do-not-change").unwrap();
        for maximized in [false, true] {
            let expected = SavedWindow {
                maximized,
                ..sample()
            };
            expected.save(&root).unwrap();
            assert_eq!(SavedWindow::load(&root), Some(expected));
            let restored = expected.restore(&[rect(0., 0., 1920., 1080.)]).unwrap();
            assert_eq!(matches!(restored, WindowBounds::Maximized(_)), maximized);
            assert_eq!(restored.get_bounds(), rect(120., 80., 1180., 760.));
        }
        assert_eq!(
            std::fs::read_to_string(root.join("workspace.json")).unwrap(),
            "account-fixture-do-not-change"
        );
    }

    #[test]
    fn maximized_capture_keeps_the_normal_restore_size() {
        let normal = rect(120., 80., 1180., 760.);
        let captured = SavedWindow::capture(WindowBounds::Maximized(normal)).unwrap();
        assert_eq!(
            captured,
            SavedWindow {
                maximized: true,
                ..sample()
            }
        );
        assert_eq!(
            SavedWindow::capture(WindowBounds::Windowed(normal)),
            Some(sample())
        );
    }

    #[test]
    fn invalid_geometry_is_rejected_and_offscreen_windows_are_recovered() {
        let display = rect(0., 0., 1920., 1080.);
        for invalid in [
            SavedWindow {
                width: f32::NAN,
                ..sample()
            },
            SavedWindow {
                height: -10.,
                ..sample()
            },
            SavedWindow {
                x: f32::INFINITY,
                ..sample()
            },
            SavedWindow {
                width: 100_000.,
                ..sample()
            },
        ] {
            assert!(invalid.restore(&[display]).is_none());
        }
        let offscreen = SavedWindow {
            x: 6000.,
            y: 3000.,
            ..sample()
        };
        let bounds = offscreen.restore(&[display]).unwrap().get_bounds();
        assert!(bounds.origin.x >= display.origin.x);
        assert!(bounds.origin.y >= display.origin.y);
        assert!(bounds.origin.x + bounds.size.width <= px(1920.));
        assert!(bounds.origin.y + bounds.size.height <= px(1080.));
        assert_eq!(bounds.size, size(px(1180.), px(760.)));
        let small = sample()
            .restore(&[rect(0., 0., 900., 600.)])
            .unwrap()
            .get_bounds();
        assert!(small.size.width <= px(900.) && small.size.height <= px(600.));
    }

    #[test]
    fn secondary_monitor_and_truncated_preferences_do_not_lose_access_to_caption() {
        let secondary = rect(-1920., 0., 1920., 1080.);
        let saved = SavedWindow {
            x: -1800.,
            ..sample()
        };
        assert_eq!(
            saved
                .restore(&[rect(0., 0., 1920., 1080.), secondary])
                .unwrap()
                .get_bounds()
                .origin
                .x,
            px(-1800.)
        );
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/window-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("window.json"), "{broken").unwrap();
        assert!(SavedWindow::load(&root).is_none());
        sample().save(&root).unwrap();
        assert_eq!(SavedWindow::load(&root), Some(sample()));
    }
}
