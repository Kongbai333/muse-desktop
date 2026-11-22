//! Account-scoped Cookie login input. No credential values are included in errors.
use wry::cookie::{Cookie, SameSite};
use zeroize::Zeroizing;

const MAX_INPUT: usize = 1_000_000;
const MAX_COOKIES: usize = 512;

pub struct ImportedCookie {
    pub name: String,
    pub value: Zeroizing<String>,
    pub domain: String,
    pub path: String,
    pub expires: Option<i64>,
    pub http_only: bool,
    pub secure: bool,
    pub same_site: Option<SameSite>,
}

pub struct CookieImport {
    pub items: Vec<ImportedCookie>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginOutcome {
    Confirmed,
    Rejected,
    Unconfirmed,
}

pub fn login_outcome(quota: &muse_core::QuotaState, elapsed_ms: u64) -> Option<LoginOutcome> {
    use muse_core::QuotaState;
    match quota {
        QuotaState::Known(_) | QuotaState::Unlimited { .. } => Some(LoginOutcome::Confirmed),
        QuotaState::LoggedOut if elapsed_ms >= 2_000 => Some(LoginOutcome::Rejected),
        _ if elapsed_ms >= 30_000 => Some(LoginOutcome::Unconfirmed),
        _ => None,
    }
}

pub fn parse(input: &str, origin: &str, now: i64) -> Result<CookieImport, &'static str> {
    if input.len() > MAX_INPUT {
        return Err("Cookie 数据过多，请只导入一个账号");
    }
    let origin = url::Url::parse(origin).map_err(|_| "登录网站无效")?;
    let host = origin.host_str().ok_or("登录网站无效")?;
    let muse = muse_core::trusted_muse_url(origin.as_str());
    let diagnostic = origin.scheme() == "http"
        && (host == "localhost"
            || host == "muse-proxy-smoke.test"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()));
    if !muse && !diagnostic {
        return Err("不支持的登录网站");
    }
    let secure = origin.scheme() == "https";
    let input = input.trim();
    if input.is_empty() {
        return Err("请粘贴此账号的 Cookie");
    }
    let mut items = if input.starts_with('[') || input.starts_with('{') {
        let value: serde_json::Value =
            serde_json::from_str(input).map_err(|_| "Cookie JSON 格式无效")?;
        if value.as_object().is_some_and(|object| {
            object.keys().any(|key| {
                ["proxy", "proxies", "network", "accounts"]
                    .iter()
                    .any(|field| key.eq_ignore_ascii_case(field))
            })
        }) {
            return Err("此入口只导入单账号 Cookie，不读取代理配置；请单独选择原账号的网络连接");
        }
        let values = value
            .as_array()
            .or_else(|| value.get("cookies").and_then(|v| v.as_array()))
            .ok_or("需要 Cookie 数组，或包含 cookies 数组的 JSON")?;
        if values.len() > MAX_COOKIES {
            return Err("Cookie 数量过多，请只导入一个账号");
        }
        values
            .iter()
            .map(|value| parse_json_cookie(value, host, secure))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        if input.contains(['\r', '\n']) {
            return Err("一次只能导入一个账号，请粘贴单账号 Cookie 文本或 JSON");
        }
        let input = if input
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cookie:"))
        {
            input[7..].trim()
        } else {
            input
        };
        let pairs = input.split(';').filter(|pair| !pair.trim().is_empty());
        let mut items = Vec::new();
        for pair in pairs {
            if items.len() >= MAX_COOKIES {
                return Err("Cookie 数量过多，请只导入一个账号");
            }
            let (name, value) = pair.split_once('=').ok_or("Cookie 需要 name=value 格式")?;
            let name = name.trim();
            items.push(ImportedCookie {
                name: name.into(),
                value: Zeroizing::new(value.trim().into()),
                domain: host.into(),
                path: "/".into(),
                expires: None,
                http_only: name == "hatch_sess",
                secure,
                same_site: None,
            });
        }
        items
    };
    let mut keys = std::collections::HashSet::new();
    let mut session: Option<&str> = None;
    for item in &items {
        let domain = item.domain.strip_prefix('.').unwrap_or(&item.domain);
        let allowed = if muse {
            ["muse.ai", "www.muse.ai"].contains(&domain)
        } else {
            domain == host
        };
        if !allowed {
            return Err("Cookie 包含其他网站的数据，请只导入 Muse 账号");
        }
        if item.name.is_empty()
            || item.name.len() > 256
            || !item
                .name
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !b"()<>@,;:\\\"/[]?={}".contains(&byte))
            || item.value.len() > 32768
            || item.value.chars().any(|ch| ch.is_control() || ch == ';')
            || !item.path.starts_with('/')
            || item.path.len() > 2048
            || item.path.chars().any(char::is_control)
        {
            return Err("Cookie 的名称、值或路径无效");
        }
        if item.same_site == Some(SameSite::None) && !item.secure {
            return Err("SameSite=None 的 Cookie 必须启用 Secure");
        }
        if !keys.insert((&item.name, domain, &item.path)) {
            return Err("Cookie 有重复项，请只导入一个账号");
        }
        if item.name == "hatch_sess" {
            if item.value.is_empty()
                || item.value.bytes().any(|byte| byte <= 32 || byte >= 127)
                || item.expires.is_some_and(|expires| expires <= now)
            {
                return Err("登录 Cookie 为空或已过期，请重新登录后导出");
            }
            if session.is_some_and(|value| value != item.value.as_str()) {
                return Err("Cookie 包含多个登录会话，请只导入一个账号");
            }
            session = Some(&item.value);
        }
    }
    if session.is_none() {
        return Err("缺少登录 Cookie hatch_sess，请确认复制了完整的 Muse Cookie");
    }
    items.retain(|item| item.expires.is_none_or(|expires| expires > now));
    Ok(CookieImport { items })
}

