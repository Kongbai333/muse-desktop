//! Reference-driven desktop chrome. Browser/session logic stays in ui.rs.

use super::*;
use crate::theme::*;

pub(super) fn glyph(name: &'static str, width: f32, color: u32) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(width))
        .text_color(rgb(color))
}

struct Hint(&'static str);
impl Render for Hint {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(0x27272A))
            .text_color(rgb(0xFFFFFF))
            .text_size(px(11.))
            .child(self.0)
    }
}

pub(super) fn icon_button(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    enabled: bool,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(28.))
        .flex_none()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(if selected {
            (colors().fill_hover << 8) | 0xFF
        } else {
            0x00000000
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .tooltip(move |_, cx| cx.new(|_| Hint(label)).into())
        .when(enabled, |element| {
            element
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().fill_hover)))
                .active(|s| s.bg(rgb(colors().track)))
                .on_click(on_click)
        })
        .child(glyph(
            icon,
            16.,
            if !enabled {
                tone(0xC0C1C8, 0x676773)
            } else if selected {
                colors().text
            } else {
                tone(0x6E6F76, 0xAAAAB5)
            },
        ))
}

pub(super) fn text_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(32.))
        .px_3()
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(rgb(if primary {
            colors().primary
        } else {
            colors().line
        }))
        .bg(rgb(if primary {
            colors().primary
        } else {
            colors().panel
        }))
        .text_color(rgb(if primary {
            colors().on_primary
        } else {
            colors().text
        }))
        .text_size(px(12.))
        .cursor_pointer()
        .hover(move |s| {
            s.bg(rgb(if primary {
                colors().primary_hover
            } else {
                colors().fill
            }))
        })
        .on_click(on_click)
        .child(label)
}

pub(super) fn input_surface(input: Entity<TextInput>) -> Div {
    div()
        .rounded_md()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(tone(0xE0E1E3, 0x44444D)))
        .bg(rgb(colors().panel))
        .child(input)
}

fn inspector_segment(
    id: &'static str,
    label: &'static str,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .h(px(28.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(if selected {
            (colors().panel << 8) | 0xFF
        } else {
            0x00000000
        }))
        .text_size(px(11.))
        .cursor_pointer()
        .text_color(rgb(if selected {
            colors().text
        } else {
            colors().secondary
        }))
        .child(label)
        .on_click(on_click)
}

pub(super) fn avatar(name: &str, _selected: bool, diameter: f32) -> Div {
    // A stable colour per account name so initials are easy to tell apart.
    const PALETTE: [u32; 6] = [0xE8590C, 0x2B8A9E, 0x5B6CF0, 0x2F9E74, 0xC2557A, 0x7C5CC4];
    let seed = name.chars().fold(0u32, |hash, ch| {
        hash.wrapping_mul(31).wrapping_add(ch as u32)
    });
    div()
        .relative()
        .size(px(diameter))
        .flex_none()
        .rounded_full()
        .bg(rgb(PALETTE[(seed as usize) % PALETTE.len()]))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(diameter * 0.38))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(0xFFFFFF))
        .child(name.chars().next().unwrap_or('M').to_string())
}

pub(super) struct AccountHint(pub String);
impl Render for AccountHint {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(0x27272A))
            .text_color(rgb(0xFFFFFF))
            .text_size(px(11.))
            .child(self.0.clone())
    }
}

