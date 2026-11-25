//! Bounded static-proxy import. Never return source lines or credentials in errors.
use crate::proxy_settings::ProxyDraft;
use muse_proxy::ProxyProtocol;
use std::{collections::HashSet, io::Read, path::Path};
use zeroize::Zeroizing;

pub const MAX_BYTES: usize = 1024 * 1024;
pub const MAX_ROWS: usize = 1000;
pub const LIMIT_MESSAGE: &str = "每批最多 1 MiB、1000 个非空条目；请拆分后重试，未截断或保存";
pub type ProxyKey = [u8; 32];

pub enum RowState {
    Ready(ProxyDraft),
    Duplicate,
    Invalid(String),
}

pub struct PreviewRow {
    pub line: usize,
    pub endpoint: String,
    pub authenticated: bool,
    pub state: RowState,
}

pub struct Preview {
    pub rows: Vec<PreviewRow>,
    pub ready: usize,
    pub duplicates: usize,
    pub errors: usize,
}

pub fn read_txt(path: &Path) -> Result<Zeroizing<String>, String> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))
    {
        return Err("请选择 UTF-8 编码的 .txt 文件".into());
    }
    let file = std::fs::File::open(path).map_err(|_| "无法读取 TXT 文件")?;
    if file.metadata().map_err(|_| "无法读取 TXT 文件")?.len() > MAX_BYTES as u64 {
        return Err(LIMIT_MESSAGE.into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取 TXT 文件")?;
    if bytes.len() > MAX_BYTES {
        return Err(LIMIT_MESSAGE.into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "TXT 必须使用 UTF-8 编码")?;
    Ok(Zeroizing::new(text.to_owned()))
}

pub fn validate_size(text: &str) -> Result<(), String> {
    if text.len() > MAX_BYTES
        || text.lines().filter(|line| !line.trim().is_empty()).count() > MAX_ROWS
    {
        return Err(LIMIT_MESSAGE.into());
    }
    Ok(())
}

fn parse_line(line: &str, default: ProxyProtocol) -> Result<ProxyDraft, String> {
    if line.len() > 16384 {
        return Err("单行超过 16384 字节，请缩短后重试".into());
    }
    // CRLF is framing; whitespace inside credentials is data, not formatting.
    let line = line.trim_start_matches([' ', '\t']);
    let explicit_url = line
        .split_once("://")
        .is_some_and(|(scheme, _)| !scheme.contains(':'));
    let draft = if explicit_url {
        let line = line.trim_end_matches([' ', '\t']);
        if line.chars().any(char::is_control) {
            return Err("代理地址不能包含控制字符".into());
        }
        ProxyDraft::from_url(line, true)?
    } else {
        let (host, rest) = if let Some(bracketed) = line.strip_prefix('[') {
            let (host, rest) = bracketed
                .split_once("]:")
                .ok_or("IPv6 请使用 [地址]:端口 或完整 URL")?;
            host.parse::<std::net::Ipv6Addr>()
                .map_err(|_| "IPv6 地址无效")?;
            (host, rest)
        } else {
            line.split_once(':')
                .ok_or("请提供 host:port、四段式或完整 URL")?
        };
        let mut parts = rest.splitn(3, ':');
        let port = parts.next().unwrap_or_default();
        let (user, secret) = match (parts.next(), parts.next()) {
            (None, None) => ("", ""),
            (Some(user), Some(secret)) if !user.is_empty() => (user, secret),
            _ => return Err("四段式需为 主机:端口:用户名:密码；IPv6 主机需加方括号".into()),
        };
        ProxyDraft {
            enabled: true,
            protocol: default,
            host: host.into(),
            port: port.into(),
            user: user.into(),
            secret: Zeroizing::new(secret.into()),
            force_proxy: true,
        }
    };
    // Reuse all existing credential/port validation and normalize aliases, IDNA,
    // IPv6, host casing and default ports before equality or preview.
    ProxyDraft::from_url(&draft.proxy_url()?, true)
}

pub fn key(draft: &ProxyDraft) -> Result<ProxyKey, String> {
    let canonical = ProxyDraft::from_url(&draft.proxy_url()?, true)?.proxy_url()?;
    let digest = ring::digest::digest(&ring::digest::SHA256, canonical.as_bytes());
    let mut key = [0; 32];
    key.copy_from_slice(digest.as_ref());
    Ok(key)
}

pub fn preview(
    text: &str,
    default: ProxyProtocol,
    mut seen: HashSet<ProxyKey>,
) -> Result<Preview, String> {
    validate_size(text)?;
    let mut preview = Preview {
        rows: Vec::new(),
        ready: 0,
        duplicates: 0,
        errors: 0,
    };
    for (index, raw) in text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .lines()
        .enumerate()
    {
        if raw.trim().is_empty() {
            continue;
        }
        let mut row = PreviewRow {
            line: index + 1,
            endpoint: String::new(),
            authenticated: false,
            state: RowState::Duplicate,
        };
        match parse_line(raw, default).and_then(|draft| key(&draft).map(|key| (draft, key))) {
            Ok((draft, key)) => {
                let host = if draft.host.contains(':') {
                    format!("[{}]", draft.host)
                } else {
                    draft.host.clone()
                };
                row.endpoint = format!("{}://{host}:{}", draft.protocol.scheme(), draft.port);
                row.authenticated = !draft.user.is_empty();
                if seen.insert(key) {
                    preview.ready += 1;
                    row.state = RowState::Ready(draft);
                } else {
                    preview.duplicates += 1;
                }
            }
            Err(error) => {
                preview.errors += 1;
                row.state = RowState::Invalid(error);
            }
        }
        preview.rows.push(row);
    }
    if preview.rows.is_empty() {
        return Err("请粘贴至少一条代理，或选择包含代理的 TXT 文件".into());
    }
    Ok(preview)
}
