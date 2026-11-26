use super::*;
use crate::proxy_settings::{self, ProxyDraft};
use muse_proxy::ProxyProtocol;
use zeroize::Zeroizing;

pub(super) struct TestResult {
    account: AccountId,
    nonce: uuid::Uuid,
    draft: ProxyDraft,
    result: Result<u128, String>,
}

pub(super) struct ProxyEditor {
    pub owner: Option<AccountId>,
    pub enabled: bool,
    pub protocol: ProxyProtocol,
    pub force: bool,
    pub host: Entity<TextInput>,
    pub port: Entity<TextInput>,
    pub user: Entity<TextInput>,
    pub secret: Entity<TextInput>,
    pub reveal: bool,
    pub message: String,
    pub good: bool,
    pub pending: Option<uuid::Uuid>,
    pub applying: bool,
    pub applied_protocol: Option<ProxyProtocol>,
    pub applied_summary: String,
    revisions: [u64; 4],
    last_count: Option<u64>,
    sender: mpsc::Sender<TestResult>,
    receiver: mpsc::Receiver<TestResult>,
}

impl ProxyEditor {
    pub fn new(cx: &mut Context<MuseWindow>) -> Self {
        let host = cx.new(|cx| {
            let mut input = TextInput::new("", cx);
            input.set_placeholder("服务器地址");
            input
        });
        let port = cx.new(|cx| TextInput::new("8080", cx));
        let user = cx.new(|cx| {
            let mut input = TextInput::new("", cx);
            input.set_placeholder("可选");
            input
        });
        let secret = cx.new(|cx| {
            let mut input = TextInput::new("", cx);
            input.set_placeholder("可选");
            input.set_secure(true, cx);
            input
        });
        let (sender, receiver) = mpsc::channel();
        Self {
            owner: None,
            enabled: false,
            protocol: ProxyProtocol::Http,
            force: true,
            host,
            port,
            user,
            secret,
            reveal: false,
            message: String::new(),
            good: false,
            pending: None,
            applying: false,
            applied_protocol: None,
            applied_summary: String::new(),
            revisions: [0; 4],
            last_count: None,
            sender,
            receiver,
        }
    }
    pub fn revisions(&self, cx: &App) -> [u64; 4] {
        [
            self.host.read(cx).revision(),
            self.port.read(cx).revision(),
            self.user.read(cx).revision(),
            self.secret.read(cx).revision(),
        ]
    }
    pub fn draft(&self, cx: &App) -> ProxyDraft {
        ProxyDraft {
            enabled: self.enabled,
            protocol: self.protocol,
            force_proxy: self.force,
            host: self.host.read(cx).text(),
            port: self.port.read(cx).text(),
            user: self.user.read(cx).text(),
            secret: Zeroizing::new(self.secret.read(cx).text()),
        }
    }
    pub fn fill(&mut self, draft: ProxyDraft, cx: &mut App) {
        self.enabled = draft.enabled;
        self.protocol = draft.protocol;
        self.force = draft.force_proxy;
        self.host
            .update(cx, |input, cx| input.set_text(&draft.host, cx));
        self.port
            .update(cx, |input, cx| input.set_text(&draft.port, cx));
        self.user
            .update(cx, |input, cx| input.set_text(&draft.user, cx));
        self.secret.update(cx, |input, cx| {
            input.set_text(&draft.secret, cx);
            input.set_secure(true, cx);
        });
        self.reveal = false;
        self.revisions = self.revisions(cx);
        self.pending = None;
        self.good = false;
    }
}

