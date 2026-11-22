use muse_core::{AccountId, AccountNetwork, Workspace};
use muse_proxy::{ProxyProtocol, Upstream};
use std::path::Path;
use zeroize::Zeroizing;

#[derive(Clone, PartialEq, Eq)]
pub struct ProxyDraft {
    pub enabled: bool,
    pub protocol: ProxyProtocol,
    pub host: String,
    pub port: String,
    pub user: String,
    pub secret: Zeroizing<String>,
    pub force_proxy: bool,
}

impl Default for ProxyDraft {
    fn default() -> Self {
        Self {
            enabled: false,
            protocol: ProxyProtocol::Http,
            host: String::new(),
            port: "8080".into(),
            user: String::new(),
            secret: Zeroizing::new(String::new()),
            force_proxy: true,
        }
    }
}

impl ProxyDraft {
    pub fn from_url(input: &str, force: bool) -> Result<Self, String> {
        let upstream = Upstream::parse(input).map_err(|error| error.to_string())?;
        let normalized = if input.contains("://") {
            Zeroizing::new(input.to_owned())
        } else {
            Zeroizing::new(format!("http://{input}"))
        };
        let url = url::Url::parse(&normalized).map_err(|_| "代理地址无效")?;
        let raw = &url[url::Position::BeforeUsername..url::Position::BeforeHost];
        let (user, secret) = raw
            .strip_suffix('@')
            .map(|value| value.split_once(':').unwrap_or((value, "")))
            .unwrap_or(("", ""));
        let decode = |value: &str| {
            percent_encoding::percent_decode_str(value)
                .decode_utf8()
                .map(|value| value.into_owned())
                .map_err(|_| "代理认证编码无效")
        };
        Ok(Self {
            enabled: true,
            protocol: upstream.protocol(),
            host: upstream.host().into(),
            port: upstream.port().to_string(),
            user: decode(user)?,
            secret: Zeroizing::new(decode(secret)?),
            force_proxy: force,
        })
    }
    pub fn proxy_url(&self) -> Result<Zeroizing<String>, String> {
        if !self.enabled {
            return Err("请先选择代理协议".into());
        }
        let host = self.host.trim();
        let port = self
            .port
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or("端口必须为 1–65535")?;
        if host.is_empty() {
            return Err("请填写代理服务器地址".into());
        }
        if host.chars().any(|ch| {
            ch.is_whitespace() || ch.is_control() || matches!(ch, '@' | '/' | '\\' | '?' | '#')
        }) {
            return Err("请只填写服务器地址，或使用粘贴地址按钮".into());
        }
        let plain_host = host.trim_matches(['[', ']']);
        let host = if plain_host.parse::<std::net::Ipv6Addr>().is_ok() {
            format!("[{plain_host}]")
        } else {
            if host.contains(':') {
                return Err("请将服务器地址和端口分开填写".into());
            }
            host.to_owned()
        };
        let encode = |value: &str| {
            percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC)
                .to_string()
        };
        if self.user.is_empty() && !self.secret.is_empty() {
            return Err("填写密码时也需要填写用户名".into());
        }
        let credentials = if self.user.is_empty() {
            Zeroizing::new(String::new())
        } else {
            Zeroizing::new(format!("{}:{}@", encode(&self.user), encode(&self.secret)))
        };
        let value = Zeroizing::new(format!(
            "{}://{}{host}:{port}/",
            self.protocol.scheme(),
            credentials.as_str()
        ));
        Upstream::parse(&value).map_err(|error| error.to_string())?;
        Ok(value)
    }
}

