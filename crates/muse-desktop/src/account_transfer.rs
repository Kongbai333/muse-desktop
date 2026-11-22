//! Password-protected, versioned migration. No credential values in errors.
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    aead, pbkdf2,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::{Read, Write},
    num::NonZeroU32,
    path::Path,
};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_FILE: usize = 6_291_456;
pub const MAX_PLAINTEXT: usize = 4_194_304;
const AAD: &[u8] = b"muse-account-transfer:v1:PBKDF2-HMAC-SHA256:600000:AES-256-GCM";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferError {
    Format,
    Password,
    Authentication,
    Payload,
    File,
    Cancelled,
}
impl TransferError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Format => "账号包格式或版本不支持，文件未导入",
            Self::Password => "迁移密码需为 8–1024 个 UTF-8 字节，请保留原有字符与空格",
            Self::Authentication => "密码错误或账号包已损坏，文件未导入",
            Self::Payload => "账号包内容、Cookie 或代理引用无效，文件未导入",
            Self::File => "无法读取或保存账号包，请检查文件与目录权限",
            Self::Cancelled => "操作已取消，未激活新账号",
        }
    }
}

// Custom deserializer intentionally makes nullable fields required, not optional keys.
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    kdf: String,
    iterations: u32,
    salt: String,
    cipher: String,
    nonce: String,
    ciphertext: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub format: String,
    pub version: u32,
    pub accounts: Vec<Account>,
    pub proxies: Vec<Proxy>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub account_ref: String,
    pub name: String,
    pub network: Network,
    pub cookies: CookieSet,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub mode: String,
    #[serde(deserialize_with = "required")]
    pub proxy_ref: Option<String>,
    pub bypass_policy: String,
    #[serde(deserialize_with = "required")]
    pub source_force_proxy: Option<bool>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proxy {
    pub proxy_ref: String,
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    #[serde(deserialize_with = "required")]
    pub auth: Option<Auth>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Auth {
    pub username: String,
    pub password: String,
}
impl Drop for Auth {
    fn drop(&mut self) {
        self.username.zeroize();
        self.password.zeroize();
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookieSet {
    pub fidelity: String,
    pub coverage_urls: Vec<String>,
    pub items: Vec<Cookie>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cookie {
    pub source_url: String,
    pub name: String,
    pub value: String,
    #[serde(deserialize_with = "required")]
    pub domain: Option<String>,
    #[serde(deserialize_with = "required")]
    pub path: Option<String>,
    #[serde(deserialize_with = "required")]
    pub expires_at_unix_seconds: Option<i64>,
    #[serde(deserialize_with = "required")]
    pub secure: Option<bool>,
    #[serde(deserialize_with = "required")]
    pub http_only: Option<bool>,
    #[serde(deserialize_with = "required")]
    pub same_site: Option<String>,
    #[serde(deserialize_with = "required")]
    pub session: Option<bool>,
    #[serde(deserialize_with = "required")]
    pub host_only: Option<bool>,
}
impl Drop for Cookie {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

fn password_valid(password: &str) -> Result<(), TransferError> {
    (8..=1024)
        .contains(&password.len())
        .then_some(())
        .ok_or(TransferError::Password)
}
fn key(password: &str, salt: &[u8]) -> Result<aead::LessSafeKey, TransferError> {
    password_valid(password)?;
    let mut derived = Zeroizing::new([0u8; 32]);
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(600_000).unwrap(),
        salt,
        password.as_bytes(),
        derived.as_mut(),
    );
    aead::UnboundKey::new(&aead::AES_256_GCM, derived.as_ref())
        .map(aead::LessSafeKey::new)
        .map_err(|_| TransferError::Authentication)
}
fn canonical_base64(value: &str) -> Result<Vec<u8>, TransferError> {
    let decoded = STANDARD.decode(value).map_err(|_| TransferError::Format)?;
    if STANDARD.encode(&decoded) != value {
        return Err(TransferError::Format);
    }
    Ok(decoded)
}
fn open_envelope(input: &[u8]) -> Result<(Vec<u8>, [u8; 12], Zeroizing<Vec<u8>>), TransferError> {
    if input.len() > MAX_FILE {
        return Err(TransferError::Format);
    }
    let envelope: Envelope = serde_json::from_slice(input).map_err(|_| TransferError::Format)?;
    if envelope.format != "muse-account-transfer"
        || envelope.version != 1
        || envelope.kdf != "PBKDF2-HMAC-SHA256"
        || envelope.iterations != 600_000
        || envelope.cipher != "AES-256-GCM"
        || serde_json::to_vec(&envelope).map_err(|_| TransferError::Format)? != input
    {
        return Err(TransferError::Format);
    }
    let salt = canonical_base64(&envelope.salt)?;
    let nonce = canonical_base64(&envelope.nonce)?;
    let ciphertext = Zeroizing::new(canonical_base64(&envelope.ciphertext)?);
    if salt.len() != 16
        || nonce.len() != 12
        || !(17..=MAX_PLAINTEXT + 16).contains(&ciphertext.len())
    {
        return Err(TransferError::Format);
    }
    Ok((
        salt,
        nonce.try_into().map_err(|_| TransferError::Format)?,
        ciphertext,
    ))
}
fn decrypt_bytes(input: &[u8], password: &str) -> Result<Zeroizing<Vec<u8>>, TransferError> {
    // Envelope costs and bounds must be checked before KDF/password work.
    let (salt, nonce, mut bytes) = open_envelope(input)?;
    let key = key(password, &salt)?;
    let length = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(AAD),
            &mut bytes,
        )
        .map_err(|_| TransferError::Authentication)?
        .len();
    bytes.truncate(length);
    Ok(bytes)
}
pub fn decrypt(input: &[u8], password: &str) -> Result<Payload, TransferError> {
    decode_payload(&decrypt_bytes(input, password)?)
}
// Private; production callers cannot choose fixed salt/nonce.
fn seal_bytes(
    plaintext: &[u8],
    password: &str,
    salt: [u8; 16],
    nonce: [u8; 12],
) -> Result<Vec<u8>, TransferError> {
    if !(1..=MAX_PLAINTEXT).contains(&plaintext.len()) {
        return Err(TransferError::Payload);
    }
    let key = key(password, &salt)?;
    let mut bytes = Zeroizing::new(plaintext.to_vec());
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(nonce),
        aead::Aad::from(AAD),
        &mut *bytes,
    )
    .map_err(|_| TransferError::Authentication)?;
    let envelope = Envelope {
        format: "muse-account-transfer".into(),
        version: 1,
        kdf: "PBKDF2-HMAC-SHA256".into(),
        iterations: 600_000,
        salt: STANDARD.encode(salt),
        cipher: "AES-256-GCM".into(),
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(&*bytes),
    };
    serde_json::to_vec(&envelope).map_err(|_| TransferError::Format)
}
fn seal_random(plaintext: &[u8], password: &str) -> Result<Vec<u8>, TransferError> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    let random = SystemRandom::new();
    random.fill(&mut salt).map_err(|_| TransferError::File)?;
    random.fill(&mut nonce).map_err(|_| TransferError::File)?;
    seal_bytes(plaintext, password, salt, nonce)
}
pub fn encrypt(payload: &Payload, password: &str) -> Result<Vec<u8>, TransferError> {
    validate(payload, 0)?;
    let bytes = Zeroizing::new(serde_json::to_vec(payload).map_err(|_| TransferError::Payload)?);
    seal_random(&bytes, password)
}
pub fn read_file(path: &Path) -> Result<Vec<u8>, TransferError> {
    let file = std::fs::File::open(path).map_err(|_| TransferError::File)?;
    if file.metadata().map_err(|_| TransferError::File)?.len() > MAX_FILE as u64 {
        return Err(TransferError::Format);
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| TransferError::File)?;
    if bytes.len() > MAX_FILE {
        return Err(TransferError::Format);
    }
    Ok(bytes)
}
pub fn write_file(path: &Path, ciphertext: &[u8]) -> Result<(), TransferError> {
    write_file_if(path, ciphertext, || true)
}
pub fn write_file_if(
    path: &Path,
    ciphertext: &[u8],
    commit: impl FnOnce() -> bool,
) -> Result<(), TransferError> {
    open_envelope(ciphertext)?;
    let parent = path.parent().ok_or(TransferError::File)?;
    let temporary = parent.join(format!(".muse-transfer-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), TransferError> {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|_| TransferError::File)?;
        file.write_all(ciphertext)
            .map_err(|_| TransferError::File)?;
        file.sync_all().map_err(|_| TransferError::File)?;
        drop(file);
        if !commit() {
            return Err(TransferError::Cancelled);
        }
        std::fs::rename(&temporary, path).map_err(|_| TransferError::File)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
pub fn decode_payload(bytes: &[u8]) -> Result<Payload, TransferError> {
    if !(1..=MAX_PLAINTEXT).contains(&bytes.len()) {
        return Err(TransferError::Payload);
    }
    let (mut quoted, mut escaped, mut depth) = (false, false, 0u32);
    for byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 16 {
                        return Err(TransferError::Payload);
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    let payload: Payload = serde_json::from_slice(bytes).map_err(|_| TransferError::Payload)?;
    validate(&payload, 0)?;
    Ok(payload)
}
fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.chars().count() <= 128
        && !value.chars().any(char::is_control)
}
fn valid_ref(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn trusted_url(value: &str) -> Option<url::Url> {
    let url = url::Url::parse(value).ok()?;
    (muse_core::trusted_muse_url(value) && url.fragment().is_none()).then_some(url)
}
fn check(ok: bool) -> Result<(), TransferError> {
    ok.then_some(()).ok_or(TransferError::Payload)
}
pub fn validate(payload: &Payload, now: i64) -> Result<(), TransferError> {
    check(
        payload.format == "muse-account-payload"
            && payload.version == 1
            && (1..=32).contains(&payload.accounts.len())
            && payload.proxies.len() <= 32,
    )?;
    let mut proxies = HashSet::new();
    let mut referenced = HashSet::new();
    for proxy in &payload.proxies {
        check(
            valid_ref(&proxy.proxy_ref)
                && proxies.insert(proxy.proxy_ref.as_str())
                && valid_name(&proxy.name)
                && matches!(proxy.protocol.as_str(), "http" | "https" | "socks5")
                && proxy.port > 0,
        )?;
        check(
            !proxy.host.is_empty()
                && !proxy
                    .host
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace() || "@/\\?#[]".contains(c))
                && (proxy.host.parse::<std::net::IpAddr>().is_ok()
                    || (!proxy.host.contains(':') && url::Host::parse(&proxy.host).is_ok())),
        )?;
        if let Some(auth) = &proxy.auth {
            check(
                !auth.username.is_empty()
                    && auth.username.len() <= 1024
                    && !auth.username.contains(':')
                    && !auth.username.chars().any(char::is_control)
                    && auth.password.len() <= 4096
                    && !auth.password.chars().any(char::is_control),
            )?;
            if proxy.protocol == "socks5" {
                check(auth.username.len() <= 255 && auth.password.len() <= 255)?;
            }
        }
    }
    let mut accounts = HashSet::new();
    for account in &payload.accounts {
        check(
            valid_ref(&account.account_ref)
                && accounts.insert(account.account_ref.as_str())
                && valid_name(&account.name),
        )?;
        let n = &account.network;
        match n.mode.as_str() {
            "proxy" => {
                let reference = n.proxy_ref.as_deref().ok_or(TransferError::Payload)?;
                check(
                    proxies.contains(reference)
                        && matches!(n.bypass_policy.as_str(), "strict" | "platform"),
                )?;
                referenced.insert(reference);
            }
            "system" => check(
                n.proxy_ref.is_none()
                    && n.bypass_policy == "platform"
                    && n.source_force_proxy.is_none(),
            )?,
            _ => return Err(TransferError::Payload),
        }
        let cookies = &account.cookies;
        check(
            matches!(cookies.fidelity.as_str(), "attributes" | "header")
                && (1..=16).contains(&cookies.coverage_urls.len())
                && (1..=256).contains(&cookies.items.len()),
        )?;
        let mut coverage = HashSet::new();
        for url in &cookies.coverage_urls {
            check(trusted_url(url).is_some() && coverage.insert(url.as_str()))?;
        }
        let mut scopes = HashSet::new();
        let mut has_session = false;
        for cookie in &cookies.items {
            let source = trusted_url(&cookie.source_url).ok_or(TransferError::Payload)?;
            check(coverage.contains(cookie.source_url.as_str()))?;
            let source_host = source.host_str().ok_or(TransferError::Payload)?;
            let domain = cookie.domain.as_deref().unwrap_or(source_host);
            check(matches!(
                domain,
                "muse.ai" | ".muse.ai" | "www.muse.ai" | ".www.muse.ai"
            ))?;
            let bare = domain.trim_start_matches('.');
            check(
                source_host == bare
                    || (cookie.host_only != Some(true)
                        && source_host.ends_with(&format!(".{bare}"))),
            )?;
            check(
                (1..=256).contains(&cookie.name.len())
                    && cookie
                        .name
                        .bytes()
                        .all(|b| b.is_ascii_graphic() && !b"()<>@,;:\\\"/[]?={}".contains(&b))
                    && cookie.value.len() <= 16384
                    && !cookie.value.chars().any(|c| c.is_control() || c == ';'),
            )?;
            if let Some(path) = &cookie.path {
                check(
                    path.starts_with('/')
                        && path.len() <= 1024
                        && !path.chars().any(|c| c.is_control() || c == ';'),
                )?;
            }
            if let Some(expiry) = cookie.expires_at_unix_seconds {
                check((1..=253_402_300_799).contains(&expiry))?;
            }
            check(match cookie.session {
                Some(true) => cookie.expires_at_unix_seconds.is_none(),
                Some(false) => cookie.expires_at_unix_seconds.is_some(),
                None => true,
            })?;
            check(
                cookie
                    .same_site
                    .as_deref()
                    .is_none_or(|s| matches!(s, "unspecified" | "lax" | "strict" | "none")),
            )?;
            check(!(cookie.same_site.as_deref() == Some("none") && cookie.secure == Some(false)))?;
            check(scopes.insert((
                cookie.name.as_str(),
                bare.to_owned(),
                cookie.path.as_deref().unwrap_or("/"),
            )))?;
            if cookie.name == "hatch_sess" {
                check(
                    !cookie.value.is_empty()
                        && cookie.expires_at_unix_seconds.is_none_or(|e| e > now),
                )?;
                has_session = true;
            }
        }
        check(has_session)?;
    }
    check(proxies == referenced)
}

#[cfg(test)]
#[path = "account_transfer_tests.rs"]
mod tests;
