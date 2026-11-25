//! Local viewing and account-bound original recovery have separate entry points.
use super::*;
use crate::studio::{self, Job, Mode, Phase};
use crate::studio_image::{ImageViewer, image_size};
use base64::Engine;

impl MuseWindow {
    pub(super) fn open_studio_media(
        &mut self,
        id: uuid::Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_studio_panels(window, cx);
        self.studio.notice.clear();
        let Some(entry) = self
            .studio
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        else {
            return;
        };
        let Some(path) = entry
            .media
            .as_ref()
            .and_then(|path| studio::safe_media_path(&self.data_dir, path))
            .filter(|path| path.is_file())
        else {
            self.studio.notice = "本地媒体文件不存在，可打开任务页面查看".into();
            cx.notify();
            return;
        };
        if entry.mode == Mode::Image {
            let transition = self
                .studio
                .viewer
                .as_ref()
                .filter(|(viewer, state)| viewer.entry == id && !state.is_exiting())
                .map_or_else(TransitionState::entering, |(_, state)| *state);
            self.studio.viewer =
                ImageViewer::open(&self.data_dir, &entry).map(|viewer| (viewer, transition));
            window.focus(&self.focus);
            cx.notify();
        } else {
            if let Ok(url) = url::Url::from_file_path(path) {
                cx.open_url(url.as_str());
            }
        }
    }

    #[allow(dead_code)]
    pub(super) fn fetch_studio_original(
        &mut self,
        id: uuid::Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_studio_media(id, window, cx);
        let Some(entry) = self
            .studio
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        else {
            return;
        };
        let Some(path) = ImageViewer::cached_path(&self.data_dir, &entry) else {
            return;
        };
        // A cached original is authoritative, even after restart or account removal.
        if entry.media_original || entry.running {
            return;
        }
        if self.studio.jobs.contains_key(&entry.account) {
            self.studio.notice = "这个账号正在处理任务，请完成后再读取原图".into();
            cx.notify();
            return;
        }
        let thumbnail = std::fs::metadata(&path)
            .ok()
            .filter(|metadata| metadata.len() <= 2 * 1024 * 1024)
            .and_then(|_| std::fs::read(&path).ok())
            .filter(|bytes| !bytes.is_empty());
        let Some(thumbnail) = thumbnail else {
            self.studio.notice = "无法读取旧预览，请在任务页面查看原图".into();
            cx.notify();
            return;
        };
        let Some(account) = self.workspace.account(entry.account) else {
            self.studio.notice = "原账号已移除，无法读取原图".into();
            cx.notify();
            return;
        };
        let saved = entry
            .source_tab
            .and_then(|id| account.tabs.iter().find(|tab| tab.id == id))
            .filter(|tab| muse_core::trusted_muse_url(&tab.url))
            .map(|tab| muse_core::BrowserIdentity {
                account_id: entry.account,
                tab_id: tab.id,
                generation: tab.generation,
            });
        let current = account
            .active_tab()
            .filter(|tab| muse_core::trusted_muse_url(&tab.url))
            .map(|tab| muse_core::BrowserIdentity {
                account_id: entry.account,
                tab_id: tab.id,
                generation: tab.generation,
            });
        let identity = if let Some(identity) = saved {
            Some(identity)
        } else if let Some(url) = entry
            .source_url
            .as_deref()
            .filter(|url| muse_core::trusted_muse_url(url))
        {
            let previous = self.workspace.active_browser_identity();
            let previous_tab = Some(account.active_tab_id());
            let tab = self.workspace.add_tab(entry.account, url).ok();
            let identity = tab.and_then(|tab_id| {
                self.workspace
                    .account(entry.account)?
                    .tabs
                    .iter()
                    .find(|tab| tab.id == tab_id)
                    .map(|tab| muse_core::BrowserIdentity {
                        account_id: entry.account,
                        tab_id,
                        generation: tab.generation,
                    })
            });
            if let Some(tab) = previous_tab {
                let _ = self.workspace.select_tab(tab);
            }
            if let Some(previous) = previous {
                let _ = self.workspace.select_tab(previous.tab_id);
            }
            self.persist();
            identity
        } else {
            current
        };
        let Some(identity) = identity else {
            self.studio.notice =
                "旧记录没有保存原任务地址，请先在该账号打开对应历史对话，再点“获取原图”".into();
            cx.notify();
            return;
        };
        if let Err(error) = self.ensure_identity(identity, window) {
            self.studio.notice = error;
            cx.notify();
            return;
        }
        if let Some(entry) = self.studio.entry_mut(id) {
            entry.status = "正在匹配原任务并读取原图…".into();
            entry.running = true;
            entry.source_tab = Some(identity.tab_id);
        }
        self.studio.jobs.insert(
            entry.account,
            Job {
                id,
                identity: Some(identity),
                phase: Phase::Recovering {
                    thumbnail: base64::engine::general_purpose::STANDARD.encode(thumbnail),
                },
                started: Instant::now(),
                last_poll: Instant::now() - Duration::from_secs(2),
                pending: false,
                prompt: String::new(),
                mode: Mode::Image,
                recovering: true,
                receipt: None,
            },
        );
        self.studio.notice.clear();
        cx.notify();
    }

