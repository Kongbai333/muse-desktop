use super::chrome::glyph;
use super::studio_ui::{StudioHint, StudioPanel, entry_ratio};
use super::*;
use crate::studio::{self, Mode};

impl MuseWindow {
    pub(super) fn render_studio_entry(
        &self,
        entry: &studio::Entry,
        column: f32,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let id = entry.id;
        let account = entry.account;
        let ratio = entry_ratio(entry);
        let media_path = entry
            .media
            .as_ref()
            .and_then(|path| studio::safe_media_path(&self.data_dir, path))
            .filter(|path| path.is_file());
        let visual = entry.mode != Mode::Chat;
        let (frame_width, frame_height) = frame_size(entry, &ratio, column);

        // Media frame: picture, generation progress, or why nothing came back.
        let mut frame = div()
            .id(SharedString::from(format!("studio-media-{id}")))
            .relative()
            .w(px(frame_width))
            .h(px(frame_height))
            .flex_none()
            .rounded(px(18.))
            .overflow_hidden()
            .bg(rgb(if entry.mode == Mode::Video && media_path.is_some() {
                0x18181B
            } else {
                tone(0xF2F2F4, 0x2B2B31)
            }));
        if let Some(path) = media_path.clone() {
            frame = frame.cursor_pointer().on_click(
                cx.listener(move |this, _, window, cx| this.open_studio_media(id, window, cx)),
            );
            if entry.mode == Mode::Image {
                let fresh = self
                    .studio
                    .fresh_media
                    .get(&id)
                    .is_some_and(|at| at.elapsed() < Duration::from_millis(1600));
                let picture = img(path)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover);
                frame = if fresh && self.motion_policy.duration(MotionKind::Modal).is_some() {
                    // Develop: the picture fades up from dark and grey into full colour.
                    frame.child(picture.with_animation(
                        SharedString::from(format!("develop-{id}")),
                        Animation::new(Duration::from_millis(900)),
                        |picture, delta| {
                            let eased = 1. - (1. - delta).powi(3);
                            picture.opacity(0.08 + 0.92 * eased).grayscale(delta < 0.45)
                        },
                    ))
                } else {
                    frame.child(picture)
                };
            } else {
                frame = frame
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(56.))
                                    .rounded_full()
                                    .bg(rgba(0x0C0C0E8C))
                                    .border_1()
                                    .border_color(rgba(0xFFFFFF33))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(div().ml(px(3.)).child(glyph("play", 22., 0xFFFFFF))),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(10.))
                            .right(px(10.))
                            .px(px(7.))
                            .py(px(3.))
                            .rounded(px(6.))
                            .bg(rgba(0x0C0C0E99))
                            .text_size(px(11.))
                            .text_color(rgb(0xFFFFFF))
                            .child(format!("00:{:02}", entry.duration)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(12.))
                            .bottom(px(10.))
                            .text_size(px(11.))
                            .text_color(rgba(0xFFFFFFB3))
                            .child("点击播放"),
                    );
            }
        } else if entry.running {
            frame = frame.child(progress_overlay(id, &entry.status));
        } else {
            frame = frame.child(
                div()
                    .absolute()
                    .inset_0()
                    .px(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .text_color(rgb(colors().secondary))
                    .child(if entry.status.trim().is_empty() {
                        "未取得结果".to_owned()
                    } else {
                        entry.status.clone()
                    }),
            );
        }

        // Prompt: three lines, click to show all.
        let expanded = self.studio.expanded_prompts.contains(&id);
        let prompt = div()
            .id(SharedString::from(format!("studio-prompt-{id}")))
            .w_full()
            .text_size(px(14.))
            .line_height(px(22.))
            .text_color(rgb(colors().text))
            .cursor_pointer()
            .when(!expanded, |element| element.line_clamp(3))
            .tooltip(move |_, cx| {
                cx.new(|_| {
                    StudioHint(if expanded {
                        "点击收起".into()
                    } else {
                        "点击展开完整提示词".into()
                    })
                })
                .into()
            })
            .child(entry.prompt.clone())
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.studio.expanded_prompts.remove(&id) {
                    this.studio.expanded_prompts.insert(id);
                }
                cx.notify();
            }));

        let aspect = if ratio == "Auto" {
            "自动画幅".to_owned()
        } else {
            ratio.clone()
        };
        let kind = match entry.mode {
            Mode::Video if entry.reference.is_some() => {
                format!(
                    "图生视频 · {aspect} · {} 秒 · {}",
                    entry.duration, entry.name
                )
            }
            Mode::Video => format!("视频 · {aspect} · {} 秒 · {}", entry.duration, entry.name),
            Mode::Image if entry.reference.is_some() => {
                format!("图生图 · {aspect} · {}", entry.name)
            }
            Mode::Image => format!("图片 · {aspect} · {}", entry.name),
            Mode::Chat => format!("对话 · {}", entry.name),
        };
        let meta = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(12.))
                    .text_color(rgb(colors().secondary))
                    .child(kind),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(rgb(colors().secondary))
                    .child(record_time(entry.created_at)),
            );

        // Only: copy image, download, copy prompt, open task page, re-edit.
        let mut actions = div().w_full().flex().flex_wrap().items_center().gap(px(2.));
        if entry.running {
            actions = actions.child(card_button(
                format!("studio-stop-{id}").into(),
                Some("stop"),
                "停止生成",
                false,
                cx.listener(move |this, _, _, cx| this.fail_studio_job(account, "已停止", cx)),
            ));
        } else {
            if media_path.is_some() && entry.mode == Mode::Image {
                actions = actions.child(icon_action(
                    format!("studio-copy-image-{id}").into(),
                    "copy",
                    "复制图片",
                    cx.listener(move |this, _, window, cx| this.copy_studio_image(id, window, cx)),
                ));
            }
            if media_path.is_some() {
                actions = actions.child(icon_action(
                    format!("studio-save-{id}").into(),
                    "download",
                    if entry.mode == Mode::Video {
                        "下载视频"
                    } else {
                        "下载图片"
                    },
                    cx.listener(move |this, _, window, cx| this.save_studio_media(id, window, cx)),
                ));
            }
            if !visual && !entry.text.is_empty() {
                let text = entry.text.clone();
                actions = actions.child(icon_action(
                    format!("studio-copy-{id}").into(),
                    "copy",
                    "复制回复",
                    cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        this.studio.notice = "回复已复制".into();
                        cx.notify();
                    }),
                ));
            }
        }
        let prompt_text = entry.prompt.clone();
        actions = actions.child(icon_action(
            format!("studio-copy-prompt-{id}").into(),
            "clipboard-text",
            "复制提示词",
            cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(prompt_text.clone()));
                this.studio.notice = "提示词已复制".into();
                cx.notify();
            }),
        ));
        actions = actions.child(icon_action(
            format!("studio-open-task-{id}").into(),
            "external-link",
            "打开任务页面",
            cx.listener(move |this, _, window, cx| this.open_studio_task(account, id, window, cx)),
        ));
        if visual && !entry.running {
            actions = actions.child(icon_action(
                format!("studio-edit-{id}").into(),
                "pencil",
                "重新编辑",
                cx.listener(move |this, _, window, cx| {
                    this.prepare_studio_entry(id, false, window, cx)
                }),
            ));
        }

        div()
            .id(SharedString::from(format!("studio-entry-{id}")))
            .w_full()
            .flex_none()
            .mb(px(30.))
            .flex()
            .flex_col()
            .items_start()
            .when(visual, |element| element.child(frame))
            .when_some(
                entry
                    .reference
                    .as_ref()
                    .and_then(|relative| studio::safe_media_path(&self.data_dir, relative))
                    .filter(|path| path.is_file()),
                |element, path| {
                    element.child(
                        div()
                            .mt(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .size(px(36.))
                                    .flex_none()
                                    .rounded(px(8.))
                                    .overflow_hidden()
                                    .border_1()
                                    .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
                                    .child(img(path).size_full().object_fit(ObjectFit::Cover)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(colors().secondary))
                                    .child(if entry.mode == Mode::Video {
                                        "图生视频 · 参考图作为画面起点"
                                    } else {
                                        "图生图 · 以参考图为基础"
                                    }),
                            ),
                    )
                },
            )
            .child(
                div()
                    .w_full()
                    .mt(px(if visual { 12. } else { 0. }))
                    .child(prompt),
            )
            .when(!entry.text.is_empty(), |element| {
                element.child(
                    div()
                        .w_full()
                        .mt(px(6.))
                        .text_size(px(13.))
                        .line_height(px(21.))
                        .text_color(rgb(tone(0x52535A, 0xB6B6C2)))
                        .child(entry.text.clone()),
                )
            })
            .child(div().w_full().mt(px(8.)).child(meta))
            .child(div().w_full().mt(px(12.)).child(actions))
    }

    pub(super) fn render_studio_entry_menu(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let (StudioPanel::EntryMenu(id, position), transition) = self.studio_panel? else {
            return None;
        };
        let entry = self.studio.entries.iter().find(|entry| entry.id == id)?;
        let account = entry.account;
        let prompt = entry.prompt.clone();
        let mut menu = div()
            .id(SharedString::from(format!("studio-entry-menu-{id}")))
            .w(px(180.))
            .p_1()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
            .bg(rgb(colors().panel))
            .shadow_md()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(menu_item(
                format!("copy-prompt-{id}").into(),
                "copy",
                "复制提示词",
                cx.listener(move |this, _, window, cx| {
                    if !this.studio_panel_accepts(transition) {
                        return;
                    }
                    this.close_studio_panels(window, cx);
                    cx.write_to_clipboard(ClipboardItem::new_string(prompt.clone()));
                }),
            ))
            .child(menu_item(
                format!("open-task-{id}").into(),
                "external-link",
                "打开任务页面",
                cx.listener(move |this, _, window, cx| {
                    if !this.studio_panel_accepts(transition) {
                        return;
                    }
                    this.close_studio_panels(window, cx);
                    this.open_studio_task(account, id, window, cx);
                }),
            ));
        if entry
            .media
            .as_ref()
            .and_then(|path| studio::safe_media_path(&self.data_dir, path))
            .is_some_and(|path| path.is_file())
        {
            menu = menu.child(menu_item(
                format!("open-media-{id}").into(),
                "image",
                if entry.mode == Mode::Image {
                    if entry.media_original {
                        "查看原图"
                    } else {
                        "放大查看"
                    }
                } else {
                    "打开生成文件"
                },
                cx.listener(move |this, _, window, cx| {
                    if this.studio_panel_accepts(transition) {
                        this.open_studio_media(id, window, cx);
                    }
                }),
            ));
        }
        Some(
            div()
                .id("studio-entry-menu-backdrop")
                .absolute()
                .inset_0()
                .when(!transition.is_exiting(), |element| {
                    element
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                if this.studio_panel_accepts(transition) {
                                    this.close_studio_panels(window, cx);
                                }
                            }),
                        )
                        .on_mouse_down_out(cx.listener(move |this, _, window, cx| {
                            if this.studio_panel_accepts(transition) {
                                this.close_studio_panels(window, cx);
                            }
                        }))
                })
                .child(
                    deferred(
                        anchored()
                            .position(position)
                            .anchor(Corner::BottomLeft)
                            .offset(point(px(-8.), px(-8.)))
                            .child(crate::motion::animate(
                                menu,
                                transition,
                                self.motion_policy,
                                MotionKind::Popover,
                                4.,
                            )),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }

    fn prepare_studio_entry(
        &mut self,
        id: uuid::Uuid,
        send: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self
            .studio
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        else {
            return;
        };
        if self.workspace.account(entry.account).is_none() {
            self.studio.notice = "这个账号已移除，请选择其他账号".into();
            cx.notify();
            return;
        }
        if send
            && (!self.browsers.account_running(entry.account)
                || self.studio.jobs.contains_key(&entry.account))
        {
            self.studio.notice = "请先启动这个账号，并等待当前任务完成".into();
            cx.notify();
            return;
        }
        self.choose_studio_mode(entry.mode, window, cx);
        let ratio = entry_ratio(&entry);
        self.studio.ratio = studio::ratio_options(entry.mode)
            .iter()
            .position(|value| *value == ratio)
            .unwrap_or(0);
        self.studio.duration = studio::DURATIONS
            .iter()
            .position(|value| *value == entry.duration)
            .unwrap_or(0);
        self.studio.selected.clear();
        self.studio.selected.insert(entry.account);
        self.studio.reference = entry
            .reference
            .as_deref()
            .and_then(|relative| crate::studio_reference::load(&self.data_dir, relative));
        self.studio
            .input
            .update(cx, |input, cx| input.set_text(&entry.prompt, cx));
        window.focus(&self.studio.input.read(cx).focus_handle(cx));
        if send {
            self.submit_studio(window, cx);
        } else {
            cx.notify();
        }
    }
}

