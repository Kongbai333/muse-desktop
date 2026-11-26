//! Native browser adapter. Every callback carries immutable account/tab identity.

use crate::{
    page_observer::{self, PageSnapshot},
    session_guard::{PageGate, ProfileBindings},
};
use muse_core::{AccountId, AccountNetwork, BrowserEvent, BrowserIdentity};
#[cfg(target_os = "windows")]
use muse_proxy::LocalAuth;
use muse_proxy::{ProxyBridge, Upstream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc::Sender},
};
#[cfg(target_os = "windows")]
use webview2_com::{
    BasicAuthenticationRequestedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_10, ICoreWebView2Environment7, ICoreWebView2NavigationCompletedEventArgs2,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler, take_pwstr,
};
#[cfg(target_os = "windows")]
use windows::core::{HSTRING, Interface, PWSTR};
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder};
#[cfg(target_os = "windows")]
use wry::{WebViewBuilderExtWindows, WebViewExtWindows};
#[path = "browser_transfer.rs"]
mod transfer;

/// Read the current WebView2 URL without exposing a navigation failure.
pub fn current_url(view: &WebView) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        view.url().ok()
    }
}

pub struct BrowserRequest<'a> {
    pub identity: BrowserIdentity,
    pub url: &'a str,
    pub profile: &'a Path,
    pub network: AccountNetwork,
    pub locale: &'a muse_core::BrowserLocale,
    pub proxy_credentials: Result<(AccountId, Option<uuid::Uuid>), String>,
    pub diagnostic_origin: Option<&'a str>,
}

pub enum BrowserMessage {
    State(BrowserIdentity, BrowserEvent),
    OpenTab(BrowserIdentity, String),
    Command(BrowserIdentity, String),
    Diagnostic(BrowserIdentity, String, String),
    Observation(BrowserIdentity, u64, Option<PageSnapshot>),
    Studio(BrowserIdentity, u64, uuid::Uuid, String),
    /// Still frame of a page, decoded off the UI thread, shown while a native
    /// overlay temporarily hides the live WebView.
    Preview(BrowserIdentity, u64, Option<Arc<gpui::RenderImage>>),
    Cookies(
        AccountId,
        BrowserIdentity,
        uuid::Uuid,
        std::result::Result<Box<crate::cookie_export::CookieBundle>, String>,
    ),
}

pub struct HostedTab {
    pub identity: BrowserIdentity,
    pub view: WebView,
    _context: WebContext,
    _bridge: Option<Arc<ProxyBridge>>,
    gate: Arc<Mutex<PageGate>>,
    last_probe: Cell<u64>,
    pending_probe: Cell<Option<(u64, u64)>>,
    force_quota: Cell<bool>,
    staged: Cell<bool>,
    /// When the page was last hidden; None while it is shown or staged.
    hidden_since: Cell<Option<u64>>,
    /// A suspend was requested and not yet undone by `wake`.
    sleep_requested: Cell<bool>,
    /// Set by WebView2 once the page is actually suspended.
    sleeping: Arc<AtomicBool>,
    /// Last memory target sent to WebView2 (true = low).
    low_memory: Cell<Option<bool>>,
}

/// Resumes a suspended page before it is shown or asked to run script.
#[cfg(target_os = "windows")]
fn wake(tab: &HostedTab) {
    let requested = tab.sleep_requested.replace(false);
    let asleep = tab.sleeping.swap(false, Ordering::Relaxed);
    if requested || asleep {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
        if let Ok(native) = tab.view.webview().cast::<ICoreWebView2_3>() {
            let _ = unsafe { native.Resume() };
        }
    }
}

/// Hidden pages ask WebView2 to keep as little memory as it can; the page
/// keeps running, so quota and studio work are unaffected.
#[cfg(target_os = "windows")]
fn set_memory_target(tab: &HostedTab, low: bool) {
    if tab.low_memory.replace(Some(low)) == Some(low) {
        return;
    }
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_19,
    };
    if let Ok(native) = tab.view.webview().cast::<ICoreWebView2_19>() {
        let level = if low {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
        } else {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
        };
        let _ = unsafe { native.SetMemoryUsageTargetLevel(level) };
    }
}

#[cfg(target_os = "windows")]
fn webview_theme(mode: muse_core::ThemeMode) -> wry::Theme {
    match mode {
        muse_core::ThemeMode::Light => wry::Theme::Light,
        muse_core::ThemeMode::Dark => wry::Theme::Dark,
    }
}

fn page_background(mode: muse_core::ThemeMode) -> wry::RGBA {
    let color = crate::theme::palette_for(mode).canvas;
    ((color >> 16) as u8, (color >> 8) as u8, color as u8, 255)
}

pub struct BrowserPool {
    tabs: HashMap<muse_core::TabId, HostedTab>,
    sender: Sender<BrowserMessage>,
    last_active: Cell<Option<BrowserIdentity>>,
    last_bounds: Cell<Option<[f64; 4]>>,
    last_zoom: Cell<Option<(muse_core::TabId, i64)>>,
    data_root: PathBuf,
    display: Cell<muse_core::DisplayPreferences>,
    bridges: HashMap<muse_core::AccountId, Arc<ProxyBridge>>,
    failures: HashMap<BrowserIdentity, String>,
    closing: HashMap<muse_core::AccountId, Vec<ClosingProcess>>,
    profiles: ProfileBindings,
    observations: HashMap<BrowserIdentity, ObserverFlags>,
    background: HashSet<BrowserIdentity>,
    cookie_only: HashSet<BrowserIdentity>,
}

#[derive(Clone, serde::Serialize)]
pub struct ObserverFlags {
    pub status: String,
    pub action_found: bool,
    pub scripts_scanned: u16,
    pub style_waiting: u16,
    pub content_present: bool,
    pub document_complete: bool,
    pub preload_found: bool,
    pub settings_loaded: bool,
}

#[cfg(target_os = "windows")]
fn read_stream(stream: &windows::Win32::System::Com::IStream) -> Option<Vec<u8>> {
    use windows::Win32::System::Com::STREAM_SEEK_SET;
    unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.ok()?;
    let mut output = Vec::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let mut read = 0u32;
        let result = unsafe {
            stream.Read(
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                Some(&mut read),
            )
        };
        if result.is_err() {
            return None;
        }
        if read == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..read as usize]);
        if output.len() > 64_000_000 {
            return None;
        }
    }
    (!output.is_empty()).then_some(output)
}

