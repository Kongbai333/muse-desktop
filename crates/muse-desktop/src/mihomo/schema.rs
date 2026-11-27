use serde_json::{Map, Value, json};
use zeroize::Zeroizing;

pub const PREFIX: &str = "muse-mihomo-v1:";
pub const OPTION: &str = "此节点选项尚未支持";
pub const INVALID: &str = "地址、端口、字段类型或认证材料无效";

pub fn kind(value: &str) -> Option<&'static str> {
    [
        "ss",
        "ssr",
        "http",
        "socks5",
        "vmess",
        "vless",
        "trojan",
        "anytls",
        "hysteria",
        "hysteria2",
        "tuic",
        "snell",
        "wireguard",
        "ssh",
        "mieru",
        "gost-relay",
        "sudoku",
        "shadowquic",
        "trusttunnel",
        "masque",
        "openvpn",
        "tailscale",
        "zerotier",
        "easytier",
        "direct",
        "rematch",
        "reject",
        "dns",
    ]
    .into_iter()
    .find(|kind| *kind == value)
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && (host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok()
            || host.split('.').all(|part| {
                !part.is_empty()
                    && part.len() <= 63
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    && !part.starts_with('-')
                    && !part.ends_with('-')
            }))
}

fn words(list: &str, field: &str) -> bool {
    list.split_ascii_whitespace().any(|key| key == field)
}

pub(crate) fn unsupported_key(key: &str) -> String {
    match key {
        "dialer-proxy" => "dialer-proxy：不能把账号流量转发到另一个代理".into(),
        "interface-name" => "interface-name：不能指定出站网卡".into(),
        "routing-mark" => "routing-mark：不能覆盖系统出站路由标记".into(),
        "certificate" | "private-key" | "ca" | "ca-str" => {
            format!("{key}：此协议不接受自带证书材料或文件路径")
        }
        "name-cert-verify" => "name-cert-verify：不能替换校验的服务端名称".into(),
        "skip-cert-verify" | "plugin" | "plugin-opts" | "smux" | "network" | "tls" | "sni"
        | "udp" | "tfo" | "mptcp" | "uuid" | "username" | "alpn" | "fingerprint"
        | "client-fingerprint" | "ech-opts" | "restls-opts" | "shadow-tls-opts" | "jls-opts" => {
            format!("{key}：此协议或传输不支持此字段")
        }
        _ => {
            let digest = ring::digest::digest(&ring::digest::SHA256, key.as_bytes());
            let [first, second, third]: [u8; 3] = digest.as_ref()[..3].try_into().unwrap();
            format!("未识别的字段（键名已隐藏，摘要 {first:02x}{second:02x}{third:02x}）")
        }
    }
}

fn normalize_bool(key: &str, value: &mut Value) -> Result<(), String> {
    *value = match value.as_str() {
        Some("true") => Value::Bool(true),
        Some("false") => Value::Bool(false),
        _ if value.is_boolean() => return Ok(()),
        _ => return Err(format!("{key}：需要布尔值 true 或 false")),
    };
    Ok(())
}

fn normalize_options(kind: &str, map: &mut Map<String, Value>) -> Result<(), String> {
    for key in [
        "udp",
        "tfo",
        "mptcp",
        "skip-cert-verify",
        if kind == "ss" {
            "udp-over-tcp"
        } else {
            "disable-reuse"
        },
    ] {
        if let Some(value) = map.get_mut(key) {
            normalize_bool(key, value)?;
        }
    }
    for key in [
        "dialer-proxy",
        "interface-name",
        "certificate",
        "private-key",
        "name-cert-verify",
    ] {
        if map.get(key) == Some(&json!("")) {
            map.remove(key);
        }
    }
    if map.get("routing-mark").and_then(Value::as_i64) == Some(0) {
        map.remove("routing-mark");
    }
    if kind == "anytls" {
        for key in ["sni", "fingerprint", "client-fingerprint", "username"] {
            if map.get(key) == Some(&json!("")) {
                map.remove(key);
            }
        }
    }
    if kind == "ss" {
        if map.get("plugin") == Some(&json!("")) {
            map.remove("plugin");
        }
        if let Some(opts) = map.get_mut("plugin-opts").and_then(Value::as_object_mut) {
            for key in ["tls", "skip-cert-verify", "mux"] {
                if let Some(value) = opts.get_mut(key) {
                    normalize_bool(key, value).map_err(|err| format!("plugin-opts.{err}"))?;
                }
            }
            if opts.is_empty() && !map.contains_key("plugin") {
                map.remove("plugin-opts");
            }
        }
    }
    if let Some(value) = map.get_mut("smux") {
        let opts = value
            .as_object_mut()
            .ok_or("smux：需要包含 enabled 的对象")?;
        if let Some(key) = opts.keys().find(|key| key.as_str() != "enabled") {
            return Err(format!("smux：{}", unsupported_key(key)));
        }
        let enabled = opts
            .get_mut("enabled")
            .ok_or("smux.enabled：需要显式指定 false")?;
        normalize_bool("smux.enabled", enabled)?;
        if enabled.as_bool() == Some(true) {
            return Err("smux.enabled：此试用版未支持启用该复用传输".into());
        }
        map.remove("smux");
    }
    Ok(())
}

