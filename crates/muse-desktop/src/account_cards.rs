use super::chrome::{avatar, glyph, icon_button};
use super::group_ui::GroupOperation;
use super::*;
use crate::account_groups::UNGROUPED;
use gpui::MouseDownEvent;
use std::{cell::Cell, rc::Rc};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq)]
enum SidebarItem {
    Account(AccountId),
    Group(Option<Uuid>),
}

struct DragPreview {
    label: String,
    offset: Point<Pixels>,
    _cancel: Subscription,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.offset.x + px(12.))
            .pt(self.offset.y + px(12.))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(colors().panel))
                    .border_1()
                    .border_color(rgb(colors().line))
                    .text_size(px(12.))
                    .text_color(rgb(colors().secondary))
                    .max_w(px(160.))
                    .truncate()
                    .child(self.label.clone()),
            )
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct AccountDetail {
    account: AccountId,
    anchor_y: Pixels,
    keyboard: bool,
}

fn reset_label(at: Option<u64>) -> String {
    at.and_then(crate::platform::local_time)
        .map(|time| format!("{}月{}日重置", time.month, time.day))
        .unwrap_or_else(|| "重置时间待同步".into())
}
fn used_percent(quota: &muse_core::QuotaState) -> Option<f64> {
    match quota {
        muse_core::QuotaState::Stale(q) => {
            muse_core::QuotaState::Known(q.clone()).remaining_ratio()
        }
        q => q.remaining_ratio(),
    }
    .map(|r| (1. - r) * 100.)
}
fn extra_pill(state: &muse_core::ExtraQuotaState) -> String {
    match state {
        muse_core::ExtraQuotaState::Known(value) | muse_core::ExtraQuotaState::Stale(value) => {
            let text = value
                .label()
                .replace("已使用 ", "")
                .replace("（剩余 ", " · 余 ")
                .replace("(剩余 ", " · 余 ")
                .replace(['）', ')'], "")
                .replace("个口令", "");
            format!("额外 {text}")
        }
        _ => "额外待同步".into(),
    }
}

const SEGMENTS: usize = 20;
fn plan_usage(quota: &muse_core::QuotaState) -> (Option<f64>, bool, String) {
    let used = used_percent(quota);
    let exhausted = used == Some(100.);
    let label = match used {
        Some(value) if value > 0. && value.round() == 0. => "<1%".into(),
        Some(value) if !exhausted && value.round() == 100. => ">99%".into(),
        Some(value) => format!("{value:.0}%"),
        None => match quota {
            muse_core::QuotaState::Unlimited { .. }
            | muse_core::QuotaState::StaleUnlimited { .. } => "不限量",
            muse_core::QuotaState::LoggedOut => "未登录",
            muse_core::QuotaState::Stale(_) => "需刷新",
            muse_core::QuotaState::Unavailable(_) => "同步失败",
            _ => "待同步",
        }
        .into(),
    };
    (used, exhausted, label)
}

fn filled_segments(used: f64) -> usize {
    if used <= 0. {
        0
    } else if used >= 100. {
        SEGMENTS
    } else {
        ((used * SEGMENTS as f64 / 100.).round() as usize).clamp(1, SEGMENTS - 1)
    }
}

