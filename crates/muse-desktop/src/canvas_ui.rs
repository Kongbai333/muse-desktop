//! Bridge the canvas to the existing account-bound studio queue.
use super::*;
use crate::{
    canvas_host::{self, CanvasHost},
    studio::{self, Entry, Job, Mode, Phase},
};
use serde_json::{Value, json};

impl MuseWindow {
    fn canvas_bounds(&self, window: &Window) -> Rect {
        let layout = self.chrome_layout(window, Instant::now());
        Rect {
            position: LogicalPosition::new(layout.left as f64, crate::theme::TITLEBAR as f64)
                .into(),
            size: LogicalSize::new(
                layout.width.max(1.) as f64,
                (f32::from(window.viewport_size().height)
                    - crate::theme::TITLEBAR
                    - crate::theme::STATUS)
                    .max(1.) as f64,
            )
            .into(),
        }
    }

    pub(super) fn sync_canvas(&mut self, window: &Window) {
        let bounds = self.canvas_bounds(window);
        let visible = self.show_start_page
            && self.canvas_open
            && !self.has_browser_overlay()
            && self.studio.viewer.is_none();
        if let Some(canvas) = &mut self.canvas {
            canvas.sync_theme(self.display.theme, false);
            canvas.sync(bounds, visible);
        }
    }

