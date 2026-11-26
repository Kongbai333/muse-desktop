use super::chrome::{glyph, icon_button};
use super::*;
use crate::{
    proxy_pool::{self, RouteChoice},
    proxy_settings::ProxyDraft,
    theme::*,
};
use muse_proxy::ProxyProtocol;
use uuid::Uuid;
use zeroize::Zeroizing;

#[path = "cookie_smoke.rs"]
pub(super) mod cookie_smoke;
#[path = "proxy_batch_dialog.rs"]
mod proxy_batch_dialog;
use proxy_batch_dialog::BatchImport;

struct ConnectionResult {
    draft: ProxyDraft,
    result: Result<(), String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PoolScreen {
    List,
    Batch,
    Edit(Option<Uuid>),
    Remove(Uuid),
}

pub(super) struct AccountDialog {
    pub owner: Option<AccountId>,
    pub removing: bool,
    screen: Option<PoolScreen>,
    batch: Option<BatchImport>,
    route: RouteChoice,
    picking: bool,
    name: Entity<TextInput>,
    cookie: Entity<TextInput>,
    host: Entity<TextInput>,
    port: Entity<TextInput>,
    user: Entity<TextInput>,
    secret: Entity<TextInput>,
    enabled: bool,
    protocol: ProxyProtocol,
    force: bool,
    language: Entity<TextInput>,
    timezone: Entity<TextInput>,
    reveal: bool,
    message: String,
    /// Shown as the name placeholder and used when the name is left empty.
    default_name: String,
    test_ok: Option<bool>,
    pending: bool,
    last_draft: ProxyDraft,
    sender: mpsc::Sender<ConnectionResult>,
    receiver: mpsc::Receiver<ConnectionResult>,
    transition: crate::motion::TransitionState,
}
impl AccountDialog {
    pub(super) fn is_drawer(&self) -> bool {
        self.screen.is_some()
    }
    pub(super) fn transition_exiting(&self) -> bool {
        self.transition.is_exiting()
    }
    fn new(
        owner: Option<AccountId>,
        removing: bool,
        name: &str,
        draft: ProxyDraft,
        cx: &mut Context<MuseWindow>,
    ) -> Self {
        let name = cx.new(|cx| {
            let mut input = TextInput::new(name, cx);
            input.set_placeholder("输入名称");
            input
        });
        let cookie = cx.new(|cx| {
            let mut input = TextInput::new("", cx);
            input.set_placeholder("hatch_sess=…; … 或 Cookie JSON");
            input.set_secure(true, cx);
            input
        });
        let host = cx.new(|cx| {
            let mut input = TextInput::new(&draft.host, cx);
            input.set_placeholder("例如 127.0.0.1");
            input
        });
        let port = cx.new(|cx| {
            let mut input = TextInput::new(&draft.port, cx);
            input.set_placeholder(&draft.protocol.default_port().to_string());
            input
        });
        let user = cx.new(|cx| {
            let mut input = TextInput::new(&draft.user, cx);
            input.set_placeholder("可选");
            input
        });
        let secret = cx.new(|cx| {
            let mut input = TextInput::new(&draft.secret, cx);
            input.set_placeholder("可选");
            input.set_secure(true, cx);
            input
        });
        let (sender, receiver) = mpsc::channel();
        let language = cx.new(|cx| {
            let mut input = TextInput::new("", cx);
            input.set_placeholder("跟随系统（留空）");
            input
        });
        let timezone = cx.new(|cx| {
            let mut input = TextInput::new("", cx);
            input.set_placeholder("跟随系统（留空）");
            input
        });
        let mut dialog = Self {
            owner,
            removing,
            screen: None,
            batch: None,
            route: RouteChoice::System,
            picking: false,
            name,
            cookie,
            host,
            port,
            user,
            secret,
            enabled: draft.enabled,
            protocol: draft.protocol,
            force: draft.force_proxy,
            language,
            timezone,
            reveal: false,
            message: String::new(),
            default_name: String::new(),
            test_ok: None,
            pending: false,
            last_draft: draft.clone(),
            sender,
            receiver,
            transition: crate::motion::TransitionState::entering(),
        };
        dialog.last_draft = dialog.proxy(cx);
        dialog
    }
    fn set_default_name(&mut self, value: String, cx: &mut App) {
        self.name
            .update(cx, |input, _| input.set_placeholder(&value));
        self.default_name = value;
    }
    /// The typed name, or the suggested default when the box is left empty.
    fn name_or_default(&self, cx: &App) -> String {
        let name = self.name.read(cx).text();
        if name.trim().is_empty() {
            self.default_name.clone()
        } else {
            name
        }
    }
    fn proxy(&self, cx: &App) -> ProxyDraft {
        ProxyDraft {
            enabled: self.enabled,
            protocol: self.protocol,
            host: self.host.read(cx).text(),
            port: {
                let port = self.port.read(cx).text();
                if port.trim().is_empty() {
                    self.protocol.default_port().to_string()
                } else {
                    port
                }
            },
            user: self.user.read(cx).text(),
            secret: Zeroizing::new(self.secret.read(cx).text()),
            force_proxy: self.force,
        }
    }
    fn fill(&mut self, draft: ProxyDraft, cx: &mut App) {
        self.enabled = draft.enabled;
        self.protocol = draft.protocol;
        self.force = draft.force_proxy;
        self.host
            .update(cx, |input, cx| input.set_text(&draft.host, cx));
        self.port
            .update(cx, |input, cx| input.set_text(&draft.port, cx));
        self.user
            .update(cx, |input, cx| input.set_text(&draft.user, cx));
        self.secret
            .update(cx, |input, cx| input.set_text(&draft.secret, cx));
    }
}

impl MuseWindow {
    pub(super) fn poll_cookie_logins(&mut self) -> bool {
        use crate::cookie_import::{LoginOutcome, login_outcome};
        let finished = self
            .cookie_logins
            .iter()
            .filter_map(|(id, (identity, started))| {
                if !self.workspace.contains_identity(*identity) {
                    return Some((*id, None));
                }
                let account = self.workspace.account(*id)?;
                login_outcome(&account.quota, started.elapsed().as_millis() as u64)
                    .map(|outcome| (*id, Some(outcome)))
            })
            .collect::<Vec<_>>();
        for (id, outcome) in &finished {
            self.cookie_logins.remove(id);
            if self.workspace.active_account_id() == Some(*id) {
                self.status = match outcome {
                    Some(LoginOutcome::Confirmed) => "Cookie 登录已确认".into(),
                    Some(LoginOutcome::Rejected) => {
                        "Cookie 登录未生效，可能已过期或不适用于当前环境，请重新导出".into()
                    }
                    Some(LoginOutcome::Unconfirmed) => {
                        "Cookie 已导入，暂未确认登录状态；请查看网页，如仍需登录请重新导出".into()
                    }
                    None => continue,
                };
            }
        }
        !finished.is_empty()
    }

