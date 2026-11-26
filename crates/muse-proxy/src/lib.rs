//! Authenticated, account-owned loopback proxy; never falls back to direct.

mod connect_response;
pub mod trace;
mod transport;
use trace::{
    Context as TraceContext, Event as TraceEvent, Outcome as TraceOutcome, Source as TraceSource,
    Span as TraceSpan, Stage as TraceStage,
};
pub use transport::{
    configure_system_transport, parse_system_proxy, system_transport_enabled,
    system_transport_status, validate_system_upstream,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::Bytes;
use fast_socks5::{
    AuthenticationMethod, Socks5Command,
    client::{Config as SocksConfig, Socks5Stream},
    util::target_addr::ToTargetAddr,
};
use http_body_util::{BodyExt, Full, combinators::UnsyncBoxBody};
use hyper::{
    Request, Response, StatusCode,
    body::Incoming,
    header::{self, HeaderMap, HeaderValue},
    http::uri::Authority,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use percent_encoding::percent_decode_str;
use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::timeout,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, RootCertStore, pki_types::ServerName},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use uuid::Uuid;

const DEADLINE: Duration = Duration::from_secs(15);
const MAX_HEAD: usize = 32 * 1024;
static HTTPS_CONNECTOR: OnceLock<Option<TlsConnector>> = OnceLock::new();
type Body = UnsyncBoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;

trait UpstreamIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<Stream: AsyncRead + AsyncWrite + Unpin + Send> UpstreamIo for Stream {}
type UpstreamStream = Box<dyn UpstreamIo>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProxyProtocol {
    #[default]
    Http,
    Https,
    Socks5,
}

impl ProxyProtocol {
    pub const ALL: [Self; 3] = [Self::Http, Self::Https, Self::Socks5];

    pub fn scheme(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Socks5 => "socks5",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Https => "HTTPS",
            Self::Socks5 => "SOCKS5",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
            Self::Socks5 => 1080,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProxyError {
    Invalid,
    Unsupported,
    Connect,
    Tls,
    Authentication,
    Protocol,
    Timeout,
    Stopped,
    SystemConfiguration,
    SystemNonLoopback,
    SystemDisabled,
    SystemUnavailable,
    SystemRejected,
    SystemProtocol,
    SystemLoop,
    SystemTimeout,
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "代理配置格式无效",
            Self::Unsupported => "代理协议仅支持 HTTP、HTTPS 或 SOCKS5",
            Self::Connect => "无法连接上游代理；未使用直连",
            Self::Tls => "HTTPS 代理的 TLS 连接或证书验证失败；未使用直连",
            Self::Authentication => "上游代理认证失败；未使用直连",
            Self::Protocol => "代理响应无效",
            Self::Timeout => "代理连接超时；未使用直连",
            Self::Stopped => "代理已停止",
            Self::SystemConfiguration => {
                "系统前置代理配置不可用：仅支持已启用的静态单回环 HTTP 端点；未切换出口"
            }
            Self::SystemUnavailable => "无法连接本机系统前置代理；未切换出口或直连",
            Self::SystemNonLoopback => "系统前置代理为非回环地址，已拒绝连接；未切换出口",
            Self::SystemDisabled => {
                "Windows 未启用静态 HTTP 系统代理，已阻止账号代理连接；未切换出口"
            }
            Self::SystemRejected => "本机系统前置代理拒绝 CONNECT；未发送账号代理认证，未切换出口",
            Self::SystemProtocol => "本机系统前置代理的 CONNECT 响应无效或超出限制；未切换出口",
            Self::SystemLoop => "账号代理与系统前置代理形成回环；已阻止连接",
            Self::SystemTimeout => "本机系统前置代理连接超时；未切换出口或直连",
        })
    }
}
impl std::error::Error for ProxyError {}

#[derive(Clone)]
pub struct Upstream {
    protocol: ProxyProtocol,
    credentials: Option<(String, String)>,
    host: String,
    port: u16,
    authorization: Option<String>,
}

