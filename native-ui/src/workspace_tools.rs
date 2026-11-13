use super::*;
use crate::workspace::{Palette, icon, icon_tool, overlay};
use gpui_kit::{Div, component::Sizable};

pub(super) struct UtilityDialog {
    serial: u64,
    session_id: String,
    target: String,
    kind: String,
    title: String,
    command: String,
    details: Vec<String>,
    plan: Value,
    busy: bool,
    error: Option<String>,
}

impl DevShellWindow {
    pub(super) fn quick_commands(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let connected = self
            .workspace
            .terminal
            .as_ref()
            .is_some_and(|t| t.input_tx.is_some());
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        KitButton::new("commands-all")
                            .ghost()
                            .small()
                            .label("全部")
                            .on_click(cx.listener(|state, _, _, cx| {
                                state.ui.command_group = None;
                                cx.notify();
                            })),
                    )
                    .children(
                        self.snapshot
                            .commands
                            .iter()
                            .filter_map(|command| command.get("group").and_then(Value::as_str))
                            .collect::<std::collections::BTreeSet<_>>()
                            .into_iter()
                            .map(|group| {
                                let group = group.to_owned();
                                KitButton::new(format!("command-group-{group}"))
                                    .ghost()
                                    .small()
                                    .label(group.clone())
                                    .on_click(cx.listener(move |state, _, _, cx| {
                                        state.ui.command_group = Some(group.clone());
                                        cx.notify();
                                    }))
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        KitButton::new("new-command")
                            .ghost()
                            .small()
                            .child(icon("plus", 14.))
                            .label("新建命令")
                            .on_click(cx.listener(|state, _, window, cx| {
                                state.ui.command_form = true;
                                state.ui.command_editing = None;
                                state.ui.command_deleting = false;
                                state
                                    .ui
                                    .command_name
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                state
                                    .ui
                                    .command_body
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                cx.notify();
                            })),
                    ),
            )
            .when(self.ui.command_form, |body| {
                body.child(
                    div()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(gpui_kit::rgb(p.border))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Input::new(&self.ui.command_name).small())
                        .child(Input::new(&self.ui.command_group_input).small())
                        .child(Textarea::new(&self.ui.command_body))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    KitButton::new("save-command")
                                        .primary()
                                        .small()
                                        .label("保存命令")
                                        .on_click(cx.listener(|state, _, _, cx| {
                                            state.save_quick_command(false, cx)
                                        })),
                                )
                                .when(self.ui.command_editing.is_some(), |bar| {
                                    bar.child(
                                        KitButton::new("delete-command")
                                            .danger()
                                            .small()
                                            .label(if self.ui.command_deleting {
                                                "确认删除"
                                            } else {
                                                "删除"
                                            })
                                            .on_click(cx.listener(|state, _, _, cx| {
                                                if state.ui.command_deleting {
                                                    state.save_quick_command(true, cx);
                                                } else {
                                                    state.ui.command_deleting = true;
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                })
                                .child(
                                    KitButton::new("cancel-command")
                                        .ghost()
                                        .small()
                                        .label("取消")
                                        .on_click(cx.listener(|state, _, _, cx| {
                                            state.ui.command_form = false;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
            })
            .child(
                div().flex().flex_wrap().gap_2().children(
                    self.snapshot
                        .commands
                        .iter()
                        .filter(|command| {
                            self.ui.command_group.as_ref().is_none_or(|group| {
                                command.get("group").and_then(Value::as_str) == Some(group)
                            })
                        })
                        .map(|command| {
                            let id = field(command, "id");
                            let body = field(command, "body");
                            let append = command
                                .get("appendCR")
                                .and_then(Value::as_bool)
                                .unwrap_or(true);
                            let edit = command.clone();
                            div()
                                .id(format!("quick-{id}"))
                                .rounded(px(9.))
                                .bg(gpui_kit::rgb(p.inset))
                                .flex()
                                .items_center()
                                .child(
                                    KitButton::new(format!("run-{id}"))
                                        .ghost()
                                        .small()
                                        .label(field(command, "name"))
                                        .disabled(!connected)
                                        .on_click(cx.listener(move |state, _, _, cx| {
                                            if let Some(sender) = state
                                                .workspace
                                                .terminal
                                                .as_ref()
                                                .and_then(|t| t.input_tx.as_ref())
                                            {
                                                let _ = sender.send(TerminalInput::Text(format!(
                                                    "{body}{}",
                                                    if append { "\n" } else { "" }
                                                )));
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    icon_tool(format!("edit-command-{id}"), "edit", "编辑命令")
                                        .on_click(cx.listener(move |state, _, window, cx| {
                                            state.ui.command_form = true;
                                            state.ui.command_editing = Some(field(&edit, "id"));
                                            state.ui.command_deleting = false;
                                            state.ui.command_name.update(cx, |input, cx| {
                                                input.set_value(field(&edit, "name"), window, cx)
                                            });
                                            state.ui.command_group_input.update(cx, |input, cx| {
                                                input.set_value(field(&edit, "group"), window, cx)
                                            });
                                            state.ui.command_body.update(cx, |input, cx| {
                                                input.set_value(field(&edit, "body"), window, cx)
                                            });
                                            cx.notify();
                                        })),
                                )
                        }),
                ),
            )
            .when(self.snapshot.commands.is_empty(), |body| {
                body.child(
                    div()
                        .p_6()
                        .text_size(px(12.))
                        .text_color(gpui_kit::rgb(p.muted))
                        .child("把常用命令收在这里，点击“新建命令”开始。"),
                )
            })
    }

    fn save_quick_command(&mut self, deleting: bool, cx: &mut Context<Self>) {
        let name = self.ui.command_name.read(cx).value().to_string();
        let body = self.ui.command_body.read(cx).value().to_string();
        if !deleting && (name.trim().is_empty() || body.trim().is_empty()) {
            self.feedback = Some("请填写命令名称和内容。".into());
            cx.notify();
            return;
        }
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        let id = self.ui.command_editing.clone().unwrap_or_default();
        let original = self.snapshot.commands.iter().find(|c| field(c, "id") == id);
        let command = serde_json::json!({"id":id,"name":name,"group":self.ui.command_group_input.read(cx).value().to_string(),"body":body,"color":original.map(|c|field(c,"color")).unwrap_or_else(||"blue".into()),"appendCR":original.and_then(|c|c.get("appendCR")).and_then(Value::as_bool).unwrap_or(true)});
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if deleting {
                        delete_json(&base, &token, &format!("/api/commands/{id}"))?;
                    } else {
                        post_json(&base, &token, "/api/commands", command)?;
                    }
                    Ok::<_, String>(BackendSnapshot::load(&base, &token))
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                match result {
                    Ok(snapshot) => {
                        state.snapshot = snapshot;
                        state.ui.command_form = false;
                    }
                    Err(error) => state.feedback = Some(error),
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn common_apps(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let connected = self
            .workspace
            .terminal
            .as_ref()
            .is_some_and(|t| t.input_tx.is_some());
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(gpui_kit::rgb(p.muted))
                    .child(if connected {
                        "选择工具，在当前 SSH 终端执行。"
                    } else {
                        "连接后选择应用"
                    }),
            )
            .child(
                div().flex().flex_wrap().gap_3().children(
                    [
                        ("bbr", "BBR优化", "network"),
                        ("clean", "清理垃圾", "erase"),
                        ("nq", "NQ测试脚本", "code"),
                        ("yabs", "YABS测试脚本", "cpu"),
                        ("speed", "国际测速", "network"),
                        ("swap", "开启/关闭SWAP", "disk"),
                    ]
                    .map(|(kind, label, image)| {
                        KitButton::new(format!("utility-{kind}"))
                            .secondary()
                            .small()
                            .child(icon(image, 16.))
                            .label(label)
                            .disabled(!connected)
                            .on_click(cx.listener(move |state, _, _, cx| {
                                state.open_utility(kind, label, cx)
                            }))
                    }),
                ),
            )
    }

    fn open_utility(&mut self, kind: &str, title: &str, cx: &mut Context<Self>) {
        let Some(session) = self
            .workspace
            .terminal
            .as_ref()
            .filter(|t| t.input_tx.is_some())
        else {
            return;
        };
        self.ui.utility_serial += 1;
        let command = match kind {
            "nq" => "bash -c 'bash <(curl -sL https://run.NodeQuality.com)'",
            "yabs" => "curl -sL https://yabs.sh | bash",
            "speed" => "curl -sL nws.sh | bash",
            _ => "",
        };
        self.ui.utility = Some(UtilityDialog {
            serial: self.ui.utility_serial,
            session_id: session.session_id.clone(),
            target: session.host.clone(),
            kind: kind.into(),
            title: title.into(),
            command: command.into(),
            details: if command.is_empty() {
                vec![]
            } else {
                vec!["该脚本为第三方测试脚本，请确认后执行。".into()]
            },
            plan: Value::Null,
            busy: false,
            error: None,
        });
        if kind == "clean" {
            self.plan_utility("clean", serde_json::json!({}), cx);
        }
        if kind == "swap" {
            self.plan_utility("swap-status", serde_json::json!({}), cx);
        }
        cx.notify();
    }

    fn plan_utility(&mut self, kind: &str, mut options: Value, cx: &mut Context<Self>) {
        let Some(dialog) = self.ui.utility.as_mut() else {
            return;
        };
        if dialog.busy {
            return;
        }
        dialog.busy = true;
        dialog.error = None;
        dialog.command.clear();
        let serial = dialog.serial;
        let session = dialog.session_id.clone();
        options["kind"] = Value::String(kind.into());
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_json(
                        &base,
                        &token,
                        &format!("/api/sessions/{session}/utilities/plan"),
                        options,
                    )
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                let Some(dialog) = state.ui.utility.as_mut().filter(|d| d.serial == serial) else {
                    return;
                };
                dialog.busy = false;
                match result {
                    Ok(plan) => {
                        if field(&plan, "sessionId") != dialog.session_id {
                            dialog.error = Some("执行方案与当前会话不匹配。".into());
                        } else {
                            dialog.details = plan
                                .get("details")
                                .and_then(Value::as_array)
                                .map(|a| {
                                    a.iter()
                                        .filter_map(Value::as_str)
                                        .map(ToString::to_string)
                                        .collect()
                                })
                                .unwrap_or_default();
                            if let Some(summary) = plan.get("summary").and_then(Value::as_str) {
                                dialog.details.insert(0, summary.into());
                            }
                            dialog.command = field(&plan, "command");
                            let blocked = field(&plan, "blockedReason");
                            if !blocked.is_empty() {
                                dialog.error = Some(blocked);
                            }
                            dialog.plan = plan;
                        }
                    }
                    Err(error) => dialog.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn utility_dialog(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let dialog = self.ui.utility.as_ref().unwrap();
        overlay(p,&dialog.title,icon_tool("utility-close","close","取消").on_click(cx.listener(|state,_,_,cx|{state.ui.utility=None;cx.notify();})))
            .child(div().id("utility-body").max_h(px(500.)).overflow_y_scroll().p_5().flex().flex_col().gap_3()
                .child(format!("执行到 {}",dialog.target))
                .child(div().text_size(px(11.)).text_color(gpui_kit::rgb(p.muted)).child("确认前请确保目标终端处于空闲命令提示符。"))
                .when(dialog.kind=="bbr",|body|body.child(div().flex().gap_2().children([("conservative","保守 BBR"),("balanced","平衡 BBR"),("aggressive","激进 BBR")].map(|(mode,label)|KitButton::new(format!("bbr-{mode}")).secondary().small().label(label).disabled(dialog.busy).on_click(cx.listener(move|state,_,_,cx|state.plan_utility("bbr",serde_json::json!({"mode":mode}),cx)))))))
                .when(dialog.kind=="swap" && !dialog.plan.is_null(),|body|body.child(div().flex().flex_wrap().gap_2()
                    .when(dialog.plan.get("hasSwap").and_then(Value::as_bool).unwrap_or(false),|bar|bar.child(KitButton::new("swap-remove").danger().small().label("删除 SWAP").disabled(dialog.busy).on_click(cx.listener(|state,_,_,cx|state.plan_utility("swap-remove",serde_json::json!({}),cx)))))
                    .when(!dialog.plan.get("hasSwap").and_then(Value::as_bool).unwrap_or(false) && !dialog.plan.get("rebootRequired").and_then(Value::as_bool).unwrap_or(false),|bar|bar.children([(512,"512 MB"),(1024,"1 GB"),(2048,"2 GB"),(4096,"4 GB")].map(|(size,label)|KitButton::new(format!("swap-{size}")).secondary().small().label(label).disabled(dialog.busy).on_click(cx.listener(move|state,_,_,cx|state.plan_utility("swap-create",serde_json::json!({"sizeMiB":size}),cx))))))))
                .when(dialog.busy,|body|body.child("正在检查服务器状态…"))
                .children(dialog.details.iter().map(|detail|div().text_size(px(12.)).child(detail.clone())))
                .when(dialog.kind=="speed",|body|body.child(div().text_color(gpui_kit::rgb(0xc69d80)).child("该脚本大约消耗 20～50 GB 流量，实际用量可能更高。")))
                .when(!dialog.command.is_empty(),|body|body.child(div().p_3().rounded_lg().bg(gpui_kit::rgb(p.inset)).font_family("Consolas").text_size(px(11.)).child(dialog.command.clone())))
                .when_some(dialog.error.clone(),|body,error|body.child(div().text_color(gpui_kit::rgb(0xcf827d)).child(error)))
                .when(!dialog.command.is_empty() && dialog.error.is_none(),|body|body.child(KitButton::new("utility-confirm").primary().label("确认执行").disabled(dialog.busy).on_click(cx.listener(|state,_,_,cx|{
                    let Some(dialog)=state.ui.utility.as_ref()else{return;};
                    let Some(terminal)=state.workspace.terminal.as_ref().filter(|t|t.session_id==dialog.session_id && t.input_tx.is_some())else{
                        if let Some(dialog)=state.ui.utility.as_mut(){dialog.error=Some("当前会话已切换或断开，请在目标服务器重新打开此工具。".into());}cx.notify();return;
                    };
                    if state.workspace.integration.ready&&!state.workspace.integration.at_prompt{if let Some(dialog)=state.ui.utility.as_mut(){dialog.error=Some("终端正在执行命令，请回到空闲提示符后再试。".into());}cx.notify();return;}
                    if let Some(sender)=&terminal.input_tx {let _=sender.send(TerminalInput::Text(format!("{}\n",dialog.command)));}
                    state.workspace.integration.at_prompt=false;
                    state.ui.utility=None;cx.notify();
                })))))
    }
}

fn field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}
