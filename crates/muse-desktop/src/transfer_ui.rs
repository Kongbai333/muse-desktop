use super::*;
use crate::{
    account_transfer as wire,
    transfer_flow::{JobGate, confirmed_password},
    transfer_plan::{ImportPlan, TargetPlatform},
};
use std::{collections::VecDeque, path::PathBuf};
use uuid::Uuid;
use zeroize::Zeroizing;
#[cfg(debug_assertions)]
#[path = "transfer_ui_smoke.rs"]
mod smoke;
#[path = "transfer_view.rs"]
mod view;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Form,
    Picking,
    Snapshot,
    Reading,
    Preview,
    Vault,
    Staging,
    Committing,
    Exporting,
    Done,
}
enum Reply {
    Picked(Uuid, Option<PathBuf>),
    Decoded(Uuid, Result<ImportPlan, String>),
    Vault(Uuid, ImportPlan, Result<(), String>),
    Committed(Uuid, ImportPlan, Result<Workspace, String>),
    Exported(Uuid, Result<(), String>),
}
impl Reply {
    fn id(&self) -> Uuid {
        match self {
            Self::Picked(id, _)
            | Self::Decoded(id, _)
            | Self::Vault(id, ..)
            | Self::Committed(id, ..)
            | Self::Exported(id, _) => *id,
        }
    }
}
pub(super) struct TransferDialog {
    export: bool,
    phase: Phase,
    gate: JobGate,
    password: Entity<TextInput>,
    confirmation: Entity<TextInput>,
    selected: HashSet<AccountId>,
    path: Option<PathBuf>,
    message: String,
    plan: Option<ImportPlan>,
    acknowledged: bool,
    next: usize,
    queue: VecDeque<AccountId>,
    snapshots: Vec<(AccountId, wire::CookieSet)>,
    snapshot_workspace: Option<Workspace>,
    pending_password: Option<Zeroizing<String>>,
    sender: mpsc::Sender<Reply>,
    receiver: mpsc::Receiver<Reply>,
}
impl TransferDialog {
    fn new(export: bool, owner: AccountId, cx: &mut Context<MuseWindow>) -> Self {
        let input = |cx: &mut Context<TextInput>| {
            let mut input = TextInput::new("", cx);
            input.set_secure(true, cx);
            input.set_exact_paste(true);
            input.set_placeholder("迁移密码（至少 8 个 UTF-8 字节）");
            input
        };
        let (sender, receiver) = mpsc::channel();
        Self {
            export,
            phase: Phase::Form,
            gate: JobGate::new(),
            password: cx.new(input),
            confirmation: cx.new(input),
            selected: HashSet::from([owner]),
            path: None,
            message: "仅迁移静态代理配置；请妥善保存迁移密码，应用不会保存或找回密码。".into(),
            plan: None,
            acknowledged: false,
            next: 0,
            queue: VecDeque::new(),
            snapshots: Vec::new(),
            snapshot_workspace: None,
            pending_password: None,
            sender,
            receiver,
        }
    }
    fn clear_inputs(&mut self, cx: &mut App) {
        self.password.update(cx, |i, cx| i.set_text("", cx));
        self.confirmation.update(cx, |i, cx| i.set_text("", cx));
    }
    fn error(&mut self, message: String) {
        self.message = message;
        self.phase = Phase::Form;
        self.gate.finish();
        self.gate = JobGate::new();
        self.pending_password = None;
        self.queue.clear();
        self.snapshots.clear();
        self.snapshot_workspace = None;
        self.acknowledged = false;
    }
}
impl MuseWindow {
    pub(super) fn transfer_commit_pending(&self) -> bool {
        self.transfer_dialog
            .as_ref()
            .is_some_and(|d| d.phase == Phase::Committing)
    }
    pub(super) fn open_transfer(
        &mut self,
        export: bool,
        owner: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.updates.open
            || self.transfer_dialog.is_some()
            || self.account_dialog.is_some()
            || self.cookie_dialog.is_some()
        {
            return;
        }
        if !self.save_enabled {
            self.status = "账号文件需要先修复，当前不能迁移账号".into();
            cx.notify();
            return;
        }
        self.transfer_dialog = Some(TransferDialog::new(export, owner, cx));
        self.ensure_page_freeze();
        self.browsers
            .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
        window.focus(&self.focus);
        cx.notify();
        if !export {
            self.pick_transfer_file(window, cx);
        }
    }
    pub(super) fn close_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .transfer_dialog
            .as_ref()
            .is_some_and(|d| d.phase == Phase::Committing || d.gate.committing())
        {
            return;
        }
        self.clear_transfer(cx);
        window.focus(&self.focus);
        self.ensure_active(window);
        cx.notify();
    }
    pub(super) fn clear_transfer(&mut self, cx: &mut App) {
        if self
            .transfer_dialog
            .as_ref()
            .is_some_and(|d| d.phase == Phase::Committing || d.gate.committing())
        {
            return;
        }
        if let Some(mut dialog) = self.transfer_dialog.take() {
            dialog.gate.cancel();
            dialog.clear_inputs(cx);
            if let Some(plan) = &dialog.plan {
                self.discard_transfer_views(plan);
            }
            cx.background_executor()
                .spawn(async move {
                    drop(dialog);
                })
                .detach();
        }
    }
    fn discard_transfer_views(&mut self, plan: &ImportPlan) {
        for a in &plan.accounts {
            self.browsers.discard_transfer(muse_core::BrowserIdentity {
                account_id: a.id,
                tab_id: a.active_tab_id(),
                generation: a.tabs[0].generation,
            });
        }
    }
    fn pick_transfer_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self
            .transfer_dialog
            .as_mut()
            .filter(|d| !d.export && d.phase == Phase::Form)
        else {
            return;
        };
        d.phase = Phase::Picking;
        let sender = d.sender.clone();
        let id = d.gate.id;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择加密账号包".into()),
        });
        cx.spawn_in(window, async move |_, _| {
            let path = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let _ = sender.send(Reply::Picked(id, path));
        })
        .detach();
        cx.notify();
    }
    pub(super) fn submit_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.transfer_dialog.as_ref() else {
            return;
        };
        if d.phase == Phase::Preview {
            self.confirm_transfer(cx);
            return;
        }
        if d.phase != Phase::Form {
            return;
        }
        if d.export {
            self.start_transfer_export(window, cx);
        } else {
            self.start_transfer_import(cx);
        }
    }
    fn start_transfer_import(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.transfer_dialog.as_mut() else {
            return;
        };
        let Some(path) = d.path.clone() else {
            d.message = "请先选择 .muse-account 加密账号包".into();
            cx.notify();
            return;
        };
        let password = match confirmed_password(d.password.read(cx).text(), "", false) {
            Ok(p) => p,
            Err(e) => {
                d.message = e.into();
                cx.notify();
                return;
            }
        };
        d.clear_inputs(cx);
        d.phase = Phase::Reading;
        d.message = "正在后台验证密码与完整账号包…".into();
        let gate = d.gate.clone();
        let sender = d.sender.clone();
        cx.background_executor()
            .spawn(async move {
                let result = (|| -> Result<_, String> {
                    let bytes = wire::read_file(&path).map_err(|e| e.message().to_owned())?;
                    if !gate.accepts(gate.id) {
                        return Err(wire::TransferError::Cancelled.message().into());
                    }
                    let payload =
                        wire::decrypt(&bytes, &password).map_err(|e| e.message().to_owned())?;
                    if !gate.accepts(gate.id) {
                        return Err(wire::TransferError::Cancelled.message().into());
                    }
                    ImportPlan::prepare(
                        payload,
                        TargetPlatform::current(),
                        (crate::page_observer::now_ms() / 1000) as i64,
                    )
                })();
                let _ = sender.send(Reply::Decoded(gate.id, result));
            })
            .detach();
        cx.notify();
    }
    fn start_transfer_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.transfer_dialog.as_mut() else {
            return;
        };
        if d.selected.is_empty() || d.selected.len() > 32 {
            d.message = "请选择 1–32 个账号".into();
            cx.notify();
            return;
        }
        let confirmation = Zeroizing::new(d.confirmation.read(cx).text());
        let password = match confirmed_password(d.password.read(cx).text(), &confirmation, true) {
            Ok(p) => p,
            Err(e) => {
                d.message = e.into();
                cx.notify();
                return;
            }
        };
        d.clear_inputs(cx);
        d.pending_password = Some(password);
        d.phase = Phase::Picking;
        d.snapshot_workspace = Some(self.workspace.clone());
        d.queue = self
            .workspace
            .accounts
            .iter()
            .filter(|a| d.selected.contains(&a.id))
            .map(|a| a.id)
            .collect();
        let sender = d.sender.clone();
        let id = d.gate.id;
        let default_dir = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .map(|p| p.join("Downloads"))
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| self.data_dir.clone());
        let chosen = cx.prompt_for_new_path(&default_dir, Some("muse-accounts.muse-account"));
        cx.spawn_in(window, async move |_, _| {
            let path = match chosen.await {
                Ok(Ok(p)) => p,
                _ => None,
            };
            let _ = sender.send(Reply::Picked(id, path));
        })
        .detach();
        cx.notify();
    }
    fn confirm_transfer(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self
            .transfer_dialog
            .as_mut()
            .filter(|d| d.phase == Phase::Preview && d.acknowledged)
        else {
            return;
        };
        let Some(mut plan) = d.plan.take() else {
            return;
        };
        for account in &mut plan.accounts {
            for tab in &mut account.tabs {
                tab.url = self.home.clone();
            }
        }
        plan.confirm();
        d.phase = Phase::Vault;
        d.message = "正在后台准备新代理凭据，尚未激活账号…".into();
        let root = self.data_dir.clone();
        let gate = d.gate.clone();
        let sender = d.sender.clone();
        cx.background_executor()
            .spawn(async move {
                if !gate.accepts(gate.id) {
                    return;
                }
                let result = plan.stage_vault(&root);
                if gate.accepts(gate.id) {
                    let _ = sender.send(Reply::Vault(gate.id, plan, result));
                }
            })
            .detach();
        cx.notify();
    }
    fn snapshot_transfer_target(
        &mut self,
        id: AccountId,
        window: &Window,
    ) -> Result<wire::CookieSet, String> {
        if let Some(identity) = self.browsers.cookie_identity(id) {
            return self.browsers.snapshot_transfer(identity);
        }
        let account = self.workspace.account(id).ok_or("账号已不存在")?;
        let identity = muse_core::BrowserIdentity {
            account_id: id,
            tab_id: Uuid::new_v4(),
            generation: 1,
        };
        let profile = account.profile_dir(&self.data_dir.join("profiles"));
        let result = self
            .browsers
            .ensure_reader(
                BrowserRequest {
                    identity,
                    url: "about:blank",
                    profile: &profile,
                    network: account.network,
                    locale: &account.browser_locale,
                    proxy_credentials: self.workspace.proxy_credentials(account),
                    diagnostic_origin: self.diagnostic_origin.as_deref(),
                },
                window,
                self.browser_bounds(window),
            )
            .and_then(|_| self.browsers.snapshot_transfer(identity));
        self.browsers.close_cookie_tab(identity);
        result
    }
    pub(super) fn poll_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(mut d) = self.transfer_dialog.take() else {
            return false;
        };
        let mut changed = false;
        while let Ok(reply) = d.receiver.try_recv() {
            if reply.id() != d.gate.id {
                cx.background_executor()
                    .spawn(async move {
                        drop(reply);
                    })
                    .detach();
                continue;
            }
            changed = true;
            match reply {
                Reply::Picked(_, Some(mut path)) => {
                    if d.export {
                        if path.extension().is_none_or(|e| e != "muse-account") {
                            path.set_extension("muse-account");
                        }
                        d.path = Some(path);
                        d.phase = Phase::Snapshot;
                        d.message = "正在读取所选账号本地快照，不访问网站…".into();
                    } else {
                        d.path = Some(path);
                        d.phase = Phase::Form;
                        d.message = "请输入导出时设置的迁移密码。".into();
                        d.password
                            .update(cx, |i, cx| i.focus_and_select(window, cx));
                    }
                }
                Reply::Picked(_, None) => {
                    d.error("已取消文件选择".into());
                }
                Reply::Decoded(_, Ok(plan)) => {
                    d.plan = Some(plan);
                    d.phase = Phase::Preview;
                    d.message =
                        "请核对所有账号、代理和下方差异；确认后新建账号，不覆盖已有账号。".into();
                }
                Reply::Decoded(_, Err(error)) => d.error(error),
                Reply::Vault(_, plan, Ok(())) => {
                    d.plan = Some(plan);
                    d.phase = Phase::Staging;
                    d.next = 0;
                    d.message = "正在隔离环境中准备路由与 Cookie，尚未访问 Muse…".into();
                }
                Reply::Vault(_, plan, Err(error)) => {
                    cx.background_executor()
                        .spawn(async move {
                            drop(plan);
                        })
                        .detach();
                    d.error(error);
                }
                Reply::Committed(_, plan, Ok(workspace)) => {
                    self.workspace = workspace;
                    self.pending_save = None;
                    self.show_start_page = false;
                    self.proxy_editor.owner = None;
                    self.sync_fields(cx);
                    let mut failed = false;
                    for a in &plan.accounts {
                        let identity = muse_core::BrowserIdentity {
                            account_id: a.id,
                            tab_id: a.active_tab_id(),
                            generation: a.tabs[0].generation,
                        };
                        if self
                            .browsers
                            .activate_cookie_import(identity, &self.home)
                            .is_ok()
                        {
                            self.cookie_logins.insert(a.id, (identity, Instant::now()));
                        } else {
                            failed = true;
                            self.browsers.invalidate_account(a.id);
                        }
                    }
                    d.gate.finish();
                    d.phase = Phase::Done;
                    d.message=if failed{"账号已保存；部分页面暂时无法打开，请稍后刷新。"}else{"账号包已导入，路由与 Cookie 已准备；正在由网页确认身份，这一步尚不代表登录成功。"}.into();
                    self.status = d.message.clone();
                }
                Reply::Committed(_, plan, Err(error)) => {
                    self.discard_transfer_views(&plan);
                    cx.background_executor()
                        .spawn(async move {
                            drop(plan);
                        })
                        .detach();
                    d.error(error);
                }
                Reply::Exported(_, result) => {
                    d.gate.finish();
                    match result {
                        Ok(()) => {
                            d.phase = Phase::Done;
                            d.message =
                                "加密账号包已保存，请通过可信渠道发送文件，并另行传递迁移密码。"
                                    .into();
                        }
                        Err(e) => d.error(e),
                    }
                }
            }
        }
        if d.phase == Phase::Snapshot {
            changed = true;
            if let Some(id) = d.queue.pop_front() {
                match self.snapshot_transfer_target(id, window) {
                    Ok(set) => d.snapshots.push((id, set)),
                    Err(e) => d.error(e),
                }
            } else if let (Some(password), Some(workspace), Some(path)) = (
                d.pending_password.take(),
                d.snapshot_workspace.take(),
                d.path.clone(),
            ) {
                d.phase = Phase::Exporting;
                d.message = "正在后台加密并保存账号包…".into();
                let snapshots = std::mem::take(&mut d.snapshots);
                let root = self.data_dir.clone();
                let gate = d.gate.clone();
                let sender = d.sender.clone();
                cx.background_executor()
                    .spawn(async move {
                        let result = (|| -> Result<(), String> {
                            if !gate.accepts(gate.id) {
                                return Err(wire::TransferError::Cancelled.message().into());
                            }
                            let payload = crate::transfer_plan::export_payload(
                                &root,
                                &workspace,
                                &snapshots,
                                TargetPlatform::current(),
                            )?;
                            let bytes = wire::encrypt(&payload, &password)
                                .map_err(|e| e.message().to_owned())?;
                            wire::write_file_if(&path, &bytes, || gate.begin_commit())
                                .map_err(|e| e.message().to_owned())
                        })();
                        let _ = sender.send(Reply::Exported(gate.id, result));
                    })
                    .detach();
            }
        }
        if d.phase == Phase::Staging {
            changed = true;
            if let Some(mut plan) = d.plan.take() {
                let result = (|| -> Result<(), String> {
                    if d.next >= plan.accounts.len() {
                        return Ok(());
                    }
                    let account = &plan.accounts[d.next];
                    let merged = plan.merge_into(&self.workspace)?;
                    let profile = account.profile_dir(&self.data_dir.join("profiles"));
                    let identity = muse_core::BrowserIdentity {
                        account_id: account.id,
                        tab_id: account.active_tab_id(),
                        generation: account.tabs[0].generation,
                    };
                    self.browsers.stage_transfer(
                        BrowserRequest {
                            identity,
                            url: "about:blank",
                            profile: &profile,
                            network: account.network,
                            locale: &account.browser_locale,
                            proxy_credentials: merged.proxy_credentials(account),
                            diagnostic_origin: self.diagnostic_origin.as_deref(),
                        },
                        &plan.cookies[&account.id],
                        window,
                        self.browser_bounds(window),
                    )?;
                    plan.mark_staged(account.id)?;
                    d.next += 1;
                    Ok(())
                })();
                match result {
                    Err(e) => {
                        self.discard_transfer_views(&plan);
                        cx.background_executor()
                            .spawn(async move {
                                drop(plan);
                            })
                            .detach();
                        d.error(e);
                    }
                    Ok(()) if d.next == plan.accounts.len() => {
                        let intact = plan.accounts.iter().all(|a| {
                            let identity = muse_core::BrowserIdentity {
                                account_id: a.id,
                                tab_id: a.active_tab_id(),
                                generation: a.tabs[0].generation,
                            };
                            self.browsers.is_cookie_reader(identity)
                                && self.browsers.view(identity.tab_id).is_some()
                        });
                        if intact && d.gate.begin_commit() {
                            d.phase = Phase::Committing;
                            d.message = "全部环境核验完成，正在提交账号数据，请稍候…".into();
                            let latest = self.workspace.clone();
                            let root = self.data_dir.clone();
                            let sender = d.sender.clone();
                            let id = d.gate.id;
                            cx.background_executor()
                                .spawn(async move {
                                    let result = plan.commit(&root, &latest);
                                    let _ = sender.send(Reply::Committed(id, plan, result));
                                })
                                .detach();
                        } else {
                            self.discard_transfer_views(&plan);
                            cx.background_executor()
                                .spawn(async move {
                                    drop(plan);
                                })
                                .detach();
                            d.error("候选环境已改变或操作已取消，整批账号未激活".into());
                        }
                    }
                    Ok(()) => d.plan = Some(plan),
                }
            }
        }
        self.transfer_dialog = Some(d);
        if changed {
            cx.notify();
        }
        changed
    }
}