    pub(super) fn exercise_account_dialogs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(identity) = self.workspace.active_browser_identity() else {
            return false;
        };
        let before = self.workspace.to_json().unwrap_or_default();
        let count = self.workspace.accounts.len();
        self.open_account_dialog(None, false, window, cx);
        let hidden = self.browsers.visible(identity.tab_id) == Some(false);
        self.new_tab(&NewTab, window, cx);
        let background_blocked = self.workspace.to_json().unwrap_or_default() == before;
        self.close_account_dialog(window, cx);
        let old_exit = self
            .account_dialog
            .as_ref()
            .map(|dialog| dialog.transition.generation());
        let cancelled = self.workspace.to_json().unwrap_or_default() == before;
        self.open_account_dialog(Some(identity.account_id), false, window, cx);
        if let Some(generation) = old_exit {
            self.finish_account_dialog_exit(generation, window, cx);
        }
        let reopened = self.account_dialog.as_ref().is_some_and(|dialog| {
            dialog.owner == Some(identity.account_id) && !dialog.transition.is_exiting()
        });
        if let Some(dialog) = &self.account_dialog {
            dialog
                .name
                .update(cx, |input, cx| input.set_text("已编辑的测试账号", cx));
        }
        self.save_account_dialog(window, cx);
        let renamed = self
            .workspace
            .account(identity.account_id)
            .is_some_and(|a| a.name == "已编辑的测试账号")
            && self.workspace.contains_identity(identity)
            && self.browsers.view(identity.tab_id).is_some();
        self.open_account_dialog(None, false, window, cx);
        if let Some(dialog) = &self.account_dialog {
            dialog
                .name
                .update(cx, |input, cx| input.set_text("弹窗新增测试", cx));
        }
        self.save_account_dialog(window, cx);
        self.save_account_dialog(window, cx);
        let created = self.workspace.accounts.len() == count + 1
            && self
                .account_dialog
                .as_ref()
                .is_none_or(|dialog| dialog.transition.is_exiting());
        let added = self.workspace.active_account_id();
        if created && let Some(id) = added {
            self.open_account_dialog(Some(id), true, window, cx);
            self.save_account_dialog(window, cx);
        }
        hidden
            && background_blocked
            && cancelled
            && reopened
            && renamed
            && created
            && self.workspace.accounts.len() == count
            && self.workspace.contains_identity(identity)
            && self.browsers.view(identity.tab_id).is_some()
    }
    pub(super) fn open_account_dialog(
        &mut self,
        owner: Option<AccountId>,
        removing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.updates.open
            || !self.save_enabled
            || self.cookie_dialog.is_some()
            || self
                .account_dialog
                .as_ref()
                .is_some_and(|dialog| !dialog.transition.is_exiting())
        {
            return;
        }
        let name = owner
            .and_then(|id| self.workspace.account(id))
            .map(|a| a.name.clone())
            .unwrap_or_default();
        let mut dialog = AccountDialog::new(owner, removing, &name, ProxyDraft::default(), cx);
        if owner.is_none() {
            dialog.set_default_name(format!("账号 {}", self.workspace.accounts.len() + 1), cx);
        }
        if let Some(account) = owner.and_then(|id| self.workspace.account(id)) {
            dialog.language.update(cx, |input, cx| {
                input.set_text(
                    account
                        .browser_locale
                        .language
                        .as_deref()
                        .unwrap_or_default(),
                    cx,
                )
            });
            dialog.timezone.update(cx, |input, cx| {
                input.set_text(
                    account
                        .browser_locale
                        .timezone
                        .as_deref()
                        .unwrap_or_default(),
                    cx,
                )
            });
            dialog.force = account.network == muse_core::AccountNetwork::Direct
                || account.network.force_proxy();
            dialog.route = RouteChoice::from(account.network);
        }
        if let Some(view) = self
            .workspace
            .active_browser_identity()
            .and_then(|id| self.browsers.view(id.tab_id))
        {
            let _ = view.focus_parent();
        }
        if removing {
            window.focus(&self.focus);
        } else {
            window.focus(&dialog.name.read(cx).focus_handle(cx));
        }
        self.account_dialog = Some(dialog);
        self.ensure_page_freeze();
        self.browsers
            .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
        cx.notify();
    }
    pub(super) fn close_account_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.account_dialog.as_mut() else {
            return;
        };
        match dialog.screen {
            Some(PoolScreen::List) if self.account_dialog_return.is_some() => {
                self.account_dialog = self.account_dialog_return.take();
                if let Some(dialog) = &self.account_dialog {
                    window.focus(&dialog.name.read(cx).focus_handle(cx));
                }
                cx.notify();
                return;
            }
            Some(PoolScreen::Edit(_) | PoolScreen::Remove(_) | PoolScreen::Batch) => {
                self.show_proxy_pool_screen(PoolScreen::List, window, cx);
                return;
            }
            _ if dialog.picking => {
                dialog.picking = false;
                cx.notify();
                return;
            }
            _ => {}
        }
        let Some(generation) = dialog.transition.begin_exit() else {
            return;
        };
        dialog.cookie.update(cx, |input, cx| input.set_text("", cx));
        window.focus(&self.focus);
        self.schedule_overlay_removal(
            MotionKind::Modal,
            generation,
            window,
            cx,
            Self::finish_account_dialog_exit,
        );
    }

    /// Click outside the panel: leave proxy sub-screens and close, like Esc twice.
    fn dismiss_account_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .account_dialog
            .as_mut()
            .filter(|dialog| !dialog.transition.is_exiting())
        else {
            return;
        };
        dialog.picking = false;
        if matches!(
            dialog.screen,
            Some(PoolScreen::Edit(_) | PoolScreen::Remove(_) | PoolScreen::Batch)
        ) {
            self.show_proxy_pool_screen(PoolScreen::List, window, cx);
        }
        self.close_account_dialog(window, cx);
    }

    fn finish_account_dialog_exit(
        &mut self,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .account_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.transition.finish_exit(generation))
        {
            return;
        }
        self.account_dialog = None;
        window.focus(&self.focus);
        self.ensure_active(window);
        cx.notify();
    }
    pub(super) fn save_account_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .account_dialog
            .as_ref()
            .filter(|dialog| !dialog.transition.is_exiting())
        else {
            return;
        };
        let owner = dialog.owner;
        let removing = dialog.removing;
        let name = dialog.name_or_default(cx);
        if let Some(screen) = dialog.screen {
            self.save_pool_screen(screen, &name, window, cx);
            return;
        }
        let (route, force) = (dialog.route, dialog.force);
        if removing {
            let Some(id) = owner else {
                return;
            };
            let mut candidate = self.workspace.clone();
            let commit = candidate
                .remove_account(id)
                .and_then(|()| candidate.save(&self.data_dir.join("workspace.json")));
            if let Err(error) = commit {
                self.account_dialog.as_mut().unwrap().message = error;
                cx.notify();
                return;
            }
            self.workspace = candidate;
            self.show_start_page = self.workspace.accounts.is_empty();
            self.navigation_transition = TransitionState::entering();
            self.browsers.invalidate_account(id);
            self.avatars.remove(id);
            self.plan_labels.remove(&id);
            self.invites.remove(&id);
            self.cached_accounts.remove(&id);
            crate::account_cache::remove(&self.data_dir, id);
            self.fail_studio_job(id, "账号已移除", cx);
            self.sync_fields(cx);
            self.close_account_dialog(window, cx);
            return;
        }
        if owner.is_none() {
            let text = Zeroizing::new(dialog.cookie.read(cx).text());
            if !text.trim().is_empty() {
                let result = crate::cookie_import::parse(
                    &text,
                    &self.home,
                    (crate::page_observer::now_ms() / 1000) as i64,
                )
                .map_err(str::to_owned)
                .and_then(|cookies| {
                    self.create_cookie_account(&name, route, force, cookies, window, cx)
                });
                if let Err(error) = result {
                    if let Some(dialog) = self.account_dialog.as_mut() {
                        dialog.message = error;
                    }
                }
                cx.notify();
                return;
            }
        }
        let locale = match muse_core::BrowserLocale::from_input(
            &dialog.language.read(cx).text(),
            &dialog.timezone.read(cx).text(),
        ) {
            Ok(locale) => locale,
            Err(error) => {
                self.account_dialog.as_mut().unwrap().message = error;
                cx.notify();
                return;
            }
        };
        match proxy_pool::save_account_with_locale(
            &self.data_dir,
            &mut self.workspace,
            owner,
            &name,
            route,
            force,
            locale,
        ) {
            Ok(saved) => {
                if saved.browser_changed {
                    self.browsers.invalidate_account(saved.id);
                }
                if owner.is_none() {
                    self.show_start_page = false;
                    self.navigation_transition = TransitionState::entering();
                    if self.home != HOME_URL
                        && let Some(identity) = self.workspace.active_browser_identity()
                    {
                        self.workspace
                            .apply_browser_event(identity, BrowserEvent::Url(self.home.clone()));
                        self.persist();
                    }
                }
                self.proxy_editor.owner = None;
                self.sync_fields(cx);
                self.status = if owner.is_some() {
                    "账号已更新"
                } else {
                    "账号已创建"
                }
                .into();
                self.close_account_dialog(window, cx);
            }
            Err(error) => self.account_dialog.as_mut().unwrap().message = error,
        }
        cx.notify();
    }

    fn create_cookie_account(
        &mut self,
        name: &str,
        route: RouteChoice,
        force: bool,
        cookies: crate::cookie_import::CookieImport,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (mut candidate, saved) =
            proxy_pool::prepare_account(&self.data_dir, &self.workspace, None, name, route, force)?;
        let locale = self
            .account_dialog
            .as_ref()
            .map(|d| {
                muse_core::BrowserLocale::from_input(
                    &d.language.read(cx).text(),
                    &d.timezone.read(cx).text(),
                )
            })
            .transpose()?
            .unwrap_or_default();
        candidate.set_browser_locale(saved.id, locale)?;
        let identity = candidate
            .active_browser_identity()
            .ok_or("无法准备新账号")?;
        candidate.apply_browser_event(identity, BrowserEvent::Url(self.home.clone()));
        let account = candidate.account(saved.id).ok_or("无法准备新账号")?;
        let profile = account.profile_dir(&self.data_dir.join("profiles"));
        self.browsers.stage_cookie_import(
            BrowserRequest {
                identity,
                url: "about:blank",
                profile: &profile,
                network: account.network,
                locale: &account.browser_locale,
                proxy_credentials: candidate.proxy_credentials(account),
                diagnostic_origin: self.diagnostic_origin.as_deref(),
            },
            &cookies,
            window,
            self.browser_bounds(window),
        )?;
        if let Err(error) = candidate.save(&self.data_dir.join("workspace.json")) {
            self.browsers.discard_cookie_import(identity, &cookies);
            return Err(error);
        }
        self.workspace = candidate;
        self.show_start_page = false;
        self.navigation_transition = TransitionState::entering();
        self.proxy_editor.owner = None;
        self.sync_fields(cx);
        self.status = match self.browsers.activate_cookie_import(identity, &self.home) {
            Ok(()) => {
                self.cookie_logins
                    .insert(saved.id, (identity, Instant::now()));
                "Cookie 已导入，正在由 Muse 确认登录状态".into()
            }
            Err(error) => {
                self.browsers.invalidate_account(saved.id);
                error
            }
        };
        self.close_account_dialog(window, cx);
        Ok(())
    }
    pub(super) fn poll_account_dialog(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(dialog) = self.account_dialog.as_mut().filter(|dialog| {
            !dialog.transition.is_exiting() && matches!(dialog.screen, Some(PoolScreen::Edit(_)))
        }) else {
            return false;
        };
        let mut changed = false;
        let current = dialog.proxy(cx);
        if current != dialog.last_draft {
            dialog.last_draft = current;
            dialog.message = "未保存的修改".into();
            dialog.test_ok = None;
            changed = true;
        }
        while let Ok(result) = dialog.receiver.try_recv() {
            dialog.pending = false;
            if dialog.proxy(cx) == result.draft {
                dialog.test_ok = Some(result.result.is_ok());
                dialog.message = match result.result {
                    Ok(()) => {
                        "兼容探测：muse.ai:443 隧道建立通过；保存后在账号窗口验证实际路由与页面"
                            .into()
                    }
                    Err(error) => error,
                };
                changed = true;
            }
        }
        changed
    }
    fn test_dialog_proxy(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .account_dialog
            .as_mut()
            .filter(|dialog| !dialog.transition.is_exiting())
        else {
            return;
        };
        if dialog.pending {
            return;
        }
        let trace_context = muse_proxy::trace::Context {
            action: muse_proxy::trace::new_id(),
            source: Some(muse_proxy::trace::Source::PoolTest),
            ..Default::default()
        };
        muse_proxy::trace::Event::new(
            trace_context,
            muse_proxy::trace::Stage::ManualAction,
            muse_proxy::trace::Outcome::Start,
        )
        .emit();
        let draft = dialog.proxy(cx);
        let upstream = match draft
            .proxy_url()
            .and_then(|url| muse_proxy::Upstream::parse(&url).map_err(|e| e.to_string()))
        {
            Ok(value) => value,
            Err(error) => {
                muse_proxy::trace::Event::new(
                    trace_context,
                    muse_proxy::trace::Stage::ProbeEnd,
                    muse_proxy::trace::Outcome::Configuration,
                )
                .emit();
                dialog.message = error;
                dialog.test_ok = Some(false);
                cx.notify();
                return;
            }
        };
        let sender = dialog.sender.clone();
        dialog.test_ok = None;
        dialog.pending = true;
        dialog.message = "正在测试代理认证及 muse.ai:443 隧道…".into();
        cx.background_executor()
            .spawn(async move {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| "无法启动连接测试".to_owned())
                    .and_then(|runtime| {
                        runtime
                            .block_on(upstream.probe_traced(trace_context))
                            .map_err(|e| e.to_string())
                    });
                let _ = sender.send(ConnectionResult { draft, result });
            })
            .detach();
        cx.notify();
    }
    fn paste_dialog_proxy(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
            && let Some(dialog) = self
                .account_dialog
                .as_mut()
                .filter(|dialog| !dialog.transition.is_exiting())
        {
            match ProxyDraft::from_url(text.trim(), dialog.force) {
                Ok(draft) => {
                    dialog.fill(draft, cx);
                    dialog.last_draft = dialog.proxy(cx);
                    dialog.message = "已填入，保存后生效".into();
                    dialog.test_ok = None;
                }
                Err(error) => {
                    dialog.message = error;
                    dialog.test_ok = Some(false);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn open_proxy_pool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.updates.open
            || !self.save_enabled
            || self.cookie_dialog.is_some()
            || self.account_context.is_some()
        {
            return;
        }
        if self
            .account_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.screen.is_some())
        {
            return;
        }
        self.account_dialog_return = self
            .account_dialog
            .take()
            .filter(|dialog| !dialog.transition.is_exiting());
        if let Some(view) = self
            .workspace
            .active_browser_identity()
            .and_then(|identity| self.browsers.view(identity.tab_id))
        {
            let _ = view.focus_parent();
        }
        self.show_proxy_pool_screen(PoolScreen::List, window, cx);
        self.animate_chrome_layout(window, cx);
    }

    fn show_proxy_pool_screen(
        &mut self,
        screen: PoolScreen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut name = String::new();
        let mut message = String::new();
        let mut draft = ProxyDraft {
            enabled: true,
            ..Default::default()
        };
        if let PoolScreen::Edit(Some(id)) | PoolScreen::Remove(id) = screen {
            let Some(profile) = self.workspace.proxy_profile(id) else {
                return;
            };
            name.clone_from(&profile.name);
            if matches!(screen, PoolScreen::Edit(_)) {
                match proxy_pool::load(&self.data_dir, profile) {
                    Ok(saved) => draft = saved,
                    Err(error) => message = error,
                }
            }
        }
        if screen == PoolScreen::Edit(None) {
            draft.port = String::new();
        }
        let mut dialog = AccountDialog::new(None, false, &name, draft, cx);
        if screen == PoolScreen::Edit(None) {
            dialog.set_default_name(
                format!("代理 {}", self.workspace.proxy_profiles.len() + 1),
                cx,
            );
        }
        dialog.screen = Some(screen);
        if screen == PoolScreen::Batch {
            dialog.batch = Some(BatchImport::new());
        }
        dialog.message = message;
        if self.account_dialog.is_some() || self.account_dialog_return.is_some() {
            dialog.transition = TransitionState::visible();
        }
        if matches!(screen, PoolScreen::Edit(_)) {
            window.focus(&dialog.name.read(cx).focus_handle(cx));
        } else {
            window.focus(&self.focus);
        }
        self.account_dialog = Some(dialog);
        cx.notify();
    }

    fn save_pool_screen(
        &mut self,
        screen: PoolScreen,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = match screen {
            PoolScreen::List => return,
            PoolScreen::Batch => {
                self.confirm_proxy_batch(window, cx);
                return;
            }
            PoolScreen::Remove(id) => {
                proxy_pool::remove(&self.data_dir, &mut self.workspace, id).map(|()| Vec::new())
            }
            PoolScreen::Edit(id) => {
                let draft = self
                    .account_dialog
                    .as_ref()
                    .expect("open proxy form")
                    .proxy(cx);
                proxy_pool::save(&self.data_dir, &mut self.workspace, id, name, &draft)
            }
        };
        match result {
            Ok(affected) => {
                for id in affected {
                    self.browsers.invalidate_account(id);
                    self.fail_studio_job(id, "账号代理已更新，请重新提交", cx);
                }
                self.refresh_proxy_editor(cx);
                self.show_proxy_pool_screen(PoolScreen::List, window, cx);
            }
            Err(error) => {
                if let Some(dialog) = self.account_dialog.as_mut() {
                    dialog.message = error;
                }
                cx.notify();
            }
        }
    }

    fn render_pool_list(&self, cx: &Context<Self>) -> Div {
        let profiles = &self.workspace.proxy_profiles;
        if profiles.is_empty() {
            return div()
                .pt(px(56.))
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .size(px(44.))
                        .rounded_full()
                        .bg(rgb(colors().fill))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(glyph("shield-check", 20., colors().secondary)),
                )
                .child(
                    div()
                        .mt_2()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("还没有代理"),
                )
                .child(
                    div()
                        .max_w(px(260.))
                        .text_size(px(12.))
                        .line_height(px(19.))
                        .text_color(rgb(colors().secondary))
                        .text_center()
                        .child("新增一个代理后，可以在编辑账号时为账号选择它。多个账号可以共用同一个代理。"),
                );
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(profiles.iter().map(|profile| {
                let id = profile.id;
                let users = proxy_pool::references(&self.workspace, id);
                let count = users.len();
                let names = users
                    .iter()
                    .filter_map(|account| self.workspace.account(*account))
                    .map(|account| account.name.clone())
                    .collect::<Vec<_>>();
                let usage = match names.len() {
                    0 => "未使用".to_owned(),
                    1 | 2 => format!("{} 使用中", names.join("、")),
                    n => format!("{} 等 {n} 个账号使用", names[..2].join("、")),
                };
                div()
                    .id(ElementId::Name(id.to_string().into()))
                    .px_3()
                    .py(px(10.))
                    .rounded_lg()
                    .flex()
                    .items_center()
                    .gap_3()
                    .hover(|style| style.bg(rgb(colors().fill)))
                    .child(
                        div()
                            .size(px(8.))
                            .flex_none()
                            .rounded_full()
                            .bg(rgb(if count > 0 {
                                0x16A34A
                            } else {
                                tone(0xD4D4D8, 0x60606D)
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors().text))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(profile.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors().secondary))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(format!(
                                        "{} · {usage}",
                                        profile.protocol.to_ascii_uppercase()
                                    )),
                            ),
                    )
                    .child(icon_button(
                        "edit-pool-proxy",
                        "pencil",
                        "编辑代理",
                        true,
                        false,
                        cx.listener(move |this, _, window, cx| {
                            this.show_proxy_pool_screen(PoolScreen::Edit(Some(id)), window, cx)
                        }),
                    ))
                    .child(icon_button(
                        "remove-pool-proxy",
                        "trash",
                        if count == 0 {
                            "删除代理"
                        } else {
                            "仍有账号使用，无法删除"
                        },
                        count == 0,
                        false,
                        cx.listener(move |this, _, window, cx| {
                            this.show_proxy_pool_screen(PoolScreen::Remove(id), window, cx)
                        }),
                    ))
            }))
    }

    fn render_dialog_network(&self, dialog: &AccountDialog, cx: &Context<Self>) -> Div {
        let mut choices: Vec<(ElementId, String, String, RouteChoice)> = vec![(
            "pick-system".into(),
            "系统网络".into(),
            "沿用 Windows 的网络设置".into(),
            RouteChoice::System,
        )];
        if dialog.route == RouteChoice::Legacy {
            choices.push((
                "pick-legacy".into(),
                "原代理（未迁入）".into(),
                "旧版保存的代理，保存后保持不变".into(),
                RouteChoice::Legacy,
            ));
        }
        for profile in &self.workspace.proxy_profiles {
            let count = proxy_pool::references(&self.workspace, profile.id).len();
            choices.push((
                ElementId::Name(profile.id.to_string().into()),
                profile.name.clone(),
                if count == 0 {
                    format!("{} · 未使用", profile.protocol.to_ascii_uppercase())
                } else {
                    format!(
                        "{} · {count} 个账号使用",
                        profile.protocol.to_ascii_uppercase()
                    )
                },
                RouteChoice::Pool(profile.id),
            ));
        }
        if let RouteChoice::Pool(id) = dialog.route
            && self.workspace.proxy_profile(id).is_none()
        {
            choices.push((
                "pick-missing".into(),
                "代理已不存在".into(),
                "请改选其他网络连接".into(),
                dialog.route,
            ));
        }
        let rows = choices
            .into_iter()
            .enumerate()
            .map(|(index, (id, title, detail, route))| {
                let active = dialog.route == route;
                div()
                    .id(id)
                    .px_3()
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(index > 0, |element| {
                        element.border_t_1().border_color(rgb(colors().line))
                    })
                    .bg(rgb(if active {
                        tone(0xF7F9FE, 0x24334F)
                    } else {
                        colors().panel
                    }))
                    .cursor_pointer()
                    .hover(move |style| {
                        style.bg(rgb(if active {
                            tone(0xF7F9FE, 0x24334F)
                        } else {
                            tone(0xFAFAFB, 0x1D1D21)
                        }))
                    })
                    .child(radio_mark(active))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors().text))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors().secondary))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(detail),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(dialog) = this.account_dialog.as_mut() {
                            dialog.route = route;
                            dialog.picking = false;
                            dialog.message.clear();
                        }
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(field_label("网络连接"))
                    .child(link_button(
                        "manage-proxy-pool",
                        "管理代理池",
                        cx.listener(|this, _, window, cx| this.open_proxy_pool(window, cx)),
                    )),
            )
            .child(
                div()
                    .id("proxy-options")
                    .max_h(px(232.))
                    .overflow_y_scroll()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
                    .children(rows),
            )
            .when(matches!(dialog.route, RouteChoice::Pool(_)), |element| {
                element.child(div().mt_1().child(checkbox_row(
                    "force-pool-proxy",
                    "本机地址也经过此代理",
                    dialog.force,
                    cx.listener(|this, _, _, cx| {
                        if let Some(dialog) = this.account_dialog.as_mut() {
                            dialog.force = !dialog.force;
                        }
                        cx.notify();
                    }),
                )))
            })
            .when(self.workspace.proxy_profiles.is_empty(), |element| {
                element.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(colors().muted))
                        .child("还没有代理。点「管理代理池」新增后，这里就可以选择。"),
                )
            })
    }

    fn render_proxy_form(&self, dialog: &AccountDialog, cx: &Context<Self>) -> Div {
        let segment = |id, label, selected, protocol: Option<ProxyProtocol>| {
            div()
                .id(id)
                .flex_1()
                .h(px(30.))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(if selected {
                    (colors().panel << 8) | 0xFF
                } else {
                    0x00000000
                }))
                .text_color(rgb(if selected {
                    colors().text
                } else {
                    colors().secondary
                }))
                .text_size(px(12.))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .cursor_pointer()
                .when(selected, |e| e.shadow_sm())
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(dialog) = this.account_dialog.as_mut() {
                        dialog.enabled = protocol.is_some();
                        if let Some(protocol) = protocol {
                            let port = dialog.port.read(cx).text();
                            if protocol != dialog.protocol
                                && port.trim() == dialog.protocol.default_port().to_string()
                            {
                                dialog.port.update(cx, |input, cx| {
                                    input.set_text(&protocol.default_port().to_string(), cx)
                                });
                            }
                            let hint = protocol.default_port().to_string();
                            dialog.port.update(cx, |input, cx| {
                                input.set_placeholder(&hint);
                                cx.notify();
                            });
                            dialog.protocol = protocol;
                        }
                    }
                    cx.notify();
                }))
        };
        let reveal = link_button(
            "dialog-reveal",
            if dialog.reveal { "隐藏" } else { "显示" },
            cx.listener(|this, _, _, cx| {
                if let Some(d) = this.account_dialog.as_mut() {
                    d.reveal = !d.reveal;
                    d.secret
                        .update(cx, |input, cx| input.set_secure(!d.reveal, cx));
                }
                cx.notify();
            }),
        );
        let (dot, status_color) = match (dialog.pending, dialog.test_ok) {
            (true, _) => (0xE8A317, colors().secondary),
            (false, Some(true)) => (0x16A34A, tone(0x15803D, 0x77D4A4)),
            (false, Some(false)) => (colors().danger, tone(0xB42318, 0xFF9B94)),
            _ => (tone(0xD4D4D8, 0x60606D), colors().secondary),
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .id("dialog-paste")
                    .h(px(38.))
                    .px_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(tone(0xD4D4D8, 0x60606D)))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(colors().fill)))
                    .child(glyph("copy", 14., colors().secondary))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(rgb(colors().text))
                            .child("从剪贴板粘贴代理地址"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(colors().muted))
                            .child("自动识别协议与账密"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.paste_dialog_proxy(cx))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(field_label("协议"))
                    .child(
                        div()
                            .p(px(3.))
                            .rounded_lg()
                            .bg(rgb(colors().fill))
                            .flex()
                            .gap_1()
                            .children(ProxyProtocol::ALL.map(|protocol| {
                                segment(
                                    protocol.scheme(),
                                    protocol.label(),
                                    dialog.enabled && dialog.protocol == protocol,
                                    Some(protocol),
                                )
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(dialog_field("服务器", dialog.host.clone())),
                    )
                    .child(
                        div()
                            .w(px(96.))
                            .flex_none()
                            .child(dialog_field("端口", dialog.port.clone())),
                    ),
            )
            .child(dialog_field("用户名（可选）", dialog.user.clone()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(field_label("密码（可选）"))
                            .child(reveal),
                    )
                    .child(field_box(dialog.secret.clone())),
            )
            .child(checkbox_row(
                "dialog-force",
                "本机地址也经过此代理",
                dialog.force,
                cx.listener(|this, _, _, cx| {
                    if let Some(dialog) = this.account_dialog.as_mut() {
                        dialog.force = !dialog.force;
                    }
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(tone(0xFAFAFB, 0x1D1D21)))
                    .border_1()
                    .border_color(rgb(colors().line))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().size(px(8.)).flex_none().rounded_full().bg(rgb(dot)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(rgb(status_color))
                            .child(if dialog.message.is_empty() {
                                "仅测试认证及 muse.ai:443 隧道，不验证目标 TLS/页面，不保存配置"
                                    .to_owned()
                            } else {
                                dialog.message.clone()
                            }),
                    )
                    .child(dialog_button(
                        "dialog-test",
                        if dialog.pending {
                            "测试中…"
                        } else {
                            "测试隧道"
                        },
                        ButtonKind::Secondary,
                        cx.listener(|this, _, _, cx| this.test_dialog_proxy(cx)),
                    )),
            )
    }

    pub(super) fn render_account_dialog(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let dialog = self.account_dialog.as_ref().unwrap();
        let transition = dialog.transition;
        let exiting = transition.is_exiting();
        let viewport = window.viewport_size();
        let (title, subtitle) = match dialog.screen {
            Some(PoolScreen::List) => (
                "代理池",
                format!(
                    "{} 个代理 · 多个账号可共用",
                    self.workspace.proxy_profiles.len()
                ),
            ),
            Some(PoolScreen::Edit(None)) => ("新增代理", "支持 HTTP、HTTPS、SOCKS5".into()),
            Some(PoolScreen::Batch) => {
                ("批量导入代理", "预览有效项后确认追加，密码始终隐藏".into())
            }
            Some(PoolScreen::Edit(Some(_))) => {
                ("编辑代理", "保存后，使用此代理的账号会重新连接".into())
            }
            Some(PoolScreen::Remove(_)) => ("删除代理", "删除后无法恢复".into()),
            None if dialog.removing => ("移除账号", "只从账号列表移除".into()),
            None if dialog.owner.is_some() => (
                "编辑账号",
                "改名不会重新加载网页；更换网络或指纹模式会重建浏览器，登录数据保留。".into(),
            ),
            None => (
                "新增账号",
                "每个账号使用独立的浏览器环境，登录互不影响。".into(),
            ),
        };
        let drawer = dialog.screen.is_some();
        let sub_screen = matches!(
            dialog.screen,
            Some(PoolScreen::Edit(_) | PoolScreen::Remove(_) | PoolScreen::Batch)
        );

        let close = div()
            .id("dialog-close")
            .size(px(30.))
            .flex_none()
            .rounded_lg()
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .hover(|s| s.bg(rgb(colors().fill)))
            .child(glyph(
                if sub_screen { "arrow-left" } else { "x" },
                15.,
                colors().secondary,
            ))
            .on_click(cx.listener(|this, _, window, cx| this.close_account_dialog(window, cx)));
        let heading = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors().text))
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(rgb(colors().secondary))
                    .child(subtitle),
            );
        let header = if drawer {
            div()
                .flex_none()
                .px_4()
                .py_3()
                .border_b_1()
                .border_color(rgb(colors().line))
                .flex()
                .items_center()
                .gap_2()
                .when(sub_screen, |element| element.child(close))
                .child(heading)
                .when(!sub_screen, |element| {
                    element.child(
                        div()
                            .id("dialog-close")
                            .size(px(30.))
                            .flex_none()
                            .rounded_lg()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(rgb(colors().fill)))
                            .child(glyph("x", 15., colors().secondary))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_account_dialog(window, cx)
                            })),
                    )
                })
        } else {
            div()
                .flex_none()
                .px_6()
                .pt_5()
                .pb_2()
                .flex()
                .items_start()
                .gap_3()
                .child(heading)
                .child(close)
        };

        let mut body = div()
            .id("account-dialog-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .when(drawer, |element| element.px_4().py_4())
            .when(!drawer, |element| element.px_6().pt_3().pb_2());
        body =
            match dialog.screen {
                Some(PoolScreen::List) => body.child(self.render_pool_list(cx)),
                Some(PoolScreen::Batch) => body.child(self.render_proxy_batch(dialog, cx)),
                Some(PoolScreen::Edit(_)) => body
                    .child(dialog_field("代理名称", dialog.name.clone()))
                    .child(self.render_proxy_form(dialog, cx)),
                Some(PoolScreen::Remove(_)) => body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(rgb(colors().text))
                                .child(format!("确认删除「{}」？", dialog.name.read(cx).text())),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(colors().secondary))
                                .child("只能删除没有账号使用的代理。"),
                        ),
                ),
                None if dialog.removing => body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(rgb(colors().text))
                                .child(format!("确认移除「{}」？", dialog.name.read(cx).text())),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(19.))
                                .text_color(rgb(colors().secondary))
                                .child(
                                    "账号的浏览器数据和登录状态会保留在本机，之后可以重新导入。",
                                ),
                        ),
                ),
                None => {
                    body.child(dialog_field("账号名称", dialog.name.clone()))
                        .when(dialog.owner.is_none(), |element| {
                            element.child(
                        div().flex().flex_col().gap_1()
                            .child(dialog_field("Cookie 登录（可选）", dialog.cookie.clone()))
                            .child(div().text_size(px(11.)).line_height(px(17.))
                                .text_color(rgb(colors().secondary))
                                .child("支持单账号 Cookie 文本或浏览器 JSON，留空则正常登录。"))
                            .child(div().text_size(px(11.)).line_height(px(17.))
                                .text_color(rgb(colors().secondary))
                                .child("Cookie 不含代理；如原账号使用代理，请先选择同一条网络连接。")),
                    )
                        })
                        .child(self.render_dialog_network(dialog, cx))
                        .child(
                            div().flex().flex_col().gap_2()
                                .child(dialog_field("默认浏览器语言（留空跟随系统）", dialog.language.clone()))
                                .child(div().text_size(px(11.)).line_height(px(17.))
                                    .text_color(rgb(colors().secondary))
                                    .child(format!("可选：{}", muse_core::BROWSER_LANGUAGES.join(" / "))))
                                .child(dialog_field("时区（留空跟随系统）", dialog.timezone.clone()))
                                .child(div().text_size(px(11.)).line_height(px(17.))
                                    .text_color(rgb(colors().secondary))
                                    .child(format!("可选：{}", muse_core::BROWSER_TIMEZONES.join(" / "))))
                                .child(div().text_size(px(11.)).line_height(px(17.))
                                    .text_color(rgb(colors().secondary))
                                    .child("只设置语言和时区；修改后重开此账号全部标签页，保留原目录与 Cookie。不根据代理推断，也不修改 Canvas、WebGL 或浏览器身份。"))
                        )
                }
            };
        if !dialog.message.is_empty() && !matches!(dialog.screen, Some(PoolScreen::Edit(_))) {
            body = body.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(rgb(if dialog.test_ok == Some(true) {
                        colors().fill
                    } else {
                        tone(0xFEF3F2, 0x3D2225)
                    }))
                    .text_size(px(12.))
                    .line_height(px(19.))
                    .text_color(rgb(if dialog.test_ok == Some(true) {
                        colors().secondary
                    } else {
                        tone(0xB42318, 0xFF9B94)
                    }))
                    .child(dialog.message.clone()),
            );
        }

        let save = |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            if this
                .account_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.screen == Some(PoolScreen::List))
            {
                this.show_proxy_pool_screen(PoolScreen::Edit(None), window, cx);
            } else {
                this.save_account_dialog(window, cx);
            }
        };
        let (save_label, save_kind) = match dialog.screen {
            Some(PoolScreen::List) => ("新增代理", ButtonKind::Primary),
            Some(PoolScreen::Batch) => ("确认导入有效项", ButtonKind::Primary),
            Some(PoolScreen::Remove(_)) => ("确认删除", ButtonKind::Danger),
            Some(PoolScreen::Edit(_)) => ("保存代理", ButtonKind::Primary),
            None if dialog.removing => ("确认移除", ButtonKind::Danger),
            None if dialog.owner.is_none() => ("创建账号", ButtonKind::Primary),
            None => ("保存", ButtonKind::Primary),
        };
        let cancel = dialog_button(
            "dialog-cancel",
            match dialog.screen {
                Some(PoolScreen::List) => "关闭",
                Some(_) => "返回",
                None => "取消",
            },
            ButtonKind::Secondary,
            cx.listener(|this, _, window, cx| this.close_account_dialog(window, cx)),
        );
        let confirm = dialog_button(
            "dialog-save",
            save_label,
            save_kind,
            cx.listener(move |this, _, window, cx| save(this, window, cx)),
        )
        .when(
            dialog
                .batch
                .as_ref()
                .is_some_and(|batch| !batch.can_confirm()),
            |el| el.opacity(0.45).cursor(CursorStyle::Arrow),
        );
        let footer = if drawer {
            div()
                .flex_none()
                .p_4()
                .border_t_1()
                .border_color(rgb(colors().line))
                .flex()
                .gap_2()
                .when(dialog.screen == Some(PoolScreen::List), |el| {
                    el.child(
                        dialog_button(
                            "pool-batch-import",
                            "批量导入",
                            ButtonKind::Secondary,
                            cx.listener(|this, _, window, cx| {
                                this.show_proxy_pool_screen(PoolScreen::Batch, window, cx)
                            }),
                        )
                        .flex_1(),
                    )
                })
                .child(cancel.flex_1())
                .child(confirm.flex_1())
        } else {
            div()
                .flex_none()
                .px_6()
                .pt_3()
                .pb_5()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .when(!dialog.removing, |element| {
                    element.child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .text_color(rgb(colors().muted))
                            .child("Ctrl+Enter 保存 · Esc 取消"),
                    )
                })
                .child(cancel)
                .child(confirm)
        };

        let panel = div()
            .id("account-modal")
            .size_full()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .bg(rgb(colors().panel))
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .child(footer);
        let surface = if drawer {
            let width = DRAWER;
            div()
                .absolute()
                .top(px(TITLEBAR))
                .right_0()
                .bottom_0()
                .w(px(width))
                .overflow_hidden()
                .child(crate::motion::slide_from_right(
                    panel.border_l_1().border_color(rgb(colors().line)),
                    transition,
                    self.motion_policy,
                    24.,
                ))
        } else {
            let max_height = (f32::from(viewport.height) - 80.).max(360.);
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(crate::motion::animate(
                    panel
                        .w(px((f32::from(viewport.width) - 32.).clamp(300., 480.)))
                        .h_auto()
                        .max_h(px(max_height))
                        .rounded(px(14.))
                        .border_1()
                        .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
                        .shadow_lg(),
                    transition,
                    self.motion_policy,
                    crate::motion::MotionKind::Modal,
                    8.,
                ))
        };
        div()
            .id("account-modal-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.dismiss_account_dialog(window, cx);
                }),
            )
            .child(crate::motion::fade(
                div()
                    .absolute()
                    .inset_0()
                    .bg(rgba(if drawer { 0x00000000 } else { 0x18181B47 })),
                transition,
                self.motion_policy,
            ))
            .child(surface)
            .when(exiting, |element| {
                element.child(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                )
            })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ButtonKind {
    Primary,
    Secondary,
    Danger,
}