fn decode_preview(bytes: &[u8]) -> Option<Arc<gpui::RenderImage>> {
    let mut pixels = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    if pixels.width() == 0
        || pixels.height() == 0
        || pixels.width() > 8192
        || pixels.height() > 8192
    {
        return None;
    }
    // GPUI frames are BGRA.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(Arc::new(gpui::RenderImage::new(vec![image::Frame::new(
        pixels,
    )])))
}

#[cfg(target_os = "windows")]
struct ClosingProcess(windows::Win32::Foundation::HANDLE);
#[cfg(target_os = "windows")]
impl Drop for ClosingProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Same steps as Chromium's page zoom menu.
pub const ZOOM_LEVELS: [f64; 15] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];

/// Next zoom step from `current` in `direction` (+1 in, -1 out).
pub fn zoom_step(current: f64, direction: i32) -> f64 {
    if direction > 0 {
        ZOOM_LEVELS
            .iter()
            .copied()
            .find(|level| *level > current + 0.005)
            .unwrap_or(ZOOM_LEVELS[ZOOM_LEVELS.len() - 1])
    } else {
        ZOOM_LEVELS
            .iter()
            .rev()
            .copied()
            .find(|level| *level < current - 0.005)
            .unwrap_or(ZOOM_LEVELS[0])
    }
}

const KEYBOARD_BRIDGE: &str = r#"
(() => {
  if (window.top !== window) return;
  window.addEventListener('keydown', event => {
    let action = null;
    if (event.altKey && event.key === 'ArrowLeft') action = 'back';
    if (event.altKey && event.key === 'ArrowRight') action = 'forward';
    if (event.ctrlKey && event.key.toLowerCase() === 'l') action = 'address';
    if (event.ctrlKey && event.key.toLowerCase() === 'r') action = 'reload';
    if (action) {
      event.preventDefault();
      window.ipc.postMessage(JSON.stringify({kind:'command', action}));
    }
  }, true);
})();
"#;

impl BrowserPool {
    pub fn new(
        sender: Sender<BrowserMessage>,
        data_root: PathBuf,
        display: muse_core::DisplayPreferences,
    ) -> Self {
        Self {
            tabs: HashMap::new(),
            sender,
            last_active: Cell::new(None),
            last_bounds: Cell::new(None),
            last_zoom: Cell::new(None),
            data_root,
            display: Cell::new(display),
            bridges: HashMap::new(),
            failures: HashMap::new(),
            closing: HashMap::new(),
            profiles: ProfileBindings::default(),
            observations: HashMap::new(),
            background: HashSet::new(),
            cookie_only: HashSet::new(),
        }
    }

    pub fn ensure(
        &mut self,
        request: BrowserRequest<'_>,
        window: &gpui::Window,
        bounds: Rect,
    ) -> Result<(), String> {
        let identity = request.identity;
        let result = self.ensure_inner(request, window, bounds);
        if let Err(error) = &result
            && !self.is_rebuilding(identity.account_id)
        {
            self.failures.insert(identity, error.clone());
        }
        result
    }

    pub fn ensure_reader(
        &mut self,
        request: BrowserRequest,
        window: &gpui::Window,
        bounds: Rect,
    ) -> Result<(), String> {
        let identity = request.identity;
        let result = self.ensure_inner(request, window, bounds);
        if let Err(error) = &result {
            if !self.is_rebuilding(identity.account_id) {
                self.failures.insert(identity, error.clone());
            }
        } else {
            self.cookie_only.insert(identity);
            self.background.insert(identity);
        }
        result
    }

    /// Import only into a newly-created, isolated about:blank account view.
    /// The caller commits the new account only after native read-back succeeds.
    pub fn stage_cookie_import(
        &mut self,
        request: BrowserRequest<'_>,
        cookies: &crate::cookie_import::CookieImport,
        window: &gpui::Window,
        bounds: Rect,
    ) -> Result<(), String> {
        let identity = request.identity;
        if request.url != "about:blank" || self.tabs.contains_key(&identity.tab_id) {
            return Err("只能将 Cookie 导入新账号的独立环境".into());
        }
        if let Err(error) = self.ensure_reader(request, window, bounds) {
            self.invalidate_account(identity.account_id);
            return Err(error);
        }
        let result = (|| {
            let view = self.view(identity.tab_id).ok_or("无法准备账号浏览器")?;
            for item in &cookies.items {
                let cookie = item.native()?;
                view.set_cookie(&cookie)
                    .map_err(|_| "原生浏览器未能写入 Cookie，新账号未保存")?;
            }
            let actual = view
                .cookies()
                .map_err(|_| "无法核验已写入的 Cookie，新账号未保存")?;
            if !cookies
                .items
                .iter()
                .all(|item| actual.iter().any(|cookie| item.matches(cookie)))
            {
                return Err("浏览器未完整接受 Cookie，请重新导出此账号的 Cookie".into());
            }
            Ok(())
        })();
        if result.is_err() {
            self.discard_cookie_import(identity, cookies);
        }
        result
    }

    pub fn discard_cookie_import(
        &mut self,
        identity: BrowserIdentity,
        cookies: &crate::cookie_import::CookieImport,
    ) {
        if self.is_cookie_reader(identity) {
            if let Some(view) = self.view(identity.tab_id) {
                for item in &cookies.items {
                    if let Ok(cookie) = item.native() {
                        let _ = view.delete_cookie(&cookie);
                    }
                }
            }
            self.invalidate_account(identity.account_id);
        }
    }

    pub fn activate_cookie_import(
        &mut self,
        identity: BrowserIdentity,
        url: &str,
    ) -> Result<(), String> {
        if !self.cookie_only.remove(&identity) {
            return Err("账号的 Cookie 导入环境已改变".into());
        }
        self.background.remove(&identity);
        self.last_active.set(None);
        self.view(identity.tab_id)
            .ok_or("账号浏览器已关闭")?
            .load_url(url)
            .map_err(|_| "Cookie 已保存，网页暂时无法打开，请刷新重试".into())
    }

