use super::*;

impl MuseWindow {
    pub(super) fn new_pool_tab(&self, tab: PoolTab, cx: &mut Context<Self>) -> AccountDialog {
        let mut dialog = AccountDialog::new(
            None,
            false,
            "",
            ProxyDraft {
                enabled: true,
                port: String::new(),
                ..Default::default()
            },
            cx,
        );
        dialog.screen = Some(PoolScreen::List);
        dialog.tab = tab;
        dialog.transition = TransitionState::visible();
        match tab {
            PoolTab::Manual => dialog.set_default_name(
                format!("代理 {}", self.workspace.proxy_profiles.len() + 1),
                cx,
            ),
            PoolTab::Subscriptions => dialog
                .secret
                .update(cx, |input, _| input.set_placeholder("https://…")),
        }
        dialog
    }

    fn select_pool_tab(&mut self, tab: PoolTab, cx: &mut Context<Self>) {
        if !self
            .account_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.screen == Some(PoolScreen::List) && dialog.tab != tab)
        {
            return;
        }
        let mut previous = self.account_dialog.take().unwrap();
        previous.sources_open = false;
        let mut next = previous
            .parked
            .take()
            .map(|dialog| *dialog)
            .unwrap_or_else(|| self.new_pool_tab(tab, cx));
        next.parked = Some(Box::new(previous));
        self.account_dialog = Some(next);
        cx.notify();
    }

    pub(super) fn pool_content(&self, dialog: &AccountDialog, screen: PoolScreen) -> PoolScreen {
        match (screen, dialog.tab) {
            (PoolScreen::List, PoolTab::Subscriptions)
                if self.workspace.subscriptions.is_empty() =>
            {
                PoolScreen::AddSubscription
            }
            (PoolScreen::List, PoolTab::Manual) if self.workspace.proxy_profiles.is_empty() => {
                PoolScreen::Edit(None)
            }
            _ => screen,
        }
    }

    pub(super) fn render_pool_view(
        &self,
        screen: PoolScreen,
        dialog: &AccountDialog,
        window: &Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let root = screen == PoolScreen::List;
        let viewport = window.viewport_size();
        let narrow = viewport.width < px(720.);
        let content = self.pool_content(dialog, screen);
        let header =
            div()
                .h(px(56.))
                .px_4()
                .flex_none()
                .flex()
                .items_center()
                .gap_3()
                .bg(rgb(colors().panel))
                .border_b_1()
                .border_color(rgb(colors().line))
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .window_control_area(gpui::WindowControlArea::Drag)
                        .child(glyph("shield-check", 20., colors().blue))
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("代理池"),
                        ),
                )
                .when(root, |bar| {
                    bar.child(
                        div().flex().gap_1().ml_2().children(
                            [
                                (PoolTab::Subscriptions, "pool-subscriptions", "订阅"),
                                (PoolTab::Manual, "pool-manual", "手动"),
                            ]
                            .map(|(tab, id, label)| {
                                let active = tab == dialog.tab;
                                keyboard_control(div().id(id))
                                    .w(px(66.))
                                    .h(px(32.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .text_color(rgb(if active {
                                        colors().blue
                                    } else {
                                        colors().secondary
                                    }))
                                    .hover(|style| style.bg(rgb(colors().fill_hover)))
                                    .when(active, |el| {
                                        el.bg(rgb(colors().selected))
                                            .font_weight(FontWeight::SEMIBOLD)
                                    })
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_pool_tab(tab, cx)
                                    }))
                            }),
                        ),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .window_control_area(gpui::WindowControlArea::Drag),
                )
                .child(icon_button(
                    "pool-close",
                    "x",
                    "关闭代理池",
                    true,
                    false,
                    cx.listener(|this, _, window, cx| {
                        this.finish_pool_window(cx);
                        window.remove_window();
                    }),
                ));
        let save = |label, kind| {
            dialog_button(
                "pool-save",
                label,
                kind,
                cx.listener(|this, _, window, cx| this.save_account_dialog(window, cx)),
            )
            .active(|style| style.opacity(0.8))
            .into_any_element()
        };
        let notice = || {
            (!dialog.message.is_empty()).then(|| {
                div()
                    .w_full()
                    .text_size(px(12.))
                    .text_color(rgb(if dialog.test_ok == Some(true) {
                        colors().secondary
                    } else {
                        colors().danger
                    }))
                    .child(dialog.message.clone())
            })
        };
        let batch = || {
            link_button(
                "pool-batch",
                "批量导入",
                cx.listener(|this, _, window, cx| {
                    this.show_proxy_pool_screen(PoolScreen::Batch, window, cx)
                }),
            )
        };
        let body = if content == PoolScreen::List {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .p(px(if narrow { 12. } else { 24. }))
                .gap_2()
                .child(if dialog.tab == PoolTab::Subscriptions {
                    self.render_subscriptions(dialog, narrow, cx)
                } else {
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(section_title("手动代理"))
                                .child(div().flex_1())
                                .child(batch())
                                .child(dialog_button(
                                    "pool-new-manual",
                                    "添加代理",
                                    ButtonKind::Primary,
                                    cx.listener(|this, _, window, cx| {
                                        this.show_proxy_pool_screen(
                                            PoolScreen::Edit(None),
                                            window,
                                            cx,
                                        )
                                    }),
                                )),
                        )
                        .child(
                            div()
                                .id("manual-proxies")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(self.render_pool_list(cx)),
                        )
                })
                .children(notice())
        } else {
            let title = match content {
                PoolScreen::AddSubscription if root => "添加第一个订阅",
                PoolScreen::AddSubscription => "添加订阅",
                PoolScreen::Edit(None) if root => "添加第一个手动代理",
                PoolScreen::Edit(None) => "添加手动代理",
                PoolScreen::Edit(Some(_)) => "编辑手动代理",
                PoolScreen::Remove(_) => "删除手动代理",
                PoolScreen::DeleteSubscription(_) => "删除订阅",
                PoolScreen::Batch => "批量导入代理",
                PoolScreen::List => unreachable!(),
            };
            let fields = div()
                .w_full()
                .max_w(px(520.))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(16.))
                .when(!root, |el| {
                    el.child(div().flex().child(link_button(
                        "pool-back",
                        "返回",
                        cx.listener(|this, _, window, cx| this.close_account_dialog(window, cx)),
                    )))
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pb_2()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(21.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .when(root && dialog.tab == PoolTab::Manual, |el| {
                            el.child(batch())
                        }),
                );
            let fields = match content {
                PoolScreen::AddSubscription => fields
                    .child(dialog_field("订阅名称", dialog.name.clone()))
                    .child(dialog_field("订阅地址", dialog.secret.clone()))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(colors().secondary))
                            .child("支持 Clash YAML 或节点列表，订阅地址加密保存。"),
                    )
                    .children(notice())
                    .child(div().flex().child(if dialog.refresh.is_some() {
                        disabled_button("正在读取…")
                    } else {
                        save("读取并添加", ButtonKind::Primary)
                    })),
                PoolScreen::Edit(_) => fields
                    .child(dialog_field("代理名称", dialog.name.clone()))
                    .child(self.render_proxy_form(dialog, cx))
                    .child(div().flex().child(save("保存代理", ButtonKind::Primary))),
                PoolScreen::Batch => fields
                    .child(self.render_proxy_batch(dialog, cx))
                    .children(notice())
                    .child(
                        div().flex().child(
                            if dialog
                                .batch
                                .as_ref()
                                .is_some_and(|batch| batch.can_confirm())
                            {
                                save("确认导入有效项", ButtonKind::Primary)
                            } else {
                                disabled_button("确认导入有效项")
                            },
                        ),
                    ),
                PoolScreen::DeleteSubscription(id) => fields
                    .child(format!(
                        "确认删除「{}」？绑定此订阅的账号将停止联网，直到手动选择新出口。",
                        self.workspace
                            .subscriptions
                            .iter()
                            .find(|sub| sub.id == id)
                            .map(|sub| sub.name.as_str())
                            .unwrap_or_default()
                    ))
                    .children(notice())
                    .child(div().flex().child(save("确认删除订阅", ButtonKind::Danger))),
                PoolScreen::Remove(_) => fields
                    .child(format!(
                        "确认删除「{}」？删除后无法恢复。",
                        dialog.name.read(cx).text()
                    ))
                    .children(notice())
                    .child(div().flex().child(save("确认删除", ButtonKind::Danger))),
                PoolScreen::List => unreachable!(),
            };
            let scroll_id = match content {
                PoolScreen::Edit(_) => "pool-manual-form",
                PoolScreen::AddSubscription => "pool-subscription-form",
                _ => "pool-detail-form",
            };
            div().flex_1().min_h_0().child(
                div().id(scroll_id).size_full().overflow_y_scroll().child(
                    div()
                        .flex()
                        .justify_center()
                        .px(px(if narrow { 18. } else { 32. }))
                        .pt(px(if narrow {
                            20.
                        } else if content == PoolScreen::AddSubscription {
                            64.
                        } else {
                            32.
                        }))
                        .pb_6()
                        .child(fields),
                ),
            )
        };
        div()
            .id("pool-panel")
            .size_full()
            .bg(rgb(colors().surface))
            .flex()
            .flex_col()
            .child(header)
            .child(body)
    }
}
