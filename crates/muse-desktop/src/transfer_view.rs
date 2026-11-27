use super::*;
impl MuseWindow {
    pub(in crate::ui) fn render_transfer(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let d = self.transfer_dialog.as_ref().unwrap();
        let password_field = |input| {
            crate::input::InputField::new(
                input,
                div()
                    .border_1()
                    .border_color(rgb(colors().line))
                    .rounded_lg()
                    .px_2()
                    .py_1(),
            )
        };
        let generation = d.gate.id;
        let mut rows = Vec::new();
        if d.export && d.phase == Phase::Form {
            for account in &self.workspace.accounts {
                let id = account.id;
                let selected = d.selected.contains(&id);
                rows.push(
                    div()
                        .id(SharedString::from(format!("transfer-account-{id}")))
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(if selected {
                            colors().blue
                        } else {
                            colors().line
                        }))
                        .flex()
                        .items_center()
                        .gap_3()
                        .cursor_pointer()
                        .child(if selected { "✓" } else { "□" })
                        .child(div().flex_1().min_w_0().child(account.name.clone()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(d) = this
                                .transfer_dialog
                                .as_mut()
                                .filter(|d| d.gate.accepts(generation) && d.phase == Phase::Form)
                            {
                                if !d.selected.remove(&id) {
                                    d.selected.insert(id);
                                }
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        if let Some(plan) = &d.plan {
            for account in &plan.accounts {
                let route = account
                    .network
                    .proxy_id()
                    .and_then(|id| plan.proxies.iter().find(|p| p.id == id))
                    .map(|p| format!("代理：{}", p.name))
                    .unwrap_or_else(|| "系统网络".into());
                rows.push(
                    div()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(colors().fill))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(account.name.clone())
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(colors().secondary))
                                .child(format!(
                                    "新建隔离账号 · {route} · {} 条 Cookie",
                                    plan.cookies[&account.id].items.len()
                                )),
                        )
                        .into_any_element(),
                );
            }
            for preview in &plan.proxy_previews {
                rows.push(
                    div()
                        .text_size(px(12.))
                        .child(preview.clone())
                        .into_any_element(),
                );
            }
            for warning in &plan.warnings {
                rows.push(
                    div()
                        .text_size(px(12.))
                        .line_height(px(19.))
                        .text_color(rgb(colors().secondary))
                        .child(warning.clone())
                        .into_any_element(),
                );
            }
        }
        let form = d.phase == Phase::Form;
        let preview = d.phase == Phase::Preview;
        let done = d.phase == Phase::Done;
        let enabled = form || (preview && d.acknowledged) || done;
        let title = if d.export {
            "导出账号与静态代理"
        } else {
            "导入加密账号包"
        };
        let primary = if done {
            "完成"
        } else if preview {
            "确认并导入"
        } else if d.export {
            "选择位置并加密导出"
        } else {
            "解密并预览"
        };
        let path = d
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "未选择账号包".into());
        let modal=div().id("transfer-modal").w(px(600.)).max_h(px((f32::from(window.viewport_size().height)-80.).max(360.))).rounded(px(18.)).border_1().border_color(rgb(colors().line))
            .bg(rgb(colors().panel)).shadow_lg().flex().flex_col().overflow_hidden()
            .child(div().px_5().py_4().border_b_1().border_color(rgb(colors().line)).flex_none().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(div().id("transfer-content").px_5().py_4().flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap_3()
                .child(div().text_size(px(12.)).line_height(px(19.)).text_color(rgb(colors().secondary)).child(if d.export{"账号名称、Cookie、上游代理认证和绑定关系全部加密。保留原 Cookie 文本功能；本文件用于跨设备迁移。"}else{"账号包只会新建账号。密码验证及整包校验通过后才显示预览，确认前不会访问 Muse。"}))
                .when(!d.export,|e|e.child(div().flex().items_center().gap_3().child(div().flex_1().text_size(px(12.)).child(path))
                    .when(form,|e|e.child(super::super::chrome::text_button("transfer-pick-file","选择文件",false,cx.listener(|this,_,window,cx|this.pick_transfer_file(window,cx)))))))
                .children(rows)
                .when(form,|e|e.child(div().flex().flex_col().gap_2().child("迁移密码")
                    .child(password_field(d.password.clone()))
                    .when(d.export,|e|e.child("再次输入迁移密码").child(password_field(d.confirmation.clone())))
                    .child(div().text_size(px(11.)).line_height(px(18.)).text_color(rgb(colors().secondary)).child("密码区分大小写，保留空格、换行及 Unicode 原字符；不自动修剪或正规化。"))))
                .when(preview,|e|e.child(div().id("transfer-acknowledge").p_3().rounded_lg().border_1().border_color(rgb(if d.acknowledged{colors().blue}else{colors().line})).cursor_pointer().text_size(px(12.))
                    .child(if d.acknowledged{"✓ 我已核对账号、静态代理和上述路由/Cookie 差异，确认新建并激活。"}else{"□ 我已核对账号、静态代理和上述路由/Cookie 差异，确认新建并激活。"})
                    .on_click(cx.listener(move|this,_,_,cx|{if let Some(d)=this.transfer_dialog.as_mut().filter(|d|d.gate.accepts(generation)&&d.phase==Phase::Preview){d.acknowledged=!d.acknowledged;}cx.notify();}))))
                .child(div().text_size(px(12.)).line_height(px(19.)).text_color(rgb(colors().secondary)).child(d.message.clone())))
            .child(div().px_5().py_3().border_t_1().border_color(rgb(colors().line)).flex_none().flex().justify_end().gap_2()
                .child(super::super::chrome::text_button("transfer-cancel",if done{"关闭"}else{"取消"},false,cx.listener(|this,_,window,cx|this.close_transfer(window,cx))))
                .child(div().id("transfer-primary").px_4().h(px(34.)).rounded_lg().flex().items_center().justify_center().bg(rgb(if enabled{colors().primary}else{colors().fill})).text_color(rgb(if enabled{colors().on_primary}else{colors().muted})).child(primary)
                    .when(enabled,|e|e.cursor_pointer().on_click(cx.listener(move|this,_,window,cx|{if this.transfer_dialog.as_ref().is_none_or(|d|d.gate.id!=generation){return;}if done{this.close_transfer(window,cx);}else{this.submit_transfer(window,cx);}})))));
        div()
            .id("transfer-backdrop")
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