/// Phone layout: portrait media is narrower, landscape fills the column.
/// Height is capped so a single result never fills the whole window.
fn frame_size(entry: &studio::Entry, ratio: &str, column: f32) -> (f32, f32) {
    let aspect = entry
        .media_width
        .zip(entry.media_height)
        .filter(|(width, height)| *width > 0 && *height > 0)
        .map(|(width, height)| width as f32 / height as f32)
        .unwrap_or_else(|| {
            let mut parts = ratio.split(':').map(|part| part.trim().parse::<f32>().ok());
            match (parts.next().flatten(), parts.next().flatten()) {
                (Some(w), Some(h)) if w > 0. && h > 0. => w / h,
                _ => 4. / 5.,
            }
        });
    let fraction = if aspect < 0.6 {
        0.62
    } else if aspect < 0.8 {
        0.76
    } else if aspect < 1.01 {
        0.86
    } else {
        1.
    };
    let width = (column * fraction).min(560. * aspect).max(160.);
    (width, width / aspect)
}

/// Step text over a moving bar, at the bottom of the frame while generating.
fn progress_overlay(id: uuid::Uuid, status: &str) -> Div {
    div()
        .absolute()
        .left(px(12.))
        .right(px(12.))
        .bottom(px(12.))
        .px(px(12.))
        .pt(px(10.))
        .pb(px(11.))
        .rounded(px(12.))
        .bg(rgba(0x0C0C0EA8))
        .flex()
        .flex_col()
        .gap(px(9.))
        .child(
            div()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(12.5))
                .text_color(rgb(0xF1EFEA))
                .child(if status.trim().is_empty() {
                    "正在生成…".to_owned()
                } else {
                    status.to_owned()
                }),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(3.))
                .rounded(px(2.))
                .overflow_hidden()
                .bg(rgba(0xFFFFFF29))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .w(relative(0.36))
                        .rounded(px(2.))
                        .bg(rgb(0x2F6FEB))
                        .with_animation(
                            SharedString::from(format!("studio-progress-{id}")),
                            Animation::new(Duration::from_millis(1300)).repeat(),
                            |bar, delta| {
                                let eased = delta * delta * (3. - 2. * delta);
                                bar.left(relative(-0.36 + 1.36 * eased))
                            },
                        ),
                ),
        )
}

