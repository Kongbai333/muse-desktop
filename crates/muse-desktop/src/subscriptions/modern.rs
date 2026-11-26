use super::{Entry, Value, decode, label, unescape};
use muse_proxy::{FixedNode, NodeKind, WebSocketTransport};
use std::collections::BTreeMap;
use url::Url;
use zeroize::Zeroizing;

type Checked<T> = Result<T, &'static str>;
const INVALID: &str = "端点、认证或参数格式无效";
const TLS: &str = "需要验证证书的标准 TLS";
const FIELD: &str = "含未支持的节点参数";
const NETWORK: &str = "仅支持 TCP 或标准 WebSocket 传输";
const FLOW: &str = "不支持 REALITY、flow 或扩展加密";
const AUTH: &str = "仅支持 alterId 0 的 VMess AEAD";

pub(super) fn supports(kind: &str) -> bool {
    matches!(kind, "vless" | "vmess" | "trojan")
}

fn result(kind: &'static str, name: String, route: Checked<FixedNode>) -> Entry {
    let route = route.and_then(|node| node.encode().map_err(|_| INVALID));
    let reason = route.as_ref().err().copied();
    Entry {
        protocol: kind,
        label: name,
        reason,
        endpoint: route.ok(),
    }
}

fn hostname(raw: &str) -> Checked<String> {
    if raw.is_empty()
        || raw
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || "@/?#%\\".contains(ch))
    {
        return Err(INVALID);
    }
    if let Ok(ip) = raw.parse::<std::net::Ipv6Addr>() {
        return Ok(ip.to_string());
    }
    Ok(url::Host::parse(raw)
        .map_err(|_| INVALID)?
        .to_string()
        .trim_matches(['[', ']'])
        .to_ascii_lowercase())
}

struct Parts<'a> {
    kind: &'a str,
    host: &'a str,
    port: u16,
    secret: &'a str,
    cipher: &'a str,
    sni: &'a str,
    net: &'a str,
    path: &'a str,
    authority: &'a str,
}

impl Parts<'_> {
    fn finish(self) -> Checked<FixedNode> {
        let kind = match (self.kind, self.cipher) {
            ("trojan", "") => NodeKind::Trojan,
            ("vless", "") => NodeKind::Vless,
            ("vmess", "" | "auto" | "aes-128-gcm") => NodeKind::VmessAes128Gcm,
            ("vmess", "chacha20-poly1305") => NodeKind::VmessChacha20Poly1305,
            _ => return Err(AUTH),
        };
        let host = hostname(self.host)?;
        let server_name = if self.sni.is_empty() {
            host.clone()
        } else {
            hostname(self.sni)?
        };
        let websocket = match self.net {
            "" | "tcp" | "raw" if self.path.is_empty() && self.authority.is_empty() => None,
            "ws" => {
                if self.path.split_once('?').is_some_and(|(_, query)| {
                    url::form_urlencoded::parse(query.as_bytes()).any(|(key, _)| key == "ed")
                }) {
                    return Err(NETWORK);
                }
                let authority = if self.authority.is_empty() {
                    let host = if server_name.contains(':') {
                        format!("[{server_name}]")
                    } else {
                        server_name.clone()
                    };
                    if self.port == 443 {
                        host
                    } else {
                        format!("{host}:{}", self.port)
                    }
                } else {
                    self.authority.to_ascii_lowercase()
                };
                Some(WebSocketTransport {
                    authority,
                    path: if self.path.is_empty() {
                        "/".into()
                    } else {
                        self.path.into()
                    },
                })
            }
            _ => return Err(NETWORK),
        };
        let credential = if matches!(kind, NodeKind::Trojan) {
            self.secret.to_owned()
        } else {
            uuid::Uuid::parse_str(self.secret)
                .map_err(|_| INVALID)?
                .hyphenated()
                .to_string()
        };
        Ok(FixedNode {
            kind,
            host,
            port: self.port,
            server_name,
            credential,
            websocket,
        })
    }
}

fn well_escaped(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().all(|(index, byte)| {
        *byte != b'%'
            || bytes
                .get(index + 1..index + 3)
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
    })
}

pub(super) fn share(kind: &'static str, uri: &str, fragment: &str) -> Entry {
    let mut name = label(&unescape(fragment).unwrap_or_default());
    let route = (|| {
        if uri.len() > 16384 || uri.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
            return Err(INVALID);
        }
        if kind == "vmess" {
            let decoded = Zeroizing::new(
                decode(uri.strip_prefix("vmess://").ok_or(INVALID)?).ok_or(INVALID)?,
            );
            let fields: VmessShare = serde_json::from_str(&decoded).map_err(|_| FIELD)?;
            if fragment.is_empty() {
                name = label(&fields.ps)
            }
            return fields.node();
        }
        if !well_escaped(uri) {
            return Err(INVALID);
        }
        let url = Url::parse(uri).map_err(|_| INVALID)?;
        if !matches!(url.path(), "" | "/") || url.password().is_some() {
            return Err(INVALID);
        }
        let mut query = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            if value.contains('\u{fffd}')
                || query.insert(key.to_string(), value.to_string()).is_some()
            {
                return Err(INVALID);
            }
            match key.as_ref() {
                "security" | "type" | "sni" | "path" | "host" => (),
                "encryption" if kind == "vless" && value == "none" => (),
                "encryption" | "flow" if value.is_empty() => (),
                "flow" | "pbk" | "sid" | "encryption" => return Err(FLOW),
                "allowInsecure" | "insecure" if matches!(value.as_ref(), "false" | "0") => (),
                "allowInsecure" | "insecure" | "fp" | "alpn" => return Err(TLS),
                "headerType" if value == "none" => (),
                _ => return Err(FIELD),
            }
        }
        let get = |key: &str| query.get(key).map(String::as_str).unwrap_or("");
        match get("security") {
            "tls" => (),
            "" if kind == "trojan" && !query.contains_key("security") => (),
            "reality" => return Err(FLOW),
            _ => return Err(TLS),
        }
        Parts {
            kind,
            host: url.host_str().ok_or(INVALID)?,
            port: url.port().ok_or(INVALID)?,
            secret: &unescape(url.username()).ok_or(INVALID)?,
            cipher: "",
            sni: get("sni"),
            net: get("type"),
            path: get("path"),
            authority: get("host"),
        }
        .finish()
    })();
    result(kind, name, route)
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct VmessShare {
    v: serde_json::Value,
    ps: String,
    add: String,
    port: serde_json::Value,
    id: String,
    aid: serde_json::Value,
    scy: String,
    net: String,
    #[serde(rename = "type")]
    header: String,
    host: String,
    path: String,
    tls: String,
    sni: String,
    alpn: String,
    fp: String,
    #[serde(rename = "allowInsecure")]
    insecure: serde_json::Value,
}

