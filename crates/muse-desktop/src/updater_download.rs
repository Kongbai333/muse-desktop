//! Bounded streaming and strict archive extraction, used only by update workers.

use crate::updater::{self, Result, Update};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Debug)]
pub struct Prepared {
    pub root: PathBuf,
    pub payload: PathBuf,
    pub update: Update,
    pub keep: bool,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        // Only an internally allocated temporary directory can be cleaned up.
        if !self.keep
            && self.root.parent() == Some(std::env::temp_dir().as_path())
            && self
                .root
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with("muse-update-"))
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

#[derive(Clone, Default)]
pub struct Source {
    #[cfg(any(test, debug_assertions))]
    fixture: Option<std::string::String>,
}
impl Source {
    pub fn new(root: &Path, diagnostics: bool) -> Self {
        let mut source = Self::default();
        #[cfg(debug_assertions)]
        if diagnostics {
            let path = root.join("update-fixture-origin.txt");
            if fs::metadata(&path).is_ok_and(|m| m.len() < 512)
                && let Ok(text) = fs::read_to_string(path)
                && let Ok(url) = url::Url::parse(text.trim())
                && url.scheme() == "http"
                && url.host_str() == Some("127.0.0.1")
                && url.port().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
            {
                source.fixture = Some(url.to_string());
            }
        }
        let _ = (root, diagnostics);
        source
    }
    fn request(&self, input: &str, api: bool) -> Result<ureq::http::Response<ureq::Body>> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .proxy(None)
            .timeout_global(Some(Duration::from_secs(600)))
            .timeout_connect(Some(Duration::from_secs(12)))
            .timeout_per_read(Some(Duration::from_secs(5)))
            .build()
            .into();
        let mut current = input.to_owned();
        for _ in 0..6 {
            if !updater::allowed_url(&current, api) {
                return Err("更新下载地址不受信任".into());
            }
            let mut destination = current.clone();
            #[cfg(any(test, debug_assertions))]
            if let Some(origin) = &self.fixture {
                destination = if api {
                    format!("{origin}latest")
                } else {
                    format!(
                        "{origin}assets/{}",
                        url::Url::parse(&current)
                            .map_err(|_| "更新地址无效")?
                            .path()
                            .rsplit('/')
                            .next()
                            .ok_or("更新地址无效")?
                    )
                };
            }
            let response = agent
                .get(&destination)
                .header(
                    "User-Agent",
                    concat!("MuseDesktop/", env!("CARGO_PKG_VERSION")),
                )
                .header(
                    "Accept",
                    if api {
                        "application/vnd.github+json"
                    } else {
                        "application/octet-stream"
                    },
                )
                .header("X-GitHub-Api-Version", "2022-11-28")
                .call()
                .map_err(|_| "无法连接GitHub，请检查网络后重试")?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|h| h.to_str().ok())
                    .ok_or("下载重定向无效")?;
                current = url::Url::parse(&current)
                    .and_then(|u| u.join(location))
                    .map_err(|_| "下载重定向无效")?
                    .into();
            } else if response.status().is_success() {
                return Ok(response);
            } else if matches!(response.status().as_u16(), 403 | 429) {
                return Err("GitHub暂时限制请求，请稍后重试".into());
            } else {
                return Err("GitHub未提供可用更新文件，请稍后重试".into());
            }
        }
        Err("下载重定向次数过多".into())
    }
    fn text(&self, input: &str, api: bool, limit: u64) -> Result<String> {
        let response = self.request(input, api)?;
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "更新信息下载失败")?;
        if bytes.len() as u64 > limit {
            return Err("更新信息超过大小限制".into());
        }
        String::from_utf8(bytes).map_err(|_| "更新信息编码无效".into())
    }
    pub fn check(&self) -> Result<Option<Update>> {
        updater::select_release(
            &self.text(updater::LATEST, true, 1024 * 1024)?,
            env!("CARGO_PKG_VERSION"),
            updater::platform(),
        )
    }
}

pub fn copy_download(
    reader: &mut impl Read,
    target: &Path,
    expected_size: u64,
    expected_hash: &str,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    let partial = target.with_extension("part");
    let result: Result<()> = (|| {
        if expected_size == 0 || expected_size > updater::MAX_PACKAGE || target.exists() {
            return Err("更新包大小或暂存位置无效".into());
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&partial)
            .map_err(|_| "无法创建更新暂存文件")?;
        let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
        let mut size = 0_u64;
        let mut buffer = [0_u8; 65536];
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("已取消下载".into());
            }
            let n = reader.read(&mut buffer).map_err(|_| "下载中断，请重试")?;
            if n == 0 {
                break;
            }
            size = size.checked_add(n as u64).ok_or("更新包过大")?;
            if size > expected_size {
                return Err("下载大小与版本信息不符".into());
            }
            file.write_all(&buffer[..n]).map_err(|_| "更新包保存失败")?;
            hash.update(&buffer[..n]);
            progress(size);
        }
        let actual: String = hash
            .finish()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if size != expected_size || actual != expected_hash {
            return Err("更新包校验失败，未安装任何文件".into());
        }
        file.sync_all().map_err(|_| "更新包保存失败")?;
        drop(file);
        if cancel.load(Ordering::Acquire) {
            return Err("已取消下载".into());
        }
        fs::rename(&partial, target).map_err(|_| "更新包保存失败".into())
    })();
    if result.is_err() {
        let _ = fs::remove_file(partial);
    }
    result
}

