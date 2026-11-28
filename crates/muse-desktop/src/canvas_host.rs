//! Local-only canvas surface. Account websites never receive this IPC bridge.
use muse_core::ThemeMode;
use std::{
    borrow::Cow,
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use wry::{
    WebContext, WebView, WebViewBuilder,
    http::{Request, Response},
};
use wry::{WebViewBuilderExtWindows, WebViewExtWindows};

pub const ORIGIN: &str = "http://muse-canvas.localhost";
pub const MAX_MESSAGE: usize = 48 * 1024 * 1024;

pub struct CanvasHost {
    pub view: WebView,
    _context: WebContext,
    pub receiver: mpsc::Receiver<serde_json::Value>,
    pub ready: bool,
    pub accounts: String,
    pub library: crate::canvas_store::CanvasLibrary,
    pub tasks: HashMap<(uuid::Uuid, String), (uuid::Uuid, String)>,
    pub last_results: String,
    bounds: Option<[f64; 4]>,
    visible: bool,
    dark: Arc<AtomicBool>,
}

impl CanvasHost {
    pub fn new(
        window: &gpui::Window,
        root: &Path,
        bounds: wry::Rect,
        theme: ThemeMode,
    ) -> Result<Self, String> {
        let library = crate::canvas_store::CanvasLibrary::load(root)?;
        let mut context = WebContext::new(Some(root.join("canvas-webview")));
        let (sender, receiver) = mpsc::channel();
        let media_root = root.to_owned();
        let dark = Arc::new(AtomicBool::new(theme == ThemeMode::Dark));
        let page_theme = dark.clone();
        let view = WebViewBuilder::new_with_web_context(&mut context)
            .with_custom_protocol("muse-canvas".into(), move |_, request| {
                serve(&media_root, request, page_theme.load(Ordering::Relaxed))
                    .unwrap_or_else(|_| response(404, "text/plain", b"Not found".to_vec()))
            })
            .with_url(format!("{ORIGIN}/"))
            .with_background_color(theme_background(theme))
            .with_theme(webview_theme(theme))
            .with_bounds(bounds)
            .with_focused(false)
            .with_visible(false)
            .with_navigation_handler(|url| url == format!("{ORIGIN}/"))
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_ipc_handler(move |request| {
                if request.uri().scheme_str() != Some("http")
                    || request.uri().authority().map(|a| a.as_str())
                        != Some("muse-canvas.localhost")
                    || request.body().len() > MAX_MESSAGE
                {
                    return;
                }
                if let Ok(message) = serde_json::from_str(request.body()) {
                    let _ = sender.send(message);
                }
            })
            .build_as_child(window)
            .map_err(|error| format!("画布加载失败：{error}"))?;
        Ok(Self {
            view,
            _context: context,
            receiver,
            ready: false,
            accounts: String::new(),
            tasks: HashMap::new(),
            library,
            last_results: String::new(),
            bounds: None,
            visible: false,
            dark,
        })
    }

    pub fn sync(&mut self, bounds: wry::Rect, visible: bool) {
        let position = bounds.position.to_logical::<f64>(1.);
        let size = bounds.size.to_logical::<f64>(1.);
        let values = [position.x, position.y, size.width, size.height];
        if self.bounds != Some(values) {
            if self.view.set_bounds(bounds).is_ok() {
                self.bounds = Some(values);
            }
        }
        if self.visible != visible && self.view.set_visible(visible).is_ok() {
            self.visible = visible;
        }
    }

    pub fn send(&self, message: serde_json::Value) {
        let _ = self
            .view
            .evaluate_script(&format!("window.museCanvasDesktop?.receive({message})"));
    }

    pub fn sync_theme(&self, theme: ThemeMode, force: bool) {
        let dark = theme == ThemeMode::Dark;
        if self.dark.swap(dark, Ordering::Relaxed) == dark && !force {
            return;
        }
        let _ = self.view.set_theme(webview_theme(theme));
        let _ = self.view.set_background_color(theme_background(theme));
        let name = if dark { "dark" } else { "light" };
        // Also set the root directly: theme changes can arrive before page ready.
        let _ = self.view.evaluate_script(&format!(
            "if(document.documentElement)document.documentElement.dataset.theme='{name}';window.museCanvasDesktop?.receive({{kind:'theme',theme:'{name}'}});"
        ));
    }
}

fn webview_theme(theme: ThemeMode) -> wry::Theme {
    match theme {
        ThemeMode::Light => wry::Theme::Light,
        ThemeMode::Dark => wry::Theme::Dark,
    }
}

fn theme_background(theme: ThemeMode) -> wry::RGBA {
    let color = crate::theme::palette_for(theme).canvas;
    ((color >> 16) as u8, (color >> 8) as u8, color as u8, 255)
}

fn response(status: u16, mime: &str, bytes: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    Response::builder().status(status).header("Content-Type", mime)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store")
        .header("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' data: blob:; connect-src 'self' blob: data:; base-uri 'none'; frame-src 'none'; form-action 'none'")
        .body(Cow::Owned(bytes)).expect("constant response headers")
}

fn serve(
    root: &Path,
    request: Request<Vec<u8>>,
    dark: bool,
) -> Result<Response<Cow<'static, [u8]>>, ()> {
    let path = request.uri().path();
    match path {
        "/" => {
            return Ok(response(
                200,
                "text/html; charset=utf-8",
                include_str!("../../../assets/muse-canvas.html")
                    .replacen(
                        "<html lang=\"zh-CN\">",
                        if dark {
                            "<html lang=\"zh-CN\" data-theme=\"dark\">"
                        } else {
                            "<html lang=\"zh-CN\" data-theme=\"light\">"
                        },
                        1,
                    )
                    .into_bytes(),
            ));
        }
        "/muse-canvas.js" => {
            return Ok(response(
                200,
                "text/javascript; charset=utf-8",
                include_bytes!("../../../assets/muse-canvas.js").to_vec(),
            ));
        }
        "/muse-canvas-theme.css" => {
            return Ok(response(
                200,
                "text/css; charset=utf-8",
                include_bytes!("../../../assets/muse-canvas-theme.css").to_vec(),
            ));
        }
        _ => {}
    }
    let decoded = percent_encoding::percent_decode_str(path.trim_start_matches('/'))
        .decode_utf8()
        .map_err(|_| ())?;
    let file = crate::studio::safe_media_path(root, &decoded).ok_or(())?;
    let canonical = file.canonicalize().map_err(|_| ())?;
    if !canonical.starts_with(root.join("media").canonicalize().map_err(|_| ())?) {
        return Err(());
    }
    let mime = match file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => return Err(()),
    };
    let mut stream = std::fs::File::open(file).map_err(|_| ())?;
    let length = stream.metadata().map_err(|_| ())?.len();
    if length == 0 {
        return Err(());
    }
    let range = request
        .headers()
        .get("Range")
        .and_then(|value| value.to_str().ok());
    let (start, end, partial) = if let Some(range) = range {
        let (start, end) = range
            .strip_prefix("bytes=")
            .and_then(|s| s.split_once('-'))
            .ok_or(())?;
        let (start, end) = if start.is_empty() {
            let tail = end.parse::<u64>().map_err(|_| ())?;
            (length.saturating_sub(tail), length - 1)
        } else {
            let start = start.parse::<u64>().map_err(|_| ())?;
            let end = if end.is_empty() {
                start.saturating_add(4 * 1024 * 1024 - 1)
            } else {
                end.parse().map_err(|_| ())?
            };
            (start, end.min(length - 1))
        };
        if start >= length || end < start {
            let mut result = response(416, mime, Vec::new());
            result.headers_mut().insert(
                "Content-Range",
                format!("bytes */{length}").parse().map_err(|_| ())?,
            );
            return Ok(result);
        }
        (start, end, true)
    } else {
        if length > 256 * 1024 * 1024 {
            return Err(());
        }
        (0, length - 1, false)
    };
    stream.seek(SeekFrom::Start(start)).map_err(|_| ())?;
    let mut bytes = Vec::new();
    stream
        .take(end - start + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    let mut result = response(if partial { 206 } else { 200 }, mime, bytes);
    result
        .headers_mut()
        .insert("Accept-Ranges", "bytes".parse().unwrap());
    result.headers_mut().insert(
        "Content-Length",
        (end - start + 1).to_string().parse().map_err(|_| ())?,
    );
    if partial {
        result.headers_mut().insert(
            "Content-Range",
            format!("bytes {start}-{end}/{length}")
                .parse()
                .map_err(|_| ())?,
        );
    }
    Ok(result)
}

