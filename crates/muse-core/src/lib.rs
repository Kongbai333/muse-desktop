//! Account, navigation and quota contracts independent of the native window.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use url::Url;
use uuid::Uuid;
mod extra_quota;
pub use extra_quota::{ExtraQuotaObservation, ExtraQuotaState};
mod display_preferences;
pub use display_preferences::{DisplayPreferences, ThemeMode};
mod browser_locale;
pub use browser_locale::{BROWSER_LANGUAGES, BROWSER_TIMEZONES, BrowserLocale};

pub const HOME_URL: &str = "https://muse.ai/";
pub type AccountId = Uuid;
pub type TabId = Uuid;
pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BrowserIdentity {
    pub account_id: AccountId,
    pub tab_id: TabId,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub enum BrowserEvent {
    Title(String),
    Url(String),
    Loading(bool),
    History { back: bool, forward: bool },
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuotaObservation {
    used: f64,
    limit: f64,
    unit: String,
    observed_at: u64,
    resets_at: Option<u64>,
}

impl QuotaObservation {
    pub fn new(
        used: f64,
        limit: f64,
        unit: &str,
        observed_at: u64,
        resets_at: Option<u64>,
    ) -> Result<Self> {
        let observation = Self {
            used,
            limit,
            unit: unit.into(),
            observed_at,
            resets_at,
        };
        observation.validate()?;
        Ok(observation)
    }

    fn validate(&self) -> Result<()> {
        if !self.used.is_finite()
            || !self.limit.is_finite()
            || self.used < 0.0
            || self.limit <= 0.0
            || self.used > self.limit
            || self.unit.trim().is_empty()
        {
            return Err("无效的额度数据".into());
        }
        Ok(())
    }

    pub fn remaining(&self) -> f64 {
        self.limit - self.used
    }
    pub fn limit(&self) -> f64 {
        self.limit
    }
    pub fn unit(&self) -> &str {
        &self.unit
    }
    pub fn observed_at(&self) -> u64 {
        self.observed_at
    }
    pub fn resets_at(&self) -> Option<u64> {
        self.resets_at
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum QuotaState {
    #[default]
    Unknown,
    Known(QuotaObservation),
    Stale(QuotaObservation),
    Unavailable(String),
    Syncing,
    LoggedOut,
    Unlimited {
        observed_at: u64,
    },
    StaleUnlimited {
        observed_at: u64,
    },
}

impl QuotaState {
    pub fn observed_at(&self) -> Option<u64> {
        match self {
            Self::Known(v) | Self::Stale(v) => Some(v.observed_at),
            Self::Unlimited { observed_at } | Self::StaleUnlimited { observed_at } => {
                Some(*observed_at)
            }
            _ => None,
        }
    }
    pub fn at_time(self, now: u64) -> Self {
        match self {
            Self::Known(value)
                if value.observed_at > now.saturating_add(30_000)
                    || now.saturating_sub(value.observed_at) >= 300_000
                    || value.resets_at.is_some_and(|reset| reset <= now) =>
            {
                Self::Stale(value)
            }
            Self::Unlimited { observed_at }
                if observed_at > now.saturating_add(30_000)
                    || now.saturating_sub(observed_at) >= 300_000 =>
            {
                Self::StaleUnlimited { observed_at }
            }
            state => state,
        }
    }
    pub fn remaining_ratio(&self) -> Option<f64> {
        match self {
            Self::Known(value) if value.validate().is_ok() => Some(value.remaining() / value.limit),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Unknown => "尚未读取额度".into(),
            Self::Known(v) if v.unit == "%" => format!("剩余 {:.0}%", v.remaining()),
            Self::Known(v) => format!("剩余 {:.0}/{:.0} {}", v.remaining(), v.limit, v.unit),
            Self::Stale(v) => format!("{} · 未更新", Self::Known(v.clone()).label()),
            Self::Unavailable(reason) => reason.clone(),
            Self::Syncing => "正在同步额度…".into(),
            Self::LoggedOut => "请先登录 Muse".into(),
            Self::Unlimited { .. } => "当前套餐不限量".into(),
            Self::StaleUnlimited { .. } => "当前套餐不限量 · 未更新".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
    pub title: String,
    #[serde(skip, default = "initial_generation")]
    pub generation: u64,
    #[serde(skip)]
    pub loading: bool,
    #[serde(skip)]
    pub can_go_back: bool,
    #[serde(skip)]
    pub can_go_forward: bool,
    #[serde(skip)]
    pub error: Option<String>,
}

fn initial_generation() -> u64 {
    1
}

impl Tab {
    fn new(url: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            url: url.into(),
            title: "Muse".into(),
            generation: 1,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            error: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub name: String,
    pub tabs: Vec<Tab>,
    active_tab_id: TabId,
    #[serde(skip)]
    pub quota: QuotaState,
    #[serde(skip)]
    pub extra_quota: ExtraQuotaState,
    #[serde(default)]
    pub network: AccountNetwork,
    #[serde(default)]
    pub fingerprint: FingerprintMode,
    #[serde(default, skip_serializing_if = "BrowserLocale::is_system")]
    pub browser_locale: BrowserLocale,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_revision: Option<Uuid>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legacy_proxy_migrated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer_origin: Option<TransferOrigin>,
}

/// Retired metadata retained for old workspaces; never enables browser readback changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FingerprintMode {
    #[default]
    Basic,
    Original,
}

/// Migration provenance only; AccountNetwork and the platform adapter remain authoritative.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferOrigin {
    pub bypass_policy: String,
    pub source_force_proxy: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AccountNetwork {
    #[default]
    Direct,
    HttpProxy {
        force_proxy: bool,
    },
    PoolProxy {
        proxy_id: Uuid,
        force_proxy: bool,
    },
}

impl AccountNetwork {
    pub fn label(self) -> &'static str {
        match self {
            Self::Direct => "系统网络",
            _ if self.force_proxy() => "账号代理 · 强制",
            _ => "账号代理",
        }
    }

    pub fn force_proxy(self) -> bool {
        match self {
            Self::Direct => false,
            Self::HttpProxy { force_proxy } | Self::PoolProxy { force_proxy, .. } => force_proxy,
        }
    }

    pub fn proxy_id(self) -> Option<Uuid> {
        match self {
            Self::PoolProxy { proxy_id, .. } => Some(proxy_id),
            _ => None,
        }
    }
}

impl Account {
    pub fn profile_dir(&self, root: &Path) -> PathBuf {
        root.join(self.id.to_string())
    }
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == self.active_tab_id)
    }
    pub fn active_tab_id(&self) -> TabId {
        self.active_tab_id
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProxyProfile {
    pub id: Uuid,
    pub name: String,
    pub protocol: String,
    pub revision: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workspace {
    version: u32,
    pub accounts: Vec<Account>,
    active_account: Option<AccountId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proxy_profiles: Vec<ProxyProfile>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            version: 1,
            accounts: vec![],
            active_account: None,
            proxy_profiles: Vec::new(),
        }
    }
    pub fn active_account_id(&self) -> Option<AccountId> {
        self.active_account
    }
    pub fn account(&self, id: AccountId) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }
    pub fn active_account(&self) -> Option<&Account> {
        self.active_account.and_then(|id| self.account(id))
    }

    pub fn proxy_profile(&self, id: Uuid) -> Option<&ProxyProfile> {
        self.proxy_profiles.iter().find(|profile| profile.id == id)
    }

    pub fn proxy_credentials(&self, account: &Account) -> Result<(Uuid, Option<Uuid>)> {
        match account.network.proxy_id() {
            Some(id) => self
                .proxy_profile(id)
                .map(|profile| (id, profile.revision))
                .ok_or_else(|| "账号关联的代理已不存在；未使用直连".into()),
            None => Ok((account.id, account.proxy_revision)),
        }
    }

    pub fn add_account(&mut self, name: &str) -> AccountId {
        let tab = Tab::new(HOME_URL);
        let id = Uuid::new_v4();
        self.accounts.push(Account {
            id,
            name: name.trim().chars().take(40).collect(),
            active_tab_id: tab.id,
            tabs: vec![tab],
            quota: QuotaState::Unknown,
            extra_quota: ExtraQuotaState::Unknown,
            network: AccountNetwork::Direct,
            fingerprint: FingerprintMode::Basic,
            browser_locale: BrowserLocale::default(),
            legacy_id: None,
            proxy_revision: None,
            legacy_proxy_migrated: false,
            transfer_origin: None,
        });
        self.active_account = Some(id);
        id
    }

    pub fn select_account(&mut self, id: AccountId) -> Result<()> {
        if self.account(id).is_none() {
            return Err("账号不存在".into());
        }
        self.active_account = Some(id);
        Ok(())
    }

    pub fn update_network(
        &mut self,
        id: AccountId,
        network: AccountNetwork,
        revision: Option<Uuid>,
    ) -> Result<()> {
        let account = self
            .accounts
            .iter_mut()
            .find(|account| account.id == id)
            .ok_or("账号不存在")?;
        if account.tabs.iter().any(|tab| tab.generation == u64::MAX) {
            return Err("页面代次已耗尽".into());
        }
        account.network = network;
        account.proxy_revision = revision;
        for tab in &mut account.tabs {
            tab.generation += 1;
            tab.loading = false;
            tab.can_go_back = false;
            tab.can_go_forward = false;
            tab.error = None;
        }
        Ok(())
    }

    pub fn set_fingerprint_mode(&mut self, id: AccountId, mode: FingerprintMode) -> Result<bool> {
        let account = self.account(id).ok_or("账号不存在")?;
        if account.fingerprint == mode {
            return Ok(false);
        }
        let (network, revision) = (account.network, account.proxy_revision);
        self.update_network(id, network, revision)?;
        self.accounts
            .iter_mut()
            .find(|a| a.id == id)
            .unwrap()
            .fingerprint = mode;
        Ok(true)
    }

    pub fn set_browser_locale(&mut self, id: AccountId, locale: BrowserLocale) -> Result<bool> {
        locale.validate()?;
        let account = self.account(id).ok_or("账号不存在")?;
        if account.browser_locale == locale {
            return Ok(false);
        }
        let (network, revision) = (account.network, account.proxy_revision);
        // Invalidate every tab generation, while retaining profile/identity/route.
        self.update_network(id, network, revision)?;
        self.accounts
            .iter_mut()
            .find(|a| a.id == id)
            .unwrap()
            .browser_locale = locale;
        Ok(true)
    }

    pub fn register_legacy(
        &mut self,
        id: AccountId,
        legacy_id: u64,
        network: AccountNetwork,
    ) -> Result<()> {
        if self.accounts.iter().any(|a| a.legacy_id == Some(legacy_id)) {
            return Err("此旧版账号已导入".into());
        }
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or("账号不存在")?;
        account.legacy_id = Some(legacy_id);
        account.network = network;
        Ok(())
    }

    pub fn rename_account(&mut self, id: AccountId, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err("账号名称不能为空".into());
        }
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or("账号不存在")?;
        account.name = name.chars().take(40).collect();
        Ok(())
    }

    pub fn remove_account(&mut self, id: AccountId) -> Result<()> {
        let index = self
            .accounts
            .iter()
            .position(|a| a.id == id)
            .ok_or("账号不存在")?;
        self.accounts.remove(index);
        if self.active_account == Some(id) {
            self.active_account = self.accounts.first().map(|a| a.id);
        }
        Ok(())
    }

    pub fn add_tab(&mut self, account_id: AccountId, url: &str) -> Result<TabId> {
        let url = normalize_web_url(url)?;
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.id == account_id)
            .ok_or("账号不存在")?;
        let tab = Tab::new(&url);
        let id = tab.id;
        account.tabs.push(tab);
        account.active_tab_id = id;
        Ok(id)
    }

    pub fn open_popup(&mut self, opener: BrowserIdentity, url: &str) -> Result<TabId> {
        if !self.contains_identity(opener) {
            return Err("弹窗来源页面已失效".into());
        }
        let was_active = self.active_browser_identity() == Some(opener);
        let previous_tab = self
            .account(opener.account_id)
            .map(|account| account.active_tab_id);
        let id = self.add_tab(opener.account_id, url)?;
        if !was_active
            && let Some(account) = self.accounts.iter_mut().find(|a| a.id == opener.account_id)
            && let Some(previous) = previous_tab
        {
            account.active_tab_id = previous;
        }
        Ok(id)
    }

    pub fn select_tab(&mut self, tab_id: TabId) -> Result<()> {
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.tabs.iter().any(|t| t.id == tab_id))
            .ok_or("页面不存在")?;
        account.active_tab_id = tab_id;
        self.active_account = Some(account.id);
        Ok(())
    }

    pub fn close_tab(&mut self, tab_id: TabId) -> Result<()> {
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.tabs.iter().any(|t| t.id == tab_id))
            .ok_or("页面不存在")?;
        let index = account
            .tabs
            .iter()
            .position(|t| t.id == tab_id)
            .ok_or("页面不存在")?;
        account.tabs.remove(index);
        if account.tabs.is_empty() {
            account.tabs.push(Tab::new(HOME_URL));
        }
        if account.active_tab_id == tab_id {
            account.active_tab_id = account.tabs[index.min(account.tabs.len() - 1)].id;
        }
        Ok(())
    }

    pub fn restart_tab(&mut self, id: TabId) -> Result<()> {
        let tab = self
            .accounts
            .iter_mut()
            .flat_map(|a| &mut a.tabs)
            .find(|t| t.id == id)
            .ok_or("页面不存在")?;
        tab.generation = tab.generation.checked_add(1).ok_or("页面代次已耗尽")?;
        tab.loading = true;
        tab.can_go_back = false;
        tab.can_go_forward = false;
        tab.error = None;
        Ok(())
    }

    pub fn active_browser_identity(&self) -> Option<BrowserIdentity> {
        let account = self.active_account()?;
        let tab = account.active_tab()?;
        Some(BrowserIdentity {
            account_id: account.id,
            tab_id: tab.id,
            generation: tab.generation,
        })
    }

    pub fn contains_identity(&self, identity: BrowserIdentity) -> bool {
        self.account(identity.account_id).is_some_and(|a| {
            a.tabs
                .iter()
                .any(|t| t.id == identity.tab_id && t.generation == identity.generation)
        })
    }

    pub fn apply_browser_event(&mut self, identity: BrowserIdentity, event: BrowserEvent) -> bool {
        if !self.contains_identity(identity) {
            return false;
        }
        let Some(tab) = self
            .accounts
            .iter_mut()
            .find(|a| a.id == identity.account_id)
            .and_then(|a| a.tabs.iter_mut().find(|t| t.id == identity.tab_id))
        else {
            return false;
        };
        match event {
            BrowserEvent::Title(title) => tab.title = title.chars().take(100).collect(),
            BrowserEvent::Url(url) => match normalize_web_url(&url) {
                Ok(url) => {
                    tab.url = url;
                    tab.error = None;
                }
                Err(_) => return false,
            },
            BrowserEvent::Loading(loading) => {
                tab.loading = loading;
                if loading {
                    tab.error = None;
                }
            }
            BrowserEvent::History { back, forward } => {
                tab.can_go_back = back;
                tab.can_go_forward = forward;
            }
            BrowserEvent::Error(error) => {
                tab.error = Some(error);
                tab.loading = false;
            }
        }
        true
    }

    pub fn apply_quota(
        &mut self,
        identity: BrowserIdentity,
        source_url: &str,
        observation: QuotaObservation,
    ) -> bool {
        if !trusted_muse_url(source_url)
            || observation.validate().is_err()
            || !self.contains_identity(identity)
        {
            return false;
        }
        if let Some(account) = self
            .accounts
            .iter_mut()
            .find(|a| a.id == identity.account_id)
        {
            if account
                .quota
                .observed_at()
                .is_some_and(|time| observation.observed_at < time)
            {
                return false;
            }
            account.quota = QuotaState::Known(observation);
            return true;
        }
        false
    }

    pub fn apply_quota_status(
        &mut self,
        identity: BrowserIdentity,
        source_url: &str,
        mut state: QuotaState,
    ) -> bool {
        if !trusted_muse_url(source_url)
            || !self.contains_identity(identity)
            || matches!(state, QuotaState::Known(_) | QuotaState::Stale(_))
        {
            return false;
        }
        if let Some(account) = self
            .accounts
            .iter_mut()
            .find(|a| a.id == identity.account_id)
        {
            if state
                .observed_at()
                .zip(account.quota.observed_at())
                .is_some_and(|(incoming, current)| incoming < current)
            {
                return false;
            }
            if account.quota == state {
                return false;
            }
            // Failure/absence is not a new successful observation. Keep the
            // original value and timestamp; logout remains authoritative.
            if matches!(
                state,
                QuotaState::Unknown | QuotaState::Syncing | QuotaState::Unavailable(_)
            ) {
                state = match &account.quota {
                    QuotaState::Known(q) | QuotaState::Stale(q) => QuotaState::Stale(q.clone()),
                    QuotaState::Unlimited { observed_at }
                    | QuotaState::StaleUnlimited { observed_at } => QuotaState::StaleUnlimited {
                        observed_at: *observed_at,
                    },
                    _ => state,
                };
            }
            if matches!(state, QuotaState::LoggedOut) {
                account.extra_quota = ExtraQuotaState::Unknown;
            }
            account.quota = state;
            return true;
        }
        false
    }

    pub fn apply_extra_quota(
        &mut self,
        identity: BrowserIdentity,
        source_url: &str,
        observation: ExtraQuotaObservation,
    ) -> bool {
        if !trusted_muse_url(source_url) || !self.contains_identity(identity) {
            return false;
        }
        let Some(account) = self
            .accounts
            .iter_mut()
            .find(|a| a.id == identity.account_id)
        else {
            return false;
        };
        if matches!(account.quota, QuotaState::LoggedOut)
            || account
                .quota
                .observed_at()
                .is_some_and(|at| at > observation.observed_at())
            || account
                .extra_quota
                .observed_at()
                .is_some_and(|at| at >= observation.observed_at())
        {
            return false;
        }
        account.extra_quota = ExtraQuotaState::Known(observation);
        true
    }

    pub fn expire_quotas(&mut self, now: u64) -> bool {
        let mut changed = false;
        for account in &mut self.accounts {
            let state = account.quota.clone().at_time(now);
            changed |= state != account.quota;
            account.quota = state;
            let extra = account.extra_quota.clone().at_time(now);
            changed |= extra != account.extra_quota;
            account.extra_quota = extra;
        }
        changed
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let workspace: Self =
            serde_json::from_str(json).map_err(|e| format!("账号文件无法读取：{e}"))?;
        if workspace.version != 1 {
            return Err("不支持的账号文件版本".into());
        }
        let mut account_ids = std::collections::HashSet::new();
        let mut tab_ids = std::collections::HashSet::new();
        let mut proxy_ids = std::collections::HashSet::new();
        for profile in &workspace.proxy_profiles {
            if !proxy_ids.insert(profile.id)
                || profile.name.trim().is_empty()
                || !matches!(profile.protocol.as_str(), "http" | "https" | "socks5")
            {
                return Err("账号文件中的代理配置无效".into());
            }
        }
        for account in &workspace.accounts {
            if !account_ids.insert(account.id)
                || account.tabs.is_empty()
                || account.active_tab().is_none()
            {
                return Err("账号文件中的账号或页面标识无效".into());
            }
            for tab in &account.tabs {
                if !tab_ids.insert(tab.id) || normalize_web_url(&tab.url).is_err() {
                    return Err("账号文件中存在重复页面或无效网址".into());
                }
            }
        }
        if workspace
            .active_account
            .is_some_and(|id| workspace.account(id).is_none())
        {
            return Err("账号文件中的选中账号不存在".into());
        }
        Ok(workspace)
    }

    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(json) => Self::from_json(&json),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(error) => Err(format!("无法读取账号文件：{error}")),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path.parent().ok_or("账号文件路径无效")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!(".workspace-{}.tmp", Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(self.to_json().map_err(std::io::Error::other)?.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|e| format!("账号数据保存失败：{e}"))
    }
}

pub fn trusted_muse_url(input: &str) -> bool {
    Url::parse(input).ok().is_some_and(|url| {
        url.scheme() == "https"
            && matches!(url.host_str(), Some("muse.ai" | "www.muse.ai"))
            && url.port_or_known_default() == Some(443)
            && url[url::Position::BeforeUsername..url::Position::BeforeHost].is_empty()
    })
}

pub fn normalize_web_url(input: &str) -> Result<String> {
    let input = input.trim();
    let url = Url::parse(input).map_err(|_| "网址格式无效")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url[url::Position::BeforeUsername..url::Position::BeforeHost].is_empty()
    {
        return Err("只允许不含账号密码的 HTTP/HTTPS 网址".into());
    }
    Ok(url.into())
}
