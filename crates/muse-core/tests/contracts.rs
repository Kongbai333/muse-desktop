use muse_core::{BrowserEvent, QuotaObservation, QuotaState, Workspace};
use muse_core::{ExtraQuotaObservation, ExtraQuotaState};
use std::path::Path;

#[test]
fn deleting_an_account_never_reuses_its_profile_directory() {
    let mut workspace = Workspace::new();
    let first = workspace.add_account("主账号");
    let path = workspace
        .account(first)
        .unwrap()
        .profile_dir(Path::new("profiles"));
    workspace.remove_account(first).unwrap();
    let second = workspace.add_account("工作账号");
    assert_ne!(
        path,
        workspace
            .account(second)
            .unwrap()
            .profile_dir(Path::new("profiles"))
    );
}

#[test]
fn delayed_page_events_cannot_update_another_account_or_generation() {
    let mut workspace = Workspace::new();
    let first = workspace.add_account("甲");
    let old_identity = workspace.active_browser_identity().unwrap();
    let second = workspace.add_account("乙");
    assert!(workspace.apply_browser_event(old_identity, BrowserEvent::Title("甲的页面".into())));
    assert_eq!(
        workspace
            .account(second)
            .unwrap()
            .active_tab()
            .unwrap()
            .title,
        "Muse"
    );
    workspace.remove_account(first).unwrap();
    assert!(!workspace.apply_browser_event(old_identity, BrowserEvent::Title("迟到的事件".into())));
    let identity = workspace.active_browser_identity().unwrap();
    workspace.restart_tab(identity.tab_id).unwrap();
    assert!(!workspace.apply_browser_event(identity, BrowserEvent::Title("旧代次".into())));
}

#[test]
fn remaining_quota_does_not_treat_unknown_as_zero() {
    assert_eq!(QuotaState::Unknown.remaining_ratio(), None);
    let quota = QuotaObservation::new(25.0, 100.0, "次", 1000, None).unwrap();
    assert_eq!(QuotaState::Known(quota).remaining_ratio(), Some(0.75));
    let exhausted = QuotaObservation::new(100.0, 100.0, "次", 1000, None).unwrap();
    assert_eq!(QuotaState::Known(exhausted).remaining_ratio(), Some(0.0));
}

#[test]
fn percentage_quota_reports_remaining_percent_and_newest_observation_wins() {
    let mut workspace = Workspace::new();
    let id = workspace.add_account("测试账号");
    let identity = workspace.active_browser_identity().unwrap();
    let newer = QuotaObservation::new(28., 100., "%", 2000, Some(9000)).unwrap();
    let older = QuotaObservation::new(10., 100., "%", 1000, None).unwrap();
    assert!(workspace.apply_quota(identity, "https://www.muse.ai/", newer));
    assert!(!workspace.apply_quota(identity, "https://muse.ai/", older));
    assert_eq!(workspace.account(id).unwrap().quota.label(), "剩余 72%");
    assert!(!workspace.apply_quota(
        identity,
        "https://user@muse.ai/",
        QuotaObservation::new(1., 100., "%", 3000, None).unwrap()
    ));
}

#[test]
fn quota_expires_at_reset_or_after_five_minutes_and_future_samples_are_rejected() {
    let fresh = QuotaState::Known(QuotaObservation::new(28., 100., "%", 1000, Some(9000)).unwrap());
    assert_eq!(fresh.clone().at_time(5000).remaining_ratio(), Some(0.72));
    assert!(matches!(fresh.at_time(9000), QuotaState::Stale(_)));
    let old = QuotaState::Known(QuotaObservation::new(28., 100., "%", 1000, None).unwrap());
    assert!(matches!(old.at_time(301001), QuotaState::Stale(_)));
    let future = QuotaState::Known(QuotaObservation::new(28., 100., "%", 90000, None).unwrap());
    assert!(future.at_time(1000).remaining_ratio().is_none());
}

