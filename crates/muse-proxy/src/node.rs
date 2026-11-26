use super::{Authority, ProxyError, ProxyProtocol, ServerName, UpstreamStream};
use bytes::{Buf, Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt, future::ready};
use marmo_net::Endpoint;
use marmo_proto::{trojan, vless, vmess};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};
use tokio_util::{
    codec::{Decoder, Encoder, Framed},
    io::{CopyToBytes, SinkWriter, StreamReader},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Copy, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Vless,
    VmessAes128Gcm,
    VmessChacha20Poly1305,
    Trojan,
}

impl NodeKind {
    pub(super) fn protocol(self) -> ProxyProtocol {
        match self {
            Self::Vless => ProxyProtocol::Vless,
            Self::Trojan => ProxyProtocol::Trojan,
            _ => ProxyProtocol::Vmess,
        }
    }
}

#[derive(Clone, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebSocketTransport {
    pub authority: String,
    pub path: String,
}

#[derive(Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedNode {
    pub kind: NodeKind,
    pub host: String,
    pub port: u16,
    pub server_name: String,
    pub credential: String,
    pub websocket: Option<WebSocketTransport>,
}

impl Drop for FixedNode {
    fn drop(&mut self) {
        self.credential.zeroize();
    }
}

impl FixedNode {
    pub fn encode(&self) -> Result<Zeroizing<String>, ProxyError> {
        self.validate()?;
        let value = Zeroizing::new(serde_json::to_string(self).map_err(|_| ProxyError::Invalid)?);
        if value.len() > 16384 {
            return Err(ProxyError::Invalid);
        }
        Ok(value)
    }

    pub(super) fn validate(&self) -> Result<(), ProxyError> {
        if self.port == 0
            || self.credential.is_empty()
            || self.credential.len() > 2048
            || self.credential.chars().any(char::is_control)
        {
            return Err(ProxyError::Invalid);
        }
        ServerName::try_from(self.host.as_str()).map_err(|_| ProxyError::Invalid)?;
        ServerName::try_from(self.server_name.as_str()).map_err(|_| ProxyError::Invalid)?;
        if !matches!(self.kind, NodeKind::Trojan) {
            vless::Uuid::parse(&self.credential).map_err(|_| ProxyError::Invalid)?;
        }
        if let Some(ws) = &self.websocket {
            let authority = ws
                .authority
                .parse::<Authority>()
                .map_err(|_| ProxyError::Invalid)?;
            url::Url::parse(&format!("http://{}/", ws.authority))
                .map_err(|_| ProxyError::Invalid)?;
            if ws.authority.contains('@')
                || authority.port().is_some_and(|port| port.as_u16() == 0)
                || !ws.path.starts_with('/')
                || ws.path.starts_with("//")
                || ws.path.contains(['#', '\\'])
                || !ws.path.is_ascii()
                || ws.path.bytes().any(|byte| byte <= 32 || byte == 127)
            {
                return Err(ProxyError::Invalid);
            }
            ServerName::try_from(authority.host().trim_matches(['[', ']']))
                .map_err(|_| ProxyError::Invalid)?;
            ws.path
                .parse::<hyper::http::uri::PathAndQuery>()
                .map_err(|_| ProxyError::Invalid)?;
        }
        Ok(())
    }

    pub(super) async fn connect(
        &self,
        socket: UpstreamStream,
        target: (&str, u16),
    ) -> Result<UpstreamStream, ProxyError> {
        let target = match target.0.parse() {
            Ok(ip) => Endpoint::from_ip(ip, target.1),
            Err(_) => Endpoint::from_domain(target.0, target.1).map_err(|_| ProxyError::Invalid)?,
        };
        let mut socket = if let Some(ws) = &self.websocket {
            ws.connect(socket).await?
        } else {
            socket
        };
        match self.kind {
            NodeKind::Trojan => {
                let mut head = Zeroizing::new([0; trojan::REQUEST_HEADER_MAX]);
                let size = trojan::encode_request_header(
                    &trojan::PasswordHash::derive(&self.credential),
                    &target,
                    trojan::COMMAND_CONNECT,
                    &mut head[..],
                )
                .map_err(|_| ProxyError::Invalid)?;
                send_header(&mut socket, &head[..size]).await?;
                Ok(socket)
            }
            NodeKind::Vless => {
                let uuid = vless::Uuid::parse(&self.credential).map_err(|_| ProxyError::Invalid)?;
                let mut head = Zeroizing::new([0; vless::REQUEST_HEADER_MAX]);
                let size = vless::encode_request(&uuid, &target, vless::COMMAND_TCP, &mut head[..])
                    .map_err(|_| ProxyError::Invalid)?;
                send_header(&mut socket, &head[..size]).await?;
                Ok(framed(socket, NodeCodec::Vless { header: true }))
            }
            kind => {
                let uuid = vless::Uuid::parse(&self.credential).map_err(|_| ProxyError::Invalid)?;
                let key = vmess::CmdKey::derive(&uuid);
                let secrets = vmess::RequestSecrets {
                    body_iv: random()?,
                    body_key: random()?,
                    response_auth: random::<1>()?[0],
                    auth_rand: random()?,
                    connection_nonce: random()?,
                };
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| ProxyError::Protocol)?
                    .as_secs();
                let security = if matches!(kind, NodeKind::VmessAes128Gcm) {
                    vmess::Security::Aes128Gcm
                } else {
                    vmess::Security::ChaCha20Poly1305
                };
                let spec = vmess::RequestSpec {
                    target: &target,
                    command: vmess::COMMAND_TCP,
                    security,
                    timestamp,
                    padding: &[],
                    secrets: &secrets,
                };
                let mut head = Zeroizing::new([0; vmess::REQUEST_HEADER_MAX]);
                let size = vmess::encode_request(&key, &spec, &mut head[..])
                    .map_err(|_| ProxyError::Invalid)?;
                let vmess::RequestProgress::Complete(request) =
                    vmess::request_progress(&head[..size], &key, timestamp)
                else {
                    return Err(ProxyError::Protocol);
                };
                let (response_key, response_iv) =
                    vmess::response_keys(&secrets.body_key, &secrets.body_iv);
                let decoder =
                    vmess::ChunkDecoder::new(security, &response_key, &response_iv, request.option);
                let encoder = vmess::ChunkEncoder::new(
                    security,
                    &secrets.body_key,
                    &secrets.body_iv,
                    request.option,
                    random()?,
                );
                send_header(&mut socket, &head[..size]).await?;
                Ok(framed(
                    socket,
                    NodeCodec::Vmess(Box::new(VmessBody {
                        response: Some(request),
                        decoder,
                        encoder,
                    })),
                ))
            }
        }
    }
}

