use super::*;
fn payload() -> crate::account_transfer::Payload {
    crate::account_transfer::decode_payload(include_bytes!(
        "../../../checks/fixtures/account-transfer-v1/payload.json"
    ))
    .unwrap()
}
fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/transfer-plan-tests")
        .join(uuid::Uuid::new_v4().to_string())
}
fn shared_payload() -> crate::account_transfer::Payload {
    let mut p = payload();
    let mut account = p.accounts[0].clone();
    account.account_ref = "second".into();
    p.accounts.push(account);
    p
}
#[test]
fn remaps_shared_refs_once_without_changing_existing_or_collapsing_equal_endpoints() {
    let mut p = shared_payload();
    let mut other = p.proxies[0].clone();
    other.proxy_ref = "distinct".into();
    p.proxies.push(other);
    let mut third = p.accounts[0].clone();
    third.account_ref = "third".into();
    third.network.proxy_ref = Some("distinct".into());
    p.accounts.push(third);
    let plan = ImportPlan::prepare(p, TargetPlatform::Windows, 1_800_000_000).unwrap();
    let mut existing = muse_core::Workspace::new();
    let original = existing.add_account("Original");
    let before = existing.to_json().unwrap();
    let merged = plan.merge_into(&existing).unwrap();
    assert_eq!(existing.to_json().unwrap(), before);
    assert_eq!(merged.accounts.len(), 4);
    assert_eq!(merged.proxy_profiles.len(), 2);
    assert_eq!(
        merged.accounts[1].network.proxy_id(),
        merged.accounts[2].network.proxy_id()
    );
    assert_ne!(
        merged.accounts[1].network.proxy_id(),
        merged.accounts[3].network.proxy_id()
    );
    assert_ne!(merged.accounts[1].id, original);
    assert!(
        plan.accounts
            .iter()
            .all(|a| a.legacy_id.is_none() && a.proxy_revision.is_none())
    );
    assert!(!merged.to_json().unwrap().contains("SYNTHETIC_"));
    let second_plan =
        ImportPlan::prepare(payload(), TargetPlatform::Windows, 1_800_000_000).unwrap();
    assert_ne!(plan.accounts[0].id, second_plan.accounts[0].id);
}
#[test]
fn merges_new_objects_into_latest_workspace_and_commit_requires_complete_staging() {
    let mut plan =
        ImportPlan::prepare(shared_payload(), TargetPlatform::Windows, 1_800_000_000).unwrap();
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let mut latest = muse_core::Workspace::new();
    let id = latest.add_account("Original");
    latest
        .rename_account(id, "Changed while preview open")
        .unwrap();
    latest.add_account("Added while preview open");
    assert!(plan.commit(&root, &latest).is_err());
    assert!(!root.join("workspace.json").exists());
    plan.confirm();
    plan.stage_vault(&root).unwrap();
    plan.mark_staged(plan.accounts[0].id).unwrap();
    assert!(plan.commit(&root, &latest).is_err());
    plan.mark_staged(plan.accounts[1].id).unwrap();
    let committed = plan.commit(&root, &latest).unwrap();
    assert_eq!(committed.accounts.len(), 4);
    assert_eq!(
        committed.account(id).unwrap().name,
        "Changed while preview open"
    );
    assert_eq!(
        muse_core::Workspace::load(&root.join("workspace.json"))
            .unwrap()
            .accounts
            .len(),
        4
    );
}
#[test]
fn failed_save_and_cancel_remove_only_new_vault_revisions() {
    for fail in [false, true] {
        let root = root();
        std::fs::create_dir_all(&root).unwrap();
        let existing = uuid::Uuid::new_v4();
        let rev = uuid::Uuid::new_v4();
        let protected = crate::secrets::ProtectedStore::new(&root);
        protected
            .save_revision(existing, rev, "http://proxy.example.invalid:8080")
            .unwrap();
        let mut plan =
            ImportPlan::prepare(payload(), TargetPlatform::Windows, 1_800_000_000).unwrap();
        plan.confirm();
        plan.stage_vault(&root).unwrap();
        assert_eq!(std::fs::read_dir(root.join("secrets")).unwrap().count(), 2);
        if fail {
            std::fs::create_dir(root.join("workspace.json")).unwrap();
            plan.mark_staged(plan.accounts[0].id).unwrap();
            assert!(plan.commit(&root, &muse_core::Workspace::new()).is_err());
        }
        drop(plan);
        assert_eq!(std::fs::read_dir(root.join("secrets")).unwrap().count(), 1);
        assert!(protected.load_selected(existing, Some(rev)).is_ok());
    }
}
#[test]
fn mac_declares_platform_conversion_and_windows_retains_its_supported_branch() {
    let p = payload();
    let mac = ImportPlan::prepare(p.clone(), TargetPlatform::Mac, 1_800_000_000).unwrap();
    assert!(!mac.accounts[0].network.force_proxy());
    assert!(mac.warnings.iter().any(|w| w.contains("本机地址旁路")));
    let provenance = mac.accounts[0].transfer_origin.as_ref().unwrap();
    assert_eq!(provenance.bypass_policy, "strict");
    let windows = ImportPlan::prepare(p, TargetPlatform::Windows, 1_800_000_000).unwrap();
    assert!(windows.accounts[0].network.force_proxy());
    assert_eq!(
        actual_policy(TargetPlatform::Mac, windows.accounts[0].network),
        "platform"
    );
    assert_eq!(
        actual_policy(TargetPlatform::Windows, windows.accounts[0].network),
        "strict"
    );
}
#[test]
fn export_reads_actual_upstream_and_preserves_shared_proxy_and_opaque_auth() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let mut plan =
        ImportPlan::prepare(shared_payload(), TargetPlatform::Windows, 1_800_000_000).unwrap();
    plan.confirm();
    plan.stage_vault(&root).unwrap();
    for id in plan.accounts.iter().map(|a| a.id).collect::<Vec<_>>() {
        plan.mark_staged(id).unwrap();
    }
    let workspace = plan.commit(&root, &muse_core::Workspace::new()).unwrap();
    let snapshots: Vec<_> = plan
        .accounts
        .iter()
        .map(|a| (a.id, payload().accounts[0].cookies.clone()))
        .collect();
    let result = export_payload(&root, &workspace, &snapshots, TargetPlatform::Windows).unwrap();
    assert_eq!(result.proxies.len(), 1);
    assert_eq!(result.accounts.len(), 2);
    assert_eq!(
        result.proxies[0].auth.as_ref().unwrap().username,
        "SYNTHETIC_USER_raw+%3D="
    );
    let text = serde_json::to_string(&result).unwrap();
    for account in &workspace.accounts {
        assert!(!text.contains(&account.id.to_string()));
    }
    assert!(!text.contains("revision"));
    assert!(!text.contains("profiles"));
}
