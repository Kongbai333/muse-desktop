use super::chrome::{avatar, glyph, icon_button};
use super::*;
use crate::theme::*;
use gpui::MouseDownEvent;

fn reset_label(at: Option<u64>) -> String {
    use windows::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
    };
    let Some(ticks) = at
        .and_then(|ms| ms.checked_add(11_644_473_600_000))
        .and_then(|ms| ms.checked_mul(10_000))
    else {
        return "重置时间待同步".into();
    };
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    if unsafe { FileTimeToSystemTime(&file, &mut utc) }.is_ok()
        && unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.is_ok()
    {
        format!("{}月{}日重置", local.wMonth, local.wDay)
    } else {
        "重置时间待同步".into()
    }
}
fn used_percent(quota: &muse_core::QuotaState) -> Option<f64> {
    quota.remaining_ratio().map(|r| (1. - r) * 100.)
}
fn extra_pill(state: &muse_core::ExtraQuotaState) -> String {
    match state {
        muse_core::ExtraQuotaState::Known(value) => {
            let text = value
                .label()
                .replace("已使用 ", "")
                .replace("（剩余 ", " · 余 ")
                .replace("(剩余 ", " · 余 ")
                .replace(['）', ')'], "")
                .replace("个口令", "");
            format!("额外 {text}")
        }
        muse_core::ExtraQuotaState::Stale(_) => "额外需刷新".into(),
        _ => "额外待同步".into(),
    }
}

const SEGMENTS: usize = 20;
/// Card text is a soft slate instead of near-black.
const CARD_INK: u32 = 0x3F3F46;
const CARD_MUTED_INK: u32 = 0x71717A;

