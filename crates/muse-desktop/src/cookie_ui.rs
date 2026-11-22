use super::chrome::{glyph, text_button};
use super::*;
use crate::{
    cookie_export::{self, CookieBundle, CookieTarget},
    theme::*,
};

impl MuseWindow {
    pub(super) fn open_cookie_dialog(
        &mut self,
        owner: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.account_dialog.is_some()
            || self
                .cookie_dialog
                .as_ref()
                .is_some_and(|dialog| !dialog.transition.is_exiting())
        {
            return;
        }
        let selected = self.workspace.active_account_id();
        let targets = self
            .workspace
            .accounts
            .iter()
            .map(|account| CookieTarget {
                id: account.id,
                name: account.name.clone(),
                identity: None,
                request: uuid::Uuid::new_v4(),
                temporary: false,
                prior_active_account: selected,
                prior_active_tab: Some(account.active_tab_id()),
                requested: false,
                finished: false,
                started: Instant::now(),
                status: "待提取".into(),
                bundle: None,
                selected: account.id == owner,
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return;
        }
        self.cookie_dialog = Some(cookie_export::CookieDialog {
            targets,
            extracting: false,
            message: "勾选账号后点击「提取并复制」，再粘贴到 muse2api 的账号导入框。".into(),
            transition: crate::motion::TransitionState::entering(),
        });
        self.ensure_page_freeze();
        self.browsers
            .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
        window.focus(&self.focus);
        cx.notify();
    }

    fn start_cookie_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.cookie_dialog.as_mut() else {
            return;
        };
        if dialog.transition.is_exiting()
            || dialog.extracting
            || !dialog.targets.iter().any(|target| target.selected)
        {
            return;
        }
        dialog.extracting = true;
        dialog.message = "正在读取所选账号…".into();
        let mut indices = Vec::new();
        for (index, target) in dialog.targets.iter_mut().enumerate() {
            if target.selected {
                target.request = uuid::Uuid::new_v4();
                target.requested = false;
                target.finished = false;
                target.bundle = None;
                target.started = Instant::now();
                target.status = "正在准备账号…".into();
                indices.push(index);
            }
        }
        for index in indices {
            self.prepare_cookie_target(index, window, cx);
        }
        cx.notify();
    }

    fn prepare_cookie_target(&mut self, index: usize, window: &Window, cx: &mut Context<Self>) {
        let Some((id, prior_account, prior_tab)) = self
            .cookie_dialog
            .as_ref()
            .and_then(|dialog| dialog.targets.get(index))
            .map(|target| {
                (
                    target.id,
                    target.prior_active_account,
                    target.prior_active_tab,
                )
            })
        else {
            return;
        };
        let (identity, temporary) = if let Some(identity) = self.browsers.cookie_identity(id) {
            (identity, false)
        } else {
            let Ok(tab_id) = self.workspace.add_tab(id, muse_core::HOME_URL) else {
                self.cookie_target_error(index, "无法创建读取页面", cx);
                return;
            };
            let Some(tab) = self
                .workspace
                .account(id)
                .and_then(|account| account.tabs.iter().find(|tab| tab.id == tab_id))
            else {
                return;
            };
            (
                muse_core::BrowserIdentity {
                    account_id: id,
                    tab_id,
                    generation: tab.generation,
                },
                true,
            )
        };
        if temporary {
            if let Some(tab) = prior_tab {
                let _ = self.workspace.select_tab(tab);
            }
            if let Some(account) = prior_account {
                let _ = self.workspace.select_account(account);
            }
        }
        if let Some(target) = self
            .cookie_dialog
            .as_mut()
            .and_then(|dialog| dialog.targets.get_mut(index))
        {
            target.identity = Some(identity);
            target.temporary = temporary;
            target.started = Instant::now();
        }
        let result = if temporary {
            self.prepare_cookie_identity(identity, window)
        } else {
            self.ensure_identity(identity, window)
        };
        if let Err(error) = result {
            self.release_cookie_identity(identity, temporary);
            self.cookie_target_error(index, &error, cx);
        }
    }

