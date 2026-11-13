//! The pinned lock message and identity verification are separate cards.
use super::*;
use crate::{
    forms::{NativeForm, string},
    workspace::{Palette, icon, icon_tool},
};
use gpui_kit::{
    Div, Focusable,
    component::select::{Select, SelectEvent},
};

pub(super) fn accept_status(current: &Value, next: &Value) -> bool {
    next["enabled"].is_boolean()
        && next["locked"].is_boolean()
        && next["revision"].as_u64().unwrap_or(0) >= current["revision"].as_u64().unwrap_or(0)
}

pub(super) fn unlock_methods(status: &Value) -> Vec<(String, String)> {
    [
        ("password", "密码解锁", "passwordConfigured"),
        ("totp", "2FA 验证码解锁", "twoFactorConfigured"),
    ]
    .into_iter()
    .filter(|(_, _, key)| status[*key] == true)
    .map(|(id, label, _)| (id.into(), label.into()))
    .collect()
}

impl DevShellWindow {
    pub(super) fn request_window_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.close_visible {
            return;
        }
        self.ui.close_visible = true;
        if self.snapshot.locked {
            self.ui
                .settings
                .unlock
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.ui.settings.lock_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_locked_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ui.close_visible = false;
        if self.ui.settings.unlock_form.is_some() {
            self.ui
                .settings
                .unlock
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        } else {
            self.ui.settings.lock_focus.focus(window, cx);
        }
        cx.notify();
    }

    fn locked_exit_description(&self, cx: &App) -> String {
        let mut count = 0;
        let mut unsaved = 0;
        let mut transfers = std::collections::BTreeSet::new();
        for session in std::iter::once(&self.workspace).chain(self.parked.iter()) {
            if session
                .terminal
                .as_ref()
                .is_some_and(|t| t.protocol == "ssh" && t.input_tx.is_some())
            {
                count += 1;
            }
            for transfer in &session.transfers {
                if matches!(transfer.status.as_str(), "queued" | "uploading") {
                    transfers.insert(&transfer.id);
                }
            }
            if let Some(preview) = session.preview.as_ref() {
                let text = session
                    .editor
                    .as_ref()
                    .map(|editor| editor.read(cx).text().to_string())
                    .unwrap_or_else(|| preview.text.clone());
                if text != preview.saved_text {
                    unsaved += 1;
                }
            }
        }
        close_description(count, transfers.len(), unsaved)
    }

