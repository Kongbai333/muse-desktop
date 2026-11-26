use muse_core::{Account, AccountId};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};
use uuid::Uuid;

const FILE: &str = "account-groups.json";
pub const UNGROUPED: &str = "未分组";

#[derive(Clone, Serialize, Deserialize)]
pub struct AccountGroup {
    pub id: Uuid,
    pub name: String,
    pub collapsed: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountGroups {
    pub groups: Vec<AccountGroup>,
    pub ungrouped_collapsed: bool,
    memberships: HashMap<AccountId, Uuid>,
    account_order: Vec<AccountId>,
}

impl AccountGroups {
    pub fn load(root: &Path) -> Result<Self, String> {
        let data = match std::fs::read(root.join(FILE)) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => return Err("无法读取账号分组".into()),
        };
        serde_json::from_slice(&data).map_err(|_| "账号分组文件有误，已禁止覆盖保存".into())
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(self).map_err(|_| "无法序列化账号分组")?;
        crate::account_cache::write(root, FILE, &data).map_err(|_| "无法保存账号分组".into())
    }

    pub fn group(&self, id: Uuid) -> Option<&AccountGroup> {
        self.groups.iter().find(|group| group.id == id)
    }

    pub fn membership(&self, account: AccountId) -> Option<Uuid> {
        self.memberships
            .get(&account)
            .copied()
            .filter(|id| self.group(*id).is_some())
    }

    pub fn save_name(&mut self, id: Option<Uuid>, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
            return Err("请输入 1–40 个字符的分组名称".into());
        }
        let key = name.to_lowercase();
        if name == UNGROUPED
            || self
                .groups
                .iter()
                .any(|group| Some(group.id) != id && group.name.to_lowercase() == key)
        {
            return Err("该分组名称已存在".into());
        }
        if let Some(id) = id {
            let group = self
                .groups
                .iter_mut()
                .find(|group| group.id == id)
                .ok_or("分组已不存在")?;
            group.name = name.to_owned();
        } else {
            self.groups.push(AccountGroup {
                id: Uuid::new_v4(),
                name: name.to_owned(),
                collapsed: false,
            });
        }
        Ok(())
    }

    pub fn ordered_accounts<'a>(&self, accounts: &'a [Account]) -> Vec<&'a Account> {
        let ranks: HashMap<_, _> = self.account_order.iter().zip(0usize..).collect();
        let mut ordered: Vec<_> = accounts.iter().collect();
        ordered.sort_by_key(|account| ranks.get(&account.id).copied().unwrap_or(usize::MAX));
        ordered
    }

    pub fn place_account(
        &mut self,
        accounts: &[Account],
        account: AccountId,
        group: Option<Uuid>,
        neighbor: Option<(AccountId, bool)>,
    ) -> bool {
        if !accounts.iter().any(|item| item.id == account)
            || group.is_some_and(|id| self.group(id).is_none())
        {
            return false;
        }
        let mut ordered: Vec<_> = self
            .ordered_accounts(accounts)
            .into_iter()
            .map(|item| item.id)
            .collect();
        let mut destination: Vec<_> = ordered
            .iter()
            .copied()
            .filter(|id| self.membership(*id) == group)
            .collect();
        let original = destination.iter().position(|id| *id == account);
        destination.retain(|id| *id != account);
        let index = match neighbor {
            Some((id, after)) => match destination.iter().position(|item| *item == id) {
                Some(index) => index + usize::from(after),
                None => return false,
            },
            None => destination.len(),
        };
        if original == Some(index) {
            return false;
        }
        ordered.retain(|id| *id != account);
        let insertion = destination
            .get(index)
            .and_then(|next| ordered.iter().position(|id| id == next))
            .unwrap_or(ordered.len());
        ordered.insert(insertion, account);
        self.account_order = ordered;
        if let Some(group) = group {
            self.memberships.insert(account, group);
        } else {
            self.memberships.remove(&account);
        }
        true
    }

    pub fn place_group(&mut self, id: Uuid, neighbor: Uuid, after: bool) -> bool {
        let Some(from) = self.groups.iter().position(|group| group.id == id) else {
            return false;
        };
        let Some(to) = self.groups.iter().position(|group| group.id == neighbor) else {
            return false;
        };
        let insertion = to + usize::from(after) - usize::from(from < to);
        if id == neighbor || from == insertion {
            return false;
        }
        let group = self.groups.remove(from);
        self.groups.insert(insertion, group);
        true
    }

    pub fn remove_group(&mut self, id: Uuid) {
        self.groups.retain(|group| group.id != id);
        self.memberships.retain(|_, group| *group != id);
    }

    pub fn toggle(&mut self, id: Option<Uuid>) {
        if let Some(id) = id {
            if let Some(group) = self.groups.iter_mut().find(|group| group.id == id) {
                group.collapsed = !group.collapsed;
            }
        } else {
            self.ungrouped_collapsed = !self.ungrouped_collapsed;
        }
    }
}