impl MuseWindow {
    pub(super) fn show_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.capture_page_zoom();
        if !self.show_start_page {
            self.navigation_transition = crate::motion::TransitionState::entering();
        }
        self.show_start_page = true;
        self.browsers
            .sync_visibility(None, self.browser_bounds(window));
        window.focus(&self.focus);
        cx.notify();
    }

    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.collapsed = !self.sidebar.collapsed;
        self.persist_window();
        self.animate_chrome_layout(window, cx);
    }

    fn toggle_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.inspector_open = !self.inspector_open;
        self.sync_fields(cx);
        self.animate_chrome_layout(window, cx);
    }

    fn rename_current(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.workspace.active_account_id() {
            let name = self.account_name.read(cx).text();
            match self.workspace.rename_account(id, &name) {
                Ok(()) => {
                    self.status = "账号名称已保存".into();
                    self.persist();
                }
                Err(error) => self.status = error,
            }
            cx.notify();
        }
    }

    fn import_previous_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.persist();
        let result = crate::import::default_paths().and_then(|(config, profiles)| {
            crate::import::import_legacy(&self.data_dir, &config, &profiles, None, None)
        });
        match result {
            Ok(imported) => match Workspace::load(&self.data_dir.join("workspace.json")) {
                Ok(mut workspace) => {
                    let migration = crate::proxy_pool::migrate(&self.data_dir, &mut workspace);
                    self.workspace = workspace;
                    self.status = if imported.already_present {
                        "旧版账号已存在，已为你选中".into()
                    } else {
                        "旧版账号已导入，原代理设置已保留".into()
                    };
                    self.select_account(imported.account_id, window, cx);
                    if let Err(error) = migration {
                        self.status = error;
                    }
                }
                Err(error) => self.status = error,
            },
            Err(error) => self.status = error,
        }
        cx.notify();
    }

    fn render_titlebar(&self, window: &Window, cx: &Context<Self>) -> Div {
        // Windows 11 rounds the window's top-right corner (8px). The close
        // button's red hover must follow it, or it shows a square corner.
        let maximized = window.is_maximized();
        let control = |id, icon, close, action: fn(&mut Window, &mut App)| {
            div()
                .id(id)
                .w(px(46.))
                .h(px(TITLEBAR))
                .flex_none()
                .when(close && !maximized, |element| element.rounded_tr(px(8.)))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |s| {
                    s.bg(rgb(if close {
                        0xE81123
                    } else {
                        tone(0xE7E8EA, 0x35353D)
                    }))
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| action(window, cx))
                .child(glyph(icon, 14., tone(0x47484F, 0xD4D4DC)))
        };
        let left = self
            .chrome_layout(window, Instant::now())
            .sidebar_extent()
            .max(48.);
        crate::windows_caption::set_tabs_start(left);
        crate::windows_caption::set_overlay(self.studio.viewer.is_some());
        div()
            .h(px(TITLEBAR))
            .flex_none()
            .flex()
            .items_center()
            .bg(rgb(colors().surface))
            .child(
                div()
                    .w(px(left))
                    .h_full()
                    .flex_none()
                    .pl(px(10.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .child(icon_button(
                        "title-sidebar",
                        "layout-sidebar",
                        if self.sidebar.collapsed {
                            "展开账号栏"
                        } else {
                            "收起账号栏"
                        },
                        true,
                        self.sidebar.collapsed,
                        cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)),
                    ))
                    .when(left >= 140., |element| {
                        element.child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(colors().text))
                                .child("Muse"),
                        )
                    }),
            )
            .when(!self.show_start_page, |element| {
                element.child(self.render_tabs(cx))
            })
            .child(
                div()
                    .id("window-drag-region")
                    .relative()
                    .flex_1()
                    .min_w(px(24.))
                    .h_full()
                    .child(
                        canvas(
                            |bounds, _, _| {
                                crate::windows_caption::set_drag_start(f32::from(bounds.origin.x))
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .child(self.render_theme_switch(cx))
            .child(control(
                "window-minimize",
                "window-minimize",
                false,
                |window, _| window.minimize_window(),
            ))
            .child(control(
                "window-zoom",
                if window.is_maximized() {
                    "window-restore"
                } else {
                    "window-maximize"
                },
                false,
                |window, _| crate::windows_host::toggle_maximize(window),
            ))
            .child(control("window-close", "x", true, |_, cx| cx.quit()))
    }

    fn render_theme_switch(&self, cx: &Context<Self>) -> Stateful<Div> {
        let option = |id: &'static str, label: &'static str, mode: muse_core::ThemeMode| {
            let selected = self.display.theme == mode;
            div()
                .id(id)
                .h(px(26.))
                .px(px(9.))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .text_color(rgb(if selected {
                    colors().text
                } else {
                    colors().secondary
                }))
                .when(selected, |element| element.bg(rgb(colors().panel)))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| this.set_theme(mode, window, cx)))
                .child(label)
        };
        div()
            .id("theme-switch")
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .mr_2()
            .p(px(2.))
            .rounded_lg()
            .bg(rgb(colors().fill))
            .child(
                canvas(
                    |bounds, _, _| crate::windows_caption::set_drag_end(f32::from(bounds.origin.x)),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(option("theme-light", "白天", muse_core::ThemeMode::Light))
            .child(option("theme-dark", "晚上", muse_core::ThemeMode::Dark))
    }

    fn render_sidebar_divider(&self, width: f32, cx: &Context<Self>) -> Stateful<Div> {
        div()
            .id("sidebar-divider")
            .w(px(width))
            .h_full()
            .flex_none()
            .bg(rgb(if self.sidebar_dragging {
                colors().blue
            } else {
                colors().surface
            }))
            .when(!self.sidebar.collapsed, |element| {
                element
                    .cursor_col_resize()
                    .hover(|s| s.bg(rgb(colors().track)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.sidebar_dragging = true;
                            crate::windows_host::capture_pointer(window, true);
                            cx.notify();
                        }),
                    )
            })
    }
    fn drag_sidebar(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sidebar_dragging {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.finish_sidebar_drag(window, cx);
            return;
        }
        self.sidebar.resize(
            f32::from(event.position.x) - crate::sidebar_state::DIVIDER / 2.,
            f32::from(window.viewport_size().width),
            self.inspector_open,
        );
        self.sidebar_motion
            .retarget(self.sidebar.preferred_extent(), Instant::now(), None);
        self.browsers
            .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
        cx.notify();
    }
    fn finish_sidebar_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_dragging {
            self.sidebar_dragging = false;
            crate::windows_host::capture_pointer(window, false);
            self.persist_window();
            cx.notify();
        }
    }

    fn render_tabs(&self, cx: &Context<Self>) -> Div {
        let account = self.workspace.active_account();
        let selected = account.map(|a| a.active_tab_id());
        let tabs = account.map(|a| a.tabs.clone()).unwrap_or_default();
        let rows = tabs
            .into_iter()
            .map(|tab| {
                let id = tab.id;
                let active = !self.show_start_page && selected == Some(id);
                let mut tab_element = div()
                    .id(id)
                    .w(px(200.))
                    .min_w(px(96.))
                    .h(px(32.))
                    .relative()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl(px(10.))
                    .pr_1()
                    .rounded_t_lg()
                    .bg(rgb(if active {
                        colors().panel
                    } else {
                        colors().surface
                    }))
                    .hover(move |s| {
                        s.bg(rgb(if active {
                            colors().panel
                        } else {
                            colors().fill_hover
                        }))
                    })
                    .cursor_pointer()
                    .child(match account {
                        Some(owner) => self.account_avatar(owner, 16., false),
                        None => div().child(glyph(
                            "world",
                            12.,
                            if active {
                                tone(0x71717A, 0xAAAAB5)
                            } else {
                                tone(0x9E9FA6, 0x92929F)
                            },
                        )),
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(12.))
                            .text_color(rgb(if active {
                                colors().text
                            } else {
                                colors().secondary
                            }))
                            .child(tab.title),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("close-{id}")))
                            .size(px(20.))
                            .flex_none()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(rgb(colors().fill_hover)))
                            .child(glyph("x", 12., tone(0x8F9097, 0xAAAAB5)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab_id(id, window, cx);
                            })),
                    );
                if active {
                    tab_element = tab_element.child(crate::motion::animate(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(1.))
                            .bg(rgb(colors().panel)),
                        self.navigation_transition,
                        self.motion_policy,
                        crate::motion::MotionKind::Popover,
                        0.,
                    ));
                }
                tab_element.on_click(cx.listener(move |this, _, window, cx| {
                    this.capture_page_zoom();
                    if this
                        .workspace
                        .active_browser_identity()
                        .is_none_or(|identity| identity.tab_id != id)
                    {
                        this.navigation_transition = TransitionState::entering();
                    }
                    this.show_start_page = false;
                    let _ = this.workspace.select_tab(id);
                    this.sync_fields(cx);
                    this.ensure_active(window);
                    this.persist();
                    cx.notify();
                }))
            })
            .collect::<Vec<_>>();
        div()
            .h(px(TITLEBAR))
            .min_w_0()
            .flex_shrink()
            .flex()
            .items_end()
            .gap(px(2.))
            .bg(rgb(colors().surface))
            .overflow_hidden()
            .children(rows)
            .child(div().flex_none().pb(px(3.)).pl_1().child(icon_button(
                "tab-add",
                "plus",
                "新建标签页 · Ctrl+T",
                true,
                false,
                cx.listener(|this, _, window, cx| this.new_tab(&NewTab, window, cx)),
            )))
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> Div {
        let tab = self.workspace.active_account().and_then(|a| a.active_tab());
        let back = !self.show_start_page && tab.is_some_and(|t| t.can_go_back);
        let forward = !self.show_start_page && tab.is_some_and(|t| t.can_go_forward);
        let loading = !self.show_start_page && tab.is_some_and(|t| t.loading);
        let proxied = matches!(
            self.workspace.active_account().map(|a| a.network),
            Some(network) if network != muse_core::AccountNetwork::Direct
        );
        let proxy_label = if proxied {
            let route = self
                .workspace
                .active_account()
                .and_then(|account| account.network.proxy_id())
                .and_then(|id| self.workspace.proxy_profile(id))
                .map(|profile| profile.name.clone());
            match (self.proxy_editor.applied_protocol, route) {
                (Some(protocol), Some(name)) => format!("{} · {name}", protocol.label()),
                (Some(protocol), None) => format!("代理 · {}", protocol.label()),
                (None, Some(name)) => name,
                (None, None) => "代理已启用".to_string(),
            }
        } else {
            "系统网络".to_string()
        };
        div()
            .h(px(TOOLBAR))
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .gap_1()
            .bg(rgb(colors().panel))
            .border_b_1()
            .border_color(rgb(tone(0xF0F1F3, 0x36363E)))
            .child(icon_button(
                "back",
                "chevron-left",
                "返回 · Alt+←",
                back,
                false,
                cx.listener(|this, _, window, cx| this.command("back", window, cx)),
            ))
            .child(icon_button(
                "forward",
                "chevron-right",
                "前进 · Alt+→",
                forward,
                false,
                cx.listener(|this, _, window, cx| this.command("forward", window, cx)),
            ))
            .child(icon_button(
                "reload",
                "reload",
                "刷新 · Ctrl+R",
                true,
                false,
                cx.listener(|this, _, window, cx| this.command("reload", window, cx)),
            ))
            .child(icon_button(
                "site-home",
                "home",
                "Muse 首页",
                true,
                false,
                cx.listener(|this, _, window, cx| this.command("home", window, cx)),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(32.))
                    .ml_2()
                    .mr_2()
                    .pl_3()
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_lg()
                    .bg(rgb(colors().fill))
                    .border_1()
                    .border_color(rgb(colors().fill))
                    .child(glyph(
                        if self.show_start_page {
                            "layout-grid"
                        } else {
                            "world"
                        },
                        13.,
                        tone(0x999AA1, 0x92929F),
                    ))
                    .when(self.show_start_page, |element| {
                        element.child(
                            div()
                                .flex_1()
                                .text_size(px(11.))
                                .text_color(rgb(tone(0x909198, 0xAAAAB5)))
                                .child("Muse 工作台"),
                        )
                    })
                    .when(!self.show_start_page, |element| {
                        element.child(div().flex_1().min_w_0().child(self.address.clone()))
                    })
                    .when(loading, |element| {
                        element.child(
                            div()
                                .size(px(6.))
                                .flex_none()
                                .rounded_full()
                                .bg(rgb(colors().blue)),
                        )
                    })
                    .child(
                        div()
                            .h(px(24.))
                            .px_2()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded_md()
                            .bg(rgb(colors().panel))
                            .border_1()
                            .border_color(rgb(colors().line))
                            .text_size(px(11.))
                            .text_color(rgb(colors().secondary))
                            .child(div().size(px(6.)).rounded_full().bg(rgb(if proxied {
                                0x16A34A
                            } else {
                                tone(0xA1A1AA, 0x92929F)
                            })))
                            .child(proxy_label),
                    ),
            )
            .child(self.render_zoom_control(cx))
            .child(icon_button(
                "copy-url",
                "copy",
                "复制当前页面网址",
                !self.show_start_page,
                false,
                cx.listener(|this, _, _, cx| {
                    if let Some(tab) = this.workspace.active_account().and_then(|a| a.active_tab())
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(tab.url.clone()));
                        this.status = "页面网址已复制".into();
                        cx.notify();
                    }
                }),
            ))
    }

    fn render_zoom_control(&self, cx: &Context<Self>) -> Div {
        let zoom = self
            .workspace
            .active_browser_identity()
            .filter(|_| !self.show_start_page)
            .and_then(|identity| self.browsers.zoom(identity.tab_id));
        let percent = zoom.map_or(100, |zoom| (zoom * 100.).round() as i64);
        let available = zoom.is_some();
        let custom = percent != 100;
        let at_min = zoom.is_some_and(|z| z <= crate::browser::ZOOM_LEVELS[0] + 0.005);
        let at_max = zoom.is_some_and(|z| {
            z >= crate::browser::ZOOM_LEVELS[crate::browser::ZOOM_LEVELS.len() - 1] - 0.005
        });
        div()
            .h(px(30.))
            .flex_none()
            .mr_1()
            .px(px(2.))
            .flex()
            .items_center()
            .rounded_lg()
            .bg(rgb(if custom {
                tone(0xEEF3FE, 0x24334F)
            } else {
                colors().fill
            }))
            .child(icon_button(
                "zoom-out",
                "window-minimize",
                "缩小 · Ctrl+-",
                available && !at_min,
                false,
                cx.listener(|this, _, window, cx| this.command("zoom_out", window, cx)),
            ))
            .child(
                div()
                    .id("zoom-reset")
                    .w(px(44.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .text_size(px(11.))
                    .font_weight(if custom {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(rgb(if !available {
                        colors().muted
                    } else if custom {
                        colors().blue
                    } else {
                        colors().secondary
                    }))
                    .when(available && custom, |element| {
                        element
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(colors().panel)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.command("zoom_reset", window, cx)
                            }))
                    })
                    .tooltip(move |_, cx| {
                        cx.new(|_| {
                            Hint(if custom {
                                "点击恢复 100% · Ctrl+0"
                            } else {
                                "页面缩放 · Ctrl+滚轮"
                            })
                        })
                        .into()
                    })
                    .child(format!("{percent}%")),
            )
            .child(icon_button(
                "zoom-in",
                "plus",
                "放大 · Ctrl+=",
                available && !at_max,
                false,
                cx.listener(|this, _, window, cx| this.command("zoom_in", window, cx)),
            ))
    }

    fn render_inspector(&self, cx: &Context<Self>) -> Div {
        let account_tab = inspector_segment(
            "settings-account-tab",
            "账号",
            self.inspector_section == InspectorSection::Account,
            cx.listener(|this, _, _, cx| {
                this.inspector_section = InspectorSection::Account;
                cx.notify();
            }),
        );
        let proxy_tab = inspector_segment(
            "settings-proxy-tab",
            "代理",
            self.inspector_section == InspectorSection::Proxy,
            cx.listener(|this: &mut MuseWindow, _, _, cx| {
                this.inspector_section = InspectorSection::Proxy;
                cx.notify();
            }),
        );
        div()
            .w(px(INSPECTOR))
            .flex_none()
            .min_w(px(0.))
            .overflow_hidden()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(colors().panel))
            .border_l_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .h(px(48.))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("账号设置"),
                    )
                    .child(icon_button(
                        "close-inspector",
                        "x",
                        "收起设置",
                        true,
                        false,
                        cx.listener(|this, _, window, cx| this.toggle_inspector(window, cx)),
                    )),
            )
            .child(
                div()
                    .mx_4()
                    .mb_3()
                    .p(px(3.))
                    .rounded_lg()
                    .bg(rgb(tone(0xEDEEF0, 0x2B2B31)))
                    .flex()
                    .gap_1()
                    .child(account_tab)
                    .child(proxy_tab),
            )
            .child(
                div()
                    .id("inspector-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_5()
                    .pb_5()
                    .when(
                        self.inspector_section == InspectorSection::Account,
                        |element| element.child(self.render_account_settings(cx)),
                    )
                    .when(
                        self.inspector_section == InspectorSection::Proxy,
                        |element| {
                            if self.smoke.is_some() {
                                element.child(self.render_proxy_settings(cx))
                            } else {
                                element.child(text_button(
                                    "inspector-proxy-pool",
                                    "管理代理池",
                                    false,
                                    cx.listener(|this, _, window, cx| {
                                        this.open_proxy_pool(window, cx)
                                    }),
                                ))
                            }
                        },
                    ),
            )
    }

    fn render_account_settings(&self, cx: &Context<Self>) -> Div {
        let account = self.workspace.active_account();
        let name = account.map(|a| a.name.clone()).unwrap_or_default();
        let quota = account.map(|a| a.quota.label()).unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(avatar(
                        &name,
                        true,
                        42.,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .overflow_hidden().whitespace_nowrap()
                            .child(name),
                    ),
            )
            .child(div().h(px(1.)).bg(rgb(colors().line)))
            .child(
                div()
                    .child(
                        div()
                            .mb_2()
                            .text_size(px(11.))
                            .text_color(rgb(colors().secondary))
                            .child("账号名称"),
                    )
                    .child(input_surface(self.account_name.clone()))
                    .child(div().mt_3().flex().justify_end().child(text_button(
                        "save-account-name",
                        "保存",
                        true,
                        cx.listener(|this, _, _, cx| this.rename_current(cx)),
                    ))),
            )
            .child(div().h(px(1.)).bg(rgb(colors().line)))
            .child(
                div()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(colors().secondary))
                            .child("套餐额度"),
                    )
                    .child(div().mt_2().text_size(px(13.)).child(quota))
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(11.))
                            .line_height(px(19.))
                            .text_color(rgb(colors().secondary))
                            .child("头像外圈显示剩余额度。页面就绪后自动同步，数据超过五分钟会标记过期。"),
                    )
                    .child(text_button("refresh-quota","刷新额度",false,
                        cx.listener(|this,_,window,cx|this.refresh_current_quota(window,cx))))
                    .when_some(account.and_then(|a|a.quota.observed_at()),|element,time| {
                        let age=crate::page_observer::now_ms().saturating_sub(time)/1000;
                        element.child(div().mt_2().text_size(px(10.)).text_color(rgb(colors().secondary)).child(format!("{} 秒前同步",age)))
                    })
                    .when_some(account.and_then(|a| match &a.quota {
                        muse_core::QuotaState::Known(v) | muse_core::QuotaState::Stale(v) => v.resets_at(),
                        _ => None,
                    }),|element,time| {
                        let hours=time.saturating_sub(crate::page_observer::now_ms()).div_ceil(3_600_000);
                        element.child(div().mt_1().text_size(px(10.)).text_color(rgb(colors().secondary)).child(format!("约 {hours} 小时后重置")))
                    }),
            )
            .child(
                div()
                    .child(div().text_size(px(11.)).text_color(rgb(colors().secondary)).child("邀请 / 额外额度"))
                    .child(div().mt_2().text_size(px(13.)).child(account.map(|a|a.extra_quota.label()).unwrap_or_else(||"尚未读取额外额度".into())))
                    .child(div().mt_2().text_size(px(11.)).line_height(px(18.)).text_color(rgb(colors().secondary)).child("单独显示 Muse 返回的额外额度，不计入套餐外圈。"))
            )
            .child(div().h(px(1.)).bg(rgb(colors().line)))
            .child(div().text_size(px(11.)).text_color(rgb(colors().secondary)).child(
                if account.is_some_and(|a|self.browsers.profile_verified(a.id)) {"浏览器目录已核验 · 独立账号环境"} else {"浏览器目录待核验"}
            ))
            .child(div().text_size(px(11.)).text_color(rgb(colors().secondary)).child(
                if self.workspace.active_browser_identity().is_some_and(|id|self.browsers.page_ready(id)) {"页面已就绪"} else {"页面准备中"}
            ))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(colors().secondary))
                    .child(account.map(|a| a.network.label()).unwrap_or("系统网络")),
            )
            .child(text_button(
                "import-previous-account",
                "导入旧版当前账号",
                false,
                cx.listener(|this, _, window, cx| this.import_previous_account(window, cx)),
            ))
            .child(
                div()
                    .id("remove-account")
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .text_color(rgb(colors().danger))
                    .child(glyph("trash", 14., colors().danger))
                    .child(
                        div()
                            .text_size(px(11.))
                            .child(if self.remove_confirmation.is_some() {
                                "再次点击，确认移除"
                            } else {
                                "移除此账号"
                            }),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.remove_active(window, cx))),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .line_height(px(18.))
                    .text_color(rgb(colors().secondary))
                    .child("移除仅影响列表，浏览器数据会保留。"),
            )
    }

    fn render_proxy_settings(&self, cx: &Context<Self>) -> Div {
        let editor = &self.proxy_editor;
        let account = self.workspace.active_account();
        let current_mode = account.map(|a| a.network.label()).unwrap_or("系统网络");
        let count = account.and_then(|account| self.browsers.request_count(account.id));
        let field = |label, input| {
            div()
                .child(
                    div()
                        .mb_1()
                        .text_size(px(11.))
                        .text_color(rgb(colors().secondary))
                        .child(label),
                )
                .child(input_surface(input))
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(colors().line))
                    .bg(rgb(tone(0xF4F5F5, 0x2B2B31)))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("当前：{current_mode}")),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(10.))
                            .text_color(rgb(colors().secondary))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(editor.applied_summary.clone()),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(10.))
                            .text_color(rgb(colors().secondary))
                            .child(
                                count
                                    .map(|count| format!("已处理 {count} 次代理请求"))
                                    .unwrap_or_else(|| "尚无代理请求".into()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(text_button(
                        "proxy-system-mode",
                        "系统网络",
                        !editor.enabled,
                        cx.listener(|this, _, _, cx| this.select_proxy_protocol(None, cx)),
                    ))
                    .children(muse_proxy::ProxyProtocol::ALL.map(|protocol| {
                        text_button(
                            protocol.scheme(),
                            protocol.label(),
                            editor.enabled && editor.protocol == protocol,
                            cx.listener(move |this, _, _, cx| {
                                this.select_proxy_protocol(Some(protocol), cx)
                            }),
                        )
                    })),
            )
            .when(editor.enabled, |element| {
                element
                    .child(field("服务器", editor.host.clone()))
                    .child(field("端口", editor.port.clone()))
                    .child(field("用户名（可选）", editor.user.clone()))
                    .child(
                        div()
                            .child(
                                div()
                                    .mb_1()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(colors().secondary))
                                            .child("密码"),
                                    )
                                    .child(
                                        div()
                                            .id("proxy-reveal-secret")
                                            .text_size(px(10.))
                                            .text_color(rgb(colors().blue))
                                            .cursor_pointer()
                                            .child(if editor.reveal { "隐藏" } else { "显示" })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.proxy_editor.reveal =
                                                    !this.proxy_editor.reveal;
                                                let secure = !this.proxy_editor.reveal;
                                                this.proxy_editor.secret.update(cx, |input, cx| {
                                                    input.set_secure(secure, cx)
                                                });
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(input_surface(editor.secret.clone())),
                    )
                    .child(
                        div()
                            .id("proxy-force")
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .child(
                                div()
                                    .size(px(14.))
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(rgb(if editor.force {
                                        colors().blue
                                    } else {
                                        tone(0xC8C9CC, 0x60606D)
                                    }))
                                    .bg(rgb(if editor.force {
                                        colors().blue
                                    } else {
                                        colors().panel
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .size_full()
                                            .text_size(px(10.))
                                            .text_color(rgb(colors().on_primary))
                                            .child(if editor.force { "✓" } else { "" }),
                                    ),
                            )
                            .child(div().text_size(px(10.)).child("本机地址也经过此代理"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.proxy_editor.force = !this.proxy_editor.force;
                                this.proxy_draft_changed(cx);
                            })),
                    )
                    .child(text_button(
                        "proxy-paste-url",
                        "从剪贴板填入代理地址",
                        false,
                        cx.listener(|this, _, _, cx| this.paste_proxy(cx)),
                    ))
            })
            .child(div().h(px(1.)).bg(rgb(colors().line)))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(text_button(
                        "proxy-test",
                        if editor.pending.is_some() {
                            "测试中…"
                        } else {
                            "测试连接"
                        },
                        false,
                        cx.listener(|this, _, _, cx| {
                            if this.proxy_editor.pending.is_none() {
                                this.test_proxy(cx);
                            }
                        }),
                    ))
                    .child(text_button(
                        "proxy-save",
                        "保存并应用",
                        true,
                        cx.listener(|this, _, window, cx| this.save_proxy(window, cx)),
                    )),
            )
            .child(
                div()
                    .id("proxy-revert")
                    .text_size(px(10.))
                    .text_color(rgb(colors().blue))
                    .cursor_pointer()
                    .child("还原已保存配置")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh_proxy_editor(cx);
                        cx.notify();
                    })),
            )
            .when(!editor.message.is_empty(), |element| {
                element.child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(18.))
                        .text_color(rgb(if editor.good {
                            tone(0x27755B, 0x77D4A4)
                        } else {
                            tone(0xB42318, 0xFF9B94)
                        }))
                        .child(editor.message.clone()),
                )
            })
            .child(
                div()
                    .text_size(px(10.))
                    .line_height(px(18.))
                    .text_color(rgb(colors().secondary))
                    .child("测试不保存配置。应用会重新加载当前账号的页面，其他账号不受影响。"),
            )
    }

    fn render_browser_fallback(&self, cx: &Context<Self>) -> Div {
        if let Some(frame) = self.frozen_page() {
            return div()
                .flex_1()
                .min_h_0()
                .relative()
                .overflow_hidden()
                .bg(rgb(colors().panel))
                .child(
                    img(frame)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Fill),
                );
        }
        let identity = self.workspace.active_browser_identity();
        let error = identity.and_then(|identity| self.browsers.startup_error(identity));
        let page_ready = identity.is_some_and(|identity| self.browsers.page_ready(identity));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .px_5()
            .text_size(px(12.))
            .text_color(rgb(colors().secondary))
            .when_some(error, |element, error| {
                element
                    .child(
                        div()
                            .text_color(rgb(colors().text))
                            .child("无法打开此账号的网页"),
                    )
                    .child(div().max_w(px(420.)).child(error.to_owned()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(text_button(
                                "retry-browser",
                                "重试",
                                true,
                                cx.listener(|this, _, window, cx| this.retry_browser(window, cx)),
                            ))
                            .child(text_button(
                                "browser-error-proxy",
                                "检查代理",
                                false,
                                cx.listener(|this, _, window, cx| {
                                    this.open_proxy_settings(window, cx)
                                }),
                            )),
                    )
            })
            .when(
                error.is_none() && !page_ready && !self.has_browser_overlay(),
                |element| element.child("正在准备此账号的页面…"),
            )
    }
}

