//! Explicit native Cookie mappings used only by encrypted account packages.
use crate::{
    account_transfer as wire,
    cookie_import::{CookieImport, ImportedCookie},
};
use zeroize::Zeroizing;

pub fn validate(set: &wire::CookieSet, now: i64) -> Result<(), String> {
    let payload = wire::Payload {
        format: "muse-account-payload".into(),
        version: 1,
        proxies: vec![],
        accounts: vec![wire::Account {
            account_ref: "candidate".into(),
            name: "候选账号".into(),
            network: wire::Network {
                mode: "system".into(),
                proxy_ref: None,
                bypass_policy: "platform".into(),
                source_force_proxy: None,
            },
            cookies: set.clone(),
        }],
    };
    wire::validate(&payload, now).map_err(|e| e.message().to_owned())
}
pub fn native_domain(cookie: &wire::Cookie) -> Result<String, String> {
    let source = url::Url::parse(&cookie.source_url).map_err(|_| "Cookie 来源无效")?;
    let raw = cookie
        .domain
        .as_deref()
        .or_else(|| source.host_str())
        .ok_or("Cookie 域无效")?;
    let bare = raw.trim_start_matches('.');
    Ok(match cookie.host_only {
        Some(true) => bare.to_owned(),
        Some(false) => format!(".{bare}"),
        None => raw.to_owned(),
    })
}
pub fn for_import(set: &wire::CookieSet, now: i64) -> Result<CookieImport, String> {
    validate(set, now)?;
    let mut items = Vec::new();
    for cookie in &set.items {
        if cookie.expires_at_unix_seconds.is_some_and(|e| e <= now) {
            continue;
        }
        items.push(ImportedCookie {
            name: cookie.name.clone(),
            value: Zeroizing::new(cookie.value.clone()),
            domain: native_domain(cookie)?,
            path: cookie.path.clone().unwrap_or_else(|| "/".into()),
            expires: cookie.expires_at_unix_seconds,
            secure: cookie.secure.unwrap_or(true),
            http_only: cookie.http_only.unwrap_or(cookie.name == "hatch_sess"),
            same_site: match cookie.same_site.as_deref() {
                Some("lax") => Some(wry::cookie::SameSite::Lax),
                Some("strict") => Some(wry::cookie::SameSite::Strict),
                Some("none") => Some(wry::cookie::SameSite::None),
                _ => None,
            },
        });
    }
    Ok(CookieImport { items })
}
pub fn scoped(mut items: Vec<wire::Cookie>) -> Result<wire::CookieSet, String> {
    let now = (crate::page_observer::now_ms() / 1000) as i64;
    items.retain(|c| c.expires_at_unix_seconds.is_none_or(|e| e > now));
    let coverage_urls = items
        .iter()
        .map(|c| c.source_url.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let set = wire::CookieSet {
        fidelity: "attributes".into(),
        coverage_urls,
        items,
    };
    validate(&set, now)?;
    Ok(set)
}
pub fn source_url(domain: &str, path: &str) -> Result<String, String> {
    let mut url = url::Url::parse(&format!("https://{}/", domain.trim_start_matches('.')))
        .map_err(|_| "Cookie 来源无效")?;
    url.set_path(path);
    Ok(url.into())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn cookies() -> crate::account_transfer::CookieSet {
        crate::account_transfer::decode_payload(include_bytes!(
            "../../../checks/fixtures/account-transfer-v1/payload.json"
        ))
        .unwrap()
        .accounts
        .remove(0)
        .cookies
    }
    #[test]
    fn retains_domain_and_path_scopes_without_inventing_lifetime_or_decoding_values() {
        let mut set = cookies();
        let mut second = set.items[0].clone();
        second.path = Some("/api".into());
        second.session = Some(true);
        second.expires_at_unix_seconds = None;
        set.items.push(second);
        let parsed = for_import(&set, 1_800_000_000).unwrap();
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(
            parsed.items[0].value.as_str(),
            "SYNTHETIC_SESSION==%2F%3D%25+ONLY"
        );
        assert_eq!(parsed.items[1].expires, None);
        assert_eq!(parsed.items[1].path, "/api");
        assert_eq!(native_domain(&set.items[0]).unwrap(), ".muse.ai");
        set.items[0].host_only = Some(true);
        assert_eq!(native_domain(&set.items[0]).unwrap(), "muse.ai");
        set.items[0].domain = Some("muse.ai".into());
        set.items[0].host_only = Some(false);
        assert_eq!(native_domain(&set.items[0]).unwrap(), ".muse.ai");
    }
    #[test]
    fn unknown_header_attributes_map_explicitly_to_https_root_and_session() {
        let mut set = cookies();
        set.fidelity = "header".into();
        let cookie = &mut set.items[0];
        cookie.domain = None;
        cookie.path = None;
        cookie.secure = None;
        cookie.http_only = None;
        cookie.same_site = None;
        cookie.session = None;
        cookie.host_only = None;
        cookie.expires_at_unix_seconds = None;
        let parsed = for_import(&set, 1_800_000_000).unwrap();
        assert_eq!(parsed.items[0].domain, "muse.ai");
        assert_eq!(parsed.items[0].path, "/");
        assert!(parsed.items[0].secure && parsed.items[0].http_only);
        assert_eq!(parsed.items[0].same_site, None);
        assert_eq!(parsed.items[0].expires, None);
    }
    #[test]
    fn expired_login_or_unsupported_partitioned_input_never_becomes_fresh() {
        let mut set = cookies();
        set.items[0].expires_at_unix_seconds = Some(100);
        assert!(for_import(&set, 1_800_000_000).is_err());
        let source = include_str!("../../../checks/fixtures/account-transfer-v1/payload.json")
            .replace(
                "\"host_only\":null",
                "\"host_only\":null,\"partitioned\":true",
            );
        assert!(crate::account_transfer::decode_payload(source.as_bytes()).is_err());
    }
}
