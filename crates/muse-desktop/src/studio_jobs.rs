use super::studio_ui::StudioPanel;
use super::*;
use crate::studio::{self, Entry, Job, Mode, Phase};
use std::io::Write;

impl MuseWindow {
    pub(super) fn submit_studio(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.account_dialog.is_some() {
            return;
        }
        let mut prompt = self.studio.input.read(cx).text();
        let mut selected = self.studio.selected.clone();
        let mut mentioned = HashSet::new();
        let mut mentions = self.workspace.accounts.iter().collect::<Vec<_>>();
        mentions.sort_by_key(|a| std::cmp::Reverse(a.name.len()));
        for account in mentions {
            let mention = format!("@{}", account.name);
            if prompt.contains(&mention) {
                if !self.browsers.account_running(account.id) {
                    self.studio.notice = format!("「{}」尚未启动，请先点击账号卡片", account.name);
                    cx.notify();
                    return;
                }
                mentioned.insert(account.id);
                prompt = prompt.replace(&mention, "");
            }
        }
        if !mentioned.is_empty() {
            selected = mentioned;
        }
        let prompt = prompt.trim();
        if prompt.starts_with('@') {
            self.studio.notice = "没有找到这个账号，请输入 @ 从已启动账号中选择".into();
            cx.notify();
            return;
        }
        if prompt.is_empty() {
            self.studio.notice = "先输入要发送的内容".into();
            cx.notify();
            return;
        }
        if prompt.len() > 100_000 {
            self.studio.notice = "消息过长，请缩短后发送".into();
            cx.notify();
            return;
        }
        if selected.is_empty()
            && let Some(id) = self
                .workspace
                .active_account_id()
                .filter(|id| self.browsers.account_running(*id))
        {
            selected.insert(id);
        }
        if let Some(account) = selected
            .iter()
            .find(|id| self.studio.jobs.contains_key(id))
            .and_then(|id| self.workspace.account(*id))
        {
            self.studio.notice = format!("「{}」正在处理任务，请等完成或先停止", account.name);
            cx.notify();
            return;
        }
        let targets = selected
            .into_iter()
            .filter(|id| self.browsers.account_running(*id) && !self.studio.jobs.contains_key(id))
            .collect::<Vec<_>>();
        if targets.is_empty() {
            self.studio.notice = "请先启动并登录一个账号，再选择空闲账号发送".into();
            cx.notify();
            return;
        }
        if self.studio.reference_loading {
            self.studio.notice = "参考图还在处理中，请稍候再发送".into();
            cx.notify();
            return;
        }
        let reference = match self.studio.reference.clone() {
            Some(reference) if self.studio.mode != Mode::Chat => {
                match std::fs::read(&reference.path) {
                    Ok(bytes)
                        if !bytes.is_empty()
                            && bytes.len() <= crate::studio_reference::MAX_BYTES =>
                    {
                        Some((reference.relative, std::sync::Arc::new(bytes)))
                    }
                    _ => {
                        self.studio.reference = None;
                        self.studio.notice = "参考图文件已不存在，请重新选择".into();
                        cx.notify();
                        return;
                    }
                }
            }
            _ => None,
        };
        self.studio.notice.clear();
        for account_id in targets {
            let Some(account) = self.workspace.account(account_id) else {
                continue;
            };
            let name = account.name.clone();
            let id = uuid::Uuid::new_v4();
            let mode = self.studio.mode;
            let ratio = studio::ratio_value(mode, self.studio.ratio).to_owned();
            let duration = studio::DURATIONS[self.studio.duration];
            self.studio.entries.push(Entry {
                id,
                conversation: self.studio.active,
                account: account_id,
                name,
                prompt: prompt.to_owned(),
                mode,
                ratio: ratio.clone(),
                duration,
                created_at: crate::page_observer::now_ms(),
                media_width: None,
                media_height: None,
                text: String::new(),
                status: "正在连接账号…".into(),
                media: None,
                running: true,
                source_tab: None,
                source_url: None,
                media_original: false,
                media_attachment: None,
                reference: reference.as_ref().map(|(relative, _)| relative.clone()),
            });
            let identity = self.studio_session(account_id, window);
            match identity {
                Ok(identity) => {
                    if let Some(entry) = self.studio.entry_mut(id) {
                        entry.source_tab = Some(identity.tab_id);
                    }
                    let (prompt, phase) = match &reference {
                        Some((_, data)) => (
                            mode.prompt_with_reference(prompt, &ratio, duration),
                            Phase::Uploading {
                                data: data.clone(),
                                offset: 0,
                            },
                        ),
                        None => (mode.prompt(prompt, &ratio, duration), Phase::Preparing),
                    };
                    if reference.is_some()
                        && let Some(entry) = self.studio.entry_mut(id)
                    {
                        entry.status = "正在上传参考图…".into();
                    }
                    self.studio.jobs.insert(
                        account_id,
                        Job {
                            id,
                            identity: Some(identity),
                            phase,
                            started: Instant::now(),
                            last_poll: Instant::now() - Duration::from_secs(2),
                            pending: false,
                            prompt,
                            mode,
                            recovering: false,
                            receipt: None,
                        },
                    );
                }
                Err(error) => {
                    if let Some(entry) = self.studio.entry_mut(id) {
                        entry.running = false;
                        entry.status = error;
                    }
                }
            }
        }
        self.studio
            .input
            .update(cx, |input, cx| input.set_text("", cx));
        if reference.is_some() {
            self.studio.reference = None;
        }
        self.close_studio_panels(window, cx);
        self.studio.scroll.scroll_to_bottom();
        self.studio.save(&self.data_dir);
        cx.notify();
    }
    fn studio_session(
        &mut self,
        id: AccountId,
        window: &Window,
    ) -> Result<muse_core::BrowserIdentity, String> {
        let conversation = self.studio.active;
        if let Some(identity) = self
            .studio
            .sessions
            .get(&(conversation, id))
            .copied()
            .filter(|identity| self.workspace.contains_identity(*identity))
        {
            self.ensure_identity(identity, window)?;
            return Ok(identity);
        }
        let selected = self.workspace.active_browser_identity();
        let previous = self.workspace.account(id).map(|a| a.active_tab_id());
        let tab = self.workspace.add_tab(id, "https://muse.ai/thread/new")?;
        let identity = muse_core::BrowserIdentity {
            account_id: id,
            tab_id: tab,
            generation: 1,
        };
        self.workspace
            .apply_browser_event(identity, BrowserEvent::Title("主页面会话".into()));
        if let Some(previous) = previous {
            let _ = self.workspace.select_tab(previous);
        }
        if let Some(selected) = selected {
            let _ = self.workspace.select_tab(selected.tab_id);
        }
        self.persist();
        self.ensure_identity(identity, window)?;
        self.studio.sessions.insert((conversation, id), identity);
        Ok(identity)
    }
    pub(super) fn tick_studio(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let revision = self.studio.input.read(cx).revision();
        if revision != self.studio.last_revision {
            self.studio.last_revision = revision;
            if self.studio.input.read(cx).text().ends_with('@') {
                self.open_studio_panel(StudioPanel::Picker);
            }
            cx.notify();
        }
        let ids = self.studio.jobs.keys().copied().collect::<Vec<_>>();
        for account in ids {
            let Some(job) = self.studio.jobs.get(&account) else {
                continue;
            };
            let limit = if job.recovering {
                90
            } else if job.mode == Mode::Chat {
                300
            } else {
                900
            };
            if job.started.elapsed() > Duration::from_secs(limit) {
                self.fail_studio_job(account, "等待 Muse 响应超时，可打开任务页面查看", cx);
                continue;
            }
            let Some(identity) = job.identity else {
                continue;
            };
            if !self.workspace.contains_identity(identity)
                || self.browsers.view(identity.tab_id).is_none()
            {
                self.fail_studio_job(account, "账号已关闭或代理已变更，请重试", cx);
                continue;
            }
            if let Some(error) = self.browsers.startup_error(identity).map(str::to_owned) {
                self.fail_studio_job(account, &error, cx);
                continue;
            }
            if job.pending {
                if job.last_poll.elapsed() > Duration::from_secs(8) {
                    self.fail_studio_job(account, "页面没有回应，请打开任务页面检查", cx);
                }
                continue;
            }
            let interval = if matches!(
                job.phase,
                Phase::Downloading { .. } | Phase::Uploading { .. }
            ) {
                0
            } else {
                400
            };
            if !self.browsers.page_ready(identity)
                || job.last_poll.elapsed() < Duration::from_millis(interval)
            {
                continue;
            }
            let request = match &job.phase {
                Phase::Uploading { data, offset } if *offset < data.len() => {
                    use base64::Engine;
                    let end = (*offset + 24 * 1024).min(data.len());
                    format!(
                        "window.__museStudio?.uploadChunk({},{},{},{})",
                        serde_json::to_string(&job.id.to_string()).unwrap(),
                        offset,
                        serde_json::to_string(
                            &base64::engine::general_purpose::STANDARD.encode(&data[*offset..end])
                        )
                        .unwrap(),
                        end == data.len()
                    )
                }
                Phase::Uploading { .. } => format!(
                    "window.__museStudio?.commitUpload({},'reference.jpg','image/jpeg')",
                    serde_json::to_string(&job.id.to_string()).unwrap()
                ),
                Phase::Preparing => format!(
                    "window.__museStudio?.begin({},{},{})",
                    serde_json::to_string(&job.id.to_string()).unwrap(),
                    serde_json::to_string(job.mode.key()).unwrap(),
                    serde_json::to_string(&job.prompt).unwrap()
                ),
                Phase::Recovering { thumbnail } => format!(
                    "window.__museStudio?.recover({},{})",
                    serde_json::to_string(&job.id.to_string()).unwrap(),
                    serde_json::json!({"thumbnail":thumbnail})
                ),
                Phase::Watching => format!("window.__museStudio?.poll('{}')", job.id),
                Phase::Downloading { bytes, .. } => {
                    format!("window.__museStudio?.read('{}',{})", job.id, bytes)
                }
            };
            let script = format!(
                "(()=>{{try{{return {request} ?? JSON.stringify({{id:'{}',state:'error',error:'账号页面尚未准备好，请重新打开后重试'}});}}catch{{return JSON.stringify({{id:'{}',state:'error',error:'网页适配失败，请打开任务页面查看'}})}}}})()",
                job.id, job.id
            );
            let id = job.id;
            if self.browsers.evaluate_studio(identity, id, &script)
                && let Some(job) = self.studio.jobs.get_mut(&account)
            {
                job.pending = true;
                job.last_poll = Instant::now();
            }
        }
    }
    pub(super) fn accept_studio_reply(
        &mut self,
        identity: muse_core::BrowserIdentity,
        id: uuid::Uuid,
        raw: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(job) = self
            .studio
            .jobs
            .get_mut(&identity.account_id)
            .filter(|j| j.id == id && j.identity == Some(identity))
        else {
            return;
        };
        job.pending = false;
        let Some(reply) = studio::decode(raw) else {
            self.fail_studio_job(identity.account_id, "网页返回了无法读取的结果", cx);
            return;
        };
        if reply.id.is_some_and(|other| other != id) {
            self.fail_studio_job(identity.account_id, "任务标识已变化，请重新发送", cx);
            return;
        }
        let account = identity.account_id;
        if !reply.text.is_empty()
            && let Some(entry) = self.studio.entry_mut(id)
        {
            entry.text = reply.text.chars().take(100_000).collect();
            self.studio.scroll.scroll_to_bottom();
        }
        match reply.state.as_str() {
            "waiting" => {
                if let Some(entry) = self.studio.entry_mut(id) {
                    entry.status = "正在等待账号连接，请确认已在 Muse 登录".into();
                }
            }
            "uploading" | "uploaded" => {
                let Some(job) = self.studio.jobs.get_mut(&account) else {
                    return;
                };
                let Phase::Uploading { data, offset } = &mut job.phase else {
                    self.fail_studio_job(account, "参考图上传状态无效", cx);
                    return;
                };
                let expected = (*offset + 24 * 1024).min(data.len());
                if reply.offset != expected
                    || (reply.state == "uploaded") != (expected == data.len())
                {
                    self.fail_studio_job(account, "参考图上传中断，请重新发送", cx);
                    return;
                }
                *offset = expected;
                let total = data.len();
                let percent = expected * 100 / total.max(1);
                if let Some(entry) = self.studio.entry_mut(id) {
                    entry.status = if expected == total {
                        "正在准备参考图…".to_owned()
                    } else {
                        format!("正在上传参考图 · {percent}%")
                    };
                }
            }
            "attached" => {
                let Some(job) = self.studio.jobs.get_mut(&account) else {
                    return;
                };
                if !matches!(&job.phase, Phase::Uploading { data, offset } if *offset == data.len())
                {
                    self.fail_studio_job(account, "参考图上传状态无效", cx);
                    return;
                }
                // Give Muse a moment to take the attachment before the prompt goes in;
                // sending still waits until the page enables its send button.
                job.phase = Phase::Preparing;
                job.last_poll = Instant::now() + Duration::from_millis(1200);
                if let Some(entry) = self.studio.entry_mut(id) {
                    entry.status = "参考图已附上，正在准备生成…".into();
                }
            }
            "preparing" => {
                self.studio.jobs.get_mut(&account).unwrap().phase = Phase::Watching;
            }
            "sending" | "running" | "downloading" | "recovering" => {
                self.studio.jobs.get_mut(&account).unwrap().phase = Phase::Watching;
                if let Some(entry) = self.studio.entry_mut(id) {
                    entry.status = match reply.state.as_str() {
                        "sending" => "正在发送…",
                        "downloading" => "正在取回生成文件…",
                        "recovering" => "正在从原任务重新读取原图…",
                        _ => "Muse 正在回复 / 生成…",
                    }
                    .into();
                }
            }
            "done" => {
                if reply.text.is_empty() {
                    self.fail_studio_job(account, "Muse 没有返回新的回复", cx);
                } else {
                    self.complete_studio_job(account, id, None, cx);
                }
            }
            "media" => {
                let mode = self.studio.jobs[&account].mode;
                if mode == Mode::Image
                    && (!reply.receipt.original
                        || reply.receipt.width == 0
                        || reply.receipt.height == 0)
                {
                    self.fail_studio_job(account, "尚未取得原始图片，没有保存缩略图", cx);
                    return;
                }
                let extension = match reply.mime.as_str() {
                    "image/png" => Some("png"),
                    "image/jpeg" => Some("jpg"),
                    "image/webp" => Some("webp"),
                    "image/gif" => Some("gif"),
                    "video/mp4" => Some("mp4"),
                    "video/webm" => Some("webm"),
                    "video/quicktime" => Some("mov"),
                    _ => None,
                };
                let correct = if mode == Mode::Video {
                    reply.mime.starts_with("video/")
                } else {
                    mode == Mode::Image && reply.mime.starts_with("image/")
                };
                if !correct
                    || reply.bytes == 0
                    || reply.bytes > 128 * 1024 * 1024
                    || extension.is_none()
                {
                    self.fail_studio_job(account, "Muse 返回了不匹配的媒体类型或过大的文件", cx);
                    return;
                }
                let suffix = if self.studio.jobs[&account].recovering {
                    format!("-original-{}", uuid::Uuid::new_v4())
                } else {
                    String::new()
                };
                let relative = format!("media/{id}{suffix}.{}", extension.unwrap());
                let file = studio::safe_media_path(&self.data_dir, &relative).unwrap();
                let prepare = std::fs::create_dir_all(self.data_dir.join("media")).and_then(|_| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&file)
                });
                if prepare.is_err() {
                    self.fail_studio_job(account, "无法保存生成文件", cx);
                    return;
                }
                self.studio.jobs.get_mut(&account).unwrap().phase = Phase::Downloading {
                    bytes: 0,
                    total: reply.bytes,
                    file,
                };
                self.studio.jobs.get_mut(&account).unwrap().receipt = Some(reply.receipt);
                if let Some(entry) = self.studio.entry_mut(id) {
                    entry.status = format!("下载中 · {:.1} MB", reply.bytes as f64 / 1048576.);
                }
            }
            "chunk" => {
                use base64::Engine;
                let Some(job) = self.studio.jobs.get_mut(&account) else {
                    return;
                };
                let Phase::Downloading { bytes, file, total } = &mut job.phase else {
                    return;
                };
                let data = base64::engine::general_purpose::STANDARD
                    .decode(&reply.data)
                    .ok();
                let Some(data) = data.filter(|d| {
                    !d.is_empty()
                        && d.len() <= 65_536
                        && *bytes == reply.offset
                        && *bytes + d.len() <= *total
                        && (!reply.eof || *bytes + d.len() == *total)
                }) else {
                    self.fail_studio_job(account, "生成文件传输中断", cx);
                    return;
                };
                let saved = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&*file)
                    .and_then(|mut f| f.write_all(&data));
                if saved.is_err() {
                    self.fail_studio_job(account, "写入生成文件失败", cx);
                    return;
                }
                *bytes += data.len();
                if reply.eof {
                    if job.mode == Mode::Image
                        && !image::image_dimensions(&*file)
                            .ok()
                            .is_some_and(|(width, height)| {
                                job.receipt.as_ref().is_some_and(|receipt| {
                                    receipt.original
                                        && width == receipt.width
                                        && height == receipt.height
                                })
                            })
                    {
                        self.fail_studio_job(account, "原图文件尺寸校验失败，旧预览仍保留", cx);
                        return;
                    }
                    let relative = format!("media/{}", file.file_name().unwrap().to_string_lossy());
                    self.complete_studio_job(account, id, Some(relative), cx);
                }
            }
            "cancelled" => self.fail_studio_job(account, "已停止", cx),
            "error" => self.fail_studio_job(
                account,
                if reply.error.is_empty() {
                    "Muse 请求失败"
                } else {
                    &reply.error
                },
                cx,
            ),
            _ => self.fail_studio_job(account, "网页返回了未知任务状态", cx),
        }
        cx.notify();
    }
    fn complete_studio_job(
        &mut self,
        account: AccountId,
        id: uuid::Uuid,
        media: Option<std::string::String>,
        cx: &mut Context<Self>,
    ) {
        let job = self.studio.jobs.remove(&account);
        let source_url = job
            .as_ref()
            .and_then(|job| job.identity)
            .and_then(|identity| self.browsers.view(identity.tab_id))
            .and_then(|view| view.url().ok())
            .filter(|url| muse_core::trusted_muse_url(url));
        if let Some(identity) = job.as_ref().and_then(|job| job.identity) {
            let _ = self.browsers.evaluate_studio(
                identity,
                id,
                &format!("window.__museStudio?.release('{id}')"),
            );
        }
        if media.is_some() {
            self.studio.fresh_media.insert(id, Instant::now());
        }
        if let Some(entry) = self.studio.entry_mut(id) {
            entry.running = false;
            entry.status = "已完成".into();
            entry.media = media;
            if let Some(url) = source_url {
                entry.source_url = Some(url);
            }
            if let Some(receipt) = job.as_ref().and_then(|job| job.receipt.as_ref()) {
                entry.media_original = entry.mode == Mode::Image && receipt.original;
                entry.media_attachment =
                    (!receipt.attachment.is_empty()).then(|| receipt.attachment.clone());
            }
            studio::update_media_dimensions(&self.data_dir, entry);
        }
        self.studio.save(&self.data_dir);
        cx.notify();
    }
    pub(super) fn fail_studio_job(
        &mut self,
        account: AccountId,
        message: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(job) = self.studio.jobs.remove(&account) else {
            return;
        };
        if let Some(identity) = job.identity {
            let _ = self.browsers.evaluate_studio(
                identity,
                job.id,
                &format!("window.__museStudio?.cancel('{}')", job.id),
            );
        }
        if let Phase::Downloading { file, .. } = job.phase {
            let _ = std::fs::remove_file(file);
        }
        if let Some(entry) = self.studio.entry_mut(job.id) {
            entry.running = false;
            entry.status = message.chars().take(240).collect();
        }
        self.studio.save(&self.data_dir);
        cx.notify();
    }
    pub(super) fn open_studio_task(
        &mut self,
        account: AccountId,
        entry: uuid::Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saved = self
            .studio
            .entries
            .iter()
            .find(|e| e.id == entry)
            .and_then(|e| e.source_tab)
            .and_then(|id| {
                self.workspace
                    .account(account)?
                    .tabs
                    .iter()
                    .find(|t| t.id == id)
                    .map(|tab| muse_core::BrowserIdentity {
                        account_id: account,
                        tab_id: tab.id,
                        generation: tab.generation,
                    })
            });
        let conversation = self
            .studio
            .entries
            .iter()
            .find(|e| e.id == entry)
            .map_or(self.studio.active, |e| e.conversation);
        if let Some(identity) = saved.or_else(|| {
            self.studio
                .sessions
                .get(&(conversation, account))
                .copied()
                .filter(|i| self.workspace.contains_identity(*i))
        }) {
            let _ = self.workspace.select_tab(identity.tab_id);
            self.navigation_transition = TransitionState::entering();
            self.show_start_page = false;
            self.sync_fields(cx);
            self.ensure_active(window);
            cx.notify();
        } else {
            self.select_account(account, window, cx);
        }
    }
}