    fn cookie_target_error(&mut self, index: usize, error: &str, cx: &mut Context<Self>) {
        if let Some(target) = self
            .cookie_dialog
            .as_mut()
            .and_then(|dialog| dialog.targets.get_mut(index))
        {
            target.status = error.chars().take(140).collect();
            target.finished = true;
            target.identity = None;
            target.temporary = false;
        }
        cx.notify();
    }

    pub(super) fn tick_cookies(&mut self, _window: &Window, cx: &mut Context<Self>) -> bool {
        let Some(dialog) = self
            .cookie_dialog
            .as_ref()
            .filter(|dialog| dialog.extracting)
        else {
            return false;
        };
        let tasks = dialog
            .targets
            .iter()
            .enumerate()
            .filter(|(_, target)| target.selected && !target.finished)
            .map(|(index, target)| {
                (
                    index,
                    target.identity,
                    target.request,
                    target.requested,
                    target.temporary,
                    target.started.elapsed(),
                )
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for (index, identity, request, requested, temporary, elapsed) in tasks {
            let Some(identity) = identity else {
                continue;
            };
            let error = if elapsed > Duration::from_secs(40) {
                Some("提取超时，请先在该账号登录后重试".into())
            } else {
                self.browsers.startup_error(identity).map(str::to_owned)
            };
            if let Some(error) = error {
                self.release_cookie_identity(identity, temporary);
                self.cookie_target_error(index, &error, cx);
                changed = true;
                continue;
            }
            if requested {
                continue;
            }
            if !self.browsers.is_cookie_reader(identity) && !self.browsers.page_ready(identity) {
                continue;
            }
            if self.browsers.extract_cookies(identity, request) {
                if let Some(target) = self
                    .cookie_dialog
                    .as_mut()
                    .and_then(|dialog| dialog.targets.get_mut(index))
                {
                    target.requested = true;
                    target.status = "正在读取登录 Cookie…".into();
                }
            } else {
                self.release_cookie_identity(identity, temporary);
                self.cookie_target_error(index, "无法读取此账号 Cookie", cx);
            }
            changed = true;
        }
        let complete = self.cookie_dialog.as_ref().is_some_and(|dialog| {
            dialog.extracting
                && dialog
                    .targets
                    .iter()
                    .filter(|target| target.selected)
                    .all(|target| target.finished)
        });
        if complete {
            let dialog = self.cookie_dialog.as_ref().unwrap();
            let selected = dialog
                .targets
                .iter()
                .filter(|target| target.selected)
                .collect::<Vec<_>>();
            let output = if selected.iter().any(|target| target.bundle.is_none()) {
                Err("部分账号提取失败，没有复制；请检查登录状态或取消勾选后重试")
            } else {
                let accounts = selected
                    .iter()
                    .map(|target| (target.name.as_str(), target.bundle.as_ref().unwrap()))
                    .collect::<Vec<_>>();
                cookie_export::muse2api_text(&accounts)
            };
            let message = match output {
                Ok(text) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                    format!(
                        "已复制 {} 个账号，可直接粘贴到 muse2api 的账号导入框。",
                        selected.len()
                    )
                }
                Err(error) => error.to_owned(),
            };
            if let Some(dialog) = self.cookie_dialog.as_mut() {
                dialog.extracting = false;
                dialog.message = message;
            }
            changed = true;
        }
        if changed {
            cx.notify();
        }
        changed
    }

    fn release_cookie_identity(&mut self, identity: muse_core::BrowserIdentity, temporary: bool) {
        if temporary {
            self.browsers.close_cookie_tab(identity);
            let _ = self.workspace.close_tab(identity.tab_id);
            self.persist();
        }
    }

    pub(super) fn accept_cookie_result(
        &mut self,
        id: AccountId,
        identity: muse_core::BrowserIdentity,
        request: uuid::Uuid,
        result: Result<Box<CookieBundle>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if id != identity.account_id || !self.browsers.accepts_cookie_result(identity) {
            return;
        }
        let Some(index) = self
            .cookie_dialog
            .as_ref()
            .filter(|dialog| dialog.extracting)
            .and_then(|dialog| {
                dialog.targets.iter().position(|target| {
                    target.selected
                        && target.id == id
                        && target.request == request
                        && target.identity == Some(identity)
                        && !target.finished
                })
            })
        else {
            return;
        };
        let temporary = self.cookie_dialog.as_ref().unwrap().targets[index].temporary;
        if let Some(target) = self
            .cookie_dialog
            .as_mut()
            .and_then(|dialog| dialog.targets.get_mut(index))
        {
            match result {
                Ok(bundle) => {
                    let session = bundle
                        .items
                        .iter()
                        .any(|item| item.name == "hatch_sess" && !item.value.is_empty());
                    target.status = if session {
                        format!("已读取 · {} 条 Cookie", bundle.items.len())
                    } else {
                        "未找到登录会话，请先登录".into()
                    };
                    target.bundle = Some(*bundle);
                }
                Err(error) => target.status = error.chars().take(140).collect(),
            }
            target.finished = true;
            target.identity = None;
            target.temporary = false;
        }
        self.release_cookie_identity(identity, temporary);
        self.browsers
            .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
        cx.notify();
    }

    pub(super) fn clear_cookie_dialog(&mut self) {
        let Some(mut dialog) = self.cookie_dialog.take() else {
            return;
        };
        if let Some((_, resources)) = dialog.begin_exit() {
            for (identity, temporary) in resources {
                self.release_cookie_identity(identity, temporary);
            }
        }
    }
    pub(super) fn close_cookie_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.cookie_dialog.as_mut() else {
            return;
        };
        let Some((generation, resources)) = dialog.begin_exit() else {
            return;
        };
        for (identity, temporary) in resources {
            self.release_cookie_identity(identity, temporary);
        }
        self.schedule_overlay_removal(
            MotionKind::Modal,
            generation,
            window,
            cx,
            Self::finish_cookie_dialog_exit,
        );
    }

