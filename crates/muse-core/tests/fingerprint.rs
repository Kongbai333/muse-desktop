use muse_core::{BrowserEvent, FingerprintMode, Workspace};

#[test]
fn legacy_workspace_gains_basic_fingerprint_without_changing_identity_or_network() {
    let mut w = Workspace::new();
    let id = w.add_account("synthetic");
    let identity = w.active_browser_identity().unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&w.to_json().unwrap()).unwrap();
    json["accounts"][0]
        .as_object_mut()
        .unwrap()
        .remove("fingerprint");
    let loaded = Workspace::from_json(&json.to_string()).unwrap();
    assert_eq!(loaded.active_account_id(), Some(id));
    assert_eq!(loaded.active_browser_identity().unwrap(), identity);
    let saved: serde_json::Value = serde_json::from_str(&loaded.to_json().unwrap()).unwrap();
    assert_eq!(saved["accounts"][0]["fingerprint"], "basic");
    assert_eq!(saved["accounts"][0]["network"]["mode"], "direct");
}

#[test]
fn fingerprint_original_mode_survives_roundtrip_and_normal_navigation() {
    let mut w = Workspace::new();
    w.add_account("synthetic");
    let mut json: serde_json::Value = serde_json::from_str(&w.to_json().unwrap()).unwrap();
    json["accounts"][0]["fingerprint"] = "original".into();
    let mut loaded = Workspace::from_json(&json.to_string()).unwrap();
    let identity = loaded.active_browser_identity().unwrap();
    loaded.apply_browser_event(identity, BrowserEvent::Url("https://muse.ai/next".into()));
    let saved: serde_json::Value = serde_json::from_str(&loaded.to_json().unwrap()).unwrap();
    assert_eq!(saved["accounts"][0]["fingerprint"], "original");
}

#[test]
fn unknown_fingerprint_mode_is_not_silently_enabled() {
    let mut w = Workspace::new();
    w.add_account("synthetic");
    let mut json: serde_json::Value = serde_json::from_str(&w.to_json().unwrap()).unwrap();
    json["accounts"][0]["fingerprint"] = "untrusted-mode".into();
    assert!(Workspace::from_json(&json.to_string()).is_err());
}

#[test]
fn changing_fingerprint_rebuilds_only_its_own_tabs_and_keeps_profile_and_route() {
    let mut w = Workspace::new();
    let first = w.add_account("first");
    w.add_tab(first, "https://muse.ai/next").unwrap();
    let second = w.add_account("second");
    let before = w.account(first).unwrap().clone();
    let other = w.active_browser_identity().unwrap();
    assert!(
        w.set_fingerprint_mode(first, FingerprintMode::Original)
            .unwrap()
    );
    let after = w.account(first).unwrap();
    assert_eq!(after.id, before.id);
    assert_eq!(after.network, before.network);
    assert_eq!(after.fingerprint, FingerprintMode::Original);
    for (a, b) in after.tabs.iter().zip(&before.tabs) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.url, b.url);
        assert_eq!(a.generation, b.generation + 1);
    }
    assert_eq!(w.active_account_id(), Some(second));
    assert!(w.contains_identity(other));
    let stable = w.to_json().unwrap();
    let generations: Vec<_> = after.tabs.iter().map(|t| t.generation).collect();
    assert!(
        !w.set_fingerprint_mode(first, FingerprintMode::Original)
            .unwrap()
    );
    assert_eq!(w.to_json().unwrap(), stable);
    assert_eq!(
        w.account(first)
            .unwrap()
            .tabs
            .iter()
            .map(|t| t.generation)
            .collect::<Vec<_>>(),
        generations
    );
}

#[test]
fn exhausted_tab_generation_cannot_partly_change_fingerprint() {
    let mut w = Workspace::new();
    let id = w.add_account("synthetic");
    w.accounts[0].tabs[0].generation = u64::MAX;
    let before = w.to_json().unwrap();
    assert!(
        w.set_fingerprint_mode(id, FingerprintMode::Original)
            .is_err()
    );
    assert_eq!(w.to_json().unwrap(), before);
    assert_eq!(w.accounts[0].tabs[0].generation, u64::MAX);
}
