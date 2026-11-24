use crate::{proxy_settings::ProxyDraft, secrets::ProtectedStore};
use muse_core::{AccountId, AccountNetwork, ProxyProfile, Workspace};
use std::path::Path;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteChoice {
    System,
    Legacy,
    Pool(Uuid),
}

impl From<AccountNetwork> for RouteChoice {
    fn from(network: AccountNetwork) -> Self {
        match network {
            AccountNetwork::Direct => Self::System,
            AccountNetwork::HttpProxy { .. } => Self::Legacy,
            AccountNetwork::PoolProxy { proxy_id, .. } => Self::Pool(proxy_id),
        }
    }
}

pub fn migrate(root: &Path, workspace: &mut Workspace) -> Result<(), String> {
    let mut candidate = workspace.clone();
    let protected = ProtectedStore::new(root);
    let mut changed = false;
    for account in &mut candidate.accounts {
        let force_proxy = match account.network {
            AccountNetwork::HttpProxy { force_proxy } => Some(force_proxy),
            AccountNetwork::Direct if !account.legacy_proxy_migrated => None,
            _ => continue,
        };
        let Ok(value) = protected.load_selected(account.id, account.proxy_revision) else {
            continue;
        };
        let draft = ProxyDraft::from_url(&value, force_proxy.unwrap_or(true))?;
        candidate.proxy_profiles.push(ProxyProfile {
            id: account.id,
            name: format!("{} · 原代理", account.name),
            protocol: draft.protocol.scheme().into(),
            revision: account.proxy_revision.take(),
        });
        if let Some(force_proxy) = force_proxy {
            account.network = AccountNetwork::PoolProxy {
                proxy_id: account.id,
                force_proxy,
            };
        }
        account.legacy_proxy_migrated = true;
        changed = true;
    }
    if changed {
        commit(root, workspace, candidate)?;
    }
    Ok(())
}

pub fn load(root: &Path, profile: &ProxyProfile) -> Result<ProxyDraft, String> {
    let value = ProtectedStore::new(root).load_selected(profile.id, profile.revision)?;
    ProxyDraft::from_url(&value, true)
}

pub fn references(workspace: &Workspace, id: Uuid) -> Vec<AccountId> {
    workspace
        .accounts
        .iter()
        .filter(|account| account.network.proxy_id() == Some(id))
        .map(|account| account.id)
        .collect()
}

pub fn save(
    root: &Path,
    workspace: &mut Workspace,
    existing: Option<Uuid>,
    name: &str,
    draft: &ProxyDraft,
) -> Result<Vec<AccountId>, String> {
    let preserving_name = existing
        .and_then(|id| workspace.proxy_profile(id))
        .is_some_and(|p| p.name == name);
    let name = if preserving_name { name } else { name.trim() };
    if name.trim().is_empty()
        || name.chars().count() > if preserving_name { 128 } else { 48 }
        || name.chars().any(char::is_control)
    {
        return Err("代理名称需为 1–48 个字符".into());
    }
    let value = draft.proxy_url()?;
    let (id, old_revision, replace) = if let Some(id) = existing {
        let old = workspace.proxy_profile(id).ok_or("代理已不存在")?;
        let unchanged = load(root, old).ok().is_some_and(|mut applied| {
            applied.force_proxy = draft.force_proxy;
            applied == *draft
        });
        (id, old.revision, !unchanged)
    } else {
        (Uuid::new_v4(), None, true)
    };
    let revision = if replace {
        Some(Uuid::new_v4())
    } else {
        old_revision
    };
    let mut candidate = workspace.clone();
    let profile = ProxyProfile {
        id,
        name: name.into(),
        revision,
        protocol: draft.protocol.scheme().into(),
    };
    if let Some(entry) = candidate
        .proxy_profiles
        .iter_mut()
        .find(|profile| profile.id == id)
    {
        *entry = profile;
    } else {
        candidate.proxy_profiles.push(profile);
    }
    let affected = if replace {
        references(&candidate, id)
    } else {
        Vec::new()
    };
    for account in &affected {
        let network = candidate.account(*account).ok_or("账号不存在")?.network;
        candidate.update_network(*account, network, None)?;
    }
    let protected = ProtectedStore::new(root);
    if let Some(revision) = revision.filter(|_| replace) {
        protected.save_revision(id, revision, &value)?;
    }
    if let Err(error) = commit(root, workspace, candidate) {
        if let Some(revision) = revision.filter(|_| replace) {
            protected.remove_revision(id, revision);
        }
        return Err(error);
    }
    if replace && let Some(old) = old_revision {
        protected.remove_revision(id, old);
    }
    Ok(affected)
}

