use crate::{mihomo, secrets::ProtectedStore};
use muse_core::{AccountId, AccountNetwork, ProxySubscription, SubscriptionNode, Workspace};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
struct PrivateData {
    url: String,
    labels: HashMap<Uuid, String>,
    #[serde(default)]
    reasons: HashMap<Uuid, String>,
}

#[derive(Deserialize)]
pub struct Entry {
    label: String,
    kind: String,
    node: Option<Value>,
    reason: Option<Restriction>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Restriction {
    NativeParse,
    OtherEgress,
    NotRemote,
    LocalResource,
    TrustModel,
    Tunnel,
    Transport,
    ActiveProbe,
    UnknownOption,
    #[serde(other)]
    Unknown,
}

impl Restriction {
    fn message(&self) -> &'static str {
        match self {
            Self::NativeParse => "内核无法解析此配置；请检查节点参数或协议",
            Self::OtherEgress => "指定了其他出口或接口；请改用固定端点",
            Self::NotRemote => "本地路由动作不能作为账号出口",
            Self::LocalResource => "引用了证书或私钥资源；请改用系统信任链",
            Self::TrustModel => "不能保证标准证书验证；请改用系统信任链",
            Self::Tunnel => "不能确认隧道的唯一对端；请改用固定端点",
            Self::Transport => "此传输未确认可安全隔离；请更换传输设置",
            Self::ActiveProbe => "节点会主动发起连接；请关闭预连接或探测",
            Self::UnknownOption => "内核未识别非空参数；请核对订阅选项",
            Self::Unknown => "无法确认隔离条件；请核对节点配置",
        }
    }
}

fn private(root: &Path, sub: &ProxySubscription) -> Result<PrivateData, String> {
    let secret = ProtectedStore::new(root).load_protected(sub.id, Some(sub.revision))?;
    serde_json::from_str(&secret).map_err(|_| "无法读取受保护的订阅信息".into())
}

pub fn hydrate(root: &Path, workspace: &mut Workspace) {
    for sub in &mut workspace.subscriptions {
        if let Ok(mut data) = private(root, sub) {
            for node in &mut sub.nodes {
                node.reason = data.reasons.remove(&node.id);
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
    let url = url::Url::parse(source).map_err(|_| "订阅地址无效")?;
    let local = url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        });
    if source.len() > 8192
        || !(local || url.scheme() == "https")
        || url.host().is_none()
        || !url[url::Position::BeforeUsername..url::Position::BeforeHost].is_empty()
        || url.fragment().is_some()
    {
        return Err(
            "订阅须使用 HTTPS（数字回环可用 HTTP）；不支持 URL 用户名、片段或重定向".into(),
        );
    }
    let data = mihomo::provider(url.as_str())?;
    let mut entries: Vec<Entry> =
        serde_json::from_slice(&data).map_err(|_| "无法读取 Mihomo 节点列表")?;
    if entries.is_empty() || entries.len() > 1000 {
        return Err("Mihomo 节点列表超出范围".into());
    }
    for entry in &mut entries {
        if entry.label.is_empty()
            || entry.label.contains("://")
            || entry.label.chars().any(char::is_control)
            || entry.label.chars().count() > 96
        {
            entry.label = "未命名节点".into();
        }
        if entry.kind.is_empty() {
            entry.kind = "未知协议".into();
        }
        if entry.node.is_some() == entry.reason.is_some() {
            return Err("Mihomo 节点状态不一致".into());
        }
    }
    Ok(entries)
}

fn encoded(mut node: Value) -> Zeroizing<String> {
    node["skip-cert-verify"] = Value::Bool(false);
    node.sort_all_objects();
    Zeroizing::new(format!("{}{node}", mihomo::NATIVE))
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
    let mut old_groups: HashMap<String, Vec<(Uuid, Uuid, String, bool)>> = HashMap::new();
    let mut retired = Vec::new();
    if let Some(old) = old {
        let mut labels = private(root, old)?.labels;
        retired.push((old.id, old.revision));
        for node in &old.nodes {
            if let Some(rev) = node.revision {
                let stored = store.load_selected(node.id, Some(rev))?;
                retired.push((node.id, rev));
                if let Ok(config) = mihomo::snapshot(&stored) {
                    old_groups
                        .entry(encoded(config).to_string())
                        .or_default()
                        .push((
                            node.id,
                            rev,
                            labels.remove(&node.id).unwrap_or_default(),
                            stored.starts_with(mihomo::NATIVE),
                        ));
                }
            }
        }
    }
    let mut counts = HashMap::new();
    let mut label_counts = HashMap::new();
    let entries: Vec<_> = entries
        .into_iter()
        .map(|entry| {
            let value = entry.node.map(encoded);
            if let Some(value) = &value {
                *counts.entry(value.to_string()).or_insert(0usize) += 1;
                *label_counts
                    .entry((value.to_string(), entry.label.clone()))
                    .or_insert(0usize) += 1;
            }
            (entry.label, entry.kind, value, entry.reason)
        })
        .collect();
    let mut sub = ProxySubscription {
        id: existing.unwrap_or_else(Uuid::new_v4),
        revision: Uuid::new_v4(),
        name: name.trim().into(),
        nodes: Vec::new(),
    };
    let mut data = PrivateData {
        url: url.into(),
        labels: HashMap::new(),
        reasons: HashMap::new(),
    };
    let mut staged = Vec::new();
    let mut kept = HashSet::new();
    let joined: HashSet<_> = old
        .into_iter()
        .flat_map(|sub| &sub.nodes)
        .filter(|node| node.selectable())
        .map(|node| node.id)
        .collect();
    let result = (|| {
        for (label, kind, value, restriction) in entries {
            let mut id = Uuid::new_v4();
            let mut revision = None;
            let reason = restriction.map(|category| category.message().to_owned());
            if let Some(value) = value {
                if value.len() > 256 * 1024 {
                    return Err("单个节点超过安全存储上限".into());
                }
                let previous = old_groups.get(value.as_str()).and_then(|group| {
                    if group.len() == 1 && counts[value.as_str()] == 1 {
                        return group.first();
                    }
                    if label_counts[&(value.to_string(), label.clone())] != 1 {
                        return None;
                    }
                    let mut matched = group.iter().filter(|row| row.2 == label);
                    let first = matched.next()?;
                    matched.next().is_none().then_some(first)
                });
                if let Some((old_id, old_rev, _, native)) = previous {
                    id = *old_id;
                    if *native {
                        revision = Some(*old_rev);
                        kept.insert((id, *old_rev));
                    }
                }
                if revision.is_none() {
                    let rev = Uuid::new_v4();
                    store.save_revision(id, rev, &value)?;
                    staged.push((id, rev));
                    revision = Some(rev);
                }
            }
            data.labels.insert(id, label.clone());
            if let Some(reason) = &reason {
                data.reasons.insert(id, reason.clone());
            }
            sub.nodes.push(SubscriptionNode {
                in_pool: Some(joined.contains(&id)),
                id,
                revision,
                name: label,
                protocol: kind,
                reason,
            });
        }
        let secret = Zeroizing::new(serde_json::to_string(&data).map_err(|_| "订阅保存失败")?);
        store.save_protected(sub.id, Some(sub.revision), &secret)?;
        staged.push((sub.id, sub.revision));
        retired.retain(|pair| !kept.contains(pair));
        commit(root, workspace, existing, Some(sub), &retired)
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
