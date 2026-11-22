use crate::secrets::ProtectedStore;
use muse_core::{AccountId, AccountNetwork, Workspace};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct ImportResult {
    pub account_id: AccountId,
    pub already_present: bool,
}
pub fn import_legacy(
    root: &Path,
    legacy_config: &Path,
    profiles: &Path,
    requested: Option<u64>,
    snapshot: Option<&Path>,
) -> Result<ImportResult, String> {
    #[derive(Deserialize)]
    struct LegacyFile {
        list: String,
        #[serde(rename = "currentId")]
        current_id: Option<u64>,
    }
    #[derive(Deserialize)]
    struct LegacyAccount {
        id: u64,
        name: String,
        #[serde(default)]
        proxy: String,
        #[serde(default, rename = "forceProxy")]
        force_proxy: bool,
    }
    let raw = zeroize::Zeroizing::new(
        fs::read_to_string(legacy_config).map_err(|_| "无法读取旧版账号配置")?,
    );
    let legacy: LegacyFile = serde_json::from_str(&raw).map_err(|_| "旧版账号配置格式无效")?;
    let accounts: Vec<LegacyAccount> =
        serde_json::from_str(&legacy.list).map_err(|_| "旧版账号列表格式无效")?;
    let legacy_id = requested
        .or(legacy.current_id)
        .ok_or("旧版未选中账号，请指定账号 ID")?;
    let account = accounts
        .iter()
        .find(|a| a.id == legacy_id)
        .ok_or("未找到指定的旧版账号")?;
    let mut workspace = Workspace::load(&root.join("workspace.json"))?;
    if let Some(existing) = workspace
        .accounts
        .iter()
        .find(|a| a.legacy_id == Some(legacy_id))
        .map(|a| a.id)
    {
        workspace.select_account(existing)?;
        workspace.save(&root.join("workspace.json"))?;
        return Ok(ImportResult {
            account_id: existing,
            already_present: true,
        });
    }
    let network = if account.proxy.trim().is_empty() {
        if account.force_proxy {
            return Err("旧账号要求强制代理，但没有代理配置；导入已停止".into());
        }
        AccountNetwork::Direct
    } else {
        muse_proxy::Upstream::parse(&account.proxy).map_err(|e| e.to_string())?;
        AccountNetwork::HttpProxy {
            force_proxy: account.force_proxy,
        }
    };
    let selected_source = snapshot
        .map(Path::to_path_buf)
        .unwrap_or_else(|| profiles.join(format!("acct_{legacy_id}")));
    let source = selected_source.join("EBWebView");
    if !source.join("Default/Network/Cookies").is_file() {
        return Err("旧版浏览器数据目录不存在或不完整".into());
    }
    require_unlocked(&source.join("Default/Network/Cookies"))?;
    if source.join("Local State").exists() {
        require_unlocked(&source.join("Local State"))?;
    }
    let name = if account.name.trim().is_empty() {
        "导入的账号"
    } else {
        account.name.trim()
    };
    let id = workspace.add_account(name);
    workspace.register_legacy(id, legacy_id, network)?;
    fs::create_dir_all(root.join("profiles")).map_err(|_| "无法创建新版浏览器目录")?;
    let parent = fs::canonicalize(root.join("profiles")).map_err(|_| "新版浏览器目录无效")?;
    let temporary = parent.join(format!(".import-{id}"));
    let destination = parent.join(id.to_string());
    if !temporary.starts_with(&parent)
        || !destination.starts_with(&parent)
        || temporary.exists()
        || destination.exists()
    {
        return Err("导入目标目录已存在，未覆盖任何数据".into());
    }
    fs::create_dir(&temporary).map_err(|_| "无法创建导入暂存目录")?;
    if let Err(error) = copy_tree(&source, &temporary.join("EBWebView")) {
        cleanup_owned(&temporary, &parent);
        return Err(error);
    }
    let protected = ProtectedStore::new(root);
    if matches!(network, AccountNetwork::HttpProxy { .. })
        && let Err(error) = protected.save_proxy(id, &account.proxy)
    {
        cleanup_owned(&temporary, &parent);
        return Err(error);
    }
    let result = (|| -> Result<(), String> {
        let old_metadata = root.join("workspace.json");
        if old_metadata.is_file() {
            fs::create_dir_all(root.join("backups")).map_err(|_| "无法创建导入前备份")?;
            fs::copy(
                &old_metadata,
                root.join("backups")
                    .join(format!("workspace-before-{id}.json")),
            )
            .map_err(|_| "无法备份当前账号配置")?;
        }
        // Both absolute paths are fresh children of the verified profile parent.
        fs::rename(&temporary, &destination).map_err(|_| "无法完成浏览器目录迁移")?;
        workspace.save(&old_metadata)
    })();
    if let Err(error) = result {
        cleanup_owned(&temporary, &parent);
        cleanup_owned(&destination, &parent);
        let _ = fs::remove_file(root.join("secrets").join(format!("proxy-{id}.bin")));
        return Err(error);
    }
    Ok(ImportResult {
        account_id: id,
        already_present: false,
    })
}