fn segment_color(index: usize) -> u32 {
    const ORANGE_INDEX: usize = (SEGMENTS - 1) * 7 / 10;
    let orange = tone(0xF39A46, 0xFFB76C);
    let (start, end, fraction) = if index <= ORANGE_INDEX {
        (colors().blue, orange, index as f32 / ORANGE_INDEX as f32)
    } else {
        (
            orange,
            colors().danger,
            (index - ORANGE_INDEX) as f32 / (SEGMENTS - 1 - ORANGE_INDEX) as f32,
        )
    };
    let channel = |shift: u32| {
        let from = ((start >> shift) & 255) as f32;
        let to = ((end >> shift) & 255) as f32;
        ((from + (to - from) * fraction).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

fn quota_meter(used: Option<f64>, height: f32, gap: f32) -> Div {
    div()
        .w_full()
        .h(px(height))
        .flex()
        .gap(px(gap))
        .when_some(used, |meter, used| {
            let filled = filled_segments(used);
            meter.children((0..SEGMENTS).map(|index| {
                let color = if index < filled {
                    segment_color(index)
                } else {
                    tone(0xD5D6DC, 0x62636E)
                };
                div().flex_1().h_full().rounded(px(1.5)).bg(rgb(color))
            }))
        })
}

#[inline(never)]
fn account_row_summary(account: &muse_core::Account) -> Div {
    let (used, exhausted, status) = plan_usage(&account.quota);
    let label = match used {
        Some(_) => format!("周额度已用 {status}"),
        None => format!("周额度{status}"),
    };
    let color = if exhausted {
        colors().danger
    } else if used.is_some() {
        colors().blue
    } else {
        colors().secondary
    };
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .line_clamp(1)
                        .text_ellipsis()
                        .font_weight(FontWeight::MEDIUM)
                        .child(account.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_size(px(11.))
                        .text_color(rgb(color))
                        .child(label),
                ),
        )
        .child(quota_meter(used, 7., 2.))
}

impl MuseWindow {
    #[inline(never)]
    fn sidebar_drag_target(
        &self,
        element: Stateful<Div>,
        target: SidebarItem,
        label: &str,
        center_y: Rc<Cell<Pixels>>,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        use SidebarItem::{Account, Group};
        let label = label.to_owned();
        let view = cx.entity().downgrade();
        let measure = center_y.clone();
        let indicator = center_y.clone();
        element
            .relative()
            .child(
                canvas(
                    move |area, _, _| measure.set(area.center().y),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .when(target != Group(None), |element| {
                element.on_drag(target, move |_, offset, window, cx| {
                    let _ = view.update(cx, |this, cx| this.clear_account_detail(cx));
                    let window_id = window.window_handle().window_id();
                    let cancel = cx.intercept_keystrokes(move |event, window, cx| {
                        if window.window_handle().window_id() == window_id
                            && event.keystroke.key == "escape"
                            && cx.stop_active_drag(window)
                        {
                            cx.stop_propagation();
                        }
                    });
                    cx.new(|_| DragPreview {
                        label: label.clone(),
                        offset,
                        _cancel: cancel,
                    })
                })
            })
            .drag_over::<SidebarItem>(move |style, source, window, _| match (*source, target) {
                (Account(_), Group(_)) => style
                    .bg(rgb(colors().fill_hover))
                    .border_1()
                    .border_color(rgb(colors().blue)),
                (Account(_), Account(_)) | (Group(Some(_)), Group(Some(_)))
                    if *source != target =>
                {
                    if window.mouse_position().y < indicator.get() {
                        style.border_t_2()
                    } else {
                        style.border_b_2()
                    }
                    .border_color(rgb(colors().blue))
                }
                _ => style,
            })
            .on_drop(cx.listener(move |this, source: &SidebarItem, window, cx| {
                let after = window.mouse_position().y >= center_y.get();
                let result = this.save_groups(|groups, accounts| {
                    Ok(match (*source, target) {
                        (Account(id), Account(neighbor)) if id != neighbor => groups.place_account(
                            accounts,
                            id,
                            groups.membership(neighbor),
                            Some((neighbor, after)),
                        ),
                        (Account(id), Group(group)) => {
                            groups.place_account(accounts, id, group, None)
                        }
                        (Group(Some(id)), Group(Some(neighbor))) => {
                            groups.place_group(id, neighbor, after)
                        }
                        _ => false,
                    })
                });
                if let Err(error) = result {
                    this.status = error;
                }
                cx.notify();
            }))
    }

    pub(super) fn prepare_account_rows(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.account_focus
            .retain(|id, _| self.workspace.account(*id).is_some());
        self.account_keyboard_focus = self.account_keyboard_focus.filter(|id| {
            self.account_focus
                .get(id)
                .is_some_and(|handle| handle.is_focused(window))
        });
        for account in &self.workspace.accounts {
            self.account_focus
                .entry(account.id)
                .or_insert_with(|| cx.focus_handle().tab_stop(true));
        }
    }

    #[inline(never)]
    fn render_account_row(
        &self,
        account: &muse_core::Account,
        compact: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let id = account.id;
        let active = !self.show_start_page && self.workspace.active_account_id() == Some(id);
        let anchor_y = Rc::new(Cell::new(px(0.)));
        let center_y = anchor_y.clone();
        let row = div()
            .id(id)
            .relative()
            .flex_none()
            .rounded_lg()
            .cursor_pointer()
            .mx_2()
            .mb_1()
            .track_focus(&self.account_focus[&id])
            .when(!active, |element| {
                element.hover(|style| style.bg(rgb(colors().fill_hover)))
            })
            .when(active, |element| {
                element
                    .bg(rgb(tone(0xDFE8F8, 0x313F56)))
                    .hover(|style| style.bg(rgb(tone(0xD8E3F5, 0x384862))))
            })
            .when(self.account_keyboard_focus == Some(id), |element| {
                element.child(
                    div()
                        .absolute()
                        .left(px(3.))
                        .top(px(if compact { 15. } else { 21. }))
                        .w(px(3.))
                        .h(px(16.))
                        .rounded_full()
                        .bg(rgb(colors().blue)),
                )
            })
            .when(compact, |element| {
                element
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.account_avatar(account, 30., active))
            })
            .when(!compact, |element| {
                element
                    .px_3()
                    .h(px(58.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.account_avatar(account, 38., active))
                    .child(account_row_summary(account))
            })
            .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                this.hover_account(id, *hovered, window, cx)
            }));
        self.sidebar_drag_target(row, SidebarItem::Account(id), &account.name, center_y, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.select_account(id, window, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.show_account_menu(id, event.position, window, cx);
                }),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if this.has_browser_overlay() {
                    return;
                }
                let key = &event.keystroke;
                match key.key.as_str() {
                    "enter" | "space" => this.select_account(id, window, cx),
                    "f2" => this.open_account_dialog(Some(id), false, window, cx),
                    "f10" if key.modifiers.shift => {
                        this.show_account_menu(id, point(px(96.), px(120.)), window, cx)
                    }
                    "i" if !key.modifiers.control && !key.modifiers.alt => {
                        if this
                            .account_detail
                            .is_some_and(|detail| detail.account == id && detail.keyboard)
                        {
                            this.clear_account_detail(cx);
                        } else {
                            this.prefetch_page_snapshot();
                            this.account_detail = Some(AccountDetail {
                                account: id,
                                anchor_y: anchor_y.get(),
                                keyboard: true,
                            });
                            cx.notify();
                        }
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }))
    }
}