fn valid_authority(authority: &str) -> bool {
    !authority.is_empty()
        && !authority.ends_with(':')
        && !authority.contains(['@', '/', '\\', '?', '#', '%'])
        && !authority
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
        && url::Url::parse(&format!("http://{authority}/"))
            .is_ok_and(|url| url.host_str().is_some_and(valid_host) && url.port() != Some(0))
}

fn scalar(field: &str, value: &Value) -> bool {
    match field {
        "skip-cert-verify" => value == &Value::Bool(false),
        "udp" | "tls" | "mux" | "remote-dns-resolve" | "disable-sni" | "reduce-rtt" | "tfo"
        | "mptcp" | "udp-over-tcp" | "disable-reuse" => value.is_boolean(),
        "ip-version" => matches!(
            value.as_str(),
            Some("dual" | "ipv4" | "ipv6" | "ipv4-prefer" | "ipv6-prefer")
        ),
        "udp-over-tcp-version" => matches!(value.as_u64(), Some(0..=2)),
        "idle-session-check-interval" | "idle-session-timeout" => {
            matches!(value.as_u64(), Some(0..=86400))
        }
        "min-idle-session" => matches!(value.as_u64(), Some(0..=32)),
        "alterId" | "version" | "mtu" | "persistent-keepalive" => {
            value.as_u64().is_some_and(|num| num <= 65535)
        }
        "alpn" | "dns" | "host-key" | "host-key-algorithms" | "allowed-ips" => {
            value.as_array().is_some_and(|items| {
                (1..=32).contains(&items.len())
                    && items.iter().all(|item| {
                        item.as_str().is_some_and(|text| {
                            !text.is_empty()
                                && text.len() <= 8192
                                && !text.chars().any(char::is_control)
                                && (field != "dns" || text.parse::<std::net::IpAddr>().is_ok())
                        })
                    })
            })
        }
        "sni" | "servername" => value.as_str().is_some_and(valid_host),
        "fingerprint" => value.as_str().is_some_and(|pin| {
            let pin = pin.replace(':', "");
            pin.len() == 64 && pin.bytes().all(|byte| byte.is_ascii_hexdigit())
        }),
        "uuid" => value
            .as_str()
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()),
        _ => value.as_str().is_some_and(|text| {
            !text.is_empty() && text.len() <= 8192 && !text.chars().any(char::is_control)
        }),
    }
}