pub fn default_paths() -> Result<(PathBuf, PathBuf), String> {
    let roaming = std::env::var_os("APPDATA").ok_or("无法定位旧版账号目录")?;
    let local = std::env::var_os("LOCALAPPDATA").ok_or("无法定位旧版浏览器目录")?;
    Ok((
        PathBuf::from(roaming).join("MuseLauncher/accounts.json"),
        PathBuf::from(local).join("MuseLauncher/Profiles"),
    ))
}

fn require_unlocked(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::OpenOptionsExt;
    let _file = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .map_err(|_| "旧版浏览器数据正在使用，请先关闭旧版程序后再导入")?;
    Ok(())
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    let metadata = fs::symlink_metadata(source).map_err(|_| "无法读取旧版浏览器数据")?;
    if metadata.file_attributes() & 0x400 != 0 {
        return Err("浏览器目录包含链接或重解析点，导入已停止".into());
    }
    if metadata.is_dir() {
        fs::create_dir(target).map_err(|_| "无法创建浏览器数据副本")?;
        for entry in fs::read_dir(source).map_err(|_| "无法遍历旧版浏览器目录")? {
            let entry = entry.map_err(|_| "无法读取浏览器目录项")?;
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        fs::copy(source, target).map_err(|_| "浏览器数据复制失败，请关闭使用该环境的程序")?;
        if fs::metadata(target).map_err(|_| "副本校验失败")?.len() != metadata.len() {
            return Err("副本大小校验失败".into());
        }
    } else {
        return Err("浏览器目录包含不支持的数据项".into());
    }
    Ok(())
}

fn cleanup_owned(path: &Path, parent: &Path) {
    if let Ok(resolved) = fs::canonicalize(path)
        && resolved.starts_with(parent)
        && resolved != parent
    {
        let _ = fs::remove_dir_all(resolved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn importing_copies_the_profile_preserves_routing_and_does_not_duplicate_accounts() {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/import-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        let source = base.join("old-profiles/acct_1/EBWebView/Default/Network");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("Cookies"), b"fixture-cookie-db").unwrap();
        let config = base.join("old.json");
        let value = serde_json::json!({"currentId":1,"list":serde_json::json!([{"id":1,"name":"原账号","proxy":"http://user:fixture@127.0.0.1:8080","forceProxy":true}]).to_string()});
        std::fs::write(&config, value.to_string()).unwrap();
        let root = base.join("new");
        let result = import_legacy(&root, &config, &base.join("old-profiles"), None, None).unwrap();
        let workspace = Workspace::load(&root.join("workspace.json")).unwrap();
        let account = workspace.account(result.account_id).unwrap();
        assert_eq!(account.name, "原账号");
        assert_eq!(
            account.network,
            muse_core::AccountNetwork::HttpProxy { force_proxy: true }
        );
        assert_eq!(account.legacy_id, Some(1));
        assert_eq!(
            std::fs::read(
                account
                    .profile_dir(&root.join("profiles"))
                    .join("EBWebView/Default/Network/Cookies")
            )
            .unwrap(),
            b"fixture-cookie-db"
        );
        assert_eq!(
            std::fs::read(source.join("Cookies")).unwrap(),
            b"fixture-cookie-db"
        );
        let json = std::fs::read_to_string(root.join("workspace.json")).unwrap();
        assert!(!json.contains("http://user:"));
        assert!(!json.contains("127.0.0.1:8080"));
        let second = import_legacy(&root, &config, &base.join("old-profiles"), None, None).unwrap();
        assert!(second.already_present);
        assert_eq!(second.account_id, result.account_id);
        assert_eq!(
            Workspace::load(&root.join("workspace.json"))
                .unwrap()
                .accounts
                .len(),
            1
        );
    }
}