    fn ensure_inner(
        &mut self,
        request: BrowserRequest<'_>,
        window: &gpui::Window,
        bounds: Rect,
    ) -> Result<(), String> {
        let BrowserRequest {
            identity,
            url,
            profile,
            network,
            locale,
            proxy_credentials,
            diagnostic_origin,
        } = request;
        #[cfg(target_os = "windows")]
        if let Some(processes) = self.closing.get(&identity.account_id) {
            use windows::Win32::{
                Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject,
            };
            if processes
                .iter()
                .any(|process| unsafe { WaitForSingleObject(process.0, 0) } != WAIT_OBJECT_0)
            {
                return Err("正在重建此账号的浏览器环境，请稍候".into());
            }
        }
        self.closing.remove(&identity.account_id);
        if self
            .tabs
            .get(&identity.tab_id)
            .is_some_and(|tab| tab.identity == identity)
        {
            return Ok(());
        }
        if let Some(error) = self.failures.get(&identity) {
            return Err(error.clone());
        }
        let bridge = if network != AccountNetwork::Direct {
            if let Some(bridge) = self.bridges.get(&identity.account_id) {
                if !bridge.is_running() {
                    return Err("此账号的代理已停止；未使用直连".into());
                }
                Some(bridge.clone())
            } else {
                let result = (|| {
                    let (id, revision) = proxy_credentials?;
                    let secret = crate::secrets::ProtectedStore::new(&self.data_root)
                        .load_selected(id, revision)?;
                    let upstream = Upstream::parse(secret.as_str()).map_err(|e| e.to_string())?;
                    ProxyBridge::start(upstream)
                        .map(Arc::new)
                        .map_err(|e| e.to_string())
                })();
                match result {
                    Ok(bridge) => {
                        self.bridges.insert(identity.account_id, bridge.clone());
                        Some(bridge)
                    }
                    Err(error) => {
                        self.failures.insert(identity, error.clone());
                        return Err(error);
                    }
                }
            }
        } else {
            None
        };
        self.tabs.remove(&identity.tab_id);
        std::fs::create_dir_all(profile).map_err(|e| e.to_string())?;
        let mut context = WebContext::new(Some(profile.to_owned()));
        let navigation_tx = self.sender.clone();
        let title_tx = self.sender.clone();
        let popup_tx = self.sender.clone();
        let load_tx = self.sender.clone();
        let ipc_tx = self.sender.clone();
        let gate = Arc::new(Mutex::new(PageGate::default()));
        let navigation_gate = gate.clone();
        let diagnostic_origin = diagnostic_origin.map(str::to_owned);
        let mut builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_url("about:blank")
            .with_background_color(page_background(self.display.get().theme))
            .with_bounds(bounds)
            .with_focused(false)
            .with_visible(false)
            // Ctrl + wheel / Ctrl + = / Ctrl + - / Ctrl + 0 while the page has focus.
            .with_hotkeys_zoom(true)
            .with_initialization_script_for_main_only(KEYBOARD_BRIDGE, true)
            .with_initialization_script_for_main_only(page_observer::INITIALIZATION, true)
            .with_initialization_script_for_main_only(crate::studio::ADAPTER, true)
            .with_navigation_handler(move |url| {
                if url == "about:blank" {
                    return true;
                }
                let allowed = muse_core::normalize_web_url(&url).is_ok();
                if allowed {
                    navigation_gate
                        .lock()
                        .unwrap()
                        .navigate(page_observer::now_ms());
                    let _ =
                        navigation_tx.send(BrowserMessage::State(identity, BrowserEvent::Url(url)));
                }
                allowed
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_tx.send(BrowserMessage::State(identity, BrowserEvent::Title(title)));
            })
            .with_on_page_load_handler(move |event, url| {
                let loading = matches!(event, PageLoadEvent::Started);
                let _ = load_tx.send(BrowserMessage::State(identity, BrowserEvent::Url(url)));
                let _ = load_tx.send(BrowserMessage::State(
                    identity,
                    BrowserEvent::Loading(loading),
                ));
            })
            .with_new_window_req_handler(move |url, _| {
                if muse_core::normalize_web_url(&url).is_ok() {
                    let _ = popup_tx.send(BrowserMessage::OpenTab(identity, url));
                }
                NewWindowResponse::Deny
            })
            .with_ipc_handler(move |request| {
                if request.body().len() > 4096 {
                    return;
                }
                let Ok(message) = serde_json::from_str::<serde_json::Value>(request.body()) else {
                    return;
                };
                match message.get("kind").and_then(|v| v.as_str()) {
                    Some("command") => {
                        if let Some(
                            action @ ("back" | "forward" | "reload" | "address" | "zoom_in"
                            | "zoom_out" | "zoom_reset"),
                        ) = message.get("action").and_then(|v| v.as_str())
                        {
                            let _ =
                                ipc_tx.send(BrowserMessage::Command(identity, action.to_owned()));
                        }
                    }
                    Some("diagnostic") => {
                        let trusted = diagnostic_origin.as_ref().is_some_and(|origin| {
                            url::Url::parse(&request.uri().to_string())
                                .ok()
                                .is_some_and(|url| url.origin().ascii_serialization() == *origin)
                        });
                        if trusted
                            && let Some(marker) = message.get("marker").and_then(|v| v.as_str())
                        {
                            let _ = ipc_tx.send(BrowserMessage::Diagnostic(
                                identity,
                                marker.chars().take(100).collect(),
                                request.uri().to_string(),
                            ));
                        }
                    }
                    _ => {}
                }
            });
        #[cfg(target_os = "windows")]
        {
            builder = builder.with_theme(webview_theme(self.display.get().theme));
        }
        #[cfg(target_os = "windows")]
        {
            locale.validate()?;
            let mut arguments =
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection".to_owned();
            if let Some(bridge) = &bridge {
                let bypass = if network.force_proxy() {
                    " --proxy-bypass-list=<-loopback>"
                } else {
                    ""
                };
                arguments = format!("--proxy-server=http://{}{bypass}", bridge.address());
            }
            // Account tabs, popups re-opened as tabs, and internal readers must
            // use identical environment language/options with the same profile.
            if let Some(tab) = self
                .tabs
                .values()
                .find(|t| t.identity.account_id == identity.account_id)
            {
                builder = builder.with_environment(tab.view.environment());
            } else if let Some(language) = locale.language.as_deref() {
                builder = builder.with_environment(crate::browser_locale::environment(
                    profile, language, &arguments,
                )?);
            } else {
                builder = builder.with_additional_browser_args(arguments);
            }
        }
        let view = builder
            .build_as_child(window)
            .map_err(|error| format!("浏览器创建失败，请检查 WebView2 Runtime：{error}"))?;
        #[cfg(target_os = "windows")]
        if let Some(bridge) = &bridge {
            install_local_auth(&view, bridge.shared_auth())?;
        }
        #[cfg(target_os = "windows")]
        {
            let mut actual = PWSTR::null();
            let environment: ICoreWebView2Environment7 = view
                .environment()
                .cast()
                .map_err(|_| "浏览器版本不支持目录核验")?;
            unsafe { environment.UserDataFolder(&mut actual) }
                .map_err(|_| "无法核验浏览器实际数据目录")?;
            let actual = take_pwstr(actual);
            self.profiles
                .verify(identity.account_id, profile, Path::new(&actual))?;
        }
        view.zoom(self.display.get().page_zoom())
            .map_err(|error| format!("无法恢复页面缩放：{error}"))?;
        #[cfg(target_os = "windows")]
        install_navigation_errors(&view, identity, self.sender.clone(), gate.clone())?;
        #[cfg(target_os = "windows")]
        crate::browser_locale::apply_timezone(&view, locale.timezone.as_deref())?;
        view.load_url(url).map_err(|_| "无法打开账号页面")?;
        self.tabs.insert(
            identity.tab_id,
            HostedTab {
                identity,
                view,
                _context: context,
                _bridge: bridge,
                gate,
                last_probe: Cell::new(0),
                pending_probe: Cell::new(None),
                force_quota: Cell::new(false),
                staged: Cell::new(false),
                hidden_since: Cell::new(None),
                sleep_requested: Cell::new(false),
                sleeping: Arc::new(AtomicBool::new(false)),
                low_memory: Cell::new(None),
            },
        );
        self.last_active.set(None);
        Ok(())
    }