async fn send_header(socket: &mut UpstreamStream, head: &[u8]) -> Result<(), ProxyError> {
    socket
        .write_all(head)
        .await
        .map_err(|_| ProxyError::Connect)?;
    socket.flush().await.map_err(|_| ProxyError::Connect)
}

fn random<const N: usize>() -> Result<[u8; N], ProxyError> {
    let mut value = [0; N];
    SystemRandom::new()
        .fill(&mut value)
        .map_err(|_| ProxyError::Protocol)?;
    Ok(value)
}

impl WebSocketTransport {
    async fn connect(&self, socket: UpstreamStream) -> Result<UpstreamStream, ProxyError> {
        let uri = format!("wss://{}{}", self.authority, self.path);
        let config = WebSocketConfig::default()
            .write_buffer_size(0)
            .max_message_size(Some(4 * 1024 * 1024))
            .max_frame_size(Some(4 * 1024 * 1024));
        let (stream, _) = tokio_tungstenite::client_async_with_config(uri, socket, Some(config))
            .await
            .map_err(|_| ProxyError::Protocol)?;
        let stream = stream
            .sink_map_err(|_| invalid())
            .with(|data: Bytes| ready(Ok::<_, io::Error>(Message::Binary(data))))
            .take_while(|result| ready(!matches!(result, Ok(Message::Close(_)))))
            .filter_map(|result| {
                ready(match result {
                    Ok(Message::Binary(data)) => Some(Ok(data)),
                    Ok(Message::Ping(_) | Message::Pong(_)) => None,
                    _ => Some(Err(invalid())),
                })
            });
        Ok(Box::new(SinkWriter::new(StreamReader::new(
            CopyToBytes::new(stream),
        ))))
    }
}

fn invalid() -> io::Error {
    io::ErrorKind::InvalidData.into()
}
fn framed(socket: UpstreamStream, codec: NodeCodec) -> UpstreamStream {
    Box::new(SinkWriter::new(StreamReader::new(Framed::new(
        socket, codec,
    ))))
}

enum NodeCodec {
    Vless { header: bool },
    Vmess(Box<VmessBody>),
}
struct VmessBody {
    response: Option<vmess::Request>,
    decoder: vmess::ChunkDecoder,
    encoder: vmess::ChunkEncoder,
}

impl Decoder for NodeCodec {
    type Item = Bytes;
    type Error = io::Error;
    fn decode(&mut self, wire: &mut BytesMut) -> io::Result<Option<Bytes>> {
        match self {
            Self::Vless { header } => {
                if *header {
                    if wire.len() < 2 {
                        return Ok(None);
                    }
                    let addons = vless::response_addons(wire).map_err(|_| invalid())?;
                    if addons != 0 {
                        return Err(invalid());
                    }
                    wire.advance(2);
                    *header = false;
                }
                Ok((!wire.is_empty()).then(|| wire.split().freeze()))
            }
            Self::Vmess(body) => {
                if let Some(request) = &body.response {
                    match vmess::response_progress(wire, request) {
                        vmess::ResponseProgress::NeedMore { .. } => return Ok(None),
                        vmess::ResponseProgress::Invalid => return Err(invalid()),
                        vmess::ResponseProgress::Complete { consumed } => wire.advance(consumed),
                    }
                    body.response = None;
                }
                match body.decoder.progress(wire).map_err(|_| invalid())? {
                    vmess::ChunkProgress::NeedMore { .. } => Ok(None),
                    vmess::ChunkProgress::Complete { consumed, data } => {
                        wire.advance(consumed);
                        Ok(Some(data.into()))
                    }
                }
            }
        }
    }
}

impl Encoder<&[u8]> for NodeCodec {
    type Error = io::Error;
    fn encode(&mut self, data: &[u8], wire: &mut BytesMut) -> io::Result<()> {
        match self {
            Self::Vless { .. } => wire.extend_from_slice(data),
            Self::Vmess(body) => {
                let mut frame = Vec::new();
                for chunk in data.chunks(vmess::CHUNK_PLAINTEXT_MAX) {
                    body.encoder
                        .encode(chunk, &mut frame)
                        .map_err(|_| invalid())?;
                    wire.extend_from_slice(&frame);
                }
            }
        }
        Ok(())
    }
}
