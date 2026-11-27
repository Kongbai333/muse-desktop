use super::account_dialog::{ButtonKind, dialog_button, dialog_field};
use super::chrome::glyph;
use super::*;
use crate::account_groups::{AccountGroups, UNGROUPED};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub(super) enum GroupOperation {
    Name(Option<Uuid>),
    Delete(Uuid),
    Move(AccountId),
}

pub(super) struct GroupDialog {
    operation: GroupOperation,
    input: Option<Entity<TextInput>>,
    destination: Option<Uuid>,
    message: String,
}

impl MuseWindow {
    pub(super) fn save_groups(
        &mut self,
        change: impl FnOnce(&mut AccountGroups, &[muse_core::Account]) -> Result<bool, String>,
    ) -> Result<(), String> {
        if let Some(error) = &self.account_group_error {
            return Err(error.clone());
        }
        let mut groups = self.account_groups.clone();
        if change(&mut groups, &self.workspace.accounts)? {
            groups.save(&self.data_dir)?;
            self.account_groups = groups;
        }
        Ok(())
    }

    pub(super) fn toggle_account_group(&mut self, id: Option<Uuid>, cx: &mut Context<Self>) {
        self.clear_account_detail(cx);
        if let Err(error) = self.save_groups(|groups, _| {
            groups.toggle(id);
            Ok(true)
        }) {
            self.status = error;
        }
        cx.notify();
    }

    pub(super) fn open_group_dialog(
        &mut self,
        operation: GroupOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = match operation {
            GroupOperation::Name(id) => {
                let name = id
                    .and_then(|id| self.account_groups.group(id))
                    .map_or("", |group| group.name.as_str());
                Some(cx.new(|cx| {
                    let mut input = TextInput::new(name, cx);
                    input.set_placeholder("输入分组名称");
                    input
                }))
            }
            _ => None,
        };
        let focus = input
            .as_ref()
            .map(|input| input.read(cx).focus_handle(cx))
            .unwrap_or_else(|| self.focus.clone());
        let destination = match operation {
            GroupOperation::Move(account) => self.account_groups.membership(account),
            _ => None,
        };
        self.group_dialog = Some(GroupDialog {
            operation,
            input,
            destination,
            message: self.account_group_error.clone().unwrap_or_default(),
        });
        self.clear_account_detail(cx);
        self.ensure_page_freeze();
        self.browsers
            .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
        window.focus(&focus);
        cx.notify();
    }

