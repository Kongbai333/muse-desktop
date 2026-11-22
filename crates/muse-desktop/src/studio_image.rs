//! Local-only image viewing; original recovery is a separate, explicit operation.
use crate::studio::{Entry, Mode, safe_media_path};
use gpui::ScrollHandle;
use std::path::{Path, PathBuf};

pub struct ImageViewer {
    pub entry: uuid::Uuid,
    /// None fits the viewport; Some(1.) shows the image at its actual size.
    pub zoom: Option<f32>,
    pub scroll: ScrollHandle,
    /// Pixel size read from the file itself, so 100% is the real size.
    pub natural: (u32, u32),
    /// Viewport width, height and scale of the last frame; zooming keeps the
    /// same point of the picture in the middle of the view.
    pub layout: std::cell::Cell<(f32, f32, f32)>,
}

/// Steps for the − / + buttons, Ctrl + wheel and Ctrl + − / =.
const ZOOM_STEPS: [f32; 15] = [
    0.05, 0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 1., 1.25, 1.5, 2., 3., 4., 6., 8.,
];

/// Next zoom step from `current`; `direction` > 0 zooms in.
pub fn zoom_step(current: f32, direction: i32) -> f32 {
    if direction > 0 {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|step| *step > current * 1.01)
            .unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1])
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|step| *step < current * 0.99)
            .unwrap_or(ZOOM_STEPS[0])
    }
}

impl ImageViewer {
    pub fn open(root: &Path, entry: &Entry) -> Option<Self> {
        let path = Self::cached_path(root, entry)?;
        let natural = image::image_dimensions(&path)
            .ok()
            .or_else(|| entry.media_width.zip(entry.media_height))
            .filter(|(width, height)| *width > 0 && *height > 0)
            .unwrap_or((1600, 900));
        Some(Self {
            entry: entry.id,
            zoom: None,
            scroll: ScrollHandle::default(),
            natural,
            layout: std::cell::Cell::new((0., 0., 0.)),
        })
    }

    pub fn cached_path(root: &Path, entry: &Entry) -> Option<PathBuf> {
        if entry.mode != Mode::Image {
            return None;
        }
        entry
            .media
            .as_deref()
            .and_then(|relative| safe_media_path(root, relative))
            .filter(|path| path.is_file())
    }

    pub fn status(entry: &Entry, notice: &str) -> String {
        if !notice.is_empty() {
            notice.into()
        } else if entry.running
            || (!entry.media_original && !entry.status.is_empty() && entry.status != "已完成")
        {
            entry.status.clone()
        } else if entry.media_original {
            "原图已保存到本地，缩放后可滚动查看细节".into()
        } else {
            "当前为本地预览，获取原图后会自动保存并显示".into()
        }
    }
}