pub fn load(root: &Path, workspace: &Workspace, id: AccountId) -> Result<ProxyDraft, String> {
    let account = workspace.account(id).ok_or("账号不存在")?;
    let enabled = account.network != AccountNetwork::Direct;
    let (credential, revision) = workspace.proxy_credentials(account)?;
    match crate::secrets::ProtectedStore::new(root).load_selected(credential, revision) {
        Ok(value) => {
            let mut draft =
                ProxyDraft::from_url(&value, !enabled || account.network.force_proxy())?;
            draft.enabled = enabled;
            Ok(draft)
        }
        Err(_) if !enabled => Ok(ProxyDraft::default()),
        Err(error) => Err(error),
    }
}
pub fn save(
    root: &Path,
    workspace: &mut Workspace,
    id: AccountId,
    draft: &ProxyDraft,
) -> Result<(), String> {
    let old_revision = workspace.account(id).ok_or("账号不存在")?.proxy_revision;
    let mut candidate = workspace.clone();
    let protected = crate::secrets::ProtectedStore::new(root);
    let revision = if draft.enabled {
        let value = draft.proxy_url()?;
        let revision = uuid::Uuid::new_v4();
        protected.save_revision(id, revision, &value)?;
        Some(revision)
    } else {
        old_revision
    };
    let network = if draft.enabled {
        AccountNetwork::HttpProxy {
            force_proxy: draft.force_proxy,
        }
    } else {
        AccountNetwork::Direct
    };
    let commit = candidate
        .update_network(id, network, revision)
        .and_then(|()| candidate.save(&root.join("workspace.json")));
    if let Err(error) = commit {
        if draft.enabled
            && let Some(revision) = revision
        {
            protected.remove_revision(id, revision);
        }
        return Err(error);
    }
    *workspace = candidate;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::ProtectedStore;

    fn fixture() -> (std::path::PathBuf, Workspace, AccountId) {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/proxy-settings-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        let mut state = Workspace::new();
        let id = state.add_account("测试账号");
        state.save(&root.join("workspace.json")).unwrap();
        (root, state, id)
    }

    #[test]
    fn editing_a_proxy_preserves_escaped_credentials_and_ipv6_hosts() {
        let sample = ["hello", "world@#"].join(":");
        let encoded =
            percent_encoding::utf8_percent_encode(&sample, percent_encoding::NON_ALPHANUMERIC);
        let url = format!("http://agent%40site:{encoded}@[::1]:8081/");
        let draft = ProxyDraft::from_url(&url, true).unwrap();
        assert_eq!(draft.host, "::1");
        assert_eq!(draft.port, "8081");
        assert_eq!(draft.user, "agent@site");
        assert_eq!(draft.secret.as_str(), "hello:world@#");
        assert!(muse_proxy::Upstream::parse(draft.proxy_url().unwrap().as_str()).is_ok());
        let restored = ProxyDraft::from_url(draft.proxy_url().unwrap().as_str(), true).unwrap();
        assert!(restored == draft);
    }

    #[test]
    fn pasted_token_only_proxy_keeps_the_password_blank_after_save_and_reload() {
        let (root, mut state, id) = fixture();
        let draft =
            ProxyDraft::from_url("socks://fixture-token:@proxy.example:7153", true).unwrap();
        assert_eq!(draft.protocol, ProxyProtocol::Socks5);
        assert_eq!(draft.host, "proxy.example");
        assert_eq!(draft.port, "7153");
        assert_eq!(draft.user, "fixture-token");
        assert!(draft.secret.is_empty());
        save(&root, &mut state, id, &draft).unwrap();
        let restored = Workspace::load(&root.join("workspace.json")).unwrap();
        assert!(load(&root, &restored, id).unwrap() == draft);
        assert!(!restored.to_json().unwrap().contains("fixture-token"));
    }

    #[test]
    fn https_and_socks5_keep_their_protocol_default_port_and_credentials_after_restart() {
        let (root, mut state, id) = fixture();
        for (input, protocol, port) in [
            ("https://agent:p%40ss@localhost", "https", "443"),
            ("socks5://agent:p%40ss@[::1]", "socks5", "1080"),
        ] {
            let draft = ProxyDraft::from_url(input, true).unwrap();
            assert_eq!(draft.port, port);
            assert_eq!(draft.secret.as_str(), "p@ss");
            assert!(
                draft
                    .proxy_url()
                    .unwrap()
                    .starts_with(&format!("{protocol}://"))
            );
            save(&root, &mut state, id, &draft).unwrap();
            let restored = Workspace::load(&root.join("workspace.json")).unwrap();
            assert!(load(&root, &restored, id).unwrap() == draft);
            assert!(!restored.to_json().unwrap().contains("p@ss"));
        }
    }

    #[test]
    fn saving_restarts_only_that_accounts_generations_and_roundtrips_the_draft() {
        let (root, mut state, id) = fixture();
        let tab = state.add_tab(id, "https://muse.ai/settings").unwrap();
        let before = state.active_browser_identity().unwrap();
        let other = state.add_account("其他账号");
        let other_before = state.active_browser_identity().unwrap();
        state.select_account(id).unwrap();
        let draft = ProxyDraft::from_url("http://agent:old-value@127.0.0.1:8081", true).unwrap();
        save(&root, &mut state, id, &draft).unwrap();
        assert!(!state.contains_identity(before));
        assert!(state.contains_identity(other_before));
        assert_eq!(state.account(id).unwrap().active_tab_id(), tab);
        assert_eq!(state.account(other).unwrap().name, "其他账号");
        assert!(load(&root, &state, id).unwrap() == draft);
        let restored = Workspace::load(&root.join("workspace.json")).unwrap();
        assert!(load(&root, &restored, id).unwrap() == draft);
        let json = restored.to_json().unwrap();
        assert!(!json.contains("old-value"));
        assert!(!json.contains("127.0.0.1:8081"));
    }

    #[test]
    fn invalid_drafts_and_failed_metadata_commits_keep_the_applied_proxy() {
        let (root, mut state, id) = fixture();
        let old = ProxyDraft::from_url("http://agent:old-value@127.0.0.1:8081", true).unwrap();
        save(&root, &mut state, id, &old).unwrap();
        let mut invalid = old.clone();
        invalid.port = "0".into();
        assert!(save(&root, &mut state, id, &invalid).is_err());
        assert!(load(&root, &state, id).unwrap() == old);
        let current = std::fs::read(root.join("workspace.json")).unwrap();
        std::fs::remove_file(root.join("workspace.json")).unwrap();
        std::fs::create_dir(root.join("workspace.json")).unwrap();
        let mut changed = old.clone();
        changed.secret = Zeroizing::new("new-value".into());
        assert!(save(&root, &mut state, id, &changed).is_err());
        assert!(load(&root, &state, id).unwrap() == old);
        assert!(!current.is_empty());
    }

    #[test]
    fn direct_mode_and_existing_imported_credentials_remain_compatible() {
        let (root, mut state, id) = fixture();
        let url = "http://agent:old-value@127.0.0.1:8081";
        ProtectedStore::new(&root).save_proxy(id, url).unwrap();
        state.accounts[0].network = AccountNetwork::HttpProxy { force_proxy: true };
        assert!(load(&root, &state, id).unwrap().enabled);
        let mut draft = load(&root, &state, id).unwrap();
        draft.enabled = false;
        save(&root, &mut state, id, &draft).unwrap();
        assert_eq!(state.account(id).unwrap().network, AccountNetwork::Direct);
        draft.enabled = true;
        save(&root, &mut state, id, &draft).unwrap();
        assert!(load(&root, &state, id).unwrap() == draft);
    }
}
