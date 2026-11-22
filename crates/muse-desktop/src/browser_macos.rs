//! WKWebView adapter. Persistent stores and authenticated routes are set before navigation.
use super::*;
use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
};
use objc2_foundation::NSString;
use wry::WebViewExtMacOS;

pub(super) fn data_store_id(view: &WebView) -> Result<AccountId, String> {
    unsafe {
        let configuration: Retained<AnyObject> = msg_send![&*view.webview(), configuration];
        let store: Retained<AnyObject> = msg_send![&*configuration, websiteDataStore];
        let identifier: Option<Retained<AnyObject>> = msg_send![&*store, identifier];
        let identifier = identifier.ok_or("需要 macOS 14 或更新版本以隔离账号数据")?;
        let string: Retained<NSString> = msg_send![&*identifier, UUIDString];
        AccountId::parse_str(&string.to_string()).map_err(|_| "无法验证账号数据存储".into())
    }
}

#[link(name = "Network", kind = "framework")]
unsafe extern "C" {
    fn nw_endpoint_create_host(
        hostname: *const std::ffi::c_char,
        port: *const std::ffi::c_char,
    ) -> *mut AnyObject;
    fn nw_proxy_config_create_http_connect(
        endpoint: *mut AnyObject,
        tls_options: *mut AnyObject,
    ) -> *mut AnyObject;
    fn nw_proxy_config_set_username_and_password(
        config: *mut AnyObject,
        username: *const std::ffi::c_char,
        password: *const std::ffi::c_char,
    );
    fn nw_proxy_config_set_failover_allowed(config: *mut AnyObject, allowed: bool);
}

pub(super) fn install_proxy(view: &WebView, bridge: &ProxyBridge) -> Result<(), String> {
    use std::ffi::CString;
    let host = CString::new(bridge.address().ip().to_string()).map_err(|_| "本机代理地址无效")?;
    let port = CString::new(bridge.address().port().to_string()).map_err(|_| "本机代理端口无效")?;
    let username = CString::new(bridge.auth().username()).map_err(|_| "本机代理凭据无效")?;
    let password = CString::new(bridge.auth().secret()).map_err(|_| "本机代理凭据无效")?;
    // Network.framework objects are reference-counted Objective-C OS objects.
    // Credentials authenticate only to our account-owned loopback bridge; upstream
    // credentials never enter JavaScript, a command line, or an unauthenticated port.
    unsafe {
        let endpoint = Retained::from_raw(nw_endpoint_create_host(host.as_ptr(), port.as_ptr()))
            .ok_or("无法创建本机代理端点")?;
        let proxy = Retained::from_raw(nw_proxy_config_create_http_connect(
            Retained::as_ptr(&endpoint).cast_mut(),
            std::ptr::null_mut(),
        ))
        .ok_or("无法创建本机代理配置")?;
        nw_proxy_config_set_username_and_password(
            Retained::as_ptr(&proxy).cast_mut(),
            username.as_ptr(),
            password.as_ptr(),
        );
        nw_proxy_config_set_failover_allowed(Retained::as_ptr(&proxy).cast_mut(), false);
        let proxies: Retained<AnyObject> = msg_send![class!(NSArray), arrayWithObject: &*proxy];
        let configuration: Retained<AnyObject> = msg_send![&*view.webview(), configuration];
        let store: Retained<AnyObject> = msg_send![&*configuration, websiteDataStore];
        let _: () = msg_send![&*store, setProxyConfigurations: &*proxies];
    }
    Ok(())
}

pub(super) fn clear_proxy(view: &WebView) {
    // A persistent store may outlive its last view. Explicitly remove the old
    // account proxy when the user switches back to system networking.
    unsafe {
        let proxies: Retained<AnyObject> = msg_send![class!(NSArray), array];
        let configuration: Retained<AnyObject> = msg_send![&*view.webview(), configuration];
        let store: Retained<AnyObject> = msg_send![&*configuration, websiteDataStore];
        let _: () = msg_send![&*store, setProxyConfigurations: &*proxies];
    }
}

pub(super) fn zoom(view: &WebView) -> Option<f64> {
    let factor: f64 = unsafe { msg_send![&*view.webview(), pageZoom] };
    (factor.is_finite() && factor > 0.).then_some(factor)
}

pub(super) fn visible(view: &WebView) -> bool {
    let hidden: Bool = unsafe { msg_send![&*view.webview(), isHidden] };
    !hidden.as_bool()
}

