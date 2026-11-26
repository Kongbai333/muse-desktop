use crate::secrets::ProtectedStore;
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use muse_core::{AccountId, AccountNetwork, ProxySubscription, SubscriptionNode, Workspace};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::Path,
    time::Duration,
};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

const LIMIT: usize = 1024 * 1024;
const MAX_NODES: usize = 1000;

#[derive(Serialize, Deserialize)]
struct PrivateData {
    url: String,
    labels: HashMap<Uuid, String>,
}

pub struct Entry {
    label: String,
    protocol: &'static str,
    endpoint: Option<Zeroizing<String>>,
}

fn private(root: &Path, sub: &ProxySubscription) -> Result<PrivateData, String> {
    let secret = ProtectedStore::new(root).load_protected(sub.id, Some(sub.revision))?;
    serde_json::from_str(&secret).map_err(|_| "无法读取受保护的订阅信息".into())
}

pub fn hydrate(root: &Path, workspace: &mut Workspace) {
    for sub in &mut workspace.subscriptions {
        if let Ok(mut data) = private(root, sub) {
            for node in &mut sub.nodes {
                node.name = data
                    .labels
                    .remove(&node.id)
                    .unwrap_or_else(|| "未命名节点".into());
            }
        }
    }
}

pub fn source(root: &Path, sub: &ProxySubscription) -> Result<Zeroizing<String>, String> {
    Ok(Zeroizing::new(private(root, sub)?.url))
}

pub fn fetch(source: &str) -> Result<Vec<Entry>, String> {
    let url = Url::parse(source).map_err(|_| "订阅地址无效")?;
    let loopback = url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        });
    if source.len() > 8192
        || (url.scheme() != "https" && !loopback)
        || url.host().is_none()
        || !url[url::Position::BeforeUsername..url::Position::BeforeHost].is_empty()
        || url.fragment().is_some()
    {
        return Err(
            "订阅须使用 HTTPS（数字回环地址可使用 HTTP），不支持 URL 用户名或重定向".into(),
        );
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();
    let response = agent
        .get(url.as_str())
        .call()
        .map_err(|_| "订阅下载失败；已保留上次节点")?;
    if !response.status().is_success() {
        return Err("订阅响应失败或重定向；已保留上次节点".into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    response
        .into_body()
        .into_reader()
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "订阅读取失败")?;
    if bytes.len() > LIMIT {
        return Err("订阅超过 1 MiB".into());
    }
    parse(std::str::from_utf8(&bytes).map_err(|_| "订阅不是 UTF-8")?)
}

fn decode(value: &str) -> Option<String> {
    [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|engine| {
            engine
                .decode(value)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        })
}
fn label(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) || value.contains("://") {
        return "未命名节点".into();
    }
    value.chars().take(96).collect()
}
fn protocol(value: &str) -> &'static str {
    match value {
        "ss" => "ss",
        "http" => "http",
        "https" => "https",
        "socks" | "socks5" | "socks5h" => "socks5",
        "vmess" => "vmess",
        "vless" => "vless",
        "trojan" => "trojan",
        "hysteria2" | "hy2" => "hysteria2",
        _ => "未知协议",
    }
}
fn encoded(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}
fn unescape(value: &str) -> Option<String> {
    percent_encoding::percent_decode_str(value)
        .decode_utf8()
        .ok()
        .map(|s| s.into_owned())
}

fn endpoint(
    kind: &str,
    host: &str,
    port: u16,
    user: &str,
    secret: &str,
) -> Option<Zeroizing<String>> {
    if port == 0
        || host.is_empty()
        || host
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || "@/?#%\\".contains(ch))
        || (user.is_empty() && !secret.is_empty())
    {
        return None;
    }
    let host = if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::Ipv6Addr>() {
        format!("[{ip}]")
    } else {
        url::Host::parse(host).ok()?.to_string()
    };
    let auth = if user.is_empty() {
        String::new()
    } else {
        format!("{}:{}@", encoded(user), encoded(secret))
    };
    let value = Zeroizing::new(format!("{kind}://{auth}{host}:{port}/"));
    muse_proxy::Upstream::parse(&value).ok()?;
    Some(value)
}

