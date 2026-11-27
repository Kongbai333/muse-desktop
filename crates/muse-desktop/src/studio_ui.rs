//! Native creation workspace, aligned with the supplied reference.
use super::chrome::glyph;
use super::*;
use crate::studio::{self, Mode};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum StudioPanel {
    Mode,
    Picker,
    Ratio,
    Duration,
    EntryMenu(uuid::Uuid, Point<Pixels>),
}

impl MuseWindow {
    pub(super) fn open_studio_panel(&mut self, panel: StudioPanel) {
        self.studio_panel = Some((panel, TransitionState::entering()));
    }

    pub(super) fn toggle_studio_panel(
        &mut self,
        panel: StudioPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.studio_panel.is_some_and(|(current, transition)| {
            !transition.is_exiting()
                && match (current, panel) {
                    (StudioPanel::EntryMenu(a, _), StudioPanel::EntryMenu(b, _)) => a == b,
                    _ => current == panel,
                }
        }) {
            self.close_studio_panels(window, cx);
        } else {
            self.open_studio_panel(panel);
            cx.notify();
        }
    }

    pub(super) fn close_studio_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, transition)) = self.studio_panel.as_mut() else {
            return;
        };
        let Some(generation) = transition.begin_exit() else {
            return;
        };
        self.schedule_overlay_removal(
            MotionKind::Popover,
            generation,
            window,
            cx,
            |this, generation, _, cx| {
                if this
                    .studio_panel
                    .is_some_and(|(_, state)| state.finish_exit(generation))
                {
                    this.studio_panel = None;
                    cx.notify();
                }
            },
        );
    }

    pub(super) fn studio_panel_accepts(&self, transition: TransitionState) -> bool {
        self.studio_panel
            .is_some_and(|(_, current)| current == transition && !current.is_exiting())
    }

    pub(super) fn choose_studio_mode(
        &mut self,
        mode: Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if mode == Mode::Chat {
            return;
        }
        self.studio.mode = mode;
        self.studio.ratio = 0;
        self.close_studio_panels(window, cx);
        self.studio.input.update(cx, |input, cx| {
            input.set_placeholder(match mode {
                Mode::Chat => "输入消息，使用 @ 选择已启动的 Muse 账号",
                Mode::Image => "描述你想生成的画面、风格和细节…",
                Mode::Video => "描述你想生成的画面、镜头或声音…",
            });
            cx.notify();
        });
    }

    pub(super) fn new_studio_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.show_start_page {
            self.show_workspace(window, cx);
        }
        let previous = self.studio.active;
        if self.studio.new_conversation() != previous {
            self.studio.conversation_transition = TransitionState::entering();
        }
        self.studio.conversation_delete = None;
        self.studio.notice.clear();
        self.close_studio_panels(window, cx);
        window.focus(&self.studio.input.read(cx).focus_handle(cx));
        self.studio.save(&self.data_dir);
        cx.notify();
    }

    fn open_studio_conversation(
        &mut self,
        id: uuid::Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.studio.active != id {
            self.studio.conversation_transition = TransitionState::entering();
        }
        self.studio.select_conversation(id);
        self.studio.notice.clear();
        self.close_studio_panels(window, cx);
        self.studio.save(&self.data_dir);
        cx.notify();
    }

    fn delete_studio_conversation(&mut self, id: uuid::Uuid, cx: &mut Context<Self>) {
        if self.studio.conversation_delete != Some(id) {
            self.studio.conversation_delete = Some(id);
            cx.notify();
            return;
        }
        let was_active = self.studio.active == id;
        if self.studio.remove_conversation(id) {
            if was_active {
                self.studio.conversation_transition = TransitionState::entering();
            }
            self.studio.save(&self.data_dir);
        } else {
            self.studio.conversation_delete = None;
            self.studio.notice = "这个会话还有任务在进行，请先停止再删除".into();
        }
        cx.notify();
    }

    fn render_conversation_list(&self, width: f32, cx: &Context<Self>) -> Div {
        let rows = self
            .studio
            .ordered_conversations()
            .into_iter()
            .map(|conversation| {
                let id = conversation.id;
                let active = id == self.studio.active;
                let confirming = self.studio.conversation_delete == Some(id);
                let count = self.studio.record_count(id);
                let running = self.studio.is_running(id);
                let detail = if running {
                    "生成中…".to_owned()
                } else if count == 0 {
                    "还没有记录".to_owned()
                } else {
                    format!("{} · {count} 条", record_date(self.studio.activity(id)))
                };
                div()
                    .id(SharedString::from(format!("conversation-{id}")))
                    .group("conversation-row")
                    .relative()
                    .mx_2()
                    .px_3()
                    .py(px(9.))
                    .flex_none()
                    .rounded_lg()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .when(!active, |element| {
                        element.hover(|s| s.bg(rgb(colors().fill)))
                    })
                    .when(active, |element| {
                        element.child(crate::motion::animate(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
                                .bg(rgb(colors().panel))
                                .shadow_sm(),
                            self.studio.conversation_transition,
                            self.motion_policy,
                            MotionKind::Modal,
                            2.,
                        ))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(px(13.))
                                    .font_weight(if active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(rgb(colors().text))
                                    .child(self.studio.title(id)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .when(running, |element| {
                                        element.child(
                                            div()
                                                .size(px(6.))
                                                .rounded_full()
                                                .bg(rgb(colors().blue)),
                                        )
                                    })
                                    .child(
                                        div()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_size(px(11.))
                                            .text_color(rgb(colors().secondary))
                                            .child(detail),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("conversation-delete-{id}")))
                            .flex_none()
                            .h(px(24.))
                            .px(px(if confirming { 8. } else { 4. }))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.))
                            .text_color(rgb(colors().on_primary))
                            .when(confirming, |element| {
                                element.bg(rgb(crate::theme::colors().danger))
                            })
                            .when(!confirming, |element| {
                                element
                                    .opacity(0.)
                                    .group_hover("conversation-row", |s| s.opacity(1.))
                                    .hover(|s| s.bg(rgb(tone(0xE9EAEC, 0x35353D))))
                            })
                            .child(if confirming {
                                div().child("删除")
                            } else {
                                div().child(glyph("trash", 13., colors().secondary))
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.delete_studio_conversation(id, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_studio_conversation(id, window, cx)
                    }))
            })
            .collect::<Vec<_>>();
        div()
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(tone(0xFAFAFB, 0x1D1D21)))
            .border_r_1()
            .border_color(rgb(tone(0xECECEF, 0x36363E)))
            .child(
                div()
                    .h(px(54.))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors().text))
                            .child("会话"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(colors().secondary))
                            .child(self.studio.conversations.len().to_string()),
                    ),
            )
            .child(
                div().px_3().pb_2().child(
                    div()
                        .id("conversation-new")
                        .h(px(36.))
                        .px_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
                        .bg(rgb(colors().panel))
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors().fill)))
                        .child(glyph("plus", 15., colors().text))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(13.))
                                .text_color(rgb(colors().text))
                                .child("新建会话"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.new_studio_conversation(window, cx)
                        })),
                ),
            )
            .child(
                div()
                    .id("conversation-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(rows),
            )
    }

    pub(super) fn render_studio_home(
        &self,
        available: f32,
        window: &Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let height = (f32::from(window.viewport_size().height)
            - crate::theme::TITLEBAR
            - crate::theme::STATUS)
            .max(1.);
        let list_width = if self.studio.conversations_open && available >= 760. {
            232.
        } else {
            0.
        };
        let main = (available - list_width).max(1.);
        let column_width = (main - if main < 760. { 48. } else { 96. }).clamp(240., 780.);
        let inset = ((main - column_width) / 2.).max(12.);
        let active = self.studio.active;
        let entries = self
            .studio
            .entries
            .iter()
            .filter(|entry| entry.mode != Mode::Chat && entry.conversation == active)
            .collect::<Vec<_>>();
        let title = self.studio.title(active);
        let running = self
            .workspace
            .accounts
            .iter()
            .filter(|account| self.browsers.account_running(account.id))
            .count();
        let header_button = |id: &'static str, icon: &'static str, hint: &'static str| {
            super::chrome::keyboard_control(div().id(id))
                .size(px(28.))
                .rounded_md()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().fill)))
                .child(glyph(icon, 15., colors().secondary))
                .tooltip(move |_, cx| cx.new(|_| StudioHint(hint.into())).into())
        };
        let header = div()
            .h(px(54.))
            .bg(rgb(colors().surface))
            .border_b_1()
            .border_color(rgb(colors().line))
            .flex_none()
            .px(px(inset.min(28.)))
            .flex()
            .items_center()
            .gap_2()
            .when(available >= 760., |element| {
                element.child(
                    header_button(
                        "studio-conversations-toggle",
                        "layout-sidebar",
                        if self.studio.conversations_open {
                            "收起会话列表"
                        } else {
                            "展开会话列表"
                        },
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.studio.conversations_open = !this.studio.conversations_open;
                        cx.notify();
                    })),
                )
            })
            .when(list_width == 0., |element| {
                element.child(
                    header_button("studio-new-conversation", "plus", "新建会话").on_click(
                        cx.listener(|this, _, window, cx| this.new_studio_conversation(window, cx)),
                    ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors().text))
                    .child(title),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(28.))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded_lg()
                    .bg(rgb(colors().fill))
                    .text_size(px(12.))
                    .text_color(rgb(colors().secondary))
                    .child(format!(
                        "已启动 {running} / {} 个账号",
                        self.workspace.accounts.len()
                    )),
            )
            .child(
                header_button("studio-bottom", "chevron-down", "回到最新记录").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.studio.scroll.scroll_to_bottom();
                        cx.notify();
                    }),
                ),
            );

        let mut content = div()
            .w(px(column_width))
            .flex_none()
            .pb_6()
            .flex()
            .flex_col();
        if entries.is_empty() {
            const STARTERS: [(&str, &str); 3] = [
                ("风格和细节", "胶片颗粒，低饱和，柔和侧光，画面留白"),
                ("动作和氛围", "人物回头的一瞬，风吹起衣角，黄昏的逆光"),
                ("描述镜头", "低角度广角，镜头缓慢推进，跟随主体"),
            ];
            content = content
                .pt(px(72.))
                .px(px(6.))
                .child(
                    div()
                        .text_size(px(24.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors().text))
                        .child("创作工作室"),
                )
                .child(
                    div()
                        .mt(px(12.))
                        .text_size(px(14.))
                        .text_color(rgb(colors().secondary))
                        .child("提示词示例"),
                )
                .child(div().h(px(24.)))
                .children(STARTERS.iter().enumerate().map(|(index, &(kind, text))| {
                    div()
                        .id(SharedString::from(format!("studio-starter-{index}")))
                        .px(px(6.))
                        .py(px(13.))
                        .border_t_1()
                        .border_color(rgb(colors().line))
                        .flex()
                        .items_start()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(colors().fill)))
                        .child(
                            div()
                                .w(px(84.))
                                .flex_none()
                                .text_size(px(12.))
                                .line_height(px(20.))
                                .text_color(rgb(colors().secondary))
                                .child(kind),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.5))
                                .line_height(px(20.))
                                .text_color(rgb(colors().text))
                                .child(text),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.studio
                                .input
                                .update(cx, |input, cx| input.set_text(text, cx));
                            window.focus(&this.studio.input.read(cx).focus_handle(cx));
                            cx.notify();
                        }))
                }));
        } else {
            // Oldest at the top and the newest at the bottom, like the phone app.
            content = content.pt_2();
            for entry in entries {
                content = content.child(self.render_studio_entry(entry, column_width, cx));
            }
        }
        let transcript = div()
            .id("studio-transcript")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.studio.scroll)
            .flex()
            .flex_col()
            .items_center()
            .child(crate::motion::animate(
                content,
                self.studio.conversation_transition,
                self.motion_policy,
                MotionKind::Modal,
                6.,
            ))
            .child(self.studio_layout_probe("transcript"));
        let main_column = div()
            .relative()
            .w(px(main))
            .h_full()
            .flex_none()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(header)
            .child(transcript)
            .child(
                div()
                    .w_full()
                    .flex_none()
                    .px(px(inset))
                    .pt_3()
                    .pb_5()
                    .child(self.render_studio_composer(column_width, window, cx)),
            );
        let mut home = div()
            .id("studio-home")
            .relative()
            .w(px(available))
            .h(px(height))
            .flex_none()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .bg(rgb(colors().canvas))
            .text_size(px(14.))
            .text_color(rgb(colors().text))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_studio_panels(window, cx);
                    if this.studio.conversation_delete.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .when(list_width > 0., |element| {
                element.child(self.render_conversation_list(list_width, cx))
            })
            .child(main_column)
            .child(self.studio_layout_probe("home"));
        if let Some(menu) = self.render_studio_entry_menu(cx) {
            home = home.child(menu);
        }
        home
    }

    // Opt-in geometry report; records bounds only, never account or conversation contents.
    pub(super) fn studio_layout_probe(&self, name: &'static str) -> impl IntoElement {
        let enabled = std::env::args().any(|argument| argument == "--studio-layout-check");
        let path = self.data_dir.join(format!("layout-{name}.json"));
        canvas(move |bounds, window, _| {
            if enabled {
                let viewport = window.viewport_size();
                let data = serde_json::json!({"x":f32::from(bounds.origin.x),"y":f32::from(bounds.origin.y),
                    "width":f32::from(bounds.size.width),"height":f32::from(bounds.size.height),
                    "viewport_width":f32::from(viewport.width),"viewport_height":f32::from(viewport.height),"scale":window.scale_factor()});
                let _ = std::fs::write(path, data.to_string());
            }
        }, |_, _, _, _| {}).absolute().inset_0()
    }
}