    pub fn view(&self, id: muse_core::TabId) -> Option<&WebView> {
        self.tabs.get(&id).map(|tab| &tab.view)
    }

    /// Page zoom of a tab as WebView2 reports it; 1.0 is 100%.
    pub fn zoom(&self, id: muse_core::TabId) -> Option<f64> {
        let tab = self.tabs.get(&id)?;
        #[cfg(target_os = "windows")]
        {
            let mut factor = 0f64;
            unsafe { tab.view.controller().ZoomFactor(&mut factor) }.ok()?;
            (factor.is_finite() && factor > 0.).then_some(factor)
        }
    }

    pub fn visible(&self, id: muse_core::TabId) -> Option<bool> {
        let tab = self.tabs.get(&id)?;
        #[cfg(target_os = "windows")]
        {
            let mut visible = Default::default();
            unsafe { tab.view.controller().IsVisible(&mut visible) }.ok()?;
            Some(visible.as_bool())
        }
    }

    pub fn set_zoom(&self, id: muse_core::TabId, factor: f64) -> Result<(), String> {
        let tab = self.tabs.get(&id).ok_or("页面尚未打开")?;
        tab.view
            .zoom(factor.clamp(ZOOM_LEVELS[0], ZOOM_LEVELS[ZOOM_LEVELS.len() - 1]))
            .map_err(|error| error.to_string())?;
        self.remember_zoom(factor);
        Ok(())
    }

    pub fn remember_zoom(&self, factor: f64) {
        let mut display = self.display.get();
        if !display.set_page_zoom(factor) {
            return;
        }
        self.display.set(display);
        for tab in self.tabs.values() {
            let _ = tab.view.zoom(display.page_zoom());
        }
    }

    pub fn set_theme(&self, mode: muse_core::ThemeMode) -> Result<(), String> {
        let mut display = self.display.get();
        display.theme = mode;
        self.display.set(display);
        let mut error = None;
        for tab in self.tabs.values() {
            #[cfg(target_os = "windows")]
            if let Err(reason) = tab.view.set_theme(webview_theme(mode)) {
                error.get_or_insert_with(|| reason.to_string());
            }
            let _ = tab.view.set_background_color(page_background(mode));
        }
        error.map_or(Ok(()), Err)
    }