    fn locked_exit_screen(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .absolute()
            .inset_0()
            .p(px(24.))
            .bg(gpui_kit::rgb(if self.ui.dark {
                0x0b1015
            } else {
                0x27333c
            }))
            .occlude()
            .track_focus(&self.ui.settings.lock_focus)
            .flex()
            .items_center()
            .justify_center()
            .on_key_down(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            .child(
                div()
                    .w(px(400.))
                    .max_w_full()
                    .p(px(28.))
                    .border_1()
                    .border_color(gpui_kit::rgb(p.border))
                    .rounded(px(20.))
                    .bg(gpui_kit::rgb(p.surface))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .size(px(52.))
                            .mb(px(16.))
                            .rounded(px(17.))
                            .border_1()
                            .border_color(gpui_kit::rgb(p.border))
                            .bg(gpui_kit::rgb(if self.ui.dark { p.inset } else { 0xf6f8fa }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("close", 26.)),
                    )
                    .child(
                        div()
                            .text_size(px(21.))
                            .line_height(px(31.5))
                            .font_weight(gpui_kit::FontWeight(550.))
                            .child("关闭当前窗口？"),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .mt(px(10.))
                            .mb(px(24.))
                            .text_size(px(12.))
                            .line_height(px(20.4))
                            .text_color(gpui_kit::rgb(p.muted))
                            .child(self.locked_exit_description(cx)),
                    )
                    .child(
                        KitButton::new("security-exit-confirm")
                            .primary()
                            .w_full()
                            .h(px(40.))
                            .rounded(px(10.))
                            .label("关闭窗口")
                            .on_click(|_, _, cx| cx.quit()),
                    )
                    .child(
                        KitButton::new("security-exit-cancel")
                            .ghost()
                            .w_full()
                            .h(px(36.))
                            .mt(px(10.))
                            .rounded(px(9.))
                            .label("继续保持锁定")
                            .on_click(cx.listener(|state, _, window, cx| {
                                state.cancel_locked_close(window, cx)
                            })),
                    ),
            )
            .into()
    }

    pub(super) fn open_unlock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let methods = unlock_methods(&self.snapshot.security);
        let method = if methods.first().is_some_and(|m| m.0 == "totp") {
            "totp"
        } else {
            "password"
        };
        let mut form = NativeForm::new("unlock", serde_json::json!({}), window, cx);
        form.add_choices("method", methods, method, window, cx);
        cx.subscribe_in(
            &form.choices["method"],
            window,
            |state, _, event: &SelectEvent<Vec<forms::Choice>>, window, cx| {
                if let SelectEvent::Confirm(Some(method)) = event {
                    state.set_unlock_method(
                        if method == "totp" { "totp" } else { "password" },
                        window,
                        cx,
                    );
                }
            },
        )
        .detach();
        self.ui.settings.unlock_form = Some(form);
        self.set_unlock_method(method, window, cx);
        self.ui
            .settings
            .unlock
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    fn set_unlock_method(
        &mut self,
        method: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ui.settings.unlock_method = method;
        self.ui.settings.error = None;
        self.ui.settings.unlock.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_masked(method == "password", window, cx);
            input.set_placeholder(
                if method == "totp" {
                    "000000"
                } else {
                    "请输入解锁密码"
                },
                window,
                cx,
            );
        });
        cx.notify();
    }

    pub(super) fn submit_unlock(&mut self, cx: &mut Context<Self>) {
        if !self.snapshot.locked
            || self.ui.settings.unlock_form.is_none()
            || self.ui.settings.busy
            || self.snapshot.security["retryAfterSeconds"]
                .as_u64()
                .unwrap_or(0)
                > 0
        {
            return;
        }
        let value = self.ui.settings.unlock.read(cx).value().to_string();
        if value.is_empty() {
            return;
        }
        self.security_call(
            "unlock",
            serde_json::json!({"method":self.ui.settings.unlock_method,"value":value}),
            cx,
        );
    }

    pub(super) fn lock_screen(
        &mut self,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if self.ui.close_visible {
            return self.locked_exit_screen(p, cx);
        }
        let editing = self.ui.settings.unlock_form.is_some();
        if !editing && !self.ui.settings.lock_focus.contains_focused(window, cx) {
            self.ui.settings.lock_focus.focus(window, cx);
        }
        let totp = self.ui.settings.unlock_method == "totp";
        let retry = self.snapshot.security["retryAfterSeconds"]
            .as_u64()
            .unwrap_or(0);
        let note = if totp {
            "验证码每 30 秒变化，已使用的验证码不能重复使用。".to_owned()
        } else {
            let hint = string(&self.snapshot.security, "passwordHint");
            if hint.is_empty() {
                String::new()
            } else {
                format!("密码提醒：{hint}")
            }
        };
        let label = |s: &str| {
            div()
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(gpui_kit::rgb(p.muted))
                .child(s.to_owned())
        };
        let mut card = div()
            .id("security-lock-card")
            .w(px(400.))
            .max_w_full()
            .max_h_full()
            .overflow_y_scroll()
            .p(px(28.))
            .border_1()
            .border_color(gpui_kit::rgb(p.border))
            .rounded(px(20.))
            .bg(gpui_kit::rgb(p.surface))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .size(px(52.))
                    .flex_shrink_0()
                    .mb(px(16.))
                    .rounded(px(17.))
                    .border_1()
                    .border_color(gpui_kit::rgb(p.border))
                    .bg(gpui_kit::rgb(if self.ui.dark { p.inset } else { 0xf6f8fa }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("lock", 26.)),
            )
            .child(
                div()
                    .text_size(px(21.))
                    .line_height(px(31.5))
                    .font_weight(gpui_kit::FontWeight(550.))
                    .child(if editing {
                        "解锁 DevShell"
                    } else {
                        "软件已被锁定"
                    }),
            )
            .child(
                div()
                    .mt(px(10.))
                    .mb(px(24.))
                    .text_size(px(12.))
                    .line_height(px(20.4))
                    .text_color(gpui_kit::rgb(p.muted))
                    .child(if editing {
                        "验证身份，返回你的工作台"
                    } else {
                        "解锁后即可继续使用，SSH 连接保持运行。"
                    }),
            );
        if let Some(form) = self.ui.settings.unlock_form.as_ref() {
            if unlock_methods(&self.snapshot.security).len() > 1 {
                card = card.child(
                    div()
                        .w_full()
                        .mb(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(label("解锁方式"))
                        .child(
                            Select::new(&form.choices["method"])
                                .w_full()
                                .h(px(40.))
                                .appearance(false)
                                .border_1()
                                .border_color(gpui_kit::rgb(p.border))
                                .bg(gpui_kit::rgb(if self.ui.dark { p.inset } else { 0xf6f8fa }))
                                .px(px(12.))
                                .rounded(px(10.))
                                .text_size(px(13.)),
                        ),
                );
            }
            card = card
                .child(
                    div()
                        .w_full()
                        .mb(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(label(if totp {
                            "验证器中的 6 位验证码"
                        } else {
                            "解锁密码"
                        }))
                        .child(
                            div()
                                .h(px(40.))
                                .border_1()
                                .rounded(px(10.))
                                .border_color(gpui_kit::rgb(
                                    if self
                                        .ui
                                        .settings
                                        .unlock
                                        .read(cx)
                                        .focus_handle(cx)
                                        .is_focused(window)
                                    {
                                        0x3e6b89
                                    } else {
                                        p.border
                                    },
                                ))
                                .bg(gpui_kit::rgb(
                                    if self
                                        .ui
                                        .settings
                                        .unlock
                                        .read(cx)
                                        .focus_handle(cx)
                                        .is_focused(window)
                                    {
                                        p.surface
                                    } else if self.ui.dark {
                                        p.inset
                                    } else {
                                        0xf6f8fa
                                    },
                                ))
                                .child(
                                    gpui_kit::Styled::h(
                                        Input::new(&self.ui.settings.unlock)
                                            .appearance(false)
                                            .bordered(false)
                                            .disabled(self.ui.settings.busy),
                                        px(38.),
                                    )
                                    .rounded(px(10.))
                                    .px(px(12.))
                                    .text_size(px(13.)),
                                ),
                        ),
                )
                .when(!note.is_empty(), |d| {
                    d.child(label(&note).w_full().mb(px(18.)))
                })
                .when_some(self.ui.settings.error.clone(), |d, error| {
                    d.child(
                        label(&error)
                            .w_full()
                            .mb(px(16.))
                            .text_color(gpui_kit::rgb(0xc46c68)),
                    )
                })
                .when(retry > 0, |d| {
                    d.child(
                        label(&format!("尝试次数过多，{retry} 秒后可重试"))
                            .w_full()
                            .mb(px(18.)),
                    )
                });
        }
        card = card
            .child(
                KitButton::new("unlock-confirm")
                    .primary()
                    .h(px(40.))
                    .px(px(20.))
                    .rounded(px(10.))
                    .text_size(px(13.))
                    .w_full()
                    .label("解锁")
                    .disabled(self.ui.settings.busy || (editing && retry > 0))
                    .on_click(cx.listener(move |state, _, window, cx| {
                        if editing {
                            state.submit_unlock(cx);
                        } else {
                            state.open_unlock(window, cx);
                        }
                    })),
            )
            .when(editing, |d| {
                d.child(
                    KitButton::new("unlock-cancel")
                        .ghost()
                        .w_full()
                        .h(px(36.))
                        .mt(px(10.))
                        .rounded(px(9.))
                        .text_size(px(12.))
                        .label("返回锁定界面")
                        .disabled(self.ui.settings.busy)
                        .on_click(cx.listener(|state, _, window, cx| {
                            state.ui.settings.unlock_form = None;
                            state.ui.settings.error = None;
                            state
                                .ui
                                .settings
                                .unlock
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            state.ui.settings.lock_focus.focus(window, cx);
                            cx.notify();
                        })),
                )
            })
            .when(!editing, |d| {
                d.when_some(self.ui.settings.error.clone(), |d, error| {
                    d.child(
                        label(&error)
                            .mt(px(16.))
                            .text_color(gpui_kit::rgb(0xc46c68)),
                    )
                })
            });
        div()
            .absolute()
            .inset_0()
            .p(px(24.))
            .bg(gpui_kit::rgb(if self.ui.dark {
                0x0b1015
            } else {
                0x27333c
            }))
            .occlude()
            .track_focus(&self.ui.settings.lock_focus)
            .flex()
            .items_center()
            .justify_center()
            .on_key_down(
                cx.listener(|state, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter" && state.ui.settings.unlock_form.is_none() {
                        state.open_unlock(window, cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .child(card)
            .child(div().absolute().top(px(12.)).right(px(12.)).child(
                icon_tool("security-window-close", "close", "关闭窗口").on_click(
                    cx.listener(|state, _, window, cx| state.request_window_close(window, cx)),
                ),
            ))
            .into()
    }
}

pub(super) fn close_description(count: usize, transfers: usize, unsaved: usize) -> String {
    let mut description = if count > 0 {
        format!("将关闭当前窗口，断开其中 {count} 个 SSH 会话。其他独立窗口会保持运行。")
    } else {
        "确认关闭当前窗口。其他独立窗口会保持运行。".into()
    };
    if transfers > 0 {
        description.push_str(&format!(" {transfers} 个传输任务尚未完成。"));
    }
    if unsaved > 0 {
        description.push_str(&format!(" {unsaved} 个远程文件有未保存的修改，退出会丢弃这些修改。请先返回编辑器保存，或明确选择丢弃后继续。"));
    }
    description
}