fn parse_json_cookie(
    value: &serde_json::Value,
    host: &str,
    default_secure: bool,
) -> Result<ImportedCookie, &'static str> {
    let text = |key: &str| -> Result<Option<&str>, &'static str> {
        match value.get(key) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(value)) => Ok(Some(value)),
            _ => Err("Cookie JSON 的字段类型无效"),
        }
    };
    let boolean = |camel: &str, snake: &str, default: bool| -> Result<bool, &'static str> {
        match value.get(camel).or_else(|| value.get(snake)) {
            None | Some(serde_json::Value::Null) => Ok(default),
            Some(serde_json::Value::Bool(value)) => Ok(*value),
            _ => Err("Cookie JSON 的安全标记无效"),
        }
    };
    let name = text("name")?.ok_or("Cookie 缺少名称")?;
    let secret = text("value")?.ok_or("Cookie 缺少值")?;
    let expires = if boolean("session", "session", false)? {
        None
    } else {
        match value.get("expirationDate").or_else(|| value.get("expires")) {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => {
                let seconds = value.as_f64().ok_or("Cookie 的有效期无效")?;
                if !seconds.is_finite() || !(-1.0..=253_402_300_799.0).contains(&seconds) {
                    return Err("Cookie 的有效期无效");
                }
                (seconds >= 0.0).then_some(seconds.floor() as i64)
            }
        }
    };
    let same_site = match value.get("sameSite").or_else(|| value.get("same_site")) {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(value)) => match value.to_ascii_lowercase().as_str() {
            "" | "unspecified" => None,
            "lax" => Some(SameSite::Lax),
            "strict" => Some(SameSite::Strict),
            "none" | "no_restriction" => Some(SameSite::None),
            _ => return Err("Cookie 的 SameSite 标记无效"),
        },
        _ => return Err("Cookie 的 SameSite 标记无效"),
    };
    Ok(ImportedCookie {
        name: name.into(),
        value: Zeroizing::new(secret.into()),
        domain: text("domain")?.unwrap_or(host).to_ascii_lowercase(),
        path: text("path")?.unwrap_or("/").into(),
        expires,
        http_only: boolean("httpOnly", "http_only", name == "hatch_sess")?,
        secure: boolean("secure", "secure", default_secure)?,
        same_site,
    })
}

impl ImportedCookie {
    pub fn native(&self) -> Result<Cookie<'static>, &'static str> {
        let mut cookie = Cookie::build((self.name.clone(), self.value.to_string()))
            .domain(self.domain.clone())
            .path(self.path.clone())
            .http_only(self.http_only)
            .secure(self.secure);
        if let Some(expires) = self.expires {
            let expires = wry::cookie::time::OffsetDateTime::from_unix_timestamp(expires)
                .map_err(|_| "Cookie 的有效期无效")?;
            cookie = cookie.expires(expires);
        }
        if let Some(same_site) = self.same_site {
            cookie = cookie.same_site(same_site);
        }
        Ok(cookie.build())
    }

    pub fn matches(&self, cookie: &Cookie<'_>) -> bool {
        cookie.name() == self.name
            && cookie.value() == self.value.as_str()
            && cookie.domain() == Some(self.domain.strip_prefix('.').unwrap_or(&self.domain))
            && cookie.path().unwrap_or("/") == self.path
            && cookie.http_only().unwrap_or(false) == self.http_only
            && cookie.secure().unwrap_or(false) == self.secure
            && match (self.expires, cookie.expires_datetime()) {
                (None, None) => true,
                (Some(expected), Some(actual)) => expected.abs_diff(actual.unix_timestamp()) <= 1,
                _ => false,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW: i64 = 1_800_000_000;

    #[test]
    fn an_unknown_session_is_never_reported_as_a_successful_login() {
        use muse_core::QuotaState;
        assert_eq!(login_outcome(&QuotaState::Unknown, 100), None);
        assert_eq!(
            login_outcome(&QuotaState::Unknown, 31_000),
            Some(LoginOutcome::Unconfirmed)
        );
        assert_eq!(login_outcome(&QuotaState::LoggedOut, 100), None);
        assert_eq!(
            login_outcome(&QuotaState::LoggedOut, 3_000),
            Some(LoginOutcome::Rejected)
        );
        assert_eq!(
            login_outcome(&QuotaState::Unlimited { observed_at: 1 }, 100),
            Some(LoginOutcome::Confirmed)
        );
    }

    #[test]
    fn cookie_values_can_contain_pipes_but_not_multiple_flattened_sessions() {
        let valid = parse("hatch_sess=synthetic|value==", "https://muse.ai/", NOW).unwrap();
        assert_eq!(valid.items[0].value.as_str(), "synthetic|value==");
        assert!(
            parse(
                "hatch_sess=first hatch_sess=second",
                "https://muse.ai/",
                NOW
            )
            .is_err()
        );
    }

    #[test]
    fn imports_an_exported_header_without_decoding_or_truncating_its_session() {
        let parsed = parse(
            "Cookie: hatch_sess=synthetic%2Fsession==; hatch_vml=synthetic-workspace",
            "https://muse.ai/",
            NOW,
        )
        .unwrap();
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].name, "hatch_sess");
        assert_eq!(parsed.items[0].value.as_str(), "synthetic%2Fsession==");
        assert_eq!(parsed.items[0].domain, "muse.ai");
        assert_eq!(parsed.items[0].path, "/");
        assert!(parsed.items[0].http_only && parsed.items[0].secure);
        assert_eq!(parsed.items[0].expires, None);
    }