impl std::fmt::Debug for Upstream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Upstream")
            .field("credentials", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl Upstream {
    pub fn parse(input: &str) -> Result<Self, ProxyError> {
        if input.len() > 16384 {
            return Err(ProxyError::Invalid);
        }
        let normalized = if input.contains("://") {
            input.to_owned()
        } else {
            format!("http://{input}")
        };
        let url = url::Url::parse(&normalized).map_err(|_| ProxyError::Invalid)?;
        let protocol = match url.scheme() {
            "http" => ProxyProtocol::Http,
            "https" => ProxyProtocol::Https,
            "socks" | "socks5" | "socks5h" => ProxyProtocol::Socks5,
            _ => return Err(ProxyError::Unsupported),
        };
        if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
            return Err(ProxyError::Invalid);
        }
        let raw_host = url.host_str().ok_or(ProxyError::Invalid)?;
        let host = url::Host::parse(raw_host)
            .map_err(|_| ProxyError::Invalid)?
            .to_string()
            .trim_matches(['[', ']'])
            .to_owned();
        let port = url.port().unwrap_or_else(|| protocol.default_port());
        if host.is_empty() || port == 0 {
            return Err(ProxyError::Invalid);
        }
        let raw = &url[url::Position::BeforeUsername..url::Position::BeforeHost];
        let credentials = if raw.is_empty() {
            None
        } else {
            let credentials = raw.strip_suffix('@').ok_or(ProxyError::Invalid)?;
            let (user, secret) = credentials.split_once(':').unwrap_or((credentials, ""));
            fn decode(input: &str) -> Result<String, ProxyError> {
                let bytes = input.as_bytes();
                for index in 0..bytes.len() {
                    if bytes[index] == b'%'
                        && (index + 2 >= bytes.len()
                            || !bytes[index + 1].is_ascii_hexdigit()
                            || !bytes[index + 2].is_ascii_hexdigit())
                    {
                        return Err(ProxyError::Invalid);
                    }
                }
                let decoded = percent_decode_str(input)
                    .decode_utf8()
                    .map_err(|_| ProxyError::Invalid)?;
                if decoded.chars().any(char::is_control) {
                    return Err(ProxyError::Invalid);
                }
                Ok(decoded.into_owned())
            }
            let user = decode(user)?;
            let secret = decode(secret)?;
            if user.is_empty()
                || (protocol != ProxyProtocol::Socks5 && user.contains(':'))
                || (protocol == ProxyProtocol::Socks5 && (user.len() > 255 || secret.len() > 255))
            {
                return Err(ProxyError::Invalid);
            }
            Some((user, secret))
        };
        let authorization = if protocol == ProxyProtocol::Socks5 {
            None
        } else {
            credentials.as_ref().map(|(user, secret)| {
                format!("Basic {}", STANDARD.encode(format!("{user}:{secret}")))
            })
        };
        Ok(Self {
            protocol,
            credentials,
            host,
            port,
            authorization,
        })
    }

    pub fn protocol(&self) -> ProxyProtocol {
        self.protocol
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Memory-only credentials for a browser adapter that verifies proxy scope.
    /// Never put these in browser arguments, URIs or diagnostic output.
    pub fn credentials(&self) -> Option<(&str, &str)> {
        self.credentials
            .as_ref()
            .map(|(user, secret)| (user.as_str(), secret.as_str()))
    }

    pub async fn probe(&self) -> Result<(), ProxyError> {
        self.probe_traced(TraceContext {
            source: Some(TraceSource::CliTest),
            ..TraceContext::default()
        })
        .await
    }

    pub fn trace_route_id(&self) -> u64 {
        trace::route_id((
            self.protocol as u8,
            &self.host,
            self.port,
            &self.authorization,
            &self.credentials,
            transport::identity(),
        ))
    }

    pub async fn probe_traced(&self, mut context: TraceContext) -> Result<(), ProxyError> {
        context.route = self.trace_route_id();
        if context.action == 0 {
            context.action = trace::new_id();
        }
        context.protocol = Some(self.protocol);
        context.transport = Some(transport::trace_kind());
        if system_transport_enabled() {
            context.route_path = Some(transport::route_path(self.protocol));
        }
        context.destination = Some(trace::Destination::Muse);
        context.target = trace::target_id("muse.ai", 443);
        TraceEvent::new(context, TraceStage::ProbeStart, TraceOutcome::Start).emit();
        let span = TraceSpan::new(context, TraceStage::ProbeEnd);
        let target = Authority::from_static("muse.ai:443");
        let result = connect_tunnel(self, &target, context).await.map(|_| ());
        span.end(
            result
                .as_ref()
                .map_or_else(|e| (*e).into(), |_| TraceOutcome::Ok),
            None,
        );
        result
    }
}

pub struct LocalAuth {
    username: String,
    secret: String,
    challenge: String,
    authorization: String,
}
impl LocalAuth {
    fn new() -> Self {
        let username = Uuid::new_v4().simple().to_string();
        let secret = Uuid::new_v4().simple().to_string();
        let challenge = format!("Basic realm=\"MuseLocal-{}\"", Uuid::new_v4().simple());
        let authorization = format!("Basic {}", STANDARD.encode(format!("{username}:{secret}")));
        Self {
            username,
            secret,
            challenge,
            authorization,
        }
    }
    pub fn authorization(&self) -> String {
        self.authorization.clone()
    }
    pub fn challenge(&self) -> &str {
        &self.challenge
    }
    pub fn username(&self) -> &str {
        &self.username
    }
    pub fn secret(&self) -> &str {
        &self.secret
    }
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("muse-proxy")
            .build()
            .expect("proxy runtime")
    })
}

