//! WKHTTPCookieStore migration adapter. Built on macOS only; no assumed host-only metadata.
use super::*;
use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
};
use objc2_foundation::{NSArray, NSString};
use std::ptr::NonNull;
use wry::WebViewExtMacOS;

fn store(view: &WebView) -> Retained<AnyObject> {
    unsafe {
        let config: Retained<AnyObject> = msg_send![&*view.webview(), configuration];
        let data: Retained<AnyObject> = msg_send![&*config, websiteDataStore];
        msg_send![&*data, httpCookieStore]
    }
}
fn wait<T>(output: &Arc<Mutex<Option<T>>>) -> Result<T, String> {
    let started = std::time::Instant::now();
    loop {
        if let Some(value) = output.lock().map_err(|_| "原生 Cookie 回调失败")?.take() {
            return Ok(value);
        }
        if started.elapsed() > std::time::Duration::from_secs(15) {
            return Err("等待原生 Cookie 回调超时，未激活账号".into());
        }
        unsafe {
            let run_loop: Retained<AnyObject> = msg_send![class!(NSRunLoop), currentRunLoop];
            let date: Retained<AnyObject> =
                msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: 0.01f64];
            let _: () = msg_send![&*run_loop, runUntilDate: &*date];
        }
    }
}
pub(super) fn read(view: &WebView) -> Result<Vec<wire::Cookie>, String> {
    let output = Arc::new(Mutex::new(None));
    let returned = output.clone();
    let block = block2::RcBlock::new(move |cookies: NonNull<NSArray<AnyObject>>| {
        let result = (|| -> Result<Vec<wire::Cookie>, String> {
            let array = unsafe { cookies.as_ref() };
            if array.len() > 4096 {
                return Err("Cookie 数量超过安全限制".into());
            }
            let mut items = Vec::new();
            for c in array.to_vec() {
                unsafe {
                    let domain: Retained<NSString> = msg_send![&*c, domain];
                    let domain = domain.to_string();
                    if !["muse.ai", ".muse.ai", "www.muse.ai", ".www.muse.ai"]
                        .contains(&domain.as_str())
                    {
                        continue;
                    }
                    let name: Retained<NSString> = msg_send![&*c, name];
                    let value: Retained<NSString> = msg_send![&*c, value];
                    let path: Retained<NSString> = msg_send![&*c, path];
                    let path = path.to_string();
                    let secure: Bool = msg_send![&*c, isSecure];
                    let http: Bool = msg_send![&*c, isHTTPOnly];
                    let session: Bool = msg_send![&*c, isSessionOnly];
                    let expires: Option<Retained<AnyObject>> = msg_send![&*c, expiresDate];
                    let expires = expires.map(|date| {
                        let seconds: f64 = msg_send![&*date, timeIntervalSince1970];
                        seconds as i64
                    });
                    let same_site: Option<Retained<NSString>> = msg_send![&*c, sameSitePolicy];
                    items.push(wire::Cookie {
                        source_url: transfer_cookies::source_url(&domain, &path)?,
                        name: name.to_string(),
                        value: value.to_string(),
                        domain: Some(domain),
                        path: Some(path),
                        expires_at_unix_seconds: expires,
                        secure: Some(secure.as_bool()),
                        http_only: Some(http.as_bool()),
                        same_site: same_site.map(|s| s.to_string().to_ascii_lowercase()),
                        session: Some(session.as_bool()),
                        host_only: None,
                    });
                }
            }
            Ok(items)
        })();
        if let Ok(mut slot) = returned.lock() {
            *slot = Some(result);
        }
    });
    unsafe {
        let _: () = msg_send![&*store(view), getAllCookies: &*block];
    }
    wait(&output)?
}
fn native_cookie(
    item: &crate::cookie_import::ImportedCookie,
) -> Result<Retained<AnyObject>, String> {
    unsafe {
        let properties: Retained<AnyObject> = msg_send![class!(NSMutableDictionary), dictionary];
        let text = |key: &str, value: &str| {
            let key = NSString::from_str(key);
            let value = NSString::from_str(value);
            let _: () = msg_send![&*properties, setObject: &*value, forKey: &*key];
        };
        text("Name", &item.name);
        text("Value", &item.value);
        text("Domain", &item.domain);
        text("Path", &item.path);
        // NSHTTPCookieSecure is a presence flag: even "FALSE" means secure.
        if item.secure {
            text("Secure", "TRUE");
        }
        text("HttpOnly", if item.http_only { "TRUE" } else { "FALSE" });
        // Matches Wry 0.57.1's Foundation dictionary mapping. macOS native read-back is mandatory;
        // Windows tests do not establish that these properties are accepted by WKHTTPCookieStore.
        if let Some(site) = item.same_site {
            text("SameSite", &site.to_string().to_ascii_lowercase());
        }
        if let Some(expiry) = item.expires {
            let date: Retained<AnyObject> =
                msg_send![class!(NSDate), dateWithTimeIntervalSince1970: expiry as f64];
            let key = NSString::from_str("Expires");
            let _: () = msg_send![&*properties, setObject: &*date, forKey: &*key];
        } else {
            text("Discard", "TRUE");
        }
        let cookie: Option<Retained<AnyObject>> =
            msg_send![class!(NSHTTPCookie), cookieWithProperties: &*properties];
        cookie.ok_or_else(|| "macOS 无法创建此 Cookie，未激活账号".into())
    }
}

pub(super) fn write(
    view: &WebView,
    item: &crate::cookie_import::ImportedCookie,
) -> Result<(), String> {
    let cookie = native_cookie(item)?;
    let output = Arc::new(Mutex::new(None));
    let returned = output.clone();
    unsafe {
        let block = block2::RcBlock::new(move || {
            if let Ok(mut slot) = returned.lock() {
                *slot = Some(());
            }
        });
        let _: () = msg_send![&*store(view), setCookie: &*cookie, completionHandler: &*block];
        wait(&output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foundation_cookie_keeps_both_secure_values_and_other_imported_attributes() {
        let expires = (crate::page_observer::now_ms() / 1000 + 3600) as i64;
        for secure in [false, true] {
            let item = crate::cookie_import::ImportedCookie {
                name: "hatch_sess".into(),
                value: zeroize::Zeroizing::new("synthetic-foundation-flag".into()),
                domain: "muse.ai".into(),
                path: "/api".into(),
                expires: Some(expires),
                http_only: true,
                secure,
                same_site: Some(wry::cookie::SameSite::Lax),
            };
            let cookie = native_cookie(&item).unwrap();
            unsafe {
                let actual_secure: Bool = msg_send![&*cookie, isSecure];
                assert_eq!(actual_secure.as_bool(), secure);
                let http_only: Bool = msg_send![&*cookie, isHTTPOnly];
                assert!(http_only.as_bool());
                let path: Retained<NSString> = msg_send![&*cookie, path];
                assert_eq!(path.to_string(), "/api");
                let site: Option<Retained<NSString>> = msg_send![&*cookie, sameSitePolicy];
                assert_eq!(site.unwrap().to_string(), "lax");
                let expiry: Option<Retained<AnyObject>> = msg_send![&*cookie, expiresDate];
                let seconds: f64 = msg_send![&*expiry.unwrap(), timeIntervalSince1970];
                assert!((seconds - expires as f64).abs() <= 1.);
            }
        }
    }
}
