use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../checks/fixtures/account-transfer-v1")
            .join(name),
    )
    .unwrap()
}
fn property(kind: &str, key: &str) -> Vec<u8> {
    let properties = String::from_utf8(fixture(&format!("{kind}.properties"))).unwrap();
    let value = properties
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap();
    if key.ends_with("_hex") {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    } else {
        STANDARD.decode(value).unwrap()
    }
}
fn pass(kind: &str) -> String {
    String::from_utf8(property(kind, "passphrase_utf8_base64")).unwrap()
}
fn payload() -> Payload {
    decode_payload(&fixture("payload.json")).unwrap()
}

#[test]
fn consumes_independent_and_java_generated_ascii_and_unicode_packages() {
    for kind in ["ascii", "unicode"] {
        for prefix in ["", "java-"] {
            let bytes = decrypt_bytes(
                &fixture(&format!("{prefix}{kind}.muse-account")),
                &pass(kind),
            )
            .unwrap();
            assert_eq!(bytes.as_slice(), fixture("payload.json"));
            let parsed = decrypt(
                &fixture(&format!("{prefix}{kind}.muse-account")),
                &pass(kind),
            )
            .unwrap();
            assert_eq!(parsed.accounts.len(), 1);
            assert_eq!(parsed.proxies.len(), 1);
        }
    }
}

#[test]
fn reproduces_fixed_vectors_exactly_with_test_only_randomness() {
    for kind in ["ascii", "unicode"] {
        let salt: [u8; 16] = property(kind, "salt_hex").try_into().unwrap();
        let nonce: [u8; 12] = property(kind, "nonce_hex").try_into().unwrap();
        let output = seal_bytes(&fixture("payload.json"), &pass(kind), salt, nonce).unwrap();
        assert_eq!(output, fixture(&format!("{kind}.muse-account")));
    }
}

#[test]
fn wrong_password_and_authenticated_byte_tampering_are_indistinguishable() {
    let input = fixture("unicode.muse-account");
    assert!(matches!(
        decrypt(&input, "SYNTHETIC_WRONG"),
        Err(TransferError::Authentication)
    ));
    for field in ["salt", "nonce", "ciphertext"] {
        let mut value: serde_json::Value = serde_json::from_slice(&input).unwrap();
        let mut bytes = STANDARD.decode(value[field].as_str().unwrap()).unwrap();
        bytes[0] ^= 1;
        value[field] = STANDARD.encode(bytes).into();
        let changed = serde_json::to_vec(&value).unwrap();
        // Value serializes keys in sorted order; reconstruct the canonical envelope.
        let envelope: Envelope = serde_json::from_slice(&changed).unwrap();
        let changed = serde_json::to_vec(&envelope).unwrap();
        assert!(matches!(
            decrypt(&changed, &pass("unicode")),
            Err(TransferError::Authentication)
        ));
    }
}

#[test]
fn canonical_envelope_rejects_metadata_changes_before_even_checking_password() {
    let original = String::from_utf8(fixture("ascii.muse-account")).unwrap();
    for input in [
        format!("{original}\n"),
        original.replace("600000", "999999999"),
        original.replace("\"version\":1", "\"version\":2"),
        original.replace("\"version\":1", "\"version\":1,\"version\":1"),
        original.replace("\"kdf\"", "\"extra\":null,\"kdf\""),
        original.replace(
            "\"version\":1,\"kdf\":\"PBKDF2-HMAC-SHA256\"",
            "\"kdf\":\"PBKDF2-HMAC-SHA256\",\"version\":1",
        ),
        original.replace("muse-account-transfer", "muse\\u002daccount-transfer"),
    ] {
        assert!(matches!(
            decrypt(input.as_bytes(), ""),
            Err(TransferError::Format)
        ));
    }
}

