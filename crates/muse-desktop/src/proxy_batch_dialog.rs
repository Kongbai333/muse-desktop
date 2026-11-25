//! Proxy-pool import sub-screen. Source text never becomes a rendered element.
use super::*;
use crate::proxy_batch::{self, Preview, RowState};

pub(super) struct BatchImport {
    id: Uuid,
    source: Option<Zeroizing<String>>,
    preview: Option<Preview>,
    protocol: ProxyProtocol,
    picking: bool,
}

impl BatchImport {
    pub(super) fn can_confirm(&self) -> bool {
        !self.picking
            && self
                .preview
                .as_ref()
                .is_some_and(|preview| preview.ready > 0)
    }

    pub(super) fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            source: None,
            preview: None,
            protocol: ProxyProtocol::Http,
            picking: false,
        }
    }
}

impl MuseWindow {
    fn set_batch_source(&mut self, source: Zeroizing<String>, cx: &mut Context<Self>) {
        let Some(dialog) = self.account_dialog.as_mut() else {
            return;
        };
        let Some(batch) = dialog.batch.as_mut() else {
            return;
        };
        batch.preview = None;
        // Drop any previous source on rejection too: no stale batch can be confirmed.
        batch.source = None;
        match proxy_batch::validate_size(&source) {
            Ok(()) => {
                batch.source = Some(source);
            }
            Err(error) => {
                dialog.message = error;
                cx.notify();
                return;
            }
        }
        self.preview_proxy_batch(cx);
    }

    fn preview_proxy_batch(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.account_dialog.as_mut() else {
            return;
        };
        let Some(batch) = dialog.batch.as_mut() else {
            return;
        };
        batch.preview = None;
        dialog.message.clear();
        if let Some(source) = batch.source.as_ref() {
            match proxy_pool::import_keys(&self.data_dir, &self.workspace)
                .and_then(|seen| proxy_batch::preview(source, batch.protocol, seen))
            {
                Ok(preview) => batch.preview = Some(preview),
                Err(error) => dialog.message = error,
            }
        }
        cx.notify();
    }

    fn paste_proxy_batch(&mut self, cx: &mut Context<Self>) {
        if self
            .account_dialog
            .as_ref()
            .and_then(|d| d.batch.as_ref())
            .is_none_or(|b| b.picking)
        {
            return;
        }
        let source = Zeroizing::new(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap_or_default(),
        );
        self.set_batch_source(source, cx);
    }

    fn pick_proxy_batch_txt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.account_dialog.as_mut() else {
            return;
        };
        let Some(batch) = dialog.batch.as_mut().filter(|batch| !batch.picking) else {
            return;
        };
        batch.picking = true;
        let id = batch.id;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择 UTF-8 代理 TXT".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let result = match path {
                Some(path) => Some(
                    cx.background_executor()
                        .spawn(async move { proxy_batch::read_txt(&path) })
                        .await,
                ),
                None => None,
            };
            let _ = this.update_in(cx, |this, _, cx| {
                let Some(dialog) = this.account_dialog.as_mut() else {
                    return;
                };
                let Some(batch) = dialog.batch.as_mut().filter(|b| b.id == id) else {
                    return;
                };
                batch.picking = false;
                match result {
                    Some(Ok(source)) => this.set_batch_source(source, cx),
                    Some(Err(error)) => {
                        batch.source = None;
                        batch.preview = None;
                        dialog.message = error;
                    }
                    None => {} // Cancelling the file picker preserves the current preview.
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn confirm_proxy_batch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.account_dialog.as_ref() else {
            return;
        };
        let Some(batch) = dialog.batch.as_ref().filter(|batch| !batch.picking) else {
            return;
        };
        let Some(preview) = batch.preview.as_ref().filter(|preview| preview.ready > 0) else {
            return;
        };
        let errors = preview.errors;
        let result = proxy_pool::append_batch(&self.data_dir, &mut self.workspace, preview);
        match result {
            Ok((imported, duplicates)) => {
                self.show_proxy_pool_screen(PoolScreen::List, window, cx);
                if let Some(dialog) = self.account_dialog.as_mut() {
                    dialog.test_ok = Some(true);
                    dialog.message = format!(
                        "已导入 {imported} 个代理；跳过重复 {duplicates} 项、错误 {errors} 项。未更改账号绑定。"
                    );
                }
            }
            Err(error) => {
                if let Some(dialog) = self.account_dialog.as_mut() {
                    dialog.message = error;
                }
            }
        }
        cx.notify();
    }

    pub(super) fn render_proxy_batch(&self, dialog: &AccountDialog, cx: &Context<Self>) -> Div {
        let batch = dialog.batch.as_ref().expect("batch import screen");
        let mut content = div().flex().flex_col().gap_3()
            .child(div().text_size(px(12.)).line_height(px(19.)).text_color(rgb(colors().secondary))
                .child("每行一条：完整代理 URL、host:port 或 主机:端口:用户名:密码。IPv6 请加方括号。无协议行使用下方默认协议。"))
            .child(div().text_size(px(11.)).line_height(px(18.)).text_color(rgb(colors().muted))
                .child("最多 1 MiB / 1000 项，TXT 须为 UTF-8。仅确认后追加有效项；返回不保存，不自动测试或绑定账号。"))
            .child(div().flex().gap_2()
                .child(dialog_button("batch-paste", "粘贴多行", ButtonKind::Secondary,
                    cx.listener(|this, _, _, cx| this.paste_proxy_batch(cx))).flex_1())
                .child(dialog_button("batch-txt", if batch.picking { "读取中…" } else { "选择 TXT" }, ButtonKind::Secondary,
                    cx.listener(|this, _, window, cx| this.pick_proxy_batch_txt(window, cx))).flex_1()))
            .child(div().text_size(px(12.)).text_color(rgb(colors().secondary)).child("无协议行的默认协议"))
            .child(div().flex().gap_1().children(ProxyProtocol::ALL.into_iter().map(|protocol| {
                div().id(ElementId::Name(format!("batch-{}", protocol.scheme()).into()))
                    .flex_1().h(px(30.)).rounded_md().flex().items_center().justify_center()
                    .bg(rgb(if protocol == batch.protocol { colors().fill } else { colors().panel }))
                    .text_size(px(12.)).text_color(rgb(colors().text)).cursor_pointer().child(protocol.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(batch) = this.account_dialog.as_mut().and_then(|d| d.batch.as_mut()) {
                            batch.protocol = protocol;
                        }
                        this.preview_proxy_batch(cx);
                    }))
            })));
        if let Some(preview) = &batch.preview {
            content = content
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors().text))
                        .child(format!(
                            "可导入 {} · 重复 {} · 错误 {}",
                            preview.ready, preview.duplicates, preview.errors
                        )),
                )
                .children(preview.rows.iter().map(|row| {
                    let (status, color) = match &row.state {
                        RowState::Ready(_) => ("待导入".to_owned(), colors().secondary),
                        RowState::Duplicate => ("重复，跳过".to_owned(), colors().muted),
                        RowState::Invalid(error) => (error.clone(), colors().danger),
                    };
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(rgb(colors().fill))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(17.))
                                .text_color(rgb(color))
                                .child(format!("第 {} 行 · {status}", row.line)),
                        )
                        .when(!row.endpoint.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .text_color(rgb(colors().text))
                                    .overflow_hidden()
                                    .child(row.endpoint.clone()),
                            )
                        })
                        .when(!row.endpoint.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors().muted))
                                    .child(if row.authenticated {
                                        "已设置认证 · 密码已隐藏"
                                    } else {
                                        "无认证"
                                    }),
                            )
                        })
                }));
        }
        content
    }
}
