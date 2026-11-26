//! Immutable account-owned browser entry point. Native paths never start a relay.
use muse_proxy::trace::{Context, Event, Outcome, RoutePath, Stage};
use muse_proxy::{ProxyBridge, ProxyProtocol, Upstream};
use std::sync::Arc;
use url::Url;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
    ICoreWebView2_10, ICoreWebView2_22,
};
use webview2_com::{
    BasicAuthenticationRequestedEventHandler, WebResourceRequestedEventHandler, take_pwstr,
};
use windows::core::{HSTRING, Interface, PWSTR};
use wry::{WebView, WebViewExtWindows};

pub struct AccountRoute {
    endpoint: Url,
    upstream: Upstream,
    bridge: Option<Arc<ProxyBridge>>,
    context: Context,
}

impl AccountRoute {
    pub fn start(upstream: Upstream, mut context: Context) -> Result<Self, String> {
        // 111 fixed proxy challenge URIs. Use a conservative 122.0.2365.46
        // floor and require the all-source resource-filter interface below.
        let scoped_auth = wry::webview_version()
            .ok()
            .is_some_and(|v| scoped_auth_runtime(&v));
        let authenticated = upstream.credentials().is_some();
        let via_system =
            muse_proxy::validate_system_upstream(&upstream).map_err(|e| e.to_string())?;
        let path = if via_system {
            match upstream.protocol() {
                ProxyProtocol::Http => RoutePath::SystemHttpBridge,
                ProxyProtocol::Https => RoutePath::SystemHttpsBridge,
                ProxyProtocol::Socks5 => RoutePath::SystemSocks5Bridge,
            }
        } else {
            match (upstream.protocol(), authenticated, scoped_auth) {
                (ProxyProtocol::Socks5, true, _) => RoutePath::Socks5AuthBridge,
                (ProxyProtocol::Http, true, false) => RoutePath::HttpAuthBridge,
                (ProxyProtocol::Https, true, false) => RoutePath::HttpsAuthBridge,
                (ProxyProtocol::Http, _, _) => RoutePath::NativeHttp,
                (ProxyProtocol::Https, _, _) => RoutePath::NativeHttps,
                (ProxyProtocol::Socks5, false, _) => RoutePath::NativeSocks5,
            }
        };
        context.route = upstream.trace_route_id();
        context.protocol = Some(upstream.protocol());
        context.route_path = Some(path);
        context.transport = Some(if via_system {
            muse_proxy::trace::Transport::SystemLoopback
        } else {
            muse_proxy::trace::Transport::DirectToAccountProxy
        });
        let host = if upstream.host().contains(':') {
            format!("[{}]", upstream.host())
        } else {
            upstream.host().to_owned()
        };
        let endpoint = Url::parse(&format!(
            "{}://{host}:{}/",
            upstream.protocol().scheme(),
            upstream.port()
        ))
        .map_err(|_| "代理端点无效")?;
        let bridge = if matches!(
            path,
            RoutePath::Socks5AuthBridge
                | RoutePath::HttpAuthBridge
                | RoutePath::HttpsAuthBridge
                | RoutePath::SystemHttpBridge
                | RoutePath::SystemHttpsBridge
                | RoutePath::SystemSocks5Bridge
        ) {
            let bridge = Arc::new(
                ProxyBridge::start_traced(upstream.clone(), context).map_err(|e| e.to_string())?,
            );
            context.bridge = bridge.trace_context().bridge;
            Some(bridge)
        } else {
            None
        };
        Event::new(context, Stage::RouteSelected, Outcome::Ok).emit();
        Ok(Self {
            endpoint,
            upstream,
            bridge,
            context,
        })
    }