#[test]
fn validates_password_bytes_without_trimming_or_unicode_normalization() {
    assert!(matches!(
        encrypt(&payload(), "short"),
        Err(TransferError::Password)
    ));
    assert!(matches!(
        encrypt(&payload(), &"界".repeat(342)),
        Err(TransferError::Password)
    ));
    let original = format!(" {}\n", pass("unicode"));
    let encrypted = encrypt(&payload(), &original).unwrap();
    assert!(decrypt(&encrypted, &original).is_ok());
    assert!(matches!(
        decrypt(&encrypted, original.trim()),
        Err(TransferError::Authentication)
    ));
}

#[test]
fn random_exports_are_different_and_write_only_ciphertext_atomically() {
    let first = encrypt(&payload(), &pass("unicode")).unwrap();
    let second = encrypt(&payload(), &pass("unicode")).unwrap();
    assert_ne!(first, second);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/transfer-codec-tests")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("export.muse-account");
    write_file(&path, &first).unwrap();
    assert_eq!(read_file(&path).unwrap(), first);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    assert!(
        !String::from_utf8(first)
            .unwrap()
            .contains("SYNTHETIC_SESSION")
    );
}

#[test]
fn rejects_unknown_duplicate_missing_fields_and_excessive_json_depth() {
    let source = String::from_utf8(fixture("payload.json")).unwrap();
    for text in [
        source.replace("\"version\":1", "\"version\":1,\"version\":1"),
        source.replace(
            "\"name\":\"合成账号 A\"",
            "\"name\":\"合成账号 A\",\"extra\":null",
        ),
        source.replace(
            "\"host_only\":null",
            "\"host_only\":null,\"partitioned\":true",
        ),
        source.replace(",\"source_force_proxy\":null", ""),
        format!("{}0{}", "[".repeat(17), "]".repeat(17)),
    ] {
        assert!(matches!(
            decode_payload(text.as_bytes()),
            Err(TransferError::Payload)
        ));
    }
}

#[test]
fn validates_references_and_scope_without_decoding_opaque_values() {
    let good = payload();
    assert_eq!(
        good.accounts[0].cookies.items[0].value,
        "SYNTHETIC_SESSION==%2F%3D%25+ONLY"
    );
    for edit in [
        "missing-ref",
        "foreign-domain",
        "missing-session",
        "invalid-host",
        "unknown-expiry",
        "duplicate-cookie",
    ] {
        let mut value: serde_json::Value =
            serde_json::from_slice(&fixture("payload.json")).unwrap();
        match edit {
            "missing-ref" => value["accounts"][0]["network"]["proxy_ref"] = "absent".into(),
            "foreign-domain" => {
                value["accounts"][0]["cookies"]["items"][0]["domain"] = ".example.com".into()
            }
            "missing-session" => {
                value["accounts"][0]["cookies"]["items"][0]["name"] = "other".into()
            }
            "invalid-host" => value["proxies"][0]["host"] = "user@localhost/path".into(),
            "unknown-expiry" => {
                value["accounts"][0]["cookies"]["items"][0]["session"] = true.into()
            }
            _ => {
                let cookie = value["accounts"][0]["cookies"]["items"][0].clone();
                value["accounts"][0]["cookies"]["items"]
                    .as_array_mut()
                    .unwrap()
                    .push(cookie);
            }
        }
        assert!(
            matches!(
                decode_payload(&serde_json::to_vec(&value).unwrap()),
                Err(TransferError::Payload)
            ),
            "{edit}"
        );
    }
}

#[test]
fn generates_rust_random_exchange_package_when_explicitly_requested() {
    let Ok(output) = std::env::var("MUSE_SYNTHETIC_TRANSFER_OUTPUT") else {
        return;
    };
    let bytes = seal_random(&fixture("payload.json"), &pass("unicode")).unwrap();
    assert_eq!(
        decrypt_bytes(&bytes, &pass("unicode")).unwrap().as_slice(),
        fixture("payload.json")
    );
    write_file(std::path::Path::new(&output), &bytes).unwrap();
}
