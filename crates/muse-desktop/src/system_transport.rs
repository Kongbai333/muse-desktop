//! Reads current user's enabled Windows proxy, never writes network settings.
use muse_proxy::ProxyError;
use std::sync::OnceLock;
use windows::{
    Win32::{
        Foundation::{GlobalFree, HGLOBAL},
        Networking::WinHttp::{
            WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WinHttpGetIEProxyConfigForCurrentUser,
        },
    },
    core::PWSTR,
};

static NOTICE: OnceLock<String> = OnceLock::new();

pub fn start(enabled: bool) {
    if !enabled {
        return;
    }
    let config = read_static_loopback();
    let result = muse_proxy::configure_system_transport(config);
    let message = match result {
        Ok(()) => {
            "账号代理前置：使用 Windows 已启用的静态回环 HTTP 代理（本次启动固定，不解析 PAC/WPAD）"
                .to_owned()
        }
        Err(error) => format!("账号代理前置不可用：{error}；系统网络账号保持原设置"),
    };
    let _ = NOTICE.set(message);
}
pub fn notice() -> Option<String> {
    NOTICE.get().cloned()
}

fn read_static_loopback() -> Result<std::net::SocketAddr, ProxyError> {
    let mut config = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    let read = unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) };
    // WinHTTP allocates these using GlobalAlloc, including on partial failure.
    let strings = ProxyStrings([
        config.lpszProxy,
        config.lpszAutoConfigUrl,
        config.lpszProxyBypass,
    ]);
    read.map_err(|_| ProxyError::SystemConfiguration)?;
    if strings.0[0].is_null() {
        return Err(ProxyError::SystemDisabled);
    }
    if !strings.0[1].is_null() {
        return Err(ProxyError::SystemConfiguration);
    }
    let value = unsafe { strings.0[0].to_string() }.map_err(|_| ProxyError::SystemConfiguration)?;
    muse_proxy::parse_system_proxy(&value)
}
struct ProxyStrings([PWSTR; 3]);
impl Drop for ProxyStrings {
    fn drop(&mut self) {
        for value in self.0 {
            if !value.is_null() {
                let _ = unsafe { GlobalFree(Some(HGLOBAL(value.0.cast()))) };
            }
        }
    }
}