fn validate(input: Value) -> Result<Value, String> {
    let mut map = input.as_object().ok_or(INVALID)?.clone();
    let kind = kind(map.get("type").and_then(Value::as_str).ok_or(INVALID)?).ok_or(OPTION)?;
    let (fields, required) = match kind {
        "ss" => (
            "cipher password plugin plugin-opts tfo mptcp ip-version udp-over-tcp udp-over-tcp-version",
            "cipher password",
        ),
        "ssr" => (
            "cipher password protocol protocol-param obfs obfs-param",
            "cipher password protocol obfs",
        ),
        "http" => ("tls username password sni fingerprint skip-cert-verify", ""),
        "socks5" => ("tls username password fingerprint skip-cert-verify", ""),
        "vmess" => (
            "tls uuid alterId cipher servername network ws-opts grpc-opts alpn skip-cert-verify client-fingerprint fingerprint",
            "uuid cipher alterId",
        ),
        "vless" => (
            "tls uuid flow encryption servername network ws-opts grpc-opts reality-opts alpn skip-cert-verify client-fingerprint fingerprint",
            "uuid",
        ),
        "trojan" => (
            "password sni network ws-opts grpc-opts reality-opts alpn skip-cert-verify client-fingerprint fingerprint",
            "password",
        ),
        "anytls" => (
            "password sni alpn client-fingerprint fingerprint skip-cert-verify tfo mptcp ip-version idle-session-check-interval idle-session-timeout min-idle-session disable-reuse",
            "password",
        ),
        "hysteria" => (
            "up down auth auth-str obfs sni alpn fingerprint skip-cert-verify protocol",
            "up down",
        ),
        "hysteria2" => (
            "password up down obfs obfs-password sni alpn fingerprint skip-cert-verify",
            "password",
        ),
        "tuic" => (
            "uuid password token sni alpn disable-sni reduce-rtt udp-relay-mode congestion-controller fingerprint skip-cert-verify",
            "",
        ),
        "snell" => ("psk version obfs-opts", "psk"),
        "wireguard" => (
            "private-key public-key pre-shared-key ip ipv6 mtu persistent-keepalive allowed-ips remote-dns-resolve dns",
            "private-key public-key",
        ),
        "ssh" => (
            "username password private-key private-key-passphrase host-key host-key-algorithms",
            "username host-key",
        ),
        "mieru" => (
            "username password transport multiplexing handshake-mode",
            "username password transport",
        ),
        "gost-relay" => (
            "tls mux username password sni skip-cert-verify fingerprint",
            "",
        ),
        "sudoku" => ("key aead-method table-type", "key"),
        "shadowquic" => (
            "username password sni alpn congestion-controller up down",
            "username password",
        ),
        "trusttunnel" => (
            "username password sni alpn fingerprint skip-cert-verify",
            "username password",
        ),
        "masque" => (
            "private-key public-key ip ipv6 sni mtu remote-dns-resolve dns skip-cert-verify",
            "private-key public-key",
        ),
        "openvpn" => {
            return Err("此版 OpenVPN 可验证签发 CA，但不验证服务端名称，不接受该节点".into());
        }
        "tailscale" | "zerotier" | "easytier" => {
            return Err("多对等网络或控制面不属于单个固定网页出口".into());
        }
        _ => return Err("内核内部动作不是可绑定的远程网页出口".into()),
    };
    if matches!(kind, "ss" | "anytls") {
        normalize_options(kind, &mut map)?;
    }
    if !map
        .get("server")
        .and_then(Value::as_str)
        .is_some_and(valid_host)
    {
        return Err("server：需要有效的主机名或 IP 地址".into());
    }
    let port = map
        .get("port")
        .and_then(|field| field.as_u64().or_else(|| field.as_str()?.parse().ok()))
        .filter(|port| (1..=65535).contains(port))
        .ok_or("port：需要 1–65535 的端口号")?;
    map.insert("port".into(), json!(port));
    map.insert("name".into(), json!("account"));
    for (key, value) in &map {
        if words("name type server port", key) {
            continue;
        }
        if matches!(key.as_str(), "skip-cert-verify" | "disable-sni")
            && value.as_bool() != Some(false)
        {
            return Err(match (key.as_str(), value.as_bool()) {
                ("skip-cert-verify", Some(true)) => {
                    "skip-cert-verify：不能跳过服务端证书校验".into()
                }
                ("disable-sni", Some(true)) => "disable-sni：不能关闭服务端名称指示".into(),
                _ => format!("{key}：需要布尔值 false"),
            });
        }
        if !words(fields, key) && key != "udp" {
            return Err(unsupported_key(key));
        }
        if key == "network" && !matches!(value.as_str(), Some("tcp" | "ws" | "grpc")) {
            return Err("network：仅支持 tcp、ws、grpc".into());
        }
        if kind == "hysteria" && key == "protocol" && value.as_str() != Some("udp") {
            return Err("protocol：Hysteria 仅接受无特权的 UDP 传输".into());
        }
        if key == "plugin" && !matches!(value.as_str(), Some("obfs" | "v2ray-plugin")) {
            return Err("plugin：此 SS 插件不在内置插件白名单内".into());
        }
        if kind == "ssh" && key == "private-key" {
            if !value.as_str().is_some_and(|pem| {
                pem.len() <= 8192
                    && pem.starts_with("-----BEGIN ")
                    && pem.contains(" PRIVATE KEY-----\n")
                    && pem.trim_end().ends_with(" PRIVATE KEY-----")
            }) {
                return Err("private-key：SSH 仅接受内联密钥，不读取本地文件".into());
            }
            continue;
        }
        if key.ends_with("-opts") {
            let nested = value
                .as_object()
                .ok_or_else(|| format!("{key}：需要对象"))?;
            let allow = match key.as_str() {
                "ws-opts" if map.get("network").and_then(Value::as_str) == Some("ws") => {
                    "path headers"
                }
                "grpc-opts" if map.get("network").and_then(Value::as_str) == Some("grpc") => {
                    "grpc-service-name"
                }
                "reality-opts"
                    if kind == "trojan"
                        || map.get("tls").and_then(Value::as_bool) == Some(true) =>
                {
                    "public-key short-id"
                }
                "obfs-opts" => "mode host",
                "plugin-opts" if map.get("plugin").and_then(Value::as_str) == Some("obfs") => {
                    "mode host"
                }
                "plugin-opts"
                    if map.get("plugin").and_then(Value::as_str) == Some("v2ray-plugin") =>
                {
                    "mode host path tls mux skip-cert-verify"
                }
                _ => return Err(format!("{key}：与当前插件或传输不匹配")),
            };
            if key == "reality-opts"
                && nested
                    .get("public-key")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            {
                return Err("reality-opts.public-key：缺少公钥".into());
            }
            for (subkey, entry) in nested {
                if !words(allow, subkey) {
                    return Err(format!("{key}：{}", unsupported_key(subkey)));
                }
                if subkey == "headers" {
                    if !entry.as_object().is_some_and(|headers| {
                        headers.len() == 1
                            && headers.iter().all(|(header, field)| {
                                header.eq_ignore_ascii_case("host")
                                    && field.as_str().is_some_and(valid_authority)
                            })
                    }) {
                        return Err(format!("{key}.headers：仅允许有效的 Host"));
                    }
                } else if subkey == "short-id" && entry == "" {
                    continue;
                } else if !scalar(subkey, entry) {
                    return Err(
                        if subkey == "skip-cert-verify" && entry == &Value::Bool(true) {
                            format!("{key}.{subkey}：不能跳过服务端证书校验")
                        } else {
                            format!("{key}.{subkey}：字段类型或格式无效")
                        },
                    );
                }
            }
        } else if !scalar(key, value) {
            let expected = match key.as_str() {
                "udp-over-tcp-version" => "需要整数 0、1 或 2",
                "idle-session-check-interval" | "idle-session-timeout" => "需要 0–86400 的整数秒数",
                "min-idle-session" => "需要 0–32 的整数",
                "ip-version" => "需要 dual、ipv4、ipv6、ipv4-prefer 或 ipv6-prefer",
                "fingerprint" => "需要 SHA-256 证书指纹",
                "sni" | "servername" => "需要有效的主机名",
                "alpn" => "需要非空协议名称列表",
                _ => "字段类型或格式无效",
            };
            return Err(format!("{key}：{expected}"));
        }
    }
    if let Some(key) = required
        .split_ascii_whitespace()
        .find(|key| !map.contains_key(*key))
    {
        return Err(format!("{key}：缺少必填字段"));
    }
    if matches!(kind, "vless" | "vmess" | "http" | "socks5" | "gost-relay")
        && map.get("tls").and_then(Value::as_bool) != Some(true)
        && [
            "sni",
            "servername",
            "client-fingerprint",
            "fingerprint",
            "alpn",
            "reality-opts",
        ]
        .iter()
        .any(|key| map.contains_key(*key))
    {
        return Err("tls：使用证书或 TLS 传输选项时必须启用 TLS".into());
    }
    if kind == "ss" {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let cipher = map["cipher"].as_str().ok_or(INVALID)?;
        if let Some(size) = match cipher {
            "2022-blake3-aes-128-gcm" => Some(16),
            "2022-blake3-aes-256-gcm" | "2022-blake3-chacha20-poly1305" => Some(32),
            _ => None,
        } {
            if !map["password"]
                .as_str()
                .ok_or(INVALID)?
                .split(':')
                .all(|part| STANDARD.decode(part).is_ok_and(|key| key.len() == size))
            {
                return Err("password：密钥须为 Base64 编码且长度与 SS2022 加密套件相符".into());
            }
        } else if !words(
            "aes-128-gcm aes-192-gcm aes-256-gcm chacha20-ietf-poly1305 xchacha20-ietf-poly1305 aes-128-ctr aes-192-ctr aes-256-ctr aes-128-cfb aes-192-cfb aes-256-cfb chacha20-ietf rc4-md5",
            cipher,
        ) {
            return Err("cipher：未支持该 SS 加密套件".into());
        }
    }
    if kind == "tuic"
        && !matches!(
            (
                map.contains_key("token"),
                map.contains_key("uuid"),
                map.contains_key("password")
            ),
            (true, false, false) | (false, true, true)
        )
    {
        return Err(INVALID.into());
    }
    if matches!(kind, "wireguard" | "masque")
        && (map.get("remote-dns-resolve") != Some(&json!(true))
            || !map.contains_key("dns")
            || !(map.contains_key("ip") || map.contains_key("ipv6")))
    {
        return Err("单对等 IP 隧道须声明隧道内 DNS 和本端地址".into());
    }
    if kind == "ssh" && !(map.contains_key("password") || map.contains_key("private-key")) {
        return Err(INVALID.into());
    }
    if kind == "ssh"
        && map.contains_key("private-key-passphrase")
        && !map.contains_key("private-key")
    {
        return Err(INVALID.into());
    }
    if kind == "ss"
        && map["cipher"]
            .as_str()
            .is_some_and(|method| matches!(method, "none" | "dummy" | "plain"))
    {
        return Err("cipher：不接受无加密的 SS 节点".into());
    }
    if matches!(kind, "http" | "socks5" | "gost-relay")
        && map.contains_key("password")
        && !map.contains_key("username")
    {
        return Err(INVALID.into());
    }
    Ok(Value::Object(map))
}