    /// True once per change of the visible tab's zoom, including Ctrl + wheel
    /// inside the page, so the percentage in the toolbar stays current.
    pub fn zoom_changed(&self, id: Option<muse_core::TabId>) -> bool {
        let current = id.and_then(|id| {
            self.zoom(id)
                .map(|zoom| (id, (zoom * 1000.).round() as i64))
        });
        if self.last_zoom.get() == current {
            return false;
        }
        self.last_zoom.set(current);
        true
    }
    pub fn account_running(&self, id: muse_core::AccountId) -> bool {
        self.tabs.values().any(|t| t.identity.account_id == id)
            && !self
                .tabs
                .values()
                .filter(|t| t.identity.account_id == id)
                .all(|t| self.cookie_only.contains(&t.identity))
    }
    pub fn account_ready(&self, id: muse_core::AccountId) -> bool {
        self.tabs.values().any(|t| {
            t.identity.account_id == id
                && !self.cookie_only.contains(&t.identity)
                && t.gate.lock().unwrap().ready()
        })
    }
    pub fn cookie_identity(&self, id: muse_core::AccountId) -> Option<BrowserIdentity> {
        self.tabs
            .values()
            .find(|tab| {
                tab.identity.account_id == id
                    && !self.cookie_only.contains(&tab.identity)
                    && current_url(&tab.view).is_some_and(|url| muse_core::trusted_muse_url(&url))
            })
            .map(|tab| tab.identity)
    }
    pub fn is_cookie_reader(&self, identity: BrowserIdentity) -> bool {
        self.cookie_only.contains(&identity)
    }
    pub fn close_cookie_tab(&mut self, identity: BrowserIdentity) {
        if self.cookie_only.remove(&identity) {
            self.background.remove(&identity);
            self.tabs.remove(&identity.tab_id);
            self.failures.remove(&identity);
            self.observations.remove(&identity);
            self.last_active.set(None);
        }
    }
    #[cfg(target_os = "windows")]
    pub fn extract_cookies(&self, identity: BrowserIdentity, request: uuid::Uuid) -> bool {
        let Some(tab) = self.tabs.get(&identity.tab_id).filter(|t| {
            t.identity == identity
                && (self.cookie_only.contains(&identity)
                    || (t.gate.lock().unwrap().ready()
                        && current_url(&t.view)
                            .is_some_and(|url| muse_core::trusted_muse_url(&url))))
        }) else {
            return false;
        };
        let sender = self.sender.clone();
        let account = identity.account_id;
        use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_2, ICoreWebView2Cookie};
        use webview2_com::{GetCookiesCompletedHandler, take_pwstr};
        use windows::core::{HSTRING, Interface, PWSTR};
        let Ok(native) = tab.view.webview().cast::<ICoreWebView2_2>() else {
            return false;
        };
        let manager = unsafe { native.CookieManager() };
        let Ok(manager) = manager else {
            return false;
        };
        let origin = HSTRING::from("https://muse.ai/");
        unsafe {
            manager.GetCookies(
                &origin,
                &GetCookiesCompletedHandler::create(Box::new(move |status, list| {
            let result=(||->std::result::Result<_,String>{
                status.map_err(|_|"读取 Muse Cookie 失败".to_owned())?;
                let Some(list)=list else{return Err("Cookie 列表暂不可用".into());};
                let mut count=0;list.Count(&mut count).map_err(|_|"读取 Cookie 数量失败")?;
                if count>512{return Err("Cookie 数量超过安全限制".into());}
                let mut cookies=Vec::with_capacity(count as usize);
                for index in 0..count {
                    let cookie:ICoreWebView2Cookie=list.GetValueAtIndex(index).map_err(|_|"读取 Cookie 项失败")?;
                    let read=|getter:fn(&ICoreWebView2Cookie,&mut PWSTR)->windows::core::Result<()>|->std::result::Result<String,String>{let mut raw=PWSTR::null();getter(&cookie,&mut raw).map_err(|_|"读取 Cookie 字段失败".to_owned())?;Ok(take_pwstr(raw))};
                    let name=read(|c,p|c.Name(p))?;let value=read(|c,p|c.Value(p))?;
                    if name.is_empty() || name.len()>256 || value.len()>32_768 || name.chars().any(char::is_control) || value.chars().any(|ch|ch=='\r'||ch=='\n') {continue;}
                    let domain=read(|c,p|c.Domain(p))?;let path=read(|c,p|c.Path(p))?;
                    if !(domain=="muse.ai"||domain==".muse.ai"||domain=="www.muse.ai"||domain==".www.muse.ai"){continue;}
                    let mut expires=0.;let mut http=Default::default();let mut secure=Default::default();let mut session=Default::default();let mut same=Default::default();
                    cookie.Expires(&mut expires).map_err(|_|"Cookie 过期字段无效")?;cookie.IsHttpOnly(&mut http).map_err(|_|"Cookie 安全标记无效")?;cookie.IsSecure(&mut secure).map_err(|_|"Cookie 安全标记无效")?;cookie.IsSession(&mut session).map_err(|_|"Cookie 会话标记无效")?;cookie.SameSite(&mut same).map_err(|_|"Cookie SameSite 无效")?;
                    let expires=(!session.as_bool()&&expires>=0.).then_some(expires as i64);
                    cookies.push(crate::cookie_export::CookieItem{name,value,domain,path,expires,http_only:http.as_bool(),secure:secure.as_bool(),same_site:match same.0{1=>"Strict",2=>"Lax",3=>"None",_=>"Unspecified"}.into()});
                }
                Ok(Box::new(crate::cookie_export::CookieBundle{items:cookies,captured_at:crate::page_observer::now_ms()}))
            })();
            let _=sender.send(BrowserMessage::Cookies(account,identity,request,result));Ok(())
                })),
            )
        }.is_ok()
    }
    pub fn set_background(&mut self, identities: impl Iterator<Item = BrowserIdentity>) {
        let mut next: HashSet<_> = identities.collect();
        next.extend(self.cookie_only.iter().copied());
        if next != self.background {
            self.background = next;
            self.last_active.set(None);
        }
    }
    pub fn evaluate_studio(
        &self,
        identity: BrowserIdentity,
        job: uuid::Uuid,
        script: &str,
    ) -> bool {
        let Some(tab) = self
            .tabs
            .get(&identity.tab_id)
            .filter(|t| t.identity == identity)
        else {
            return false;
        };
        wake(tab);
        let epoch = tab.gate.lock().unwrap().epoch();
        let sender = self.sender.clone();
        tab.view
            .evaluate_script_with_callback(script, move |raw| {
                let _ = sender.send(BrowserMessage::Studio(identity, epoch, job, raw));
            })
            .is_ok()
    }
    pub fn accepts_studio(&self, identity: BrowserIdentity, epoch: u64) -> bool {
        self.tabs.get(&identity.tab_id).is_some_and(|t| {
            t.identity == identity
                && t.gate.lock().unwrap().epoch() == epoch
                && current_url(&t.view).is_some_and(|url| muse_core::trusted_muse_url(&url))
        })
    }
    pub fn accepts_cookie_result(&self, identity: BrowserIdentity) -> bool {
        self.tabs.get(&identity.tab_id).is_some_and(|t| {
            t.identity == identity
                && (self.cookie_only.contains(&identity)
                    || current_url(&t.view).is_some_and(|url| muse_core::trusted_muse_url(&url)))
        })
    }

    pub fn startup_error(&self, identity: BrowserIdentity) -> Option<&str> {
        self.failures
            .get(&identity)
            .map(String::as_str)
            .or_else(|| {
                self.tabs
                    .get(&identity.tab_id)
                    .filter(|tab| tab.identity == identity)
                    .and_then(|tab| {
                        tab.gate
                            .lock()
                            .unwrap()
                            .readiness_timed_out()
                            .then_some("页面暂未就绪，请重试或检查代理连接")
                    })
            })
    }

    pub fn profile_verified(&self, id: muse_core::AccountId) -> bool {
        self.profiles.verified(id) && self.tabs.values().any(|tab| tab.identity.account_id == id)
    }
    pub fn observer_flags(&self, identity: BrowserIdentity) -> Option<&ObserverFlags> {
        self.observations.get(&identity)
    }
    pub fn readiness_diagnostics(&self, identity: BrowserIdentity) -> serde_json::Value {
        self.tabs
            .get(&identity.tab_id)
            .map(|tab| {
                tab.gate
                    .lock()
                    .unwrap()
                    .diagnostics(page_observer::now_ms())
            })
            .unwrap_or(serde_json::Value::Null)
    }
    /// Exercise the real timeout/reload path only inside the existing proxy fixture.
    pub fn exercise_readiness_timeout(&mut self, identity: BrowserIdentity) -> bool {
        if !std::env::args().any(|arg| {
            matches!(
                arg.as_str(),
                "--proxy-smoke-test" | "--proxy-edit-smoke-test"
            )
        }) {
            return false;
        }
        let Some(tab) = self.tabs.get(&identity.tab_id) else {
            return false;
        };
        tab.gate
            .lock()
            .unwrap()
            .navigate(page_observer::now_ms().saturating_sub(25_001));
        self.poll_page(identity);
        self.readiness_diagnostics(identity)["failed"] == true
    }
    pub fn visible_views_ready(&self) -> bool {
        self.tabs.values().all(|tab| {
            self.visible(tab.identity.tab_id).is_some_and(|visible| {
                !visible || tab.staged.get() || tab.gate.lock().unwrap().ready()
            })
        })
    }
    pub fn active_render_ready(&self, identity: BrowserIdentity) -> bool {
        self.tabs.get(&identity.tab_id).is_some_and(|tab| {
            self.page_ready(identity)
                && !tab.staged.get()
                && self.visible(identity.tab_id) == Some(true)
        })
    }

    pub fn page_ready(&self, identity: BrowserIdentity) -> bool {
        self.tabs
            .get(&identity.tab_id)
            .is_some_and(|tab| tab.identity == identity && tab.gate.lock().unwrap().ready())
    }

    pub fn poll_page(&mut self, identity: BrowserIdentity) {
        let Some(tab) = self
            .tabs
            .get(&identity.tab_id)
            .filter(|tab| tab.identity == identity)
        else {
            return;
        };
        let now = page_observer::now_ms();
        let (epoch, ready, failed) = {
            let mut gate = tab.gate.lock().unwrap();
            if gate.timed_out(now) {
                let _ = self.sender.send(BrowserMessage::State(
                    identity,
                    BrowserEvent::Error("页面就绪检查超时".into()),
                ));
            }
            (gate.epoch(), gate.ready(), gate.failed())
        };
        if failed || self.failures.contains_key(&identity) {
            return;
        }
        if let Some((pending_epoch, at)) = tab.pending_probe.get() {
            if pending_epoch == epoch && now.saturating_sub(at) < 3000 {
                return;
            }
            tab.pending_probe.set(None);
        }
        if now.saturating_sub(tab.last_probe.get()) < if ready { 2000 } else { 180 } {
            return;
        }
        tab.last_probe.set(now);
        tab.pending_probe.set(Some((epoch, now)));
        let force = tab.force_quota.replace(false);
        let sender = self.sender.clone();
        if tab
            .view
            .evaluate_script_with_callback(&page_observer::script(force), move |value| {
                let _ = sender.send(BrowserMessage::Observation(
                    identity,
                    epoch,
                    page_observer::decode(&value),
                ));
            })
            .is_err()
        {
            tab.pending_probe.set(None);
        }
    }

    pub fn accept_observation(
        &mut self,
        identity: BrowserIdentity,
        epoch: u64,
        snapshot: Option<&PageSnapshot>,
    ) -> bool {
        let Some(tab) = self
            .tabs
            .get(&identity.tab_id)
            .filter(|tab| tab.identity == identity)
        else {
            return false;
        };
        if tab
            .pending_probe
            .get()
            .is_some_and(|(pending, _)| pending == epoch)
        {
            tab.pending_probe.set(None);
        }
        let Some(snapshot) = snapshot else {
            self.observations.insert(
                identity,
                ObserverFlags {
                    status: "decode_failed".into(),
                    action_found: false,
                    scripts_scanned: 0,
                    style_waiting: 0,
                    content_present: false,
                    document_complete: false,
                    preload_found: false,
                    settings_loaded: false,
                },
            );
            return false;
        };
        if current_url(&tab.view).as_deref() != Some(snapshot.url.as_str()) {
            self.observations.insert(
                identity,
                ObserverFlags {
                    status: "document_changed".into(),
                    action_found: snapshot.action_found,
                    scripts_scanned: snapshot.scripts_scanned,
                    style_waiting: snapshot.style_waiting,
                    content_present: snapshot.content_present,
                    document_complete: snapshot.document_state == "complete",
                    preload_found: snapshot.preload_found,
                    settings_loaded: snapshot.settings_loaded,
                },
            );
            return false;
        }
        let mut gate = tab.gate.lock().unwrap();
        let accepted = gate.observe(epoch, snapshot.ready, page_observer::now_ms());
        if accepted {
            self.last_active.set(None);
            let status = match snapshot.quota_status.as_str() {
                "ok" | "waiting" | "syncing" | "not_found" | "request_failed" | "logged_out"
                | "unrecognized" | "unsupported" | "auth_required" | "rate_limited" => {
                    snapshot.quota_status.clone()
                }
                _ => "unknown".into(),
            };
            self.observations.insert(
                identity,
                ObserverFlags {
                    status,
                    action_found: snapshot.action_found,
                    scripts_scanned: snapshot.scripts_scanned,
                    style_waiting: snapshot.style_waiting,
                    content_present: snapshot.content_present,
                    document_complete: snapshot.document_state == "complete",
                    preload_found: snapshot.preload_found,
                    settings_loaded: snapshot.settings_loaded,
                },
            );
        }
        accepted
    }

    pub fn refresh_quota(&self, identity: BrowserIdentity) {
        if let Some(tab) = self
            .tabs
            .get(&identity.tab_id)
            .filter(|tab| tab.identity == identity)
        {
            wake(tab);
            tab.force_quota.set(true);
            tab.last_probe.set(0);
        }
    }

    /// Captures the visible page as a PNG. The decoded frame arrives as
    /// `BrowserMessage::Preview` with the same `request` number.
    #[cfg(target_os = "windows")]
    pub fn capture_preview(&self, identity: BrowserIdentity, request: u64) -> bool {
        use webview2_com::{
            CapturePreviewCompletedHandler,
            Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
        };
        let Some(tab) = self
            .tabs
            .get(&identity.tab_id)
            .filter(|tab| tab.identity == identity)
        else {
            return false;
        };
        let Some(stream) = (unsafe { windows::Win32::UI::Shell::SHCreateMemStream(None) }) else {
            return false;
        };
        let sender = self.sender.clone();
        let output = stream.clone();
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |status| {
            let bytes = status.ok().and_then(|()| read_stream(&output));
            std::thread::spawn(move || {
                let image = bytes.and_then(|bytes| decode_preview(&bytes));
                let _ = sender.send(BrowserMessage::Preview(identity, request, image));
            });
            Ok(())
        }));
        unsafe {
            tab.view.webview().CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                &stream,
                &handler,
            )
        }
        .is_ok()
    }

    /// Asks the page to open, read and close its invite popover. The result
    /// arrives with the next page snapshot.
    pub fn refresh_invite(&self, identity: BrowserIdentity) -> bool {
        self.tabs
            .get(&identity.tab_id)
            .filter(|tab| tab.identity == identity)
            .is_some_and(|tab| {
                wake(tab);
                tab.view
                    .evaluate_script("void window.__museDesktopObserver?.refreshInvite?.()")
                    .is_ok()
            })
    }

    pub fn request_count(&self, id: muse_core::AccountId) -> Option<u64> {
        self.bridges.get(&id).map(|bridge| bridge.request_count())
    }
    pub fn is_rebuilding(&self, id: muse_core::AccountId) -> bool {
        self.closing.contains_key(&id)
    }

    pub fn invalidate_account(&mut self, id: muse_core::AccountId) {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
            let mut processes = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for tab in self
                .tabs
                .values()
                .filter(|tab| tab.identity.account_id == id)
            {
                let mut pid = 0;
                if unsafe { tab.view.webview().BrowserProcessId(&mut pid) }.is_ok()
                    && pid != 0
                    && seen.insert(pid)
                    && let Ok(handle) = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
                {
                    processes.push(ClosingProcess(handle));
                }
            }
            if !processes.is_empty() {
                self.closing.insert(id, processes);
            }
        }
        self.tabs.retain(|_, tab| tab.identity.account_id != id);
        self.bridges.remove(&id);
        self.failures
            .retain(|identity, _| identity.account_id != id);
        self.observations
            .retain(|identity, _| identity.account_id != id);
        self.cookie_only
            .retain(|identity| identity.account_id != id);
        self.background.retain(|identity| identity.account_id != id);
        self.last_active.set(None);
    }

    pub fn sync_visibility(&self, active: Option<BrowserIdentity>, bounds: Rect) {
        let active_changed = self.last_active.replace(active) != active;
        let size = bounds.size.to_logical::<f64>(1.0);
        let position = bounds.position.to_logical::<f64>(1.0);
        let key = [position.x, position.y, size.width, size.height];
        let bounds_changed = self.last_bounds.replace(Some(key)) != Some(key);
        for tab in self.tabs.values() {
            let cookie_reader = self.cookie_only.contains(&tab.identity);
            let visible =
                !cookie_reader && Some(tab.identity) == active && tab.gate.lock().unwrap().ready();
            let staging = (Some(tab.identity) == active || self.background.contains(&tab.identity))
                && !visible
                && !cookie_reader
                && self.startup_error(tab.identity).is_none();
            let shown = visible || staging;
            if shown {
                wake(tab);
            }
            if active_changed
                || Some(tab.identity) == active
                || self.background.contains(&tab.identity)
            {
                if staging && (!tab.staged.get() || active_changed || bounds_changed) {
                    let mut offscreen = bounds;
                    offscreen.position =
                        wry::dpi::LogicalPosition::new(position.x + size.width + 1000., position.y)
                            .into();
                    let _ = tab.view.set_bounds(offscreen);
                }
                let _ = tab.view.set_visible(visible || staging);
            }
            if visible && (active_changed || bounds_changed || tab.staged.get()) {
                let _ = tab.view.set_bounds(bounds);
            }
            tab.staged.set(staging);
            if shown {
                tab.hidden_since.set(None);
                set_memory_target(tab, false);
            } else {
                if tab.hidden_since.get().is_none() {
                    tab.hidden_since.set(Some(page_observer::now_ms()));
                }
                set_memory_target(tab, true);
            }
        }
    }

    /// Suspends pages that have been hidden and idle for `after_ms`. A suspended
    /// page keeps its login and content but releases most CPU and memory; it is
    /// resumed automatically when shown, refreshed or used by the studio.
    #[cfg(target_os = "windows")]
    pub fn sleep_idle(&self, after_ms: u64) -> bool {
        use webview2_com::{
            Microsoft::Web::WebView2::Win32::ICoreWebView2_3, TrySuspendCompletedHandler,
        };
        let now = page_observer::now_ms();
        let mut changed = false;
        for tab in self.tabs.values() {
            if tab.sleep_requested.get()
                || self.background.contains(&tab.identity)
                || self.cookie_only.contains(&tab.identity)
                || tab
                    .hidden_since
                    .get()
                    .is_none_or(|since| now.saturating_sub(since) < after_ms)
            {
                continue;
            }
            let Ok(native) = tab.view.webview().cast::<ICoreWebView2_3>() else {
                continue;
            };
            let flag = tab.sleeping.clone();
            let handler = TrySuspendCompletedHandler::create(Box::new(move |result, suspended| {
                flag.store(result.is_ok() && suspended, Ordering::Relaxed);
                Ok(())
            }));
            if unsafe { native.TrySuspend(&handler) }.is_ok() {
                tab.sleep_requested.set(true);
                changed = true;
            }
        }
        changed
    }

    /// Every page of the account is suspended.
    pub fn account_sleeping(&self, id: muse_core::AccountId) -> bool {
        let mut tabs = self
            .tabs
            .values()
            .filter(|t| t.identity.account_id == id && !self.cookie_only.contains(&t.identity))
            .peekable();
        tabs.peek().is_some() && tabs.all(|t| t.sleeping.load(Ordering::Relaxed))
    }

    pub fn sleeping_count(&self) -> usize {
        self.tabs
            .values()
            .filter(|t| t.sleeping.load(Ordering::Relaxed))
            .count()
    }

    /// Resumes every page of the account, e.g. before a studio task.
    pub fn wake_account(&self, id: muse_core::AccountId) {
        for tab in self.tabs.values().filter(|t| t.identity.account_id == id) {
            wake(tab);
        }
    }

    pub fn retain_registered(&mut self, workspace: &muse_core::Workspace) {
        self.tabs.retain(|_, tab| {
            // Unpublished migration candidates intentionally have no workspace entry yet.
            self.cookie_only.contains(&tab.identity) || workspace.contains_identity(tab.identity)
        });
        self.cookie_only.retain(|identity| {
            self.tabs
                .get(&identity.tab_id)
                .is_some_and(|tab| tab.identity == *identity)
        });
        let candidate_accounts: HashSet<_> = self
            .cookie_only
            .iter()
            .map(|identity| identity.account_id)
            .collect();
        self.bridges
            .retain(|id, _| workspace.account(*id).is_some() || candidate_accounts.contains(id));
        self.failures.retain(|identity, _| {
            workspace.contains_identity(*identity) || self.cookie_only.contains(identity)
        });
        self.observations.retain(|identity, _| {
            workspace.contains_identity(*identity) || self.cookie_only.contains(identity)
        });
        self.background.retain(|identity| {
            workspace.account(identity.account_id).is_some() || self.cookie_only.contains(identity)
        });
        self.closing
            .retain(|id, _| workspace.account(*id).is_some());
    }
}

