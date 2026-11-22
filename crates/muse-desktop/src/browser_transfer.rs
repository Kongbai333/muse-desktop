//! Local native account snapshots and unpublished migration staging. No website navigation.
use super::*;
use crate::{account_transfer as wire, transfer_cookies};
#[cfg(target_os = "macos")]
#[path = "browser_transfer_macos.rs"]
mod native;

impl BrowserPool {
    pub fn snapshot_transfer(&self, identity: BrowserIdentity) -> Result<wire::CookieSet, String> {
        let tab = self
            .tabs
            .get(&identity.tab_id)
            .filter(|t| t.identity == identity)
            .ok_or("账号浏览器已改变")?;
        if !self.cookie_only.contains(&identity)
            && !current_url(&tab.view).is_some_and(|url| muse_core::trusted_muse_url(&url))
        {
            return Err("请先打开所选账号的 Muse 页面，或重新打开账号后导出".into());
        }
        transfer_cookies::scoped(native::read(&tab.view)?)
    }
    pub fn stage_transfer(
        &mut self,
        request: BrowserRequest<'_>,
        cookies: &wire::CookieSet,
        window: &gpui::Window,
        bounds: Rect,
    ) -> Result<(), String> {
        let identity = request.identity;
        let imported =
            transfer_cookies::for_import(cookies, (page_observer::now_ms() / 1000) as i64)?;
        if request.url != "about:blank" || self.tabs.contains_key(&identity.tab_id) {
            return Err("账号包只能导入新隔离环境".into());
        }
        if let Err(error) = self.ensure_reader(request, window, bounds) {
            self.invalidate_account(identity.account_id);
            return Err(error);
        }
        let result = (|| {
            let view = self.view(identity.tab_id).ok_or("无法准备账号浏览器")?;
            for cookie in &imported.items {
                native::write(view, cookie)?;
            }
            // Native completion/read-back is the API barrier. Session cookies intentionally have no persisted lifetime.
            let actual = native::read(view)?;
            for expected in &imported.items {
                let found = actual.iter().any(|c| {
                    c.name == expected.name
                        && c.value == expected.value.as_str()
                        && c.domain.as_deref().map(|d| d.trim_start_matches('.'))
                            == Some(expected.domain.trim_start_matches('.'))
                        && c.path.as_deref().unwrap_or("/") == expected.path
                        && c.secure == Some(expected.secure)
                        && c.http_only == Some(expected.http_only)
                        && match (c.expires_at_unix_seconds, expected.expires) {
                            (None, None) => true,
                            (Some(a), Some(b)) => a.abs_diff(b) <= 1,
                            _ => false,
                        }
                        && expected.same_site.is_none_or(|site| {
                            c.same_site
                                .as_deref()
                                .is_some_and(|s| s.eq_ignore_ascii_case(&site.to_string()))
                        })
                        && (expected.domain.starts_with('.')
                            == c.domain.as_deref().is_some_and(|d| d.starts_with('.')))
                });
                if !found {
                    return Err("原生浏览器未保留完整 Cookie 属性或有效期，整批账号未激活；请在源设备重新登录并导出后重试".into());
                }
            }
            Ok(())
        })();
        if result.is_err() {
            self.discard_transfer(identity);
        }
        result
    }
    pub fn discard_transfer(&mut self, identity: BrowserIdentity) {
        if !self.cookie_only.contains(&identity) {
            return;
        }
        if let Some(view) = self.view(identity.tab_id) {
            let _ = view.clear_all_browsing_data();
        }
        self.invalidate_account(identity.account_id);
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use webview2_com::{
        GetCookiesCompletedHandler,
        Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX, COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE,
            COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT, ICoreWebView2_2, ICoreWebView2Cookie,
        },
    };
    pub(super) fn read(view: &WebView) -> Result<Vec<wire::Cookie>, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let native = view
            .webview()
            .cast::<ICoreWebView2_2>()
            .map_err(|_| "原生 Cookie API 不可用")?;
        unsafe {
            native
                .CookieManager()
                .map_err(|_| "原生 Cookie API 不可用")?
                .GetCookies(
                    &HSTRING::new(),
                    &GetCookiesCompletedHandler::create(Box::new(move |status, list| {
                        let result = (|| -> Result<_, String> {
                            status.map_err(|_| "读取原生 Cookie 失败")?;
                            let list = list.ok_or("原生 Cookie 列表不可用")?;
                            let mut count = 0;
                            list.Count(&mut count).map_err(|_| "读取原生 Cookie 失败")?;
                            if count > 4096 {
                                return Err("Cookie 数量超过安全限制".into());
                            }
                            let mut items = Vec::new();
                            for index in 0..count {
                                let c = list
                                    .GetValueAtIndex(index)
                                    .map_err(|_| "读取原生 Cookie 失败")?;
                                let text =
                                    |getter: fn(
                                        &ICoreWebView2Cookie,
                                        &mut PWSTR,
                                    )
                                        -> windows::core::Result<()>|
                                     -> Result<String, String> {
                                        let mut raw = PWSTR::null();
                                        getter(&c, &mut raw)
                                            .map_err(|_| "读取原生 Cookie 字段失败")?;
                                        Ok(take_pwstr(raw))
                                    };
                                let domain = text(|c, p| c.Domain(p))?;
                                if !["muse.ai", ".muse.ai", "www.muse.ai", ".www.muse.ai"]
                                    .contains(&domain.as_str())
                                {
                                    continue;
                                }
                                let path = text(|c, p| c.Path(p))?;
                                let (mut expires, mut secure, mut http, mut session, mut site) = (
                                    0.,
                                    Default::default(),
                                    Default::default(),
                                    Default::default(),
                                    Default::default(),
                                );
                                c.Expires(&mut expires)
                                    .map_err(|_| "读取 Cookie 有效期失败")?;
                                c.IsSecure(&mut secure)
                                    .map_err(|_| "读取 Cookie 属性失败")?;
                                c.IsHttpOnly(&mut http)
                                    .map_err(|_| "读取 Cookie 属性失败")?;
                                c.IsSession(&mut session)
                                    .map_err(|_| "读取 Cookie 属性失败")?;
                                c.SameSite(&mut site).map_err(|_| "读取 Cookie 属性失败")?;
                                let same_site = if site == COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX {
                                    "lax"
                                } else if site == COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT {
                                    "strict"
                                } else if site == COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE {
                                    "none"
                                } else {
                                    "unspecified"
                                };
                                items.push(wire::Cookie {
                                    source_url: transfer_cookies::source_url(&domain, &path)?,
                                    name: text(|c, p| c.Name(p))?,
                                    value: text(|c, p| c.Value(p))?,
                                    domain: Some(domain),
                                    path: Some(path),
                                    expires_at_unix_seconds: (!session.as_bool())
                                        .then_some(expires as i64),
                                    secure: Some(secure.as_bool()),
                                    http_only: Some(http.as_bool()),
                                    same_site: Some(same_site.into()),
                                    session: Some(session.as_bool()),
                                    host_only: None,
                                });
                            }
                            Ok(items)
                        })();
                        let _ = tx.send(result);
                        Ok(())
                    })),
                )
                .map_err(|_| "读取原生 Cookie 失败")?;
        }
        webview2_com::wait_with_pump(rx).map_err(|_| "等待原生 Cookie 失败")?
    }
    pub(super) fn write(
        view: &WebView,
        item: &crate::cookie_import::ImportedCookie,
    ) -> Result<(), String> {
        let native = view
            .webview()
            .cast::<ICoreWebView2_2>()
            .map_err(|_| "原生 Cookie API 不可用")?;
        unsafe {
            let manager = native
                .CookieManager()
                .map_err(|_| "原生 Cookie API 不可用")?;
            // Wry's Cookie::domain() strips the leading dot. Pass the explicitly mapped scope directly.
            let c = manager
                .CreateCookie(
                    &HSTRING::from(&item.name),
                    &HSTRING::from(item.value.as_str()),
                    &HSTRING::from(&item.domain),
                    &HSTRING::from(&item.path),
                )
                .map_err(|_| "无法创建原生 Cookie")?;
            if let Some(expiry) = item.expires {
                c.SetExpires(expiry as f64)
                    .map_err(|_| "无法写入 Cookie 有效期")?;
            }
            c.SetIsSecure(item.secure)
                .map_err(|_| "无法写入 Cookie 安全属性")?;
            c.SetIsHttpOnly(item.http_only)
                .map_err(|_| "无法写入 Cookie 安全属性")?;
            if let Some(site) = item.same_site {
                c.SetSameSite(match site {
                    wry::cookie::SameSite::Lax => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX,
                    wry::cookie::SameSite::Strict => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT,
                    wry::cookie::SameSite::None => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE,
                })
                .map_err(|_| "无法写入 Cookie SameSite")?;
            }
            manager
                .AddOrUpdateCookie(&c)
                .map_err(|_| "写入原生 Cookie 失败".to_owned())
        }
    }
}