pub fn encode(map: Map<String, Value>) -> Result<Zeroizing<String>, String> {
    let canonical = validate(Value::Object(map))?;
    let data = Zeroizing::new(format!("{PREFIX}{canonical}"));
    if data.len() > 16384 {
        return Err("节点配置超过 16 KiB".into());
    }
    Ok(data)
}

pub fn restore(stored: &str) -> Result<Value, String> {
    if stored.len() > 16384 {
        return Err(INVALID.into());
    }
    validate(
        serde_json::from_str(stored.strip_prefix(PREFIX).ok_or(INVALID)?).map_err(|_| INVALID)?,
    )
}

pub fn for_route(stored: &str) -> Result<Value, String> {
    use muse_proxy::{FixedNode, NodeKind, ProxyProtocol, Upstream};
    if stored.starts_with(PREFIX) {
        return restore(stored);
    }
    let upstream = Upstream::parse(stored).map_err(|_| INVALID)?;
    let mut node = json!({"server":upstream.host(),"port":upstream.port(),"type":upstream.protocol().scheme()});
    match upstream.protocol() {
        ProxyProtocol::Http
        | ProxyProtocol::Https
        | ProxyProtocol::Socks5
        | ProxyProtocol::Shadowsocks => {
            if upstream.protocol() == ProxyProtocol::Https {
                node["type"] = json!("http");
                node["tls"] = json!(true);
            }
            if let Some((user, pass)) = upstream.credentials() {
                node[if upstream.protocol() == ProxyProtocol::Shadowsocks {
                    "cipher"
                } else {
                    "username"
                }] = json!(user);
                if !pass.is_empty() {
                    node["password"] = json!(pass);
                }
            }
        }
        _ => {
            let old: FixedNode = serde_json::from_str(stored).map_err(|_| INVALID)?;
            if matches!(old.kind, NodeKind::Trojan) {
                node["sni"] = json!(old.server_name);
                node["password"] = json!(old.credential);
            } else {
                node["tls"] = json!(true);
                node["servername"] = json!(old.server_name);
                node["uuid"] = json!(old.credential);
                if !matches!(old.kind, NodeKind::Vless) {
                    node["alterId"] = json!(0);
                    node["cipher"] = json!(if matches!(old.kind, NodeKind::VmessAes128Gcm) {
                        "aes-128-gcm"
                    } else {
                        "chacha20-poly1305"
                    });
                }
            }
            if let Some(ws) = &old.websocket {
                node["network"] = json!("ws");
                node["ws-opts"] = json!({"path":ws.path,"headers":{"Host":ws.authority}});
            }
        }
    }
    validate(node)
}
