use super::chrome::glyph;
use super::studio_ui::{StudioPanel, mode_icon, ratio_label};
use super::*;
use crate::studio::{self, Mode};

impl MuseWindow {
    pub(super) fn render_studio_composer(
        &self,
        width: f32,
        window: &Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let mode = self.studio.mode;
        let compact = width < 560.;
        let open = self
            .studio_panel
            .filter(|(_, transition)| !transition.is_exiting())
            .map(|(panel, _)| panel);
        let selected = self
            .workspace
            .accounts
            .iter()
            .filter(|account| {
                self.studio.selected.contains(&account.id)
                    && self.browsers.account_running(account.id)
            })
            .collect::<Vec<_>>();
        let text_present = !self.studio.input.read(cx).text().trim().is_empty();
        // submit_studio owns target validation, including typed @mentions.
        let ready = text_present;
        let mut menu =
            self.render_studio_options(width, f32::from(window.viewport_size().height), cx);
        let mut take_menu = |panel: StudioPanel| -> Option<AnyElement> {
            if menu.as_ref().is_some_and(|(current, _)| *current == panel) {
                menu.take().map(|(_, element)| element)
            } else {
                None
            }
        };

        let mut toolbar = div()
            .h(px(34.))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .h(px(30.))
                    .p(px(2.))
                    .flex_none()
                    .rounded_lg()
                    .bg(rgb(colors().fill))
                    .flex()
                    .items_center()
                    .children([Mode::Image, Mode::Video].into_iter().map(|item| {
                        mode_segment(
                            item,
                            mode == item,
                            compact,
                            cx.listener(move |this, _, window, cx| {
                                this.choose_studio_mode(item, window, cx);
                                window.focus(&this.studio.input.read(cx).focus_handle(cx));
                                cx.notify();
                            }),
                        )
                    })),
            )
            .child(
                div()
                    .w(px(1.))
                    .h(px(16.))
                    .mx_1()
                    .flex_none()
                    .bg(rgb(tone(0xE4E4E7, 0x3F3F46))),
            )
            .child(
                composer_pill("studio-reference", false, compact)
                    .child(glyph("image", 14., colors().secondary))
                    .when(!compact, |element| {
                        element.child(if self.studio.reference.is_some() {
                            "更换参考图"
                        } else {
                            "参考图"
                        })
                    })
                    .tooltip(|_, cx| {
                        cx.new(|_| {
                            super::studio_ui::StudioHint(
                                "选择参考图，也可以把图片拖到输入框".into(),
                            )
                        })
                        .into()
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_studio_panels(window, cx);
                        this.pick_studio_reference(window, cx);
                    })),
            );

        if mode != Mode::Chat {
            let ratio = studio::ratio_value(mode, self.studio.ratio);
            let pill = composer_pill("studio-ratio", open == Some(StudioPanel::Ratio), compact)
                .child(glyph(ratio_icon(ratio), 14., colors().secondary))
                .child(ratio_label(ratio).to_owned())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_studio_panel(StudioPanel::Ratio, window, cx);
                }));
            toolbar = toolbar.child(anchor(pill, take_menu(StudioPanel::Ratio), false));
        }
        if mode == Mode::Video {
            let pill = composer_pill(
                "studio-duration",
                open == Some(StudioPanel::Duration),
                compact,
            )
            .child(glyph("duration", 14., colors().secondary))
            .child(format!("{} 秒", studio::DURATIONS[self.studio.duration]))
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_studio_panel(StudioPanel::Duration, window, cx);
            }));
            toolbar = toolbar.child(anchor(pill, take_menu(StudioPanel::Duration), false));
        }

        let account_pill = {
            let mut pill = composer_pill(
                "studio-accounts",
                open == Some(StudioPanel::Picker),
                compact,
            );
            pill = match selected.as_slice() {
                [] => pill
                    .child(glyph("at-sign", 14., colors().secondary))
                    .when(!compact, |element| element.child("选择账号")),
                [account] => {
                    pill.child(self.account_avatar(account, 18., false))
                        .when(!compact, |element| {
                            element.child(
                                div()
                                    .max_w(px(96.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(account.name.clone()),
                            )
                        })
                }
                many => pill
                    .child(div().flex().items_center().children(
                        many.iter().take(3).enumerate().map(|(index, account)| {
                            div()
                                .when(index > 0, |element| element.ml(px(-6.)))
                                .rounded_full()
                                .border_2()
                                .border_color(rgb(colors().panel))
                                .child(self.account_avatar(account, 18., false))
                        }),
                    ))
                    .when(!compact, |element| {
                        element.child(format!("{} 个账号", many.len()))
                    }),
            };
            pill.child(glyph("chevron-down", 11., colors().secondary))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_studio_panel(StudioPanel::Picker, window, cx);
                }))
        };
        toolbar = toolbar.child(anchor(
            account_pill,
            take_menu(StudioPanel::Picker),
            compact,
        ));

        toolbar = toolbar.child(div().flex_1().min_w_0()).child(
            div()
                .id("studio-send")
                .size(px(32.))
                .flex_none()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(if ready {
                    colors().primary
                } else {
                    colors().fill
                }))
                .child(glyph(
                    "arrow-up",
                    17.,
                    if ready {
                        colors().on_primary
                    } else {
                        tone(0x9C9DA4, 0x92929F)
                    },
                ))
                .when(ready, |element| {
                    element
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(tone(0x4C4D54, 0xFFFFFF))))
                        .on_click(cx.listener(|this, _, window, cx| this.submit_studio(window, cx)))
                }),
        );

        let mut composer = div()
            .id("studio-composer")
            .relative()
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if !self.studio.notice.is_empty() {
            composer = composer.child(
                div()
                    .w_full()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(rgb(tone(0xA15C07, 0xF3C06A)))
                    .child(self.studio.notice.clone()),
            );
        }
        let reference_row = self.render_reference_row(mode, cx);
        let reference_height = if reference_row.is_some() { 52. } else { 0. };
        composer = composer.child(
            div()
                .id("studio-composer-card")
                .w_full()
                .h(px(124. + reference_height))
                .flex_none()
                .rounded(px(16.))
                .border_1()
                .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
                .bg(rgb(colors().panel))
                .shadow_md()
                .pl_4()
                .pr_3()
                .when(compact, |element| element.pl_3())
                .pt_3()
                .pb(px(10.))
                .flex()
                .flex_col()
                .drag_over::<ExternalPaths>(|style, _, _, _| {
                    style
                        .border_color(rgb(colors().blue))
                        .bg(rgb(tone(0xF7F9FE, 0x24334F)))
                })
                .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                    if let Some(path) = paths
                        .paths()
                        .iter()
                        .find(|path| crate::studio_reference::is_image_path(path))
                    {
                        this.attach_studio_reference(path.clone(), cx);
                    } else {
                        this.studio.notice = "请拖入 JPG、PNG、WebP 或 GIF 图片".into();
                        cx.notify();
                    }
                }))
                .when_some(reference_row, |element, row| element.child(row))
                .child(
                    div()
                        .id("studio-input-area")
                        .h(px(66.))
                        .flex_none()
                        .w_full()
                        .min_w_0()
                        .child(self.studio.input.clone())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_studio_panels(window, cx);
                            window.focus(&this.studio.input.read(cx).focus_handle(cx));
                            cx.notify();
                        })),
                )
                .child(toolbar),
        );
        composer.child(
            div()
                .w_full()
                .pt_1()
                .flex()
                .justify_center()
                .text_size(px(11.))
                .text_color(rgb(tone(0xA1A1AA, 0x92929F)))
                .child("Enter 发送，Shift+Enter 换行。可以把图片拖到输入框作为参考图；同一账号的任务依次进行，不同账号可以同时生成。"),
        )
        .child(self.studio_layout_probe("composer"))
    }

    /// Opens the system file dialog for a reference image.
    pub(super) fn pick_studio_reference(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.studio.reference_loading {
            return;
        }
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择参考图".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            for _ in 0..60 {
                let path = path.clone();
                if this
                    .update(cx, |this, cx| this.attach_studio_reference(path, cx))
                    .is_ok()
                    || this.upgrade().is_none()
                {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
        })
        .detach();
    }

    /// Prepares the picture off the UI thread and attaches it to the next prompt.
    pub(super) fn attach_studio_reference(
        &mut self,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.studio.reference_loading {
            return;
        }
        if !crate::studio_reference::is_image_path(&path) {
            self.studio.notice = "请选择 JPG、PNG、WebP 或 GIF 图片".into();
            cx.notify();
            return;
        }
        self.studio.reference_loading = true;
        self.studio.notice.clear();
        cx.notify();
        let root = self.data_dir.clone();
        let task = cx
            .background_executor()
            .spawn(async move { crate::studio_reference::from_file(&root, &path) });
        cx.spawn(async move |this, cx| {
            let mut result = Some(task.await);
            for _ in 0..60 {
                let attempt = this.update(cx, |this, cx| {
                    this.studio.reference_loading = false;
                    match result.take() {
                        Some(Ok(reference)) => this.studio.reference = Some(reference),
                        Some(Err(error)) => this.studio.notice = error,
                        None => {}
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

    /// Thumbnail of the attached reference above the prompt, or a loading line.
    fn render_reference_row(&self, mode: Mode, cx: &Context<Self>) -> Option<Div> {
        if self.studio.reference_loading {
            return Some(
                div()
                    .h(px(44.))
                    .mb(px(8.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(44.))
                            .rounded(px(10.))
                            .bg(rgb(colors().fill))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(glyph("image", 18., colors().secondary)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(colors().secondary))
                            .child("正在处理参考图…"),
                    ),
            );
        }
        let reference = self.studio.reference.as_ref()?;
        let detail = format!(
            "{} · {} × {} · {:.1} MB",
            if mode == Mode::Video {
                "作为视频画面起点"
            } else {
                "以这张图为基础生成"
            },
            reference.width,
            reference.height,
            reference.bytes as f64 / 1_048_576.
        );
        Some(
            div()
                .h(px(44.))
                .mb(px(8.))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .relative()
                        .size(px(44.))
                        .flex_none()
                        .rounded(px(10.))
                        .overflow_hidden()
                        .border_1()
                        .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
                        .child(
                            img(reference.path.clone())
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(colors().text))
                                .child("参考图"),
                        )
                        .child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_size(px(11.))
                                .text_color(rgb(colors().secondary))
                                .child(detail),
                        ),
                )
                .child(
                    div()
                        .id("studio-reference-remove")
                        .size(px(26.))
                        .flex_none()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(colors().fill)))
                        .tooltip(|_, cx| {
                            cx.new(|_| super::studio_ui::StudioHint("移除参考图".into()))
                                .into()
                        })
                        .child(glyph("x", 13., colors().secondary))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.studio.reference = None;
                            cx.notify();
                        })),
                ),
        )
    }

    fn render_studio_options(
        &self,
        width: f32,
        height: f32,
        cx: &Context<Self>,
    ) -> Option<(StudioPanel, AnyElement)> {
        let (panel, transition) = self.studio_panel?;
        let max_height = (height - 285.).clamp(130., 360.);
        let base = |w: f32| {
            div()
                .id("studio-options")
                .w(px(w.min(width)))
                .p(px(4.))
                .rounded(px(10.))
                .border_1()
                .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
                .bg(rgb(colors().panel))
                .shadow_lg()
                .occlude()
                .max_h(px(max_height))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(1.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let element = match panel {
            StudioPanel::Picker => {
                let accounts = &self.workspace.accounts;
                base(260.)
                    .child(menu_heading("用哪个账号生成 · 可多选"))
                    .when(accounts.is_empty(), |element| {
                        element.child(
                            div()
                                .px(px(10.))
                                .py_2()
                                .text_size(px(12.))
                                .text_color(rgb(colors().secondary))
                                .child("还没有账号，先在左侧新增一个"),
                        )
                    })
                    .children(accounts.iter().map(|account| {
                        let id = account.id;
                        let running = self.browsers.account_running(id);
                        let busy = self.studio.jobs.contains_key(&id);
                        let chosen = running && self.studio.selected.contains(&id);
                        let used = account
                            .quota
                            .remaining_ratio()
                            .map(|ratio| (1. - ratio) * 100.);
                        let detail = if !running {
                            "未启动".to_owned()
                        } else if busy {
                            "生成中".to_owned()
                        } else if self.browsers.account_sleeping(id) {
                            "休眠 · 发送时唤醒".to_owned()
                        } else if let Some(used) = used {
                            format!("在线 · 已用 {used:.0}%")
                        } else {
                            "在线".to_owned()
                        };
                        div()
                            .id(SharedString::from(format!("studio-picker-{id}")))
                            .h(px(36.))
                            .flex_none()
                            .px(px(8.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .when(!running, |element| element.opacity(0.5))
                            .when(running, |element| {
                                element
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(colors().fill)))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if !this.studio_panel_accepts(transition) {
                                            return;
                                        }
                                        if !this.studio.selected.remove(&id) {
                                            this.studio.selected.insert(id);
                                        }
                                        let text = this.studio.input.read(cx).text();
                                        if text.ends_with('@') {
                                            this.studio.input.update(cx, |input, cx| {
                                                input.set_text(text.trim_end_matches('@'), cx)
                                            });
                                        }
                                        window.focus(&this.studio.input.read(cx).focus_handle(cx));
                                        cx.notify();
                                    }))
                            })
                            .child(self.account_avatar(account, 22., false))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(px(13.))
                                    .font_weight(if chosen {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(rgb(colors().text))
                                    .child(account.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors().secondary))
                                    .child(detail),
                            )
                            .child(check_slot(chosen))
                    }))
            }
            StudioPanel::Ratio => base(188.).child(menu_heading("画面比例")).children(
                studio::ratio_options(self.studio.mode)
                    .iter()
                    .enumerate()
                    .map(|(index, ratio)| {
                        let selected = self.studio.ratio == index;
                        menu_row(
                            SharedString::from(format!("ratio-option-{index}")),
                            selected,
                        )
                        .child(glyph(
                            ratio_icon(ratio),
                            14.,
                            if selected {
                                colors().text
                            } else {
                                colors().secondary
                            },
                        ))
                        .child(div().flex_1().child(ratio_name(ratio)))
                        .child(check_slot(selected))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if !this.studio_panel_accepts(transition) {
                                    return;
                                }
                                this.studio.ratio = index;
                                this.close_studio_panels(window, cx);
                                cx.notify();
                            },
                        ))
                    }),
            ),
            StudioPanel::Duration => base(150.).child(menu_heading("视频时长")).children(
                studio::DURATIONS
                    .iter()
                    .enumerate()
                    .map(|(index, duration)| {
                        let selected = self.studio.duration == index;
                        menu_row(
                            SharedString::from(format!("duration-option-{index}")),
                            selected,
                        )
                        .child(glyph(
                            "duration",
                            14.,
                            if selected {
                                colors().text
                            } else {
                                colors().secondary
                            },
                        ))
                        .child(div().flex_1().child(format!("{duration} 秒")))
                        .child(check_slot(selected))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if !this.studio_panel_accepts(transition) {
                                    return;
                                }
                                this.studio.duration = index;
                                this.close_studio_panels(window, cx);
                                cx.notify();
                            },
                        ))
                    }),
            ),
            _ => return None,
        };
        Some((
            panel,
            crate::motion::animate_above(element, transition, self.motion_policy, 4.),
        ))
    }
}