fn dialog_button(
    id: &'static str,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (background, hover, text) = match kind {
        ButtonKind::Primary => (
            colors().primary,
            colors().primary_hover,
            colors().on_primary,
        ),
        ButtonKind::Secondary => (colors().fill, colors().fill_hover, colors().text),
        ButtonKind::Danger => (
            colors().danger,
            tone(0xB42318, 0xFF9B94),
            colors().on_primary,
        ),
    };
    let label: SharedString = label.into();
    div()
        .id(id)
        .h(px(34.))
        .px_4()
        .flex_none()
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(background))
        .text_color(rgb(text))
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(hover)))
        .on_click(on_click)
        .child(label)
}

fn link_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_1()
        .rounded_sm()
        .text_size(px(12.))
        .text_color(rgb(colors().blue))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(tone(0xEEF3FE, 0x24334F))))
        .on_click(on_click)
        .child(label)
}

fn radio_mark(active: bool) -> Div {
    div()
        .size(px(16.))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(rgb(if active {
            colors().blue
        } else {
            tone(0xC9C9CF, 0x60606D)
        }))
        .bg(rgb(colors().panel))
        .flex()
        .items_center()
        .justify_center()
        .when(active, |element| {
            element.child(div().size(px(8.)).rounded_full().bg(rgb(colors().blue)))
        })
}

fn checkbox_row(
    id: &'static str,
    label: &'static str,
    checked: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .child(
            div()
                .size(px(16.))
                .flex_none()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(if checked {
                    colors().blue
                } else {
                    tone(0xC9C9CF, 0x60606D)
                }))
                .bg(rgb(if checked {
                    colors().blue
                } else {
                    colors().panel
                }))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .text_color(rgb(colors().on_primary))
                .child(if checked { "✓" } else { "" }),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(colors().secondary))
                .child(label),
        )
        .on_click(on_click)
}

fn field_label(label: &'static str) -> Div {
    div()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(colors().secondary))
        .child(label)
}

fn field_box(input: Entity<TextInput>) -> Div {
    div()
        .rounded_lg()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
        .bg(rgb(colors().panel))
        .child(input)
}

fn dialog_field(label: &'static str, input: Entity<TextInput>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(field_label(label))
        .child(field_box(input))
}
