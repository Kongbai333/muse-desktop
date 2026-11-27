use super::chrome::{glyph, icon_button, keyboard_control};
use super::*;
use crate::{
    proxy_pool::{self, RouteChoice},
    proxy_settings::ProxyDraft,
};
use muse_proxy::ProxyProtocol;
use uuid::Uuid;
use zeroize::Zeroizing;

#[path = "cookie_smoke.rs"]
pub(super) mod cookie_smoke;
#[path = "pool_view.rs"]
mod pool_view;
#[path = "proxy_batch_dialog.rs"]
mod proxy_batch_dialog;
use proxy_batch_dialog::BatchImport;

struct ConnectionResult {
    draft: ProxyDraft,
    result: Result<(), String>,
}

pub(super) enum SourceFeedback {
    Reading(Uuid),
    Updated,
    Changed,
    Failed(String),
}

pub(super) struct PoolWindow {
    host: WeakEntity<MuseWindow>,
    focus: FocusHandle,
    frame: Option<crate::windows_caption::CaptionBridge>,
    border: Option<u32>,
}

impl Render for PoolWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = modal_border();
        if self.border != Some(border) {
            if let Some(frame) = &self.frame {
                frame.set_pool_border(border);
            }
            self.border = Some(border);
        }
        let panel = self
            .host
            .update(cx, |host, cx| {
                host.account_dialog
                    .as_ref()
                    .is_some_and(AccountDialog::is_drawer)
                    .then(|| host.render_account_dialog(window, cx))
            })
            .ok()
            .flatten();
        let host = self.host.clone();
        let save = host.clone();
        div()
            .relative()
            .size_full()
            .font_family("Segoe UI Variable")
            .text_size(px(13.))
            .bg(rgb(colors().panel))
            .text_color(rgb(colors().text))
            .track_focus(&self.focus)
            .key_context("AccountDialog")
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev()
                    } else {
                        window.focus_next()
                    }
                    cx.stop_propagation();
                }
            })
            .on_action(move |_: &CancelAccountDialog, window, cx| {
                let _ = host.update(cx, |host, cx| host.close_account_dialog(window, cx));
            })
            .on_action(move |_: &SaveAccountDialog, window, cx| {
                let _ = save.update(cx, |host, cx| host.save_account_dialog(window, cx));
            })
            .children(panel)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PoolScreen {
    List,
    AddSubscription,
    DeleteSubscription(Uuid),
    Batch,
    Edit(Option<Uuid>),
    Remove(Uuid),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PoolTab {
    #[default]
    Subscriptions,
    Manual,
}

pub(super) struct AccountDialog {
    pub owner: Option<AccountId>,
    pub removing: bool,
    screen: Option<PoolScreen>,
    tab: PoolTab,
    parked: Option<Box<AccountDialog>>,
    source: Option<Uuid>,
    sources_open: bool,
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
    advanced: bool,
    language: Entity<TextInput>,
    timezone: Entity<TextInput>,
    message: String,
    /// Shown as the name placeholder and used when the name is left empty.
    default_name: String,
    test_ok: Option<bool>,
    pending: bool,
    refresh: Option<Uuid>,
    last_draft: ProxyDraft,
    sender: mpsc::Sender<ConnectionResult>,
    receiver: mpsc::Receiver<ConnectionResult>,
    transition: crate::motion::TransitionState,
}
impl AccountDialog {
    pub(super) fn is_drawer(&self) -> bool {
        self.screen.is_some()
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
            tab: PoolTab::default(),
            parked: None,
            source: None,
            sources_open: false,
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
            advanced: false,
            language,
            timezone,
            message: String::new(),
            default_name: String::new(),
            test_ok: None,
            pending: false,
            refresh: None,
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
    fn reading_form(&mut self, ticket: Uuid) -> Option<&mut Self> {
        if self.refresh == Some(ticket) {
            Some(self)
        } else {
            self.parked.as_deref_mut()?.reading_form(ticket)
        }
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
    pub(super) fn tab_account_field(
        &self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = &self.account_dialog else {
            return;
        };
        let fields = match dialog.screen {
            Some(PoolScreen::AddSubscription) => vec![&dialog.name, &dialog.secret],
            Some(PoolScreen::Edit(_)) => vec![
                &dialog.name,
                &dialog.host,
                &dialog.port,
                &dialog.user,
                &dialog.secret,
            ],
            None if !dialog.removing => {
                let mut fields = vec![&dialog.name];
                if dialog.owner.is_none() {
                    fields.push(&dialog.cookie);
                }
                fields.extend([&dialog.language, &dialog.timezone]);
                fields
            }
            _ => return,
        };
        let count = fields.len();
        let next = fields
            .iter()
            .position(|field| field.read(cx).is_focused(window))
            .map_or(0, |index| {
                (index + if backwards { count - 1 } else { 1 }) % count
            });
        window.focus(&fields[next].read(cx).focus_handle(cx));
        cx.stop_propagation();
    }

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
        if self.pool_window.is_some() && self.proxy_drawer_visible() {
            if self.pool_window.map(|handle| handle.window_id())
                != Some(window.window_handle().window_id())
            {
                return;
            }
            if self
                .account_dialog
                .as_ref()
                .is_some_and(|d| d.screen == Some(PoolScreen::List))
            {
                self.finish_pool_window(cx);
                window.remove_window();
            } else {
                self.show_proxy_pool_screen(PoolScreen::List, window, cx);
            }
            return;
        }
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
        if self
            .pool_window
            .is_some_and(|handle| handle.window_id() != window.window_handle().window_id())
        {
            return;
        }
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
            self.save_pool_screen(self.pool_content(dialog, screen), &name, window, cx);
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
        let manual_empty = self.workspace.proxy_profiles.is_empty();
        let Some(dialog) = self.account_dialog.as_mut().filter(|dialog| {
            !dialog.transition.is_exiting()
                && (matches!(dialog.screen, Some(PoolScreen::Edit(_)))
                    || (manual_empty
                        && dialog.screen == Some(PoolScreen::List)
                        && dialog.tab == PoolTab::Manual))
        }) else {
            return false;
        };
        let mut changed = false;
        let current = dialog.proxy(cx);
        if current != dialog.last_draft {
            dialog.last_draft = current;
            dialog.message.clear();
            dialog.test_ok = None;
            changed = true;
        }
        while let Ok(result) = dialog.receiver.try_recv() {
            dialog.pending = false;
            changed = true;
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
        if let Some(existing) = self.pool_window {
            cx.defer(move |cx| {
                let _ = existing.update(cx, |_, window, _| window.activate_window());
            });
            return;
        }
        if self.updates.open
            || !self.save_enabled
            || self.cookie_dialog.is_some()
            || self.transfer_dialog.is_some()
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
        self.account_context = None;
        if let Some(view) = self
            .workspace
            .active_browser_identity()
            .and_then(|identity| self.browsers.view(identity.tab_id))
        {
            let _ = view.focus_parent();
        }
        self.show_proxy_pool_screen(PoolScreen::List, window, cx);
        let host = cx.entity().downgrade();
        let focus = cx.focus_handle();
        self.pool_focus = Some(focus.clone());
        let display = window.display(cx).map(|display| display.id());
        let work = crate::windows_caption::work_area(window)
            .unwrap_or_else(|| Bounds::centered(display, size(px(1112.), px(792.)), cx));
        let bounds = Bounds::centered_at(
            work.center(),
            size(
                (work.size.width - px(32.)).clamp(px(1.), px(1080.)),
                (work.size.height - px(32.)).clamp(px(1.), px(760.)),
            ),
        );
        cx.defer(move |cx| {
            let owner = host.clone();
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    display_id: display,
                    window_min_size: Some(size(
                        bounds.size.width.min(px(420.)),
                        bounds.size.height.min(px(360.)),
                    )),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Muse · 代理管理".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    let on_close = host.clone();
                    window.on_window_should_close(cx, move |_, cx| {
                        let _ = on_close.update(cx, |host, cx| host.finish_pool_window(cx));
                        true
                    });
                    cx.new(|cx| {
                        if let Some(entity) = host.upgrade() {
                            cx.observe(&entity, |_, _, cx| cx.notify()).detach();
                            cx.observe_release_in(&entity, window, |_, _, window, _| {
                                window.remove_window()
                            })
                            .detach();
                        }
                        window.focus(&focus);
                        let frame =
                            crate::windows_caption::CaptionBridge::install_pool(window).ok();
                        PoolWindow {
                            host,
                            focus,
                            frame,
                            border: None,
                        }
                    })
                },
            );
            let _ = owner.update(cx, |host, cx| {
                match opened {
                    Ok(handle) => host.pool_window = Some(handle),
                    Err(_) => {
                        host.finish_pool_window(cx);
                        host.status = "无法打开代理管理窗口".into();
                    }
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    fn finish_pool_window(&mut self, cx: &mut Context<Self>) {
        self.pool_window = None;
        self.pool_focus = None;
        self.account_dialog = self.account_dialog_return.take();
        cx.notify();
    }

    fn refresh_subscription(&mut self, existing: Option<Uuid>, cx: &mut Context<Self>) {
        let Some(dialog) = self.account_dialog.as_mut() else {
            return;
        };
        let Some(window) = self.pool_window else {
            return;
        };
        if dialog.refresh.is_some()
            || existing.is_some_and(|id| {
                matches!(
                    self.pool_feedback.get(&id),
                    Some(SourceFeedback::Reading(_))
                )
            })
        {
            return;
        }
        let (name, url) = if let Some(id) = existing {
            let Some(sub) = self.workspace.subscriptions.iter().find(|sub| sub.id == id) else {
                return;
            };
            match crate::subscriptions::source(&self.data_dir, sub) {
                Ok(source) => (sub.name.clone(), source),
                Err(error) => {
                    self.pool_feedback.insert(id, SourceFeedback::Failed(error));
                    cx.notify();
                    return;
                }
            }
        } else {
            (
                dialog.name.read(cx).text().trim().to_owned(),
                Zeroizing::new(dialog.secret.read(cx).text().trim().to_owned()),
            )
        };
        if name.is_empty() || name.chars().count() > 48 || name.contains("://") {
            dialog.message = "请填写 1–48 字的订阅名称".into();
            cx.notify();
            return;
        }
        if existing.is_none() && url.is_empty() {
            dialog.message = "请填写订阅地址".into();
            cx.notify();
            return;
        }
        let nonce = Uuid::new_v4();
        if let Some(id) = existing {
            self.pool_feedback
                .insert(id, SourceFeedback::Reading(nonce));
        } else {
            dialog.refresh = Some(nonce);
            dialog.message.clear();
        }
        cx.spawn(async move |host, cx| {
            let (url, result) = cx
                .background_executor()
                .spawn(async move {
                    let result = crate::subscriptions::fetch(&url);
                    (url, result)
                })
                .await;
            let added = host.update(cx, |host, cx| {
                    if let Some(id) = existing {
                        if !matches!(host.pool_feedback.get(&id), Some(SourceFeedback::Reading(ticket)) if *ticket == nonce) { return None; }
                    } else {
                        host.account_dialog.as_mut()?.reading_form(nonce)?;
                    }
                    let result = result.and_then(|entries| {
                        crate::subscriptions::replace(
                            &host.data_dir,
                            &mut host.workspace,
                            existing,
                            &name,
                            &url,
                            entries,
                        )
                    });
                    match result {
                        Ok(affected) => {
                            let affected_count = affected.len();
                            for id in affected {
                                host.browsers.invalidate_account(id);
                                host.fail_studio_job(id, "绑定的节点已变更，请检查账号出口", cx);
                            }
                            if let Some(id) = existing.or_else(|| host.workspace.subscriptions.last().map(|sub| sub.id)) {
                                host.pool_feedback.insert(id, if affected_count == 0 { SourceFeedback::Updated } else { SourceFeedback::Changed });
                            }
                            cx.notify();
                            if existing.is_none() {
                                let id = host.workspace.subscriptions.last()?.id;
                                let dialog = host.account_dialog.as_mut()?.reading_form(nonce)?;
                                dialog.refresh = None;
                                dialog.source = Some(id);
                                dialog.name.update(cx, |input, cx| input.set_text("", cx));
                                dialog.secret.update(cx, |input, cx| input.set_text("", cx));
                                if dialog.screen == Some(PoolScreen::AddSubscription) { return Some(id); }
                            }
                        }
                        Err(error) => {
                            if let Some(id) = existing {
                                host.pool_feedback.insert(id, SourceFeedback::Failed(error));
                            } else if let Some(dialog) = host.account_dialog.as_mut().and_then(|dialog| dialog.reading_form(nonce)) {
                                dialog.refresh = None;
                                dialog.message = error;
                                dialog.test_ok = Some(false);
                            }
                        }
                    }
                    cx.notify();
                    None
            }).ok().flatten();
            if let Some(id) = added {
                let _ = window.update(cx, |_, window, cx| { let _ = host.update(cx, |host, cx| {
                    host.show_proxy_pool_screen(PoolScreen::List, window, cx);
                    if let Some(dialog) = &mut host.account_dialog { dialog.source = Some(id); }
                }); });
            }
        })
        .detach();
        cx.notify();
    }

    fn render_subscriptions(
        &self,
        dialog: &AccountDialog,
        narrow: bool,
        cx: &Context<Self>,
    ) -> Div {
        let mut sources: Vec<_> = self.workspace.subscriptions.iter().collect();
        sources.sort_by(|left, right| (&left.name, left.id).cmp(&(&right.name, right.id)));
        let selected = sources
            .iter()
            .copied()
            .find(|sub| Some(sub.id) == dialog.source)
            .or_else(|| sources.first().copied());
        let nav = div()
            .flex()
            .flex_col()
            .gap_2()
            .min_h_0()
            .flex_none()
            .when(!narrow, |el| {
                el.w(px(196.))
                    .pr_3()
                    .border_r_1()
                    .border_color(rgb(colors().line))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(if narrow && selected.is_some() {
                        keyboard_control(div().id("source-selector"))
                            .flex_1()
                            .min_w_0()
                            .px_2()
                            .h(px(34.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded_md()
                            .bg(rgb(colors().fill))
                            .cursor_pointer()
                            .child(
                                div().flex_1().truncate().child(
                                    selected.map(|sub| sub.name.clone()).unwrap_or_default(),
                                ),
                            )
                            .child(glyph("chevron-down", 14., colors().secondary))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(dialog) = &mut this.account_dialog {
                                    dialog.sources_open = !dialog.sources_open;
                                }
                                cx.notify();
                            }))
                            .into_any_element()
                    } else {
                        section_title("订阅来源").flex_1().into_any_element()
                    })
                    .child(icon_button(
                        "pool-add-sub",
                        "plus",
                        "添加订阅",
                        true,
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.show_proxy_pool_screen(PoolScreen::AddSubscription, window, cx)
                        }),
                    )),
            )
            .when(!narrow || dialog.sources_open, |el| {
                el.child(
                    div()
                        .id("pool-sources")
                        .min_h_0()
                        .when(narrow, |list| list.max_h(px(86.)))
                        .overflow_y_scroll()
                        .children(sources.iter().map(|sub| {
                            let id = sub.id;
                            let active = selected.is_some_and(|sub| sub.id == id);
                            keyboard_control(div().id(id))
                                .w_full()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_2()
                                .h(px(34.))
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(colors().fill_hover)))
                                .when(active, |el| {
                                    el.bg(rgb(colors().selected))
                                        .font_weight(FontWeight::SEMIBOLD)
                                })
                                .child(glyph(
                                    "shield-check",
                                    15.,
                                    if active {
                                        colors().blue
                                    } else {
                                        colors().secondary
                                    },
                                ))
                                .child(div().flex_1().min_w_0().truncate().child(sub.name.clone()))
                                .tooltip({
                                    let name = sub.name.clone();
                                    move |_, cx| {
                                        cx.new(|_| super::chrome::AccountHint(name.clone())).into()
                                    }
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(dialog) = &mut this.account_dialog {
                                        dialog.source = Some(id);
                                        dialog.sources_open = false;
                                        dialog.message.clear();
                                    }
                                    cx.notify();
                                }))
                        })),
                )
            });
        div()
            .flex()
            .flex_1()
            .min_h_0()
            .gap_3()
            .when(narrow, |el| el.flex_col())
            .child(nav)
            .children(selected.map(|sub| {
                let id = sub.id;
                let feedback = self.pool_feedback.get(&id);
                let reading = matches!(feedback, Some(SourceFeedback::Reading(_)));
                let ready = sub.nodes.iter().filter(|node| node.selectable()).count();
                div()
                    .id(ElementId::Name(id.to_string().into()))
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .py_1()
                            .bg(rgb(colors().surface))
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .id("selected-source-name")
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(sub.name.clone())
                                    .tooltip({
                                        let name = sub.name.clone();
                                        move |_, cx| {
                                            cx.new(|_| super::chrome::AccountHint(name.clone()))
                                                .into()
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(12.))
                                    .text_color(rgb(colors().secondary))
                                    .child(format!("已加入 {ready} / {}", sub.nodes.len())),
                            )
                            .child(icon_button(
                                "refresh-sub",
                                "reload",
                                if reading {
                                    "正在读取订阅"
                                } else {
                                    "刷新订阅"
                                },
                                !reading,
                                false,
                                cx.listener(move |this, _, _, cx| {
                                    this.refresh_subscription(Some(id), cx)
                                }),
                            ))
                            .child(icon_button(
                                "delete-sub",
                                "trash",
                                "删除订阅",
                                !reading,
                                false,
                                cx.listener(move |this, _, window, cx| {
                                    this.show_proxy_pool_screen(
                                        PoolScreen::DeleteSubscription(id),
                                        window,
                                        cx,
                                    )
                                }),
                            )),
                    )
                    .when_some(feedback, |el, state| {
                        let (text, color, icon) = match state {
                            SourceFeedback::Reading(_) => {
                                ("正在读取订阅…".into(), colors().secondary, "reload")
                            }
                            SourceFeedback::Updated => {
                                ("节点配置已更新".into(), colors().secondary, "circle-check")
                            }
                            SourceFeedback::Changed => (
                                "节点配置已更新，请检查受影响账号的绑定".into(),
                                colors().attention,
                                "help-circle",
                            ),
                            SourceFeedback::Failed(reason) => {
                                (reason.clone(), colors().danger, "help-circle")
                            }
                        };
                        el.child(
                            div()
                                .px_2()
                                .py_1()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(glyph(icon, 14., color).flex_none())
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .text_size(px(crate::theme::CAPTION))
                                        .text_color(rgb(color))
                                        .child(div().w_full().child(text))
                                        .when(matches!(state, SourceFeedback::Failed(_)), |el| {
                                            el.child(
                                                div()
                                                    .w_full()
                                                    .text_color(rgb(colors().secondary))
                                                    .child("已保留原节点和账号绑定，可重新刷新。"),
                                            )
                                        }),
                                ),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .pt_2()
                            .pb_1()
                            .flex()
                            .gap_2()
                            .text_size(px(crate::theme::CAPTION))
                            .text_color(rgb(colors().secondary))
                            .child(div().flex_1().child("节点名称"))
                            .child(column(54., "协议"))
                            .child(column(94., "加入代理池")),
                    )
                    .child(
                        div()
                            .id("source-nodes")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .when(sub.nodes.is_empty(), |el| {
                                el.child(quiet_line("没有缓存节点，可刷新订阅。"))
                            })
                            .children(sub.nodes.iter().map(|node| {
                                let ready = node.revision.is_some();
                                let joined = node.selectable();
                                let node_id = node.id;
                                let bindings = self
                                    .workspace
                                    .accounts
                                    .iter()
                                    .filter(|account| {
                                        RouteChoice::from(account.network)
                                            == RouteChoice::Node(node_id)
                                    })
                                    .count();
                                let header = div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id(node.id)
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .child(node.name.clone())
                                            .tooltip({
                                                let name = node.name.clone();
                                                move |_, cx| {
                                                    cx.new(|_| {
                                                        super::chrome::AccountHint(name.clone())
                                                    })
                                                    .into()
                                                }
                                            }),
                                    )
                                    .child(
                                        column(54., node.protocol.to_ascii_uppercase())
                                            .text_color(rgb(colors().secondary)),
                                    )
                                    .child(
                                        div()
                                            .id(ElementId::Name(format!("join-{node_id}").into()))
                                            .w(px(94.))
                                            .flex_none()
                                            .h(px(28.))
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .rounded_md()
                                            .when(ready, |el| {
                                                keyboard_control(el)
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(rgb(colors().fill_hover)))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        let result = proxy_pool::set_membership(
                                                            &this.data_dir,
                                                            &mut this.workspace,
                                                            id,
                                                            node_id,
                                                            !joined,
                                                        );
                                                        if let Some(dialog) =
                                                            &mut this.account_dialog
                                                        {
                                                            dialog.message =
                                                                result.err().unwrap_or_default();
                                                        }
                                                        cx.notify();
                                                    }))
                                            })
                                            .child(
                                                div()
                                                    .w(px(32.))
                                                    .h(px(18.))
                                                    .rounded_full()
                                                    .p(px(2.))
                                                    .flex()
                                                    .items_center()
                                                    .bg(rgb(if joined {
                                                        colors().primary
                                                    } else {
                                                        colors().track
                                                    }))
                                                    .when(joined, |el| el.justify_end())
                                                    .child(div().size(px(14.)).rounded_full().bg(
                                                        rgb(if joined {
                                                            colors().on_primary
                                                        } else {
                                                            colors().panel
                                                        }),
                                                    )),
                                            )
                                            .child(if !ready {
                                                "受限"
                                            } else if joined {
                                                "已开"
                                            } else {
                                                "已关"
                                            }),
                                    );
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_none()
                                    .px_2()
                                    .py(px(6.))
                                    .border_t_1()
                                    .border_color(rgb(colors().line))
                                    .text_size(px(13.))
                                    .child(header)
                                    .when(bindings > 0, |row| {
                                        row.child(
                                            div()
                                                .text_size(px(crate::theme::CAPTION))
                                                .text_color(rgb(colors().secondary))
                                                .child(format!(
                                                    "{bindings} 个账号使用 · 现有绑定不变"
                                                )),
                                        )
                                    })
                                    .when(!ready, |row| {
                                        row.child(
                                            div()
                                                .w_full()
                                                .pt(px(2.))
                                                .text_size(px(crate::theme::CAPTION))
                                                .text_color(rgb(colors().attention))
                                                .child(node.reason.clone().unwrap_or_else(|| {
                                                    "刷新订阅可查看原因".into()
                                                })),
                                        )
                                    })
                            })),
                    )
            }))
    }

    fn show_proxy_pool_screen(
        &mut self,
        screen: PoolScreen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if screen == PoolScreen::List {
            let mut dialog = self
                .account_dialog
                .take()
                .and_then(|dialog| {
                    if dialog.screen == Some(PoolScreen::List) {
                        Some(dialog)
                    } else {
                        dialog.parked.map(|list| *list)
                    }
                })
                .unwrap_or_else(|| self.new_pool_tab(PoolTab::Subscriptions, cx));
            dialog.sources_open = false;
            self.account_dialog = Some(dialog);
            if let Some(focus) = &self.pool_focus {
                window.focus(focus);
            }
            cx.notify();
            return;
        }
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
        if let Some(previous) = self
            .account_dialog
            .as_ref()
            .filter(|dialog| dialog.screen.is_some())
        {
            dialog.tab = previous.tab;
            dialog.source = previous.source;
        }
        if screen == PoolScreen::Edit(None) {
            dialog.set_default_name(
                format!("代理 {}", self.workspace.proxy_profiles.len() + 1),
                cx,
            );
        }
        dialog.screen = Some(screen);
        if screen == PoolScreen::AddSubscription {
            dialog
                .secret
                .update(cx, |input, _| input.set_placeholder("订阅地址"));
        }
        if screen == PoolScreen::Batch {
            dialog.batch = Some(BatchImport::new());
        }
        dialog.message = message;
        if self.account_dialog.is_some() || self.account_dialog_return.is_some() {
            dialog.transition = TransitionState::visible();
        }
        dialog.parked = self.account_dialog.take().map(Box::new);
        if matches!(screen, PoolScreen::Edit(_) | PoolScreen::AddSubscription) {
            window.focus(&dialog.name.read(cx).focus_handle(cx));
        } else {
            window.focus(self.pool_focus.as_ref().unwrap_or(&self.focus));
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
            PoolScreen::AddSubscription => {
                self.refresh_subscription(None, cx);
                return;
            }
            PoolScreen::DeleteSubscription(id) => {
                crate::subscriptions::remove(&self.data_dir, &mut self.workspace, id)
            }
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
                if let PoolScreen::DeleteSubscription(id) = screen {
                    self.pool_feedback.remove(&id);
                }
                for id in affected {
                    self.browsers.invalidate_account(id);
                    self.fail_studio_job(id, "账号代理已更新，请重新提交", cx);
                }
                self.refresh_proxy_editor(cx);
                if screen == PoolScreen::Edit(None)
                    && self
                        .account_dialog
                        .as_ref()
                        .is_some_and(|d| d.screen == Some(PoolScreen::List))
                {
                    let mut fresh = self.new_pool_tab(PoolTab::Manual, cx);
                    fresh.parked = self.account_dialog.take().and_then(|d| d.parked);
                    self.account_dialog = Some(fresh);
                }
                self.show_proxy_pool_screen(PoolScreen::List, window, cx);
            }
            Err(error) => {
                if let Some(dialog) = self.account_dialog.as_mut() {
                    dialog.message = error;
                    dialog.test_ok = Some(false);
                }
                cx.notify();
            }
        }
    }

    fn render_pool_list(&self, cx: &Context<Self>) -> Div {
        let profiles = &self.workspace.proxy_profiles;
        if profiles.is_empty() {
            return quiet_line("尚未添加手动代理");
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(profiles.iter().map(|profile| {
                let id = profile.id;
                let users = proxy_pool::references(&self.workspace, id);
                let count = users.len();
                div()
                    .id(ElementId::Name(id.to_string().into()))
                    .px_3()
                    .h(px(46.))
                    .rounded_lg()
                    .flex()
                    .items_center()
                    .gap_3()
                    .hover(|style| style.bg(rgb(colors().fill)))
                    .child(glyph("world", 16., colors().secondary).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .id("manual-proxy-name")
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(profile.name.clone())
                            .tooltip({
                                let name = profile.name.clone();
                                move |_, cx| {
                                    cx.new(|_| super::chrome::AccountHint(name.clone())).into()
                                }
                            }),
                    )
                    .child(
                        column(52., profile.protocol.to_ascii_uppercase())
                            .text_color(rgb(colors().secondary)),
                    )
                    .child(
                        column(72., format!("{count} 个账号")).text_color(rgb(colors().secondary)),
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
        for sub in &self.workspace.subscriptions {
            for node in sub.nodes.iter().filter(|node| node.selectable()) {
                choices.push((
                    ElementId::Name(node.id.to_string().into()),
                    format!("{} / {}", sub.name, node.name),
                    format!("{} · 固定订阅节点", node.protocol.to_ascii_uppercase()),
                    RouteChoice::Node(node.id),
                ));
            }
        }
        let current_note = if matches!(dialog.route, RouteChoice::Node(_))
            && !choices
                .iter()
                .any(|(_, _, _, route)| *route == dialog.route)
        {
            let retained = self.workspace.subscriptions.iter().find_map(|sub| {
                sub.nodes
                    .iter()
                    .find(|node| {
                        RouteChoice::Node(node.id) == dialog.route && node.revision.is_some()
                    })
                    .map(|node| format!("{} / {}", sub.name, node.name))
            });
            let preserving = dialog
                .owner
                .and_then(|id| self.workspace.account(id))
                .is_some_and(|account| RouteChoice::from(account.network) == dialog.route);
            Some(match retained {
                Some(name) if preserving => format!("当前：{name} · 未加入代理池 · 现有绑定不变"),
                Some(name) => format!("{name} 未加入代理池，请重新选择"),
                None => "原订阅节点不可用 · 未切换网络".into(),
            })
        } else {
            None
        };
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
            .when_some(current_note, |el, note| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(colors().secondary))
                        .child(note),
                )
            })
            .when(
                matches!(dialog.route, RouteChoice::Pool(_) | RouteChoice::Node(_)),
                |element| {
                    element.child(link_button(
                        "advanced-network",
                        if dialog.advanced {
                            "收起高级设置"
                        } else {
                            "高级设置"
                        },
                        cx.listener(|this, _, _, cx| {
                            if let Some(dialog) = &mut this.account_dialog {
                                dialog.advanced = !dialog.advanced;
                            }
                            cx.notify();
                        }),
                    ))
                },
            )
            .when(
                dialog.advanced && !matches!(dialog.route, RouteChoice::System),
                |element| {
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
                },
            )
            .when(
                self.workspace.proxy_profiles.is_empty() && self.workspace.subscriptions.is_empty(),
                |element| {
                    element.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(colors().muted))
                            .child("还没有代理。点「管理代理池」新增后，这里就可以选择。"),
                    )
                },
            )
    }

    fn render_proxy_form(&self, dialog: &AccountDialog, cx: &Context<Self>) -> Div {
        let segment = |id, label, selected, protocol: Option<ProxyProtocol>| {
            keyboard_control(div().id(id))
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
        let auth = dialog.advanced
            || !dialog.user.read(cx).text().is_empty()
            || !dialog.secret.read(cx).text().is_empty();
        div()
            .flex()
            .flex_col()
            .gap_4()
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(glyph(
                        if auth {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        14.,
                        colors().secondary,
                    ))
                    .child(link_button(
                        "pool-auth",
                        "身份认证（可选）",
                        cx.listener(|this, _, _, cx| {
                            if let Some(dialog) = &mut this.account_dialog {
                                dialog.advanced = !dialog.advanced;
                            }
                            cx.notify();
                        }),
                    )),
            )
            .when(auth, |el| {
                el.child(dialog_field("用户名", dialog.user.clone()))
                    .child(dialog_field("密码", dialog.secret.clone()))
            })
            .child(keyboard_control(checkbox_row(
                "dialog-force",
                "本机地址也经过此代理",
                dialog.force,
                cx.listener(|this, _, _, cx| {
                    if let Some(dialog) = this.account_dialog.as_mut() {
                        dialog.force = !dialog.force;
                    }
                    cx.notify();
                }),
            )))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(link_button(
                        "dialog-paste",
                        "粘贴代理地址",
                        cx.listener(|this, _, _, cx| this.paste_dialog_proxy(cx)),
                    ))
                    .child(if dialog.pending {
                        disabled_button("测试中…")
                    } else {
                        link_button(
                            "dialog-test",
                            "测试隧道",
                            cx.listener(|this, _, _, cx| this.test_dialog_proxy(cx)),
                        )
                        .into_any_element()
                    }),
            )
            .when(!dialog.message.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(if dialog.test_ok == Some(false) {
                            colors().danger
                        } else {
                            colors().secondary
                        }))
                        .child(dialog.message.clone()),
                )
            })
    }

    pub(super) fn render_account_dialog(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let dialog = self.account_dialog.as_ref().unwrap();
        if let Some(screen) = dialog.screen {
            return self.render_pool_view(screen, dialog, window, cx);
        }
        let transition = dialog.transition;
        let exiting = transition.is_exiting();
        let viewport = window.viewport_size();
        let (title, subtitle) = if dialog.removing {
            ("移除账号", "只从账号列表移除")
        } else if dialog.owner.is_some() {
            (
                "编辑账号",
                "改名不会重新加载网页；更换网络或指纹模式会重建浏览器，登录数据保留。",
            )
        } else {
            ("新增账号", "每个账号使用独立的浏览器环境，登录互不影响。")
        };

        let close = icon_button(
            "dialog-close",
            "x",
            "关闭",
            true,
            false,
            cx.listener(move |this, _, window, cx| {
                this.close_account_dialog(window, cx);
            }),
        );
        let heading = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(px(crate::theme::HEADING))
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
        let header = div()
            .flex_none()
            .px_6()
            .pt_5()
            .pb_2()
            .flex()
            .items_start()
            .gap_3()
            .child(heading)
            .child(close);

        let mut body = div()
            .id("account-dialog-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .px_6()
            .pt_3()
            .pb_2();
        body = if dialog.removing {
            body.child(
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
                            .child("账号的浏览器数据和登录状态会保留在本机，之后可以重新导入。"),
                    ),
            )
        } else {
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
        };
        let notice = (!dialog.message.is_empty()).then(|| {
            div()
                .flex_none()
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
                .child(dialog.message.clone())
        });
        body = body.children(notice);

        let (save_label, save_kind) = if dialog.removing {
            ("确认移除", ButtonKind::Danger)
        } else if dialog.owner.is_none() {
            ("创建账号", ButtonKind::Primary)
        } else {
            ("保存", ButtonKind::Primary)
        };
        let cancel = dialog_button(
            "dialog-cancel",
            "取消",
            ButtonKind::Secondary,
            cx.listener(|this, _, window, cx| this.close_account_dialog(window, cx)),
        );
        let confirm = dialog_button(
            "dialog-save",
            save_label,
            save_kind,
            cx.listener(|this, _, window, cx| this.save_account_dialog(window, cx)),
        )
        .into_any_element();
        let footer = div()
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
            .child(confirm);

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
        let surface = {
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
                        .border_color(rgb(modal_border()))
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
                div().absolute().inset_0().bg(rgba(0x18181B47)),
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
pub(super) enum ButtonKind {
    Primary,
    Secondary,
    Danger,
}

fn disabled_button(label: &'static str) -> AnyElement {
    div()
        .h(px(34.))
        .px_4()
        .flex()
        .items_center()
        .rounded_lg()
        .bg(rgb(colors().fill))
        .text_color(rgb(colors().muted))
        .child(label)
        .into_any_element()
}

fn section_title(label: &'static str) -> Div {
    div()
        .text_size(px(crate::theme::SECTION))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label)
}

fn column(width: f32, text: impl Into<SharedString>) -> Div {
    div().w(px(width)).flex_none().child(text.into())
}

fn quiet_line(label: &'static str) -> Div {
    div()
        .px_2()
        .py_2()
        .text_size(px(crate::theme::CAPTION))
        .text_color(rgb(colors().secondary))
        .child(label)
}

pub(super) fn dialog_button(
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
    keyboard_control(div().id(id))
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
    keyboard_control(div().id(id))
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

fn field_box(input: Entity<TextInput>) -> impl IntoElement {
    crate::input::InputField::new(
        input,
        div()
            .rounded_lg()
            .overflow_hidden()
            .border_1()
            .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
            .bg(rgb(colors().panel)),
    )
}

fn modal_border() -> u32 {
    tone(0xE4E4E7, 0x3F3F46)
}

pub(super) fn dialog_field(label: &'static str, input: Entity<TextInput>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(field_label(label))
        .child(field_box(input))
}
