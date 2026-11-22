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
pub(super) fn write(
    view: &WebView,
    item: &crate::cookie_import::ImportedCookie,
) -> Result<(), String> {
    let output = Arc::new(Mutex::new(None));
    let returned = output.clone();
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
        text("Secure", if item.secure { "TRUE" } else { "FALSE" });
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
        let cookie = cookie.ok_or("macOS 无法创建此 Cookie，未激活账号")?;
        let block = block2::RcBlock::new(move || {
            if let Ok(mut slot) = returned.lock() {
                *slot = Some(());
            }
        });
        let _: () = msg_send![&*store(view), setCookie: &*cookie, completionHandler: &*block];
        wait(&output)
    }
}