    #[test]
    fn imports_browser_json_with_domain_path_expiry_and_security_attributes() {
        let parsed = parse(
            r#"[{"name":"hatch_sess","value":"synthetic-json","domain":".muse.ai","path":"/","expirationDate":1800003600.75,"httpOnly":true,"secure":true,"sameSite":"lax"}]"#,
            "https://muse.ai/",
            NOW,
        )
        .unwrap();
        let item = &parsed.items[0];
        assert_eq!(item.expires, Some(1_800_003_600));
        assert_eq!(item.domain, ".muse.ai");
        assert_eq!(item.same_site, Some(SameSite::Lax));
        assert!(item.http_only && item.secure);
    }

    #[test]
    fn browser_session_cookies_do_not_gain_an_invented_expiry() {
        let parsed = parse(
            r#"{"cookies":[{"name":"hatch_sess","value":"synthetic-session","domain":"muse.ai","session":true,"expirationDate":-1,"httpOnly":true,"secure":true}]}"#,
            "https://muse.ai/",
            NOW,
        )
        .unwrap();
        assert_eq!(parsed.items[0].expires, None);
    }

    #[test]
    fn rejects_expired_foreign_or_conflicting_login_cookies_without_echoing_values() {
        for input in [
            "hatch_vml=synthetic-private-value",
            "hatch_sess=",
            "hatch_sess=synthetic-private-value; hatch_sess=another-account",
            "hatch_sess=synthetic-private-value\r\nX-Header: injected",
            r#"[{"name":"hatch_sess","value":"synthetic-private-value","domain":"evil-muse.ai"}]"#,
            r#"[{"name":"hatch_sess","value":"synthetic-private-value","domain":"muse.ai","expires":1799999999}]"#,
            r#"[{"name":"hatch_sess","value":"synthetic-private-value","domain":"muse.ai","path":"not-a-path"}]"#,
            r#"[{"name":"hatch_sess","value":"synthetic-private-value","domain":"muse.ai","secure":false,"sameSite":"no_restriction"}]"#,
        ] {
            let error = parse(input, "https://muse.ai/", NOW).err().unwrap();
            assert!(!error.contains("synthetic-private-value"));
        }
    }

    #[test]
    fn cookie_scope_comes_from_the_caller_not_from_the_imported_text() {
        let parsed = parse("hatch_sess=synthetic-local", "http://127.0.0.1:12345/", NOW).unwrap();
        assert_eq!(parsed.items[0].domain, "127.0.0.1");
        assert!(!parsed.items[0].secure);
        assert!(
            parse(
                "hatch_sess=synthetic-local",
                "https://unrelated.example/",
                NOW
            )
            .is_err()
        );
        assert!(
            parse(
                r#"[{"name":"hatch_sess","value":"synthetic-local","domain":"127.0.0.1"}]"#,
                "https://muse.ai/",
                NOW,
            )
            .is_err()
        );
    }

    #[test]
    fn a_cookie_only_entry_does_not_silently_ignore_embedded_account_or_proxy_routes() {
        for key in ["proxy", "proxies", "network", "accounts"] {
            let mut value =
                serde_json::json!({"cookies":[{"name":"hatch_sess","value":"synthetic-session"}]});
            value
                .as_object_mut()
                .unwrap()
                .insert(key.into(), serde_json::json!({"id":"synthetic-reference"}));
            assert!(parse(&value.to_string(), "https://muse.ai/", NOW).is_err());
        }
    }

    #[test]
    fn oversized_and_multiple_account_inputs_are_rejected_before_any_browser_write() {
        let large = format!("hatch_sess={}", "x".repeat(1_000_001));
        assert!(parse(&large, "https://muse.ai/", NOW).is_err());
        assert!(
            parse(
                "first | hatch_sess=synthetic-first\nsecond | hatch_sess=synthetic-second",
                "https://muse.ai/",
                NOW,
            )
            .is_err()
        );
    }
}
