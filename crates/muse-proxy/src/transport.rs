//! Optional, immutable launch-only TCP transport. Never selects another exit.
use super::*;
use std::{
    net::{IpAddr, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::ReadBuf;

static SYSTEM: OnceLock<Result<SocketAddr, ProxyError>> = OnceLock::new();

pub fn parse_system_proxy(value: &str) -> Result<SocketAddr, ProxyError> {
    if value.len() > 2048
        || value.trim() != value
        || value.chars().any(char::is_whitespace)
        || value.contains(';')
    {
        return Err(ProxyError::SystemConfiguration);
    }
    let value = value.strip_prefix("http=").unwrap_or(value);
    if value.contains('=') {
        return Err(ProxyError::SystemConfiguration);
    }
    let url = url::Url::parse(&if value.contains("://") {
        value.to_owned()
    } else {
        format!("http://{value}")
    })
    .map_err(|_| ProxyError::SystemConfiguration)?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ProxyError::SystemConfiguration);
    }
    let ip = url
        .host_str()
        .and_then(|s| s.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        .map(|v| v.to_canonical())
        .ok_or(ProxyError::SystemConfiguration)?;
    if !ip.is_loopback() {
        return Err(ProxyError::SystemNonLoopback);
    }
    let port = url
        .port_or_known_default()
        .filter(|p| *p != 0)
        .ok_or(ProxyError::SystemConfiguration)?;
    Ok(SocketAddr::new(ip, port))
}
pub fn configure_system_transport(
    config: Result<SocketAddr, ProxyError>,
) -> Result<(), ProxyError> {
    let config = config.and_then(|a| {
        if a.ip().to_canonical().is_loopback() && a.port() != 0 {
            Ok(a)
        } else {
            Err(ProxyError::SystemConfiguration)
        }
    });
    SYSTEM
        .set(config)
        .map_err(|_| ProxyError::SystemConfiguration)?;
    let result = config.map(|_| ());
    TraceEvent::new(
        TraceContext {
            transport: Some(trace::Transport::SystemLoopback),
            ..Default::default()
        },
        TraceStage::SystemConfig,
        if result.is_ok() {
            TraceOutcome::Ok
        } else {
            TraceOutcome::Configuration
        },
    )
    .emit();
    result
}
pub fn system_transport_enabled() -> bool {
    SYSTEM.get().is_some()
}
pub fn system_transport_status() -> Result<bool, ProxyError> {
    SYSTEM.get().copied().transpose().map(|c| c.is_some())
}
pub fn validate_system_upstream(upstream: &Upstream) -> Result<bool, ProxyError> {
    if upstream.fixed_node {
        return Ok(false);
    }
    let system = SYSTEM.get().copied().transpose()?;
    if let Some(address) = system {
        if upstream.port == address.port()
            && upstream
                .host
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.to_canonical().is_loopback())
        {
            TraceEvent::new(
                TraceContext {
                    route: upstream.trace_route_id(),
                    protocol: Some(upstream.protocol),
                    transport: Some(trace::Transport::SystemLoopback),
                    ..Default::default()
                },
                TraceStage::SystemConfig,
                TraceOutcome::Blocked,
            )
            .emit();
            return Err(ProxyError::SystemLoop);
        }
    }
    Ok(system.is_some())
}
pub(super) fn identity() -> Option<Result<SocketAddr, ProxyError>> {
    SYSTEM.get().copied()
}
pub(super) fn trace_kind(upstream: &Upstream) -> trace::Transport {
    if !upstream.fixed_node && system_transport_enabled() {
        trace::Transport::SystemLoopback
    } else {
        trace::Transport::DirectToAccountProxy
    }
}
pub(super) fn route_path(protocol: ProxyProtocol) -> trace::RoutePath {
    match protocol {
        ProxyProtocol::Http => trace::RoutePath::SystemHttpBridge,
        ProxyProtocol::Https => trace::RoutePath::SystemHttpsBridge,
        ProxyProtocol::Socks5 => trace::RoutePath::SystemSocks5Bridge,
        ProxyProtocol::Shadowsocks => trace::RoutePath::ShadowsocksBridge,
    }
}
pub(super) fn timeout_error(pending: TraceStage) -> ProxyError {
    if matches!(
        pending,
        TraceStage::SystemConfig
            | TraceStage::SystemTcp
            | TraceStage::SystemConnect
            | TraceStage::SystemConnectWrite
            | TraceStage::SystemConnectRead
            | TraceStage::SystemConnectParse
    ) {
        ProxyError::SystemTimeout
    } else {
        ProxyError::Timeout
    }
}