#[cfg(target_os = "windows")]
fn install_local_auth(view: &WebView, auth: Arc<LocalAuth>) -> Result<(), String> {
    let native: ICoreWebView2_10 = view
        .webview()
        .cast()
        .map_err(|_| "WebView2 版本不支持本机代理认证")?;
    let callback = BasicAuthenticationRequestedEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            unsafe {
                let mut challenge = PWSTR::null();
                args.Challenge(&mut challenge)?;
                let challenge = take_pwstr(challenge);
                // Only random local bridge credentials can enter this callback;
                // provider credentials are held exclusively by the Rust bridge.
                if challenge == auth.challenge() {
                    let response = args.Response()?;
                    response.SetUserName(&HSTRING::from(auth.username()))?;
                    response.SetPassword(&HSTRING::from(auth.secret()))?;
                }
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { native.add_BasicAuthenticationRequested(&callback, &mut token) }
        .map_err(|_| "无法注册本机代理认证回调")?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_navigation_errors(
    view: &WebView,
    identity: BrowserIdentity,
    sender: Sender<BrowserMessage>,
    gate: Arc<Mutex<PageGate>>,
) -> Result<(), String> {
    let start_gate = gate.clone();
    let starting = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            let mut id = 0;
            unsafe {
                args.NavigationId(&mut id)?;
            }
            let mut redirected = Default::default();
            unsafe {
                args.IsRedirected(&mut redirected)?;
            }
            let mut gate = start_gate.lock().unwrap();
            gate.native_navigation_started(id);
            if redirected.as_bool() {
                gate.native_redirect();
            }
        }
        Ok(())
    }));
    let mut starting_token = 0;
    unsafe {
        view.webview()
            .add_NavigationStarting(&starting, &mut starting_token)
    }
    .map_err(|_| "无法注册网页导航状态回调")?;
    let callback = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            let mut success = Default::default();
            unsafe {
                args.IsSuccess(&mut success)?;
            }
            let mut status = Default::default();
            let mut navigation_id = 0;
            unsafe {
                args.WebErrorStatus(&mut status)?;
                args.NavigationId(&mut navigation_id)?;
            }
            let http_status = args
                .cast::<ICoreWebView2NavigationCompletedEventArgs2>()
                .ok()
                .and_then(|args| {
                    let mut code = 0;
                    unsafe { args.HttpStatusCode(&mut code) }
                        .ok()
                        .map(|()| code)
                });
            if !gate.lock().unwrap().record_navigation_completed(
                navigation_id,
                success.as_bool(),
                status.0,
                http_status,
            ) {
                return Ok(());
            }
            if !success.as_bool() {
                if status.0 == 14 {
                    return Ok(());
                }
                if !gate
                    .lock()
                    .unwrap()
                    .fail_navigation(navigation_id, status.0)
                {
                    return Ok(());
                }
                let _ = sender.send(BrowserMessage::State(
                    identity,
                    BrowserEvent::Error(navigation_error(status.0, http_status)),
                ));
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        view.webview()
            .add_NavigationCompleted(&callback, &mut token)
    }
    .map_err(|_| "无法注册网页加载状态回调")?;
    Ok(())
}

fn navigation_error(status: i32, http_status: Option<i32>) -> String {
    let response = match http_status {
        Some(code @ 100..=599) => format!("已收到 HTTP {code}，导航未完成"),
        Some(0) => "未收到 HTTP 响应".into(),
        _ => "HTTP 状态不可用".into(),
    };
    let category = match status {
        1..=5 => "TLS 证书验证失败",
        6 | 12 => "无法连接目标或代理",
        7 => "连接或响应超时",
        8 => "HTTP 响应无效",
        9 => "连接中止",
        10 => "连接被重置（来源未确定）",
        11 => "网络连接断开",
        13 => "域名解析失败",
        15 => "重定向失败",
        17 => "网站要求身份认证",
        18 => "代理认证未通过",
        _ => "WebView2 未提供明确传输原因",
    };
    format!("{response}；{category}（WebView2 {status}）。代理路由未改变；隧道测试不代表页面可用")
}
