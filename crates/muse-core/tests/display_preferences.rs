use muse_core::{DisplayPreferences, ThemeMode};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("muse-display-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn first_launch_uses_daytime_and_actual_size() {
    let root = fixture();
    let preferences = DisplayPreferences::load(&root);
    assert_eq!(preferences.theme, ThemeMode::Light);
    assert_eq!(preferences.page_zoom(), 1.0);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn reopening_restores_the_last_zoom_and_night_mode() {
    let root = fixture();
    std::fs::write(root.join("workspace.json"), "account-data-must-stay").unwrap();
    let mut preferences = DisplayPreferences::default();
    preferences.theme = ThemeMode::Dark;
    assert!(preferences.set_page_zoom(1.25));
    preferences.save(&root).unwrap();
    let restored = DisplayPreferences::load(&root);
    assert_eq!(restored.theme, ThemeMode::Dark);
    assert_eq!(restored.page_zoom(), 1.25);
    assert_eq!(
        std::fs::read_to_string(root.join("workspace.json")).unwrap(),
        "account-data-must-stay"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn saving_again_replaces_preferences_including_a_reset_to_actual_size() {
    let root = fixture();
    let mut preferences = DisplayPreferences::default();
    preferences.set_page_zoom(0.8);
    preferences.theme = ThemeMode::Dark;
    preferences.save(&root).unwrap();
    preferences.set_page_zoom(1.0);
    preferences.theme = ThemeMode::Light;
    preferences.save(&root).unwrap();
    let restored = DisplayPreferences::load(&root);
    assert_eq!(restored.page_zoom(), 1.0);
    assert_eq!(restored.theme, ThemeMode::Light);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_zoom_cannot_replace_a_readable_saved_preference() {
    let mut preferences = DisplayPreferences::default();
    preferences.set_page_zoom(1.5);
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0] {
        assert!(!preferences.set_page_zoom(value));
        assert_eq!(preferences.page_zoom(), 1.5);
    }
    assert!(preferences.set_page_zoom(0.1));
    assert_eq!(preferences.page_zoom(), 0.25);
    assert!(preferences.set_page_zoom(10.0));
    assert_eq!(preferences.page_zoom(), 3.0);
    assert!(!preferences.set_page_zoom(3.0));
}

#[test]
fn bad_or_partial_preferences_recover_without_resetting_valid_fields() {
    let root = fixture();
    for contents in ["{truncated", "[]", &"x".repeat(5000)] {
        std::fs::write(root.join("display.json"), contents).unwrap();
        let restored = DisplayPreferences::load(&root);
        assert_eq!(restored.theme, ThemeMode::Light);
        assert_eq!(restored.page_zoom(), 1.0);
    }
    std::fs::write(root.join("display.json"), r#"{"theme":"dark"}"#).unwrap();
    let restored = DisplayPreferences::load(&root);
    assert_eq!(restored.theme, ThemeMode::Dark);
    assert_eq!(restored.page_zoom(), 1.0);
    std::fs::write(
        root.join("display.json"),
        r#"{"theme":"dark","page_zoom":-1.0}"#,
    )
    .unwrap();
    let restored = DisplayPreferences::load(&root);
    assert_eq!(restored.theme, ThemeMode::Dark);
    assert_eq!(restored.page_zoom(), 1.0);
    std::fs::remove_dir_all(root).unwrap();
}