pub(super) async fn dial(
    upstream: &Upstream,
    mut context: TraceContext,
    pending: &mut TraceStage,
) -> Result<UpstreamStream, ProxyError> {
    context.transport = Some(trace_kind(upstream));
    let system = if upstream.fixed_node {
        None
    } else {
        SYSTEM.get().copied().transpose()?
    };
    let Some(system) = system else {
        *pending = TraceStage::ProviderTcp;
        let span = TraceSpan::new(context, *pending);
        TraceEvent::new(context, *pending, TraceOutcome::Start).emit();
        let socket = TcpStream::connect((upstream.host.as_str(), upstream.port))
            .await
            .map_err(|error| {
                span.io(&error);
                ProxyError::Connect
            })?;
        span.end(TraceOutcome::Ok, None);
        return Ok(Box::new(socket));
    };
    context.route_path = Some(route_path(upstream.protocol));
    *pending = TraceStage::SystemConfig;
    if upstream.port == system.port() {
        let loopback = if let Ok(ip) = upstream.host.parse::<IpAddr>() {
            ip.to_canonical().is_loopback()
        } else {
            tokio::net::lookup_host((upstream.host.as_str(), upstream.port))
                .await
                .map_err(|_| ProxyError::SystemConfiguration)?
                .any(|a| a.ip().to_canonical().is_loopback())
        };
        if loopback {
            TraceEvent::new(context, *pending, TraceOutcome::Blocked).emit();
            return Err(ProxyError::SystemLoop);
        }
    }
    *pending = TraceStage::SystemTcp;
    let tcp = TraceSpan::new(context, *pending);
    TraceEvent::new(context, *pending, TraceOutcome::Start).emit();
    let socket = TcpStream::connect(system).await.map_err(|error| {
        tcp.io(&error);
        ProxyError::SystemUnavailable
    })?;
    tcp.end(TraceOutcome::Ok, None);
    let mut socket: UpstreamStream = Box::new(socket);
    let host = if upstream.host.contains(':') {
        format!("[{}]", upstream.host)
    } else {
        upstream.host.clone()
    };
    let authority = format!("{host}:{}", upstream.port);
    let connect = TraceSpan::new(context, TraceStage::SystemConnect);
    TraceEvent::new(context, TraceStage::SystemConnect, TraceOutcome::Start).emit();
    *pending = TraceStage::SystemConnectWrite;
    let write = TraceSpan::new(context, *pending);
    TraceEvent::new(context, *pending, TraceOutcome::Start).emit();
    // The only destination is the configured account proxy; never its website.
    // Deliberately no account credentials, cookies or automatic proxy auth here.
    socket
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await
        .map_err(|e| {
            write.io(&e);
            ProxyError::SystemUnavailable
        })?;
    socket.flush().await.map_err(|e| {
        write.io(&e);
        ProxyError::SystemUnavailable
    })?;
    write.end(TraceOutcome::Ok, None);
    *pending = TraceStage::SystemConnectRead;
    let prefix = connect_response::read(&mut socket, context, true, &connect).await?;
    Ok(Box::new(Prefixed {
        prefix: std::io::Cursor::new(prefix),
        socket,
    }))
}

struct Prefixed {
    prefix: std::io::Cursor<Vec<u8>>,
    socket: UpstreamStream,
}
impl AsyncRead for Prefixed {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let start = self.prefix.position() as usize;
        let count = (self.prefix.get_ref().len() - start).min(buf.remaining());
        if count > 0 {
            buf.put_slice(&self.prefix.get_ref()[start..start + count]);
            self.prefix.set_position((start + count) as u64);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.socket).poll_read(cx, buf)
    }
}
impl AsyncWrite for Prefixed {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.socket).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}