pub(super) fn set_theme(view: &WebView, mode: muse_core::ThemeMode) {
    let name = NSString::from_str(match mode {
        muse_core::ThemeMode::Light => "NSAppearanceNameAqua",
        muse_core::ThemeMode::Dark => "NSAppearanceNameDarkAqua",
    });
    unsafe {
        let appearance: Retained<AnyObject> =
            msg_send![class!(NSAppearance), appearanceNamed: &*name];
        let _: () = msg_send![&*view.webview(), setAppearance: &*appearance];
    }
}

impl BrowserPool {
    // WKWebView has no public equivalent of WebView2.TrySuspend. Hidden views
    // remain available to studio tasks, and are never labelled as suspended.
    pub fn sleep_idle(&self, _: u64) -> bool {
        false
    }

    pub fn extract_cookies(&self, identity: BrowserIdentity, request: uuid::Uuid) -> bool {
        let Some(tab) = self.tabs.get(&identity.tab_id).filter(|tab| {
            tab.identity == identity
                && (self.cookie_only.contains(&identity)
                    || (tab.gate.lock().unwrap().ready()
                        && tab
                            .view
                            .url()
                            .ok()
                            .is_some_and(|url| muse_core::trusted_muse_url(&url))))
        }) else {
            return false;
        };
        let result = tab
            .view
            .cookies_for_url("https://muse.ai/")
            .map_err(|_| "无法读取 Muse Cookie".to_owned())
            .and_then(|cookies| {
                if cookies.len() > 512 {
                    return Err("Cookie 数量超过安全限制".into());
                }
                let items = cookies
                    .into_iter()
                    .filter_map(|cookie| {
                        let domain = cookie.domain().unwrap_or("muse.ai");
                        if !["muse.ai", ".muse.ai", "www.muse.ai", ".www.muse.ai"].contains(&domain)
                            || cookie.name().is_empty()
                            || cookie.name().len() > 256
                            || cookie.value().len() > 32768
                            || cookie.name().chars().any(char::is_control)
                            || cookie.value().contains(['\r', '\n'])
                        {
                            return None;
                        }
                        Some(crate::cookie_export::CookieItem {
                            name: cookie.name().into(),
                            value: cookie.value().into(),
                            domain: domain.into(),
                            path: cookie.path().unwrap_or("/").into(),
                            expires: cookie.expires_datetime().map(|date| date.unix_timestamp()),
                            http_only: cookie.http_only().unwrap_or(false),
                            secure: cookie.secure().unwrap_or(false),
                            same_site: cookie
                                .same_site()
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "Unspecified".into()),
                        })
                    })
                    .collect();
                Ok(Box::new(crate::cookie_export::CookieBundle {
                    items,
                    captured_at: page_observer::now_ms(),
                }))
            });
        let _ = self.sender.send(BrowserMessage::Cookies(
            identity.account_id,
            identity,
            request,
            result,
        ));
        true
    }

    pub fn capture_preview(&self, identity: BrowserIdentity, request: u64) -> bool {
        let Some(tab) = self
            .tabs
            .get(&identity.tab_id)
            .filter(|tab| tab.identity == identity)
        else {
            return false;
        };
        let sender = self.sender.clone();
        let callback =
            block2::RcBlock::new(move |image: *mut AnyObject, _error: *mut AnyObject| {
                let bytes = if image.is_null() {
                    None
                } else {
                    unsafe { snapshot_png(&*image) }
                };
                let sender = sender.clone();
                std::thread::spawn(move || {
                    let image = bytes.and_then(|bytes| decode_preview(&bytes));
                    let _ = sender.send(BrowserMessage::Preview(identity, request, image));
                });
            });
        unsafe {
            let _: () = msg_send![&*tab.view.webview(), takeSnapshotWithConfiguration: std::ptr::null::<AnyObject>(), completionHandler: &*callback];
        }
        true
    }
}

unsafe fn snapshot_png(image: &AnyObject) -> Option<Vec<u8>> {
    unsafe {
        let tiff: Option<Retained<AnyObject>> = msg_send![image, TIFFRepresentation];
        let tiff = tiff?;
        let bitmap: Option<Retained<AnyObject>> =
            msg_send![class!(NSBitmapImageRep), imageRepWithData: &*tiff];
        let bitmap = bitmap?;
        let options: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionary];
        let png: Option<Retained<AnyObject>> =
            msg_send![&*bitmap, representationUsingType: 4usize, properties: &*options];
        let png = png?;
        let length: usize = msg_send![&*png, length];
        if length == 0 || length > 64_000_000 {
            return None;
        }
        let pointer: *const u8 = msg_send![&*png, bytes];
        if pointer.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(pointer, length).to_vec())
    }
}
