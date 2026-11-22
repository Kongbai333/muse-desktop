//! Fail closed on credential-store errors and bound on-disk reference reads.

use std::{io::Read, path::Path};
use zeroize::Zeroizing;

pub fn previous_secret(result: Result<Vec<u8>, i32>) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    match result {
        Ok(value) => Ok(Some(Zeroizing::new(value))),
        Err(-25300) => Ok(None), // errSecItemNotFound, not a permission failure.
        Err(_) => Err("无法读取既有钥匙串凭据，未覆盖原配置".into()),
    }
}

pub fn read_reference(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|_| "此账号的代理凭据引用不存在；未使用直连")?;
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取代理凭据引用")?;
    if bytes.len() > 4096 {
        return Err("代理凭据引用无效".into());
    }
    String::from_utf8(bytes).map_err(|_| "代理凭据引用无效".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denied_or_locked_keychain_reads_must_not_be_treated_as_a_new_password() {
        for code in [-25293, -25308, -50] {
            assert!(previous_secret(Err(code)).is_err());
        }
        assert!(previous_secret(Err(-25300)).unwrap().is_none());
        assert_eq!(
            previous_secret(Ok(b"existing-value".to_vec()))
                .unwrap()
                .unwrap()
                .as_slice(),
            b"existing-value"
        );
    }

    #[test]
    fn oversized_keychain_reference_is_rejected_before_it_is_loaded() {
        let path = std::env::temp_dir().join(format!("muse-reference-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, vec![b'x'; 5000]).unwrap();
        assert!(read_reference(&path).is_err());
        std::fs::write(&path, "MuseDesktop.Keychain.v1\nproxy-fixture\n").unwrap();
        assert_eq!(
            read_reference(&path).unwrap(),
            "MuseDesktop.Keychain.v1\nproxy-fixture\n"
        );
        std::fs::remove_file(path).unwrap();
    }
}