pub fn remove(root: &Path, workspace: &mut Workspace, id: Uuid) -> Result<(), String> {
    if !references(workspace, id).is_empty() {
        return Err("此代理仍有账号使用，请先切换这些账号的网络连接".into());
    }
    let revision = workspace.proxy_profile(id).ok_or("代理已不存在")?.revision;
    let mut candidate = workspace.clone();
    candidate.proxy_profiles.retain(|profile| profile.id != id);
    commit(root, workspace, candidate)?;
    if let Some(revision) = revision {
        ProtectedStore::new(root).remove_revision(id, revision);
    }
    Ok(())
}

pub fn prepare_account(
    root: &Path,
    workspace: &Workspace,
    owner: Option<AccountId>,
    name: &str,
    route: RouteChoice,
    force_proxy: bool,
) -> Result<(Workspace, crate::account_settings::SavedAccount), String> {
    let preserving_name = owner
        .and_then(|id| workspace.account(id))
        .is_some_and(|a| a.name == name);
    let name = if preserving_name { name } else { name.trim() };
    if name.trim().is_empty() || name.chars().count() > if preserving_name { 128 } else { 40 } {
        return Err("账号名称需为 1–40 个字符".into());
    }
    let network = match route {
        RouteChoice::Pool(proxy_id) => {
            let profile = workspace
                .proxy_profile(proxy_id)
                .ok_or("选择的代理已不存在")?;
            if owner.is_none_or(|id| {
                workspace
                    .account(id)
                    .is_none_or(|account| account.network.proxy_id() != Some(proxy_id))
            }) {
                load(root, profile)?;
            }
            AccountNetwork::PoolProxy {
                proxy_id,
                force_proxy,
            }
        }
        RouteChoice::System => AccountNetwork::Direct,
        RouteChoice::Legacy => owner
            .and_then(|id| workspace.account(id))
            .map(|account| account.network)
            .filter(|network| matches!(network, AccountNetwork::HttpProxy { .. }))
            .ok_or("原代理配置已改变，请重新打开账号设置")?,
    };
    let mut candidate = workspace.clone();
    let id = match owner {
        Some(id) => {
            if !preserving_name {
                candidate.rename_account(id, name)?;
            }
            id
        }
        None => candidate.add_account(name),
    };
    let browser_changed = candidate.account(id).ok_or("账号不存在")?.network != network;
    if browser_changed {
        candidate.update_network(id, network, None)?;
    }
    Ok((
        candidate,
        crate::account_settings::SavedAccount {
            id,
            browser_changed,
        },
    ))
}

#[cfg(test)]
pub fn save_account(
    root: &Path,
    workspace: &mut Workspace,
    owner: Option<AccountId>,
    name: &str,
    route: RouteChoice,
    force_proxy: bool,
) -> Result<crate::account_settings::SavedAccount, String> {
    let (candidate, saved) = prepare_account(root, workspace, owner, name, route, force_proxy)?;
    commit(root, workspace, candidate)?;
    Ok(saved)
}

fn commit(root: &Path, workspace: &mut Workspace, candidate: Workspace) -> Result<(), String> {
    candidate.save(&root.join("workspace.json"))?;
    *workspace = candidate;
    Ok(())
}