pub struct ProxyBridge {
    address: SocketAddr,
    auth: Arc<LocalAuth>,
    cancel: CancellationToken,
    tasks: TaskTracker,
    requests: Arc<AtomicU64>,
    trace_context: TraceContext,
}
impl ProxyBridge {
    pub fn start(upstream: Upstream) -> Result<Self, ProxyError> {
        Self::start_traced(upstream, TraceContext::default())
    }
    pub fn start_traced(upstream: Upstream, mut context: TraceContext) -> Result<Self, ProxyError> {
        validate_system_upstream(&upstream)?;
        context.route = upstream.trace_route_id();
        context.bridge = trace::new_id();
        context.protocol = Some(upstream.protocol);
        context.transport = Some(transport::trace_kind());
        if system_transport_enabled() {
            context.route_path = Some(transport::route_path(upstream.protocol));
        }
        context.source = Some(TraceSource::Bridge);
        TraceEvent::new(context, TraceStage::BridgeStart, TraceOutcome::Start).emit();
        let auth = Arc::new(LocalAuth::new());
        let cancel = CancellationToken::new();
        let tasks = TaskTracker::new();
        let run_cancel = cancel.clone();
        let run_tasks = tasks.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let shared_auth = auth.clone();
        let requests = Arc::new(AtomicU64::new(0));
        let shared_requests = requests.clone();
        runtime().spawn(async move {
            // Cancellation and closing the task set also run on bind failure or
            // unwinding, so a failed listener cannot leave child tasks alive.
            let _lifetime = BridgeLifetime {
                cancel: run_cancel.clone(),
                tasks: run_tasks.clone(),
                trace_context: context,
            };
            let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await {
                Ok(listener) => listener,
                Err(error) => {
                    TraceSpan::new(context, TraceStage::BridgeError).io(&error);
                    let _ = ready_tx.send(Err(ProxyError::Connect));
                    return;
                }
            };
            let Ok(address) = listener.local_addr() else {
                let _ = ready_tx.send(Err(ProxyError::Connect));
                return;
            };
            if ready_tx.send(Ok(address)).is_err() {
                return;
            }
            run(
                listener,
                Arc::new(upstream),
                shared_auth,
                run_cancel,
                run_tasks,
                shared_requests,
                context,
            )
            .await;
        });
        let address = match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(address)) => address,
            result => {
                cancel.cancel();
                let error = result
                    .ok()
                    .and_then(Result::err)
                    .unwrap_or(ProxyError::Timeout);
                TraceEvent::new(context, TraceStage::BridgeError, error.into()).emit();
                return Err(error);
            }
        };
        TraceEvent::new(context, TraceStage::BridgeStart, TraceOutcome::Ok).emit();
        Ok(Self {
            address,
            auth,
            cancel,
            tasks,
            requests,
            trace_context: context,
        })
    }
    pub fn trace_context(&self) -> TraceContext {
        self.trace_context
    }
    pub fn request_count(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn auth(&self) -> &LocalAuth {
        &self.auth
    }
    pub fn shared_auth(&self) -> Arc<LocalAuth> {
        self.auth.clone()
    }
    pub fn is_running(&self) -> bool {
        !self.cancel.is_cancelled()
    }
    /// Stop this account's listener and await all HTTP/dial/upgrade/tunnel tasks.
    /// Call from an async worker; dropping the last owner requests the same stop.
    pub async fn shutdown(&self) -> Result<(), ProxyError> {
        self.cancel.cancel();
        timeout(Duration::from_secs(5), self.tasks.wait())
            .await
            .map_err(|_| ProxyError::Timeout)
    }
}
impl Drop for ProxyBridge {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

struct BridgeLifetime {
    cancel: CancellationToken,
    tasks: TaskTracker,
    trace_context: TraceContext,
}
impl Drop for BridgeLifetime {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.tasks.close();
        TraceEvent::new(
            self.trace_context,
            TraceStage::BridgeEnd,
            TraceOutcome::Stopped,
        )
        .emit();
    }
}

