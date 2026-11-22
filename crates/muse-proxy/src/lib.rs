//! Authenticated, account-owned loopback proxy; never falls back to direct.

use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::Bytes;
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
    sync::{Semaphore, watch},
    task::JoinSet,
    time::timeout,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, RootCertStore, pki_types::ServerName},
};
use tokio_socks::tcp::Socks5Stream;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyError {
    Invalid,
    Unsupported,
    Connect,
    Tls,
    Authentication,
    Protocol,
    Timeout,
    Stopped,
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
        })
    }
}
impl std::error::Error for ProxyError {}

#[derive(Clone)]
pub struct Upstream {
    protocol: ProxyProtocol,
    socks_auth: Option<(String, String)>,
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
        let (authorization, socks_auth) = if protocol == ProxyProtocol::Socks5 {
            (None, credentials)
        } else {
            let basic = credentials.map(|(user, secret)| {
                format!("Basic {}", STANDARD.encode(format!("{user}:{secret}")))
            });
            (basic, None)
        };
        Ok(Self {
            protocol,
            socks_auth,
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

    pub async fn probe(&self) -> Result<(), ProxyError> {
        let target = Authority::from_static("muse.ai:443");
        connect_tunnel(self, &target).await.map(|_| ())
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
    stop: watch::Sender<bool>,
    requests: Arc<AtomicU64>,
}
impl ProxyBridge {
    pub fn start(upstream: Upstream) -> Result<Self, ProxyError> {
        let auth = Arc::new(LocalAuth::new());
        let (stop, stopped) = watch::channel(false);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let shared_auth = auth.clone();
        let requests = Arc::new(AtomicU64::new(0));
        let shared_requests = requests.clone();
        runtime().spawn(async move {
            let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await {
                Ok(listener) => listener,
                Err(_) => {
                    let _ = ready_tx.send(Err(ProxyError::Connect));
                    return;
                }
            };
            let Ok(address) = listener.local_addr() else {
                let _ = ready_tx.send(Err(ProxyError::Connect));
                return;
            };
            let _ = ready_tx.send(Ok(address));
            run(
                listener,
                Arc::new(upstream),
                shared_auth,
                stopped,
                shared_requests,
            )
            .await;
        });
        let address = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| ProxyError::Timeout)??;
        Ok(Self {
            address,
            auth,
            stop,
            requests,
        })
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
}
impl Drop for ProxyBridge {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

async fn run(
    listener: TcpListener,
    upstream: Arc<Upstream>,
    auth: Arc<LocalAuth>,
    mut stopped: watch::Receiver<bool>,
    requests: Arc<AtomicU64>,
) {
    let permits = Arc::new(Semaphore::new(96));
    let tunnels = Arc::new(Semaphore::new(96));
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            result = listener.accept() => {
                let Ok((socket, peer)) = result else { break; };
                if !peer.ip().is_loopback() { continue; }
                let Ok(permit) = permits.clone().try_acquire_owned() else { continue; };
                let upstream = upstream.clone(); let auth = auth.clone(); let mut connection_stop = stopped.clone(); let tunnels = tunnels.clone(); let requests=requests.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let service_stop = connection_stop.clone();
                    let service = service_fn(move |request| handle(request, upstream.clone(), auth.clone(), service_stop.clone(), tunnels.clone(), requests.clone()));
                    let mut builder = hyper::server::conn::http1::Builder::new();
                    builder.keep_alive(false).max_buf_size(MAX_HEAD).timer(hyper_util::rt::TokioTimer::new()).header_read_timeout(DEADLINE);
                    let connection = builder.serve_connection(TokioIo::new(socket), service).with_upgrades();
                    tokio::select! { _ = connection => {}, _ = connection_stop.changed() => {} }
                });
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {},
        }
    }
    connections.abort_all();
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
    stopped: watch::Receiver<bool>,
    tunnels: Arc<Semaphore>,
    requests: Arc<AtomicU64>,
) -> Result<Response<Body>, Infallible> {
    let values: Vec<_> = request
        .headers()
        .get_all(header::PROXY_AUTHORIZATION)
        .iter()
        .collect();
    if values.len() != 1 || values[0].to_str().ok() != Some(auth.authorization.as_str()) {
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
        let (upstream, prefix) = match connect_tunnel(&upstream, authority).await {
            Ok(connection) => connection,
            Err(error) => return Ok(full(StatusCode::BAD_GATEWAY, &error.to_string())),
        };
        let upgrade = hyper::upgrade::on(&mut request);
        tokio::spawn(async move {
            let _permit = tunnel_permit;
            let mut stop = stopped;
            tokio::select! {
                _ = stop.changed() => {},
                _ = async move {
                    if let Ok(upgraded) = upgrade.await {
                        let mut client = TokioIo::new(upgraded);
                        let mut upstream = upstream;
                        if client.write_all(&prefix).await.is_ok() { let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await; }
                    }
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
    let socket = match timeout(DEADLINE, async {
        match upstream.protocol {
            ProxyProtocol::Socks5 => connect_socks(&upstream, &target, 80).await,
            _ => connect_http_proxy(&upstream).await,
        }
    })
    .await
    {
        Ok(Ok(socket)) => socket,
        Ok(Err(error)) => return Ok(full(StatusCode::BAD_GATEWAY, &error.to_string())),
        Err(_) => {
            return Ok(full(
                StatusCode::GATEWAY_TIMEOUT,
                &ProxyError::Timeout.to_string(),
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
    let mut connection_stop = stopped;
    tokio::spawn(async move {
        tokio::select! { _ = connection => {}, _ = connection_stop.changed() => {} }
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
    match timeout(DEADLINE, sender.send_request(request)).await {
        Ok(Ok(mut response)) => {
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
        Ok(Err(_)) => Ok(full(
            StatusCode::BAD_GATEWAY,
            "Upstream HTTP request failed",
        )),
        Err(_) => Ok(full(
            StatusCode::GATEWAY_TIMEOUT,
            &ProxyError::Timeout.to_string(),
        )),
    }
}

async fn connect_tunnel(
    upstream: &Upstream,
    authority: &Authority,
) -> Result<(UpstreamStream, Vec<u8>), ProxyError> {
    timeout(DEADLINE, async {
        if upstream.protocol == ProxyProtocol::Socks5 {
            return connect_socks(upstream, authority, 443)
                .await
                .map(|stream| (stream, Vec::new()));
        }
        let mut socket = connect_http_proxy(upstream).await?;
        let mut head = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
        if let Some(value) = &upstream.authorization {
            head.push_str(&format!("Proxy-Authorization: {value}\r\n"));
        }
        head.push_str("\r\n");
        socket
            .write_all(head.as_bytes())
            .await
            .map_err(|_| ProxyError::Connect)?;
        socket.flush().await.map_err(|_| ProxyError::Connect)?;
        let mut response = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            let count = socket
                .read(&mut chunk)
                .await
                .map_err(|_| ProxyError::Connect)?;
            if count == 0 {
                return Err(ProxyError::Protocol);
            }
            response.extend_from_slice(&chunk[..count]);
            while let Some(end) = response.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                if end > MAX_HEAD {
                    return Err(ProxyError::Protocol);
                }
                let text =
                    std::str::from_utf8(&response[..end]).map_err(|_| ProxyError::Protocol)?;
                let mut status = text
                    .lines()
                    .next()
                    .ok_or(ProxyError::Protocol)?
                    .split_whitespace();
                if !matches!(status.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
                    return Err(ProxyError::Protocol);
                }
                let code: u16 = status
                    .next()
                    .and_then(|c| c.parse().ok())
                    .ok_or(ProxyError::Protocol)?;
                if code == 407 {
                    return Err(ProxyError::Authentication);
                }
                if (100..200).contains(&code) {
                    response.drain(..end + 4);
                    continue;
                }
                if !(200..300).contains(&code) {
                    return Err(ProxyError::Connect);
                }
                return Ok((socket, response[end + 4..].to_vec()));
            }
            if response.len() > MAX_HEAD {
                return Err(ProxyError::Protocol);
            }
        }
    })
    .await
    .map_err(|_| ProxyError::Timeout)?
}

async fn connect_http_proxy(upstream: &Upstream) -> Result<UpstreamStream, ProxyError> {
    let socket = TcpStream::connect((upstream.host.as_str(), upstream.port))
        .await
        .map_err(|_| ProxyError::Connect)?;
    if upstream.protocol == ProxyProtocol::Http {
        return Ok(Box::new(socket));
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
    let tls = connector
        .connect(name, socket)
        .await
        .map_err(|_| ProxyError::Tls)?;
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
) -> Result<UpstreamStream, ProxyError> {
    let proxy = (upstream.host.as_str(), upstream.port);
    let target = (
        authority.host().trim_matches(['[', ']']),
        authority.port_u16().unwrap_or(default_port),
    );
    let result = match &upstream.socks_auth {
        Some((user, secret)) => {
            let secret = if secret.is_empty() {
                "x"
            } else {
                secret.as_str()
            };
            Socks5Stream::connect_with_password(proxy, target, user, secret).await
        }
        None => Socks5Stream::connect(proxy, target).await,
    };
    result
        .map(|stream| Box::new(stream.into_inner()) as UpstreamStream)
        .map_err(|error| match error {
            tokio_socks::Error::NoAcceptableAuthMethods
            | tokio_socks::Error::AuthorizationRequired
            | tokio_socks::Error::PasswordAuthFailure(_)
            | tokio_socks::Error::UnknownAuthMethod => ProxyError::Authentication,
            _ => ProxyError::Connect,
        })
}

#[cfg(test)]
mod tls_tests;