    pub(super) fn close_studio_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, state)) = self.studio.viewer.as_mut() else {
            return;
        };
        let Some(generation) = state.begin_exit() else {
            return;
        };
        self.schedule_overlay_removal(
            MotionKind::Modal,
            generation,
            window,
            cx,
            |this, generation, window, cx| {
                if this
                    .studio
                    .viewer
                    .as_ref()
                    .is_some_and(|(_, state)| state.finish_exit(generation))
                {
                    this.studio.viewer = None;
                    this.studio.notice.clear();
                    if this.show_start_page
                        && this.account_dialog.is_none()
                        && this.cookie_dialog.is_none()
                    {
                        window.focus(&this.focus);
                    }
                    cx.notify();
                }
            },
        );
    }

    /// Sets the viewer zoom (None fits the window) and keeps the same point of
    /// the picture in the middle of the view.
    pub(super) fn set_studio_zoom(&mut self, zoom: Option<f32>, cx: &mut Context<Self>) {
        if let Some((viewer, state)) = &mut self.studio.viewer
            && !state.is_exiting()
        {
            let (view_width, view_height, old) = viewer.layout.get();
            let zoom = zoom.map(|zoom| zoom.clamp(0.05, 8.));
            let new = zoom.unwrap_or_else(|| {
                image_size(
                    viewer.natural.0,
                    viewer.natural.1,
                    view_width,
                    view_height,
                    None,
                )
                .2
            });
            viewer.zoom = zoom;
            if view_width > 0. && view_height > 0. && old > 0. {
                let offset = viewer.scroll.offset();
                let recenter = |offset: f32, view: f32, natural: u32| {
                    let natural = natural.max(1) as f32;
                    let before = (natural * old).max(view);
                    let after = (natural * new).max(view);
                    let middle = ((-offset + view / 2.) / before).clamp(0., 1.);
                    (-(middle * after - view / 2.)).clamp(-(after - view).max(0.), 0.)
                };
                viewer.scroll.set_offset(point(
                    px(recenter(f32::from(offset.x), view_width, viewer.natural.0)),
                    px(recenter(f32::from(offset.y), view_height, viewer.natural.1)),
                ));
            } else {
                viewer.scroll.set_offset(point(px(0.), px(0.)));
            }
        }
        cx.notify();
    }

    /// One zoom step in or out from the current scale.
    pub(super) fn step_studio_zoom(&mut self, direction: i32, cx: &mut Context<Self>) {
        let Some(current) = self.studio.viewer.as_ref().map(|(viewer, _)| {
            viewer
                .zoom
                .unwrap_or_else(|| viewer.layout.get().2.max(0.05))
        }) else {
            return;
        };
        self.set_studio_zoom(Some(crate::studio_image::zoom_step(current, direction)), cx);
    }

    fn media_source(&self, id: uuid::Uuid) -> Option<(studio::Entry, std::path::PathBuf)> {
        let entry = self
            .studio
            .entries
            .iter()
            .find(|entry| entry.id == id)?
            .clone();
        let path = entry
            .media
            .as_ref()
            .and_then(|path| studio::safe_media_path(&self.data_dir, path))
            .filter(|path| path.is_file())?;
        Some((entry, path))
    }

    /// Asks where to save the generated file, then copies it there.
    pub(super) fn save_studio_media(
        &mut self,
        id: uuid::Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((entry, source)) = self.media_source(id) else {
            self.studio.notice = "本地文件不存在，无法保存".into();
            cx.notify();
            return;
        };
        let extension = source
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let directory = self
            .studio
            .save_dir
            .clone()
            .filter(|directory| directory.is_dir())
            .unwrap_or_else(|| default_save_dir(&self.data_dir));
        let name = suggested_file_name(&entry, &extension);
        let chosen = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut target))) = chosen.await else {
                return;
            };
            if target.extension().is_none() {
                target.set_extension(&extension);
            }
            let destination = target.clone();
            let copied = cx
                .background_executor()
                .spawn(async move {
                    if destination == source {
                        return Ok(());
                    }
                    std::fs::copy(&source, &destination).map(|_| ())
                })
                .await;
            let message = match &copied {
                Ok(()) => format!("已保存到 {}", target.display()),
                Err(error) => format!("保存失败：{error}"),
            };
            let folder = target.parent().map(std::path::Path::to_path_buf);
            for _ in 0..60 {
                let attempt = this.update(cx, |this, cx| {
                    this.studio.notice = message.clone();
                    if copied.is_ok() {
                        this.studio.save_dir = folder.clone();
                    }
                    cx.notify();
                });
                if attempt.is_ok() || this.upgrade().is_none() {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
        })
        .detach();
    }

    /// Puts the picture on the clipboard (bitmap and PNG), ready to paste into
    /// chat apps, documents and image editors.
    pub(super) fn copy_studio_image(
        &mut self,
        id: uuid::Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((entry, source)) = self.media_source(id) else {
            self.studio.notice = "本地图片不存在，无法复制".into();
            cx.notify();
            return;
        };
        if entry.mode != Mode::Image {
            self.studio.notice = "视频不能复制到剪贴板，请用「保存」".into();
            cx.notify();
            return;
        }
        self.studio.notice = "正在复制图片…".into();
        cx.notify();
        let payload = cx
            .background_executor()
            .spawn(async move { clipboard_payload(&source) });
        cx.spawn_in(window, async move |this, cx| {
            let mut payload = Some(payload.await);
            for _ in 0..60 {
                let attempt = this.update_in(cx, |this, window, cx| {
                    let message = match payload.take() {
                        Some(Ok((dib, png))) => match write_image_clipboard(window, &dib, &png) {
                            Ok(()) => "图片已复制，可以直接粘贴".to_owned(),
                            Err(error) => error,
                        },
                        Some(Err(error)) => error,
                        None => return,
                    };
                    this.studio.notice = message;
                    cx.notify();
                });
                if attempt.is_ok() || this.upgrade().is_none() {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
        })
        .detach();
    }

    pub(super) fn render_studio_image_overlay(&self, window: &Window, cx: &Context<Self>) -> Div {
        let (_, transition) = self.studio.viewer.as_ref().unwrap();
        div()
            .absolute()
            .inset_0()
            .occlude()
            .child(crate::motion::animate(
                self.render_studio_image_viewer(window, cx),
                *transition,
                self.motion_policy,
                MotionKind::Modal,
                8.,
            ))
            .when(transition.is_exiting(), |element| {
                element.child(div().absolute().inset_0().occlude())
            })
    }

    fn render_studio_image_viewer(&self, window: &Window, cx: &Context<Self>) -> Stateful<Div> {
        let (viewer, transition) = self.studio.viewer.as_ref().unwrap();
        let transition = *transition;
        let accepts = move |this: &Self| {
            this.studio
                .viewer
                .as_ref()
                .is_some_and(|(_, state)| *state == transition && !state.is_exiting())
        };
        let overlay = div()
            .id("studio-image-viewer")
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x111113F7))
            .text_size(px(14.))
            .text_color(rgb(0xF5F6F6))
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        let Some((entry, path)) = (|| {
            let entry = self
                .studio
                .entries
                .iter()
                .find(|entry| entry.id == viewer.entry)?;
            Some((entry, ImageViewer::cached_path(&self.data_dir, entry)?))
        })() else {
            return overlay
                .items_center()
                .justify_center()
                .gap_4()
                .child("本地图片已不存在")
                .child(viewer_button(
                    "viewer-close-missing",
                    "x",
                    cx.listener(|this, _, window, cx| this.close_studio_image(window, cx)),
                ));
        };
        let id = entry.id;
        let viewport_width = (f32::from(window.viewport_size().width) - 48.).max(1.);
        let viewport_height = (f32::from(window.viewport_size().height) - 120.).max(1.);
        let (width, height) = viewer.natural;
        let (image_width, image_height, scale) =
            image_size(width, height, viewport_width, viewport_height, viewer.zoom);
        viewer.layout.set((viewport_width, viewport_height, scale));
        let fitted = viewer.zoom.is_none();

        // Only zoom out / zoom in, download and close.
        let toolbar = div()
            .w_full()
            .h(px(56.))
            .flex_none()
            .px_5()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(12.))
                    .text_color(rgb(0x9A9BA2))
                    .child(format!("{width} × {height}")),
            )
            .child(viewer_button(
                "viewer-zoom-out",
                "zoom-out",
                cx.listener(|this, _, _, cx| this.step_studio_zoom(-1, cx)),
            ))
            .child(
                div()
                    .w(px(52.))
                    .flex_none()
                    .text_center()
                    .text_size(px(13.))
                    .text_color(rgb(0xD4D4D8))
                    .child(format!("{:.0}%", scale * 100.)),
            )
            .child(viewer_button(
                "viewer-zoom-in",
                "zoom-in",
                cx.listener(|this, _, _, cx| this.step_studio_zoom(1, cx)),
            ))
            .child(div().w(px(1.)).h(px(18.)).mx(px(6.)).bg(rgba(0xFFFFFF26)))
            .child(viewer_button(
                "viewer-save",
                "download",
                cx.listener(move |this, _, window, cx| {
                    if accepts(this) {
                        this.save_studio_media(id, window, cx);
                    }
                }),
            ))
            .child(viewer_button(
                "viewer-close",
                "x",
                cx.listener(|this, _, window, cx| this.close_studio_image(window, cx)),
            ));
        let image = div()
            .id("studio-image-scroll")
            .w(px(viewport_width))
            .h(px(viewport_height))
            .flex_none()
            .overflow_scroll()
            .track_scroll(&viewer.scroll)
            .child(
                div()
                    .id("studio-image-canvas")
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        if !event.modifiers.control {
                            return;
                        }
                        cx.stop_propagation();
                        let delta = f32::from(event.delta.pixel_delta(px(20.)).y);
                        if delta.abs() >= 0.5 {
                            this.step_studio_zoom(if delta > 0. { 1 } else { -1 }, cx);
                        }
                    }))
                    .w(px(image_width.max(viewport_width)))
                    .h(px(image_height.max(viewport_height)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() >= 2 && accepts(this) {
                            this.set_studio_zoom(if fitted { Some(1.) } else { None }, cx);
                        }
                    }))
                    .child(
                        img(path)
                            .w(px(image_width))
                            .h(px(image_height))
                            .flex_none()
                            .object_fit(ObjectFit::Fill)
                            .with_fallback(|| {
                                div()
                                    .text_size(px(14.))
                                    .child("无法打开本地图片")
                                    .into_any_element()
                            }),
                    ),
            );
        let notice = self.studio.notice.clone();
        overlay
            .child(toolbar)
            .child(div().px_6().child(image))
            .child(
                div()
                    .h(px(64.))
                    .w_full()
                    .flex_none()
                    .px_6()
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(13.))
                    .text_color(rgb(0xB0B1B8))
                    .child(notice),
            )
    }
}