async fn run(
    listener: TcpListener,
    upstream: Arc<Upstream>,
    auth: Arc<LocalAuth>,
    cancel: CancellationToken,
    tasks: TaskTracker,
    requests: Arc<AtomicU64>,
    context: TraceContext,
) {
    let permits = Arc::new(Semaphore::new(96));
    let tunnels = Arc::new(Semaphore::new(96));
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            result = listener.accept() => {
                let (socket, peer) = match result {Ok(pair)=>pair,Err(error)=>{TraceSpan::new(context,TraceStage::BridgeError).io(&error);break;}};
                if !peer.ip().is_loopback() { continue; }
                let Ok(permit) = permits.clone().try_acquire_owned() else { continue; };
                let upstream = upstream.clone(); let auth = auth.clone(); let connection_stop = cancel.clone(); let tunnels = tunnels.clone(); let requests=requests.clone(); let child_tasks = tasks.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let service_stop = connection_stop.clone();
                    let service = service_fn(move |request| handle(request, upstream.clone(), auth.clone(), service_stop.clone(), child_tasks.clone(), tunnels.clone(), requests.clone(),context));
                    let mut builder = hyper::server::conn::http1::Builder::new();
                    builder.keep_alive(false).half_close(true).max_buf_size(MAX_HEAD).timer(hyper_util::rt::TokioTimer::new()).header_read_timeout(DEADLINE);
                    let connection = builder.serve_connection(TokioIo::new(socket), service).with_upgrades();
                    tokio::select! { biased; _ = connection_stop.cancelled() => {}, _ = connection => {} }
                });
            },
        }
    }
    cancel.cancel();
    drop(listener);
    tasks.close();
    tasks.wait().await;
}

fn full(status: StatusCode, text: &str) -> Response<Body> {
    let body = Full::new(Bytes::copy_from_slice(text.as_bytes()))
        .map_err(|never| match never {})
        .boxed_unsync();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CONNECTION, "close")
        .body(body)
        .expect("static response")
}

fn strip_hop(headers: &mut HeaderMap) {
    let extra: Vec<String> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|value| value.split(',').map(|token| token.trim().to_owned()))
        .collect();
    for name in extra {
        headers.remove(name.as_str());
    }
    for name in [
        "connection",
        "proxy-connection",
        "keep-alive",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "proxy-authorization",
        "proxy-authenticate",
    ] {
        headers.remove(name);
    }
}