pub(super) struct StudioHint(pub String);
impl Render for StudioHint {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(0x27272A))
            .text_color(rgb(0xFFFFFF))
            .text_size(px(12.))
            .child(self.0.clone())
    }
}

pub(super) fn mode_icon(mode: Mode) -> &'static str {
    match mode {
        Mode::Chat => "message",
        Mode::Image => "image",
        Mode::Video => "video",
    }
}
pub(super) fn mode_title(mode: Mode) -> &'static str {
    match mode {
        Mode::Chat => "聊天",
        Mode::Image => "图片生成",
        Mode::Video => "视频生成",
    }
}
pub(super) fn ratio_label(ratio: &str) -> &str {
    if ratio == "Auto" { "自动" } else { ratio }
}

pub(super) fn entry_ratio(entry: &studio::Entry) -> String {
    if entry.ratio != "Auto" {
        return entry.ratio.clone();
    }
    if let Some((width, height)) = entry.media_width.zip(entry.media_height) {
        let actual = width as f32 / height.max(1) as f32;
        for (ratio, value) in [
            ("1:1", 1.),
            ("16:9", 16. / 9.),
            ("9:16", 9. / 16.),
            ("4:3", 4. / 3.),
            ("3:4", 3. / 4.),
        ] {
            if (actual - value).abs() < 0.025 {
                return ratio.into();
            }
        }
    }
    studio::ratio_options(entry.mode)
        .iter()
        .filter(|ratio| **ratio != "Auto")
        .find(|ratio| entry.prompt.contains(**ratio) || entry.text.contains(**ratio))
        .map_or_else(|| "Auto".into(), |ratio| (*ratio).to_owned())
}

fn record_date(at: u64) -> String {
    (at > 0)
        .then(|| crate::platform::local_time(at))
        .flatten()
        .map(|time| format!("{}月{}日", time.month, time.day))
        .unwrap_or_else(|| "历史记录".into())
}

pub(super) fn action_button(
    id: SharedString,
    icon: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.))
        .px_2()
        .flex_none()
        .rounded_lg()
        .bg(rgba(0x00000000))
        .flex()
        .items_center()
        .gap_1()
        .text_size(px(12.))
        .text_color(rgb(colors().text))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(colors().fill)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(glyph(icon, 14., colors().secondary))
        .when(!label.is_empty(), |element| element.child(label))
        .on_click(on_click)
}