    pub(super) fn tick_canvas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.canvas.is_none() && self.canvas_open && self.show_start_page {
            match CanvasHost::new(
                window,
                &self.data_dir,
                self.canvas_bounds(window),
                self.display.theme,
            ) {
                Ok(canvas) => self.canvas = Some(canvas),
                Err(error) => {
                    self.canvas_open = false;
                    self.status = error;
                    cx.notify();
                }
            }
        }
        let messages = self
            .canvas
            .as_ref()
            .map(|c| c.receiver.try_iter().take(16).collect::<Vec<_>>())
            .unwrap_or_default();
        for message in messages {
            match message["kind"].as_str().unwrap_or("") {
                "ready" => {
                    if let Some(canvas) = &mut self.canvas {
                        canvas.ready = true;
                        canvas.accounts.clear();
                        canvas.last_results.clear();
                        canvas.sync_theme(self.display.theme, true);
                    }
                    self.send_canvas_loaded(None);
                }
                "canvasAction" => self.handle_canvas_action(&message, cx),
                "studio" => {
                    if let Ok(id) = canvas_id(&message) {
                        if self.studio.conversations.iter().any(|c| c.id == id) {
                            self.studio.select_conversation(id);
                        }
                    }
                    self.canvas_open = false;
                    self.sync_canvas(window);
                    window.focus(&self.focus);
                    cx.notify();
                }
                "save" => {
                    let result = if self.save_enabled {
                        canvas_id(&message).and_then(|id| {
                            self.canvas
                                .as_ref()
                                .ok_or_else(|| "画布尚未加载".to_owned())?
                                .library
                                .save_graph(&self.data_dir, id, &message["graph"])
                        })
                    } else {
                        Err("本地配置不可写，画布暂未保存".into())
                    };
                    self.canvas_reply(json!({"kind":"saved", "canvasId":message["canvasId"], "error":result.err()}));
                }
                "import" => {
                    let result = if self.save_enabled {
                        canvas_host::import_media(
                            &self.data_dir,
                            message["data"].as_str().unwrap_or(""),
                        )
                    } else {
                        Err("本地配置不可写，无法导入媒体".into())
                    };
                    self.canvas_reply(match result {
                        Ok(url) => {
                            json!({"kind":"asset", "requestId":message["requestId"], "url":url})
                        }
                        Err(error) => {
                            json!({"kind":"asset", "requestId":message["requestId"], "error":error})
                        }
                    });
                }
                "generate" => {
                    if let Err(error) = self.submit_canvas(&message, window, cx) {
                        self.canvas_reply(json!({"kind":"result", "nodeId":message["nodeId"],
                            "canvasId":message["canvasId"], "requestId":message["requestId"], "running":false, "error":error}));
                    }
                }
                "cancel" => {
                    let node = message["nodeId"].as_str().unwrap_or("");
                    let Ok(canvas_id) = canvas_id(&message) else {
                        continue;
                    };
                    let key = (canvas_id, node.to_owned());
                    let task = self
                        .canvas
                        .as_ref()
                        .and_then(|c| c.tasks.get(&key))
                        .cloned();
                    if let Some((id, _)) =
                        task.filter(|(_, r)| Some(r.as_str()) == message["requestId"].as_str())
                    {
                        if let Some(account) = self
                            .studio
                            .entries
                            .iter()
                            .find(|e| e.id == id)
                            .map(|e| e.account)
                        {
                            if self
                                .studio
                                .jobs
                                .get(&account)
                                .is_some_and(|job| job.id == id)
                            {
                                self.fail_studio_job(account, "已停止", cx);
                            }
                        }
                        if let Some(canvas) = &mut self.canvas {
                            canvas.tasks.remove(&key);
                        }
                    }
                }
                _ => {}
            }
        }
        self.sync_canvas(window);
        let accounts = self
            .workspace
            .accounts
            .iter()
            .map(|account| {
                json!({
                    "id":account.id, "name":account.name,
                    "running":self.browsers.account_running(account.id),
                    "busy":self.studio.jobs.contains_key(&account.id), "responseFormat":"url"
                })
            })
            .collect::<Vec<_>>();
        let Some(canvas) = &mut self.canvas else {
            return;
        };
        if !canvas.ready {
            return;
        }
        let accounts = json!({"kind":"accounts", "accounts":accounts}).to_string();
        if canvas.accounts != accounts {
            let _ = canvas
                .view
                .evaluate_script(&format!("window.museCanvasDesktop?.receive({accounts})"));
            canvas.accounts = accounts;
        }
        let mut finished = Vec::new();
        let results = canvas.tasks.iter().filter_map(|((canvas_id, node), (id, request))| {
            let Some(entry) = self.studio.entries.iter().find(|entry| entry.id == *id) else {
                finished.push((*canvas_id, node.clone()));
                return Some(json!({"kind":"result", "nodeId":node, "requestId":request,
                    "canvasId":canvas_id, "running":false, "error":"任务记录已移除，请重新生成"}));
            };
            let output = entry.media.as_ref().filter(|p| studio::safe_media_path(&self.data_dir, p).is_some_and(|p| p.is_file()))
                .map(|path| format!("{}/{path}", canvas_host::ORIGIN));
            let mut warning = None;
            if !entry.running {
                match canvas.library.complete_task(&self.data_dir, *canvas_id, node, request, output.as_deref(), &entry.status) {
                    Ok(()) => finished.push((*canvas_id, node.clone())),
                    Err(error) => warning = Some(error),
                }
            }
            Some(json!({"kind":"result", "nodeId":node, "requestId":request, "jobId":id,
                "canvasId":canvas_id, "warning":warning,
                "running":entry.running, "status":entry.status,
                "error":if !entry.running && output.is_none() { Some(&entry.status) } else { None },
                "url":output, "elapsed":self.studio.jobs.get(&entry.account).filter(|job| job.id == *id).map(|job|job.started.elapsed().as_secs()).unwrap_or(0)}))
        }).collect::<Vec<_>>();
        let result_string = serde_json::to_string(&results).unwrap_or_default();
        if canvas.last_results != result_string {
            for result in results {
                canvas.send(result);
            }
            canvas.last_results = result_string;
        }
        for node in finished {
            canvas.tasks.remove(&node);
        }
    }

    fn canvas_reply(&self, message: Value) {
        if let Some(canvas) = &self.canvas {
            canvas.send(message);
        }
    }

    fn send_canvas_loaded(&self, error: Option<String>) {
        let Some(canvas) = &self.canvas else {
            return;
        };
        match canvas.library.graph(&self.data_dir, canvas.library.active) {
            Ok(graph) => {
                let tasks=canvas.tasks.iter().map(|((canvas_id,node_id),(_,request))|json!({"canvasId":canvas_id,"nodeId":node_id,"requestId":request})).collect::<Vec<_>>();
                canvas.send(json!({"kind":"canvasLoaded", "library":canvas.library,
                    "canvasId":canvas.library.active, "graph":graph, "tasks":tasks, "error":error}));
            }
            Err(error) => canvas.send(json!({"kind":"canvasError", "error":error})),
        }
    }

    fn handle_canvas_action(&mut self, message: &Value, cx: &mut Context<Self>) {
        let result = (|| -> Result<(), String> {
            if !self.save_enabled {
                return Err("本地配置不可写，未切换画布".into());
            }
            let from = canvas_id(message)?;
            let canvas = self.canvas.as_mut().ok_or("画布尚未加载")?;
            if from != canvas.library.active {
                return Err("画布已切换，请重试".into());
            }
            canvas
                .library
                .save_graph(&self.data_dir, from, &message["graph"])?;
            match message["action"].as_str().unwrap_or("") {
                "create" => canvas.library.create(
                    &self.data_dir,
                    message["name"].as_str().unwrap_or("未命名画布"),
                )?,
                "select" => {
                    let id = serde_json::from_value(message["targetId"].clone())
                        .map_err(|_| "画布无效")?;
                    canvas.library.select(&self.data_dir, id)?;
                }
                "rename" => {
                    canvas.library.rename(
                        &self.data_dir,
                        from,
                        message["name"].as_str().unwrap_or(""),
                    )?;
                    if let Some(conversation) =
                        self.studio.conversations.iter_mut().find(|c| c.id == from)
                    {
                        conversation.title = format!("画布 · {}", canvas.library.info(from)?.name);
                        self.studio.save(&self.data_dir);
                    }
                }
                "delete" => {
                    if canvas.tasks.keys().any(|(id, _)| *id == from) {
                        return Err("此画布还有生成任务，请完成或停止任务后再删除".into());
                    }
                    canvas.library.remove(&self.data_dir, from)?;
                }
                _ => return Err("不支持的画布操作".into()),
            }
            canvas.last_results.clear();
            Ok(())
        })();
        match result {
            Ok(()) => self.send_canvas_loaded(None),
            Err(error)
                if self
                    .canvas
                    .as_ref()
                    .is_some_and(|c| Some(c.library.active) != canvas_id(message).ok()) =>
            {
                self.send_canvas_loaded(Some(error));
            }
            Err(error) => self.canvas_reply(json!({"kind":"canvasError", "error":error})),
        }
        cx.notify();
    }

    fn submit_canvas(
        &mut self,
        message: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.save_enabled || self.has_browser_overlay() {
            return Err("请先关闭对话框并确认本地配置可写".into());
        }
        let canvas_id = canvas_id(message)?;
        let canvas_name = self
            .canvas
            .as_ref()
            .ok_or("画布尚未加载")?
            .library
            .info(canvas_id)?
            .name
            .clone();
        if self
            .canvas
            .as_ref()
            .is_none_or(|c| c.library.active != canvas_id)
        {
            return Err("画布已切换，请返回原画布后重试".into());
        }
        let node = message["nodeId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() < 128)
            .ok_or("节点无效")?
            .to_owned();
        let request = message["requestId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() < 128)
            .ok_or("任务无效")?
            .to_owned();
        if self
            .canvas
            .as_ref()
            .is_some_and(|c| c.tasks.contains_key(&(canvas_id, node.clone())))
        {
            return Err("这个节点正在生成".into());
        }
        let account: AccountId =
            serde_json::from_value(message["accountId"].clone()).map_err(|_| "请选择执行账号")?;
        let name = self
            .workspace
            .account(account)
            .ok_or("账号不存在，请重新选择")?
            .name
            .clone();
        if !self.browsers.account_running(account) {
            return Err(format!("请先从左侧启动并登录「{name}」"));
        }
        if self.studio.jobs.contains_key(&account) {
            return Err(format!("「{name}」正在处理任务，请稍后重试"));
        }
        let mode = match message["mode"].as_str() {
            Some("image") => Mode::Image,
            Some("video") => Mode::Video,
            _ => return Err("不支持的节点类型".into()),
        };
        let prompt = message["prompt"].as_str().unwrap_or("").trim().to_owned();
        if prompt.is_empty() || prompt.len() > 100_000 {
            return Err("提示词为空或过长，请填写内容或适当缩短后重试".into());
        }
        let ratio = message["ratio"]
            .as_str()
            .filter(|r| studio::ratio_options(mode).contains(r))
            .ok_or("不支持的画幅比例")?
            .to_owned();
        let duration = message["duration"].as_u64().unwrap_or(6);
        if !studio::DURATIONS.iter().any(|d| u64::from(*d) == duration) {
            return Err("不支持的视频时长".into());
        }
        let refs = message["references"].as_array().ok_or("参考图数据无效")?;
        if refs.len() > 1 {
            return Err("桌面账号当前支持一张参考图，请只保留一张参考图或首帧".into());
        }
        let reference = if let Some(value) = refs.first() {
            let value = value.as_str().ok_or("参考图无效")?;
            // JSON workflows can contain embedded images from the supplied UI.
            let imported = value
                .starts_with("data:image/")
                .then(|| canvas_host::import_media(&self.data_dir, value))
                .transpose()?;
            let path =
                canvas_host::reference_path(&self.data_dir, imported.as_deref().unwrap_or(value))
                    .ok_or("参考图不在本机，请重新上传")?;
            Some(crate::studio_reference::from_file(&self.data_dir, &path)?)
        } else {
            None
        };
        let bytes = reference
            .as_ref()
            .map(|r| {
                std::fs::read(&r.path)
                    .map(std::sync::Arc::new)
                    .map_err(|_| "无法读取参考图".to_owned())
            })
            .transpose()?;
        self.canvas
            .as_ref()
            .ok_or("画布尚未加载")?
            .library
            .save_graph(&self.data_dir, canvas_id, &message["graph"])?;
        if !self.studio.conversations.iter().any(|c| c.id == canvas_id) {
            self.studio.conversations.push(studio::Conversation {
                id: canvas_id,
                created_at: crate::page_observer::now_ms(),
                title: format!("画布 · {canvas_name}"),
            });
        }
        let identity = self.studio_session_for(account, canvas_id, window)?;
        let id = uuid::Uuid::new_v4();
        let duration = duration as u8;
        let effective = if reference.is_some() {
            mode.prompt_with_reference(&prompt, &ratio, duration)
        } else {
            mode.prompt(&prompt, &ratio, duration)
        };
        self.studio.entries.push(Entry {
            id,
            conversation: canvas_id,
            account,
            name,
            prompt,
            mode,
            ratio,
            duration,
            created_at: crate::page_observer::now_ms(),
            media_width: None,
            media_height: None,
            text: String::new(),
            status: "正在连接账号…".into(),
            media: None,
            running: true,
            source_tab: Some(identity.tab_id),
            source_url: None,
            media_original: false,
            media_attachment: None,
            reference: reference.map(|r| r.relative),
        });
        self.studio.jobs.insert(
            account,
            Job {
                id,
                identity: Some(identity),
                phase: bytes
                    .map(|data| Phase::Uploading { data, offset: 0 })
                    .unwrap_or(Phase::Preparing),
                started: Instant::now(),
                last_poll: Instant::now() - Duration::from_secs(2),
                pending: false,
                prompt: effective,
                mode,
                recovering: false,
                receipt: None,
            },
        );
        if let Some(canvas) = &mut self.canvas {
            canvas.tasks.insert((canvas_id, node), (id, request));
        }
        self.studio.save(&self.data_dir);
        cx.notify();
        Ok(())
    }
}

fn canvas_id(message: &Value) -> Result<uuid::Uuid, String> {
    serde_json::from_value::<uuid::Uuid>(message["canvasId"].clone())
        .ok()
        .filter(|id| !id.is_nil())
        .ok_or_else(|| "请选择一个有效画布".into())
}
