//! Account-folder ownership and document readiness without native dependencies.

use muse_core::AccountId;
use std::{collections::HashMap, path::Path};

#[derive(Default)]
pub struct ProfileBindings(HashMap<AccountId, String>);
impl ProfileBindings {
    pub fn verified(&self, id: AccountId) -> bool {
        self.0.contains_key(&id)
    }
    pub fn verify(&mut self, id: AccountId, expected: &Path, actual: &Path) -> Result<(), String> {
        let normalize = |path: &Path| -> Result<String, String> {
            std::fs::canonicalize(path)
                .map(|path| {
                    path.to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .trim_end_matches(['\\', '/'])
                        .to_lowercase()
                })
                .map_err(|_| "无法核验账号浏览器目录".into())
        };
        let expected = normalize(expected)?;
        let actual = normalize(actual)?;
        if expected != actual {
            return Err("浏览器数据目录与此账号不一致，已阻止加载".into());
        }
        if self
            .0
            .iter()
            .any(|(owner, path)| *owner != id && path == &actual)
        {
            return Err("浏览器数据目录已属于其他账号，已阻止加载".into());
        }
        self.0.insert(id, actual);
        Ok(())
    }
}

#[derive(Default)]
pub struct PageGate {
    epoch: u64,
    ready: bool,
    started: u64,
    first_content: Option<u64>,
    failed: bool,
}
impl PageGate {
    pub fn navigate(&mut self, now: u64) -> u64 {
        self.epoch = self.epoch.saturating_add(1);
        self.ready = false;
        self.first_content = None;
        self.started = now;
        self.failed = false;
        self.epoch
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn observe(&mut self, epoch: u64, content: bool, now: u64) -> bool {
        if epoch != self.epoch || self.failed {
            return false;
        }
        if content {
            let first = *self.first_content.get_or_insert(now);
            if now.saturating_sub(first) >= 80 {
                self.ready = true;
            }
        } else if !self.ready {
            self.first_content = None;
        }
        true
    }
    pub fn timed_out(&mut self, now: u64) -> bool {
        if !self.ready
            && !self.failed
            && self.epoch > 0
            && now.saturating_sub(self.started) >= 25_000
        {
            self.fail();
            return true;
        }
        false
    }
    pub fn fail(&mut self) {
        self.failed = true;
        self.ready = false;
    }
    pub fn diagnostics(&self, now: u64) -> serde_json::Value {
        serde_json::json!({"epoch":self.epoch,"ready":self.ready,"failed":self.failed,"content_seen":self.first_content.is_some(),"elapsed_ms":now.saturating_sub(self.started)})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_profile_override_and_shared_folders_are_rejected_before_navigation() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/profile-binding-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let mut guard = ProfileBindings::default();
        assert!(guard.verify(a, &first, &first).is_ok());
        assert!(guard.verify(a, &first.join("."), &first).is_ok());
        assert!(guard.verify(b, &second, &first).is_err());
        assert!(guard.verify(b, &first, &first).is_err());
        assert!(guard.verify(b, &second, &second).is_ok());
    }

    #[test]
    fn readiness_requires_stable_content_and_old_navigation_cannot_commit() {
        let mut gate = PageGate::default();
        let old = gate.navigate(1000);
        assert!(!gate.ready());
        assert!(gate.observe(old, true, 1100));
        assert!(!gate.ready());
        assert!(gate.observe(old, true, 1250));
        assert!(gate.ready());
        let current = gate.navigate(1300);
        assert_ne!(old, current);
        assert!(!gate.observe(old, true, 1600));
        assert!(!gate.ready());
        assert!(gate.observe(current, false, 1700));
        assert!(!gate.ready());
        assert!(gate.observe(current, true, 1800));
        assert!(gate.observe(current, true, 1950));
        assert!(gate.ready());
    }

    #[test]
    fn blank_or_failed_documents_timeout_instead_of_becoming_visible() {
        let mut gate = PageGate::default();
        let epoch = gate.navigate(1000);
        assert!(gate.observe(epoch, false, 2000));
        assert!(!gate.timed_out(5000));
        assert!(gate.timed_out(26000));
        assert!(!gate.observe(epoch, true, 27000));
        assert!(!gate.ready());
        let retry = gate.navigate(30000);
        assert!(gate.observe(retry, true, 30100));
        gate.fail();
        assert!(!gate.observe(retry, true, 31000));
        assert!(!gate.ready());
    }
}