fn share(line: &str) -> Entry {
    let (uri, fragment) = line.split_once('#').unwrap_or((line, ""));
    let (scheme, body) = uri.split_once("://").unwrap_or(("", ""));
    let kind = protocol(scheme);
    let route = (|| {
        if uri.len() > 16384 || uri.contains('?') || uri.chars().any(char::is_control) {
            return None;
        }
        if kind == "ss" {
            let legacy;
            let (auth, addr) = if let Some(parts) = body.rsplit_once('@') {
                parts
            } else {
                legacy = decode(body)?;
                legacy.rsplit_once('@')?
            };
            let auth = if auth.contains(':') {
                unescape(auth)?
            } else {
                decode(&unescape(auth)?)?
            };
            let (cipher, secret) = auth.split_once(':')?;
            let addr = Url::parse(&format!("ss://{addr}")).ok()?;
            if addr.query().is_some()
                || !matches!(addr.path(), "" | "/")
                || !addr.username().is_empty()
                || addr.fragment().is_some()
            {
                return None;
            }
            endpoint(kind, addr.host_str()?, addr.port()?, cipher, secret)
        } else if matches!(kind, "https" | "http" | "socks5") {
            let proxy = muse_proxy::Upstream::parse(uri).ok()?;
            let (user, secret) = proxy.credentials().unwrap_or(("", ""));
            endpoint(kind, proxy.host(), proxy.port(), user, secret)
        } else {
            None
        }
    })();
    Entry {
        label: label(&unescape(fragment).unwrap_or_default()),
        protocol: kind,
        endpoint: route,
    }
}

fn clash(value: &Value) -> Entry {
    let kind = protocol(value.get("type").and_then(Value::as_str).unwrap_or(""));
    let mut kind = kind;
    let route = (|| {
        let map = value.as_mapping()?;
        for (key, value) in map {
            match key.as_str()? {
                "name" | "type" | "server" | "port" => (),
                "udp" if value.is_bool() => (),
                "network" if value.as_str() == Some("tcp") => (),
                "cipher" if kind == "ss" => (),
                "password" => (),
                "username" if kind != "ss" => (),
                "tls" if kind == "http" && value.is_bool() => (),
                "skip-cert-verify" if value.as_bool() == Some(false) => (),
                _ => return None,
            }
        }
        if kind == "http" && value.get("tls").and_then(Value::as_bool) == Some(true) {
            kind = "https";
        }
        let string = |key: &str| -> Option<&str> {
            match value.get(key) {
                Some(field) => field.as_str(),
                None => Some(""),
            }
        };
        endpoint(
            kind,
            string("server")?,
            value.get("port")?.as_u64()?.try_into().ok()?,
            string(if kind == "ss" { "cipher" } else { "username" })?,
            string("password")?,
        )
    })();
    Entry {
        label: label(value.get("name").and_then(Value::as_str).unwrap_or("")),
        protocol: kind,
        endpoint: route,
    }
}

fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let text = text.trim_start_matches('\u{feff}').trim();
    #[derive(Deserialize)]
    struct Clash {
        proxies: Vec<Value>,
    }
    if let Ok(doc) = serde_yaml_ng::from_str::<Clash>(text) {
        if doc.proxies.len() > MAX_NODES {
            return Err("订阅超过 1000 个节点".into());
        }
        return Ok(doc.proxies.iter().map(clash).collect());
    }
    let decoded;
    let text = if text.contains("://") {
        text
    } else {
        decoded = Zeroizing::new(
            decode(&text.split_ascii_whitespace().collect::<String>())
                .ok_or("无法解析订阅；保留上次节点")?,
        );
        decoded.as_str()
    };
    let mut entries = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        if !line.split_once("://").is_some_and(|(kind, _)| {
            kind.len() <= 20
                && !kind.is_empty()
                && kind.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) {
            return Err("无法解析节点列表；保留上次节点".into());
        }
        if entries.len() >= MAX_NODES {
            return Err("订阅超过 1000 个节点".into());
        }
        entries.push(share(line));
    }
    (!entries.is_empty())
        .then_some(entries)
        .ok_or_else(|| "无法识别订阅内容；保留上次节点".into())
}