pub fn save_account_with_fingerprint(
    root: &Path,
    workspace: &mut Workspace,
    owner: Option<AccountId>,
    name: &str,
    route: RouteChoice,
    force_proxy: bool,
    mode: muse_core::FingerprintMode,
) -> Result<crate::account_settings::SavedAccount, String> {
    let (mut candidate, mut saved) =
        prepare_account(root, workspace, owner, name, route, force_proxy)?;
    saved.browser_changed |= candidate.set_fingerprint_mode(saved.id, mode)?;
    commit(root, workspace, candidate)?;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_save_rebuilds_only_on_change_and_keeps_the_profile() {
        let (root, mut workspace) = fixture();
        let id = workspace.add_account("before");
        let old = workspace.active_browser_identity().unwrap();
        let profile = workspace
            .account(id)
            .unwrap()
            .profile_dir(&root.join("profiles"));
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("synthetic-login-canary"), b"keep").unwrap();
        let saved = save_account_with_fingerprint(
            &root,
            &mut workspace,
            Some(id),
            "after",
            RouteChoice::System,
            true,
            muse_core::FingerprintMode::Original,
        )
        .unwrap();
        assert!(saved.browser_changed);
        assert!(!workspace.contains_identity(old));
        let loaded = Workspace::load(&root.join("workspace.json")).unwrap();
        assert_eq!(
            loaded.account(id).unwrap().fingerprint,
            muse_core::FingerprintMode::Original
        );
        assert_eq!(
            std::fs::read(profile.join("synthetic-login-canary")).unwrap(),
            b"keep"
        );
        let stable = workspace.active_browser_identity().unwrap();
        let renamed = save_account_with_fingerprint(
            &root,
            &mut workspace,
            Some(id),
            "rename only",
            RouteChoice::System,
            true,
            muse_core::FingerprintMode::Original,
        )
        .unwrap();
        assert!(!renamed.browser_changed);
        assert!(workspace.contains_identity(stable));
    }

    #[test]
    fn fingerprint_save_failure_does_not_publish_the_name_or_mode() {
        let (root, mut workspace) = fixture();
        let id = workspace.add_account("before");
        let before = workspace.to_json().unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir(root.join("workspace.json")).unwrap();
        assert!(
            save_account_with_fingerprint(
                &root,
                &mut workspace,
                Some(id),
                "after",
                RouteChoice::System,
                false,
                muse_core::FingerprintMode::Original
            )
            .is_err()
        );
        assert_eq!(workspace.to_json().unwrap(), before);
    }

    #[test]
    fn preparing_an_import_keeps_existing_accounts_and_the_saved_workspace_unchanged() {
        let (root, mut workspace) = fixture();
        let first = workspace.add_account("existing");
        workspace.save(&root.join("workspace.json")).unwrap();
        let before = std::fs::read(root.join("workspace.json")).unwrap();
        let (candidate, prepared) = prepare_account(
            &root,
            &workspace,
            None,
            "imported",
            RouteChoice::System,
            false,
        )
        .unwrap();
        assert_eq!(workspace.accounts.len(), 1);
        assert_eq!(workspace.active_account_id(), Some(first));
        assert_eq!(candidate.accounts.len(), 2);
        assert_ne!(prepared.id, first);
        assert_eq!(candidate.active_account_id(), Some(prepared.id));
        assert_eq!(std::fs::read(root.join("workspace.json")).unwrap(), before);
        assert!(
            !candidate
                .account(prepared.id)
                .unwrap()
                .profile_dir(&root.join("profiles"))
                .exists()
        );
    }

    fn fixture() -> (std::path::PathBuf, Workspace) {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/proxy-pool-fixtures")
            .join(Uuid::new_v4().to_string());
        (root, Workspace::new())
    }

    #[test]
    fn migrates_both_legacy_credential_formats_without_restarting_accounts_or_enabling_disabled_proxies()
     {
        let (root, mut workspace) = fixture();
        let older = workspace.add_account("旧版账号");
        let newer = workspace.add_account("原桌面账号");
        let disabled = workspace.add_account("系统网络账号");
        let protected = ProtectedStore::new(&root);
        let revision = Uuid::new_v4();
        let upstream = "socks://fixture-token:@proxy.example:7153";
        protected.save_proxy(older, upstream).unwrap();
        protected.save_revision(newer, revision, upstream).unwrap();
        protected.save_proxy(disabled, upstream).unwrap();
        workspace
            .update_network(
                older,
                AccountNetwork::HttpProxy { force_proxy: false },
                None,
            )
            .unwrap();
        workspace
            .update_network(
                newer,
                AccountNetwork::HttpProxy { force_proxy: true },
                Some(revision),
            )
            .unwrap();
        workspace.select_account(newer).unwrap();
        let active = workspace.active_browser_identity().unwrap();
        migrate(&root, &mut workspace).unwrap();
        assert_eq!(workspace.active_browser_identity(), Some(active));
        workspace = Workspace::load(&root.join("workspace.json")).unwrap();
        migrate(&root, &mut workspace).unwrap();
        assert_eq!(workspace.proxy_profiles.len(), 3);
        let restored = workspace.active_browser_identity().unwrap();
        assert_eq!(
            (restored.account_id, restored.tab_id),
            (active.account_id, active.tab_id)
        );
        assert_eq!(
            workspace.account(disabled).unwrap().network,
            AccountNetwork::Direct
        );
        for (id, old_revision, force) in [(older, None, false), (newer, Some(revision), true)] {
            let account = workspace.account(id).unwrap();
            assert_eq!(
                account.network,
                AccountNetwork::PoolProxy {
                    proxy_id: id,
                    force_proxy: force
                }
            );
            assert_eq!(
                workspace.proxy_credentials(account).unwrap(),
                (id, old_revision)
            );
            assert_eq!(
                load(&root, workspace.proxy_profile(id).unwrap())
                    .unwrap()
                    .user,
                "fixture-token"
            );
        }
        let json = workspace.to_json().unwrap();
        assert!(!json.contains("fixture-token"));
        assert!(!json.contains("proxy.example"));
        remove(&root, &mut workspace, disabled).unwrap();
        migrate(&root, &mut workspace).unwrap();
        assert!(workspace.proxy_profile(disabled).is_none());
    }

    #[test]
    fn a_shared_proxy_reloads_only_its_accounts_and_cannot_be_deleted_until_unlinked() {
        let (root, mut workspace) = fixture();
        let first = workspace.add_account("First");
        let second = workspace.add_account("Second");
        let unaffected = workspace.add_account("Other");
        let unrelated_browser = workspace.active_browser_identity().unwrap();
        let mut draft = ProxyDraft::from_url("https://proxy.example:443", true).unwrap();
        assert!(
            save(&root, &mut workspace, None, "Shared", &draft)
                .unwrap()
                .is_empty()
        );
        let id = workspace.proxy_profiles[0].id;
        for account in [first, second] {
            save_account(
                &root,
                &mut workspace,
                Some(account),
                "Bound",
                RouteChoice::Pool(id),
                true,
            )
            .unwrap();
        }
        assert!(remove(&root, &mut workspace, id).is_err());
        workspace.select_account(second).unwrap();
        let old_browser = workspace.active_browser_identity().unwrap();
        assert!(
            save(&root, &mut workspace, Some(id), "Renamed", &draft)
                .unwrap()
                .is_empty()
        );
        assert!(workspace.contains_identity(old_browser));
        draft.port = "8443".into();
        assert_eq!(
            save(&root, &mut workspace, Some(id), "Renamed", &draft).unwrap(),
            [first, second]
        );
        assert!(!workspace.contains_identity(old_browser));
        assert!(workspace.contains_identity(unrelated_browser));
        assert_eq!(
            workspace.account(unaffected).unwrap().network,
            AccountNetwork::Direct
        );
        for account in [first, second] {
            save_account(
                &root,
                &mut workspace,
                Some(account),
                "Unlinked",
                RouteChoice::System,
                true,
            )
            .unwrap();
        }
        remove(&root, &mut workspace, id).unwrap();
        assert!(
            Workspace::load(&root.join("workspace.json"))
                .unwrap()
                .proxy_profiles
                .is_empty()
        );
    }

    #[test]
    fn failed_workspace_write_leaves_the_active_shared_proxy_and_credentials_intact() {
        let (root, mut workspace) = fixture();
        let first = workspace.add_account("First");
        let mut draft = ProxyDraft::from_url("socks5://proxy.example:1080", true).unwrap();
        save(&root, &mut workspace, None, "Shared", &draft).unwrap();
        let id = workspace.proxy_profiles[0].id;
        save_account(
            &root,
            &mut workspace,
            Some(first),
            "First",
            RouteChoice::Pool(id),
            true,
        )
        .unwrap();
        let previous = workspace.to_json().unwrap();
        let path = root.join("workspace.json");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        draft.port = "1081".into();
        assert!(save(&root, &mut workspace, Some(id), "Changed", &draft).is_err());
        assert_eq!(workspace.to_json().unwrap(), previous);
        assert_eq!(
            load(&root, workspace.proxy_profile(id).unwrap())
                .unwrap()
                .port,
            "1080"
        );
        assert_eq!(std::fs::read_dir(root.join("secrets")).unwrap().count(), 1);
    }
}
