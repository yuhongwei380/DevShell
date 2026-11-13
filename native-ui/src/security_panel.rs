//! Layout and enablement rules from the pinned security-lock settings dialog.
use super::*;
use crate::{
    forms::string,
    workspace::{Palette, icon, icon_tool},
};
use gpui_kit::{
    Div,
    component::{checkbox::Checkbox, select::Select},
};

fn text(value: impl Into<gpui_kit::SharedString>, size: f32) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size * 1.5))
        .child(value.into())
}
fn section(p: Palette) -> Div {
    div()
        .p(px(16.))
        .border_1()
        .border_color(gpui_kit::rgb(p.border))
        .rounded(px(12.))
        .flex()
        .flex_col()
}
pub(super) fn security_draft(value: &Value) -> Value {
    let mut draft = value.clone();
    draft["passwordEnabled"] =
        Value::Bool(draft["passwordConfigured"] == true || draft["twoFactorConfigured"] != true);
    draft["twoFactorEnabled"] = Value::Bool(draft["twoFactorConfigured"] == true);
    let automatic = draft["idleSeconds"].as_u64().unwrap_or(0) > 0;
    draft["lockMode"] = Value::from(if automatic { "idle" } else { "manual" });
    if !automatic {
        draft["idleSeconds"] = Value::from(300);
    }
    draft
}
impl DevShellWindow {
    fn security_switch(
        &self,
        checked: bool,
        disabled: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> gpui_kit::base::Switch {
        let update = cx.listener(|state, value: &bool, _, cx| {
            state
                .form
                .as_mut()
                .unwrap()
                .flags
                .insert("enabled".into(), *value);
            cx.notify();
        });
        gpui_kit::base::Switch::new("security-enabled")
            .checked(checked)
            .disabled(disabled)
            .accessibility_label("启用安全锁定")
            .w(px(38.))
            .h(px(22.))
            .flex_shrink_0()
            .rounded_full()
            .border_1()
            .border_color(gpui_kit::rgb(if checked { 0x56829e } else { p.border }))
            .bg(gpui_kit::rgb(if checked { 0x56829e } else { p.border }))
            .relative()
            .child(
                div()
                    .absolute()
                    .left(px(if checked { 18. } else { 2. }))
                    .top(px(2.))
                    .size(px(16.))
                    .rounded_full()
                    .bg(gpui_kit::rgb(0xffffff))
                    .shadow_sm(),
            )
            .on_change(move |value, _, window, cx| update(&value, window, cx))
    }
    fn security_input(&self, key: &str, label: &str, enabled: bool, p: Palette) -> Div {
        let f = self.form.as_ref().unwrap();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(text(label.to_owned(), 12.).text_color(gpui_kit::rgb(p.muted)))
            .child(
                div()
                    .relative()
                    .child(
                        gpui_kit::Styled::h(Input::new(&f.inputs[key]).disabled(!enabled), px(40.))
                            .px(px(12.))
                            .text_size(px(13.))
                            .rounded(px(10.))
                            .border_color(gpui_kit::rgb(p.border))
                            .bg(gpui_kit::rgb(if self.ui.dark {
                                0x131a21
                            } else {
                                0xf6f8fa
                            }))
                            .when(!enabled, |input| input.opacity(0.5)),
                    )
                    .when(key == "idleSeconds", |d| {
                        d.child(
                            text("秒", 12.)
                                .absolute()
                                .right(px(13.))
                                .top(px(11.))
                                .text_color(gpui_kit::rgb(p.muted)),
                        )
                    }),
            )
    }
    fn security_select(&self, key: &str, label: &str, enabled: bool, p: Palette) -> Div {
        let f = self.form.as_ref().unwrap();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(text(label.to_owned(), 12.).text_color(gpui_kit::rgb(p.muted)))
            .child(
                Select::new(&f.choices[key])
                    .disabled(!enabled)
                    .h(px(40.))
                    .px(px(12.))
                    .text_size(px(13.))
                    .rounded(px(10.))
                    .bg(gpui_kit::rgb(if self.ui.dark {
                        0x131a21
                    } else {
                        0xf6f8fa
                    })),
            )
    }
    fn security_method(
        &self,
        key: &'static str,
        title: &str,
        note: &str,
        image: &str,
        enabled: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let f = self.form.as_ref().unwrap();
        div()
            .flex()
            .items_center()
            .gap(px(11.))
            .child(
                div()
                    .size(px(34.))
                    .rounded(px(10.))
                    .bg(gpui_kit::rgb(if self.ui.dark {
                        0x131a21
                    } else {
                        0xf6f8fa
                    }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(image, 18.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(text(title.to_owned(), 13.).font_weight(gpui_kit::FontWeight(550.)))
                    .child(
                        text(note.to_owned(), 11.)
                            .line_height(px(17.6))
                            .mt(px(3.))
                            .text_color(gpui_kit::rgb(p.muted)),
                    ),
            )
            .child(
                Checkbox::new(format!("security-{key}"))
                    .checked(f.flag(key))
                    .disabled(!enabled)
                    .accessibility_label(title.to_owned())
                    .on_click(cx.listener(move |state, value: &bool, _, cx| {
                        if let Some(f) = state.form.as_mut() {
                            f.flags.insert(key.into(), *value);
                        }
                        cx.notify();
                    })),
            )
    }

    pub(super) fn original_security_dialog(
        &self,
        p: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let f = self.form.as_ref().unwrap();
        let granted = f.original["_grant"].as_str().is_some_and(|s| !s.is_empty());
        let verifying = self.snapshot.security["enabled"] == true && !granted;
        let enabled = granted && f.flag("enabled") && !f.busy;
        let manual = f.selected("lockMode", cx) == "manual";
        let inset = if self.ui.dark { 0x131a21 } else { 0xf6f8fa };
        let mut body = div()
            .id("security-settings-body")
            .min_h_0()
            .overflow_y_scroll()
            .px(px(22.))
            .pb(px(18.))
            .flex()
            .flex_col();
        if verifying {
            let authorization = section(p).mb(px(4.)).bg(gpui_kit::rgb(inset))
                .child(text("验证后修改", 13.).font_weight(gpui_kit::FontWeight(550.)))
                .child(text("修改或关闭安全锁定前，请验证当前密码或验证码。", 11.)
                    .mt(px(12.)).mb(px(14.)).text_color(gpui_kit::rgb(p.muted)))
                .child(div().flex().gap(px(14.))
                    .child(self.security_select("proofMethod", "验证方式", !f.busy, p))
                    .child(self.security_input("authValue", "密码或验证码", !f.busy, p)))
                .child(KitButton::new("security-authorize").secondary().h(px(36.))
                    .px(px(13.)).rounded(px(9.)).mt(px(12.)).self_start()
                    .child(text("验证并编辑", 12.)).disabled(f.busy)
                    .on_click(cx.listener(|state, _, _, cx| {
                        let f = state.form.as_ref().unwrap();
                        let proof = serde_json::json!({"method":f.selected("proofMethod",cx),"value":f.text("authValue",cx)});
                        state.security_call("authorize", proof, cx);
                    })))
                .when(f.error.is_some(), |d| d.child(f.error_panel()));
            body = body.child(authorization);
        } else {
            let enable = div()
                .px(px(16.))
                .py(px(14.))
                .mb(px(16.))
                .border_1()
                .border_color(gpui_kit::rgb(p.border))
                .rounded(px(12.))
                .bg(gpui_kit::rgb(inset))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .child(
                    div()
                        .flex_1()
                        .child(text("启用安全锁定", 13.).font_weight(gpui_kit::FontWeight(550.)))
                        .child(
                            text("保护所有窗口，重新启动后也需要解锁", 11.)
                                .line_height(px(17.6))
                                .mt(px(3.))
                                .text_color(gpui_kit::rgb(p.muted)),
                        ),
                )
                .child(self.security_switch(f.flag("enabled"), !granted || f.busy, p, cx));
            let password = section(p)
                .child(self.security_method(
                    "passwordEnabled",
                    "密码解锁",
                    "使用至少 4 位密码验证身份",
                    "key",
                    enabled,
                    p,
                    cx,
                ))
                .when(f.flag("passwordEnabled"), |d| {
                    d.child(
                        div()
                            .mt(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(14.))
                                    .child(self.security_input("password", "设置密码", enabled, p))
                                    .child(self.security_input(
                                        "repeatPassword",
                                        "确认密码",
                                        enabled,
                                        p,
                                    )),
                            )
                            .child(self.security_input(
                                "passwordHint",
                                "密码提醒  可选",
                                enabled,
                                p,
                            )),
                    )
                });
            let mut totp = section(p).child(self.security_method(
                "twoFactorEnabled",
                "2FA 验证码解锁",
                "使用验证器 App 中的动态验证码",
                "shield",
                enabled,
                p,
                cx,
            ));
            if f.flag("twoFactorEnabled") {
                let enrolled = f.original["twoFactorConfigured"] == true;
                let status = if f.original.get("_enrollment").is_some() {
                    "等待验证码确认，保存后生效"
                } else if enrolled {
                    "已绑定验证器"
                } else {
                    "尚未绑定验证器"
                };
                let mut binding = div().mt(px(16.)).flex().flex_col().gap(px(16.)).child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(text(status, 12.).text_color(gpui_kit::rgb(p.muted)))
                        .child(
                            KitButton::new("security-enroll")
                                .secondary()
                                .h(px(36.))
                                .px(px(13.))
                                .rounded(px(9.))
                                .child(text(
                                    if enrolled {
                                        "重新绑定验证器"
                                    } else {
                                        "扫码绑定验证器"
                                    },
                                    12.,
                                ))
                                .disabled(!enabled)
                                .on_click(cx.listener(|state, _, _, cx| {
                                    let grant =
                                        state.form.as_ref().unwrap().original["_grant"].clone();
                                    state.security_call(
                                        "enroll",
                                        serde_json::json!({"grant":grant}),
                                        cx,
                                    );
                                })),
                        ),
                );
                if let Some(image) = f.image.clone() {
                    let enrollment = div()
                        .border_t_1()
                        .border_color(gpui_kit::rgb(p.border))
                        .pt(px(16.))
                        .flex()
                        .items_center()
                        .gap(px(18.))
                        .child(
                            div()
                                .w(px(190.))
                                .flex_shrink_0()
                                .p(px(8.))
                                .border_1()
                                .border_color(gpui_kit::rgb(p.border))
                                .rounded(px(12.))
                                .bg(gpui_kit::rgb(0xffffff))
                                .child(gpui_kit::img(image).size(px(172.))),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    text("扫码绑定验证器", 13.)
                                        .font_weight(gpui_kit::FontWeight(550.)),
                                )
                                .child(
                                    text(
                                        "用验证器 App 扫描左侧二维码，再填写 6 位验证码并保存。",
                                        11.,
                                    )
                                    .mt(px(8.))
                                    .mb(px(12.))
                                    .text_color(gpui_kit::rgb(p.muted)),
                                )
                                .child(self.security_input("code", "绑定验证码", enabled, p))
                                .child(
                                    text("二维码仅在本机生成。", 11.)
                                        .mt(px(10.))
                                        .text_color(gpui_kit::rgb(p.muted)),
                                ),
                        );
                    binding = binding.child(enrollment).child(
                        div()
                            .child(text("无法扫码？手动输入密钥", 12.))
                            .child(
                                text(string(&f.original["_enrollment"], "secret"), 11.)
                                    .font_family("Consolas"),
                            )
                            .child(
                                text("请妥善保管验证器或此密钥。", 11.)
                                    .text_color(gpui_kit::rgb(p.muted)),
                            ),
                    );
                }
                totp = totp.child(binding);
            }
            let timing = section(p)
                .child(
                    text("锁定时间", 13.)
                        .font_weight(gpui_kit::FontWeight(550.))
                        .mb(px(14.)),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(14.))
                        .child(self.security_select("lockMode", "锁定方式", enabled, p))
                        .child(self.security_input(
                            "idleSeconds",
                            "无操作时长",
                            enabled && !manual,
                            p,
                        )),
                )
                .child(
                    text(
                        if manual {
                            "点击明暗按钮右侧的锁，即可立即锁定。"
                        } else {
                            "无鼠标或键盘操作达到设定时长后锁定，也可随时手动上锁。"
                        },
                        11.,
                    )
                    .line_height(px(18.7))
                    .mt(px(12.))
                    .text_color(gpui_kit::rgb(p.muted)),
                );
            body = body
                .child(enable)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .when(!enabled, |d| d.opacity(0.55))
                        .child(password)
                        .child(totp)
                        .child(timing),
                )
                .child(
                    text(
                        "锁定后 SSH 连接继续保持。启用两种解锁方式时，任选一种即可解锁。",
                        11.,
                    )
                    .line_height(px(18.7))
                    .mt(px(14.))
                    .mx(px(2.))
                    .text_color(gpui_kit::rgb(p.muted)),
                );
        }
        let header = div()
            .p(px(22.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .size(px(44.))
                    .rounded(px(14.))
                    .border_1()
                    .border_color(gpui_kit::rgb(p.border))
                    .bg(gpui_kit::rgb(inset))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("lock", 22.)),
            )
            .child(
                div()
                    .flex_1()
                    .child(
                        text("安全锁定", 19.)
                            .line_height(px(26.))
                            .font_weight(gpui_kit::FontWeight(550.)),
                    )
                    .child(
                        text("离开电脑时，保护你的工作台", 12.)
                            .mt(px(5.))
                            .text_color(gpui_kit::rgb(p.muted)),
                    ),
            )
            .child(
                icon_tool("security-settings-close", "close", "关闭安全锁定设置")
                    .size(px(30.))
                    .self_start()
                    .disabled(f.busy)
                    .on_click(cx.listener(|state, _, _, cx| state.dismiss_native_form(cx))),
            );
        let footer = div()
            .flex_shrink_0()
            .px(px(22.))
            .py(px(14.))
            .border_t_1()
            .border_color(gpui_kit::rgb(p.border))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .when(f.error.is_some(), |d| d.child(f.error_panel())),
            )
            .child(
                KitButton::new("security-save")
                    .primary()
                    .h(px(40.))
                    .min_w(px(108.))
                    .px(px(20.))
                    .rounded(px(10.))
                    .child(text("保存设置", 13.).font_weight(gpui_kit::FontWeight(550.)))
                    .disabled(!granted || f.busy)
                    .on_click(cx.listener(|state, _, _, cx| state.save_security_settings(cx))),
            );
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(600.))
                    .max_w(window.viewport_size().width - px(32.))
                    .max_h(window.viewport_size().height - px(32.))
                    .rounded(px(20.))
                    .border_1()
                    .border_color(gpui_kit::rgb(p.border))
                    .bg(gpui_kit::rgb(p.surface))
                    .text_color(gpui_kit::rgb(p.text))
                    .shadow_lg()
                    .occlude()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(body)
                    .when(!verifying, |d| d.child(footer)),
            )
    }
}