impl Render for MuseWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let inspector_target = self.inspector_target();
        self.inspector_motion.retarget(
            inspector_target,
            now,
            self.motion_policy
                .duration(crate::motion::MotionKind::Layout),
        );
        let layout = self.chrome_layout(window, now);
        let divider_width = (layout.sidebar_extent() - crate::sidebar_state::COMPACT)
            .clamp(0., crate::sidebar_state::DIVIDER);
        if self.sidebar_motion.is_animating(now) || self.inspector_motion.is_animating(now) {
            window.request_animation_frame();
        }
        self.ensure_page_freeze();
        self.browsers
            .sync_visibility(self.visible_browser_identity(), Self::browser_rect(layout));
        let loading = !self.show_start_page
            && self
                .workspace
                .active_account()
                .and_then(|a| a.active_tab())
                .is_some_and(|t| t.loading);
        let status = if !self.save_enabled {
            format!("配置文件有误，已禁止覆盖保存：{}", self.status)
        } else if let Some(error) = self
            .workspace
            .active_account()
            .and_then(|account| account.active_tab())
            .and_then(|tab| tab.error.clone())
        {
            error
        } else {
            self.status.clone()
        };
        div()
            .key_context(if self.updates.open {
                "UpdateDialog"
            } else if self.transfer_dialog.is_some() {
                "TransferDialog"
            } else if self.account_dialog.is_some() {
                "Muse AccountDialog"
            } else if self.cookie_dialog.is_some() {
                "Muse CookieDialog"
            } else if self.account_context.is_some() {
                "Muse AccountContext"
            } else {
                "Muse"
            })
            .relative()
            .track_focus(&self.focus)
            .w(window.viewport_size().width)
            .h(window.viewport_size().height)
            .overflow_hidden()
            .flex()
            .flex_col()
            .font_family("Segoe UI Variable")
            .text_size(px(13.))
            .text_color(rgb(colors().text))
            .bg(rgb(colors().panel))
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::forward))
            .on_action(cx.listener(Self::reload))
            .on_action(cx.listener(Self::go_home))
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::focus_address))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::copy_viewer_image))
            .on_action(cx.listener(Self::submit_address))
            .on_action(cx.listener(|this, _: &DismissStudioPanels, window, cx| {
                if this.has_browser_overlay() {
                    this.dismiss_browser_overlay(window, cx);
                } else if this.studio.viewer.is_some() {
                    this.close_studio_image(window, cx);
                } else {
                    this.close_studio_panels(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CancelAccountDialog, window, cx| {
                this.dismiss_browser_overlay(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SaveAccountDialog, window, cx| {
                if this.transfer_dialog.is_some() {
                    this.submit_transfer(window, cx);
                } else {
                    this.save_account_dialog(window, cx);
                }
            }))
            .on_mouse_move(cx.listener(Self::drag_sidebar))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.finish_sidebar_drag(window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.finish_sidebar_drag(window, cx)),
            )
            .child(self.render_titlebar(window, cx))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .flex()
                    .child(self.render_account_cards(layout.sidebar_extent() - divider_width, cx))
                    .when(divider_width > 0., |element| {
                        element.child(self.render_sidebar_divider(divider_width, cx))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .when(!self.show_start_page, |element| {
                                element.child(self.render_toolbar(cx))
                            })
                            .when(self.show_start_page, |element| {
                                element.child(crate::motion::animate(
                                    self.render_studio_home(layout.width, window, cx),
                                    self.navigation_transition,
                                    self.motion_policy,
                                    crate::motion::MotionKind::Modal,
                                    8.,
                                ))
                            })
                            .when(!self.show_start_page, |element| {
                                element.child(self.render_browser_fallback(cx))
                            }),
                    )
                    .when(layout.inspector_width > 0., |element| {
                        element.child(
                            div()
                                .w(px(layout.inspector_width))
                                .h_full()
                                .flex_none()
                                .overflow_hidden()
                                .when(self.inspector_open, |element| {
                                    element.child(self.render_inspector(cx))
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(STATUS))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_t_1()
                    .border_color(rgb(colors().line))
                    .bg(rgb(colors().surface))
                    .child(glyph("shield-check", 12., colors().muted))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(rgb(colors().muted))
                            .child("账号数据仅保存在本机"),
                    )
                    .child(div().flex_1())
                    .when(loading, |element| {
                        element.child(div().size(px(6.)).rounded_full().bg(rgb(colors().blue)))
                    })
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(11.))
                            .text_color(rgb(colors().secondary))
                            .child(if self.show_start_page {
                                format!("工作室 · {} 个账号", self.workspace.accounts.len())
                            } else if loading {
                                "正在加载页面".into()
                            } else if status.is_empty() || status.contains("保存在本机") {
                                self.workspace
                                    .active_account()
                                    .map(|account| account.name.clone())
                                    .unwrap_or_default()
                            } else {
                                status
                            }),
                    ),
            )
            .when(
                self.show_start_page && self.studio.viewer.is_some(),
                |element| element.child(self.render_studio_image_overlay(window, cx)),
            )
            .when(self.account_context.is_some(), |element| {
                element.child(self.render_account_context(cx))
            })
            .when(self.account_dialog.is_some(), |element| {
                element.child(self.render_account_dialog(window, cx))
            })
            .when(self.cookie_dialog.is_some(), |element| {
                element.child(self.render_cookie_dialog(window, cx))
            })
            .when(self.transfer_dialog.is_some(), |element| {
                element.child(self.render_transfer(window, cx))
            })
            .when(self.updates.open, |element| {
                element.child(self.render_updates(cx))
            })
            .child(self.studio_layout_probe("root"))
    }
}
