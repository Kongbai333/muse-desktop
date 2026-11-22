//! Reference images for image-to-image and image-to-video.
//!
//! A chosen picture is decoded, turned upright from its EXIF orientation,
//! limited to 2048 px on the long edge, flattened onto white and stored as a
//! JPEG under `media/`, so it stays with the record that used it. The page
//! adapter (`muse-studio.js`) receives it in 24 KB chunks and attaches it to
//! the Muse composer before the prompt is sent.

use std::path::{Path, PathBuf};

/// Muse page adapter limit for one attachment.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_EDGE: u32 = 2048;
const MAX_SOURCE: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Reference {
    /// Path relative to the data directory, e.g. `media/ref-<uuid>.jpg`.
    pub relative: String,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
}

pub fn from_file(root: &Path, source: &Path) -> Result<Reference, String> {
    let size = std::fs::metadata(source)
        .map_err(|_| "无法读取这个文件".to_owned())?
        .len();
    if size == 0 {
        return Err("这个文件是空的".into());
    }
    if size > MAX_SOURCE {
        return Err("图片文件超过 64 MB，请换一张小一点的图片".into());
    }
    let bytes = std::fs::read(source).map_err(|_| "无法读取这个文件".to_owned())?;
    from_bytes(root, &bytes)
}

pub fn from_bytes(root: &Path, bytes: &[u8]) -> Result<Reference, String> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "无法识别这张图片".to_owned())?;
    if reader.format().is_none() {
        return Err("请选择 JPG、PNG、WebP 或 GIF 图片".into());
    }
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "不支持这种图片格式".to_owned())?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 120_000_000 {
        return Err("图片尺寸不受支持".into());
    }
    let mut picture =
        image::DynamicImage::from_decoder(decoder).map_err(|_| "图片解码失败".to_owned())?;
    picture.apply_orientation(orientation);
    if picture.width().max(picture.height()) > MAX_EDGE {
        picture = picture.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::Triangle);
    }
    let rgb = flatten(&picture);
    let (width, height) = rgb.dimensions();
    let mut encoded = Vec::new();
    for quality in [88u8, 80, 72, 64, 56] {
        encoded.clear();
        image::DynamicImage::ImageRgb8(rgb.clone())
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut encoded,
                quality,
            ))
            .map_err(|_| "图片压缩失败".to_owned())?;
        if encoded.len() <= MAX_BYTES {
            break;
        }
    }
    if encoded.is_empty() || encoded.len() > MAX_BYTES {
        return Err("图片压缩后仍超过 8 MB，请换一张图片".into());
    }
    let relative = format!("media/ref-{}.jpg", uuid::Uuid::new_v4());
    let path = crate::studio::safe_media_path(root, &relative).ok_or("参考图路径无效")?;
    std::fs::create_dir_all(root.join("media")).map_err(|_| "无法保存参考图".to_owned())?;
    std::fs::write(&path, &encoded).map_err(|_| "无法保存参考图".to_owned())?;
    Ok(Reference {
        relative,
        path,
        width,
        height,
        bytes: encoded.len(),
    })
}

/// A reference saved earlier, e.g. when a record is opened for editing again.
pub fn load(root: &Path, relative: &str) -> Option<Reference> {
    let path = crate::studio::safe_media_path(root, relative)?;
    let bytes = std::fs::metadata(&path).ok()?.len() as usize;
    if bytes == 0 || bytes > MAX_BYTES {
        return None;
    }
    let (width, height) = image::image_dimensions(&path).ok()?;
    Some(Reference {
        relative: relative.to_owned(),
        path,
        width,
        height,
        bytes,
    })
}

/// Transparent pixels become white, as Muse shows a JPEG.
fn flatten(picture: &image::DynamicImage) -> image::RgbImage {
    if !picture.color().has_alpha() {
        return picture.to_rgb8();
    }
    let rgba = picture.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = u16::from(a);
        let blend = |channel: u8| ((u16::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8;
        rgb.put_pixel(x, y, image::Rgb([blend(r), blend(g), blend(b)]));
    }
    rgb
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/reference-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn large_transparent_png_becomes_bounded_white_backed_jpeg() {
        let root = root();
        let mut source = image::RgbaImage::from_pixel(3000, 1500, image::Rgba([0, 0, 0, 0]));
        source.put_pixel(10, 10, image::Rgba([255, 0, 0, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        source.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let reference = from_bytes(&root, png.get_ref()).unwrap();
        assert_eq!((reference.width, reference.height), (2048, 1024));
        assert!(reference.bytes <= MAX_BYTES);
        let saved = image::open(&reference.path).unwrap().to_rgb8();
        assert!(
            saved
                .get_pixel(1000, 600)
                .0
                .iter()
                .all(|channel| *channel > 240)
        );
        assert!(load(&root, &reference.relative).is_some());
        assert!(load(&root, "../outside.jpg").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_images_are_rejected() {
        let root = root();
        assert!(from_bytes(&root, b"not an image").is_err());
        assert!(is_image_path(Path::new("a/B.JPEG")));
        assert!(!is_image_path(Path::new("a/b.txt")));
        std::fs::remove_dir_all(root).unwrap();
    }
}
