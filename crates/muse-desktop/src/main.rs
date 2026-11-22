#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod account_cache;
mod account_settings;
mod assets;
mod browser;
mod cookie_export;
mod diagnostics;
mod import;
mod input;
mod input_mask;
mod motion;
mod page_observer;
mod platform;
mod proxy_pool;
mod proxy_settings;
mod proxy_smoke;
#[cfg(any(target_os = "macos", test))]
mod secret_policy;
#[cfg(target_os = "windows")]
mod secrets;
#[cfg(target_os = "macos")]
#[path = "secrets_macos.rs"]
mod secrets;
mod session_guard;
mod session_probe;
mod sidebar_state;
mod site_avatar;
mod studio;
mod studio_image;
mod studio_reference;
mod theme;
mod ui;
mod window_state;
#[cfg(target_os = "windows")]
mod windows_caption;
#[cfg(target_os = "macos")]
#[path = "caption_macos.rs"]
mod windows_caption;
#[cfg(target_os = "windows")]
mod windows_host;
#[cfg(target_os = "macos")]
#[path = "host_macos.rs"]
mod windows_host;

use gpui::{
    App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use std::path::PathBuf;

fn main() {
    // GPUI's default DirectComposition target sits above child HWNDs. Wry's
    // WebView2 child must be able to paint over the native workspace surface.
    // SAFETY: this runs at process entry before any application threads start.
    unsafe { std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1") };
    let arguments: Vec<String> = std::env::args().collect();
    let data_dir = arguments
        .windows(2)
        .find(|pair| pair[0] == "--data-dir")
        .map(|pair| PathBuf::from(&pair[1]))
        .unwrap_or_else(platform::data_dir);
    // The app has no console window, so a panic would otherwise close it silently.
    // Keep the last panic (message, source location and backtrace) next to the data.
    {
        let crash_log = data_dir.join("crash.log");
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            let report = format!(
                "thread: {}\n{}\n\n{}\n",
                thread.name().unwrap_or("<unnamed>"),
                info,
                std::backtrace::Backtrace::force_capture()
            );
            if let Some(parent) = crash_log.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&crash_log)
            {
                let _ = std::io::Write::write_all(&mut file, report.as_bytes());
            }
            default_hook(info);
        }));
    }
    let proxy_smoke = arguments.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--proxy-smoke-test" | "--proxy-edit-smoke-test"
        )
    });
    let smoke = proxy_smoke || arguments.iter().any(|arg| arg == "--smoke-test");
    let value_after = |flag: &str| {
        arguments
            .windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].clone())
    };
    if arguments.iter().any(|arg| arg == "--import-legacy") {
        let result = (|| -> Result<serde_json::Value, String> {
            let (config, profiles) = import::default_paths()?;
            let requested = value_after("--legacy-account-id")
                .map(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|_| "旧版账号 ID 无效".to_owned())
                })
                .transpose()?;
            let source = value_after("--source-profile").map(PathBuf::from);
            let imported =
                import::import_legacy(&data_dir, &config, &profiles, requested, source.as_deref())?;
            Ok(
                serde_json::json!({"imported":true,"account_id":imported.account_id,"already_present":imported.already_present,"proxy_policy_preserved":true}),
            )
        })();
        let report = match result {
            Ok(report) => report,
            Err(error) => serde_json::json!({"imported":false,"error":error}),
        };
        let _ = std::fs::create_dir_all(&data_dir);
        let _ = std::fs::write(
            data_dir.join("import-result.json"),
            serde_json::to_string_pretty(&report).unwrap_or_default(),
        );
        if arguments.iter().any(|arg| arg == "--import-only") || report["imported"] != true {
            return;
        }
    }
    if arguments
        .iter()
        .any(|argument| argument == "--check-upstream")
    {
        let result = (|| -> Result<(), String> {
            let workspace = muse_core::Workspace::load(&data_dir.join("workspace.json"))?;
            let account = workspace.active_account().ok_or("未选中账号")?;
            if account.network == muse_core::AccountNetwork::Direct {
                return Err("当前账号使用系统网络，尚未启用账号代理".into());
            }
            let (id, revision) = workspace.proxy_credentials(account)?;
            let secret = secrets::ProtectedStore::new(&data_dir).load_selected(id, revision)?;
            let upstream =
                muse_proxy::Upstream::parse(secret.as_str()).map_err(|e| e.to_string())?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "无法创建网络检查任务")?;
            runtime
                .block_on(upstream.probe())
                .map_err(|e| e.to_string())
        })();
        let report = match result {
            Ok(()) => {
                serde_json::json!({"connected_through_upstream":true,"authentication_accepted":true,"direct_fallback":false})
            }
            Err(error) => {
                serde_json::json!({"connected_through_upstream":false,"error":error,"direct_fallback":false})
            }
        };
        let _ = std::fs::write(
            data_dir.join("upstream-check.json"),
            serde_json::to_string_pretty(&report).unwrap_or_default(),
        );
        return;
    }
    let diagnostics = smoke || arguments.iter().any(|arg| arg == "--diagnostics");
    let diagnostic_home = if diagnostics {
        Some(diagnostics::start().expect("Cannot bind local diagnostic server"))
    } else {
        None
    };
    // CI maps this reserved test hostname to the local read-only fixture. A
    // numeric loopback URL can be implicitly exempted from macOS proxy rules.
    let diagnostic_home = diagnostic_home.map(|home| {
        if smoke && std::env::var("MUSE_SMOKE_PROXY_HOST").as_deref() == Ok("muse-proxy-smoke.test")
        {
            let mut url = url::Url::parse(&home).expect("Valid diagnostic fixture URL");
            url.set_host(Some("muse-proxy-smoke.test"))
                .expect("Valid fixed fixture hostname");
            url.to_string()
        } else {
            home
        }
    });
    if proxy_smoke
        && let Err(error) = proxy_smoke::prepare(
            &data_dir,
            diagnostic_home.as_deref().unwrap_or_default(),
            !arguments
                .iter()
                .any(|argument| argument == "--proxy-edit-smoke-test"),
        )
    {
        let _ = std::fs::write(
            data_dir.join("smoke-result.json"),
            serde_json::json!({"passed":false,"error":error}).to_string(),
        );
        return;
    }
    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            input::bind_keys(cx);
            ui::bind_keys(cx);
            let bounds = Bounds::centered(None, size(px(1360.), px(880.)), cx);
            let displays = cx
                .displays()
                .iter()
                .map(|display| display.bounds())
                .collect::<Vec<_>>();
            let saved_bounds = window_state::SavedWindow::load(&data_dir)
                .or_else(|| window_state::SavedWindow::capture(WindowBounds::Windowed(bounds)))
                .and_then(|saved| saved.restore(&displays))
                .unwrap_or(WindowBounds::Windowed(bounds));
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(saved_bounds),
                    window_min_size: Some(size(px(1030.), px(680.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Muse".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    cx.new(|cx| ui::MuseWindow::new(data_dir, diagnostic_home, smoke, window, cx))
                },
            )
            .expect("Cannot open GPUI window");
            cx.activate(true);
        });
}