async fn handle(
    mut request: Request<Incoming>,
    upstream: Arc<Upstream>,
    auth: Arc<LocalAuth>,
    cancel: CancellationToken,
    tasks: TaskTracker,
    tunnels: Arc<Semaphore>,
    requests: Arc<AtomicU64>,
    mut context: TraceContext,
) -> Result<Response<Body>, Infallible> {
    context.inbound = trace::new_id();
    context.destination = request
        .uri()
        .authority()
        .map(|a| trace::Destination::from_host(a.host()));
    context.target = request.uri().authority().map_or(0, |a| {
        trace::target_id(a.host(), a.port_u16().unwrap_or(80))
    });
    let values: Vec<_> = request
        .headers()
        .get_all(header::PROXY_AUTHORIZATION)
        .iter()
        .collect();
    if values.len() != 1 || values[0].to_str().ok() != Some(auth.authorization.as_str()) {
        TraceEvent::new(context, TraceStage::LocalAuth, TraceOutcome::Challenge).emit();
        let mut response = full(
            StatusCode::PROXY_AUTHENTICATION_REQUIRED,
            "Local proxy authentication required",
        );
        response.headers_mut().insert(
            header::PROXY_AUTHENTICATE,
            HeaderValue::from_str(auth.challenge()).expect("generated challenge"),
        );
        return Ok(response);
    }
    requests.fetch_add(1, Ordering::Relaxed);
    TraceEvent::new(
        context,
        if request.method() == hyper::Method::CONNECT {
            TraceStage::InboundConnect
        } else {
            TraceStage::InboundHttp
        },
        TraceOutcome::Start,
    )
    .emit();
    if request.method() == hyper::Method::CONNECT {
        let Ok(tunnel_permit) = tunnels.try_acquire_owned() else {
            return Ok(full(
                StatusCode::SERVICE_UNAVAILABLE,
                "Proxy tunnel capacity reached",
            ));
        };
        let Some(authority) = request.uri().authority() else {
            return Ok(full(StatusCode::BAD_REQUEST, "Invalid CONNECT target"));
        };
        if authority.as_str().contains('@')
            || authority.host().is_empty()
            || authority.port_u16().is_none_or(|port| port == 0)
        {
            return Ok(full(StatusCode::BAD_REQUEST, "Invalid CONNECT target"));
        }
        let (upstream, prefix) = match connect_tunnel(&upstream, authority, context).await {
            Ok(connection) => connection,
            Err(error) => return Ok(full(StatusCode::BAD_GATEWAY, &error.to_string())),
        };
        let upgrade = hyper::upgrade::on(&mut request);
        tasks.spawn(async move {
            let _permit = tunnel_permit;
            let relay=TraceSpan::new(context,TraceStage::RelayEnd);
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {relay.end(TraceOutcome::Cancelled,None);},
                _ = async move {
                    match upgrade.await { Ok(upgraded) => {
                        let mut client = TokioIo::new(upgraded);
                        let mut upstream = upstream;
                        match client.write_all(&prefix).await {
                            Ok(())=>match tokio::io::copy_bidirectional(&mut client, &mut upstream).await {Ok(_)=>relay.end(TraceOutcome::Eof,None),Err(error)=>relay.io(&error)},
                            Err(error)=>relay.io(&error)
                        }
                    },Err(_)=>relay.end(TraceOutcome::Protocol,None)}
                } => {}
            }
        });
        return Ok(full(StatusCode::OK, ""));
    }
    let Some(target) = request
        .uri()
        .authority()
        .filter(|authority| {
            request.uri().scheme_str() == Some("http")
                && !authority.as_str().contains('@')
                && !authority.host().is_empty()
                && (authority.port().is_none() || authority.port_u16().is_some_and(|port| port > 0))
        })
        .cloned()
    else {
        return Ok(full(
            StatusCode::BAD_REQUEST,
            "Only absolute HTTP requests or CONNECT are supported",
        ));
    };
    let mut pending = TraceStage::ProviderTcp;
    let socket = match timeout(DEADLINE, async {
        match upstream.protocol {
            ProxyProtocol::Socks5 => {
                connect_socks(&upstream, &target, 80, context, &mut pending).await
            }
            _ => connect_http_proxy(&upstream, context, &mut pending).await,
        }
    })
    .await
    {
        Ok(Ok(socket)) => socket,
        Ok(Err(error)) => {
            TraceEvent::new(context, TraceStage::HttpForward, error.into()).emit();
            return Ok(full(StatusCode::BAD_GATEWAY, &error.to_string()));
        }
        Err(_) => {
            let mut event =
                TraceEvent::new(context, TraceStage::HttpForward, TraceOutcome::Timeout);
            event.pending_stage = Some(pending);
            event.emit();
            return Ok(full(
                StatusCode::GATEWAY_TIMEOUT,
                &transport::timeout_error(pending).to_string(),
            ));
        }
    };
    let (mut sender, connection) =
        match hyper::client::conn::http1::handshake(TokioIo::new(socket)).await {
            Ok(result) => result,
            Err(_) => {
                return Ok(full(
                    StatusCode::BAD_GATEWAY,
                    "Upstream HTTP connection failed",
                ));
            }
        };
    tasks.spawn(async move {
        tokio::select! { biased; _ = cancel.cancelled() => {}, _ = connection => {} }
    });
    strip_hop(request.headers_mut());
    if upstream.protocol == ProxyProtocol::Socks5 {
        let path = request
            .uri()
            .path_and_query()
            .cloned()
            .unwrap_or_else(|| hyper::http::uri::PathAndQuery::from_static("/"));
        *request.uri_mut() = path.into();
        request.headers_mut().insert(
            header::HOST,
            HeaderValue::from_str(target.as_str()).expect("validated authority"),
        );
    }
    if let Some(value) = &upstream.authorization {
        request.headers_mut().insert(
            header::PROXY_AUTHORIZATION,
            HeaderValue::from_str(value).expect("encoded authorization"),
        );
    }
    let forward = TraceSpan::new(context, TraceStage::HttpForward);
    match timeout(DEADLINE, sender.send_request(request)).await {
        Ok(Ok(mut response)) => {
            let mut event = TraceEvent::new(context, TraceStage::HttpForward, TraceOutcome::Ok);
            event.duration_ms = Some(forward.elapsed_ms());
            event.http_status = Some(response.status().as_u16() as i32);
            event.emit();
            if upstream.protocol != ProxyProtocol::Socks5
                && response.status() == StatusCode::PROXY_AUTHENTICATION_REQUIRED
            {
                return Ok(full(
                    StatusCode::BAD_GATEWAY,
                    &ProxyError::Authentication.to_string(),
                ));
            }
            strip_hop(response.headers_mut());
            Ok(response.map(|body| {
                body.map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
                    .boxed_unsync()
            }))
        }
        Ok(Err(_)) => {
            forward.end(TraceOutcome::IoUnknown, None);
            Ok(full(
                StatusCode::BAD_GATEWAY,
                "Upstream HTTP request failed",
            ))
        }
        Err(_) => {
            forward.end(TraceOutcome::Timeout, None);
            Ok(full(
                StatusCode::GATEWAY_TIMEOUT,
                &ProxyError::Timeout.to_string(),
            ))
        }
    }
}