fn sidebar_nav(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    trailing: Option<String>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(36.))
        .px_3()
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .rounded_lg()
        .text_size(px(13.))
        .text_color(rgb(colors().secondary))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(colors().fill_hover)))
        .child(glyph(icon, 16., colors().secondary))
        .child(label)
        .when_some(trailing, |element, text| {
            element.child(div().flex_1()).child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(colors().muted))
                    .child(text),
            )
        })
        .on_click(on_click)
}

impl MuseWindow {
    pub(super) fn render_account_cards(&self, width: f32, cx: &Context<Self>) -> Div {
        // Each sidebar variant renders in its own small stack frame. Debug builds on
        // Windows run GPUI on a 1 MB main-thread stack and builder chains are large.
        if width < 200. {
            self.render_compact_accounts(cx).w(px(width))
        } else {
            self.render_expanded_accounts(width, cx)
        }
    }

    #[inline(never)]
    fn render_detail_content(
        &self,
        account: &muse_core::Account,
        selected: Option<AccountId>,
        cx: &Context<Self>,
    ) -> Div {
        let id = account.id;
        let active = !self.show_start_page && selected == Some(id);
        let refreshing = self.refresh_accounts.contains_key(&id);
        let (used, exhausted, percent) = plan_usage(&account.quota);
        let reset = match &account.quota {
            muse_core::QuotaState::Known(value) | muse_core::QuotaState::Stale(value) => {
                value.resets_at()
            }
            _ => None,
        };
        let plan = self
            .plan_labels
            .get(&id)
            .map(|label| label.trim_start_matches("Muse").trim().to_owned())
            .filter(|label| !label.is_empty());
        let stale = (self.cached_accounts.contains(&id)
            || matches!(
                account.quota,
                muse_core::QuotaState::Stale(_) | muse_core::QuotaState::StaleUnlimited { .. }
            ))
        .then_some(
            if reset.is_some_and(|at| at <= crate::page_observer::now_ms()) {
                "旧周期"
            } else {
                "未更新"
            },
        );
        let status = if self.browsers.account_sleeping(id) {
            "休眠中"
        } else if self.browsers.account_ready(id) {
            "已启动"
        } else if self.browsers.account_running(id) {
            "启动中"
        } else {
            "未启动"
        };
        let proxied = account.network != muse_core::AccountNetwork::Direct;
        let network: String = account
            .network
            .proxy_id()
            .and_then(|proxy| self.workspace.proxy_profile(proxy))
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| account.network.label().to_owned());
        div()
            .w_full()
            .p_3()
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .when(exhausted && !active, |element| element.opacity(0.6))
                            .child(self.account_avatar(account, 40., active)),
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
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .line_clamp(1)
                                            .text_ellipsis()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(account.name.clone()),
                                    )
                                    .when_some(plan, |row, plan| {
                                        row.child(
                                            div()
                                                .flex_none()
                                                .max_w(px(70.))
                                                .line_clamp(1)
                                                .text_ellipsis()
                                                .px_1()
                                                .rounded_sm()
                                                .bg(rgb(colors().fill))
                                                .text_size(px(11.))
                                                .text_color(rgb(colors().secondary))
                                                .child(plan),
                                        )
                                    })
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(11.))
                                            .text_color(rgb(colors().secondary))
                                            .child(status),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        glyph(
                                            if proxied { "shield-check" } else { "world" },
                                            12.,
                                            colors().secondary,
                                        )
                                        .flex_none(),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .line_clamp(1)
                                            .text_ellipsis()
                                            .text_size(px(11.))
                                            .text_color(rgb(colors().secondary))
                                            .child(network),
                                    ),
                            ),
                    )
                    .child(icon_button(
                        "account-refresh",
                        "reload",
                        "刷新额度",
                        !refreshing,
                        refreshing,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.refresh_account(id, window, cx);
                        }),
                    ))
                    .child(icon_button(
                        "account-edit",
                        "pencil",
                        "编辑账号",
                        true,
                        false,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.open_account_dialog(Some(id), false, window, cx);
                        }),
                    )),
            )
            // Quota label, percentage and segmented meter.
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(colors().secondary))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child("套餐额度 已用")
                                    .when_some(stale, |row, label| row.child(label))
                                    .when(refreshing, |row| row.child("正在刷新")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(if exhausted {
                                        colors().danger
                                    } else if used.is_some() {
                                        colors().blue
                                    } else {
                                        colors().muted
                                    }))
                                    .child(percent),
                            ),
                    )
                    .child(quota_meter(used, 8., 3.)),
            )
            // Reset date and invitation quota.
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(rgb(colors().muted))
                            .child(reset_label(reset)),
                    )
                    .child(
                        div().min_w_0().flex().items_center().gap_1().child(
                            div()
                                .min_w_0()
                                .text_size(px(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(colors().secondary))
                                .child(extra_pill(&account.extra_quota)),
                        ),
                    ),
            )
            // Invite code (click to copy) and how many friends redeemed it.
            .child(self.render_invite_row(id, cx))
    }

    fn hover_account(
        &mut self,
        id: AccountId,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if hovered {
            if cx.has_active_drag() {
                return;
            }
            // A page still frame is prepared while the pointer is on the
            // sidebar, so menus and dialogs can cover the page without a gap.
            self.prefetch_page_snapshot();
            if self.avatars.is_animated(id)
                && self.avatar_hover.is_none_or(|(current, _)| current != id)
            {
                self.avatar_hover = Some((id, Instant::now()));
                cx.notify();
            }
            if self.has_browser_overlay() || self.sidebar_dragging {
                return;
            }
            if self
                .account_detail
                .is_some_and(|detail| detail.account == id)
            {
                self.detail_open_timer = None;
                self.detail_close_timer = None;
                return;
            }
            let detail = AccountDetail {
                account: id,
                anchor_y: window.mouse_position().y,
                keyboard: false,
            };
            self.detail_open_timer = Some((
                id,
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(270))
                        .await;
                    let _ = this.update_in(cx, |this, _, cx| {
                        if !this.has_browser_overlay() {
                            this.account_detail = Some(detail);
                            cx.notify();
                        }
                    });
                }),
            ));
        } else {
            if self.avatar_hover.is_some_and(|(current, _)| current == id) {
                self.avatar_hover = None;
                cx.notify();
            }
            if self
                .detail_open_timer
                .as_ref()
                .is_some_and(|(pending, _)| *pending == id)
            {
                self.detail_open_timer = None;
            }
            if self
                .account_detail
                .is_some_and(|detail| detail.account == id && !detail.keyboard)
            {
                self.schedule_detail_close(id, window, cx);
            }
        }
    }

    fn schedule_detail_close(
        &mut self,
        id: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.detail_close_timer = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(220))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this
                    .account_detail
                    .is_some_and(|detail| detail.account == id && !detail.keyboard)
                    && !this
                        .detail_bounds
                        .get()
                        .is_some_and(|bounds| bounds.contains(&window.mouse_position()))
                {
                    this.account_detail = None;
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn clear_account_detail(&mut self, cx: &mut Context<Self>) {
        self.detail_open_timer = None;
        self.detail_close_timer = None;
        self.detail_bounds.set(None);
        if self.account_detail.take().is_some() {
            cx.notify();
        }
    }

    #[inline(never)]
    pub(super) fn render_account_detail(
        &self,
        side: f32,
        viewport: Size<Pixels>,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let detail = self.account_detail?;
        let account = self.workspace.account(detail.account)?;
        let width = (f32::from(viewport.width) - side - 8.).clamp(1., 370.);
        let max_height = (f32::from(viewport.height) - 60.).clamp(1., 335.);
        let top = (detail.anchor_y - px(35.)).max(px(36.));
        let id = detail.account;
        let bounds = self.detail_bounds.clone();
        Some(
            anchored()
                .position(point(px(side), top))
                .snap_to_window_with_margin(px(8.))
                .child(
                    div()
                        .id("account-detail")
                        .w(px(width))
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(colors().line))
                        .bg(rgb(colors().panel))
                        .shadow_lg()
                        .child(
                            canvas(move |area, _, _| bounds.set(Some(area)), |_, _, _, _| {})
                                .absolute()
                                .inset_0(),
                        )
                        .child(
                            div()
                                .id("account-detail-scroll")
                                .w_full()
                                .max_h(px(max_height))
                                .overflow_y_scroll()
                                .occlude()
                                .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, _| {
                                    this.detail_open_timer = None;
                                    this.detail_close_timer = None;
                                }))
                                .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                                    if !*hovered && !detail.keyboard {
                                        this.schedule_detail_close(id, window, cx);
                                    }
                                }))
                                .child(self.render_detail_content(
                                    account,
                                    self.workspace.active_account_id(),
                                    cx,
                                )),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_invite_row(&self, id: AccountId, cx: &Context<Self>) -> Div {
        let row = div()
            .pt(px(8.))
            .border_t_1()
            .border_color(rgb(colors().line))
            .flex()
            .items_center()
            .gap_2();
        let Some(invite) = self.invites.get(&id) else {
            return row
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(rgb(colors().muted))
                        .child("邀请码"),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(11.))
                        .text_color(rgb(colors().muted))
                        .child("等待页面提供邀请信息"),
                );
        };
        let code = invite.code.clone();
        let mut counts = Vec::new();
        if let Some(redeemed) = invite.redeemed {
            counts.push(format!("已邀请 {redeemed} 人"));
        }
        if let Some(remaining) = invite.remaining {
            counts.push(format!("剩 {remaining} 次"));
        }
        let oldest_count = [invite.remaining_at, invite.redeemed_at, invite.earned_at]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(invite.observed_at);
        let age = crate::page_observer::now_ms().saturating_sub(oldest_count) / 60_000;
        let synced = match age {
            0 => "刚刚同步".to_owned(),
            1..=59 => format!("{age} 分钟前同步"),
            _ => format!("{} 小时前同步", age / 60),
        };
        let detail = Some(
            invite
                .earned
                .as_ref()
                .map(|earned| format!("已赚取 {earned} 词元 · "))
                .unwrap_or_default()
                + &synced
                + " · 被动同步，未返回字段保留旧值",
        );
        row.child(
            div()
                .flex_none()
                .text_size(px(11.))
                .text_color(rgb(colors().muted))
                .child("邀请码"),
        )
        .child(
            div()
                .id(SharedString::from(format!("invite-code-{id}")))
                .flex_none()
                .h(px(22.))
                .px(px(6.))
                .rounded_md()
                .bg(rgb(colors().fill))
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(colors().fill_hover)))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors().text))
                        .child(code.clone()),
                )
                .child(glyph("copy", 11., colors().muted))
                .tooltip(move |_, cx| {
                    cx.new(|_| super::chrome::AccountHint("复制邀请码".into()))
                        .into()
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                    this.status = "邀请码已复制".into();
                    cx.notify();
                })),
        )
        .child(div().flex_1())
        .child(
            div()
                .id(SharedString::from(format!("invite-count-{id}")))
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(11.))
                .text_color(rgb(colors().secondary))
                .child(if counts.is_empty() {
                    "邀请数据待同步".to_owned()
                } else {
                    counts.join(" · ")
                })
                .when_some(detail, |element, detail| {
                    element.tooltip(move |_, cx| {
                        cx.new(|_| super::chrome::AccountHint(detail.clone()))
                            .into()
                    })
                }),
        )
    }

    #[inline(never)]
    fn render_expanded_accounts(&self, width: f32, cx: &Context<Self>) -> Div {
        div()
            .w(px(width))
            .flex_none()
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_mouse_move(
                cx.listener(|this, _: &gpui::MouseMoveEvent, _, _| this.prefetch_page_snapshot()),
            )
            .bg(rgb(colors().surface))
            .border_r_1()
            .border_color(rgb(colors().line))
            .child(div().pt_2().child(self.render_home_card(false, cx)))
            .child(
                div()
                    .flex_none()
                    .pl_4()
                    .pr_2()
                    .pt_4()
                    .pb_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(colors().muted))
                            .child("账号"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(colors().muted))
                            .child(self.workspace.accounts.len().to_string()),
                    )
                    .child(div().flex_1())
                    .child(self.new_group_button(cx))
                    .child(icon_button(
                        "sidebar-add",
                        "plus",
                        "新增账号",
                        true,
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.open_account_dialog(None, false, window, cx)
                        }),
                    )),
            )
            .child(
                div()
                    .id("accounts-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb_2()
                    .children(self.render_account_sections(false, cx)),
            )
            .child(
                div()
                    .flex_none()
                    .p_2()
                    .border_t_1()
                    .border_color(rgb(colors().line))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(sidebar_nav(
                        "sidebar-proxy-pool",
                        "shield-check",
                        "代理池",
                        Some(self.workspace.proxy_profiles.len().to_string()),
                        cx.listener(|this, _, window, cx| this.open_proxy_pool(window, cx)),
                    ))
                    .child(sidebar_nav(
                        "sidebar-updates",
                        "download",
                        "检查更新",
                        None,
                        cx.listener(|this, _, window, cx| this.show_updates(window, cx)),
                    ))
                    .child(sidebar_nav(
                        "sidebar-add-footer",
                        "plus",
                        "新增账号",
                        None,
                        cx.listener(|this, _, window, cx| {
                            this.open_account_dialog(None, false, window, cx)
                        }),
                    )),
            )
    }
    /// White selected-card surface. It fades and rises in whenever the selection
    /// moves, so switching accounts or returning to the studio is not an abrupt cut.
    fn selection_layer(&self, id: SharedString) -> AnyElement {
        crate::motion::animate(
            div()
                .id(id)
                .absolute()
                .inset_0()
                .rounded_lg()
                .border_1()
                .border_color(rgb(tone(0xE4E4E7, 0x3F3F46)))
                .bg(rgb(colors().panel))
                .shadow_sm(),
            self.navigation_transition,
            self.motion_policy,
            crate::motion::MotionKind::Modal,
            3.,
        )
    }

    #[inline(never)]
    fn render_compact_accounts(&self, cx: &Context<Self>) -> Div {
        div()
            .w(px(crate::sidebar_state::COMPACT))
            .flex_none()
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .py_2()
            .on_mouse_move(
                cx.listener(|this, _: &gpui::MouseMoveEvent, _, _| this.prefetch_page_snapshot()),
            )
            .bg(rgb(colors().surface))
            .border_r_1()
            .border_color(rgb(colors().line))
            .child(self.render_home_card(true, cx))
            .child(div().w(px(28.)).h(px(1.)).bg(rgb(colors().line)))
            .child(
                div()
                    .id("compact-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(self.render_account_sections(true, cx)),
            )
            .child(self.new_group_button(cx))
            .child(icon_button(
                "compact-add",
                "plus",
                "新增账号",
                true,
                false,
                cx.listener(|this, _, window, cx| {
                    this.open_account_dialog(None, false, window, cx)
                }),
            ))
            .child(icon_button(
                "compact-proxy-pool",
                "shield-check",
                "代理池",
                true,
                false,
                cx.listener(|this, _, window, cx| this.open_proxy_pool(window, cx)),
            ))
            .child(icon_button(
                "compact-updates",
                "download",
                "检查更新",
                true,
                false,
                cx.listener(|this, _, window, cx| this.show_updates(window, cx)),
            ))
    }

    fn new_group_button(&self, cx: &Context<Self>) -> Stateful<Div> {
        icon_button(
            "sidebar-new-group",
            "folder",
            "新建分组",
            true,
            false,
            cx.listener(|this, _, window, cx| {
                this.open_group_dialog(GroupOperation::Name(None), window, cx)
            }),
        )
    }

    fn render_account_sections(&self, compact: bool, cx: &Context<Self>) -> Vec<AnyElement> {
        let mut accounts_by_group: HashMap<Option<Uuid>, Vec<&muse_core::Account>> = HashMap::new();
        for account in self
            .account_groups
            .ordered_accounts(&self.workspace.accounts)
        {
            accounts_by_group
                .entry(self.account_groups.membership(account.id))
                .or_default()
                .push(account);
        }
        std::iter::once((None, UNGROUPED, self.account_groups.ungrouped_collapsed))
            .chain(
                self.account_groups
                    .groups
                    .iter()
                    .map(|group| (Some(group.id), group.name.as_str(), group.collapsed)),
            )
            .map(|(id, name, collapsed)| {
                let accounts = accounts_by_group.remove(&id).unwrap_or_default();
                self.render_account_section(id, name, collapsed, &accounts, compact, cx)
                    .into_any_element()
            })
            .collect()
    }

    #[inline(never)]
    fn render_account_section(
        &self,
        id: Option<Uuid>,
        name: &str,
        collapsed: bool,
        accounts: &[&muse_core::Account],
        compact: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let title = name.to_owned();
        let header = div()
            .id(SharedString::from(id.map_or_else(
                || "group-ungrouped".to_owned(),
                |id| format!("group-{id}"),
            )))
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .hover(|style| style.bg(rgb(colors().fill_hover)))
            .child(glyph(
                if collapsed {
                    "chevron-right"
                } else {
                    "chevron-down"
                },
                12.,
                colors().muted,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(colors().secondary))
                    .child(name.to_owned()),
            )
            .when(!compact, |element| {
                element.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(colors().muted))
                        .child(accounts.len().to_string()),
                )
            })
            .tooltip(move |_, cx| cx.new(|_| super::chrome::AccountHint(title.clone())).into())
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_account_group(id, cx)))
            .when_some(id, |element, id| {
                element.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        this.open_group_dialog(GroupOperation::Name(Some(id)), window, cx);
                        cx.stop_propagation();
                    }),
                )
            });
        div()
            .id(SharedString::from(id.map_or_else(
                || "ungrouped-section".to_owned(),
                |id| format!("section-{id}"),
            )))
            .flex()
            .flex_col()
            .mb_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(compact, |element| element.mx_1())
                    .when(!compact, |element| element.mx_3())
                    .child(self.sidebar_drag_target(
                        header,
                        SidebarItem::Group(id),
                        name,
                        Rc::new(Cell::new(px(0.))),
                        cx,
                    ))
                    .when_some(id.filter(|_| !compact), |element, id| {
                        element.child(icon_button(
                            "edit-group",
                            "pencil",
                            "编辑分组",
                            true,
                            false,
                            cx.listener(move |this, _, window, cx| {
                                this.open_group_dialog(GroupOperation::Name(Some(id)), window, cx)
                            }),
                        ))
                    }),
            )
            .when(!collapsed, |element| {
                element.children(
                    accounts
                        .iter()
                        .map(|account| self.render_account_row(account, compact, cx)),
                )
            })
    }
    #[inline(never)]
    fn render_home_card(&self, compact: bool, cx: &Context<Self>) -> Stateful<Div> {
        let active = self.show_start_page;
        let card = div()
            .id("sidebar-home")
            .relative()
            .flex_none()
            .rounded_lg()
            .flex()
            .items_center()
            .gap_3()
            .cursor_pointer()
            .when(!active, |element| {
                element.hover(|s| s.bg(rgb(colors().fill_hover)))
            })
            .when(active, |element| {
                element.child(self.selection_layer("home-selection".into()))
            })
            .child(glyph(
                "home",
                17.,
                if active {
                    colors().text
                } else {
                    colors().secondary
                },
            ))
            .on_click(cx.listener(|this, _, window, cx| this.show_workspace(window, cx)));
        if compact {
            card.size(px(48.)).justify_center()
        } else {
            card.mx_2()
                .h(px(40.))
                .px_3()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.))
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(rgb(if active {
                            colors().text
                        } else {
                            colors().secondary
                        }))
                        .child("工作室"),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(rgb(colors().muted))
                        .child("图片 · 视频"),
                )
        }
    }
    pub(super) fn clear_untethered_account_detail(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !window.is_window_active()
            || (!window.is_window_hovered()
                && self.account_detail.is_none_or(|detail| !detail.keyboard))
        {
            self.clear_account_detail(cx);
        }
    }
    fn show_account_menu(
        &mut self,
        id: AccountId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.account_dialog.is_none() && self.cookie_dialog.is_none() {
            self.account_context = Some((id, position, TransitionState::entering()));
            self.clear_account_detail(cx);
            window.focus(&self.focus);
            self.ensure_page_freeze();
            self.browsers
                .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
            cx.notify();
        }
    }
    pub(super) fn dismiss_account_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, _, transition)) = self.account_context.as_mut() else {
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
            |this, generation, window, cx| {
                if this
                    .account_context
                    .is_some_and(|(_, _, state)| state.finish_exit(generation))
                {
                    this.account_context = None;
                    this.ensure_active(window);
                    cx.notify();
                }
            },
        );
    }

    pub(super) fn render_account_context(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (id, position, transition) = self.account_context.unwrap();
        let accepts_action = move |this: &Self| {
            !transition.is_exiting()
                && this
                    .account_context
                    .is_some_and(|(_, _, state)| state == transition)
        };
        let name = self
            .workspace
            .account(id)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        let viewport = window.viewport_size();
        let x = f32::from(position.x).clamp(8., (f32::from(viewport.width) - 240.).max(8.));
        let y = f32::from(position.y).clamp(8., (f32::from(viewport.height) - 290.).max(8.));
        let item = |key: &'static str, icon: &'static str, label: &'static str, danger: bool| {
            div()
                .id(key)
                .h(px(32.))
                .px_2()
                .rounded_md()
                .flex()
                .items_center()
                .gap_3()
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(rgb(if danger {
                    tone(0xC54F4A, 0xFF9B94)
                } else {
                    tone(0x3F3F46, 0xD4D4DC)
                }))
                .hover(|s| {
                    s.bg(rgb(if danger {
                        tone(0xFFF1F0, 0x3D2225)
                    } else {
                        tone(0xF4F5F5, 0x2B2B31)
                    }))
                })
                .child(glyph(
                    icon,
                    15.,
                    if danger {
                        tone(0xC54F4A, 0xFF9B94)
                    } else {
                        tone(0x7E7F86, 0xAAAAB5)
                    },
                ))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if !accepts_action(this) {
                        return;
                    }
                    if key == "context-group" {
                        this.account_context = None;
                        this.open_group_dialog(GroupOperation::Move(id), window, cx);
                        return;
                    }
                    this.dismiss_account_context(window, cx);
                    match key {
                        "context-refresh" => this.refresh_account(id, window, cx),
                        "context-edit" => this.open_account_dialog(Some(id), false, window, cx),
                        _ => this.open_account_dialog(Some(id), true, window, cx),
                    }
                }))
        };
        div()
            .id("account-context-backdrop")
            .absolute()
            .inset_0()
            .bg(rgba(0x00000000))
            .when(!transition.is_exiting(), |element| {
                element.occlude().on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        if accepts_action(this) {
                            this.dismiss_account_context(window, cx);
                            cx.stop_propagation();
                        }
                    }),
                )
            })
            .child(crate::motion::animate(
                div()
                    .id("account-context-menu")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(232.))
                    .p(px(6.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(rgb(tone(0xE5E6E8, 0x3F3F46)))
                    .bg(rgb(colors().panel))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_size(px(10.))
                            .text_color(rgb(tone(0x9FA0A7, 0x92929F)))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(name),
                    )
                    .child(item("context-refresh", "reload", "刷新账号信息", false))
                    .child(item("context-edit", "pencil", "编辑账号", false))
                    .child(item("context-group", "folder", "移动到分组…", false))
                    .child(
                        div()
                            .mx_2()
                            .my_1()
                            .h(px(1.))
                            .bg(rgb(tone(0xEFF0F2, 0x36363E))),
                    )
                    .child(
                        div()
                            .id("context-cookies")
                            .h(px(32.))
                            .px_2()
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .gap_3()
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(rgb(tone(0x3F3F46, 0xD4D4DC)))
                            .hover(|s| s.bg(rgb(tone(0xF4F5F5, 0x2B2B31))))
                            .child(glyph("cookie", 15., tone(0x7E7F86, 0xAAAAB5)))
                            .child("提取 Cookie")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if accepts_action(this) {
                                    this.dismiss_account_context(window, cx);
                                    this.open_cookie_dialog(id, window, cx);
                                }
                            })),
                    )
                    .child(
                        div()
                            .id("context-export-accounts")
                            .h(px(32.))
                            .px_2()
                            .rounded_lg()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_size(px(12.))
                            .text_color(rgb(tone(0x3F3F46, 0xD4D4DC)))
                            .hover(|s| s.bg(rgb(tone(0xF4F5F5, 0x2B2B31))))
                            .child(glyph("download", 15., tone(0x7E7F86, 0xAAAAB5)))
                            .child("导出账号与静态代理")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if accepts_action(this) {
                                    this.dismiss_account_context(window, cx);
                                    this.open_transfer(true, id, window, cx);
                                }
                            })),
                    )
                    .child(
                        div()
                            .id("context-import-accounts")
                            .h(px(32.))
                            .px_2()
                            .rounded_lg()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_size(px(12.))
                            .text_color(rgb(tone(0x3F3F46, 0xD4D4DC)))
                            .hover(|s| s.bg(rgb(tone(0xF4F5F5, 0x2B2B31))))
                            .child(glyph("folder", 15., tone(0x7E7F86, 0xAAAAB5)))
                            .child("导入加密账号包")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if accepts_action(this) {
                                    this.dismiss_account_context(window, cx);
                                    this.open_transfer(false, id, window, cx);
                                }
                            })),
                    )
                    .child(item("context-remove", "trash", "移除账号", true)),
                transition,
                self.motion_policy,
                MotionKind::Popover,
                4.,
            ))
            .into_any_element()
    }
    pub(super) fn account_avatar(
        &self,
        account: &muse_core::Account,
        diameter: f32,
        selected: bool,
    ) -> Div {
        // Still first frame by default; the clip plays only while the pointer is
        // over this account's card, timed by the window's own clock.
        let index = self
            .avatar_hover
            .filter(|(hovered, _)| *hovered == account.id)
            .map_or(0, |(_, started)| {
                (started.elapsed().as_millis() / u128::from(self.avatars.frame_ms(account.id)))
                    as usize
            });
        if let Some(image) = self.avatars.frame(account.id, index) {
            div()
                .size(px(diameter))
                .flex_none()
                .rounded_full()
                .bg(rgb(tone(0xF5F6F6, 0x2B2B31)))
                .child(
                    img(image.clone())
                        .id(SharedString::from(format!(
                            "avatar-{}-{:?}",
                            account.id, image.id
                        )))
                        .size_full()
                        .rounded_full()
                        .object_fit(ObjectFit::Cover),
                )
        } else {
            avatar(&account.name, selected, diameter)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{extra_pill, used_percent};
    #[test]
    fn near_full_quota_does_not_claim_exhaustion() {
        let observation = muse_core::QuotaObservation::new(99.8, 100., "%", 1, None).unwrap();
        let (_, exhausted, label) = super::plan_usage(&muse_core::QuotaState::Known(observation));
        assert!(!exhausted);
        assert_eq!(label, ">99%");
        for (used, expected) in [
            (0., 0),
            (0.2, 1),
            (37., 7),
            (97.5, 19),
            (99.8, 19),
            (100., 20),
        ] {
            assert_eq!(super::filled_segments(used), expected);
        }
    }

    #[test]
    fn unknown_and_expired_quota_never_become_zero_usage() {
        assert_eq!(used_percent(&muse_core::QuotaState::Unknown), None);
        let q = muse_core::QuotaObservation::new(9., 100., "%", 1000, None).unwrap();
        assert!(
            (used_percent(&muse_core::QuotaState::Known(q.clone())).unwrap() - 9.).abs() < 0.001
        );
        assert!((used_percent(&muse_core::QuotaState::Stale(q)).unwrap() - 9.).abs() < 0.001);
        assert_eq!(
            extra_pill(&muse_core::ExtraQuotaState::Unknown),
            "额外待同步"
        );
    }
}
