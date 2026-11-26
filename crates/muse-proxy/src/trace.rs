//! Opt-in, typed observability only. No addresses, credentials, URLs or free text
//! can cross this interface. The application supplies a nonblocking sink.
use std::{
    collections::{HashMap, hash_map::RandomState},
    hash::{BuildHasher, Hash},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

macro_rules! tags {
    ($name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant),+ }
        impl $name { pub fn label(self) -> &'static str { match self { $(Self::$variant => $label),+ } } }
    };
}
tags!(Stage {
    SystemConfig => "system_transport_config", SystemTcp => "system_proxy_tcp",
    SystemConnect => "system_proxy_connect", SystemConnectWrite => "system_proxy_connect_write",
    SystemConnectRead => "system_proxy_connect_read", SystemConnectParse => "system_proxy_connect_parse",
    RouteSelected => "route_selected", AuthEndpointBlocked => "auth_endpoint_blocked",
    ProbeStart => "probe_start", ProbeEnd => "probe_end",
    BridgeStart => "bridge_start", BridgeEnd => "bridge_end", BridgeError => "bridge_error",
    InboundConnect => "inbound_connect", InboundHttp => "inbound_http", LocalAuth => "local_auth",
    ProviderTcp => "provider_tcp", ProviderTls => "provider_tls",
    SocksNegotiateAuth => "socks_negotiate_auth", SocksConnect => "socks_connect",
    HttpConnect => "http_connect", HttpConnectWrite => "http_connect_write",
    HttpConnectFlush => "http_connect_flush", HttpConnectRead => "http_connect_read",
    HttpConnectParse => "http_connect_parse", TunnelEstablished => "tunnel_established",
    RelayEnd => "relay_end", HttpForward => "http_forward",
    ViewCreate => "view_create", ViewError => "view_error", ViewInvalidate => "view_invalidate",
    WebViewAuth => "webview_auth", AppNavigation => "app_navigation",
    NativeStart => "native_start", NativeRedirect => "native_redirect", NativeComplete => "native_complete",
    DomReady => "dom_ready", ReadinessTimeout => "readiness_timeout", ManualAction => "manual_action"
});
tags!(Outcome {
    MatchedProxy => "matched_proxy_endpoint", RejectedAuth => "untrusted_auth_cancelled",
    Start => "start", Ok => "ok", Failed => "failed", Configuration => "configuration",
    Connect => "connect_failed", Tls => "tls_failed", Authentication => "authentication_failed",
    Protocol => "protocol_failed", Timeout => "timeout", Stopped => "stopped",
    Cancelled => "cancelled", Reset => "reset", Aborted => "aborted", Refused => "refused",
    IoUnknown => "io_failure_direction_unknown", Eof => "completed_both_directions",
    ResponseEof => "eof_before_complete_connect_response",
    Challenge => "local_challenge_required", MatchedLocal => "matched_local_bridge",
    OtherChallenge => "other_challenge_untouched", Allowed => "allowed", Blocked => "blocked",
    Stale => "stale_completion_ignored", Uncorrelated => "correlation_unavailable", Reload => "reload", Back => "back",
    Forward => "forward", Home => "home", Address => "address", Retry => "retry",
    Select => "select_account", Auto => "automatic"
});
tags!(Source { Bridge => "bridge", WebView => "webview", PoolTest => "pool_draft_test", AccountTest => "account_draft_test", CliTest => "cli_saved_route_test" });
tags!(Binding { Direct => "direct", Legacy => "legacy", Pool => "pool" });
tags!(RoutePath {
    ShadowsocksBridge => "shadowsocks_aead_bridge",
    SystemHttpBridge => "system_http_bridge", SystemHttpsBridge => "system_https_bridge", SystemSocks5Bridge => "system_socks5_bridge",
    NativeHttp => "native_http", NativeHttps => "native_https", NativeSocks5 => "native_socks5",
    Socks5AuthBridge => "socks5_auth_bridge", HttpAuthBridge => "http_auth_compatibility_bridge",
    HttpsAuthBridge => "https_auth_compatibility_bridge"
});
tags!(Transport { DirectToAccountProxy => "direct_to_account_proxy", SystemLoopback => "system_loopback_http" });
tags!(Destination { Muse => "muse", Instagram => "instagram", Other => "other" });
tags!(ParseFailure {
    HeadTooLarge => "head_too_large", InvalidUtf8 => "invalid_utf8",
    MissingStatusLine => "missing_status_line", InvalidVersion => "invalid_http_version",
    InvalidStatus => "invalid_status_code"
});
impl Destination {
    pub fn from_host(host: &str) -> Self {
        if matches!(host, "muse.ai" | "www.muse.ai") {
            Self::Muse
        } else if host == "instagram.com" || host.ends_with(".instagram.com") {
            Self::Instagram
        } else {
            Self::Other
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    pub account: u64,
    pub route: u64,
    pub bridge: u64,
    pub inbound: u64,
    pub tab: u64,
    pub navigation: u64,
    pub action: u64,
    pub generation: u64,
    pub target: u64,
    pub source: Option<Source>,
    pub binding: Option<Binding>,
    pub protocol: Option<crate::ProxyProtocol>,
    pub route_path: Option<RoutePath>,
    pub transport: Option<Transport>,
    pub proxy_required: Option<bool>,
    pub force_proxy: Option<bool>,
    pub destination: Option<Destination>,
}
#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub context: Context,
    pub stage: Stage,
    pub outcome: Outcome,
    pub duration_ms: Option<u64>,
    pub code: Option<i32>,
    pub http_status: Option<i32>,
    pub web_error: Option<i32>,
    pub redirects: Option<u32>,
    pub accepted: Option<bool>,
    pub native_success: Option<bool>,
    pub pending_stage: Option<Stage>,
    pub parse_failure: Option<ParseFailure>,
}
impl Event {
    pub fn new(context: Context, stage: Stage, outcome: Outcome) -> Self {
        Self {
            context,
            stage,
            outcome,
            duration_ms: None,
            code: None,
            http_status: None,
            web_error: None,
            redirects: None,
            accepted: None,
            native_success: None,
            pending_stage: None,
            parse_failure: None,
        }
    }
    pub fn emit(self) {
        if !enabled() {
            return;
        }
        if let Some(sink) = SINK.get() {
            // An optional diagnostic backend must never unwind into navigation.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(self)));
        }
    }
}
#[derive(Clone, Copy)]
pub struct Span {
    pub context: Context,
    pub stage: Stage,
    started: Instant,
}
impl Span {
    pub fn new(context: Context, stage: Stage) -> Self {
        Self {
            context,
            stage,
            started: Instant::now(),
        }
    }
    pub fn end(&self, outcome: Outcome, code: Option<i32>) {
        let mut event = Event::new(self.context, self.stage, outcome);
        event.duration_ms = Some(self.started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        event.code = code;
        event.emit();
    }
    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    pub fn io(&self, error: &std::io::Error) {
        self.end(io_outcome(error), error.raw_os_error());
    }
}
pub fn io_outcome(error: &std::io::Error) -> Outcome {
    match error.kind() {
        std::io::ErrorKind::ConnectionReset => Outcome::Reset,
        std::io::ErrorKind::ConnectionAborted => Outcome::Aborted,
        std::io::ErrorKind::ConnectionRefused => Outcome::Refused,
        std::io::ErrorKind::TimedOut => Outcome::Timeout,
        _ => Outcome::IoUnknown,
    }
}
impl From<crate::ProxyError> for Outcome {
    fn from(error: crate::ProxyError) -> Self {
        match error {
            crate::ProxyError::Invalid
            | crate::ProxyError::Unsupported
            | crate::ProxyError::SystemConfiguration
            | crate::ProxyError::SystemLoop
            | crate::ProxyError::SystemNonLoopback
            | crate::ProxyError::SystemDisabled => Self::Configuration,
            crate::ProxyError::Connect => Self::Connect,
            crate::ProxyError::Tls => Self::Tls,
            crate::ProxyError::Authentication => Self::Authentication,
            crate::ProxyError::Protocol => Self::Protocol,
            crate::ProxyError::Timeout => Self::Timeout,
            crate::ProxyError::Stopped => Self::Stopped,
            crate::ProxyError::SystemUnavailable => Self::Connect,
            crate::ProxyError::SystemRejected | crate::ProxyError::SystemProtocol => Self::Protocol,
            crate::ProxyError::SystemTimeout => Self::Timeout,
        }
    }
}
type Sink = Arc<dyn Fn(Event) + Send + Sync>;
static SINK: OnceLock<Sink> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
pub fn install(sink: Sink) {
    if SINK.set(sink).is_ok() {
        ENABLED.store(true, Ordering::Relaxed);
    }
}
pub fn stop() {
    ENABLED.store(false, Ordering::Relaxed);
}
pub fn new_id() -> u64 {
    if enabled() {
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    } else {
        0
    }
}

/// Process-random keyed hash is used only as an in-memory lookup key. Neither
/// the hash nor its inputs are sent to the sink or persisted.
pub fn route_id(value: impl Hash) -> u64 {
    if !enabled() {
        return 0;
    }
    static SEED: OnceLock<RandomState> = OnceLock::new();
    static ROUTES: OnceLock<Mutex<HashMap<u64, u64>>> = OnceLock::new();
    let key = SEED.get_or_init(RandomState::new).hash_one(value);
    let Ok(mut routes) = ROUTES.get_or_init(|| Mutex::new(HashMap::new())).try_lock() else {
        return 0;
    };
    if let Some(id) = routes.get(&key) {
        return *id;
    }
    if routes.len() >= 256 {
        return 0;
    }
    let id = new_id();
    routes.insert(key, id);
    id
}

/// Same endpoint in one process only; no URL, host, port or keyed hash leaves
/// this bounded lookup. Matching a target does not assign a tunnel to a tab.
pub fn target_id(host: &str, port: u16) -> u64 {
    if !enabled() {
        return 0;
    }
    let host = host.trim_matches(['[', ']']).trim_end_matches('.');
    let normalized = host
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| host.to_ascii_lowercase());
    static SEED: OnceLock<RandomState> = OnceLock::new();
    static TARGETS: OnceLock<Mutex<HashMap<u64, u64>>> = OnceLock::new();
    let key = SEED
        .get_or_init(RandomState::new)
        .hash_one((normalized, port));
    let Ok(mut targets) = TARGETS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .try_lock()
    else {
        return 0;
    };
    if let Some(id) = targets.get(&key) {
        return *id;
    }
    if targets.len() >= 1024 {
        return 0;
    }
    let id = new_id();
    targets.insert(key, id);
    id
}