    pub(super) fn close_group_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.group_dialog = None;
        window.focus(&self.focus);
        cx.notify();
    }

    pub(super) fn submit_group_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.group_dialog.as_ref() else {
            return;
        };
        let result = match dialog.operation {
            GroupOperation::Name(id) => {
                let name = dialog.input.as_ref().expect("name input").read(cx).text();
                self.save_groups(|groups, _| groups.save_name(id, &name).map(|()| true))
            }
            GroupOperation::Delete(id) => self.save_groups(|groups, _| {
                groups.remove_group(id);
                Ok(true)
            }),
            GroupOperation::Move(account) => {
                let destination = dialog.destination;
                self.save_groups(|groups, accounts| {
                    Ok(groups.membership(account) != destination
                        && groups.place_account(accounts, account, destination, None))
                })
            }
        };
        match result {
            Ok(()) => self.close_group_dialog(window, cx),
            Err(error) => {
                if let Some(dialog) = self.group_dialog.as_mut() {
                    dialog.message = error;
                }
                cx.notify();
            }
        }
    }

    #[inline(never)]
    pub(super) fn render_group_dialog(&self, cx: &Context<Self>) -> Div {
        let dialog = self.group_dialog.as_ref().expect("group dialog");
        let (title, submit, kind) = match dialog.operation {
            GroupOperation::Name(None) => ("新建分组", "创建", ButtonKind::Primary),
            GroupOperation::Name(Some(_)) => ("重命名分组", "保存", ButtonKind::Primary),
            GroupOperation::Delete(_) => ("删除分组", "删除分组", ButtonKind::Danger),
            GroupOperation::Move(_) => ("移动账号到分组", "移动", ButtonKind::Primary),
        };
        let body = match dialog.operation {
            GroupOperation::Name(_) => {
                dialog_field("分组名称", dialog.input.clone().expect("name input"))
            }
            GroupOperation::Delete(id) => {
                let name = self
                    .account_groups
                    .group(id)
                    .map_or("", |group| group.name.as_str());
                div().child(format!("删除「{name}」后，其中的账号将移到「{UNGROUPED}」。账号、浏览器资料和代理绑定会保留。"))
            }
            GroupOperation::Move(account) => self.render_group_destinations(dialog, account, cx),
        };
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x00000042))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(420.))
                    .max_w_full()
                    .p_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(colors().line))
                    .bg(rgb(colors().panel))
                    .shadow_xl()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .text_size(px(crate::theme::HEADING))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(body)
                    .when(!dialog.message.is_empty(), |element| {
                        element.child(
                            div()
                                .text_color(rgb(colors().danger))
                                .child(dialog.message.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .when_some(
                                match dialog.operation {
                                    GroupOperation::Name(id) => id,
                                    _ => None,
                                },
                                |element, id| {
                                    element.child(dialog_button(
                                        "group-delete",
                                        "删除…",
                                        ButtonKind::Secondary,
                                        cx.listener(move |this, _, window, cx| {
                                            this.open_group_dialog(
                                                GroupOperation::Delete(id),
                                                window,
                                                cx,
                                            );
                                        }),
                                    ))
                                },
                            )
                            .child(div().flex_1())
                            .child(dialog_button(
                                "group-cancel",
                                "取消",
                                ButtonKind::Secondary,
                                cx.listener(|this, _, window, cx| {
                                    this.close_group_dialog(window, cx)
                                }),
                            ))
                            .child(dialog_button(
                                "group-save",
                                submit,
                                kind,
                                cx.listener(|this, _, window, cx| {
                                    this.submit_group_dialog(window, cx)
                                }),
                            )),
                    ),
            )
    }

    #[inline(never)]
    fn render_group_destinations(
        &self,
        dialog: &GroupDialog,
        account: AccountId,
        cx: &Context<Self>,
    ) -> Div {
        let current = self.account_groups.membership(account);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when_some(self.workspace.account(account), |element, account| {
                element.child(
                    div()
                        .text_color(rgb(colors().secondary))
                        .child(account.name.clone()),
                )
            })
            .child(
                div()
                    .id("group-destinations")
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .children(
                        std::iter::once((None, UNGROUPED))
                            .chain(
                                self.account_groups
                                    .groups
                                    .iter()
                                    .map(|group| (Some(group.id), group.name.as_str())),
                            )
                            .map(|(id, name)| {
                                let selected = dialog.destination == id;
                                div()
                                    .id(SharedString::from(id.map_or_else(
                                        || "ungrouped".to_owned(),
                                        |id| id.to_string(),
                                    )))
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .cursor_pointer()
                                    .when(selected, |element| element.bg(rgb(colors().fill)))
                                    .hover(|style| style.bg(rgb(colors().fill_hover)))
                                    .child(glyph(
                                        if selected { "circle-check" } else { "folder" },
                                        14.,
                                        if selected {
                                            colors().blue
                                        } else {
                                            colors().muted
                                        },
                                    ))
                                    .child(
                                        div().flex_1().min_w_0().truncate().child(name.to_owned()),
                                    )
                                    .when(current == id, |element| {
                                        element.child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(rgb(colors().muted))
                                                .child("当前分组"),
                                        )
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(dialog) = this.group_dialog.as_mut() {
                                            dialog.destination = id;
                                            cx.notify();
                                        }
                                    }))
                            }),
                    ),
            )
    }

    pub(super) fn group_dialog_key(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match key.key.as_str() {
            "escape" => self.close_group_dialog(window, cx),
            "enter" => self.submit_group_dialog(window, cx),
            "up" | "down" => {
                let Some(dialog) = self.group_dialog.as_mut() else {
                    return;
                };
                if !matches!(dialog.operation, GroupOperation::Move(_)) {
                    return;
                }
                let destinations = std::iter::once(None)
                    .chain(
                        self.account_groups
                            .groups
                            .iter()
                            .map(|group| Some(group.id)),
                    )
                    .collect::<Vec<_>>();
                let current = destinations
                    .iter()
                    .position(|id| *id == dialog.destination)
                    .unwrap_or(0);
                let next = if key.key == "up" {
                    current.saturating_sub(1)
                } else {
                    (current + 1).min(destinations.len() - 1)
                };
                dialog.destination = destinations[next];
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}
