//! Bounded frames copied inside an account page, never fetched by the shell.

use gpui::RenderImage;
use muse_core::AccountId;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

struct Avatar {
    signature: u64,
    image: Arc<RenderImage>,
    /// The same clip split into single-frame images. The window chooses the
    /// frame itself, so playback never fast-forwards after the window was
    /// minimised and only runs while the pointer is over the account.
    frames: Vec<Arc<RenderImage>>,
    frame_ms: u32,
}
#[derive(Default)]
pub struct AvatarCache(HashMap<AccountId, Avatar>, Option<PathBuf>);
fn decode_frame(data: &str) -> Option<image::RgbaImage> {
    use base64::Engine;
    if data.len() > 48_000 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.strip_prefix("data:image/png;base64,")?)
        .ok()?;
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if width == 0 || height == 0 || width > 128 || height > 128 {
        return None;
    }
    let mut pixels = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(pixels)
}
fn signature(value: impl Hash) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hash);
    hash.finish()
}
fn center_square(pixels: image::RgbaImage) -> image::RgbaImage {
    let (width, height) = pixels.dimensions();
    let edge = width.min(height);
    if width == height {
        pixels
    } else {
        image::imageops::crop_imm(&pixels, (width - edge) / 2, (height - edge) / 2, edge, edge)
            .to_image()
    }
}
impl AvatarCache {
    pub fn load(root: &Path, workspace: &muse_core::Workspace) -> Self {
        let directory = root.join("account-cache");
        let mut cache = Self::default();
        for account in &workspace.accounts {
            let path = directory.join(format!("{}-avatar.json", account.id));
            if std::fs::metadata(&path).is_ok_and(|m| m.len() <= 2_400_000)
                && let Ok(bytes) = std::fs::read(path)
                && let Ok(clip) =
                    serde_json::from_slice::<crate::page_observer::AvatarAnimation>(&bytes)
            {
                if clip.frames.len() == 1 {
                    cache.update(account.id, &clip.frames[0]);
                } else {
                    cache.update_animation(account.id, &clip);
                }
            }
        }
        cache.1 = Some(directory);
        cache
    }
    fn persist(&self, id: AccountId, clip: &crate::page_observer::AvatarAnimation) {
        if let Some(root) = &self.1
            && let Ok(bytes) = serde_json::to_vec(clip)
        {
            let _ = crate::account_cache::write(root, &format!("{id}-avatar.json"), &bytes);
        }
    }
    pub fn update(&mut self, id: AccountId, data: &str) -> bool {
        let key = signature(data);
        if self
            .0
            .get(&id)
            .is_some_and(|a| a.signature == key || a.image.frame_count() > 1)
        {
            return false;
        }
        let Some(pixels) = decode_frame(data) else {
            return false;
        };
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(center_square(
            pixels,
        ))]));
        self.0.insert(
            id,
            Avatar {
                signature: key,
                frames: vec![image.clone()],
                image,
                frame_ms: 125,
            },
        );
        self.persist(
            id,
            &crate::page_observer::AvatarAnimation {
                frames: vec![data.to_owned()],
                frame_ms: 125,
            },
        );
        true
    }
    pub fn update_animation(
        &mut self,
        id: AccountId,
        clip: &crate::page_observer::AvatarAnimation,
    ) -> bool {
        if !(2..=48).contains(&clip.frames.len()) || !(100..=250).contains(&clip.frame_ms) {
            return false;
        }
        let key = signature((&clip.frames, clip.frame_ms));
        if self.0.get(&id).is_some_and(|a| a.signature == key) {
            return false;
        }
        let mut frames = Vec::with_capacity(clip.frames.len());
        let mut singles = Vec::with_capacity(clip.frames.len());
        let mut dimensions = None;
        let mut differs = false;
        for data in &clip.frames {
            let Some(pixels) = decode_frame(data) else {
                return false;
            };
            if dimensions.is_some_and(|d| d != pixels.dimensions()) {
                return false;
            }
            dimensions = Some(pixels.dimensions());
            let pixels = center_square(pixels);
            if let Some(first) = frames.first() {
                let first: &image::Frame = first;
                differs |= first.buffer() != &pixels;
            }
            singles.push(Arc::new(RenderImage::new(vec![image::Frame::new(
                pixels.clone(),
            )])));
            frames.push(image::Frame::from_parts(
                pixels,
                0,
                0,
                image::Delay::from_numer_denom_ms(clip.frame_ms, 1),
            ));
        }
        if !differs {
            frames.truncate(1);
            singles.truncate(1);
        }
        self.0.insert(
            id,
            Avatar {
                signature: key,
                image: Arc::new(RenderImage::new(frames)),
                frames: singles,
                frame_ms: clip.frame_ms,
            },
        );
        self.persist(id, clip);
        true
    }
    pub fn image(&self, id: AccountId) -> Option<Arc<RenderImage>> {
        self.0.get(&id).map(|a| a.image.clone())
    }
    /// One still frame of the clip; `index` wraps around.
    pub fn frame(&self, id: AccountId, index: usize) -> Option<Arc<RenderImage>> {
        let avatar = self.0.get(&id)?;
        avatar
            .frames
            .get(index % avatar.frames.len().max(1))
            .cloned()
    }
    pub fn frame_ms(&self, id: AccountId) -> u32 {
        self.0.get(&id).map_or(125, |a| a.frame_ms.max(1))
    }
    pub fn is_animated(&self, id: AccountId) -> bool {
        self.0.get(&id).is_some_and(|a| a.frames.len() > 1)
    }
    pub fn remove(&mut self, id: AccountId) {
        self.0.remove(&id);
        if let Some(root) = &self.1 {
            let _ = std::fs::remove_file(root.join(format!("{id}-avatar.json")));
        }
    }
    pub fn retain(&mut self, state: &muse_core::Workspace) {
        self.0.retain(|id, _| state.account(*id).is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    fn png() -> String {
        let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([20, 150, 220, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
        )
    }
    #[test]
    fn page_avatar_is_account_scoped_and_repeated_frames_are_not_reallocated() {
        let mut cache = AvatarCache::default();
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let data = png();
        assert!(cache.update(a, &data));
        assert!(cache.image(a).is_some());
        assert!(cache.image(b).is_none());
        assert!(!cache.update(a, &data));
        cache.remove(a);
        assert!(cache.image(a).is_none());
    }
    #[test]
    fn urls_invalid_images_and_oversized_payloads_are_rejected() {
        let mut cache = AvatarCache::default();
        let id = uuid::Uuid::new_v4();
        for value in [
            "https://example.test/avatar.png".to_string(),
            "data:image/png;base64,eA==".into(),
            format!("data:image/png;base64,{}", "a".repeat(100000)),
        ] {
            assert!(!cache.update(id, &value));
        }
        assert!(cache.image(id).is_none());
    }
    #[test]
    fn png_dimension_limit_is_checked_before_decoding() {
        let mut bytes = vec![0u8; 24];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[16..20].copy_from_slice(&100000u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&100000u32.to_be_bytes());
        let value = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        let mut cache = AvatarCache::default();
        assert!(!cache.update(uuid::Uuid::new_v4(), &value));
    }
    #[test]
    fn animated_account_clip_plays_distinct_frames_and_static_polls_cannot_freeze_it() {
        let mut cache = AvatarCache::default();
        let id = uuid::Uuid::new_v4();
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(8, 8, image::Rgba([220, 20, 50, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let second = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
        );
        let clip = crate::page_observer::AvatarAnimation {
            frames: vec![png(), second],
            frame_ms: 125,
        };
        assert!(cache.update_animation(id, &clip));
        let animated = cache.image(id).unwrap();
        assert_eq!(animated.frame_count(), 2);
        assert_ne!(animated.as_bytes(0), animated.as_bytes(1));
        assert_eq!(animated.delay(1).numer_denom_ms(), (125, 1));
        assert!(!cache.update(id, &png()));
        assert_eq!(cache.image(id).unwrap().id, animated.id);
        assert!(cache.image(uuid::Uuid::new_v4()).is_none());
        let static_clip = crate::page_observer::AvatarAnimation {
            frames: vec![png(); 2],
            frame_ms: 125,
        };
        assert!(cache.update_animation(id, &static_clip));
        assert_eq!(
            cache.image(id).unwrap().frame_count(),
            1,
            "a newly selected static avatar must replace the old animation"
        );
        cache.remove(id);
        assert!(cache.image(id).is_none());
    }
    #[test]
    fn malformed_clips_leave_the_previous_valid_avatar_intact() {
        let mut cache = AvatarCache::default();
        let id = uuid::Uuid::new_v4();
        cache.update(id, &png());
        let original = cache.image(id).unwrap();
        for (frames, frame_ms) in [
            (vec![png(); 49], 125),
            (vec![png(), "invalid".into()], 125),
            (vec![png(); 2], 0),
        ] {
            assert!(!cache.update_animation(
                id,
                &crate::page_observer::AvatarAnimation { frames, frame_ms }
            ));
            assert_eq!(cache.image(id).unwrap().id, original.id);
        }
    }
}