pub fn image_size(
    width: u32,
    height: u32,
    viewport_width: f32,
    viewport_height: f32,
    zoom: Option<f32>,
) -> (f32, f32, f32) {
    let width = width.max(1) as f32;
    let height = height.max(1) as f32;
    let fit = (viewport_width.max(1.) / width)
        .min(viewport_height.max(1.) / height)
        .min(8.);
    let scale = zoom
        .filter(|zoom| zoom.is_finite() && *zoom > 0.)
        .unwrap_or(fit);
    (width * scale, height * scale, scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::{Entry, Mode};

    fn entry() -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": "10000000-0000-0000-0000-000000000001",
            "account": "20000000-0000-0000-0000-000000000002",
            "name": "cache fixture", "prompt": "fixture", "mode": "image",
            "text": "", "status": "已完成", "running": false,
            "media": "media/original.png", "media_original": true,
            "media_width": 1600, "media_height": 900
        }))
        .unwrap()
    }

    fn cached_fixture() -> (PathBuf, Entry, Vec<u8>) {
        let root = std::env::temp_dir().join(format!("muse-image-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("media")).unwrap();
        let path = root.join("media/original.png");
        image::RgbaImage::from_pixel(16, 9, image::Rgba([50, 100, 150, 255]))
            .save(&path)
            .unwrap();
        let mut image = entry();
        image.media_width = Some(16);
        image.media_height = Some(9);
        let bytes = std::fs::read(path).unwrap();
        (root, image, bytes)
    }

    #[test]
    fn cached_original_reopens_after_history_restore_without_source_account() {
        let (root, image, bytes) = cached_fixture();
        let path = root.join("media/original.png");
        let restored: Entry = serde_json::from_slice(&serde_json::to_vec(&image).unwrap()).unwrap();
        assert!(restored.source_tab.is_none());
        assert!(restored.source_url.is_none());
        for _ in 0..3 {
            let viewer =
                ImageViewer::open(&root, &restored).expect("open the local original offline");
            assert_eq!(viewer.entry, restored.id);
            assert_eq!(
                ImageViewer::cached_path(&root, &restored),
                Some(path.clone())
            );
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_and_unsafe_images_do_not_open_a_viewer() {
        let (root, mut image, _) = cached_fixture();
        image.media = Some(format!("media/missing-{}.png", uuid::Uuid::new_v4()));
        assert!(ImageViewer::open(&root, &image).is_none());
        image.media = Some("media/../outside.png".into());
        assert!(ImageViewer::open(&root, &image).is_none());
        image.media = Some("media/original.png".into());
        image.mode = Mode::Video;
        assert!(ImageViewer::open(&root, &image).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_preview_remains_a_preview_when_opened_repeatedly() {
        let (root, mut image, bytes) = cached_fixture();
        image.media_original = false;
        for _ in 0..3 {
            assert!(ImageViewer::open(&root, &image).is_some());
            assert!(!image.media_original);
            assert_eq!(
                std::fs::read(root.join("media/original.png")).unwrap(),
                bytes
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn original_recovery_failure_stays_visible_after_the_job_stops() {
        let mut image = entry();
        image.media_original = false;
        image.running = false;
        image.status = "没有找到原任务，旧预览仍保留".into();
        assert_eq!(
            ImageViewer::status(&image, ""),
            "没有找到原任务，旧预览仍保留"
        );
        assert_eq!(ImageViewer::status(&image, "原账号已移除"), "原账号已移除");
    }

    #[test]
    fn zoom_steps_move_to_the_neighbouring_step_and_stop_at_the_ends() {
        assert_eq!(zoom_step(1., 1), 1.25);
        assert_eq!(zoom_step(1., -1), 0.75);
        assert_eq!(zoom_step(0.42, 1), 0.5);
        assert_eq!(zoom_step(0.42, -1), 0.33);
        assert_eq!(zoom_step(8., 1), 8.);
        assert_eq!(zoom_step(0.05, -1), 0.05);
    }

    #[test]
    fn viewer_uses_the_real_file_size_for_100_percent() {
        let (root, mut image, _) = cached_fixture();
        image.media_width = Some(1600);
        image.media_height = Some(900);
        let viewer = ImageViewer::open(&root, &image).unwrap();
        assert_eq!(viewer.natural, (16, 9));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fit_and_actual_size_preserve_portrait_and_landscape_dimensions() {
        assert_eq!(image_size(1600, 900, 800., 600., None), (800., 450., 0.5));
        assert_eq!(
            image_size(900, 1600, 800., 600., None),
            (337.5, 600., 0.375)
        );
        assert_eq!(
            image_size(1600, 900, 800., 600., Some(1.)),
            (1600., 900., 1.)
        );
        assert_eq!(
            image_size(900, 1600, 800., 600., Some(2.)),
            (1800., 3200., 2.)
        );
        let (width, height, _) = image_size(0, 0, 0., 0., None);
        assert!(width.is_finite() && height.is_finite() && width > 0. && height > 0.);
    }
}