/// Compact bordered button under a result (34 px, like the phone app).
fn card_button(
    id: SharedString,
    icon: Option<&'static str>,
    label: &'static str,
    ghost: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let color = if ghost {
        colors().secondary
    } else {
        colors().text
    };
    div()
        .id(id)
        .h(px(34.))
        .px(px(if label.is_empty() {
            9.
        } else if icon.is_some() {
            10.
        } else {
            12.
        }))
        .flex_none()
        .rounded(px(10.))
        .border_1()
        .border_color(rgba(if ghost {
            0x00000000
        } else {
            (tone(0xD4D4D8, 0x60606D) << 8) | 0xFF
        }))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(13.))
        .text_color(rgb(color))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(colors().fill)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .when_some(icon, |element, icon| element.child(glyph(icon, 15., color)))
        .when(!label.is_empty(), |element| element.child(label))
        .on_click(on_click)
}

/// Square icon button under a result; the name shows on hover.
fn icon_action(
    id: SharedString,
    icon: &'static str,
    hint: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(32.))
        .flex_none()
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(colors().fill)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(glyph(icon, 17., colors().secondary))
        .tooltip(move |_, cx| cx.new(|_| StudioHint(hint.into())).into())
        .on_click(on_click)
}

/// Opens Explorer with the generated file selected.
#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub(super) fn reveal_in_folder(path: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn();
}

/// "11/22 10:30" in local time.
fn record_time(at: u64) -> String {
    (at > 0)
        .then(|| crate::platform::local_time(at))
        .flatten()
        .map(|time| {
            format!(
                "{}/{} {:02}:{:02}",
                time.month, time.day, time.hour, time.minute
            )
        })
        .unwrap_or_default()
}
fn menu_item(
    id: SharedString,
    icon: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(px(32.))
        .flex_none()
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(colors().fill)))
        .child(glyph(icon, 14., colors().secondary))
        .child(label)
        .on_click(on_click)
}
