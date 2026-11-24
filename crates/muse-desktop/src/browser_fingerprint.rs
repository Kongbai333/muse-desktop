//! Stable, local account readback policy. Does not export the local account UUID.
use muse_core::{AccountId, FingerprintMode};

const ADAPTER: &str = include_str!("browser_fingerprint.js");

pub fn initialization(account: AccountId, mode: FingerprintMode) -> Option<String> {
    if mode == FingerprintMode::Original {
        return None;
    }
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    digest.update(b"muse-desktop:account-readback:v1\0");
    digest.update(account.as_bytes());
    let digest = digest.finish();
    let key: Vec<u32> = digest
        .as_ref()
        .chunks_exact(4)
        .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
        .collect();
    Some(ADAPTER.replace(
        "__MUSE_FINGERPRINT_SEED__",
        &serde_json::to_string(&key).unwrap(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_readback_seed_is_stable_but_different_accounts_do_not_share_it() {
        let first = uuid::Uuid::from_u128(1);
        let second = uuid::Uuid::from_u128(2);
        let a = initialization(first, FingerprintMode::Basic).unwrap();
        assert_eq!(a, initialization(first, FingerprintMode::Basic).unwrap());
        assert!(
            a != initialization(second, FingerprintMode::Basic).unwrap(),
            "accounts must not share readback seed"
        );
        assert!(!a.contains(&first.to_string()));
        assert!(!a.contains(&first.simple().to_string()));
    }

    #[test]
    fn original_mode_installs_no_script() {
        assert!(initialization(uuid::Uuid::from_u128(3), FingerprintMode::Original).is_none());
    }
}