pub fn import_media(root: &Path, data: &str) -> Result<String, String> {
    use base64::Engine;
    let (header, content) = data.split_once(',').ok_or("媒体格式无效")?;
    let extension = match header {
        "data:image/png;base64" => "png",
        "data:image/jpeg;base64" => "jpg",
        "data:image/webp;base64" => "webp",
        "data:image/gif;base64" => "gif",
        "data:video/mp4;base64" => "mp4",
        "data:video/webm;base64" => "webm",
        _ => return Err("支持 PNG、JPG、WebP、GIF、MP4 或 WebM".into()),
    };
    if content.len() > 44 * 1024 * 1024 {
        return Err("媒体文件超过 32 MB".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content)
        .map_err(|_| "媒体数据无效")?;
    if bytes.is_empty() || bytes.len() > 32 * 1024 * 1024 {
        return Err("媒体大小无效".into());
    }
    let relative = format!("media/canvas-{}.{}", uuid::Uuid::new_v4(), extension);
    std::fs::create_dir_all(root.join("media")).map_err(|_| "无法保存媒体")?;
    std::fs::write(root.join(&relative), bytes).map_err(|_| "无法保存媒体")?;
    Ok(format!("{ORIGIN}/{relative}"))
}

pub fn reference_path(root: &Path, value: &str) -> Option<PathBuf> {
    let relative = value.strip_prefix(&format!("{ORIGIN}/"))?;
    let path = crate::studio::safe_media_path(root, relative)?
        .canonicalize()
        .ok()?;
    path.starts_with(root.join("media").canonicalize().ok()?)
        .then_some(path)
}