#[test]
fn quota_status_updates_cannot_bypass_validation_or_replace_newer_usage() {
    let mut workspace = Workspace::new();
    let id = workspace.add_account("测试账号");
    let identity = workspace.active_browser_identity().unwrap();
    let observation = QuotaObservation::new(30., 100., "%", 5000, None).unwrap();
    workspace.apply_quota(identity, "https://muse.ai/", observation.clone());
    assert!(!workspace.apply_quota_status(
        identity,
        "https://muse.ai/",
        QuotaState::Known(observation)
    ));
    assert!(!workspace.apply_quota_status(
        identity,
        "https://muse.ai/",
        QuotaState::Unlimited { observed_at: 1000 }
    ));
    assert_eq!(
        workspace.account(id).unwrap().quota.remaining_ratio(),
        Some(0.7)
    );
    assert!(!workspace.apply_quota_status(
        identity,
        "https://muse.ai.evil.test/",
        QuotaState::LoggedOut
    ));
    assert!(workspace.apply_quota_status(identity, "https://muse.ai/", QuotaState::LoggedOut));
    assert!(
        workspace
            .account(id)
            .unwrap()
            .quota
            .remaining_ratio()
            .is_none()
    );
}

#[test]
fn extra_pool_observations_are_account_scoped_ordered_and_cleared_on_logout() {
    let mut workspace = Workspace::new();
    let first = workspace.add_account("甲");
    let identity = workspace.active_browser_identity().unwrap();
    let second = workspace.add_account("乙");
    let observation =
        ExtraQuotaObservation::new(Some(600.), Some(1000.), Some("剩余 600 积分"), 5000).unwrap();
    assert!(!workspace.apply_extra_quota(
        identity,
        "https://muse.ai.evil.test/",
        observation.clone()
    ));
    assert!(workspace.apply_extra_quota(identity, "https://muse.ai/", observation));
    assert_eq!(
        workspace.account(first).unwrap().extra_quota.label(),
        "剩余 600 积分"
    );
    assert!(matches!(
        workspace.account(second).unwrap().extra_quota,
        ExtraQuotaState::Unknown
    ));
    assert!(!workspace.apply_extra_quota(
        identity,
        "https://muse.ai/",
        ExtraQuotaObservation::new(Some(700.), None, None, 4000).unwrap()
    ));
    workspace.apply_quota(
        identity,
        "https://muse.ai/",
        QuotaObservation::new(30., 100., "%", 6000, None).unwrap(),
    );
    assert!(!workspace.apply_extra_quota(
        identity,
        "https://muse.ai/",
        ExtraQuotaObservation::new(Some(700.), None, None, 5500).unwrap()
    ));
    assert!(workspace.apply_quota_status(identity, "https://muse.ai/", QuotaState::LoggedOut));
    assert!(matches!(
        workspace.account(first).unwrap().extra_quota,
        ExtraQuotaState::Unknown
    ));
    assert!(!workspace.apply_extra_quota(
        identity,
        "https://muse.ai/",
        ExtraQuotaObservation::new(Some(600.), None, None, 5000).unwrap()
    ));
    assert!(!workspace.to_json().unwrap().contains("600 积分"));
}

#[test]
fn malformed_quota_is_rejected_instead_of_becoming_a_progress_ring() {
    for (used, limit) in [
        (f64::NAN, 100.0),
        (1.0, 0.0),
        (-1.0, 100.0),
        (101.0, 100.0),
        (1.0, f64::INFINITY),
    ] {
        assert!(QuotaObservation::new(used, limit, "次", 1000, None).is_err());
    }
}

#[test]
fn metadata_roundtrip_preserves_accounts_tabs_and_active_selection() {
    let mut workspace = Workspace::new();
    let first = workspace.add_account("主账号");
    let second = workspace.add_account("工作账号");
    workspace
        .add_tab(second, "https://muse.ai/library")
        .unwrap();
    workspace.select_account(first).unwrap();
    let json = workspace.to_json().unwrap();
    let restored = Workspace::from_json(&json).unwrap();
    assert_eq!(restored.active_account_id(), Some(first));
    assert_eq!(restored.account(second).unwrap().tabs.len(), 2);
    assert_eq!(restored.account(first).unwrap().name, "主账号");
    assert!(Workspace::from_json("{broken").is_err());
}

