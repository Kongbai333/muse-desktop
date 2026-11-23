//! Public, application-global update policy. No Muse session is used here.

use semver::Version;
use serde::Deserialize;

pub const LATEST: &str = "https://api.github.com/repos/Kongbai333/muse-desktop/releases/latest";
pub const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
pub type Result<T> = std::result::Result<T, String>;

pub fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "win-x64"
    } else if cfg!(target_arch = "aarch64") {
        "macos-arm64"
    } else {
        "macos-x64"
    }
}

#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    pub notes: String,
    pub name: String,
    pub url: String,
    pub checksum_url: String,
    pub size: u64,
    pub platform: String,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: Option<std::string::String>,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
}

pub fn select_release(json: &str, current: &str, platform: &str) -> Result<Option<Update>> {
    if json.len() > 1024 * 1024 {
        return Err("更新信息过大".into());
    }
    let release: Release = serde_json::from_str(json).map_err(|_| "更新信息格式无效")?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let tag = release
        .tag_name
        .strip_prefix('v')
        .ok_or("更新版本格式无效")?;
    let version = Version::parse(tag).map_err(|_| "更新版本格式无效")?;
    let current = Version::parse(current).map_err(|_| "当前版本格式无效")?;
    if !version.pre.is_empty() || !version.build.is_empty() || version <= current {
        return Ok(None);
    }
    if !matches!(platform, "win-x64" | "macos-arm64" | "macos-x64") {
        return Err("暂不支持此架构更新".into());
    }
    let suffix = if platform == "win-x64" {
        "zip"
    } else {
        "app.zip"
    };
    let name = format!("Muse-Desktop-{version}-{platform}.{suffix}");
    let asset = |name: &str| -> Result<&Asset> {
        let mut matches = release.assets.iter().filter(|a| a.name == name);
        let a = matches
            .next()
            .ok_or("此版本尚未提供本机完整更新包及校验文件，请稍后重试")?;
        if matches.next().is_some() || a.size == 0 || a.size > MAX_PACKAGE {
            return Err("更新资产无效".into());
        }
        let expected = format!(
            "https://github.com/Kongbai333/muse-desktop/releases/download/v{version}/{name}"
        );
        if a.browser_download_url != expected {
            return Err("更新资产不属于官方版本".into());
        }
        Ok(a)
    };
    let package = asset(&name)?;
    let checksum = asset(&format!("{name}.sha256"))?;
    if checksum.size > 4096 {
        return Err("校验文件过大".into());
    }
    Ok(Some(Update {
        version: version.to_string(),
        notes: release
            .body
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .take(12_000)
            .collect(),
        name: name.clone(),
        url: package.browser_download_url.clone(),
        checksum_url: checksum.browser_download_url.clone(),
        size: package.size,
        platform: platform.into(),
    }))
}

/// Redirects are checked before every network request. Signed CDN query
/// parameters never enter logs or persisted update metadata.
pub fn allowed_url(input: &str, api: bool) -> bool {
    let Ok(u) = url::Url::parse(input) else {
        return false;
    };
    if u.scheme() != "https"
        || u.port_or_known_default() != Some(443)
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
    {
        return false;
    }
    if api {
        return u.as_str() == LATEST;
    }
    match u.host_str() {
        Some("github.com") => u
            .path()
            .starts_with("/Kongbai333/muse-desktop/releases/download/"),
        Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com") => true,
        _ => false,
    }
}
pub fn parse_checksum(text: &str, name: &str) -> Result<String> {
    let lines = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.len() != 1 {
        return Err("校验文件无效".into());
    }
    let fields = lines[0].split_whitespace().collect::<Vec<_>>();
    if fields.len() != 2
        || fields[1].trim_start_matches('*') != name
        || fields[0].len() != 64
        || !fields[0].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("校验文件不匹配".into());
    }
    Ok(fields[0].to_ascii_lowercase())
}

#[cfg(test)]
pub fn digest(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(version: &str) -> String {
        serde_json::json!({"tag_name":format!("v{version}"),"draft":false,"prerelease":false,"body":"notes",
            "assets":[{"name":format!("Muse-Desktop-{version}-win-x64.zip"),"size":10,"browser_download_url":format!("https://github.com/Kongbai333/muse-desktop/releases/download/v{version}/Muse-Desktop-{version}-win-x64.zip")},
            {"name":format!("Muse-Desktop-{version}-win-x64.zip.sha256"),"size":110,"browser_download_url":format!("https://github.com/Kongbai333/muse-desktop/releases/download/v{version}/Muse-Desktop-{version}-win-x64.zip.sha256")}]}).to_string()
    }
    #[test]
    fn stable_release_requires_newer_version_and_exact_platform_assets() {
        assert!(
            select_release(&release("0.2.0"), "0.1.2", "win-x64")
                .unwrap()
                .is_some()
        );
        for version in ["0.1.1", "0.1.2", "0.2.0-beta.1"] {
            assert!(
                select_release(&release(version), "0.1.2", "win-x64")
                    .unwrap()
                    .is_none()
            );
        }
        assert!(select_release(&release("0.2.0"), "0.1.2", "macos-arm64").is_err());
        let mut value: serde_json::Value = serde_json::from_str(&release("0.2.0")).unwrap();
        value["draft"] = true.into();
        assert!(
            select_release(&value.to_string(), "0.1.2", "win-x64")
                .unwrap()
                .is_none()
        );
        value["draft"] = false.into();
        value["prerelease"] = true.into();
        assert!(
            select_release(&value.to_string(), "0.1.2", "win-x64")
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn url_policy_rejects_credentialed_lookalike_or_off_repository_assets() {
        for url in [
            "http://github.com/Kongbai333/muse-desktop/releases/download/v1/x",
            "https://github.com.evil.test/x",
            "https://user@github.com/Kongbai333/muse-desktop/x",
            "https://github.com/other/repo/releases/download/v1/x",
            "https://release-assets.githubusercontent.com:444/x",
        ] {
            assert!(!allowed_url(url, false), "{url}");
        }
        assert!(allowed_url(
            "https://release-assets.githubusercontent.com/github-production-release-asset/x?sig=synthetic",
            false
        ));
        assert!(!allowed_url("https://api.github.com/other", true));
    }
    #[test]
    fn checksums_are_exact_and_bound_to_one_filename() {
        let h = "a".repeat(64);
        assert_eq!(
            parse_checksum(&format!("{h}  app.zip\n"), "app.zip").unwrap(),
            h
        );
        for text in [
            format!("{h}  other.zip"),
            format!("{h}  app.zip\n{h}  app.zip"),
            "x".repeat(64),
        ] {
            assert!(parse_checksum(&text, "app.zip").is_err());
        }
    }
}