impl MuseWindow {
    pub(super) fn refresh_proxy_editor(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.workspace.active_account_id() else {
            return;
        };
        let (draft, message) = match proxy_settings::load(&self.data_dir, &self.workspace, id) {
            Ok(draft) => (draft, String::new()),
            Err(error) => {
                let draft = ProxyDraft {
                    enabled: matches!(
                        self.workspace.account(id).map(|a| a.network),
                        Some(network) if network != muse_core::AccountNetwork::Direct
                    ),
                    ..Default::default()
                };
                (draft, error)
            }
        };
        self.proxy_editor.applied_summary = if draft.enabled {
            format!("{} · {}:{}", draft.protocol.label(), draft.host, draft.port)
        } else {
            "系统网络".into()
        };
        self.proxy_editor.applied_protocol =
            (draft.enabled && message.is_empty()).then_some(draft.protocol);
        self.proxy_editor.fill(draft, cx);
        self.proxy_editor.owner = Some(id);
        self.proxy_editor.message = message;
        self.proxy_editor.applying = false;
    }
    pub(super) fn open_proxy_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.smoke.is_none() {
            self.open_account_dialog(self.workspace.active_account_id(), false, window, cx);
            return;
        }
        self.inspector_open = true;
        self.inspector_section = InspectorSection::Proxy;
        if self.proxy_editor.owner != self.workspace.active_account_id() {
            self.refresh_proxy_editor(cx);
        }
        self.animate_chrome_layout(window, cx);
    }
    pub(super) fn proxy_draft_changed(&mut self, cx: &mut Context<Self>) {
        self.proxy_editor.pending = None;
        self.proxy_editor.good = false;
        self.proxy_editor.message = "未保存的更改".into();
        cx.notify();
    }
    pub(super) fn select_proxy_protocol(
        &mut self,
        protocol: Option<ProxyProtocol>,
        cx: &mut Context<Self>,
    ) {
        let editor = &mut self.proxy_editor;
        editor.enabled = protocol.is_some();
        if let Some(protocol) = protocol {
            let port = editor.port.read(cx).text();
            if protocol != editor.protocol
                && (editor.host.read(cx).text().trim().is_empty()
                    || port == editor.protocol.default_port().to_string())
            {
                editor.port.update(cx, |input, cx| {
                    input.set_text(&protocol.default_port().to_string(), cx)
                });
            }
            editor.protocol = protocol;
        }
        editor.revisions = editor.revisions(cx);
        self.proxy_draft_changed(cx);
    }
    pub(super) fn paste_proxy(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.proxy_editor.message = "剪贴板里没有代理地址".into();
            cx.notify();
            return;
        };
        match ProxyDraft::from_url(text.trim(), self.proxy_editor.force) {
            Ok(draft) => {
                self.proxy_editor.fill(draft, cx);
                self.proxy_editor.message = "已填入，保存后生效".into();
            }
            Err(error) => self.proxy_editor.message = error,
        }
        cx.notify();
    }
    pub(super) fn test_proxy(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.workspace.active_account_id() else {
            return;
        };
        let trace_context = muse_proxy::trace::Context {
            account: crate::proxy_trace::account(account),
            action: muse_proxy::trace::new_id(),
            source: Some(muse_proxy::trace::Source::AccountTest),
            ..Default::default()
        };
        muse_proxy::trace::Event::new(
            trace_context,
            muse_proxy::trace::Stage::ManualAction,
            muse_proxy::trace::Outcome::Start,
        )
        .emit();
        let draft = self.proxy_editor.draft(cx);
        let upstream = match draft
            .proxy_url()
            .and_then(|url| muse_proxy::Upstream::parse(&url).map_err(|error| error.to_string()))
        {
            Ok(upstream) => upstream,
            Err(error) => {
                muse_proxy::trace::Event::new(
                    trace_context,
                    muse_proxy::trace::Stage::ProbeEnd,
                    muse_proxy::trace::Outcome::Configuration,
                )
                .emit();
                self.proxy_editor.message = error;
                self.proxy_editor.good = false;
                cx.notify();
                return;
            }
        };
        let nonce = uuid::Uuid::new_v4();
        let sender = self.proxy_editor.sender.clone();
        self.proxy_editor.pending = Some(nonce);
        self.proxy_editor.good = false;
        self.proxy_editor.message = "正在测试代理认证及 muse.ai:443 隧道…".into();
        cx.background_executor()
            .spawn(async move {
                let started = Instant::now();
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| "无法创建代理测试任务".to_owned())
                    .and_then(|runtime| {
                        runtime
                            .block_on(upstream.probe_traced(trace_context))
                            .map_err(|error| error.to_string())
                    })
                    .map(|()| started.elapsed().as_millis());
                let _ = sender.send(TestResult {
                    account,
                    nonce,
                    draft,
                    result,
                });
            })
            .detach();
        cx.notify();
    }
    pub(super) fn save_proxy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.workspace.active_account_id() else {
            return;
        };
        if self.proxy_editor.owner != Some(id) {
            self.refresh_proxy_editor(cx);
            return;
        }
        let draft = self.proxy_editor.draft(cx);
        if let Err(error) = proxy_settings::save(&self.data_dir, &mut self.workspace, id, &draft) {
            self.proxy_editor.message = error;
            self.proxy_editor.good = false;
            cx.notify();
            return;
        }
        self.proxy_editor.applied_summary = if draft.enabled {
            format!("{} · {}:{}", draft.protocol.label(), draft.host, draft.port)
        } else {
            "系统网络".into()
        };
        self.proxy_editor.applied_protocol = draft.enabled.then_some(draft.protocol);
        let tabs = self
            .workspace
            .account(id)
            .map(|a| a.tabs.iter().map(|t| t.id).collect::<Vec<_>>())
            .unwrap_or_default();
        self.ready_pages.retain(|id, _| !tabs.contains(id));
        self.markers.remove(&id);
        self.browsers.invalidate_account(id);
        self.proxy_editor.applying = true;
        self.proxy_editor.pending = None;
        self.proxy_editor.revisions = self.proxy_editor.revisions(cx);
        self.proxy_editor.message = "已保存，正在重新加载此账号…".into();
        self.proxy_editor.good = true;
        self.pending_save = None;
        self.show_start_page = false;
        self.ensure_active(window);
        cx.notify();
    }
    pub(super) fn poll_proxy_editor(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        let revisions = self.proxy_editor.revisions(cx);
        if revisions != self.proxy_editor.revisions {
            self.proxy_editor.revisions = revisions;
            self.proxy_editor.pending = None;
            self.proxy_editor.good = false;
            self.proxy_editor.message = "未保存的更改".into();
            changed = true;
        }
        while let Ok(result) = self.proxy_editor.receiver.try_recv() {
            if self.workspace.active_account_id() != Some(result.account)
                || self.proxy_editor.pending != Some(result.nonce)
                || self.proxy_editor.draft(cx) != result.draft
            {
                continue;
            }
            self.proxy_editor.pending = None;
            match result.result {
                Ok(ms) => {
                    self.proxy_editor.good = true;
                    self.proxy_editor.message = format!(
                        "兼容探测：muse.ai:443 隧道通过 · {ms} ms；实际路由与页面请在账号窗口验证"
                    );
                }
                Err(error) => {
                    self.proxy_editor.good = false;
                    self.proxy_editor.message = error;
                }
            }
            changed = true;
        }
        if let Some(id) = self.workspace.active_account_id() {
            let count = self.browsers.request_count(id);
            if self.proxy_editor.last_count != count {
                self.proxy_editor.last_count = count;
                changed = true;
            }
            if self.proxy_editor.applying
                && !self.browsers.is_rebuilding(id)
                && self
                    .workspace
                    .active_browser_identity()
                    .and_then(|identity| self.browsers.view(identity.tab_id))
                    .is_some()
            {
                self.proxy_editor.applying = false;
                self.proxy_editor.good = true;
                self.proxy_editor.message = "已应用；页面访问结果以账号标签页为准".into();
                self.status = "代理配置已应用".into();
                changed = true;
            }
        }
        changed
    }
}