fn viewer_button(
    id: &'static str,
    icon: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(34.))
        .flex_none()
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(rgba(0xFFFFFF1F)))
        .active(|style| style.bg(rgba(0xFFFFFF2E)))
        .child(super::chrome::glyph(icon, 18., 0xF5F6F6))
        .on_click(on_click)
}

/// Pictures folder of the current user, or the app data folder.
fn default_save_dir(data_dir: &std::path::Path) -> std::path::PathBuf {
    std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .map(|home| {
            let pictures = home.join("Pictures");
            if pictures.is_dir() { pictures } else { home }
        })
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| data_dir.to_path_buf())
}

/// "Muse-<start of prompt>-1122-1030.png", safe as a Windows file name.
fn suggested_file_name(entry: &studio::Entry, extension: &str) -> String {
    let topic = entry
        .prompt
        .chars()
        .filter(|ch| !ch.is_control() && !r#"\/:*?"<>|"#.contains(*ch))
        .take(18)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let stamp = (entry.created_at > 0)
        .then(|| crate::platform::local_time(entry.created_at))
        .flatten()
        .map(|time| {
            format!(
                "{:02}{:02}-{:02}{:02}{:02}",
                time.month, time.day, time.hour, time.minute, time.second
            )
        })
        .unwrap_or_default();
    let mut name = String::from("Muse");
    for part in [topic.trim(), stamp.as_str()] {
        if !part.is_empty() {
            name.push('-');
            name.push_str(part);
        }
    }
    format!("{name}.{extension}")
}

/// Clipboard data for a picture: a 24-bit DIB (what chat apps and Office read)
/// and PNG bytes (what browsers and editors prefer, keeping transparency).
fn clipboard_payload(path: &std::path::Path) -> Result<(Vec<u8>, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|_| "无法读取图片文件".to_owned())?;
    let picture = image::load_from_memory(&bytes).map_err(|_| "无法解码这张图片".to_owned())?;
    let rgba = picture.to_rgba8();
    let png = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        bytes
    } else {
        let mut encoded = std::io::Cursor::new(Vec::new());
        picture
            .write_to(&mut encoded, image::ImageFormat::Png)
            .map_err(|_| "图片转换失败".to_owned())?;
        encoded.into_inner()
    };
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 80_000_000 {
        return Err("图片尺寸不受支持".into());
    }
    let stride = (width as usize * 3 + 3) & !3;
    let size = stride * height as usize;
    let mut dib = Vec::with_capacity(40 + size);
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&(width as i32).to_le_bytes());
    dib.extend_from_slice(&(height as i32).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&24u16.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&(size as u32).to_le_bytes());
    dib.extend_from_slice(&2835i32.to_le_bytes());
    dib.extend_from_slice(&2835i32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    let padding = stride - width as usize * 3;
    for y in (0..height).rev() {
        for x in 0..width {
            let [r, g, b, a] = rgba.get_pixel(x, y).0;
            let alpha = u16::from(a);
            let blend =
                |channel: u8| ((u16::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8;
            dib.extend_from_slice(&[blend(b), blend(g), blend(r)]);
        }
        dib.extend(std::iter::repeat(0u8).take(padding));
    }
    Ok((dib, png))
}

#[cfg(target_os = "windows")]
fn write_image_clipboard(window: &Window, dib: &[u8], png: &[u8]) -> Result<(), String> {
    use windows::Win32::{
        Foundation::{GlobalFree, HANDLE},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW,
                SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
    };
    let owner = crate::windows_host::hwnd(window)?;
    const CF_DIB: u32 = 8;
    fn place(format: u32, bytes: &[u8]) -> bool {
        unsafe {
            let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else {
                return false;
            };
            let target = GlobalLock(memory);
            if target.is_null() {
                let _ = GlobalFree(Some(memory));
                return false;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(memory);
            if SetClipboardData(format, Some(HANDLE(memory.0))).is_ok() {
                true
            } else {
                let _ = GlobalFree(Some(memory));
                false
            }
        }
    }
    let mut opened = false;
    for _ in 0..10 {
        if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    if !opened {
        return Err("剪贴板正被其他程序占用，请稍后再试".into());
    }
    let placed = unsafe {
        let _ = EmptyClipboard();
        let bitmap = place(CF_DIB, dib);
        let format = RegisterClipboardFormatW(windows::core::w!("PNG"));
        let portable = format != 0 && place(format, png);
        let _ = CloseClipboard();
        bitmap || portable
    };
    if placed {
        Ok(())
    } else {
        Err("复制失败，请重试".into())
    }
}