#[test]
fn untrusted_urls_are_not_accepted_as_website_quota_observations() {
    let mut workspace = Workspace::new();
    let account = workspace.add_account("主账号");
    let identity = workspace.active_browser_identity().unwrap();
    let observation = QuotaObservation::new(25.0, 100.0, "次", 1000, None).unwrap();
    assert!(!workspace.apply_quota(
        identity,
        "https://muse.ai.evil.example/",
        observation.clone()
    ));
    assert!(!workspace.apply_quota(identity, "http://muse.ai/", observation.clone()));
    assert!(workspace.apply_quota(identity, "https://muse.ai/settings", observation));
    assert_eq!(
        workspace.account(account).unwrap().quota.remaining_ratio(),
        Some(0.75)
    );
}

#[test]
fn a_background_popup_does_not_steal_the_selected_account() {
    let mut workspace = Workspace::new();
    let first = workspace.add_account("甲");
    let opener = workspace.active_browser_identity().unwrap();
    let second = workspace.add_account("乙");
    workspace
        .open_popup(opener, "https://muse.ai/help")
        .unwrap();
    assert_eq!(workspace.active_account_id(), Some(second));
    assert_eq!(workspace.account(first).unwrap().tabs.len(), 2);
    assert_eq!(
        workspace.account(first).unwrap().active_tab_id(),
        opener.tab_id
    );
    workspace.remove_account(first).unwrap();
    assert!(
        workspace
            .open_popup(opener, "https://muse.ai/help")
            .is_err()
    );
}

#[test]
fn closing_the_last_tab_creates_a_new_identity_with_a_return_home() {
    let mut workspace = Workspace::new();
    workspace.add_account("主账号");
    let old = workspace.active_browser_identity().unwrap();
    workspace.close_tab(old.tab_id).unwrap();
    let new = workspace.active_browser_identity().unwrap();
    assert_ne!(old.tab_id, new.tab_id);
    assert!(!workspace.contains_identity(old));
    assert_eq!(
        workspace
            .active_account()
            .unwrap()
            .active_tab()
            .unwrap()
            .url,
        "https://muse.ai/"
    );
}

#[test]
fn disk_save_replaces_previous_state_and_corrupt_load_does_not_overwrite_it() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/contract-fixtures")
        .join(uuid::Uuid::new_v4().to_string());
    let path = directory.join("workspace.json");
    let mut workspace = Workspace::new();
    let id = workspace.add_account("第一次");
    workspace.save(&path).unwrap();
    workspace.rename_account(id, "第二次").unwrap();
    workspace.save(&path).unwrap();
    assert_eq!(
        Workspace::load(&path).unwrap().account(id).unwrap().name,
        "第二次"
    );
    std::fs::write(&path, "{broken").unwrap();
    assert!(Workspace::load(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
}

#[test]
fn duplicate_profile_identifiers_in_metadata_are_rejected() {
    let mut workspace = Workspace::new();
    workspace.add_account("甲");
    workspace.add_account("乙");
    let mut value: serde_json::Value = serde_json::from_str(&workspace.to_json().unwrap()).unwrap();
    value["accounts"][1]["id"] = value["accounts"][0]["id"].clone();
    assert!(Workspace::from_json(&value.to_string()).is_err());
}

#[test]
fn a_stale_or_unavailable_quota_never_claims_to_be_current_remaining_usage() {
    let observation = QuotaObservation::new(25.0, 100.0, "次", 1000, None).unwrap();
    assert_eq!(QuotaState::Stale(observation).remaining_ratio(), None);
    assert_eq!(
        QuotaState::Unavailable("未登录".into()).remaining_ratio(),
        None
    );
}

#[test]
fn older_workspaces_without_network_metadata_keep_their_existing_accounts() {
    let mut workspace = Workspace::new();
    let id = workspace.add_account("原账号");
    let mut value: serde_json::Value = serde_json::from_str(&workspace.to_json().unwrap()).unwrap();
    value["accounts"][0]
        .as_object_mut()
        .unwrap()
        .remove("network");
    value["accounts"][0]
        .as_object_mut()
        .unwrap()
        .remove("legacy_id");
    let restored = Workspace::from_json(&value.to_string()).unwrap();
    assert_eq!(
        restored.account(id).unwrap().network,
        muse_core::AccountNetwork::Direct
    );
    assert_eq!(restored.account(id).unwrap().name, "原账号");
}
