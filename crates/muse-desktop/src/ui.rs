use crate::theme::{colors, tone};
use crate::{
    browser::{BrowserMessage, BrowserPool, BrowserRequest},
    input::TextInput,
    motion::{MotionKind, TransitionState},
};
use gpui::{prelude::*, *};
use muse_core::{AccountId, BrowserEvent, HOME_URL, Workspace};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use wry::{
    Rect,
    dpi::{LogicalPosition, LogicalSize},
};

actions!(
    muse,
    [
        Back,
        Forward,
        Reload,
        Home,
        NewTab,
        CloseTab,
        FocusAddress,
        SubmitAddress,
        DismissStudioPanels,
        CancelAccountDialog,
        SaveAccountDialog,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        CopyStudioImage
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        crate::platform::key_binding("alt-left", Back, Some("Muse")),
        crate::platform::key_binding("alt-right", Forward, Some("Muse")),
        crate::platform::key_binding("ctrl-r", Reload, Some("Muse")),
        crate::platform::key_binding("ctrl-l", FocusAddress, Some("Muse")),
        crate::platform::key_binding("ctrl-t", NewTab, Some("Muse")),
        crate::platform::key_binding("ctrl-w", CloseTab, Some("Muse")),
        crate::platform::key_binding("enter", SubmitAddress, Some("TextInput")),
        crate::platform::key_binding("escape", DismissStudioPanels, Some("Muse")),
        crate::platform::key_binding("escape", CancelAccountDialog, Some("AccountDialog")),
        crate::platform::key_binding("escape", CancelAccountDialog, Some("CookieDialog")),
        crate::platform::key_binding("escape", CancelAccountDialog, Some("TransferDialog")),
        crate::platform::key_binding("escape", CancelAccountDialog, Some("UpdateDialog")),
        crate::platform::key_binding("ctrl-enter", SaveAccountDialog, Some("TransferDialog")),
        crate::platform::key_binding("escape", CancelAccountDialog, Some("AccountContext")),
        crate::platform::key_binding("ctrl-enter", SaveAccountDialog, Some("AccountDialog")),
        crate::platform::key_binding("ctrl-=", ZoomIn, Some("Muse")),
        crate::platform::key_binding("ctrl-+", ZoomIn, Some("Muse")),
        crate::platform::key_binding("ctrl--", ZoomOut, Some("Muse")),
        crate::platform::key_binding("ctrl-0", ZoomReset, Some("Muse")),
        crate::platform::key_binding("ctrl-c", CopyStudioImage, Some("Muse")),
    ]);
}

/// Invite counts are re-read on this cadence for every running account.
const INVITE_EVERY: Duration = Duration::from_secs(30 * 60);
/// When a read could not run (page busy, user active, popover not found).
const INVITE_RETRY: Duration = Duration::from_secs(5 * 60);
const INVITE_TIMEOUT: Duration = Duration::from_secs(25);
/// Hidden, idle account pages are suspended after this long.
const SLEEP_AFTER_MS: u64 = 10 * 60 * 1000;

struct Smoke {
    stage: u8,
    started: Instant,
    first: AccountId,
    original_tab: muse_core::TabId,
    first_marker: Option<String>,
    second: Option<AccountId>,
    edit_report: Option<serde_json::Value>,
    edit_before: Option<muse_core::BrowserIdentity>,
    edit_count: usize,
    paint_started: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InspectorSection {
    Account,
    Proxy,
}

pub struct MuseWindow {
    workspace: Workspace,
    browsers: BrowserPool,
    receiver: mpsc::Receiver<BrowserMessage>,
    data_dir: PathBuf,
    address: Entity<TextInput>,
    account_name: Entity<TextInput>,
    focus: FocusHandle,
    home: String,
    diagnostic_origin: Option<String>,
    status: String,
    save_enabled: bool,
    pending_save: Option<Instant>,
    display: muse_core::DisplayPreferences,
    remove_confirmation: Option<AccountId>,
    markers: HashMap<AccountId, String>,
    ready_pages: HashMap<muse_core::TabId, String>,
    smoke: Option<Smoke>,
    _bounds_observer: Subscription,
    show_start_page: bool,
    sidebar: crate::sidebar_state::SidebarState,
    sidebar_motion: crate::motion::MotionValue,
    sidebar_dragging: bool,
    inspector_open: bool,
    inspector_motion: crate::motion::MotionValue,
    motion_policy: crate::motion::MotionPolicy,
    navigation_transition: crate::motion::TransitionState,
    studio_panel: Option<(studio_ui::StudioPanel, TransitionState)>,
    session_probe: Option<crate::session_probe::SessionProbe>,
    proxy_editor: proxy_editor::ProxyEditor,
    inspector_section: InspectorSection,
    saved_window: Option<crate::window_state::SavedWindow>,
    pending_window_save: Option<Instant>,
    _caption: Option<crate::windows_caption::CaptionBridge>,
    avatars: crate::site_avatar::AvatarCache,
    plan_labels: HashMap<AccountId, String>,
    invites: HashMap<AccountId, crate::account_cache::InviteInfo>,
    invite_jobs: HashMap<AccountId, (muse_core::BrowserIdentity, Instant)>,
    /// Latest still frame of the active page, captured ahead of time.
    page_snapshot: Option<(muse_core::TabId, std::sync::Arc<RenderImage>, Instant)>,
    /// The frame shown in place of the page while a dialog or menu is open.
    freeze_frame: Option<(muse_core::TabId, std::sync::Arc<RenderImage>)>,
    freeze_checked: bool,
    snapshot_request: Option<(u64, muse_core::TabId, Instant)>,
    snapshot_counter: u64,
    /// Account whose animated avatar is playing, and since when.
    avatar_hover: Option<(AccountId, Instant)>,
    sleeping_pages: usize,
    invite_next: HashMap<AccountId, Instant>,
    account_dialog: Option<account_dialog::AccountDialog>,
    account_dialog_return: Option<account_dialog::AccountDialog>,
    cached_accounts: HashSet<AccountId>,
    refresh_accounts: HashMap<AccountId, (muse_core::BrowserIdentity, Instant)>,
    studio: crate::studio::Studio,
    cookie_dialog: Option<crate::cookie_export::CookieDialog>,
    transfer_dialog: Option<transfer_ui::TransferDialog>,
    cookie_logins: HashMap<AccountId, (muse_core::BrowserIdentity, Instant)>,
    account_context: Option<(AccountId, gpui::Point<Pixels>, TransitionState)>,
    updates: update_ui::UpdateUi,
}

impl MuseWindow {
    pub fn new(
        data_dir: PathBuf,
        diagnostic_home: Option<String>,
        smoke: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let display = muse_core::DisplayPreferences::load(&data_dir);
        crate::theme::set_mode(display.theme);
        let (mut workspace, mut startup_error) =
            match Workspace::load(&data_dir.join("workspace.json")) {
                Ok(state) => (state, None),
                Err(error) => (Workspace::new(), Some(error)),
            };
        if !smoke && startup_error.is_none() {
            startup_error = crate::proxy_pool::migrate(&data_dir, &mut workspace).err();
        }
        if workspace.accounts.is_empty() {
            workspace.add_account("主账号");
        }
        let home = diagnostic_home.clone().unwrap_or_else(|| HOME_URL.into());
        if let Some(ref local_home) = diagnostic_home {
            for account in &mut workspace.accounts {
                for tab in &mut account.tabs {
                    tab.url = local_home.clone();
                }
            }
        }
        let diagnostic_origin = diagnostic_home
            .as_ref()
            .and_then(|url| url::Url::parse(url).ok())
            .map(|url| url.origin().ascii_serialization());
        let address = cx.new(|cx| TextInput::new(&home, cx));
        let name = workspace
            .active_account()
            .map(|a| a.name.as_str())
            .unwrap_or("");
        let account_name = cx.new(|cx| TextInput::new(name, cx));
        let (sender, receiver) = mpsc::channel();
        let first_identity = workspace.active_browser_identity();
        let smoke = smoke.then(|| {
            let identity = first_identity.expect("default account exists");
            Smoke {
                stage: 0,
                started: Instant::now(),
                first: identity.account_id,
                original_tab: identity.tab_id,
                first_marker: None,
                second: None,
                edit_report: None,
                edit_before: None,
                edit_count: 0,
                paint_started: None,
            }
        });
        let observer = cx.observe_window_bounds(window, |this, window, _| {
            this.browsers
                .sync_visibility(this.visible_browser_identity(), this.browser_bounds(window));
            this.capture_window(window);
        });
        cx.on_app_quit(|this, cx| {
            this.capture_page_zoom();
            this.persist_display();
            this.clear_cookie_dialog();
            this.clear_transfer(cx);
            this.persist();
            this.persist_window();
            this.studio.save(&this.data_dir);
            async {}
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                // WebView2 construction pumps native events. A timer can wake
                // while GPUI still holds the window borrow; retry on the next
                // tick instead of permanently losing the event consumer.
                if this
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_err()
                    && this.upgrade().is_none()
                {
                    break;
                }
            }
        })
        .detach();
        cx.defer_in(window, |this, window, cx| {
            this.refresh_proxy_editor(cx);
            this.capture_window(window);
            this.ensure_active(window);
            if this.smoke.is_none() {
                if std::env::args().any(|arg| arg == "--proxy-settings") {
                    this.open_proxy_pool(window, cx);
                } else if std::env::args().any(|arg| arg == "--account-settings") {
                    this.open_account_dialog(this.workspace.active_account_id(), false, window, cx);
                }
            }
            this.persist();
            cx.notify();
        });
        let saved_window = crate::window_state::SavedWindow::capture(window.window_bounds());
        let (caption, caption_error) = match crate::windows_caption::CaptionBridge::install(window)
        {
            Ok(caption) => (Some(caption), None),
            Err(error) => (None, Some(error)),
        };
        let sidebar = crate::sidebar_state::SidebarState::load(&data_dir);
        let motion_policy = crate::motion::MotionPolicy::system();
        let motion_started = Instant::now();
        let sidebar_extent = sidebar.preferred_extent();
        let sidebar_motion =
            crate::motion::MotionValue::new(sidebar_extent, sidebar_extent, motion_started, None);
        let inspector_motion = crate::motion::MotionValue::new(0., 0., motion_started, None);
        let avatars = crate::site_avatar::AvatarCache::load(&data_dir, &workspace);
        let (plan_labels, cached_accounts) =
            crate::account_cache::restore(&data_dir, &mut workspace);
        let invites = crate::account_cache::restore_invites(&data_dir, &workspace);
        let invite_next = workspace
            .accounts
            .iter()
            .map(|account| {
                let wait = invites
                    .get(&account.id)
                    .map(|info| {
                        let age = crate::page_observer::now_ms().saturating_sub(info.observed_at);
                        INVITE_EVERY.saturating_sub(Duration::from_millis(age))
                    })
                    .unwrap_or_default()
                    .max(Duration::from_secs(60));
                (account.id, Instant::now() + wait)
            })
            .collect::<HashMap<_, _>>();
        let mut studio = crate::studio::Studio::new(&data_dir, cx);
        for entry in studio.entries.iter().rev() {
            if studio
                .sessions
                .contains_key(&(entry.conversation, entry.account))
            {
                continue;
            }
            if let Some(tab) = entry.source_tab.and_then(|id| {
                workspace
                    .account(entry.account)?
                    .tabs
                    .iter()
                    .find(|t| t.id == id)
            }) {
                studio.sessions.insert(
                    (entry.conversation, entry.account),
                    muse_core::BrowserIdentity {
                        account_id: entry.account,
                        tab_id: tab.id,
                        generation: tab.generation,
                    },
                );
            }
        }
        let updates =
            update_ui::UpdateUi::new(&data_dir, diagnostic_home.is_none() && smoke.is_none());
        Self {
            workspace,
            browsers: BrowserPool::new(sender, data_dir.clone(), display),
            receiver,
            session_probe: crate::session_probe::SessionProbe::new(&data_dir),
            data_dir,
            address,
            account_name,
            proxy_editor: proxy_editor::ProxyEditor::new(cx),
            inspector_section: if std::env::args().any(|argument| argument == "--proxy-settings") {
                InspectorSection::Proxy
            } else {
                InspectorSection::Account
            },
            focus: cx.focus_handle(),
            home,
            diagnostic_origin,
            status: startup_error
                .clone()
                .or(caption_error)
                .unwrap_or_else(|| "账号数据保存在本机".into()),
            save_enabled: startup_error.is_none(),
            pending_save: None,
            display,
            remove_confirmation: None,
            markers: HashMap::new(),
            ready_pages: HashMap::new(),
            smoke,
            _bounds_observer: observer,
            show_start_page: diagnostic_home.is_none(),
            sidebar,
            sidebar_motion,
            sidebar_dragging: false,
            inspector_open: false,
            inspector_motion,
            motion_policy,
            navigation_transition: TransitionState::visible(),
            studio_panel: None,
            saved_window,
            pending_window_save: None,
            _caption: caption,
            avatars,
            plan_labels,
            invites,
            invite_jobs: HashMap::new(),
            page_snapshot: None,
            freeze_frame: None,
            freeze_checked: false,
            snapshot_request: None,
            snapshot_counter: 0,
            avatar_hover: None,
            sleeping_pages: 0,
            invite_next,
            account_dialog: None,
            account_dialog_return: None,
            cached_accounts,
            refresh_accounts: HashMap::new(),
            studio,
            cookie_dialog: None,
            transfer_dialog: None,
            cookie_logins: HashMap::new(),
            account_context: None,
            updates,
        }
    }

