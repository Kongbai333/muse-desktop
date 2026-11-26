//! Native-only locale setup. Never changes UA, script-visible prototypes or profile paths.
use std::{path::Path, sync::mpsc};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2Environment,
        ICoreWebView2EnvironmentOptions,
    },
};
use windows::core::{HSTRING, PCWSTR};
use wry::WebViewExtWindows;

pub fn environment(
    profile: &Path,
    language: &str,
    arguments: &str,
) -> Result<ICoreWebView2Environment, String> {
    let options = CoreWebView2EnvironmentOptions::default();
    let (tx, rx) = mpsc::channel();
    unsafe {
        options.set_language(language.to_owned());
        options.set_additional_browser_arguments(arguments.to_owned());
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(profile.as_os_str()),
            &ICoreWebView2EnvironmentOptions::from(options),
            &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
                move |result, environment| {
                    let _ = tx.send(result.is_ok().then_some(environment).flatten());
                    Ok(())
                },
            )),
        )
        .map_err(|_| "无法准备浏览器语言设置；未打开账号页面")?;
    }
    webview2_com::wait_with_pump(rx)
        .map_err(|_| "浏览器语言设置未完成；未打开账号页面".to_owned())?
        .ok_or_else(|| "无法应用浏览器语言；请等待此账号浏览器退出后重试".to_owned())
}

/// Call completion is checked while the view is still about:blank. This applies
/// per target; every same-account controller receives the same selected value.
pub fn apply_timezone(view: &wry::WebView, timezone: Option<&str>) -> Result<(), String> {
    let Some(timezone) = timezone else {
        return Ok(());
    };
    let params = serde_json::json!({"timezoneId": timezone}).to_string();
    let (tx, rx) = mpsc::channel();
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
        let _ = tx.send(result.is_ok());
        Ok(())
    }));
    unsafe {
        view.webview().CallDevToolsProtocolMethod(
            &HSTRING::from("Emulation.setTimezoneOverride"),
            &HSTRING::from(params),
            &handler,
        )
    }
    .map_err(|_| "当前 WebView2 无法设置时区；未打开账号页面")?;
    if webview2_com::wait_with_pump(rx).map_err(|_| "浏览器时区设置未完成；未打开账号页面")?
    {
        Ok(())
    } else {
        Err("浏览器拒绝时区设置；未打开账号页面，请选择支持的时区或跟随系统".into())
    }
}
