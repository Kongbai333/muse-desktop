//! Account forms commit name and protected routing together, never on typing.

use muse_core::AccountId;
#[cfg(test)]
use {crate::proxy_settings::ProxyDraft, muse_core::Workspace, std::path::Path};

pub struct SavedAccount {
    pub id: AccountId,
    pub browser_changed: bool,
}
#[cfg(test)]
pub fn save(
    root: &Path,
    workspace: &mut Workspace,
    owner: Option<AccountId>,
    name: &str,
    proxy: &ProxyDraft,
) -> Result<SavedAccount, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("账号名称需为 1–40 个字符".into());
    }
    if proxy.enabled {
        proxy.proxy_url()?;
    }
    let previous = owner.and_then(|id| crate::proxy_settings::load(root, workspace, id).ok());
    let browser_changed = match previous {
        Some(previous) if !previous.enabled && !proxy.enabled => false,
        Some(previous) => previous != *proxy,
        None => true,
    };
    let mut candidate = workspace.clone();
    let id = if let Some(id) = owner {
        candidate.rename_account(id, name)?;
        id
    } else {
        candidate.add_account(name)
    };
    if browser_changed && (owner.is_some() || proxy.enabled) {
        crate::proxy_settings::save(root, &mut candidate, id, proxy)?;
    } else {
        candidate.save(&root.join("workspace.json"))?;
    }
    *workspace = candidate;
    Ok(SavedAccount {
        id,
        browser_changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (std::path::PathBuf, Workspace, AccountId) {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/account-form-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        let mut state = Workspace::new();
        let id = state.add_account("原账号");
        state.save(&root.join("workspace.json")).unwrap();
        (root, state, id)
    }
    #[test]
    fn name_only_edit_preserves_browser_identity_selection_and_profile() {
        let (root, mut state, id) = fixture();
        let before = state.active_browser_identity().unwrap();
        let other = state.add_account("其他账号");
        let result = save(
            &root,
            &mut state,
            Some(id),
            "  新名称  ",
            &ProxyDraft::default(),
        )
        .unwrap();
        assert!(!result.browser_changed);
        assert!(state.contains_identity(before));
        assert_eq!(state.active_account_id(), Some(other));
        assert_eq!(state.account(id).unwrap().name, "新名称");
    }
    #[test]
    fn name_only_edit_of_a_proxied_account_does_not_rotate_credentials_or_reload() {
        let (root, mut state, id) = fixture();
        let proxy = ProxyDraft::from_url("http://127.0.0.1:8081", true).unwrap();
        crate::proxy_settings::save(&root, &mut state, id, &proxy).unwrap();
        let identity = state.active_browser_identity().unwrap();
        let revision = state.account(id).unwrap().proxy_revision;
        let result = save(&root, &mut state, Some(id), "只改名", &proxy).unwrap();
        assert!(!result.browser_changed);
        assert!(state.contains_identity(identity));
        assert_eq!(state.account(id).unwrap().proxy_revision, revision);
    }
    #[test]
    fn new_account_is_created_only_after_a_valid_atomic_save() {
        let (root, mut state, _) = fixture();
        let original = state.to_json().unwrap();
        let invalid = ProxyDraft {
            enabled: true,
            host: "127.0.0.1".into(),
            port: "0".into(),
            ..Default::default()
        };
        assert!(save(&root, &mut state, None, "新账号", &invalid).is_err());
        assert_eq!(state.to_json().unwrap(), original);
        assert!(save(&root, &mut state, None, " ", &ProxyDraft::default()).is_err());
        let saved = save(&root, &mut state, None, "新账号", &ProxyDraft::default()).unwrap();
        assert_eq!(state.accounts.len(), 2);
        assert_eq!(state.active_account_id(), Some(saved.id));
        let restored = Workspace::load(&root.join("workspace.json")).unwrap();
        assert_eq!(restored.accounts.len(), 2);
    }
    #[test]
    fn proxy_change_invalidates_only_edited_account_and_failed_commit_keeps_original() {
        let (root, mut state, id) = fixture();
        let original_identity = state.active_browser_identity().unwrap();
        let other = state.add_account("其他账号");
        let other_identity = state.active_browser_identity().unwrap();
        let draft = ProxyDraft::from_url("http://127.0.0.1:8081", true).unwrap();
        let saved = save(&root, &mut state, Some(id), "新名称", &draft).unwrap();
        assert!(saved.browser_changed);
        assert!(!state.contains_identity(original_identity));
        assert!(state.contains_identity(other_identity));
        assert_eq!(state.active_account_id(), Some(other));
        let before = state.to_json().unwrap();
        std::fs::remove_file(root.join("workspace.json")).unwrap();
        std::fs::create_dir(root.join("workspace.json")).unwrap();
        assert!(
            save(
                &root,
                &mut state,
                Some(id),
                "失败名称",
                &ProxyDraft::default()
            )
            .is_err()
        );
        assert_eq!(state.to_json().unwrap(), before);
    }
}