    fn capture_window(&mut self, window: &Window) {
        if crate::windows_host::is_minimized(window) {
            return;
        }
        let saved = crate::window_state::SavedWindow::capture(window.window_bounds());
        if saved.is_some() && saved != self.saved_window {
            self.saved_window = saved;
            self.pending_window_save = Some(Instant::now());
        }
    }

    fn persist_window(&mut self) {
        if let Err(error) = self.sidebar.save(&self.data_dir) {
            self.status = error;
        }
        if let Some(saved) = self.saved_window
            && let Err(error) = saved.save(&self.data_dir)
        {
            self.status = error;
        }
        self.pending_window_save = None;
    }

    /// The proxy pool is a side sheet: the page stays visible beside it.
    pub(super) fn proxy_drawer_visible(&self) -> bool {
        self.transfer_dialog.is_none()
            && self.cookie_dialog.is_none()
            && self.account_context.is_none()
            && self
                .account_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.is_drawer())
    }

    pub(super) fn inspector_target(&self) -> f32 {
        if self.inspector_open {
            crate::theme::INSPECTOR
        } else if self.proxy_drawer_visible()
            && self
                .account_dialog
                .as_ref()
                .is_some_and(|dialog| !dialog.transition_exiting())
        {
            crate::theme::DRAWER
        } else {
            0.
        }
    }

    /// Native WebViews sit above every GPUI element, so dialogs and menus must
    /// hide the live page. Before hiding it, a still frame is captured and shown
    /// in its place, so the page never turns blank behind an overlay.
    fn page_frozen(&self) -> bool {
        !self.show_start_page && self.has_browser_overlay() && !self.proxy_drawer_visible()
    }

    /// Captures the active page in the background while nothing covers it.
    /// Called while the pointer moves over the sidebar, where every menu and
    /// dialog is opened from; throttled to one capture every 1.5 seconds.
    pub(super) fn prefetch_page_snapshot(&mut self) {
        if self.smoke.is_some() || self.show_start_page || self.has_browser_overlay() {
            return;
        }
        let Some(identity) = self.workspace.active_browser_identity() else {
            return;
        };
        let tab = identity.tab_id;
        let fresh = self.page_snapshot.as_ref().is_some_and(|(shot, _, at)| {
            *shot == tab && at.elapsed() < Duration::from_millis(1500)
        });
        let pending = self
            .snapshot_request
            .is_some_and(|(_, _, at)| at.elapsed() < Duration::from_secs(3));
        if fresh || pending || !self.browsers.active_render_ready(identity) {
            return;
        }
        self.snapshot_counter += 1;
        if self
            .browsers
            .capture_preview(identity, self.snapshot_counter)
        {
            self.snapshot_request = Some((self.snapshot_counter, tab, Instant::now()));
        }
    }

    /// Picks the frame that stands in for the page as soon as an overlay opens.
    /// The live page is hidden at once, so nothing is ever drawn over a menu.
    pub(super) fn ensure_page_freeze(&mut self) {
        if !self.page_frozen() {
            self.freeze_frame = None;
            self.freeze_checked = false;
            return;
        }
        if self.freeze_checked {
            return;
        }
        self.freeze_checked = true;
        let tab = self
            .workspace
            .active_browser_identity()
            .map(|identity| identity.tab_id);
        self.freeze_frame = self
            .page_snapshot
            .as_ref()
            .filter(|(shot, _, at)| Some(*shot) == tab && at.elapsed() < Duration::from_secs(15))
            .map(|(shot, image, _)| (*shot, image.clone()));
    }

    pub(super) fn frozen_page(&self) -> Option<std::sync::Arc<RenderImage>> {
        let tab = self.workspace.active_browser_identity()?.tab_id;
        self.freeze_frame
            .as_ref()
            .filter(|(frozen, _)| self.page_frozen() && *frozen == tab)
            .map(|(_, image)| image.clone())
    }

    fn visible_browser_identity(&self) -> Option<muse_core::BrowserIdentity> {
        if self.show_start_page || self.page_frozen() {
            None
        } else {
            self.workspace.active_browser_identity()
        }
    }

    fn has_browser_overlay(&self) -> bool {
        self.updates.open
            || self.account_dialog.is_some()
            || self.transfer_dialog.is_some()
            || self.cookie_dialog.is_some()
            || self.account_context.is_some()
    }

    fn dismiss_browser_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.updates.open {
            self.close_updates(window, cx);
        } else if self.transfer_dialog.is_some() {
            self.close_transfer(window, cx);
        } else if self.account_dialog.is_some() {
            self.close_account_dialog(window, cx);
        } else if self.cookie_dialog.is_some() {
            self.close_cookie_dialog(window, cx);
        } else if self.account_context.is_some() {
            self.dismiss_account_context(window, cx);
        }
    }

    fn schedule_overlay_removal(
        &mut self,
        kind: MotionKind,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
        finish: fn(&mut Self, u64, &mut Window, &mut Context<Self>),
    ) {
        if let Some(duration) = self.motion_policy.duration(kind) {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(duration).await;
                loop {
                    if this
                        .update_in(cx, |this, window, cx| finish(this, generation, window, cx))
                        .is_ok()
                        || this.upgrade().is_none()
                    {
                        break;
                    }
                    // WebView2 创建期间可能暂时占用窗口，下一帧再尝试完成关闭。
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                }
            })
            .detach();
        } else {
            finish(self, generation, window, cx);
        }
        cx.notify();
    }

    fn browser_bounds(&self, window: &Window) -> Rect {
        Self::browser_rect(self.chrome_layout(window, Instant::now()))
    }

    fn chrome_layout(&self, window: &Window, now: Instant) -> crate::theme::ChromeLayout {
        let size = window.viewport_size();
        crate::theme::ChromeLayout::at_motion_frame(
            f32::from(size.width),
            f32::from(size.height),
            &self.sidebar_motion,
            &self.inspector_motion,
            now,
        )
    }

    fn browser_rect(geometry: crate::theme::ChromeLayout) -> Rect {
        Rect {
            position: LogicalPosition::new(geometry.left as f64, geometry.top as f64).into(),
            size: LogicalSize::new(geometry.width as f64, geometry.height as f64).into(),
        }
    }

    fn animate_chrome_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.retarget_panel_layout(self.motion_policy.duration(MotionKind::Layout));
        self.ensure_active(window);
        cx.notify();
    }

    fn retarget_panel_layout(&mut self, duration: Option<Duration>) {
        let now = Instant::now();
        self.sidebar_motion
            .retarget(self.sidebar.preferred_extent(), now, duration);
        let target = self.inspector_target();
        self.inspector_motion.retarget(target, now, duration);
    }

    fn persist(&mut self) {
        if self.transfer_commit_pending() {
            return;
        }
        if self.save_enabled
            && let Err(error) = self.workspace.save(&self.data_dir.join("workspace.json"))
        {
            self.status = error;
        }
        self.pending_save = None;
    }

    fn persist_display(&mut self) {
        if let Err(error) = self.display.save(&self.data_dir) {
            self.status = error;
        }
    }

    /// Read the actual native factor, including browser-handled Ctrl + wheel.
    fn capture_page_zoom(&mut self) -> bool {
        let Some(identity) = self.workspace.active_browser_identity() else {
            return false;
        };
        let Some(factor) = self.browsers.zoom(identity.tab_id) else {
            return false;
        };
        if !self.display.set_page_zoom(factor) {
            return false;
        }
        self.browsers.remember_zoom(self.display.page_zoom());
        self.persist_display();
        true
    }

    fn set_theme(
        &mut self,
        mode: muse_core::ThemeMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.display.theme == mode {
            return;
        }
        self.display.theme = mode;
        crate::theme::set_mode(mode);
        self.status = match mode {
            muse_core::ThemeMode::Light => "已切换到白天模式",
            muse_core::ThemeMode::Dark => "已切换到晚上模式",
        }
        .into();
        if let Err(error) = self.browsers.set_theme(mode) {
            self.status = format!("客户端主题已切换，网页主题设置失败：{error}");
        }
        // A snapshot from the previous palette must not cover the new page.
        self.page_snapshot = None;
        self.freeze_frame = None;
        self.freeze_checked = false;
        self.snapshot_request = None;
        self.persist_display();
        window.refresh();
        cx.notify();
    }

    fn ensure_active(&mut self, window: &Window) {
        self.browsers.retain_registered(&self.workspace);
        if self.show_start_page {
            self.browsers
                .sync_visibility(None, self.browser_bounds(window));
            return;
        }
        if let Some(identity) = self.workspace.active_browser_identity() {
            let account = self
                .workspace
                .account(identity.account_id)
                .expect("registered identity");
            let tab = account.active_tab().expect("active tab");
            let profile = account.profile_dir(&self.data_dir.join("profiles"));
            if let Err(error) = self.browsers.ensure(
                BrowserRequest {
                    identity,
                    url: &tab.url,
                    profile: &profile,
                    network: account.network,
                    proxy_credentials: self.workspace.proxy_credentials(account),
                    diagnostic_origin: self.diagnostic_origin.as_deref(),
                },
                window,
                self.browser_bounds(window),
            ) {
                self.status = error;
            }
        }
        self.browsers
            .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
    }

    fn retry_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.workspace.active_account() else {
            return;
        };
        let (id, network, revision) = (account.id, account.network, account.proxy_revision);
        if let Err(error) = self.workspace.update_network(id, network, revision) {
            self.status = error;
        } else {
            self.browsers.invalidate_account(id);
            self.status = "正在重新打开此账号的浏览器".into();
            self.ensure_active(window);
            self.persist();
        }
        cx.notify();
    }

    fn sync_fields(&mut self, cx: &mut Context<Self>) {
        if let Some(account) = self.workspace.active_account() {
            let name = account.name.clone();
            let url = account
                .active_tab()
                .map(|t| t.url.clone())
                .unwrap_or_default();
            self.address
                .update(cx, |input, cx| input.set_text(&url, cx));
            self.account_name
                .update(cx, |input, cx| input.set_text(&name, cx));
        }
        if self.proxy_editor.owner != self.workspace.active_account_id() {
            self.refresh_proxy_editor(cx);
        }
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let committing = self.transfer_commit_pending();
        self.poll_transfer(window, cx);
        if committing || self.transfer_commit_pending() {
            return;
        }
        let mut changed = self.poll_proxy_editor(cx);
        changed |= self.poll_updates(window, cx);
        changed |= self.poll_account_dialog(cx);
        let zoom_changed = self.browsers.zoom_changed(
            self.visible_browser_identity()
                .map(|identity| identity.tab_id),
        );
        if zoom_changed {
            self.capture_page_zoom();
            changed = true;
        }
        self.avatars.retain(&self.workspace);
        let now = crate::page_observer::now_ms();
        for account in &mut self.workspace.accounts {
            // Even sleeping/cached values age, but their last values stay visible.
            let quota = account.quota.clone().at_time(now);
            let extra = account.extra_quota.clone().at_time(now);
            changed |= quota != account.quota || extra != account.extra_quota;
            account.quota = quota;
            account.extra_quota = extra;
        }
        self.capture_window(window);
        if self
            .pending_window_save
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(400))
        {
            self.persist_window();
        }
        let mut selected_changed = false;
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                BrowserMessage::State(identity, event) => {
                    if self.browsers.is_cookie_reader(identity) {
                        continue;
                    }
                    let active = self.workspace.active_browser_identity() == Some(identity);
                    let persistent = matches!(event, BrowserEvent::Title(_) | BrowserEvent::Url(_));
                    if self.workspace.apply_browser_event(identity, event) {
                        changed = true;
                        if persistent {
                            self.pending_save = Some(Instant::now());
                        }
                        if active && !self.address.read(cx).is_focused(window) {
                            let url = self
                                .workspace
                                .active_account()
                                .and_then(|a| a.active_tab())
                                .map(|t| t.url.clone())
                                .unwrap_or_default();
                            if self.address.read(cx).text() != url {
                                self.address
                                    .update(cx, |input, cx| input.set_text(&url, cx));
                            }
                        }
                    }
                }
                BrowserMessage::OpenTab(identity, url) => {
                    let was_active = self.workspace.active_browser_identity() == Some(identity);
                    if self.workspace.open_popup(identity, &url).is_ok() {
                        self.status = "已在同一账号的新标签页打开；原页面仍然保留".into();
                        self.pending_save = Some(Instant::now());
                        changed = true;
                        selected_changed = was_active;
                    }
                }
                BrowserMessage::Preview(identity, request, image) => {
                    if let Some((pending, tab, at)) = self.snapshot_request
                        && pending == request
                        && tab == identity.tab_id
                    {
                        self.snapshot_request = None;
                        if let Some(image) = image {
                            // The frame was taken when the request was sent.
                            if let Some((_, old, _)) =
                                self.page_snapshot.replace((tab, image.clone(), at))
                                && !self
                                    .freeze_frame
                                    .as_ref()
                                    .is_some_and(|(_, frozen)| frozen.id == old.id)
                            {
                                cx.drop_image(old, Some(&mut *window));
                            }
                            // An overlay opened while this capture was on its way.
                            if self.page_frozen()
                                && self.freeze_frame.is_none()
                                && self.workspace.active_browser_identity().map(|i| i.tab_id)
                                    == Some(tab)
                            {
                                self.freeze_frame = Some((tab, image));
                                changed = true;
                            }
                        }
                    }
                }
                BrowserMessage::Command(identity, command) => {
                    if self.workspace.active_browser_identity() == Some(identity) {
                        self.command(&command, window, cx);
                    }
                }
                BrowserMessage::Diagnostic(identity, marker, source_url) => {
                    if self.workspace.contains_identity(identity) {
                        self.markers.insert(identity.account_id, marker);
                        self.ready_pages.insert(identity.tab_id, source_url);
                        changed = true;
                    }
                }
                BrowserMessage::Studio(identity, epoch, job, raw) => {
                    if self
                        .studio
                        .jobs
                        .get(&identity.account_id)
                        .is_none_or(|active| active.id != job)
                    {
                        continue;
                    }
                    if self.workspace.contains_identity(identity)
                        && self.browsers.accepts_studio(identity, epoch)
                    {
                        self.accept_studio_reply(identity, job, &raw, cx);
                        changed = true;
                    } else {
                        self.fail_studio_job(identity.account_id, "任务页面已变化，请重新发送", cx);
                    }
                }
                BrowserMessage::Observation(identity, epoch, snapshot) => {
                    if self.workspace.contains_identity(identity)
                        && self
                            .browsers
                            .accept_observation(identity, epoch, snapshot.as_ref())
                    {
                        let snapshot = snapshot.unwrap();
                        if snapshot.ready
                            && muse_core::trusted_muse_url(&snapshot.url)
                            && self
                                .workspace
                                .account(identity.account_id)
                                .is_some_and(|a| a.active_tab_id() == identity.tab_id)
                        {
                            let previous_quota = self
                                .workspace
                                .account(identity.account_id)
                                .and_then(|a| a.quota.observed_at());
                            let previous_extra = self
                                .workspace
                                .account(identity.account_id)
                                .and_then(|a| a.extra_quota.observed_at());
                            let previous_plan = self.plan_labels.get(&identity.account_id).cloned();
                            if snapshot.quota_status == "logged_out" {
                                self.avatars.remove(identity.account_id);
                                self.plan_labels.remove(&identity.account_id);
                                self.invites.remove(&identity.account_id);
                                self.cached_accounts.remove(&identity.account_id);
                                crate::account_cache::remove(&self.data_dir, identity.account_id);
                            } else {
                                if let Some(data) = snapshot.avatar_png.as_deref() {
                                    self.avatars.update(identity.account_id, data);
                                }
                                if let Some(clip) = snapshot.avatar_animation.as_ref() {
                                    self.avatars.update_animation(identity.account_id, clip);
                                }
                                if let Some(label) =
                                    snapshot.plan_label.as_deref().filter(|label| {
                                        !label.is_empty()
                                            && label.chars().count() <= 32
                                            && !label.chars().any(char::is_control)
                                    })
                                {
                                    self.plan_labels
                                        .insert(identity.account_id, label.to_owned());
                                }
                                if let Some(mut info) =
                                    snapshot.invite.as_ref().and_then(|invite| invite.info())
                                {
                                    let old = self.invites.get(&identity.account_id);
                                    let now = crate::page_observer::now_ms();
                                    if info.observed_at <= now.saturating_add(30_000)
                                        && now.saturating_sub(info.observed_at) < 300_000
                                        && old.is_none_or(|old| info.observed_at > old.observed_at)
                                    {
                                        if let Some(old) = old {
                                            info = info.merge(old);
                                        }
                                        let persist = old.is_none_or(|old| {
                                            !old.same_content(&info)
                                                || info.observed_at.saturating_sub(old.observed_at)
                                                    >= 60_000
                                        });
                                        if persist {
                                            crate::account_cache::store_invite(
                                                &self.data_dir,
                                                identity.account_id,
                                                &info,
                                            );
                                        }
                                        self.invite_jobs.remove(&identity.account_id);
                                        self.invite_next.insert(
                                            identity.account_id,
                                            Instant::now() + INVITE_EVERY,
                                        );
                                        self.invites.insert(identity.account_id, info);
                                    }
                                }
                            }
                            let state = snapshot
                                .quota
                                .as_ref()
                                .filter(|_| snapshot.quota_status == "ok")
                                .and_then(|q| q.as_state(crate::page_observer::now_ms()));
                            if state.is_some() {
                                self.cached_accounts.remove(&identity.account_id);
                                self.refresh_accounts.remove(&identity.account_id);
                            }
                            match state {
                                Some(muse_core::QuotaState::Known(observation)) => {
                                    if self
                                        .workspace
                                        .account(identity.account_id)
                                        .and_then(|a| a.quota.observed_at())
                                        != Some(observation.observed_at())
                                    {
                                        self.workspace.apply_quota(
                                            identity,
                                            &snapshot.url,
                                            observation,
                                        );
                                    }
                                }
                                Some(state) => {
                                    self.workspace.apply_quota_status(
                                        identity,
                                        &snapshot.url,
                                        state,
                                    );
                                }
                                None => {
                                    let has_quota = self
                                        .cached_accounts
                                        .contains(&identity.account_id)
                                        || self.workspace.account(identity.account_id).is_some_and(
                                            |a| {
                                                matches!(
                                                    a.quota,
                                                    muse_core::QuotaState::Known(_)
                                                        | muse_core::QuotaState::Unlimited { .. }
                                                )
                                            },
                                        );
                                    let status = match snapshot.quota_status.as_str() {
                                        "logged_out" => Some(muse_core::QuotaState::LoggedOut),
                                        "syncing" | "waiting" if !has_quota => {
                                            Some(muse_core::QuotaState::Syncing)
                                        }
                                        "request_failed" | "rate_limited" | "auth_required" => {
                                            Some(muse_core::QuotaState::Unavailable(
                                                "额度同步失败，可重试".into(),
                                            ))
                                        }
                                        "not_found" | "unrecognized" => {
                                            Some(muse_core::QuotaState::Unavailable(
                                                "暂未读取到套餐额度".into(),
                                            ))
                                        }
                                        _ => None,
                                    };
                                    if let Some(status) = status {
                                        self.workspace.apply_quota_status(
                                            identity,
                                            &snapshot.url,
                                            status,
                                        );
                                    }
                                    if matches!(
                                        snapshot.quota_status.as_str(),
                                        "request_failed"
                                            | "auth_required"
                                            | "rate_limited"
                                            | "not_found"
                                            | "unrecognized"
                                            | "logged_out"
                                    ) {
                                        self.refresh_accounts.remove(&identity.account_id);
                                    }
                                }
                            }
                            if let Some(extra) = snapshot
                                .quota
                                .as_ref()
                                .and_then(|q| q.extra(crate::page_observer::now_ms()))
                            {
                                self.workspace
                                    .apply_extra_quota(identity, &snapshot.url, extra);
                            }
                            if snapshot.quota_status != "logged_out"
                                && (previous_quota
                                    != self
                                        .workspace
                                        .account(identity.account_id)
                                        .and_then(|a| a.quota.observed_at())
                                    || previous_extra
                                        != self
                                            .workspace
                                            .account(identity.account_id)
                                            .and_then(|a| a.extra_quota.observed_at())
                                    || previous_plan
                                        != self.plan_labels.get(&identity.account_id).cloned())
                            {
                                crate::account_cache::store(
                                    &self.data_dir,
                                    &self.workspace,
                                    identity.account_id,
                                    self.plan_labels.get(&identity.account_id),
                                );
                            }
                            // The page owns bounded retry/backoff. Forcing here on
                            // every snapshot used to bypass its rate limit.
                        }
                        changed = true;
                    }
                }
                BrowserMessage::Cookies(account, identity, request, result) => {
                    self.accept_cookie_result(account, identity, request, result, window, cx);
                    changed = true;
                }
            }
        }
        if selected_changed {
            self.navigation_transition = crate::motion::TransitionState::entering();
            self.sync_fields(cx);
            self.persist();
        }
        if let Some(identity) = self.workspace.active_browser_identity()
            && let Some(view) = self.browsers.view(identity.tab_id)
        {
            let back = view.can_go_back().unwrap_or(false);
            let forward = view.can_go_forward().unwrap_or(false);
            let differs = self
                .workspace
                .active_account()
                .and_then(|a| a.active_tab())
                .is_some_and(|t| t.can_go_back != back || t.can_go_forward != forward);
            if differs {
                self.workspace
                    .apply_browser_event(identity, BrowserEvent::History { back, forward });
                changed = true;
            }
            if let Some(url) = crate::browser::current_url(view) {
                let differs = self
                    .workspace
                    .active_account()
                    .and_then(|a| a.active_tab())
                    .is_some_and(|t| t.url != url);
                if differs
                    && self
                        .workspace
                        .apply_browser_event(identity, BrowserEvent::Url(url.clone()))
                {
                    if !self.address.read(cx).is_focused(window) {
                        self.address
                            .update(cx, |input, cx| input.set_text(&url, cx));
                    }
                    self.pending_save = Some(Instant::now());
                    changed = true;
                }
            }
        }
        if self
            .pending_save
            .is_some_and(|at| at.elapsed() > Duration::from_millis(500))
        {
            self.persist();
        }
        changed |= self.poll_cookie_logins();
        changed |= self.tick_cookies(window, cx);
        self.tick_studio(window, cx);
        let refresh_count = self.refresh_accounts.len();
        self.refresh_accounts.retain(|id, (identity, at)| {
            self.workspace.contains_identity(*identity)
                && at.elapsed() < Duration::from_secs(45)
                && self.workspace.account(*id).is_some()
        });
        changed |= refresh_count != self.refresh_accounts.len();
        self.schedule_invite_reads();
        self.ensure_page_freeze();
        // Animated avatars advance only while one is hovered, at 10 fps.
        changed |= self.avatar_hover.is_some();
        let backgrounds = self
            .refresh_accounts
            .values()
            .map(|(identity, _)| *identity)
            .chain(self.invite_jobs.values().map(|(identity, _)| *identity))
            .chain(self.studio.jobs.values().filter_map(|job| job.identity))
            .chain(self.cookie_logins.values().map(|(identity, _)| *identity))
            .filter(|identity| !self.browsers.is_cookie_reader(*identity))
            .collect::<Vec<_>>();
        self.browsers.set_background(backgrounds.iter().copied());
        self.ensure_active(window);
        for identity in backgrounds {
            self.browsers.poll_page(identity);
        }
        if let Some(identity) = self.workspace.active_browser_identity() {
            self.browsers.poll_page(identity);
        }
        if self.smoke.is_none() {
            changed |= self.browsers.sleep_idle(SLEEP_AFTER_MS);
        }
        let sleeping = self.browsers.sleeping_count();
        if sleeping != self.sleeping_pages {
            self.sleeping_pages = sleeping;
            changed = true;
        }
        if let Some(probe) = &mut self.session_probe {
            probe.tick(
                &self.browsers,
                &self.workspace,
                self.workspace
                    .active_account_id()
                    .and_then(|id| self.avatars.image(id))
                    .map_or(0, |image| image.frame_count()),
            );
        }
        if self.smoke.is_some() {
            self.advance_smoke(window, cx);
        }
        if changed {
            cx.notify();
        }
    }

    fn refresh_current_quota(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.workspace.active_account_id() {
            self.refresh_account(id, window, cx);
        }
    }
    fn ensure_identity(
        &mut self,
        identity: muse_core::BrowserIdentity,
        window: &Window,
    ) -> Result<(), String> {
        let account = self
            .workspace
            .account(identity.account_id)
            .ok_or("账号不存在")?;
        let tab = account
            .tabs
            .iter()
            .find(|t| t.id == identity.tab_id && t.generation == identity.generation)
            .ok_or("账号页面已变化")?;
        let profile = account.profile_dir(&self.data_dir.join("profiles"));
        self.browsers.ensure(
            BrowserRequest {
                identity,
                url: &tab.url,
                profile: &profile,
                network: account.network,
                proxy_credentials: self.workspace.proxy_credentials(account),
                diagnostic_origin: self.diagnostic_origin.as_deref(),
            },
            window,
            self.browser_bounds(window),
        )
    }
    pub(super) fn prepare_cookie_identity(
        &mut self,
        identity: muse_core::BrowserIdentity,
        window: &Window,
    ) -> Result<(), String> {
        let Some(account) = self.workspace.account(identity.account_id) else {
            return Err("账号不存在".into());
        };
        let profile = account.profile_dir(&self.data_dir.join("profiles"));
        self.browsers.ensure_reader(
            BrowserRequest {
                identity,
                url: "about:blank",
                profile: &profile,
                network: account.network,
                proxy_credentials: self.workspace.proxy_credentials(account),
                diagnostic_origin: None,
            },
            window,
            self.browser_bounds(window),
        )
    }
    /// Passive reads only. Never open an invitation UI on the user's behalf.
    fn schedule_invite_reads(&mut self) {
        let now = Instant::now();
        self.invite_jobs
            .retain(|_, (_, started)| now.duration_since(*started) < INVITE_TIMEOUT);
        let due = self
            .workspace
            .accounts
            .iter()
            .filter(|account| {
                !self.invite_jobs.contains_key(&account.id)
                    && !self.studio.jobs.contains_key(&account.id)
                    && !self.cookie_dialog.is_some()
                    && self.browsers.account_running(account.id)
                    && !self.browsers.account_sleeping(account.id)
                    && !matches!(account.quota, muse_core::QuotaState::LoggedOut)
                    && self
                        .invite_next
                        .get(&account.id)
                        .is_none_or(|at| now >= *at)
            })
            .filter_map(|account| {
                let tab = account.active_tab()?;
                Some(muse_core::BrowserIdentity {
                    account_id: account.id,
                    tab_id: tab.id,
                    generation: tab.generation,
                })
            })
            .take(2_usize.saturating_sub(self.invite_jobs.len()))
            .collect::<Vec<_>>();
        for identity in due {
            self.invite_next
                .insert(identity.account_id, now + INVITE_RETRY);
            if self.browsers.page_ready(identity) && self.browsers.refresh_invite(identity) {
                self.invite_jobs
                    .insert(identity.account_id, (identity, now));
            }
        }
    }

    fn refresh_account(&mut self, id: AccountId, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .refresh_accounts
            .get(&id)
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(45))
        {
            return;
        }
        let Some(account) = self.workspace.account(id) else {
            return;
        };
        let Some(tab) = account.active_tab() else {
            return;
        };
        let identity = muse_core::BrowserIdentity {
            account_id: id,
            tab_id: tab.id,
            generation: tab.generation,
        };
        match self.ensure_identity(identity, window) {
            Ok(()) => {
                self.refresh_accounts.insert(id, (identity, Instant::now()));
                self.browsers.refresh_quota(identity);
                if self.browsers.refresh_invite(identity) {
                    self.invite_jobs.insert(id, (identity, Instant::now()));
                }
                self.status = "正在刷新这个账号的信息".into();
            }
            Err(error) => self.status = error,
        }
        cx.notify();
    }

    fn command(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_browser_overlay() || (self.show_start_page && command.starts_with("zoom")) {
            return;
        }
        if self.show_start_page {
            self.navigation_transition = TransitionState::entering();
            self.show_start_page = false;
            self.sync_fields(cx);
            self.ensure_active(window);
        }
        if command == "address" {
            if let Some(identity) = self.workspace.active_browser_identity()
                && let Some(view) = self.browsers.view(identity.tab_id)
            {
                let _ = view.focus_parent();
            }
            self.address
                .update(cx, |input, cx| input.focus_and_select(window, cx));
            return;
        }
        let Some(identity) = self.workspace.active_browser_identity() else {
            return;
        };
        if command == "reload" && self.browsers.view(identity.tab_id).is_none() {
            if self.workspace.restart_tab(identity.tab_id).is_ok() {
                self.ensure_active(window);
            }
            cx.notify();
            return;
        }
        let Some(view) = self.browsers.view(identity.tab_id) else {
            return;
        };
        if let Some(direction) = match command {
            "zoom_in" => Some(1),
            "zoom_out" => Some(-1),
            "zoom_reset" => Some(0),
            _ => None,
        } {
            let current = self.browsers.zoom(identity.tab_id).unwrap_or(1.);
            let target = if direction == 0 {
                1.
            } else {
                crate::browser::zoom_step(current, direction)
            };
            match self.browsers.set_zoom(identity.tab_id, target) {
                Ok(()) => {
                    self.status = format!("页面缩放 {:.0}%", target * 100.);
                    self.capture_page_zoom();
                }
                Err(error) => self.status = format!("无法缩放页面：{error}"),
            }
            cx.notify();
            return;
        }
        let result = match command {
            "back" => view.go_back(),
            "forward" => view.go_forward(),
            "reload" => view.reload(),
            "home" => view.load_url(&self.home),
            _ => return,
        };
        if let Err(error) = result {
            self.status = format!("操作未完成：{error}");
        }
        cx.notify();
    }

    fn zoom_in(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
        if self.studio.viewer.is_some() {
            self.step_studio_zoom(1, cx);
        } else {
            self.command("zoom_in", window, cx);
        }
    }
    fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
        if self.studio.viewer.is_some() {
            self.step_studio_zoom(-1, cx);
        } else {
            self.command("zoom_out", window, cx);
        }
    }
    fn zoom_reset(&mut self, _: &ZoomReset, window: &mut Window, cx: &mut Context<Self>) {
        if self.studio.viewer.is_some() {
            self.set_studio_zoom(Some(1.), cx);
        } else {
            self.command("zoom_reset", window, cx);
        }
    }
    fn copy_viewer_image(
        &mut self,
        _: &CopyStudioImage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.studio.viewer.as_ref().map(|(viewer, _)| viewer.entry) {
            self.copy_studio_image(id, window, cx);
        }
    }

    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        self.command("back", window, cx);
    }
    fn forward(&mut self, _: &Forward, window: &mut Window, cx: &mut Context<Self>) {
        self.command("forward", window, cx);
    }
    fn reload(&mut self, _: &Reload, window: &mut Window, cx: &mut Context<Self>) {
        self.command("reload", window, cx);
    }
    fn go_home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.command("home", window, cx);
    }
    fn focus_address(&mut self, _: &FocusAddress, window: &mut Window, cx: &mut Context<Self>) {
        self.command("address", window, cx);
    }

    fn submit_address(&mut self, _: &SubmitAddress, window: &mut Window, cx: &mut Context<Self>) {
        if self.transfer_dialog.is_some() {
            self.submit_transfer(window, cx);
            return;
        }
        if self.show_start_page && self.studio.input.read(cx).is_focused(window) {
            self.submit_studio(window, cx);
            return;
        }
        // The account name editor also implements TextInput; Enter only navigates
        // while the address editor owns keyboard focus.
        if !self.address.read(cx).is_focused(window) {
            return;
        }
        let text = self.address.read(cx).text();
        let input = if text.contains("://") {
            text
        } else {
            format!("https://{text}")
        };
        match muse_core::normalize_web_url(&input) {
            Ok(url) => {
                if let Some(identity) = self.workspace.active_browser_identity()
                    && let Some(view) = self.browsers.view(identity.tab_id)
                {
                    if let Err(error) = view.load_url(&url) {
                        self.status = error.to_string();
                    }
                    let _ = view.focus();
                }
            }
            Err(error) => self.status = error,
        }
        cx.notify();
    }

    fn add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self
            .workspace
            .add_account(&format!("账号 {}", self.workspace.accounts.len() + 1));
        if self.home != HOME_URL
            && let Some(identity) = self.workspace.active_browser_identity()
        {
            self.workspace
                .apply_browser_event(identity, BrowserEvent::Url(self.home.clone()));
        }
        self.select_account(id, window, cx);
    }

    fn select_account(&mut self, id: AccountId, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_browser_overlay() {
            return;
        }
        self.capture_page_zoom();
        if self.show_start_page || self.workspace.active_account_id() != Some(id) {
            self.navigation_transition = crate::motion::TransitionState::entering();
        }
        self.show_start_page = false;
        let _ = self.workspace.select_account(id);
        self.remove_confirmation = None;
        self.sync_fields(cx);
        self.ensure_active(window);
        self.persist();
        cx.notify();
    }

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_browser_overlay() {
            return;
        }
        self.capture_page_zoom();
        self.navigation_transition = crate::motion::TransitionState::entering();
        self.show_start_page = false;
        if let Some(account) = self.workspace.active_account_id() {
            if let Err(error) = self.workspace.add_tab(account, &self.home) {
                self.status = error;
            }
            self.sync_fields(cx);
            self.ensure_active(window);
            self.persist();
            cx.notify();
        }
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_browser_overlay() {
            return;
        }
        if let Some(identity) = self.workspace.active_browser_identity() {
            self.close_tab_id(identity.tab_id, window, cx);
        }
    }

    fn close_tab_id(&mut self, id: muse_core::TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.capture_page_zoom();
        if self
            .workspace
            .active_browser_identity()
            .is_some_and(|identity| identity.tab_id == id)
        {
            self.navigation_transition = crate::motion::TransitionState::entering();
        }
        let _ = self.workspace.close_tab(id);
        if self.home != HOME_URL
            && let Some(identity) = self.workspace.active_browser_identity()
            && self
                .workspace
                .active_account()
                .and_then(|a| a.active_tab())
                .is_some_and(|t| t.url == HOME_URL)
        {
            self.workspace
                .apply_browser_event(identity, BrowserEvent::Url(self.home.clone()));
        }
        self.sync_fields(cx);
        self.ensure_active(window);
        self.persist();
        cx.notify();
    }

    fn remove_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.workspace.active_account_id() else {
            return;
        };
        if self.remove_confirmation != Some(id) {
            self.remove_confirmation = Some(id);
            self.status = "再次点击「确认移除」将移除此账号；浏览器登录目录会保留".into();
            cx.notify();
            return;
        }
        let _ = self.workspace.remove_account(id);
        if self.workspace.accounts.is_empty() {
            self.workspace.add_account("主账号");
        }
        self.remove_confirmation = None;
        self.status = "已从列表移除；原账号的浏览器数据仍保留在本机".into();
        self.sync_fields(cx);
        self.ensure_active(window);
        self.persist();
        cx.notify();
    }

    fn advance_smoke(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut smoke) = self.smoke.take() else {
            return;
        };
        #[cfg(debug_assertions)]
        if std::env::args().any(|arg| arg == "--encrypted-transfer-smoke-test") {
            self.advance_transfer_smoke(&mut smoke, window, cx);
            self.smoke = Some(smoke);
            return;
        }
        if std::env::args().any(|a| a == "--profile-binding-smoke-test") {
            let identity = self.workspace.active_browser_identity().unwrap();
            let rejected = self
                .browsers
                .startup_error(identity)
                .is_some_and(|error| error.contains("数据目录与此账号不一致"));
            if rejected || smoke.started.elapsed() > Duration::from_secs(10) {
                let no_navigation = crate::proxy_smoke::authenticated_requests() == Some(0);
                let report = serde_json::json!({"passed":rejected && no_navigation,"actual_profile_override_rejected":rejected,"no_upstream_navigation":no_navigation});
                let _ = std::fs::write(self.data_dir.join("smoke-result.json"), report.to_string());
                cx.quit();
                return;
            }
            self.smoke = Some(smoke);
            return;
        }
        if smoke.started.elapsed() > Duration::from_secs(50) {
            let tab = self.workspace.active_account().and_then(|a| a.active_tab());
            let identity = self.workspace.active_browser_identity().unwrap();
            let report = serde_json::json!({"passed":false,"stage":smoke.stage,"error":"smoke timeout","status":self.status,"proxy":crate::proxy_smoke::diagnostics(),"page_error":tab.and_then(|t|t.error.clone()),"page_loading":tab.map(|t|t.loading),"readiness":self.browsers.readiness_diagnostics(identity),"observer":self.browsers.observer_flags(identity)});
            let _ = std::fs::write(self.data_dir.join("smoke-result.json"), report.to_string());
            cx.quit();
            return;
        }
        let active = self
            .workspace
            .active_account()
            .and_then(|a| a.active_tab())
            .cloned();
        let ready = active
            .as_ref()
            .is_some_and(|tab| !tab.loading && self.ready_pages.get(&tab.id) == Some(&tab.url))
            && self
                .workspace
                .active_browser_identity()
                .is_some_and(|id| self.browsers.page_ready(id));
        match smoke.stage {
            0 if self.markers.contains_key(&smoke.first) && ready => {
                smoke.first_marker = self.markers.get(&smoke.first).cloned();
                if crate::proxy_smoke::authenticated_requests().is_some() {
                    let identity = self.workspace.active_browser_identity().unwrap();
                    if !self.browsers.exercise_readiness_timeout(identity) {
                        let report = serde_json::json!({"passed":false,"error":"readiness timeout fixture did not fail closed"});
                        let _ = std::fs::write(
                            self.data_dir.join("smoke-result.json"),
                            report.to_string(),
                        );
                        cx.quit();
                        return;
                    }
                    self.command("reload", window, cx);
                    smoke.stage = 20;
                } else {
                    if let Some(view) = self.browsers.view(smoke.original_tab) {
                        let _ = view.load_url(&format!("{}next", self.home));
                    }
                    smoke.stage = 1;
                }
            }
            20 if ready => {
                if let Some(view) = self.browsers.view(smoke.original_tab) {
                    let _ = view.load_url(&format!("{}next", self.home));
                }
                smoke.stage = 1;
            }
            1 if ready
                && active
                    .as_ref()
                    .is_some_and(|t| t.url.ends_with("/next") && t.can_go_back) =>
            {
                self.command("back", window, cx);
                smoke.stage = 2;
            }
            2 if ready
                && active
                    .as_ref()
                    .is_some_and(|t| t.url == self.home && t.can_go_forward) =>
            {
                self.command("forward", window, cx);
                smoke.stage = 3;
            }
            3 if ready && active.as_ref().is_some_and(|t| t.url.ends_with("/next")) => {
                self.add_account(window, cx);
                smoke.second = self.workspace.active_account_id();
                smoke.stage = 4;
            }
            4 => {
                if let Some(second) = smoke.second.and_then(|id| self.markers.get(&id)) {
                    if Some(second) == smoke.first_marker.as_ref() {
                        let _ = std::fs::write(
                            self.data_dir.join("smoke-result.json"),
                            "{\"passed\":false,\"error\":\"profiles share storage\"}",
                        );
                        cx.quit();
                        return;
                    }
                    self.select_account(smoke.first, window, cx);
                    smoke.stage = 5;
                }
            }
            5 if ready
                && active
                    .as_ref()
                    .is_some_and(|t| t.id == smoke.original_tab && t.url.ends_with("/next")) =>
            {
                if let Some(view) = self.browsers.view(smoke.original_tab) {
                    let _ = view.evaluate_script("window.open('/popup','_blank')");
                }
                smoke.stage = 6;
            }
            6 if ready && active.as_ref().is_some_and(|t| t.url.ends_with("/popup")) => {
                // A loaded DOM can still be hidden beneath the GPUI compositor.
                // Paint a fixture-only marker, then inspect actual window pixels.
                if smoke.paint_started.is_none() {
                    if let Some(view) = self.browsers.view(active.as_ref().unwrap().id) {
                        let _ = view.evaluate_script(
                            "(() => { const marker=document.createElement('div'); marker.style.cssText='position:fixed;inset:0;z-index:2147483647;background:#23b88b;pointer-events:none'; document.body.appendChild(marker); })()",
                        );
                    }
                    smoke.paint_started = Some(Instant::now());
                    self.smoke = Some(smoke);
                    return;
                }
                let rect = self.browser_bounds(window);
                let position = rect.position.to_logical::<f32>(1.0);
                let size = rect.size.to_logical::<f32>(1.0);
                let pixels_visible = crate::windows_host::pixel_matches(
                    window,
                    position.x + size.width / 2.,
                    position.y + size.height / 2.,
                    0x23B88B,
                )
                .ok();
                if cfg!(target_os = "windows")
                    && pixels_visible != Some(true)
                    && smoke.paint_started.unwrap().elapsed() < Duration::from_secs(3)
                {
                    self.smoke = Some(smoke);
                    return;
                }
                let preserved = self.workspace.account(smoke.first).is_some_and(|a| {
                    a.tabs
                        .iter()
                        .any(|t| t.id == smoke.original_tab && t.url.ends_with("/next"))
                });
                let unknown = self
                    .workspace
                    .accounts
                    .iter()
                    .all(|a| a.quota.remaining_ratio().is_none());
                let shared = self.markers.get(&smoke.first) == smoke.first_marker.as_ref();
                let mut follows_panels = true;
                // 在诊断窗口模拟关闭动画，不改 Windows 的全局设置。
                let system_motion = self.motion_policy;
                self.motion_policy = crate::motion::MotionPolicy::from_setting(Some(false));
                for (sidebar, inspector) in
                    [(true, false), (false, false), (true, true), (false, true)]
                {
                    self.sidebar.collapsed = !sidebar;
                    self.inspector_open = inspector;
                    self.animate_chrome_layout(window, cx);
                    let now = Instant::now();
                    follows_panels &= !self.sidebar_motion.is_animating(now)
                        && !self.inspector_motion.is_animating(now)
                        && self.sidebar_motion.value_at(now) == self.sidebar.preferred_extent()
                        && self.inspector_motion.value_at(now)
                            == if inspector {
                                crate::theme::INSPECTOR
                            } else {
                                0.
                            };
                    let expected = self.browser_bounds(window);
                    let expected_position = expected.position.to_logical::<f64>(1.0);
                    let expected_size = expected.size.to_logical::<f64>(1.0);
                    let actual = self
                        .workspace
                        .active_browser_identity()
                        .and_then(|identity| self.browsers.view(identity.tab_id))
                        .and_then(|view| view.bounds().ok());
                    follows_panels &= actual.is_some_and(|actual| {
                        let position = actual
                            .position
                            .to_logical::<f64>(window.scale_factor() as f64);
                        let size = actual.size.to_logical::<f64>(window.scale_factor() as f64);
                        (position.x - expected_position.x).abs() < 1.0
                            && (position.y - expected_position.y).abs() < 1.0
                            && (size.width - expected_size.width).abs() < 1.0
                            && (size.height - expected_size.height).abs() < 1.0
                    });
                }
                self.motion_policy = system_motion;
                self.sidebar.collapsed = false;
                self.inspector_open = false;
                self.retarget_panel_layout(None);
                self.show_start_page = true;
                self.ensure_active(window);
                let hides_on_workspace = self
                    .workspace
                    .active_browser_identity()
                    .is_some_and(|identity| self.browsers.visible(identity.tab_id) == Some(false));
                self.show_start_page = false;
                self.ensure_active(window);
                let proxy_requests = crate::proxy_smoke::authenticated_requests();
                let proxy_ok = proxy_requests.is_none_or(|count| count > 0);
                let profile_verified = self
                    .workspace
                    .accounts
                    .iter()
                    .all(|account| self.browsers.profile_verified(account.id));
                let readiness_verified = self.browsers.visible_views_ready()
                    && self
                        .workspace
                        .active_browser_identity()
                        .is_some_and(|id| self.browsers.active_render_ready(id));
                let render_check_passed = cfg!(target_os = "macos") || pixels_visible == Some(true);
                let mut report = serde_json::json!({"passed":preserved && unknown && shared && follows_panels && hides_on_workspace && proxy_ok && render_check_passed && profile_verified && readiness_verified,"actual_profile_bindings_verified":profile_verified,"visible_views_passed_readiness":readiness_verified,"webview_pixels_visible":pixels_visible,"navigation_back":true,"navigation_forward":true,"independent_account_storage":true,"same_account_tabs_share_storage":shared,"opener_page_preserved":preserved,"quota_unknown_without_provider":unknown,"webview_follows_sidebar_and_inspector":follows_panels,"webview_hides_on_native_workspace":hides_on_workspace,"native_authenticated_upstream_requests":proxy_requests,"accounts":self.workspace.accounts.len()});
                if proxy_requests.is_some() {
                    report["readiness_timeout_reload_recovered"] = serde_json::json!(true);
                }
                if std::env::args().any(|arg| arg == "--cookie-import-smoke-test") {
                    report["cookie_import_proxy_requests_before"] =
                        serde_json::json!(crate::proxy_smoke::authenticated_requests());
                    match account_dialog::cookie_smoke::begin(self, window, cx) {
                        Ok(original) => {
                            report["cookie_import_isolated_and_private"] = serde_json::json!(true);
                            report["cookie_import_proxy_route_preserved"] = serde_json::json!(true);
                            smoke.edit_before = Some(original);
                            smoke.edit_report = Some(report);
                            smoke.stage = 12;
                            self.smoke = Some(smoke);
                            return;
                        }
                        Err(error) => {
                            report["passed"] = serde_json::json!(false);
                            report["cookie_import_error"] = serde_json::json!(error);
                        }
                    }
                }
                if std::env::args().any(|arg| arg == "--account-dialog-smoke-test") {
                    let passed = self.exercise_account_dialogs(window, cx);
                    report["account_dialog_cancel_save_create_remove"] = serde_json::json!(passed);
                    report["passed"] = serde_json::json!(report["passed"] == true && passed);
                    if !std::env::args().any(|arg| arg == "--proxy-edit-smoke-test") {
                        smoke.edit_report = Some(report);
                        smoke.stage = 11;
                        self.smoke = Some(smoke);
                        return;
                    }
                }
                if std::env::args().any(|argument| argument == "--proxy-edit-smoke-test") {
                    match crate::proxy_smoke::rotate_credentials()
                        .and_then(|url| crate::proxy_settings::ProxyDraft::from_url(&url, true))
                    {
                        Ok(draft) => {
                            smoke.edit_report = Some(report);
                            smoke.edit_before = self.workspace.active_browser_identity();
                            self.proxy_editor.fill(draft, cx);
                            self.proxy_editor.owner = Some(smoke.first);
                            self.inspector_open = true;
                            self.retarget_panel_layout(None);
                            self.inspector_section = InspectorSection::Proxy;
                            self.test_proxy(cx);
                            smoke.stage = 7;
                        }
                        Err(error) => {
                            let _ = std::fs::write(
                                self.data_dir.join("smoke-result.json"),
                                serde_json::json!({"passed":false,"error":error}).to_string(),
                            );
                            cx.quit();
                            return;
                        }
                    }
                    self.smoke = Some(smoke);
                    return;
                }
                self.persist();
                let _ = std::fs::write(
                    self.data_dir.join("smoke-result.json"),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
                cx.quit();
                return;
            }
            7 if self.proxy_editor.pending.is_none() => {
                if !self.proxy_editor.good {
                    let _ = std::fs::write(
                        self.data_dir.join("smoke-result.json"),
                        serde_json::json!({"passed":false,"error":self.proxy_editor.message})
                            .to_string(),
                    );
                    cx.quit();
                    return;
                }
                smoke.edit_count = crate::proxy_smoke::authenticated_requests().unwrap_or(0);
                self.save_proxy(window, cx);
                smoke.stage = 8;
            }
            8 if ready && !self.proxy_editor.applying => {
                let storage = self.markers.get(&smoke.first) == smoke.first_marker.as_ref();
                let fresh = smoke
                    .edit_before
                    .is_some_and(|identity| !self.workspace.contains_identity(identity));
                let traffic =
                    crate::proxy_smoke::authenticated_requests().unwrap_or(0) > smoke.edit_count;
                let mut report = smoke.edit_report.take().unwrap_or_default();
                let base = report["passed"] == true;
                report["same_endpoint_credentials_applied"] = serde_json::json!(fresh && traffic);
                report["storage_preserved_after_proxy_change"] = serde_json::json!(storage);
                report["draft_connection_test_passed"] = serde_json::json!(true);
                report["passed"] = serde_json::json!(base && fresh && traffic && storage);
                if report["passed"] == true {
                    smoke.edit_report = Some(report);
                    self.proxy_editor.enabled = false;
                    self.save_proxy(window, cx);
                    smoke.stage = 9;
                    self.smoke = Some(smoke);
                    return;
                }
                self.persist();
                let _ = std::fs::write(
                    self.data_dir.join("smoke-result.json"),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
                cx.quit();
                return;
            }
            9 if ready && !self.proxy_editor.applying => {
                let direct =
                    self.workspace.account(smoke.first).is_some_and(|account| {
                        account.network == muse_core::AccountNetwork::Direct
                    }) && self.browsers.request_count(smoke.first).is_none();
                if let Some(report) = &mut smoke.edit_report {
                    report["system_network_switch_applied"] = serde_json::json!(direct);
                }
                self.proxy_editor.enabled = true;
                self.save_proxy(window, cx);
                smoke.stage = 10;
            }
            10 if ready && !self.proxy_editor.applying => {
                let enabled = self.workspace.account(smoke.first).is_some_and(|account| {
                    matches!(account.network, muse_core::AccountNetwork::HttpProxy { .. })
                }) && self
                    .browsers
                    .request_count(smoke.first)
                    .is_some_and(|count| count > 0);
                let other_preserved = smoke
                    .second
                    .and_then(|id| self.workspace.account(id))
                    .and_then(|account| self.browsers.view(account.active_tab_id()))
                    .is_some();
                let mut report = smoke.edit_report.take().unwrap_or_default();
                report["http_proxy_reenabled"] = serde_json::json!(enabled);
                report["other_account_view_preserved"] = serde_json::json!(other_preserved);
                report["native_authenticated_upstream_requests"] =
                    serde_json::json!(crate::proxy_smoke::authenticated_requests());
                report["passed"] = serde_json::json!(
                    report["passed"] == true
                        && report["system_network_switch_applied"] == true
                        && enabled
                        && other_preserved
                );
                self.persist();
                let _ = std::fs::write(
                    self.data_dir.join("smoke-result.json"),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
                cx.quit();
                return;
            }
            12 if self.account_dialog.is_none() => {
                let ready = self
                    .workspace
                    .active_browser_identity()
                    .is_some_and(|identity| {
                        self.browsers.page_ready(identity)
                            && self.ready_pages.contains_key(&identity.tab_id)
                            && self
                                .workspace
                                .active_account()
                                .and_then(|account| account.active_tab())
                                .is_some_and(|tab| tab.title != "Muse" && !tab.loading)
                    });
                if ready {
                    let sent = self
                        .workspace
                        .active_account()
                        .and_then(|account| account.active_tab())
                        .is_some_and(|tab| tab.title.starts_with("Cookie导入验证通过"));
                    let mut report = smoke.edit_report.take().unwrap_or_default();
                    let routed = report["cookie_import_proxy_requests_before"]
                        .as_u64()
                        .is_none_or(|before| {
                            crate::proxy_smoke::authenticated_requests()
                                .is_some_and(|after| after > before as usize)
                        });
                    let expected = report["accounts"].as_u64().unwrap_or(0) as usize;
                    let restored = smoke.edit_before.is_some_and(|original| {
                        account_dialog::cookie_smoke::finish(self, original, expected, window, cx)
                    });
                    report["cookie_import_cookie_sent_on_navigation"] = serde_json::json!(sent);
                    report["cookie_import_expected_route_used"] = serde_json::json!(routed);
                    report["cookie_import_original_account_restored"] = serde_json::json!(restored);
                    report["passed"] =
                        serde_json::json!(report["passed"] == true && sent && routed && restored);
                    smoke.edit_report = Some(report);
                    smoke.stage = 11;
                }
            }
            11 if self.account_dialog.is_none() => {
                let visible = self
                    .visible_browser_identity()
                    .is_some_and(|identity| self.browsers.visible(identity.tab_id) == Some(true));
                let mut report = smoke.edit_report.take().unwrap_or_default();
                report["account_dialog_restored_browser"] = serde_json::json!(visible);
                report["passed"] = serde_json::json!(report["passed"] == true && visible);
                self.persist();
                let _ = std::fs::write(
                    self.data_dir.join("smoke-result.json"),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
                cx.quit();
                return;
            }
            _ => {}
        }
        self.smoke = Some(smoke);
    }
}

#[path = "chrome.rs"]
mod chrome;

#[path = "proxy_editor.rs"]
mod proxy_editor;

#[path = "account_cards.rs"]
mod account_cards;
#[path = "account_dialog.rs"]
mod account_dialog;
#[path = "cookie_ui.rs"]
mod cookie_ui;
#[path = "studio_composer.rs"]
mod studio_composer;
#[path = "studio_jobs.rs"]
mod studio_jobs;
#[path = "studio_media.rs"]
mod studio_media;
#[path = "studio_transcript.rs"]
mod studio_transcript;
#[path = "studio_ui.rs"]
mod studio_ui;
#[path = "transfer_smoke.rs"]
mod transfer_smoke;
#[path = "transfer_ui.rs"]
mod transfer_ui;
#[path = "update_ui.rs"]
mod update_ui;