async fn connect_tunnel(
    upstream: &Upstream,
    authority: &Authority,
    context: TraceContext,
) -> Result<(UpstreamStream, Vec<u8>), ProxyError> {
    let tunnel = TraceSpan::new(context, TraceStage::TunnelEstablished);
    let mut pending = TraceStage::ProviderTcp;
    let result = timeout(DEADLINE, async {
        if upstream.protocol == ProxyProtocol::Socks5 {
            return connect_socks(upstream, authority, 443, context, &mut pending)
                .await
                .map(|stream| (stream, Vec::new()));
        }
        let mut socket = connect_http_proxy(upstream, context, &mut pending).await?;
        let connect = TraceSpan::new(context, TraceStage::HttpConnect);
        TraceEvent::new(context, TraceStage::HttpConnect, TraceOutcome::Start).emit();
        let mut head = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
        if let Some(value) = &upstream.authorization {
            head.push_str(&format!("Proxy-Authorization: {value}\r\n"));
        }
        head.push_str("\r\n");
        pending = TraceStage::HttpConnectWrite;
        let write = TraceSpan::new(context, pending);
        TraceEvent::new(context, pending, TraceOutcome::Start).emit();
        socket.write_all(head.as_bytes()).await.map_err(|error| {
            write.io(&error);
            ProxyError::Connect
        })?;
        write.end(TraceOutcome::Ok, None);
        pending = TraceStage::HttpConnectFlush;
        let flush = TraceSpan::new(context, pending);
        TraceEvent::new(context, pending, TraceOutcome::Start).emit();
        socket.flush().await.map_err(|error| {
            flush.io(&error);
            ProxyError::Connect
        })?;
        flush.end(TraceOutcome::Ok, None);
        pending = TraceStage::HttpConnectRead;
        let prefix = connect_response::read(&mut socket, context, false, &connect).await?;
        Ok((socket, prefix))
    })
    .await
    .map_err(|_| {
        let mut event = TraceEvent::new(context, pending, TraceOutcome::Timeout);
        event.pending_stage = Some(pending);
        event.duration_ms = Some(tunnel.elapsed_ms());
        event.emit();
        transport::timeout_error(pending)
    })
    .and_then(|result| result);
    tunnel.end(
        result
            .as_ref()
            .map_or_else(|e| (*e).into(), |_| TraceOutcome::Ok),
        None,
    );
    result
}

