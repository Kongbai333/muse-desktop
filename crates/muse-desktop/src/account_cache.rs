//! Last observed display data. Never contains cookies, credentials or page text.
use muse_core::{
    AccountId, ExtraQuotaObservation, ExtraQuotaState, QuotaObservation, QuotaState, Workspace,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Serialize, Deserialize)]
struct CachedAccount {
    plan: Option<std::string::String>,
    quota: Option<QuotaObservation>,
    unlimited: Option<u64>,
    extra: Option<CachedExtra>,
}
#[derive(Serialize, Deserialize)]
struct CachedExtra {
    remaining: Option<f64>,
    total: Option<f64>,
    label: String,
    observed_at: u64,
}

/// Invite code and counts from the site's invite popover. Display data only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InviteInfo {
    pub code: String,
    pub remaining: Option<u32>,
    pub redeemed: Option<u32>,
    pub earned: Option<String>,
    pub observed_at: u64,
    #[serde(default)]
    pub remaining_at: Option<u64>,
    #[serde(default)]
    pub redeemed_at: Option<u64>,
    #[serde(default)]
    pub earned_at: Option<u64>,
}
impl InviteInfo {
    /// Omitted fields must not clear the last success or refresh its age.
    pub fn merge(mut self, previous: &Self) -> Self {
        if self.code != previous.code {
            return self;
        }
        macro_rules! merge_field {
            ($value:ident, $at:ident) => {
                if self.$value.is_none()
                    || self.$at.unwrap_or(self.observed_at)
                        < previous.$at.unwrap_or(previous.observed_at)
                {
                    self.$value = previous.$value.clone();
                    self.$at = previous
                        .$at
                        .or(previous.$value.as_ref().map(|_| previous.observed_at));
                }
            };
        }
        merge_field!(remaining, remaining_at);
        merge_field!(redeemed, redeemed_at);
        merge_field!(earned, earned_at);
        self
    }
    pub fn same_content(&self, other: &Self) -> bool {
        self.code == other.code
            && self.remaining == other.remaining
            && self.redeemed == other.redeemed
            && self.earned == other.earned
    }
}
fn invite_path(root: &Path, id: AccountId) -> std::path::PathBuf {
    root.join("account-cache").join(format!("{id}-invite.json"))
}
pub fn restore_invites(root: &Path, workspace: &Workspace) -> HashMap<AccountId, InviteInfo> {
    workspace
        .accounts
        .iter()
        .filter_map(|account| {
            let path = invite_path(root, account.id);
            if !std::fs::metadata(&path).is_ok_and(|m| m.len() < 4096) {
                return None;
            }
            let info: InviteInfo = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
            ((3..=24).contains(&info.code.len())
                && info.code.chars().all(|c| c.is_ascii_alphanumeric()))
            .then_some((account.id, info))
        })
        .collect()
}
pub fn store_invite(root: &Path, id: AccountId, info: &InviteInfo) {
    if let Ok(bytes) = serde_json::to_vec(info) {
        let _ = write(
            &root.join("account-cache"),
            &format!("{id}-invite.json"),
            &bytes,
        );
    }
}

pub fn write(root: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let temporary = root.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, root.join(name))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
pub fn restore(
    root: &Path,
    workspace: &mut Workspace,
) -> (HashMap<AccountId, String>, HashSet<AccountId>) {
    let mut labels = HashMap::new();
    let mut cached = HashSet::new();
    for account in &mut workspace.accounts {
        let path = root
            .join("account-cache")
            .join(format!("{}-display.json", account.id));
        if !std::fs::metadata(&path).is_ok_and(|m| m.len() < 16_384) {
            continue;
        }
        let Some(value) = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<CachedAccount>(&b).ok())
        else {
            continue;
        };
        if let Some(label) = value.plan.filter(|s| {
            !s.is_empty() && s.chars().count() <= 32 && !s.chars().any(char::is_control)
        }) {
            labels.insert(account.id, label);
        }
        if let Some(q) = value.quota.filter(|q| {
            q.remaining().is_finite()
                && q.remaining() >= 0.
                && q.limit() > 0.
                && q.remaining() <= q.limit()
        }) {
            account.quota = QuotaState::Known(q);
            cached.insert(account.id);
        } else if let Some(observed_at) = value.unlimited {
            account.quota = QuotaState::Unlimited { observed_at };
            cached.insert(account.id);
        }
        if let Some(e) = value.extra
            && let Ok(e) =
                ExtraQuotaObservation::new(e.remaining, e.total, Some(&e.label), e.observed_at)
        {
            account.extra_quota = ExtraQuotaState::Known(e);
            cached.insert(account.id);
        }
    }
    (labels, cached)
}
pub fn store(root: &Path, workspace: &Workspace, id: AccountId, plan: Option<&String>) {
    let Some(account) = workspace.account(id) else {
        return;
    };
    let (quota, unlimited) = match &account.quota {
        QuotaState::Known(q) | QuotaState::Stale(q) => (Some(q.clone()), None),
        QuotaState::Unlimited { observed_at } | QuotaState::StaleUnlimited { observed_at } => {
            (None, Some(*observed_at))
        }
        _ => return,
    };
    let extra = match &account.extra_quota {
        ExtraQuotaState::Known(e) | ExtraQuotaState::Stale(e) => Some(CachedExtra {
            remaining: e.remaining(),
            total: e.total(),
            label: e.label(),
            observed_at: e.observed_at(),
        }),
        _ => None,
    };
    let value = CachedAccount {
        plan: plan.cloned(),
        quota,
        unlimited,
        extra,
    };
    if let Ok(bytes) = serde_json::to_vec(&value) {
        let _ = write(
            &root.join("account-cache"),
            &format!("{id}-display.json"),
            &bytes,
        );
    }
}
pub fn remove(root: &Path, id: AccountId) {
    let _ = std::fs::remove_file(invite_path(root, id));
    let _ = std::fs::remove_file(
        root.join("account-cache")
            .join(format!("{id}-display.json")),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_invite_updates_keep_each_fields_original_success_time_and_valid_zero() {
        let old: InviteInfo = serde_json::from_str(
            r#"{"code":"SYNTH123","remaining":7,"redeemed":2,"earned":"10","observed_at":1000}"#,
        )
        .unwrap();
        let next:InviteInfo=serde_json::from_str(r#"{"code":"SYNTH123","remaining":null,"redeemed":0,"earned":null,"observed_at":5000,"redeemed_at":5000}"#).unwrap();
        let merged = next.merge(&old);
        assert_eq!(merged.remaining, Some(7));
        assert_eq!(merged.remaining_at, Some(1000));
        assert_eq!(merged.redeemed, Some(0));
        assert_eq!(merged.redeemed_at, Some(5000));
        assert_eq!(merged.earned.as_deref(), Some("10"));
        assert_eq!(merged.earned_at, Some(1000));
        let changed:InviteInfo=serde_json::from_str(r#"{"code":"OTHER123","remaining":null,"redeemed":null,"earned":null,"observed_at":6000}"#).unwrap();
        assert_eq!(changed.merge(&merged).remaining, None);
    }
}