/// Toolbar pill in the composer. `open` marks the pill whose menu is showing.
fn composer_pill(id: &'static str, open: bool, compact: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px(px(if compact { 6. } else { 10. }))
        .flex_none()
        .rounded_lg()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(rgb(if open {
            colors().text
        } else {
            colors().secondary
        }))
        .bg(rgba(if open {
            (colors().fill << 8) | 0xFF
        } else {
            0x00000000
        }))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(colors().fill)).text_color(rgb(colors().text)))
}

/// Pill with its menu opening just above it.
fn anchor(pill: Stateful<Div>, menu: Option<AnyElement>, align_right: bool) -> Div {
    div()
        .relative()
        .flex_none()
        .child(pill)
        .when_some(menu, |element, menu| {
            element.child(
                div()
                    .absolute()
                    .bottom(px(38.))
                    .when(align_right, |element| element.right_0())
                    .when(!align_right, |element| element.left(px(-4.)))
                    .child(menu),
            )
        })
}

fn menu_heading(label: &'static str) -> Div {
    div()
        .px(px(10.))
        .pt(px(6.))
        .pb_1()
        .text_size(px(11.))
        .text_color(rgb(tone(0xA1A1AA, 0x92929F)))
        .child(label)
}

fn menu_row(id: SharedString, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(32.))
        .flex_none()
        .px(px(10.))
        .rounded_md()
        .flex()
        .items_center()
        .gap(px(10.))
        .text_size(px(13.))
        .text_color(rgb(colors().text))
        .font_weight(if selected {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .cursor_pointer()
        .hover(|style| style.bg(rgb(colors().fill)))
}

fn check_slot(checked: bool) -> Div {
    div().size(px(14.)).flex_none().when(checked, |element| {
        element.child(glyph("circle-check", 14., colors().blue))
    })
}

fn ratio_name(ratio: &str) -> String {
    match ratio {
        "1:1" => "方形 1:1".into(),
        "16:9" => "横屏 16:9".into(),
        "9:16" => "竖屏 9:16".into(),
        "4:3" => "横幅 4:3".into(),
        "3:4" => "竖幅 3:4".into(),
        _ => "自动".into(),
    }
}

fn ratio_icon(ratio: &str) -> &'static str {
    match ratio {
        "1:1" => "ratio-square",
        "16:9" => "ratio-wide",
        "9:16" => "ratio-phone",
        "4:3" => "ratio-classic",
        "3:4" => "ratio-portrait",
        _ => "ratio-auto",
    }
}

fn mode_segment(
    mode: Mode,
    selected: bool,
    compact: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("studio-segment-{}", mode.key())))
        .h(px(26.))
        .px_2()
        .flex_none()
        .rounded_md()
        .flex()
        .items_center()
        .gap_1()
        .text_size(px(12.))
        .text_color(rgb(if selected {
            colors().text
        } else {
            colors().secondary
        }))
        .bg(rgba(if selected {
            (colors().panel << 8) | 0xFF
        } else {
            0x00000000
        }))
        .when(selected, |element| element.shadow_sm())
        .cursor_pointer()
        .child(glyph(
            mode_icon(mode),
            13.,
            if selected {
                colors().text
            } else {
                colors().secondary
            },
        ))
        .when(!compact, |element| {
            element.child(match mode {
                Mode::Image => "图片",
                Mode::Video => "视频",
                Mode::Chat => "聊天",
            })
        })
        .on_click(on_click)
}