    pub fn is_running(&self) -> bool {
        self.bridge.as_ref().is_none_or(|b| b.is_running())
    }
    pub fn trace_context(&self) -> Context {
        self.context
    }
    pub fn request_count(&self) -> Option<u64> {
        self.bridge.as_ref().map(|b| b.request_count())
    }
    pub fn label(&self) -> &'static str {
        match self.context.route_path {
            Some(RoutePath::SystemHttpBridge) => "系统回环前置 → 账号 HTTP 代理",
            Some(RoutePath::SystemHttpsBridge) => "系统回环前置 → 账号 HTTPS 代理",
            Some(RoutePath::SystemSocks5Bridge) => "系统回环前置 → 账号 SOCKS5 代理",
            Some(RoutePath::NativeHttp) => "WebView2 原生 HTTP",
            Some(RoutePath::NativeHttps) => "WebView2 原生 HTTPS",
            Some(RoutePath::NativeSocks5) => "WebView2 原生 SOCKS5（无认证）",
            Some(RoutePath::Socks5AuthBridge) => "认证 SOCKS5 兼容桥",
            Some(RoutePath::HttpAuthBridge) => "认证 HTTP 兼容桥（旧运行时）",
            Some(RoutePath::HttpsAuthBridge) => "认证 HTTPS 兼容桥（旧运行时）",
            None => "代理路由未创建",
        }
    }
    pub fn arguments(&self, force: bool) -> String {
        // No DIRECT/PAC fallback and no credentials in this endpoint.
        let endpoint = self
            .bridge
            .as_ref()
            .map(|b| format!("http://{}", b.address()))
            .unwrap_or_else(|| self.endpoint.as_str().trim_end_matches('/').to_owned());
        format!(
            "--proxy-server={endpoint}{}",
            if force {
                " --proxy-bypass-list=<-loopback>"
            } else {
                ""
            }
        )
    }

    /// Reused for each normal tab, cookie reader, and popup reopened as a tab.
    pub fn install(self: &Arc<Self>, view: &WebView, mut context: Context) -> Result<(), String> {
        context.route_path = self.context.route_path;
        if let Some(bridge) = &self.bridge {
            return super::browser::install_local_auth(view, bridge.shared_auth(), context);
        }
        if self.upstream.credentials().is_none() {
            return Ok(());
        }
        let mut actual_version = PWSTR::null();
        unsafe { view.environment().BrowserVersionString(&mut actual_version) }
            .map_err(|_| "无法核验代理认证运行时；未打开账号页面")?;
        if !scoped_auth_runtime(&take_pwstr(actual_version)) {
            return Err("浏览器运行时不支持安全代理认证；未打开账号页面".into());
        }
        // An origin hosted on the proxy's own endpoint would be ambiguous with
        // a 407 URI. Refuse it for all resource types before sending requests.
        let route = self.clone();
        let environment = view.environment();
        let filter = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                unsafe {
                    let request = args.Request()?;
                    let mut uri = PWSTR::null();
                    request.Uri(&mut uri)?;
                    if route.same_endpoint(&take_pwstr(uri)) {
                        Event::new(context, Stage::AuthEndpointBlocked, Outcome::Blocked).emit();
                        let response = environment.CreateWebResourceResponse(
                            None::<&windows::Win32::System::Com::IStream>,
                            403,
                            &HSTRING::from("Proxy endpoint is not a web origin"),
                            &HSTRING::from("Content-Length: 0\r\n"),
                        )?;
                        args.SetResponse(&response)?;
                    }
                }
            }
            Ok(())
        }));
        let native = view.webview();
        let scoped: ICoreWebView2_22 = native
            .cast()
            .map_err(|_| "浏览器不支持全来源认证端点保护；未打开账号页面")?;
        let mut token = 0;
        (|| -> windows::core::Result<()> {
            unsafe {
                scoped.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    &HSTRING::from("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )?;
                native.add_WebResourceRequested(&filter, &mut token)?;
            }
            Ok(())
        })()
        .map_err(|_| "无法保护代理认证端点；未使用直连")?;
        let route = self.clone();
        let callback =
            BasicAuthenticationRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    unsafe {
                        let mut uri = PWSTR::null();
                        args.Uri(&mut uri)?;
                        let uri = take_pwstr(uri);
                        let mut challenge = PWSTR::null();
                        args.Challenge(&mut challenge)?;
                        let challenge = take_pwstr(challenge);
                        let matches = route.proxy_challenge(&uri);
                        // Do not answer ordinary origin 401s, even with the same realm.
                        // Cancel unmatched challenges to avoid an accidental browser UI.
                        let accepted =
                            matches && challenge.to_ascii_lowercase().starts_with("basic ");
                        if accepted {
                            if let Some((user, secret)) = route.upstream.credentials() {
                                let response = args.Response()?;
                                response.SetUserName(&HSTRING::from(user))?;
                                response.SetPassword(&HSTRING::from(secret))?;
                            }
                        } else {
                            args.SetCancel(true)?;
                        }
                        Event::new(
                            context,
                            Stage::WebViewAuth,
                            if accepted {
                                Outcome::MatchedProxy
                            } else {
                                Outcome::RejectedAuth
                            },
                        )
                        .emit();
                    }
                }
                Ok(())
            }));
        let native: ICoreWebView2_10 = native
            .cast()
            .map_err(|_| "WebView2 不支持代理认证范围核验；未使用直连")?;
        unsafe { native.add_BasicAuthenticationRequested(&callback, &mut token) }
            .map_err(|_| "无法注册代理认证；未使用直连")?;
        Ok(())
    }

    fn same_endpoint(&self, uri: &str) -> bool {
        Url::parse(uri).ok().is_some_and(|u| {
            u.scheme() == self.endpoint.scheme()
                && u.host() == self.endpoint.host()
                && u.port_or_known_default() == self.endpoint.port_or_known_default()
        })
    }
    fn proxy_challenge(&self, uri: &str) -> bool {
        self.same_endpoint(uri)
            && Url::parse(uri).ok().is_some_and(|u| {
                u.username().is_empty()
                    && u.password().is_none()
                    && matches!(u.path(), "" | "/")
                    && u.query().is_none()
                    && u.fragment().is_none()
            })
    }
}

fn scoped_auth_runtime(version: &str) -> bool {
    let parts = version
        .split('.')
        .take(4)
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>();
    parts.is_ok_and(|v| v.len() == 4 && v.as_slice() >= [122, 0, 2365, 46].as_slice())
}