    fn finish_cookie_dialog_exit(
        &mut self,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .cookie_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.transition.finish_exit(generation))
        {
            return;
        }
        self.cookie_dialog = None;
        window.focus(&self.focus);
        self.ensure_active(window);
        cx.notify();
    }

    pub(super) fn render_cookie_dialog(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let dialog = self.cookie_dialog.as_ref().unwrap();
        let transition = dialog.transition;
        let exiting = transition.is_exiting();
        let selected = dialog
            .targets
            .iter()
            .filter(|target| target.selected)
            .count();
        let all = selected == dialog.targets.len();
        let enabled = !dialog.extracting && selected > 0;
        let rows = dialog
            .targets
            .iter()
            .map(|target| {
                let id = target.id;
                let checked = target.selected;
                div()
                    .id(SharedString::from(format!("cookie-target-{id}")))
                    .flex_none()
                    .p_3()
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(if checked { 0xC9D8F8 } else { 0xE8E9EB }))
                    .bg(rgb(if checked { 0xF8F9F9 } else { 0xFFFFFF }))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(20.))
                            .flex_none()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(if checked { BLUE } else { 0xCFD0D3 }))
                            .bg(rgb(if checked { BLUE } else { 0xFFFFFF }))
                            .text_color(rgb(0xFFFFFF))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(if checked { "✓" } else { "" }),
                    )
                    .when_some(self.workspace.account(id), |element, account| {
                        element.child(self.account_avatar(account, 32., false))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(target.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(SECONDARY))
                                    .child(target.status.clone()),
                            ),
                    )
                    .when(!dialog.extracting && !exiting, |element| {
                        element
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(dialog) = this.cookie_dialog.as_mut().filter(|dialog| {
                                    dialog.transition == transition && !dialog.extracting
                                }) && let Some(target) =
                                    dialog.targets.iter_mut().find(|target| target.id == id)
                                {
                                    target.selected = !target.selected;
                                }
                                cx.notify();
                            }))
                    })
            })
            .collect::<Vec<_>>();
        let max_height = (f32::from(window.viewport_size().height) - 80.).max(340.);
        let modal = div()
            .id("cookie-modal")
            .w(px(560.))
            .max_h(px(max_height))
            .rounded(px(18.))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(0xE4E5E7))
            .bg(rgb(0xF9F9FA))
            .shadow_lg()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_5()
                    .py_4()
                    .flex_none()
                    .bg(rgb(0xFFFFFF))
                    .rounded_tl(px(18.))
                    .rounded_tr(px(18.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(glyph("cookie", 21., BLUE))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(17.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("提取 Cookie"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(SECONDARY))
                                    .child("选择账号，复制为 muse2api 可直接导入的格式"),
                            ),
                    )
                    .child(
                        div()
                            .id("cookie-close")
                            .size(px(28.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0xF2F3F5)))
                            .child(glyph("x", 15., SECONDARY))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_cookie_dialog(window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .px_5()
                    .pt_3()
                    .pb_2()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(SECONDARY))
                            .child(format!(
                                "已选择 {selected} / {} 个账号",
                                dialog.targets.len()
                            )),
                    )
                    .child(
                        div()
                            .id("cookie-select-all")
                            .text_size(px(11.))
                            .text_color(rgb(BLUE))
                            .child(if all { "取消全选" } else { "全选" })
                            .when(!dialog.extracting && !exiting, |element| {
                                element.cursor_pointer().on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if let Some(dialog) =
                                            this.cookie_dialog.as_mut().filter(|dialog| {
                                                dialog.transition == transition
                                                    && !dialog.extracting
                                            })
                                        {
                                            for target in &mut dialog.targets {
                                                target.selected = !all;
                                            }
                                        }
                                        cx.notify();
                                    },
                                ))
                            }),
                    ),
            )
            .child(
                div()
                    .id("cookie-target-list")
                    .px_5()
                    .pb_3()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(rows),
            )
            .child(
                div()
                    .px_5()
                    .pb_3()
                    .flex_none()
                    .text_size(px(11.))
                    .line_height(px(18.))
                    .text_color(rgb(SECONDARY))
                    .child(dialog.message.clone()),
            )
            .child(
                div()
                    .px_5()
                    .py_3()
                    .flex_none()
                    .rounded_bl(px(18.))
                    .rounded_br(px(18.))
                    .border_t_1()
                    .border_color(rgb(0xEAEBED))
                    .bg(rgb(0xFFFFFF))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(text_button(
                        "cookie-dismiss",
                        "关闭",
                        false,
                        cx.listener(|this, _, window, cx| this.close_cookie_dialog(window, cx)),
                    ))
                    .child(
                        div()
                            .id("cookie-extract-copy")
                            .h(px(34.))
                            .px_4()
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.))
                            .bg(rgb(if enabled { INK } else { FILL }))
                            .text_color(rgb(if enabled { 0xFFFFFF } else { 0x9E9FA6 }))
                            .child(if dialog.extracting {
                                "正在提取…".into()
                            } else {
                                format!("提取并复制（{selected}）")
                            })
                            .when(enabled && !exiting, |element| {
                                element
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(INK_HOVER)))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_cookie_copy(window, cx)
                                    }))
                            }),
                    ),
            );
        div()
            .id("cookie-modal-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(crate::motion::fade(
                div().absolute().inset_0().bg(rgba(0x18181B47)),
                transition,
                self.motion_policy,
            ))
            .child(crate::motion::animate(
                modal,
                transition,
                self.motion_policy,
                crate::motion::MotionKind::Modal,
                8.,
            ))
            .when(exiting, |element| {
                element.child(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                )
            })
    }
}