pub fn replace(
    root: &Path,
    workspace: &mut Workspace,
    existing: Option<Uuid>,
    name: &str,
    url: &str,
    entries: Vec<Entry>,
) -> Result<Vec<AccountId>, String> {
    if name.trim().is_empty()
        || name.chars().count() > 48
        || name.chars().any(char::is_control)
        || name.contains("://")
    {
        return Err("请填写 1–48 字的订阅名称".into());
    }
    let old = existing
        .map(|id| {
            workspace
                .subscriptions
                .iter()
                .find(|sub| sub.id == id)
                .ok_or("订阅已删除")
        })
        .transpose()?;
    if old.is_none() && workspace.subscriptions.len() >= 64 {
        return Err("最多保存 64 个订阅".into());
    }
    let store = ProtectedStore::new(root);
    let mut known = HashMap::new();
    let mut discard = Vec::new();
    if let Some(old) = old {
        discard.push((old.id, old.revision));
        for node in &old.nodes {
            if let Some(revision) = node.revision {
                let value = store.load_selected(node.id, Some(revision))?;
                known.insert(value.to_string(), (node.id, revision));
            }
        }
    }
    let mut sub = ProxySubscription {
        id: existing.unwrap_or_else(Uuid::new_v4),
        revision: Uuid::new_v4(),
        name: name.trim().into(),
        nodes: Vec::new(),
    };
    let mut staged = Vec::new();
    let mut labels = HashMap::new();
    let mut seen = HashSet::new();
    let result = (|| {
        for entry in entries {
            let (id, revision) = if let Some(value) = entry.endpoint {
                if !seen.insert(value.to_string()) {
                    continue;
                }
                let (id, rev) = if let Some(pair) = known.remove(value.as_str()) {
                    pair
                } else {
                    let pair = (Uuid::new_v4(), Uuid::new_v4());
                    store.save_revision(pair.0, pair.1, &value)?;
                    staged.push(pair);
                    pair
                };
                (id, Some(rev))
            } else {
                (Uuid::new_v4(), None)
            };
            labels.insert(id, entry.label.clone());
            sub.nodes.push(SubscriptionNode {
                id,
                revision,
                name: entry.label,
                protocol: entry.protocol.into(),
            });
        }
        let secret = Zeroizing::new(
            serde_json::to_string(&PrivateData {
                url: url.to_owned(),
                labels,
            })
            .map_err(|_| "订阅保存失败")?,
        );
        store.save_protected(sub.id, Some(sub.revision), &secret)?;
        staged.push((sub.id, sub.revision));
        discard.extend(known.into_values());
        commit(root, workspace, existing, Some(sub), &discard)
    })();
    if result.is_err() {
        for (id, rev) in staged {
            store.remove_revision(id, rev);
        }
    }
    result
}

fn commit(
    root: &Path,
    workspace: &mut Workspace,
    old: Option<Uuid>,
    new: Option<ProxySubscription>,
    discard: &[(Uuid, Uuid)],
) -> Result<Vec<AccountId>, String> {
    let mut candidate = workspace.clone();
    candidate.subscriptions.retain(|sub| Some(sub.id) != old);
    candidate.subscriptions.extend(new);
    let affected: Vec<_> = candidate
        .accounts
        .iter()
        .filter_map(|account| match account.network {
            AccountNetwork::SubscriptionNode { node_id, .. }
                if discard.iter().any(|(id, _)| *id == node_id) =>
            {
                Some((account.id, account.network))
            }
            _ => None,
        })
        .collect();
    for (id, network) in &affected {
        candidate.update_network(*id, *network, None)?;
    }
    candidate.save(&root.join("workspace.json"))?;
    *workspace = candidate;
    let store = ProtectedStore::new(root);
    for (id, rev) in discard {
        store.remove_revision(*id, *rev);
    }
    Ok(affected.into_iter().map(|(id, _)| id).collect())
}

pub fn remove(root: &Path, workspace: &mut Workspace, id: Uuid) -> Result<Vec<AccountId>, String> {
    let sub = workspace
        .subscriptions
        .iter()
        .find(|sub| sub.id == id)
        .ok_or("订阅不存在")?;
    let mut discard = vec![(id, sub.revision)];
    discard.extend(
        sub.nodes
            .iter()
            .filter_map(|node| node.revision.map(|rev| (node.id, rev))),
    );
    commit(root, workspace, Some(id), None, &discard)
}
