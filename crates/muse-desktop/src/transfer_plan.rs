//! Unpublished account graph and reversible vault staging.
use crate::{account_transfer as wire, proxy_settings::ProxyDraft, secrets::ProtectedStore};
use muse_core::{AccountId, AccountNetwork, ProxyProfile, Workspace};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TargetPlatform {
    Windows,
    Mac,
}
impl TargetPlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Mac
        }
    }
}
pub fn actual_policy(platform: TargetPlatform, network: AccountNetwork) -> &'static str {
    if platform == TargetPlatform::Windows
        && network != AccountNetwork::Direct
        && network.force_proxy()
    {
        "strict"
    } else {
        "platform"
    }
}
struct VaultStage {
    root: PathBuf,
    entries: Vec<(Uuid, Uuid)>,
    committed: bool,
}
impl Drop for VaultStage {
    fn drop(&mut self) {
        if !self.committed {
            let store = ProtectedStore::new(&self.root);
            for (id, revision) in &self.entries {
                store.remove_revision(*id, *revision);
            }
        }
    }
}
pub struct ImportPlan {
    pub accounts: Vec<muse_core::Account>,
    pub proxies: Vec<ProxyProfile>,
    pub cookies: HashMap<AccountId, wire::CookieSet>,
    pub warnings: Vec<String>,
    pub proxy_previews: Vec<String>,
    upstream: Vec<(Uuid, Uuid, Zeroizing<String>)>,
    staged: HashSet<AccountId>,
    confirmed: bool,
    vault: Option<VaultStage>,
}
impl ImportPlan {
    pub fn prepare(
        payload: wire::Payload,
        platform: TargetPlatform,
        now: i64,
    ) -> Result<Self, String> {
        wire::validate(&payload, now).map_err(|e| e.message().to_owned())?;
        let mut plan = Self {
            accounts: Vec::new(),
            proxies: Vec::new(),
            cookies: HashMap::new(),
            warnings: vec!["所有账号均新建隔离环境；Cookie 写入后仍需网页确认真实身份。".into()],
            proxy_previews: Vec::new(),
            upstream: Vec::new(),
            staged: HashSet::new(),
            confirmed: false,
            vault: None,
        };
        let mut references = HashMap::new();
        if !payload.proxies.is_empty() {
            plan.warnings.push(
                "本机地址旁路规则可能随平台变化，将按目标设备实际策略运行，不保证与来源逐项相同。"
                    .into(),
            );
        }
        for proxy in &payload.proxies {
            let display_host = if proxy.host.contains(':') {
                format!("[{}]", proxy.host)
            } else {
                proxy.host.clone()
            };
            plan.proxy_previews.push(format!(
                "{} · {}://{}:{} · {}",
                proxy.name,
                proxy.protocol,
                display_host,
                proxy.port,
                if proxy.auth.is_some() {
                    "包含认证（隐藏）"
                } else {
                    "无认证"
                }
            ));
            let id = Uuid::new_v4();
            let revision = Uuid::new_v4();
            let draft = wire_proxy_draft(proxy)?;
            let upstream = draft.proxy_url().map_err(|_| "账号包中的代理无法恢复")?;
            references.insert(proxy.proxy_ref.clone(), id);
            plan.proxies.push(ProxyProfile {
                id,
                name: proxy.name.clone(),
                protocol: proxy.protocol.clone(),
                revision: Some(revision),
            });
            plan.upstream.push((id, revision, upstream));
            if device_address(&proxy.host) {
                plan.warnings.push(format!(
                    "代理「{}」使用本机或局域网地址；目标设备可能无法到达，地址不会自动改写。",
                    proxy.name
                ));
            }
        }
        let mut fragment = Workspace::new();
        let mut unknown_cookie_attributes = false;
        for account in payload.accounts {
            let id = fragment.add_account(&account.name);
            let network = if account.network.mode == "system" {
                plan.warnings.push(format!(
                    "「{}」使用系统网络，目标设备出口可能不同，请确认。",
                    account.name
                ));
                AccountNetwork::Direct
            } else {
                let proxy_id = *references
                    .get(account.network.proxy_ref.as_deref().ok_or("缺少代理引用")?)
                    .ok_or("代理引用不存在")?;
                let force_proxy = platform == TargetPlatform::Windows
                    && account.network.bypass_policy == "strict";
                let network = AccountNetwork::PoolProxy {
                    proxy_id,
                    force_proxy,
                };
                if actual_policy(platform, network) != account.network.bypass_policy {
                    plan.warnings.push(format!("「{}」的本机地址旁路规则将从严格代理转换为 macOS 平台策略；公开 Muse 请求仍使用指定代理，失败不直连。", account.name));
                }
                network
            };
            fragment.update_network(id, network, None)?;
            let local = fragment
                .accounts
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or("无法准备新账号")?;
            local.name = account.name;
            local.transfer_origin = Some(muse_core::TransferOrigin {
                bypass_policy: account.network.bypass_policy,
                source_force_proxy: account.network.source_force_proxy,
            });
            if account.cookies.fidelity == "header"
                || account.cookies.items.iter().any(|c| {
                    c.host_only.is_none()
                        || c.domain.is_none()
                        || c.path.is_none()
                        || c.secure.is_none()
                        || c.http_only.is_none()
                        || c.same_site.is_none()
                        || c.session.is_none()
                })
            {
                unknown_cookie_attributes = true;
            }
            plan.cookies.insert(id, account.cookies);
        }
        if unknown_cookie_attributes {
            plan.warnings.extend([
                "部分 Cookie 属性未知：缺少域或路径时，使用来源 HTTPS 主机与根路径。".into(),
                "未知 Secure 开启；未知 HttpOnly 仅对登录 Cookie 开启。".into(),
                "未知 SameSite 使用浏览器默认；缺少有效期按会话处理，已有期限不延长。".into(),
                "host-only 保真度未知；不能保证与源浏览器的所有 Cookie 行为一致。".into(),
            ]);
        }
        plan.accounts = fragment.accounts;
        Ok(plan)
    }
    pub fn confirm(&mut self) {
        self.confirmed = true;
    }
    pub fn stage_vault(&mut self, root: &Path) -> Result<(), String> {
        if !self.confirmed || self.vault.is_some() {
            return Err("请先确认导入预览".into());
        }
        let mut stage = VaultStage {
            root: root.to_owned(),
            entries: Vec::new(),
            committed: false,
        };
        let store = ProtectedStore::new(root);
        for (id, revision, value) in &self.upstream {
            store
                .save_revision(*id, *revision, value)
                .map_err(|_| "无法保存新代理的受保护凭据，未激活账号")?;
            stage.entries.push((*id, *revision));
        }
        self.vault = Some(stage);
        self.upstream.clear();
        Ok(())
    }
    pub fn mark_staged(&mut self, id: AccountId) -> Result<(), String> {
        if self.vault.is_none() || !self.accounts.iter().any(|a| a.id == id) {
            return Err("候选账号环境无效".into());
        }
        self.staged.insert(id);
        Ok(())
    }
    pub fn merge_into(&self, latest: &Workspace) -> Result<Workspace, String> {
        if self.accounts.iter().any(|a| latest.account(a.id).is_some())
            || self
                .proxies
                .iter()
                .any(|p| latest.proxy_profile(p.id).is_some())
        {
            return Err("新账号标识发生冲突，请重新导入".into());
        }
        let mut merged = latest.clone();
        merged.proxy_profiles.extend(self.proxies.clone());
        merged.accounts.extend(self.accounts.clone());
        merged.select_account(self.accounts.first().ok_or("没有可导入账号")?.id)?;
        Ok(merged)
    }
    /// Caller holds the UI's short metadata-commit barrier; native views are still about:blank.
    pub fn commit(&mut self, root: &Path, latest: &Workspace) -> Result<Workspace, String> {
        if !self.confirmed
            || self.staged.len() != self.accounts.len()
            || self
                .vault
                .as_ref()
                .is_none_or(|s| s.root != root || s.committed)
        {
            return Err("账号的代理与 Cookie 尚未全部准备完成".into());
        }
        let merged = self.merge_into(latest)?;
        merged
            .save(&root.join("workspace.json"))
            .map_err(|_| "账号数据保存失败，未激活新账号")?;
        self.vault.as_mut().unwrap().committed = true;
        Ok(merged)
    }
}
fn wire_proxy_draft(proxy: &wire::Proxy) -> Result<ProxyDraft, String> {
    let protocol = match proxy.protocol.as_str() {
        "http" => muse_proxy::ProxyProtocol::Http,
        "https" => muse_proxy::ProxyProtocol::Https,
        "socks5" => muse_proxy::ProxyProtocol::Socks5,
        _ => return Err("不支持的代理协议".into()),
    };
    Ok(ProxyDraft {
        enabled: true,
        protocol,
        host: proxy.host.clone(),
        port: proxy.port.to_string(),
        user: proxy
            .auth
            .as_ref()
            .map(|a| a.username.clone())
            .unwrap_or_default(),
        secret: Zeroizing::new(
            proxy
                .auth
                .as_ref()
                .map(|a| a.password.clone())
                .unwrap_or_default(),
        ),
        force_proxy: true,
    })
}
fn device_address(host: &str) -> bool {
    let local = host.trim_end_matches('.').to_ascii_lowercase();
    if local == "localhost"
        || local.ends_with(".localhost")
        || local.ends_with(".local")
        || local.ends_with(".lan")
        || local.ends_with(".home.arpa")
        || !local.contains(['.', ':'])
    {
        return true;
    }
    match local.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
                || ip.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()
                })
        }
        _ => false,
    }
}
pub fn export_payload(
    root: &Path,
    workspace: &Workspace,
    snapshots: &[(AccountId, wire::CookieSet)],
    platform: TargetPlatform,
) -> Result<wire::Payload, String> {
    if snapshots.is_empty() || snapshots.len() > 32 {
        return Err("请选择 1–32 个账号".into());
    }
    let mut payload = wire::Payload {
        format: "muse-account-payload".into(),
        version: 1,
        accounts: Vec::new(),
        proxies: Vec::new(),
    };
    let mut references: HashMap<(Uuid, Option<Uuid>), String> = HashMap::new();
    let mut seen = HashSet::new();
    for (id, cookies) in snapshots {
        if !seen.insert(*id) {
            return Err("账号选择重复".into());
        }
        let account = workspace.account(*id).ok_or("账号已不存在")?;
        if matches!(account.network, AccountNetwork::SubscriptionNode { .. }) {
            return Err(
                "v1 账号包无法表示订阅绑定；未导出。请取消勾选此账号，或主动修改其网络连接。"
                    .into(),
            );
        }
        let proxy_ref = if account.network == AccountNetwork::Direct {
            None
        } else {
            let credential = workspace.proxy_credentials(account)?;
            let reference = if let Some(reference) = references.get(&credential) {
                reference.clone()
            } else {
                let reference = format!("proxy-{}", references.len() + 1);
                let value = ProtectedStore::new(root)
                    .load_selected(credential.0, credential.1)
                    .map_err(|_| "无法读取所选账号的上游代理凭据，导出已停止")?;
                let mut draft = ProxyDraft::from_url(&value, account.network.force_proxy())
                    .map_err(|_| "所选账号的上游代理无效")?;
                let auth = (!draft.user.is_empty()).then(|| wire::Auth {
                    username: std::mem::take(&mut draft.user),
                    password: draft.secret.to_string(),
                });
                payload.proxies.push(wire::Proxy {
                    proxy_ref: reference.clone(),
                    name: account
                        .network
                        .proxy_id()
                        .and_then(|id| workspace.proxy_profile(id))
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| "原账号代理".into()),
                    protocol: draft.protocol.scheme().into(),
                    host: draft.host,
                    port: draft.port.parse().map_err(|_| "代理端口无效")?,
                    auth,
                });
                references.insert(credential, reference.clone());
                reference
            };
            Some(reference)
        };
        payload.accounts.push(wire::Account {
            account_ref: format!("account-{}", payload.accounts.len() + 1),
            name: account.name.clone(),
            network: wire::Network {
                mode: if proxy_ref.is_some() {
                    "proxy"
                } else {
                    "system"
                }
                .into(),
                proxy_ref,
                bypass_policy: actual_policy(platform, account.network).into(),
                source_force_proxy: if account.network == AccountNetwork::Direct {
                    None
                } else {
                    account
                        .transfer_origin
                        .as_ref()
                        .map(|o| o.source_force_proxy)
                        .unwrap_or(Some(account.network.force_proxy()))
                },
            },
            cookies: cookies.clone(),
        });
    }
    wire::validate(&payload, (crate::page_observer::now_ms() / 1000) as i64)
        .map_err(|e| e.message().to_owned())?;
    Ok(payload)
}
#[cfg(test)]
#[path = "transfer_plan_tests.rs"]
mod tests;