async fn connect_http_proxy(
    upstream: &Upstream,
    context: TraceContext,
    pending: &mut TraceStage,
) -> Result<UpstreamStream, ProxyError> {
    let socket = transport::dial(upstream, context, pending).await?;
    if upstream.protocol == ProxyProtocol::Http {
        return Ok(socket);
    }
    let name = ServerName::try_from(upstream.host.clone()).map_err(|_| ProxyError::Invalid)?;
    let connector = HTTPS_CONNECTOR
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
            (!roots.is_empty()).then(|| tls_connector(roots))
        })
        .as_ref()
        .ok_or(ProxyError::Tls)?;
    *pending = TraceStage::ProviderTls;
    let tls_trace = TraceSpan::new(context, TraceStage::ProviderTls);
    TraceEvent::new(context, TraceStage::ProviderTls, TraceOutcome::Start).emit();
    let tls = connector.connect(name, socket).await.map_err(|_| {
        tls_trace.end(TraceOutcome::Tls, None);
        ProxyError::Tls
    })?;
    tls_trace.end(TraceOutcome::Ok, None);
    Ok(Box::new(tls))
}

fn tls_connector(roots: RootCertStore) -> TlsConnector {
    TlsConnector::from(Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ))
}

async fn connect_socks(
    upstream: &Upstream,
    authority: &Authority,
    default_port: u16,
    context: TraceContext,
    pending: &mut TraceStage,
) -> Result<UpstreamStream, ProxyError> {
    // Resolve/connect the proxy with Tokio, rather than the client's synchronous
    // ToSocketAddrs helper. Target domains are sent to SOCKS without local DNS.
    let socket = transport::dial(upstream, context, pending).await?;
    let target = (
        authority.host().trim_matches(['[', ']']),
        authority.port_u16().unwrap_or(default_port),
    )
        .to_target_addr()
        .map_err(|_| ProxyError::Invalid)?;
    let auth = match &upstream.credentials {
        Some((user, secret)) => {
            let secret = if secret.is_empty() {
                "x"
            } else {
                secret.as_str()
            };
            Some(AuthenticationMethod::Password {
                username: user.clone(),
                password: secret.to_owned(),
            })
        }
        None => None,
    };
    *pending = TraceStage::SocksNegotiateAuth;
    let negotiate = TraceSpan::new(context, TraceStage::SocksNegotiateAuth);
    TraceEvent::new(context, TraceStage::SocksNegotiateAuth, TraceOutcome::Start).emit();
    let mut stream = Socks5Stream::use_stream(socket, auth, SocksConfig::default())
        .await
        .map_err(|error| traced_socks_error(error, &negotiate))?;
    negotiate.end(TraceOutcome::Ok, None);
    *pending = TraceStage::SocksConnect;
    let connect = TraceSpan::new(context, TraceStage::SocksConnect);
    TraceEvent::new(context, TraceStage::SocksConnect, TraceOutcome::Start).emit();
    stream
        .request(Socks5Command::TCPConnect, target)
        .await
        .map_err(|error| traced_socks_error(error, &connect))?;
    connect.end(TraceOutcome::Ok, None);
    Ok(Box::new(stream))
}

fn traced_socks_error(error: fast_socks5::SocksError, span: &TraceSpan) -> ProxyError {
    let code = if let fast_socks5::SocksError::Io(io) = &error {
        io.raw_os_error()
    } else {
        None
    };
    let io_outcome = if let fast_socks5::SocksError::Io(io) = &error {
        Some(trace::io_outcome(io))
    } else {
        None
    };
    let category = socks_error(error);
    span.end(io_outcome.unwrap_or_else(|| category.into()), code);
    category
}

fn socks_error(error: fast_socks5::SocksError) -> ProxyError {
    // Library errors can include a username. Never expose their Display/Debug.
    match error {
        fast_socks5::SocksError::AuthMethodUnacceptable(_)
        | fast_socks5::SocksError::AuthenticationRejected(_) => ProxyError::Authentication,
        fast_socks5::SocksError::ReplyError(fast_socks5::ReplyError::ConnectionTimeout) => {
            ProxyError::Timeout
        }
        _ => ProxyError::Connect,
    }
}

#[cfg(test)]
mod tls_tests;
