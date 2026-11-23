//! Application update UI; workers never hold a GPUI or browser handle.
use super::*;
use crate::{
    updater::{Result, Update},
    updater_download::Prepared,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    Checking,
    Latest,
    Available,
    Downloading,
    Ready,
    Launching,
    Failed,
}
enum Reply {
    Checked(u64, Result<Option<Update>>),
    Progress(u64, u64),
    Prepared(u64, Result<Prepared>),
    Helper(u64, Prepared, Result<crate::updater_install::LaunchOutcome>),
    #[cfg(target_os = "macos")]
    Handoff(u64, Prepared, Result<()>),
}
pub(super) struct UpdateUi {
    pub open: bool,
    phase: Phase,
    update: Option<Update>,
    prepared: Option<Prepared>,
    message: String,
    received: u64,
    generation: u64,
    cancel: Arc<AtomicBool>,
    sender: mpsc::Sender<Reply>,
    receiver: mpsc::Receiver<Reply>,
    started: Instant,
    automatic: bool,
    notice: bool,
    last_attempt: Option<Instant>,
    source: crate::updater_download::Source,
    notify_on_result: bool,
    #[cfg(target_os = "macos")]
    handoff_allowed: bool,
}
impl UpdateUi {
    pub fn new(root: &std::path::Path, automatic: bool) -> Self {
        let (sender, receiver) = mpsc::channel();
        let now = crate::page_observer::now_ms();
        let cache = root.join("update-check.json");
        let recent = std::fs::metadata(&cache)
            .ok()
            .filter(|m| m.len() < 128)
            .and_then(|_| std::fs::read(&cache).ok())
            .and_then(|b| serde_json::from_slice::<u64>(&b).ok())
            .is_some_and(|at| at <= now && now - at < 6 * 60 * 60 * 1000);
        let outcome = root.join("update-result.json");
        let result = std::fs::metadata(&outcome)
            .ok()
            .filter(|m| m.len() < 4096)
            .and_then(|_| std::fs::read(&outcome).ok())
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let failed = result.as_ref().is_some_and(|r| r["installed"] == false);
        let message = if failed {
            result
                .as_ref()
                .and_then(|r| r["error"].as_str())
                .filter(|s| s.chars().count() < 500)
                .unwrap_or("更新未完成，旧版与回退副本已保留，请重新检查更新")
                .to_owned()
        } else {
            String::new()
        };
        if result.is_some() {
            let _ = std::fs::remove_file(outcome);
        }
        Self {
            open: false,
            phase: if failed { Phase::Failed } else { Phase::Idle },
            update: None,
            prepared: None,
            message,
            received: 0,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            sender,
            receiver,
            started: Instant::now(),
            automatic: automatic && !recent,
            notice: failed,
            last_attempt: None,
            source: crate::updater_download::Source::new(root, !automatic),
            notify_on_result: false,
            #[cfg(target_os = "macos")]
            handoff_allowed: false,
        }
    }
}
impl Drop for UpdateUi {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl MuseWindow {
    pub(super) fn show_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_browser_overlay() && !self.updates.open {
            return;
        }
        self.updates.open = true;
        self.updates.notice = false;
        self.ensure_page_freeze();
        window.focus(&self.focus);
        if matches!(self.updates.phase, Phase::Idle | Phase::Latest)
            || (self.updates.phase == Phase::Failed && self.updates.update.is_none())
        {
            self.check_updates(true);
        }
        self.browsers
            .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
        cx.notify();
    }
    fn check_updates(&mut self, manual: bool) {
        let d = &mut self.updates;
        if matches!(
            d.phase,
            Phase::Checking | Phase::Downloading | Phase::Launching
        ) {
            return;
        }
        if d.last_attempt
            .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
        {
            if manual {
                d.message = "刚刚检查过，请稍后重试".into();
            }
            return;
        }
        d.automatic = false;
        d.last_attempt = Some(Instant::now());
        d.generation += 1;
        let id = d.generation;
        d.notify_on_result = !manual;
        d.phase = Phase::Checking;
        d.message = "正在检查GitHub官方稳定版本…".into();
        let sender = d.sender.clone();
        let root = self.data_dir.clone();
        let source = d.source.clone();
        std::thread::spawn(move || {
            let result = source.check();
            // Cache the attempt, including offline/rate-limit failures, to avoid
            // repeatedly hitting GitHub across application restarts.
            let _ = crate::account_cache::write(
                &root,
                "update-check.json",
                crate::page_observer::now_ms().to_string().as_bytes(),
            );
            let _ = sender.send(Reply::Checked(id, result));
        });
    }
    pub(super) fn close_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.updates.phase == Phase::Launching {
            return;
        }
        if self.updates.phase == Phase::Downloading {
            self.cancel_update();
        }
        self.updates.notify_on_result = false;
        self.updates.open = false;
        window.focus(&self.focus);
        self.ensure_active(window);
        cx.notify();
    }
    fn cancel_update(&mut self) {
        self.updates.cancel.store(true, Ordering::Release);
        self.updates.generation += 1;
        self.updates.phase = Phase::Available;
        self.updates.message = "已取消下载，未安装更新".into();
        self.updates.received = 0;
    }
    pub(super) fn download_update(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.updates.phase,
            Phase::Downloading | Phase::Launching | Phase::Checking
        ) {
            return;
        }
        let Some(update) = self.updates.update.clone() else {
            self.check_updates(true);
            cx.notify();
            return;
        };
        let d = &mut self.updates;
        d.generation += 1;
        let id = d.generation;
        d.received = 0;
        d.prepared = None;
        d.phase = Phase::Downloading;
        d.message = "正在下载并校验更新包…".into();
        d.cancel = Arc::new(AtomicBool::new(false));
        let cancel = d.cancel.clone();
        let sender = d.sender.clone();
        let source = d.source.clone();
        std::thread::spawn(move || {
            let mut last = Instant::now();
            let result = source.prepare(update, &cancel, |size| {
                if last.elapsed() >= Duration::from_millis(100) {
                    last = Instant::now();
                    let _ = sender.send(Reply::Progress(id, size));
                }
            });
            let _ = sender.send(Reply::Prepared(id, result));
        });
        cx.notify();
    }
    pub(super) fn install_update(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.updates.phase != Phase::Ready {
            return;
        }
        if !self.save_enabled
            || self.transfer_commit_pending()
            || !self.studio.jobs.is_empty()
            || !self.cookie_logins.is_empty()
        {
            self.updates.message = "请先完成正在进行的账号/创作任务，并确认数据可保存后更新".into();
            cx.notify();
            return;
        }
        if let Err(error) = self.workspace.save(&self.data_dir.join("workspace.json")) {
            self.updates.message = error;
            cx.notify();
            return;
        }
        self.capture_page_zoom();
        self.persist_display();
        self.persist_window();
        self.studio.save(&self.data_dir);
        let Some(mut prepared) = self.updates.prepared.take() else {
            return;
        };
        self.updates.phase = Phase::Launching;
        self.updates.message = "正在准备安全重启，原应用退出前不会替换文件…".into();
        let sender = self.updates.sender.clone();
        let id = self.updates.generation;
        let data_dir = self.data_dir.clone();
        let background = !window.is_window_active();
        std::thread::spawn(move || {
            let result =
                crate::updater_install::launch_helper(&mut prepared, &data_dir, background);
            let _ = sender.send(Reply::Helper(id, prepared, result));
        });
        cx.notify();
    }
    pub(super) fn poll_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.updates.automatic && self.updates.started.elapsed() > Duration::from_secs(4) {
            self.check_updates(false);
        }
        let mut changed = false;
        while let Ok(reply) = self.updates.receiver.try_recv() {
            let id = match &reply {
                Reply::Checked(id, _)
                | Reply::Progress(id, _)
                | Reply::Prepared(id, _)
                | Reply::Helper(id, ..) => *id,
                #[cfg(target_os = "macos")]
                Reply::Handoff(id, ..) => *id,
            };
            if id != self.updates.generation {
                continue;
            }
            match reply {
                Reply::Checked(_, Ok(Some(update))) => {
                    self.updates.phase = Phase::Available;
                    self.updates.message =
                        "发现新版本。点击下载后才会获取更新包，账号与代理数据会保留。".into();
                    self.updates.update = Some(update);
                    self.updates.notice = self.updates.notify_on_result && !self.updates.open;
                }
                Reply::Checked(_, Ok(None)) => {
                    self.updates.phase = Phase::Latest;
                    self.updates.update = None;
                    self.updates.message =
                        format!("当前已是最新稳定版 {}", env!("CARGO_PKG_VERSION"));
                }
                Reply::Checked(_, Err(error)) | Reply::Prepared(_, Err(error)) => {
                    self.updates.phase = Phase::Failed;
                    self.updates.message = error;
                }
                Reply::Progress(_, size) => self.updates.received = size,
                Reply::Prepared(_, Ok(prepared)) => {
                    #[cfg(target_os = "macos")]
                    {
                        self.updates.handoff_allowed = false;
                    }
                    self.updates.received = prepared.update.size;
                    self.updates.phase = Phase::Ready;
                    self.updates.message =
                        "SHA256与更新包结构校验通过。重启更新将保留账号、代理及原启动参数。".into();
                    self.updates.prepared = Some(prepared);
                }
                Reply::Helper(_, prepared, Err(error)) => {
                    #[cfg(target_os = "macos")]
                    {
                        self.updates.handoff_allowed = false;
                    }
                    self.updates.phase = Phase::Ready;
                    self.updates.message = error;
                    self.updates.prepared = Some(prepared);
                }
                Reply::Helper(
                    _,
                    prepared,
                    Ok(crate::updater_install::LaunchOutcome::Started(attempt)),
                ) => {
                    // A helper-ready receipt is not installation authorization.
                    // Save the latest state, then grant the one commit marker.
                    let saved = self.workspace.save(&self.data_dir.join("workspace.json"));
                    if saved.is_ok()
                        && std::fs::write(attempt.join("install-authorized"), b"install").is_ok()
                    {
                        self.updates.prepared = Some(prepared);
                        cx.quit();
                        return true;
                    }
                    self.updates.phase = Phase::Ready;
                    self.updates.message = "最新状态未保存，已停止重启更新".into();
                    self.updates.prepared = Some(prepared);
                }
                #[cfg(target_os = "macos")]
                Reply::Helper(
                    _,
                    prepared,
                    Ok(crate::updater_install::LaunchOutcome::NeedsPermission),
                ) => {
                    self.updates.phase = Phase::Ready;
                    self.updates.handoff_allowed = true;
                    self.updates.message="Bundle、签名和实际数据目录检查通过，但安装位置不可写。可以在Finder中按系统权限提示交接安装。".into();
                    self.updates.prepared = Some(prepared);
                }
                #[cfg(target_os = "macos")]
                Reply::Handoff(_, mut prepared, result) => {
                    self.updates.phase = Phase::Ready;
                    self.updates.message = match result {
                        Ok(()) => {
                            prepared.keep = true;
                            "Finder已定位通过复核的Muse.app。退出Muse后按系统权限提示替换应用，保留独立的数据目录。本包为ad-hoc签名，未公证。".into()
                        }
                        Err(error) => {
                            self.updates.handoff_allowed = false;
                            error
                        }
                    };
                    self.updates.prepared = Some(prepared);
                }
            }
            changed = true;
        }
        if self.updates.notice && !self.has_browser_overlay() && !self.studio.viewer.is_some() {
            self.updates.open = true;
            self.updates.notice = false;
            self.ensure_page_freeze();
            self.browsers
                .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
            changed = true;
            // Background notifications do not steal native keyboard focus.
        }
        changed
    }
    #[cfg(target_os = "macos")]
    fn reveal_update(&mut self) {
        if !self.updates.handoff_allowed || self.updates.phase != Phase::Ready {
            return;
        }
        let Some(prepared) = self.updates.prepared.take() else {
            return;
        };
        self.updates.phase = Phase::Launching;
        self.updates.message = "正在重新核验Bundle、签名和实际数据目录…".into();
        let sender = self.updates.sender.clone();
        let id = self.updates.generation;
        let data = self.data_dir.clone();
        std::thread::spawn(move || {
            let result = crate::updater_install::verify_handoff(&prepared, &data).and_then(|()| {
                std::process::Command::new("/usr/bin/open")
                    .arg("-R")
                    .arg(&prepared.payload)
                    .spawn()
                    .map(|_| ())
                    .map_err(|_| "无法打开Finder，请重试".into())
            });
            let _ = sender.send(Reply::Handoff(id, prepared, result));
        });
    }
    pub(super) fn render_updates(&self, cx: &Context<Self>) -> Stateful<Div> {
        let d = &self.updates;
        let active = matches!(
            d.phase,
            Phase::Checking | Phase::Downloading | Phase::Launching
        );
        let heading = d
            .update
            .as_ref()
            .map(|u| format!("Muse {} 可用", u.version))
            .unwrap_or_else(|| "检查更新".into());
        let mut actions = div().flex().items_center().justify_end().gap_2();
        if d.phase != Phase::Launching {
            actions = actions.child(chrome::text_button(
                "update-later",
                if d.phase == Phase::Downloading {
                    "取消下载"
                } else {
                    "稍后"
                },
                false,
                cx.listener(|this, _, window, cx| this.close_updates(window, cx)),
            ));
        }
        if !active {
            actions = actions.child(chrome::text_button(
                "update-primary",
                if d.phase == Phase::Ready {
                    "重启并更新"
                } else if d.update.is_some() {
                    "下载更新"
                } else {
                    "重新检查"
                },
                true,
                cx.listener(|this, _, window, cx| {
                    if this.updates.phase == Phase::Ready {
                        this.install_update(window, cx);
                    } else {
                        this.download_update(cx);
                    }
                }),
            ));
        }
        #[cfg(target_os = "macos")]
        if d.phase == Phase::Ready && d.handoff_allowed {
            actions = actions.child(chrome::text_button(
                "update-finder",
                "在Finder中安装",
                false,
                cx.listener(|this, _, _, cx| {
                    this.reveal_update();
                    cx.notify();
                }),
            ));
        }
        let modal = div()
            .w(px(560.))
            .max_h(px(560.))
            .rounded_xl()
            .border_1()
            .border_color(rgb(colors().line))
            .bg(rgb(colors().panel))
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_5()
                    .py_4()
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(chrome::glyph("download", 20., colors().text))
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(heading),
                    ),
            )
            .child(
                div()
                    .id("update-notes")
                    .px_5()
                    .py_4()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(colors().secondary))
                            .child(format!(
                                "当前版本 {} · GitHub / Kongbai333/muse-desktop · {}",
                                env!("CARGO_PKG_VERSION"),
                                crate::updater::platform()
                            )),
                    )
                    .when_some(d.update.as_ref(), |e, u| {
                        e.child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(20.))
                                .child(u.notes.clone()),
                        )
                    })
                    .when(d.phase == Phase::Downloading, |e| {
                        let total = d.update.as_ref().map_or(1, |u| u.size).max(1);
                        let percent = (d.received as f32 / total as f32).clamp(0., 1.);
                        e.child(
                            div()
                                .h(px(5.))
                                .w_full()
                                .rounded_full()
                                .bg(rgb(colors().fill))
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(percent))
                                        .bg(rgb(colors().blue))
                                        .rounded_full(),
                                ),
                        )
                        .child(format!(
                            "{:.1} / {:.1} MB · {:.0}%",
                            d.received as f64 / 1048576.,
                            total as f64 / 1048576.,
                            percent * 100.
                        ))
                    })
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(20.))
                            .text_color(rgb(colors().secondary))
                            .child(d.message.clone()),
                    )
                    .when(cfg!(target_os = "macos"), |e| {
                        e.child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(colors().muted))
                                .child(
                                    "macOS包为ad-hoc签名，未经过Apple公证。安装不会绕过系统权限。",
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .px_5()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(colors().line))
                    .child(actions),
            );
        div()
            .id("update-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .bg(rgba(0x18181B70))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(modal)
    }
}
