//! Minimal typed results from a scoped ExecuteScript callback.

use muse_core::{QuotaObservation, QuotaState};
use serde::Deserialize;

pub const INITIALIZATION: &str = include_str!("../../../assets/muse-observer.js");

#[derive(Deserialize, serde::Serialize)]
pub struct AvatarAnimation {
    pub frames: Vec<String>,
    pub frame_ms: u32,
}

#[derive(Deserialize)]
pub struct PageSnapshot {
    #[serde(default)]
    pub avatar_animation: Option<Box<AvatarAnimation>>,
    pub url: String,
    pub ready: bool,
    pub quota: Option<QuotaPayload>,
    #[serde(default)]
    pub quota_status: String,
    #[serde(default)]
    pub action_found: bool,
    #[serde(default)]
    pub scripts_scanned: u16,
    #[serde(default)]
    pub style_waiting: u16,
    #[serde(default)]
    pub content_present: bool,
    #[serde(default)]
    pub document_state: String,
    #[serde(default)]
    pub preload_found: bool,
    #[serde(default)]
    pub settings_loaded: bool,
    #[serde(default)]
    pub avatar_png: Option<String>,
    #[serde(default)]
    pub plan_label: Option<String>,
    #[serde(default)]
    pub invite: Option<InvitePayload>,
}

/// Read from the site's own invite popover while it is open.
#[derive(Deserialize)]
pub struct InvitePayload {
    pub code: String,
    #[serde(default)]
    pub remaining: Option<f64>,
    #[serde(default)]
    pub redeemed: Option<f64>,
    #[serde(default)]
    pub earned: Option<String>,
    #[serde(default)]
    pub observed_at: u64,
}
impl InvitePayload {
    pub fn info(&self) -> Option<crate::account_cache::InviteInfo> {
        let code = self.code.trim();
        let count = |value: Option<f64>| {
            value
                .filter(|v| v.is_finite() && *v >= 0. && *v <= 1_000_000. && v.fract() == 0.)
                .map(|v| v as u32)
        };
        let earned = self
            .earned
            .as_deref()
            .map(str::trim)
            .filter(|e| {
                !e.is_empty() && e.chars().count() <= 24 && !e.chars().any(char::is_control)
            })
            .map(str::to_owned);
        ((3..=24).contains(&code.len()) && code.chars().all(|c| c.is_ascii_alphanumeric())).then(
            || crate::account_cache::InviteInfo {
                code: code.to_owned(),
                remaining: count(self.remaining),
                redeemed: count(self.redeemed),
                earned,
                observed_at: self.observed_at,
            },
        )
    }
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum QuotaPayload {
    Known {
        used_percent: f64,
        observed_at: u64,
        resets_at: Option<u64>,
        #[serde(default)]
        extra: Option<ExtraPayload>,
    },
    Unlimited {
        observed_at: u64,
        #[serde(default)]
        extra: Option<ExtraPayload>,
    },
}
#[derive(Deserialize)]
pub struct ExtraPayload {
    remaining: Option<f64>,
    total: Option<f64>,
    display_value: Option<String>,
}
impl QuotaPayload {
    pub fn extra(&self, now: u64) -> Option<muse_core::ExtraQuotaObservation> {
        let (extra, observed_at) = match self {
            Self::Known {
                extra, observed_at, ..
            }
            | Self::Unlimited { extra, observed_at } => (extra.as_ref()?, *observed_at),
        };
        if observed_at > now.saturating_add(30_000) || now.saturating_sub(observed_at) >= 300_000 {
            return None;
        }
        muse_core::ExtraQuotaObservation::new(
            extra.remaining,
            extra.total,
            extra.display_value.as_deref(),
            observed_at,
        )
        .ok()
    }
    pub fn as_state(&self, now: u64) -> Option<QuotaState> {
        let state = match self {
            Self::Known {
                used_percent,
                observed_at,
                resets_at,
                ..
            } => {
                if *observed_at > now.saturating_add(30_000) {
                    return None;
                }
                QuotaState::Known(
                    QuotaObservation::new(*used_percent, 100., "%", *observed_at, *resets_at)
                        .ok()?,
                )
            }
            Self::Unlimited { observed_at, .. } => QuotaState::Unlimited {
                observed_at: *observed_at,
            },
        };
        match state.at_time(now) {
            current @ (QuotaState::Known(_) | QuotaState::Unlimited { .. }) => Some(current),
            _ => None,
        }
    }
}

pub fn decode(raw: &str) -> Option<PageSnapshot> {
    if raw.len() > 2_500_000 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let snapshot: PageSnapshot = if let Some(text) = value.as_str() {
        serde_json::from_str(text).ok()?
    } else {
        serde_json::from_value(value).ok()?
    };
    if snapshot.url.len() > 4096 {
        return None;
    }
    Some(snapshot)
}

pub fn script(force: bool) -> String {
    format!(
        "(() => JSON.stringify(window.__museDesktopObserver?.snapshot({force}) ?? {{url:location.href,ready:false,quota:null,quota_status:'waiting'}}))()"
    )
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_snapshots_discard_invalid_or_expired_percentages() {
        for used in [-1., 101., f64::NAN] {
            assert!(
                QuotaPayload::Known {
                    used_percent: used,
                    observed_at: 1000,
                    resets_at: None,
                    extra: None
                }
                .as_state(2000)
                .is_none()
            );
        }
        assert!(
            QuotaPayload::Known {
                used_percent: 20.,
                observed_at: 1000,
                resets_at: Some(1500),
                extra: None
            }
            .as_state(2000)
            .is_none()
        );
        assert!(
            QuotaPayload::Unlimited {
                observed_at: 90000,
                extra: None
            }
            .as_state(1000)
            .is_none()
        );
        let raw=serde_json::json!({"url":"https://muse.ai/","ready":true,"quota":{"state":"known","used_percent":20,"observed_at":1000,"resets_at":null}}).to_string();
        let snapshot = decode(&serde_json::to_string(&raw).unwrap()).unwrap();
        assert_eq!(
            snapshot
                .quota
                .unwrap()
                .as_state(2000)
                .unwrap()
                .remaining_ratio(),
            Some(0.8)
        );
        assert!(decode(&"x".repeat(66000)).is_none());
    }
}