fn invite_value(state: &muse_core::ExtraQuotaState) -> String {
    match state {
        muse_core::ExtraQuotaState::Known(_) => extra_pill(state)
            .trim_start_matches("额外")
            .trim()
            .to_owned(),
        muse_core::ExtraQuotaState::Stale(_) => "需刷新".into(),
        _ => "待同步".into(),
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
        .text_color(rgb(SECONDARY))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(FILL_HOVER)))
        .child(glyph(icon, 16., SECONDARY))
        .child(label)
        .when_some(trailing, |element, text| {
            element.child(div().flex_1()).child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED_TEXT))
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
    fn render_account_row(
        &self,
        account: &muse_core::Account,
        selected: Option<AccountId>,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let id = account.id;
        let active = !self.show_start_page && selected == Some(id);
        let refreshing = self.refresh_accounts.contains_key(&id);
        let used = used_percent(&account.quota);
        let exhausted = used.is_some_and(|value| value >= 99.5);
        let reset = match &account.quota {
            muse_core::QuotaState::Known(value) => value.resets_at(),
            _ => None,
        };
        let percent: String = match used {
            Some(_) if exhausted => "已用完".into(),
            Some(value) => format!("{value:.0}%"),
            None => match account.quota {
                muse_core::QuotaState::Unlimited { .. } => "不限量",
                muse_core::QuotaState::LoggedOut => "未登录",
                muse_core::QuotaState::Stale(_) => "需刷新",
                muse_core::QuotaState::Unavailable(_) => "同步失败",
                _ => "待同步",
            }
            .into(),
        };
        let plan = self
            .plan_labels
            .get(&id)
            .map(|label| label.trim_start_matches("Muse").trim().to_owned())
            .filter(|label| !label.is_empty());
        let proxied = account.network != muse_core::AccountNetwork::Direct;
        let network: String = account
            .network
            .proxy_id()
            .and_then(|proxy| self.workspace.proxy_profile(proxy))
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| account.network.label().to_owned());
        let filled = used.map_or(0, |value| {
            if value <= 0. {
                0
            } else {
                ((value / 100. * SEGMENTS as f64).round() as usize).clamp(1, SEGMENTS)
            }
        });
        let segment_color = if exhausted {
            0xF04438
        } else if used.is_some_and(|value| value >= 90.) {
            0xF79009
        } else {
            BLUE
        };
        div()
            .id(id)
            .mx_2()
            .mb_1()
            .p_3()
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded_xl()
            .cursor_pointer()
            .when(!active, |element| {
                element.hover(|style| style.bg(rgb(FILL_HOVER)))
            })
            .when(active, |element| {
                element.child(
                    self.card_selection_layer(SharedString::from(format!("row-selection-{id}"))),
                )
            })
            // Avatar, name, plan badge, network, refresh and edit.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .when(exhausted && !active, |element| element.opacity(0.6))
                            .child(self.account_avatar(account, 36., active))
                            .child(
                                div()
                                    .absolute()
                                    .right(px(-1.))
                                    .bottom(px(-1.))
                                    .child(self.account_running_dot(id, cx)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_shrink()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(if exhausted && !active {
                                                MUTED_TEXT
                                            } else {
                                                CARD_INK
                                            }))
                                            .child(account.name.clone()),
                                    )
                                    .when_some(plan, |element, plan| {
                                        element.child(
                                            div()
                                                .flex_none()
                                                .px(px(6.))
                                                .rounded_md()
                                                .border_1()
                                                .border_color(rgb(0xE4E4E7))
                                                .bg(rgb(FILL))
                                                .text_size(px(10.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(rgb(SECONDARY))
                                                .child(plan),
                                        )
                                    })
                                    .when(self.browsers.account_sleeping(id), |element| {
                                        element.child(
                                            div()
                                                .flex_none()
                                                .text_size(px(10.))
                                                .text_color(rgb(MUTED_TEXT))
                                                .child("休眠"),
                                        )
                                    })
                                    .when(self.cached_accounts.contains(&id), |element| {
                                        element.child(
                                            div()
                                                .flex_none()
                                                .text_size(px(10.))
                                                .text_color(rgb(MUTED_TEXT))
                                                .child("缓存"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .min_w_0()
                                    .child(glyph(
                                        if proxied { "shield-check" } else { "world" },
                                        12.,
                                        MUTED_TEXT,
                                    ))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_size(px(12.))
                                            .text_color(rgb(SECONDARY))
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
                                    .text_color(rgb(CARD_MUTED_INK))
                                    .child("套餐额度 已用"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(if exhausted {
                                        0xD92D20
                                    } else if used.is_some() {
                                        BLUE
                                    } else {
                                        MUTED_TEXT
                                    }))
                                    .child(percent),
                            ),
                    )
                    .child(div().w_full().h(px(8.)).flex().gap(px(3.)).children(
                        (0..SEGMENTS).map(|index| {
                            div().flex_1().h_full().rounded(px(2.)).bg(rgb(
                                if exhausted || index < filled {
                                    segment_color
                                } else {
                                    TRACK
                                },
                            ))
                        }),
                    )),
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
                            .text_color(rgb(MUTED_TEXT))
                            .child(reset_label(reset)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED_TEXT))
                                    .child("邀请额度"),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(CARD_MUTED_INK))
                                    .child(invite_value(&account.extra_quota)),
                            ),
                    ),
            )
            // Invite code (click to copy) and how many friends redeemed it.
            .child(self.render_invite_row(id, cx))
            .on_hover(
                cx.listener(move |this, hovered: &bool, _, cx| {
                    this.hover_account(id, *hovered, cx)
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select_account(id, window, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.show_account_menu(id, event.position, window, cx);
                }),
            )
    }

    fn hover_account(&mut self, id: AccountId, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            // A page still frame is prepared while the pointer is on the
            // sidebar, so menus and dialogs can cover the page without a gap.
            self.prefetch_page_snapshot();
            if self.avatars.is_animated(id)
                && self.avatar_hover.is_none_or(|(current, _)| current != id)
            {
                self.avatar_hover = Some((id, Instant::now()));
                cx.notify();
            }
        } else if self.avatar_hover.is_some_and(|(current, _)| current == id) {
            self.avatar_hover = None;
            cx.notify();
        }
    }

    fn render_invite_row(&self, id: AccountId, cx: &Context<Self>) -> Div {
        let row = div()
            .pt(px(8.))
            .border_t_1()
            .border_color(rgb(LINE))
            .flex()
            .items_center()
            .gap_2();
        let Some(invite) = self.invites.get(&id) else {
            return row
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED_TEXT))
                        .child("邀请码"),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED_TEXT))
                        .child("在 Muse 页面打开「邀请」后同步"),
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
        let age = crate::page_observer::now_ms().saturating_sub(invite.observed_at) / 60_000;
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
                + " · 每 30 分钟自动更新",
        );
        row.child(
            div()
                .flex_none()
                .text_size(px(11.))
                .text_color(rgb(MUTED_TEXT))
                .child("邀请码"),
        )
        .child(
            div()
                .id(SharedString::from(format!("invite-code-{id}")))
                .flex_none()
                .h(px(22.))
                .px(px(6.))
                .rounded_md()
                .bg(rgb(FILL))
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(FILL_HOVER)))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(CARD_INK))
                        .child(code.clone()),
                )
                .child(glyph("copy", 11., MUTED_TEXT))
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
                .text_color(rgb(CARD_MUTED_INK))
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

    /// Selected account card: a white raised surface with a hairline border and
    /// soft shadow under the content. It fades and rises in on every switch.
    fn card_selection_layer(&self, id: SharedString) -> AnyElement {
        crate::motion::animate(
            div()
                .id(id)
                .absolute()
                .inset_0()
                .rounded_xl()
                .border_1()
                .border_color(rgb(0xE4E4E7))
                .bg(rgb(0xFFFFFF))
                .shadow_sm(),
            self.navigation_transition,
            self.motion_policy,
            crate::motion::MotionKind::Modal,
            3.,
        )
    }

    #[inline(never)]
    fn render_expanded_accounts(&self, width: f32, cx: &Context<Self>) -> Div {
        let selected = self.workspace.active_account_id();
        let rows = self
            .workspace
            .accounts
            .iter()
            .map(|account| self.render_account_row(account, selected, cx))
            .collect::<Vec<_>>();
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
            .bg(rgb(SIDE))
            .border_r_1()
            .border_color(rgb(LINE))
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
                            .text_color(rgb(MUTED_TEXT))
                            .child("账号"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED_TEXT))
                            .child(self.workspace.accounts.len().to_string()),
                    )
                    .child(div().flex_1())
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
                    .children(rows),
            )
            .child(
                div()
                    .flex_none()
                    .p_2()
                    .border_t_1()
                    .border_color(rgb(LINE))
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
                .border_color(rgb(0xE4E4E7))
                .bg(rgb(0xFFFFFF))
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
            .bg(rgb(SIDE))
            .border_r_1()
            .border_color(rgb(LINE))
            .child(self.render_home_card(true, cx))
            .child(div().w(px(28.)).h(px(1.)).bg(rgb(LINE)))
            .child(
                div()
                    .id("compact-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .children(self.workspace.accounts.iter().map(|account| {
                        let id = account.id;
                        let active =
                            !self.show_start_page && self.workspace.active_account_id() == Some(id);
                        let name = account.name.clone();
                        div()
                            .id(id)
                            .size(px(48.))
                            .flex_none()
                            .relative()
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .when(!active, |element| element.hover(|s| s.bg(rgb(FILL_HOVER))))
                            .when(active, |element| {
                                element.child(self.selection_layer(SharedString::from(format!(
                                    "compact-selection-{id}"
                                ))))
                            })
                            .tooltip(move |_, cx| {
                                cx.new(|_| super::chrome::AccountHint(name.clone())).into()
                            })
                            .child(
                                div()
                                    .relative()
                                    .child(self.account_avatar(account, 34., active))
                                    .child(
                                        div()
                                            .absolute()
                                            .right_0()
                                            .bottom_0()
                                            .child(self.account_running_dot(id, cx)),
                                    ),
                            )
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                this.hover_account(id, *hovered, cx)
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_account(id, window, cx)
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.show_account_menu(id, event.position, window, cx);
                                }),
                            )
                    })),
            )
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
            .when(!active, |element| element.hover(|s| s.bg(rgb(FILL_HOVER))))
            .when(active, |element| {
                element.child(self.selection_layer("home-selection".into()))
            })
            .child(glyph("home", 17., if active { TEXT } else { SECONDARY }))
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
                        .text_color(rgb(if active { TEXT } else { SECONDARY }))
                        .child("工作室"),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED_TEXT))
                        .child("图片 · 视频"),
                )
        }
    }
    fn account_running_dot(&self, id: AccountId, _cx: &Context<Self>) -> Stateful<Div> {
        let running = self.browsers.account_running(id);
        let ready = self.browsers.account_ready(id);
        let sleeping = self.browsers.account_sleeping(id);
        let label = if sleeping {
            "休眠中 · 点击账号或发送任务时自动唤醒"
        } else if ready {
            "已启动"
        } else if running {
            "启动中"
        } else {
            "未启动"
        };
        div()
            .id(SharedString::from(format!("running-{id}")))
            .size(px(11.))
            .flex_none()
            .rounded_full()
            .border_2()
            .border_color(rgb(0xFFFFFF))
            .bg(rgb(if sleeping {
                0xA7C7B7
            } else if ready {
                0x35B77A
            } else if running {
                0xEAB35A
            } else {
                0xDBDCDF
            }))
            .tooltip(move |_, cx| cx.new(|_| super::chrome::AccountHint(label.into())).into())
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

    pub(super) fn render_account_context(&self, cx: &Context<Self>) -> AnyElement {
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
        let x = f32::from(position.x).clamp(8., 300.);
        let y = f32::from(position.y).clamp(8., 620.);
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
                .text_color(rgb(if danger { 0xC54F4A } else { 0x3F3F46 }))
                .hover(|s| s.bg(rgb(if danger { 0xFFF1F0 } else { 0xF4F5F5 })))
                .child(glyph(icon, 15., if danger { 0xC54F4A } else { 0x7E7F86 }))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if !accepts_action(this) {
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
                    .border_color(rgb(0xE5E6E8))
                    .bg(rgb(0xFFFFFF))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_size(px(10.))
                            .text_color(rgb(0x9FA0A7))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(name),
                    )
                    .child(item("context-refresh", "reload", "刷新账号信息", false))
                    .child(item("context-edit", "pencil", "编辑账号", false))
                    .child(div().mx_2().my_1().h(px(1.)).bg(rgb(0xEFF0F2)))
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
                            .text_color(rgb(0x3F3F46))
                            .hover(|s| s.bg(rgb(0xF4F5F5)))
                            .child(glyph("cookie", 15., 0x7E7F86))
                            .child("提取 Cookie")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if accepts_action(this) {
                                    this.dismiss_account_context(window, cx);
                                    this.open_cookie_dialog(id, window, cx);
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
        let fallback = avatar(&account.name, selected, diameter);
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
            fallback.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .overflow_hidden()
                    .bg(rgb(0xF5F6F6))
                    .child(
                        img(image.clone())
                            .id(SharedString::from(format!(
                                "avatar-{}-{:?}",
                                account.id, image.id
                            )))
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    ),
            )
        } else {
            fallback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{extra_pill, used_percent};
    #[test]
    fn unknown_and_expired_quota_never_become_zero_usage() {
        assert_eq!(used_percent(&muse_core::QuotaState::Unknown), None);
        let q = muse_core::QuotaObservation::new(9., 100., "%", 1000, None).unwrap();
        assert!(
            (used_percent(&muse_core::QuotaState::Known(q.clone())).unwrap() - 9.).abs() < 0.001
        );
        assert_eq!(used_percent(&muse_core::QuotaState::Stale(q)), None);
        assert_eq!(
            extra_pill(&muse_core::ExtraQuotaState::Unknown),
            "额外待同步"
        );
    }
}