pub fn safe_path(name: &str) -> Result<PathBuf> {
    if name.len() > 1024 || name.contains(['\\', ':']) || name.starts_with('/') {
        return Err("更新包包含不安全路径".into());
    }
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    for part in trimmed.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || matches!(part, "." | "..")
            || part.ends_with(['.', ' '])
            || part.chars().any(char::is_control)
            || matches!(
                stem.as_str(),
                "CON"
                    | "PRN"
                    | "AUX"
                    | "NUL"
                    | "COM1"
                    | "COM2"
                    | "COM3"
                    | "COM4"
                    | "COM5"
                    | "COM6"
                    | "COM7"
                    | "COM8"
                    | "COM9"
                    | "LPT1"
                    | "LPT2"
                    | "LPT3"
                    | "LPT4"
                    | "LPT5"
                    | "LPT6"
                    | "LPT7"
                    | "LPT8"
                    | "LPT9"
            )
        {
            return Err("更新包包含不安全路径".into());
        }
    }
    Ok(PathBuf::from(trimmed))
}

pub fn extract_package(
    archive: &Path,
    destination: &Path,
    update: &Update,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    if update.platform != "win-x64" {
        return Err("暂不支持此架构更新".into());
    }
    let mut zip = zip::ZipArchive::new(File::open(archive).map_err(|_| "无法读取更新包")?)
        .map_err(|_| "更新包格式无效")?;
    if zip.len() > 8192 {
        return Err("更新包文件数量异常".into());
    }
    let prefix = format!("Muse-Desktop-{}-win-x64", update.version);
    let mut seen = HashSet::new();
    let mut total = 0_u64;
    for i in 0..zip.len() {
        if cancel.load(Ordering::Acquire) {
            return Err("已取消下载".into());
        }
        let mut entry = zip.by_index(i).map_err(|_| "更新包文件无效")?;
        let relative = safe_path(entry.name())?;
        if entry.encrypted()
            || entry.is_symlink()
            || entry
                .unix_mode()
                .is_some_and(|m| !matches!(m & 0o170000, 0 | 0o100000 | 0o040000))
        {
            return Err("更新包不允许链接或特殊文件".into());
        }
        if !seen.insert(relative.to_string_lossy().to_lowercase()) {
            return Err("更新包包含重复路径".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("更新包解压体积异常")?;
        if total > 1024 * 1024 * 1024
            || entry.size() > updater::MAX_PACKAGE
            || (entry.size() > 1024 * 1024
                && entry.size() > entry.compressed_size().saturating_mul(200))
        {
            return Err("更新包解压体积异常".into());
        }
        let inside = relative
            .strip_prefix(&prefix)
            .map_err(|_| "更新包目录与平台不符")?;
        if !inside.as_os_str().is_empty() && !windows_app_file(inside) {
            return Err("更新包包含非应用文件".into());
        }
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(output).map_err(|_| "无法解压更新包")?;
            continue;
        }
        fs::create_dir_all(output.parent().ok_or("更新包路径无效")?)
            .map_err(|_| "无法解压更新包")?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)
            .map_err(|_| "无法解压更新包")?;
        let written = std::io::copy(
            &mut entry.by_ref().take(updater::MAX_PACKAGE + 1),
            &mut file,
        )
        .map_err(|_| "更新包文件损坏")?;
        if written != entry.size() {
            return Err("更新包文件长度不符".into());
        }
        file.sync_all().map_err(|_| "无法保存解压文件")?;
    }
    let payload = destination.join(prefix);
    let executable = payload.join("Muse.exe");
    if !executable.is_file() {
        return Err("更新包缺少应用程序".into());
    }
    Ok(payload)
}
pub fn windows_app_file(path: &Path) -> bool {
    matches!(
        path.to_str(),
        Some(
            "Muse.exe"
                | "mihomo.exe"
                | "LICENSE"
                | "NOTICE"
                | "README.txt"
                | "THIRD_PARTY_NOTICES.md"
        )
    ) || path.starts_with("LICENSES")
        || path == Path::new("kernel")
        || [
            "README.md",
            "build.ps1",
            "mihomo-source.zip",
            "changes.patch",
        ]
        .iter()
        .any(|name| path == Path::new("kernel").join(name))
}
impl Source {
    pub fn prepare(
        &self,
        update: Update,
        cancel: &AtomicBool,
        mut progress: impl FnMut(u64),
    ) -> Result<Prepared> {
        let root = std::env::temp_dir().join(format!("muse-update-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).map_err(|_| "无法创建更新暂存目录")?;
        let mut prepared = Prepared {
            payload: root.join("pending"),
            root,
            update,
            keep: false,
        };
        let checksum = updater::parse_checksum(
            &self.text(&prepared.update.checksum_url, false, 4096)?,
            &prepared.update.name,
        )?;
        let response = self.request(&prepared.update.url, false)?;
        if let Some(length) = response
            .headers()
            .get("content-length")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            && length != prepared.update.size
        {
            return Err("更新包大小与版本信息不符".into());
        }
        let path = prepared.root.join("update.zip");
        copy_download(
            &mut response.into_body().into_reader(),
            &path,
            prepared.update.size,
            &checksum,
            cancel,
            &mut progress,
        )?;
        prepared.payload = extract_package(
            &path,
            &prepared.root.join("extracted"),
            &prepared.update,
            cancel,
        )?;
        Ok(prepared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_cancelled_and_corrupt_downloads_never_commit_a_package() {
        let root = std::env::temp_dir().join(format!("muse-update-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("update.zip");
        let cancel = AtomicBool::new(false);
        let bytes = b"synthetic zip bytes";
        assert!(
            copy_download(
                &mut &bytes[..],
                &path,
                bytes.len() as u64,
                &"a".repeat(64),
                &cancel,
                |_| {}
            )
            .is_err()
        );
        assert!(!path.exists());
        assert!(
            copy_download(
                &mut &bytes[..4],
                &path,
                bytes.len() as u64,
                &crate::updater::digest(bytes),
                &cancel,
                |_| {}
            )
            .is_err()
        );
        cancel.store(true, Ordering::Release);
        assert!(
            copy_download(
                &mut &bytes[..],
                &path,
                bytes.len() as u64,
                &crate::updater::digest(bytes),
                &cancel,
                |_| {}
            )
            .is_err()
        );
        cancel.store(false, Ordering::Release);
        copy_download(
            &mut &bytes[..],
            &path,
            bytes.len() as u64,
            &crate::updater::digest(bytes),
            &cancel,
            |_| {},
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!root.join("update.part").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn archive_paths_reject_traversal_absolute_links_and_windows_aliases() {
        for p in [
            "../bad",
            "/bad",
            "C:/bad",
            "ok/../bad",
            "ok\\bad",
            "ok/CON.txt",
            "ok/file.",
            "ok//bad",
            "ok/./bad",
        ] {
            assert!(safe_path(p).is_err(), "{p}");
        }
        assert!(safe_path("application/LICENSES/NOTICE").is_ok());
    }
    #[test]
    fn real_zip_extraction_rejects_links_traversal_bombs_and_unexpected_payloads() {
        let root = std::env::temp_dir().join(format!("muse-zip-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let update = Update {
            version: "99.9.9".into(),
            platform: "win-x64".into(),
            name: String::new(),
            url: String::new(),
            checksum_url: String::new(),
            size: 1,
            notes: String::new(),
        };
        for (index, name, link, bomb, valid) in [
            (
                0,
                "Muse-Desktop-99.9.9-win-x64/Muse.exe",
                false,
                false,
                true,
            ),
            (1, "../escape", false, false, false),
            (
                2,
                "Muse-Desktop-99.9.9-win-x64/Muse.exe",
                true,
                false,
                false,
            ),
            (
                3,
                "Muse-Desktop-99.9.9-win-x64/Muse.exe",
                false,
                true,
                false,
            ),
            (
                4,
                "Muse-Desktop-99.9.9-win-x64/workspace.json",
                false,
                false,
                false,
            ),
        ] {
            let path = root.join(format!("{index}.zip"));
            let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            if link {
                zip.add_symlink(name, "/outside", options).unwrap();
            } else {
                zip.start_file(name, options).unwrap();
                zip.write_all(&if bomb {
                    vec![0; 2 * 1024 * 1024]
                } else {
                    b"synthetic executable".to_vec()
                })
                .unwrap();
            }
            zip.finish().unwrap();
            let result = extract_package(
                &path,
                &root.join(format!("out-{index}")),
                &update,
                &AtomicBool::new(false),
            );
            assert_eq!(result.is_ok(), valid, "{index}");
        }
        assert!(!root.parent().unwrap().join("escape").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