fn number(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

impl VmessShare {
    fn node(&self) -> Checked<FixedNode> {
        if !self.v.is_null() && number(&self.v) != Some(2) {
            return Err(FIELD);
        }
        if !self.aid.is_null() && number(&self.aid) != Some(0) {
            return Err(AUTH);
        }
        let secure = self.insecure.is_null()
            || self.insecure.as_bool() == Some(false)
            || self.insecure.as_str() == Some("false")
            || number(&self.insecure) == Some(0);
        if self.tls != "tls" || !secure || !self.fp.is_empty() || !self.alpn.is_empty() {
            return Err(TLS);
        }
        if !matches!(self.header.as_str(), "" | "none") {
            return Err(NETWORK);
        }
        Parts {
            kind: "vmess",
            host: &self.add,
            port: number(&self.port)
                .and_then(|port| port.try_into().ok())
                .ok_or(INVALID)?,
            secret: &self.id,
            cipher: &self.scy,
            sni: &self.sni,
            net: &self.net,
            path: &self.path,
            authority: &self.host,
        }
        .finish()
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Checked<&'a str> {
    match value.get(key) {
        None => Ok(""),
        Some(field) => field.as_str().ok_or(INVALID),
    }
}

pub(super) fn clash(kind: &'static str, value: &Value) -> Entry {
    let route = (|| {
        for (key, field) in value.as_mapping().ok_or(INVALID)? {
            match key.as_str().ok_or(FIELD)? {
                "name" | "type" | "server" | "port" | "tls" | "network" | "ws-opts" | "sni"
                | "servername" => (),
                "udp" if field.is_bool() => (),
                "uuid" if kind != "trojan" => (),
                "password" if kind == "trojan" => (),
                "cipher" if kind == "vmess" => (),
                "alterId" if kind == "vmess" && field.as_u64() == Some(0) => (),
                "alterId" => return Err(AUTH),
                "encryption" if kind == "vless" && field.as_str() == Some("none") => (),
                "flow" if field.as_str() == Some("") => (),
                "flow" | "reality-opts" | "encryption" => return Err(FLOW),
                "skip-cert-verify" if field.as_bool() == Some(false) => (),
                "skip-cert-verify" | "client-fingerprint" | "alpn" => return Err(TLS),
                "grpc-opts" => return Err(NETWORK),
                _ => return Err(FIELD),
            }
        }
        if !match value.get("tls") {
            None => kind == "trojan",
            Some(field) => field.as_bool() == Some(true),
        } {
            return Err(TLS);
        }
        if value.get("sni").is_some() && value.get("servername").is_some() {
            return Err(INVALID);
        }
        let net = text(value, "network")?;
        let mut path = "";
        let mut authority = "";
        if let Some(opts) = value.get("ws-opts") {
            if net != "ws" {
                return Err(NETWORK);
            }
            for (key, field) in opts.as_mapping().ok_or(INVALID)? {
                match key.as_str() {
                    Some("path") => path = field.as_str().ok_or(INVALID)?,
                    Some("headers") => {
                        let headers = field.as_mapping().ok_or(INVALID)?;
                        if headers.len() > 1 {
                            return Err(FIELD);
                        }
                        for (key, val) in headers {
                            if !key
                                .as_str()
                                .is_some_and(|key| key.eq_ignore_ascii_case("host"))
                            {
                                return Err(FIELD);
                            }
                            authority = val.as_str().ok_or(INVALID)?;
                        }
                    }
                    _ => return Err(NETWORK),
                }
            }
        }
        Parts {
            kind,
            host: text(value, "server")?,
            port: value
                .get("port")
                .and_then(Value::as_u64)
                .and_then(|port| port.try_into().ok())
                .ok_or(INVALID)?,
            secret: text(value, if kind == "trojan" { "password" } else { "uuid" })?,
            cipher: text(value, "cipher")?,
            sni: text(
                value,
                if value.get("servername").is_some() {
                    "servername"
                } else {
                    "sni"
                },
            )?,
            net,
            path,
            authority,
        }
        .finish()
    })();
    result(
        kind,
        label(value.get("name").and_then(Value::as_str).unwrap_or("")),
        route,
    )
}
