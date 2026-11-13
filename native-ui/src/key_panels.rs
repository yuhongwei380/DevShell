//! The pinned key manager and its separate import/generate/edit/public dialogs.
use super::*;
use crate::{
    forms::string,
    workspace::{Palette, icon, icon_tool},
};
use gpui_kit::Div;

fn text(value: impl Into<gpui_kit::SharedString>, size: f32) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size * 1.8))
        .child(value.into())
}
fn button(id: impl Into<gpui_kit::ElementId>, label: &str, p: Palette) -> KitButton {
    KitButton::new(id)
        .ghost()
        .h(px(28.))
        .px(px(10.))
        .rounded(px(8.))
        .bg(gpui_kit::rgb(if crate::ui_scale::dark() {
            0x233340
        } else {
            0xedf4f8
        }))
        .text_color(gpui_kit::rgb(p.text))
        .child(text(label.to_owned(), 11.))
}

impl DevShellWindow {
    pub(super) fn original_keys_manager(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .child(
                div()
                    .w(px(650.))
                    .max_w(gpui_kit::relative(0.95))
                    .max_h(gpui_kit::relative(0.95))
                    .px(px(28.))
                    .py(px(25.))
                    .rounded(px(19.))
                    .bg(gpui_kit::rgb(p.surface))
                    .text_color(gpui_kit::rgb(p.text))
                    .shadow_lg()
                    .occlude()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .mb(px(16.))
                            .child(text("密钥管理", 17.).font_weight(gpui_kit::FontWeight(550.)))
                            .child(
                                icon_tool("keys-close", "close", "关闭密钥管理")
                                    .size(px(30.))
                                    .on_click(cx.listener(|state, _, _, cx| {
                                        state.ui.settings.keys_visible = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(15.))
                            .child(
                                text("在多台服务器间复用同一密钥", 11.)
                                    .flex_1()
                                    .text_color(gpui_kit::rgb(p.muted)),
                            )
                            .child(button("key-import", "导入私钥", p).on_click(cx.listener(
                                |state, _, window, cx| {
                                    state.open_key_editor(
                                        "import",
                                        serde_json::json!({}),
                                        window,
                                        cx,
                                    )
                                },
                            )))
                            .child(button("key-generate", "生成密钥", p).on_click(cx.listener(
                                |state, _, window, cx| {
                                    state.open_key_editor(
                                        "generate",
                                        serde_json::json!({}),
                                        window,
                                        cx,
                                    )
                                },
                            ))),
                    )
                    .child(
                        div()
                            .id("keys-scroll")
                            .min_h_0()
                            .overflow_y_scroll()
                            .when(self.snapshot.keys.is_empty(), |d| {
                                d.child(
                                    text("还没有密钥，可导入已有私钥或生成 Ed25519 密钥。", 11.)
                                        .text_center()
                                        .p(px(35.))
                                        .mb(px(20.))
                                        .text_color(gpui_kit::rgb(p.muted)),
                                )
                            })
                            .children(self.snapshot.keys.iter().map(|key| {
                                let id = string(key, "id");
                                let public = key.clone();
                                let edit = key.clone();
                                let delete = key.clone();
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(13.))
                                    .py(px(15.))
                                    .border_t_1()
                                    .border_color(gpui_kit::rgb(p.border))
                                    .child(icon("key", 22.).text_color(gpui_kit::rgb(p.muted)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.))
                                            .child(
                                                text(string(key, "name"), 13.)
                                                    .font_weight(gpui_kit::FontWeight::MEDIUM),
                                            )
                                            .child(
                                                text(
                                                    format!(
                                                        "{} · {}",
                                                        string(key, "publicKey")
                                                            .split_whitespace()
                                                            .next()
                                                            .unwrap_or("SSH"),
                                                        if key["encrypted"] == true {
                                                            if key["hasPassphrase"] == true {
                                                                "已加密 · 口令已保存"
                                                            } else {
                                                                "已加密 · 口令未保存"
                                                            }
                                                        } else {
                                                            "无口令"
                                                        }
                                                    ),
                                                    10.,
                                                )
                                                .text_color(gpui_kit::rgb(p.muted)),
                                            )
                                            .child(
                                                text(string(key, "fingerprint"), 10.)
                                                    .text_color(gpui_kit::rgb(p.muted)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(5.))
                                            .child(
                                                button(format!("key-public-{id}"), "公钥", p)
                                                    .bg(gpui_kit::rgba(0x00000000))
                                                    .on_click(cx.listener(
                                                        move |state, _, window, cx| {
                                                            state.open_native_form(
                                                                "key-public",
                                                                public.clone(),
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                            .child(
                                                button(format!("key-edit-{id}"), "编辑", p)
                                                    .bg(gpui_kit::rgba(0x00000000))
                                                    .on_click(cx.listener(
                                                        move |state, _, window, cx| {
                                                            state.open_key_editor(
                                                                "edit",
                                                                edit.clone(),
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                            .child(
                                                button(format!("key-delete-{id}"), "删除", p)
                                                    .bg(gpui_kit::rgba(0x00000000))
                                                    .text_color(gpui_kit::rgb(if self.ui.dark {
                                                        0xdf9690
                                                    } else {
                                                        0xa33d48
                                                    }))
                                                    .on_click(cx.listener(
                                                        move |state, _, window, cx| {
                                                            state.open_native_form(
                                                                "delete-key",
                                                                delete.clone(),
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            ),
                                    )
                            })),
                    ),
            )
    }
    pub(super) fn open_key_editor(
        &mut self,
        mode: &'static str,
        mut key: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        key["_mode"] = Value::from(mode);
        key["generate"] = Value::Bool(mode == "generate");
        self.open_native_form("key", key, window, cx);
        use gpui_kit::Focusable;
        let form = self.form.as_ref().unwrap();
        form.inputs["name"].update(cx, |input, cx| {
            input.set_placeholder("例如：个人服务器", window, cx)
        });
        form.inputs["sourcePath"].update(cx, |input, cx| {
            input.set_placeholder("~/.ssh/id_ed25519", window, cx)
        });
        for key in ["name", "sourcePath", "passphrase"] {
            let input = form.inputs[key].clone();
            cx.subscribe_in(
                &input,
                window,
                |state, _, event: &gpui_kit::component::input::InputEvent, _, cx| {
                    if matches!(
                        event,
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                    ) && state
                        .form
                        .as_ref()
                        .is_some_and(|f| f.kind == "key" && !f.busy)
                    {
                        state.submit_native_form(cx);
                    }
                },
            )
            .detach();
        }
        window.focus(&form.inputs["name"].read(cx).focus_handle(cx), cx);
    }
    fn key_field(&self, key: &str, label: &str, p: Palette) -> Div {
        let f = self.form.as_ref().unwrap();
        div()
            .flex()
            .flex_col()
            .gap(px(7.))
            .mb(px(12.))
            .child(text(label.to_owned(), 11.).line_height(px(15.4)))
            .child(
                gpui_kit::Styled::h(Input::new(&f.inputs[key]), px(37.))
                    .px(px(10.))
                    .text_size(px(12.))
                    .rounded(px(9.))
                    .bg(gpui_kit::rgb(if self.ui.dark {
                        0x131a21
                    } else {
                        0xf9fbfc
                    }))
                    .border_color(gpui_kit::rgb(p.border)),
            )
    }
    pub(super) fn original_key_dialog(
        &self,
        p: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let f = self.form.as_ref().unwrap();
        let public = f.kind == "key-public";
        let delete = f.kind == "delete-key";
        let mode =
            f.original["_mode"]
                .as_str()
                .unwrap_or(if !string(&f.original, "id").is_empty() {
                    "edit"
                } else if f.flag("generate") {
                    "generate"
                } else {
                    "import"
                });
        let title = if public {
            format!("{} · 公钥", string(&f.original, "name"))
        } else if delete {
            format!("删除密钥「{}」？", string(&f.original, "name"))
        } else {
            match mode {
                "generate" => "生成 Ed25519 密钥",
                "edit" => "编辑密钥",
                _ => "导入私钥",
            }
            .to_owned()
        };
        let note = match mode {
            "generate" => {
                "生成后可复制公钥，添加到服务器的 authorized_keys。填写口令可加密私钥。口令随密钥保存在本机加密配置中，所有引用此密钥的 SSH 连接自动使用。"
            }
            "edit" => {
                if f.original["encrypted"] == true {
                    "填写此私钥的正确口令即可保存；不会更改私钥文件的加密口令。口令随密钥保存在本机加密配置中，所有引用此密钥的 SSH 连接自动使用。"
                } else {
                    "此私钥没有加密，无需口令。"
                }
            }
            _ => {
                "导入后保存一份本机副本，原文件不受影响。口令随密钥保存在本机加密配置中，所有引用此密钥的 SSH 连接自动使用。"
            }
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .child(
                div()
                    .w(px(if delete { 430. } else { 540. }))
                    .max_w(window.viewport_size().width - px(30.))
                    .max_h(window.viewport_size().height - px(32.))
                    .px(px(28.))
                    .py(px(25.))
                    .rounded(px(19.))
                    .bg(gpui_kit::rgb(p.surface))
                    .text_color(gpui_kit::rgb(p.text))
                    .shadow_lg()
                    .occlude()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .mb(px(16.))
                            .child(
                                text(title, 17.)
                                    .line_height(px(24.))
                                    .font_weight(gpui_kit::FontWeight(550.)),
                            )
                            .when(!delete, |d| {
                                d.child(
                                    icon_tool("key-form-close", "close", "关闭密钥窗口")
                                        .size(px(30.))
                                        .disabled(f.busy)
                                        .on_click(cx.listener(|state, _, _, cx| {
                                            state.dismiss_native_form(cx)
                                        })),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("key-form-scroll")
                            .min_h_0()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .when(!public && !delete, |d| {
                                d.child(self.key_field("name", "名称", p))
                                    .when(mode == "import", |d| {
                                        d.child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(7.))
                                                .mb(px(12.))
                                                .child(text("私钥文件", 11.).line_height(px(15.4)))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(8.))
                                                        .child(
                                                            div().flex_1().min_w_0().child(
                                                                gpui_kit::Styled::h(
                                                                    Input::new(
                                                                        &f.inputs["sourcePath"],
                                                                    ),
                                                                    px(37.),
                                                                )
                                                                .text_size(px(12.)),
                                                            ),
                                                        )
                                                        .child(
                                                            button(
                                                                "key-choose-file",
                                                                "选择文件",
                                                                p,
                                                            )
                                                            .h(px(37.))
                                                            .on_click(cx.listener(
                                                                |state, _, window, cx| {
                                                                    state
                                                                        .choose_key_file(window, cx)
                                                                },
                                                            )),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(7.))
                                                .mb(px(12.))
                                                .child(
                                                    text("或粘贴私钥", 11.).line_height(px(15.4)),
                                                )
                                                .child(
                                                    Textarea::new(&f.areas["privateKey"])
                                                        .font_family(crate::appearance::FONTS[0].2)
                                                        .h(px(118.))
                                                        .text_size(px(12.))
                                                        .rounded(px(9.)),
                                                ),
                                        )
                                    })
                                    .when(mode != "edit" || f.original["encrypted"] == true, |d| {
                                        d.child(self.key_field(
                                            "passphrase",
                                            if mode == "generate" {
                                                "加密口令（可选）"
                                            } else {
                                                "私钥口令（已加密时填写）"
                                            },
                                            p,
                                        ))
                                    })
                                    .child(
                                        text(note, 11.)
                                            .mb(px(20.))
                                            .text_color(gpui_kit::rgb(p.muted)),
                                    )
                            })
                            .when(public, |d| {
                                d.child(
                                    text(string(&f.original, "fingerprint"), 11.)
                                        .mb(px(20.))
                                        .text_color(gpui_kit::rgb(p.muted)),
                                )
                                .child(
                                    Textarea::new(&f.areas["publicKey"])
                                        .readonly(true)
                                        .h(px(110.))
                                        .px(px(10.))
                                        .text_size(px(12.))
                                        .rounded(px(9.)),
                                )
                            })
                            .when(delete, |d| {
                                d.child(
                                    text("删除密钥库中的本机副本。导入时的原文件不受影响。", 11.)
                                        .mb(px(20.))
                                        .text_color(gpui_kit::rgb(p.muted)),
                                )
                            })
                            .when(f.error.is_some(), |d| d.child(f.error_panel())),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(12.))
                            .when(public || delete, |d| d.mt(px(22.)))
                            .when(delete, |d| {
                                d.child(button("delete-key-cancel", "取消", p).h(px(36.)).on_click(
                                    cx.listener(|state, _, _, cx| state.dismiss_native_form(cx)),
                                ))
                            })
                            .child(
                                KitButton::new("key-form-save")
                                    .primary()
                                    .h(px(36.))
                                    .px(px(18.))
                                    .rounded(px(9.))
                                    .when(!public && !delete, |b| b.w_full())
                                    .child(text(
                                        if public {
                                            "复制公钥"
                                        } else if delete {
                                            "删除"
                                        } else {
                                            "保存密钥"
                                        },
                                        12.,
                                    ))
                                    .disabled(f.busy)
                                    .on_click(cx.listener(move |state, _, _, cx| {
                                        if public {
                                            if let Some(f) = state.form.as_ref() {
                                                cx.write_to_clipboard(
                                                    gpui_kit::ClipboardItem::new_string(string(
                                                        &f.original,
                                                        "publicKey",
                                                    )),
                                                );
                                                state.feedback = Some("公钥已复制".into());
                                                cx.notify();
                                            }
                                        } else {
                                            state.submit_native_form(cx);
                                        }
                                    })),
                            ),
                    ),
            )
    }
    fn choose_key_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let serial = self.form.as_ref().unwrap().original["_nativeSerial"].clone();
        let picker = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择私钥文件".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picker.await else {
                return;
            };
            let Some(path) = paths.first().cloned() else {
                return;
            };
            let _ = this.update_in(cx, |state, window, cx| {
                if let Some(f) = state
                    .form
                    .as_ref()
                    .filter(|f| f.kind == "key" && f.original["_nativeSerial"] == serial)
                {
                    f.inputs["sourcePath"].update(cx, |input, cx| {
                        input.set_value(path.to_string_lossy().to_string(), window, cx)
                    });
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
