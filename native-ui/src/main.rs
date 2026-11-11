use std::{
    env, fs,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    process,
    sync::mpsc,
    thread,
    time::Duration,
};

use base64::Engine;
use ely_gpui_component::{
    Assets as ElyAssets,
    buttons::Button as ElyButton,
    primitives::FocusScope,
    typography::{Heading, Paragraph},
};
use gpui_kit::{
    App, AppContext, Context, Disableable, Entity, FocusHandle, IntoElement, ParentElement, Render,
    Styled, Window, WindowOptions,
    component::{
        button::{Button as KitButton, ButtonVariants},
        input::{Input, InputState},
    },
    div, px,
};
use serde_json::Value;

#[derive(Clone, Debug)]
struct BackendSnapshot {
    base: String,
    version: String,
    build: String,
    servers: Vec<ServerSummary>,
    groups: usize,
    locked: bool,
    error: Option<String>,
    config_dir: Option<String>,
    config_format: Option<String>,
    security_summary: Option<String>,
}

#[derive(Clone, Debug)]
struct ServerSummary {
    id: String,
    name: String,
    host: String,
    port: u16,
    user: String,
}

#[derive(Clone, Debug)]
struct PendingHostKey {
    profile_id: String,
    challenge: HostKeyChallenge,
}

#[derive(Clone, Debug)]
struct HostKeyChallenge {
    host: String,
    fingerprint: String,
    previous_fingerprint: String,
    algorithm: String,
}

#[derive(Debug)]
enum SessionConnectError {
    HostKey(HostKeyChallenge),
    Message(String),
}

impl From<String> for SessionConnectError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

struct DevShellWindow {
    focus: FocusHandle,
    snapshot: BackendSnapshot,
    token: String,
    settings_visible: bool,
    update_status: Option<String>,
    update_checking: bool,
    profile_form_open: bool,
    profile_saving: bool,
    pending_profile_delete: Option<String>,
    profile_editing_id: Option<String>,
    connecting: Option<String>,
    feedback: Option<String>,
    terminal: Option<TerminalState>,
    command_input: Entity<InputState>,
    file_name_input: Entity<InputState>,
    local_path_input: Entity<InputState>,
    secret_input: Entity<InputState>,
    profile_name_input: Entity<InputState>,
    profile_host_input: Entity<InputState>,
    profile_user_input: Entity<InputState>,
    profile_port_input: Entity<InputState>,
    pending_host_key: Option<PendingHostKey>,
    files: Option<FileListState>,
    monitor: Option<MonitorState>,
    pending_delete: Option<String>,
    pending_rename: Option<String>,
    preview: Option<FilePreviewState>,
    transfers: Vec<TransferState>,
}

#[derive(Clone, Debug)]
struct TerminalState {
    session_id: String,
    output: String,
    status: String,
    input_tx: Option<mpsc::Sender<String>>,
}

#[derive(Clone, Debug)]
struct FileListState {
    path: String,
    entries: Vec<RemoteEntry>,
    status: String,
}

#[derive(Clone, Debug)]
struct RemoteEntry {
    name: String,
    kind: String,
    bytes: i64,
    owner: String,
}

#[derive(Clone, Debug)]
struct FilePreviewState {
    path: String,
    encoding: String,
    text: String,
    status: String,
}

#[derive(Clone, Debug)]
struct TransferState {
    id: String,
    name: String,
    status: String,
    done: i64,
    total: i64,
    error: Option<String>,
    retryable: bool,
}

#[derive(Clone, Debug)]
struct MonitorState {
    cpu: f64,
    memory_used: u64,
    memory_total: u64,
    load: String,
    sampled_at: String,
    status: String,
}

enum TerminalEvent {
    Output(String),
    Info(String),
    Closed(String),
}

impl Render for DevShellWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.snapshot.clone();
        let focus = self.focus.clone();
        let view = cx.entity();
        let pending_host_key = self.pending_host_key.clone();
        let pending_profile = pending_host_key
            .as_ref()
            .map(|pending| pending.profile_id.as_str());
        let status = if snapshot.locked {
            "配置已锁定"
        } else if snapshot.error.is_some() {
            "后端连接异常"
        } else {
            "后端已连接"
        };
        let status_detail = snapshot
            .error
            .clone()
            .unwrap_or_else(|| format!("{} · 构建 {}", snapshot.version, snapshot.build));

        FocusScope::new(&focus)
            .root()
            .size_full()
            .bg(gpui_kit::rgb(0x101318))
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .text_color(gpui_kit::rgb(0xd7dce5))
                    .child(
                        div()
                            .h(px(64.))
                            .px_6()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(gpui_kit::rgb(0x2b313b))
                            .child(Heading::h1("DevShell"))
                            .child(
                                KitButton::new("settings")
                                    .ghost()
                                    .label("设置")
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _, cx| {
                                            view.update(cx, |state, cx| {
                                                state.settings_visible = !state.settings_visible;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .child(
                                div()
                                    .w(px(248.))
                                    .p_4()
                                    .border_r_1()
                                    .border_color(gpui_kit::rgb(0x2b313b))
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(Paragraph::new("工作区"))
                                    .child(sidebar_button("服务器", true))
                                    .child(sidebar_button("终端", false))
                                    .child(sidebar_button("文件", false))
                                    .child(sidebar_button("监控", false))
                                    .child(
                                        KitButton::new("sidebar-settings")
                                            .ghost()
                                            .label("设置")
                                            .on_click({
                                                let view = view.clone();
                                                move |_, _, cx| {
                                                    view.update(cx, |state, cx| {
                                                        state.settings_visible =
                                                            !state.settings_visible;
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .p_8()
                                    .flex()
                                    .flex_col()
                                    .gap_6()
                                    .child(Paragraph::new("原生工作区"))
                                    .child(Input::new(&self.secret_input).mask_toggle())
                                    .when(self.settings_visible, |workspace| {
                                        workspace.child(settings_panel(
                                            snapshot.clone(),
                                            view.clone(),
                                            self.update_status.clone(),
                                            self.update_checking,
                                        ))
                                    })
                                    .when_some(pending_host_key, |workspace, pending| {
                                        let detail = if pending.challenge.previous_fingerprint.is_empty() {
                                            format!(
                                                "首次连接 {}，请核实主机指纹后再次点击对应服务器。",
                                                pending.challenge.host
                                            )
                                        } else {
                                            format!(
                                                "{} 的主机指纹已变化，请核实新指纹后再次点击对应服务器。",
                                                pending.challenge.host
                                            )
                                        };
                                        workspace.child(
                                            div()
                                                .p_3()
                                                .rounded_md()
                                                .bg(gpui_kit::rgb(0x2b2116))
                                                .border_1()
                                                .border_color(gpui_kit::rgb(0x8a5a22))
                                                .flex()
                                                .flex_col()
                                                .gap_1()
                                                .child(Paragraph::new(detail))
                                                .child(Paragraph::new(format!(
                                                    "算法：{} · 指纹：{}",
                                                    pending.challenge.algorithm,
                                                    pending.challenge.fingerprint
                                                )))
                                                .when(!pending.challenge.previous_fingerprint.is_empty(), |card| {
                                                    card.child(Paragraph::new(format!(
                                                        "旧指纹：{}",
                                                        pending.challenge.previous_fingerprint
                                                    )))
                                                }),
                                        )
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .gap_4()
                                            .child(stat_card("服务器", snapshot.servers.len()))
                                            .child(stat_card("分组", snapshot.groups)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .child(Paragraph::new("最近配置的服务器"))
                                                    .child(
                                                        KitButton::new("new-server")
                                                            .secondary()
                                                            .label(if self.profile_form_open {
                                                                "收起新增"
                                                            } else {
                                                                "新增服务器"
                                                            })
                                                            .on_click({
                                                                let view = view.clone();
                                                                move |_, window, cx| {
                                                                    view.update(cx, |state, cx| {
                                                                        let opening = !state.profile_form_open;
                                                                        state.profile_form_open = opening;
                                                                        if opening {
                                                                            state.profile_editing_id = None;
                                                                            state.profile_name_input.update(cx, |input, cx| {
                                                                                input.set_value("", window, cx)
                                                                            });
                                                                            state.profile_host_input.update(cx, |input, cx| {
                                                                                input.set_value("", window, cx)
                                                                            });
                                                                            state.profile_user_input.update(cx, |input, cx| {
                                                                                input.set_value("", window, cx)
                                                                            });
                                                                            state.profile_port_input.update(cx, |input, cx| {
                                                                                input.set_value("22", window, cx)
                                                                            });
                                                                        }
                                                                        cx.notify();
                                                                    });
                                                                }
                                                            }),
                                                    ),
                                            )
                                            .when(self.profile_form_open, |list| {
                                                list.child(profile_form(
                                                    view.clone(),
                                                    self.profile_name_input.clone(),
                                                    self.profile_host_input.clone(),
                                                    self.profile_user_input.clone(),
                                                    self.profile_port_input.clone(),
                                                    self.profile_editing_id.clone(),
                                                    self.profile_saving,
                                                ))
                                            })
                                            .children(
                                                snapshot.servers.iter().take(8).cloned().map(
                                                    |server| {
                                                        server_row(
                                                            view.clone(),
                                                            server,
                                                            self.connecting.as_deref(),
                                                            pending_profile,
                                                            self.pending_profile_delete.as_deref(),
                                                        )
                                                    },
                                                ),
                                            ),
                                    )
                                    .when_some(self.terminal.clone(), |workspace, terminal| {
                                        workspace.child(terminal_panel(
                                            terminal,
                                            self.command_input.clone(),
                                            view.clone(),
                                        ))
                                    })
                                    .when_some(self.files.clone(), |workspace, files| {
                                        workspace.child(file_panel(
                                            files,
                                            view.clone(),
                                            self.file_name_input.clone(),
                                            self.local_path_input.clone(),
                                            self.pending_delete.clone(),
                                            self.pending_rename.clone(),
                                        ))
                                    })
                                    .when_some(self.monitor.clone(), |workspace, monitor| {
                                        workspace.child(monitor_panel(monitor))
                                    })
                                    .when_some(self.preview.clone(), |workspace, preview| {
                                        workspace.child(file_preview_panel(preview))
                                    })
                                    .when(!self.transfers.is_empty(), |workspace| {
                                        workspace.child(transfer_panel(
                                            self.transfers.clone(),
                                            view.clone(),
                                        ))
                                    })
                                    .child(
                                        div()
                                            .p_5()
                                            .rounded_lg()
                                            .bg(gpui_kit::rgb(0x191f27))
                                            .border_1()
                                            .border_color(gpui_kit::rgb(0x2b313b))
                                            .flex()
                                            .flex_col()
                                            .gap_3()
                                            .child(Paragraph::new(status))
                                            .child(Paragraph::new(status_detail))
                                            .when_some(self.feedback.clone(), |card, message| {
                                                card.child(Paragraph::new(message))
                                            })
                                            .child(Paragraph::new(format!(
                                                "服务地址：{}",
                                                snapshot.base
                                            )))
                                            .child(ElyButton::new("refresh", "刷新后端状态").on_click({
                                                let view = view.clone();
                                                move |_, _, cx| {
                                                    let view = view.clone();
                                                    view.update(cx, |state, cx| {
                                                        let base = state.snapshot.base.clone();
                                                        let token = state.token.clone();
                                                        state.feedback = Some("正在刷新后端状态…".into());
                                                        cx.notify();
                                                        cx.spawn(async move |this, cx| {
                                                            let snapshot = cx
                                                                .background_executor()
                                                                .spawn(async move {
                                                                    BackendSnapshot::load(&base, &token)
                                                                })
                                                                .await;
                                                            let _ = this.update(cx, |state, cx| {
                                                                state.snapshot = snapshot;
                                                                state.feedback = Some("后端状态已刷新。".into());
                                                                cx.notify();
                                                            });
                                                        })
                                                        .detach();
                                                    });
                                                }
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h(px(32.))
                            .px_6()
                            .flex()
                            .items_center()
                            .border_t_1()
                            .border_color(gpui_kit::rgb(0x2b313b))
                            .child(Paragraph::new("GPUI Kit + ELY GPUI · WebView 已移除")),
                    ),
            )
    }
}

fn sidebar_button(label: &'static str, selected: bool) -> impl IntoElement {
    let button = KitButton::new(label).label(label);
    if selected {
        button.primary()
    } else {
        button.ghost()
    }
}

fn stat_card(label: &'static str, value: usize) -> impl IntoElement {
    div()
        .w(px(180.))
        .p_5()
        .rounded_lg()
        .bg(gpui_kit::rgb(0x191f27))
        .border_1()
        .border_color(gpui_kit::rgb(0x2b313b))
        .flex()
        .flex_col()
        .gap_2()
        .child(Paragraph::new(label))
        .child(Heading::h2(value.to_string()))
}

fn settings_panel(
    snapshot: BackendSnapshot,
    view: Entity<DevShellWindow>,
    update_status: Option<String>,
    update_checking: bool,
) -> impl IntoElement {
    let config_dir = snapshot
        .config_dir
        .unwrap_or_else(|| "当前锁定或不可读取".into());
    let config_format = snapshot
        .config_format
        .unwrap_or_else(|| "当前锁定或不可读取".into());
    let security = snapshot
        .security_summary
        .unwrap_or_else(|| "当前锁定或不可读取".into());
    let update_label = if update_checking {
        "正在检查更新…"
    } else {
        "检查更新"
    };
    div()
        .p_5()
        .rounded_lg()
        .bg(gpui_kit::rgb(0x191f27))
        .border_1()
        .border_color(gpui_kit::rgb(0x2b313b))
        .flex()
        .flex_col()
        .gap_2()
        .child(Heading::h2("原生设置概览"))
        .child(Paragraph::new(format!(
            "版本：{} · 构建：{}",
            snapshot.version, snapshot.build
        )))
        .child(Paragraph::new(format!("配置目录：{config_dir}")))
        .child(Paragraph::new(format!("存储格式：{config_format}")))
        .child(Paragraph::new(format!("安全锁定：{security}")))
        .when_some(update_status, |card, status| {
            card.child(Paragraph::new(format!("更新状态：{status}")))
        })
        .child(Paragraph::new(
            "配置、密钥和安全锁定仍由 Go 本地 API 管理，原生窗口不复制写入逻辑。",
        ))
        .child(
            KitButton::new("check-updates")
                .secondary()
                .label(update_label)
                .disabled(update_checking)
                .on_click({
                    let view = view.clone();
                    move |_, _, cx| {
                        let view = view.clone();
                        view.update(cx, |state, cx| {
                            let base = state.snapshot.base.clone();
                            let token = state.token.clone();
                            state.update_checking = true;
                            state.feedback = Some("正在检查 release 更新…".into());
                            cx.notify();
                            cx.spawn(async move |this, cx| {
                                let result = cx
                                    .background_executor()
                                    .spawn(async move { check_updates(&base, &token) })
                                    .await;
                                let _ = this.update(cx, |state, cx| {
                                    state.update_checking = false;
                                    state.update_status = Some(match result {
                                        Ok(status) => status,
                                        Err(error) => format!("检查失败：{error}"),
                                    });
                                    state.feedback = Some("更新检查完成。".into());
                                    cx.notify();
                                });
                            })
                            .detach();
                        });
                    }
                }),
        )
        .child(
            KitButton::new("close-settings")
                .secondary()
                .label("收起设置")
                .on_click(move |_, _, cx| {
                    view.update(cx, |state, cx| {
                        state.settings_visible = false;
                        cx.notify();
                    });
                }),
        )
}

fn profile_form(
    view: Entity<DevShellWindow>,
    name_input: Entity<InputState>,
    host_input: Entity<InputState>,
    user_input: Entity<InputState>,
    port_input: Entity<InputState>,
    editing_id: Option<String>,
    saving: bool,
) -> impl IntoElement {
    let editing = editing_id.is_some();
    div()
        .p_4()
        .rounded_md()
        .bg(gpui_kit::rgb(0x171c23))
        .border_1()
        .border_color(gpui_kit::rgb(0x2b313b))
        .flex()
        .flex_col()
        .gap_2()
        .child(Paragraph::new(if editing {
            "编辑 SSH 服务器"
        } else {
            "新增 SSH 服务器"
        }))
        .child(Input::new(&name_input))
        .child(Input::new(&host_input))
        .child(Input::new(&port_input))
        .child(Input::new(&user_input))
        .child(Paragraph::new(
            "密码沿用上方的 SSH 凭据输入；留空则保存后连接时再输入。",
        ))
        .child(
            KitButton::new("save-server")
                .secondary()
                .label(if saving {
                    "保存中…"
                } else if editing {
                    "保存修改"
                } else {
                    "保存服务器"
                })
                .disabled(saving)
                .on_click(move |_, _, cx| {
                    view.update(cx, |state, cx| {
                        let name = state
                            .profile_name_input
                            .read(cx)
                            .value(cx)
                            .trim()
                            .to_string();
                        let host = state
                            .profile_host_input
                            .read(cx)
                            .value(cx)
                            .trim()
                            .to_string();
                        let user = state
                            .profile_user_input
                            .read(cx)
                            .value(cx)
                            .trim()
                            .to_string();
                        let port_text = state
                            .profile_port_input
                            .read(cx)
                            .value(cx)
                            .trim()
                            .to_string();
                        let secret = state.secret_input.read(cx).value(cx).to_string();
                        if name.is_empty() || host.is_empty() || user.is_empty() {
                            state.feedback = Some("请填写服务器名称、地址和用户名。".into());
                            cx.notify();
                            return;
                        }
                        let port = match port_text.parse::<u16>() {
                            Ok(port) if port > 0 => port,
                            _ => {
                                state.feedback = Some("端口必须是 1–65535 的数字。".into());
                                cx.notify();
                                return;
                            }
                        };
                        let base = state.snapshot.base.clone();
                        let token = state.token.clone();
                        state.profile_saving = true;
                        state.feedback = Some("正在保存服务器配置…".into());
                        cx.notify();
                        cx.spawn(async move |this, cx| {
                            let result = cx
                                .background_executor()
                                .spawn(async move {
                                    save_profile_and_reload(
                                        &base,
                                        &token,
                                        editing_id.as_deref(),
                                        &name,
                                        &host,
                                        port,
                                        &user,
                                        &secret,
                                    )
                                })
                                .await;
                            let _ = this.update(cx, |state, cx| {
                                state.profile_saving = false;
                                match result {
                                    Ok(snapshot) => {
                                        state.snapshot = snapshot;
                                        state.profile_form_open = false;
                                        state.profile_editing_id = None;
                                        state.feedback = Some("服务器配置已保存。".into());
                                    }
                                    Err(error) => state.feedback = Some(error),
                                }
                                cx.notify();
                            });
                        })
                        .detach();
                    });
                }),
        )
}

fn terminal_panel(
    terminal: TerminalState,
    command_input: Entity<InputState>,
    view: Entity<DevShellWindow>,
) -> impl IntoElement {
    let output = if terminal.output.is_empty() {
        "等待远端终端输出…".to_string()
    } else {
        terminal.output
    };
    div()
        .p_4()
        .rounded_lg()
        .bg(gpui_kit::rgb(0x080b0f))
        .border_1()
        .border_color(gpui_kit::rgb(0x2b313b))
        .flex()
        .flex_col()
        .gap_2()
        .child({
            let session_id = terminal.session_id.clone();
            let view = view.clone();
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(Paragraph::new(format!(
                    "终端 · {} · {}",
                    terminal.session_id, terminal.status
                )))
                .child(
                    KitButton::new("terminal-disconnect")
                        .danger()
                        .label("断开")
                        .on_click(move |_, _, cx| {
                            disconnect_session(view.clone(), session_id.clone(), cx);
                        }),
                )
        })
        .child(Paragraph::new(output))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Input::new(&command_input))
                .child(
                    KitButton::new("terminal-send")
                        .primary()
                        .label("发送")
                        .on_click(move |_, _, cx| {
                            let text = command_input.read(cx).value(cx).to_string();
                            if text.is_empty() {
                                return;
                            }
                            view.update(cx, |state, _| {
                                if let Some(sender) = state
                                    .terminal
                                    .as_ref()
                                    .and_then(|terminal| terminal.input_tx.clone())
                                {
                                    let _ = sender.send(format!("{text}\n"));
                                }
                            });
                        }),
                ),
        );
}

fn file_panel(
    files: FileListState,
    view: Entity<DevShellWindow>,
    file_name_input: Entity<InputState>,
    local_path_input: Entity<InputState>,
    pending_delete: Option<String>,
    pending_rename: Option<String>,
) -> impl IntoElement {
    let current_path = files.path.clone();
    let parent = parent_remote_path(&current_path);
    div()
        .p_4()
        .rounded_lg()
        .bg(gpui_kit::rgb(0x191f27))
        .border_1()
        .border_color(gpui_kit::rgb(0x2b313b))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(Paragraph::new(format!(
                    "文件 · {} · {}",
                    files.path, files.status
                )))
                .when_some(parent, |bar, parent| {
                    let view = view.clone();
                    bar.child(
                        KitButton::new("file-parent")
                            .ghost()
                            .label("上一级")
                            .on_click(move |_, _, cx| {
                                navigate_files(view.clone(), parent.clone(), cx)
                            }),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Input::new(&file_name_input))
                .child({
                    let view = view.clone();
                    let file_name_input = file_name_input.clone();
                    let path = current_path.clone();
                    KitButton::new("file-mkdir")
                        .secondary()
                        .label("新建目录")
                        .on_click(move |_, _, cx| {
                            let name = file_name_input.read(cx).value(cx).to_string();
                            if !name.trim().is_empty() {
                                create_directory(view.clone(), path.clone(), name, cx);
                            }
                        })
                })
                .child({
                    let view = view.clone();
                    let file_name_input = file_name_input.clone();
                    let path = current_path.clone();
                    KitButton::new("file-new-file")
                        .secondary()
                        .label("新建文件")
                        .on_click(move |_, _, cx| {
                            let name = file_name_input.read(cx).value(cx).to_string();
                            if !name.trim().is_empty() {
                                create_empty_file(view.clone(), path.clone(), name, cx);
                            }
                        })
                })
                .child(Input::new(&local_path_input))
                .child({
                    let view = view.clone();
                    let local_path_input = local_path_input.clone();
                    let path = current_path.clone();
                    KitButton::new("file-upload")
                        .secondary()
                        .label("上传")
                        .on_click(move |_, _, cx| {
                            let local_path = local_path_input.read(cx).value(cx).to_string();
                            if !local_path.trim().is_empty() {
                                upload_local(view.clone(), path.clone(), local_path, cx);
                            }
                        })
                }),
        )
        .children(files.entries.into_iter().take(12).map(|entry| {
            let is_folder = entry.kind == "folder";
            let name = entry.name.clone();
            let target = join_remote_path(&current_path, &name);
            let view = view.clone();
            let is_pending = pending_delete.as_deref() == Some(target.as_str());
            let is_rename_pending = pending_rename.as_deref() == Some(target.as_str());
            let delete_view = view.clone();
            let delete_target = target.clone();
            let delete_parent = current_path.clone();
            let rename_view = view.clone();
            let rename_target = target.clone();
            let rename_parent = current_path.clone();
            let download_view = view.clone();
            let download_target = target.clone();
            let download_name = name.clone();
            let preview_view = view.clone();
            let preview_target = target.clone();
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(if is_folder {
                    KitButton::new(format!("file-folder-{}", name))
                        .ghost()
                        .label(format!("▸ {}", name))
                        .on_click(move |_, _, cx| navigate_files(view.clone(), target.clone(), cx))
                } else {
                    KitButton::new(format!("file-entry-{}", name))
                        .text()
                        .label(format!("· {} · {} bytes", name, entry.bytes))
                        .on_click(move |_, _, cx| {
                            preview_remote(preview_view.clone(), preview_target.clone(), cx);
                        })
                })
                .child(Paragraph::new(entry.owner))
                .child(
                    KitButton::new(format!("file-rename-{}", name))
                        .secondary()
                        .label(if is_rename_pending {
                            "确认重命名"
                        } else {
                            "重命名"
                        })
                        .on_click({
                            let file_name_input = file_name_input.clone();
                            move |_, _, cx| {
                                let new_name = file_name_input.read(cx).value(cx).to_string();
                                if is_rename_pending && !new_name.trim().is_empty() {
                                    rename_remote(
                                        rename_view.clone(),
                                        rename_target.clone(),
                                        rename_parent.clone(),
                                        new_name,
                                        cx,
                                    );
                                } else {
                                    let _ = rename_view.update(cx, |state, cx| {
                                        state.pending_rename = Some(rename_target.clone());
                                        state.feedback =
                                            Some("输入新名称后再次点击“确认重命名”".into());
                                        cx.notify();
                                    });
                                }
                            }
                        }),
                )
                .when(!is_folder, |row| {
                    row.child(
                        KitButton::new(format!("file-download-{}", name))
                            .secondary()
                            .label("下载")
                            .on_click(move |_, _, cx| {
                                download_remote(
                                    download_view.clone(),
                                    download_target.clone(),
                                    download_name.clone(),
                                    cx,
                                );
                            }),
                    )
                })
                .child(
                    KitButton::new(format!("file-delete-{}", name))
                        .danger()
                        .label(if is_pending { "确认删除" } else { "删除" })
                        .on_click(move |_, _, cx| {
                            if is_pending {
                                delete_remote(
                                    delete_view.clone(),
                                    delete_target.clone(),
                                    delete_parent.clone(),
                                    cx,
                                );
                            } else {
                                let _ = delete_view.update(cx, |state, cx| {
                                    state.pending_delete = Some(delete_target.clone());
                                    state.feedback = Some("再次点击“确认删除”以继续".into());
                                    cx.notify();
                                });
                            }
                        }),
                )
        }))
}

fn monitor_panel(monitor: MonitorState) -> impl IntoElement {
    let memory_percent = if monitor.memory_total == 0 {
        0.0
    } else {
        monitor.memory_used as f64 / monitor.memory_total as f64 * 100.0
    };
    div()
        .p_4()
        .rounded_lg()
        .bg(gpui_kit::rgb(0x191f27))
        .border_1()
        .border_color(gpui_kit::rgb(0x2b313b))
        .flex()
        .flex_col()
        .gap_2()
        .child(Paragraph::new(format!("监控 · {}", monitor.status)))
        .child(Paragraph::new(format!(
            "CPU {:.1}% · 内存 {:.1}% · 负载 {}",
            monitor.cpu, memory_percent, monitor.load
        )))
        .child(Paragraph::new(format!(
            "内存 {} / {} MB · 采样 {}",
            monitor.memory_used / 1024 / 1024,
            monitor.memory_total / 1024 / 1024,
            monitor.sampled_at
        )));
}

fn file_preview_panel(preview: FilePreviewState) -> impl IntoElement {
    let text = if preview.text.chars().count() > 12000 {
        preview.text.chars().take(12000).collect()
    } else {
        preview.text
    };
    div()
        .p_4()
        .rounded_lg()
        .bg(gpui_kit::rgb(0x080b0f))
        .border_1()
        .border_color(gpui_kit::rgb(0x2b313b))
        .flex()
        .flex_col()
        .gap_2()
        .child(Paragraph::new(format!(
            "预览 · {} · 编码 {} · {}",
            preview.path, preview.encoding, preview.status
        )))
        .child(Paragraph::new(text));
}

fn transfer_panel(transfers: Vec<TransferState>, view: Entity<DevShellWindow>) -> impl IntoElement {
    div()
        .p_4()
        .rounded_lg()
        .bg(gpui_kit::rgb(0x191f27))
        .border_1()
        .border_color(gpui_kit::rgb(0x2b313b))
        .flex()
        .flex_col()
        .gap_2()
        .child(Paragraph::new("传输队列"))
        .children(transfers.into_iter().map(|transfer| {
            let transfer_id = transfer.id.clone();
            let retryable = transfer.retryable;
            let active = matches!(transfer.status.as_str(), "queued" | "uploading");
            let action_view = view.clone();
            let progress = if transfer.total > 0 {
                format!(
                    "{:.0}%",
                    transfer.done as f64 / transfer.total as f64 * 100.0
                )
            } else {
                "-".into()
            };
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(Paragraph::new(format!(
                    "{} · {} · {}",
                    transfer.name, transfer.status, progress
                )))
                .child(Paragraph::new(transfer.error.unwrap_or_else(|| {
                    format!("{}/{}", transfer.done, transfer.total)
                })))
                .child(if retryable {
                    KitButton::new(format!("transfer-retry-{}", transfer_id))
                        .secondary()
                        .label("重试")
                        .on_click(move |_, _, cx| {
                            retry_transfer(action_view.clone(), transfer_id.clone(), cx);
                        })
                } else if active {
                    KitButton::new(format!("transfer-cancel-{}", transfer_id))
                        .danger()
                        .label("取消")
                        .on_click(move |_, _, cx| {
                            cancel_transfer(action_view.clone(), transfer_id.clone(), cx);
                        })
                } else {
                    KitButton::new(format!("transfer-done-{}", transfer_id))
                        .text()
                        .label("")
                })
        }))
}

fn navigate_files(view: Entity<DevShellWindow>, path: String, cx: &mut App) {
    let _ = view.update(cx, |state, cx| {
        let Some(session_id) = state
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some(format!("正在读取远端目录 {}…", path));
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_files(&base, &token, &session_id, &path) })
                .await;
            let _ = view.update(cx, |state, cx| {
                state.files = Some(match result {
                    Ok(files) => files,
                    Err(error) => FileListState {
                        path,
                        entries: Vec::new(),
                        status: error,
                    },
                });
                cx.notify();
            });
        })
        .detach();
    });
}

fn preview_remote(view: Entity<DevShellWindow>, path: String, cx: &mut App) {
    let _ = view.update(cx, |state, cx| {
        let Some(session_id) = state
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some(format!("正在读取文件 {}…", path));
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_preview(&base, &token, &session_id, &path) })
                .await;
            let _ = view.update(cx, |state, cx| {
                match result {
                    Ok(preview) => {
                        state.preview = Some(preview);
                        state.feedback = Some("文件已加载".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
}

fn create_directory(view: Entity<DevShellWindow>, path: String, name: String, cx: &mut App) {
    let _ = view.update(cx, |state, cx| {
        let Some(session_id) = state
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some(format!("正在创建目录 {}…", name));
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_file_action(&base, &token, &session_id, "mkdir", &path, &name)?;
                    load_files(&base, &token, &session_id, &path)
                })
                .await;
            let _ = view.update(cx, |state, cx| {
                match result {
                    Ok(files) => {
                        state.files = Some(files);
                        state.feedback = Some("目录已创建".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
}

fn create_empty_file(view: Entity<DevShellWindow>, path: String, name: String, cx: &mut App) {
    let _ = view.update(cx, |state, cx| {
        let Some(session_id) = state
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some(format!("正在创建文件 {}…", name));
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_file_action(&base, &token, &session_id, "new-file", &path, &name)?;
                    load_files(&base, &token, &session_id, &path)
                })
                .await;
            let _ = view.update(cx, |state, cx| {
                match result {
                    Ok(files) => {
                        state.files = Some(files);
                        state.feedback = Some("文件已创建".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
}

fn upload_local(view: Entity<DevShellWindow>, directory: String, local_path: String, cx: &mut App) {
    let _ = view.update(cx, |state, cx| {
        let Some(session_id) = state
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some(format!("正在上传 {}…", local_path));
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_upload_local(&base, &token, &session_id, &directory, &local_path)
                })
                .await;
            let _ = view.update(cx, |state, cx| {
                state.feedback = Some(match result {
                    Ok(message) => message,
                    Err(error) => error,
                });
                cx.notify();
            });
        })
        .detach();
    });
}

fn retry_transfer(view: Entity<DevShellWindow>, transfer_id: String, cx: &mut App) {
    transfer_action(view, transfer_id, "POST", "retry", cx);
}

fn cancel_transfer(view: Entity<DevShellWindow>, transfer_id: String, cx: &mut App) {
    transfer_action(view, transfer_id, "DELETE", "cancel", cx);
}

fn transfer_action(
    view: Entity<DevShellWindow>,
    transfer_id: String,
    method: &str,
    action: &str,
    cx: &mut App,
) {
    let method = method.to_string();
    let action = action.to_string();
    let _ = view.update(cx, |state, cx| {
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some(format!(
            "正在{}传输…",
            if action == "retry" {
                "重试"
            } else {
                "取消"
            }
        ));
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { request_transfer_action(&base, &token, &transfer_id, &method) })
                .await;
            let _ = view.update(cx, |state, cx| {
                state.feedback = Some(match result {
                    Ok(()) => "传输操作已提交".into(),
                    Err(error) => error,
                });
                cx.notify();
            });
        })
        .detach();
    });
}

fn delete_remote(view: Entity<DevShellWindow>, target: String, parent: String, cx: &mut App) {
    let _ = view.update(cx, |state, cx| {
        let Some(session_id) = state
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.pending_delete = None;
        state.feedback = Some(format!("正在删除 {}…", target));
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_file_action(&base, &token, &session_id, "delete", &target, "")?;
                    load_files(&base, &token, &session_id, &parent)
                })
                .await;
            let _ = view.update(cx, |state, cx| {
                match result {
                    Ok(files) => {
                        state.files = Some(files);
                        state.feedback = Some("已删除".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
}

fn rename_remote(
    view: Entity<DevShellWindow>,
    target: String,
    parent: String,
    name: String,
    cx: &mut App,
) {
    let _ = view.update(cx, |state, cx| {
        let Some(session_id) = state
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.pending_rename = None;
        state.feedback = Some(format!("正在重命名 {}…", target));
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_file_action(&base, &token, &session_id, "rename", &target, &name)?;
                    load_files(&base, &token, &session_id, &parent)
                })
                .await;
            let _ = view.update(cx, |state, cx| {
                match result {
                    Ok(files) => {
                        state.files = Some(files);
                        state.feedback = Some("已重命名".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
}

fn download_remote(view: Entity<DevShellWindow>, target: String, name: String, cx: &mut App) {
    let _ = view.update(cx, |state, cx| {
        let Some(session_id) = state
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some(format!("正在下载 {}…", name));
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { download_file(&base, &token, &session_id, &target, &name) })
                .await;
            let _ = view.update(cx, |state, cx| {
                state.feedback = Some(match result {
                    Ok(path) => format!("已下载到 {}", path.display()),
                    Err(error) => error,
                });
                cx.notify();
            });
        })
        .detach();
    });
}

fn disconnect_session(view: Entity<DevShellWindow>, session_id: String, cx: &mut App) {
    let _ = view.update(cx, |state, cx| {
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some("正在断开会话…".into());
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { delete_session(&base, &token, &session_id) })
                .await;
            let _ = view.update(cx, |state, cx| {
                match result {
                    Ok(()) => {
                        state.terminal = None;
                        state.files = None;
                        state.monitor = None;
                        state.preview = None;
                        state.transfers.clear();
                        state.feedback = Some("会话已断开".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
}

fn join_remote_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn parent_remote_path(path: &str) -> Option<String> {
    if path == "/" {
        return None;
    }
    let parent = path
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("/");
    Some(if parent.is_empty() {
        "/".into()
    } else {
        parent.into()
    })
}

fn server_row(
    view: Entity<DevShellWindow>,
    server: ServerSummary,
    connecting: Option<&str>,
    pending_profile: Option<&str>,
    pending_delete: Option<&str>,
) -> impl IntoElement {
    let server_id = server.id.clone();
    let is_connecting = connecting == Some(server.id.as_str());
    let is_host_key_pending = pending_profile == Some(server.id.as_str());
    let is_delete_pending = pending_delete == Some(server.id.as_str());
    let delete_profile_id = server.id.clone();
    let delete_view = view.clone();
    let edit_server = server.clone();
    let edit_view = view.clone();
    let label = if is_connecting {
        "连接中…"
    } else if is_host_key_pending {
        "确认指纹并连接"
    } else {
        "连接"
    };
    div()
        .p_3()
        .rounded_md()
        .bg(gpui_kit::rgb(0x191f27))
        .border_1()
        .border_color(gpui_kit::rgb(0x2b313b))
        .flex()
        .items_center()
        .justify_between()
        .child(Paragraph::new(server.name))
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(Paragraph::new(format!("{}:{}", server.host, server.port)))
                .child(
                    KitButton::new(server_id.clone())
                        .secondary()
                        .label(label)
                        .disabled(is_connecting)
                        .on_click(move |_, _, cx| {
                            let view = view.clone();
                            let profile_id = server_id.clone();
                            view.update(cx, |state, cx| {
                                let base = state.snapshot.base.clone();
                                let token = state.token.clone();
                                let secret = state.secret_input.read(cx).value(cx).to_string();
                                let approval = state
                                    .pending_host_key
                                    .as_ref()
                                    .filter(|pending| pending.profile_id == profile_id)
                                    .map(|pending| pending.challenge.clone());
                                state.connecting = Some(profile_id.clone());
                                state.feedback = Some("正在连接服务器…".into());
                                cx.notify();
                                let request_profile_id = profile_id.clone();
                                let request_approval = approval.clone();
                                cx.spawn(async move |this, cx| {
                                    let result = cx
                                        .background_executor()
                                        .spawn(
                                            async move {
                                                post_session(
                                                    &base,
                                                    &token,
                                                    &request_profile_id,
                                                    &secret,
                                                    request_approval.as_ref(),
                                                )
                                            },
                                        )
                                        .await;
                                    let _ = this.update(cx, |state, cx| {
                                        state.connecting = None;
                                        match result {
                                            Ok(session_id) => {
                                                state.pending_host_key = None;
                                                state.feedback = Some(format!(
                                                    "连接已建立，会话 {}；正在打开终端。",
                                                    session_id
                                                ));
                                                let (sender, receiver) = mpsc::channel();
                                                let (input_tx, input_rx) = mpsc::channel();
                                                let base = state.snapshot.base.clone();
                                                let token = state.token.clone();
                                                let terminal_id = session_id.clone();
                                                thread::spawn(move || {
                                                    let event = match terminal_reader(
                                                        &base,
                                                        &token,
                                                        &terminal_id,
                                                        &sender,
                                                        input_rx,
                                                    ) {
                                                        Ok(()) => TerminalEvent::Closed(
                                                            "终端连接已关闭".into(),
                                                        ),
                                                        Err(error) => TerminalEvent::Closed(
                                                            format!("终端错误：{error}"),
                                                        ),
                                                    };
                                                    let _ = sender.send(event);
                                                });
                                                state.terminal = Some(TerminalState {
                                                    session_id: session_id.clone(),
                                                    output: String::new(),
                                                    status: "连接中".into(),
                                                    input_tx: Some(input_tx),
                                                });
                                                let view = cx.entity();
                                                cx.spawn(async move |_, cx| {
                                                    let mut closed = false;
                                                    while !closed {
                                                        while let Ok(event) = receiver.try_recv() {
                                                            let _ = view.update(cx, |state, cx| {
                                                                if let Some(terminal) =
                                                                    state.terminal.as_mut()
                                                                {
                                                                    match event {
                                                                        TerminalEvent::Output(text) => {
                                                                            terminal.output.push_str(&text);
                                                                            let count = terminal.output.chars().count();
                                                                            if count > 12000 {
                                                                                terminal.output = terminal
                                                                                    .output
                                                                                    .chars()
                                                                                    .skip(count - 12000)
                                                                                    .collect();
                                                                            }
                                                                        }
                                                                        TerminalEvent::Info(message) => {
                                                                            terminal.status = message;
                                                                        }
                                                                        TerminalEvent::Closed(message) => {
                                                                            terminal.status = message;
                                                                            closed = true;
                                                                        }
                                                                    }
                                                                }
                                                                cx.notify();
                                                            });
                                                        }
                                                        if !closed {
                                                            cx.background_executor()
                                                                .timer(Duration::from_millis(50))
                                                                .await;
                                                        }
                                                    }
                                                })
                                                .detach();
                                                let files_base = state.snapshot.base.clone();
                                                let files_token = state.token.clone();
                                                let files_session = session_id.clone();
                                                let files_view = cx.entity();
                                                cx.spawn(async move |_, cx| {
                                                    let result = cx
                                                        .background_executor()
                                                        .spawn(async move {
                                                            load_files(
                                                                &files_base,
                                                                &files_token,
                                                                &files_session,
                                                                "/",
                                                            )
                                                        })
                                                        .await;
                                                    let _ = files_view.update(cx, |state, cx| {
                                                        state.files = Some(match result {
                                                            Ok(files) => files,
                                                            Err(error) => FileListState {
                                                                path: "/".into(),
                                                                entries: Vec::new(),
                                                                status: error,
                                                            },
                                                        });
                                                        cx.notify();
                                                    });
                                                })
                                                .detach();
                                                let monitor_base = state.snapshot.base.clone();
                                                let monitor_token = state.token.clone();
                                                let transfer_session = session_id.clone();
                                                let monitor_session = session_id;
                                                let monitor_view = cx.entity();
                                                cx.spawn(async move |_, cx| {
                                                    loop {
                                                        let result = cx
                                                            .background_executor()
                                                            .spawn({
                                                                let monitor_base = monitor_base.clone();
                                                                let monitor_token = monitor_token.clone();
                                                                let monitor_session = monitor_session.clone();
                                                                async move {
                                                                    load_monitor(
                                                                        &monitor_base,
                                                                        &monitor_token,
                                                                        &monitor_session,
                                                                    )
                                                                }
                                                            })
                                                            .await;
                                                        if monitor_view
                                                            .update(cx, |state, cx| {
                                                                state.monitor = Some(match result {
                                                                    Ok(monitor) => monitor,
                                                                    Err(error) => MonitorState {
                                                                        cpu: 0.0,
                                                                        memory_used: 0,
                                                                        memory_total: 0,
                                                                        load: "未知".into(),
                                                                        sampled_at: "-".into(),
                                                                        status: error,
                                                                    },
                                                                });
                                                                cx.notify();
                                                            })
                                                            .is_err()
                                                        {
                                                            break;
                                                        }
                                                        cx.background_executor()
                                                            .timer(Duration::from_secs(5))
                                                            .await;
                                                    }
                                                })
                                                .detach();
                                                let transfer_base = state.snapshot.base.clone();
                                                let transfer_token = state.token.clone();
                                                let transfer_view = cx.entity();
                                                cx.spawn(async move |_, cx| {
                                                    loop {
                                                        let result = cx
                                                            .background_executor()
                                                            .spawn({
                                                                let transfer_base = transfer_base.clone();
                                                                let transfer_token = transfer_token.clone();
                                                                let transfer_session = transfer_session.clone();
                                                                async move {
                                                                    load_transfers(
                                                                        &transfer_base,
                                                                        &transfer_token,
                                                                        &transfer_session,
                                                                    )
                                                                }
                                                            })
                                                            .await;
                                                        if transfer_view
                                                            .update(cx, |state, cx| {
                                                                if let Ok(transfers) = result {
                                                                    state.transfers = transfers;
                                                                }
                                                                cx.notify();
                                                            })
                                                            .is_err()
                                                        {
                                                            break;
                                                        }
                                                        cx.background_executor()
                                                            .timer(Duration::from_secs(2))
                                                            .await;
                                                    }
                                                })
                                                .detach();
                                            }
                                            Err(SessionConnectError::HostKey(challenge)) => {
                                                state.pending_host_key = Some(PendingHostKey {
                                                    profile_id: profile_id.clone(),
                                                    challenge,
                                                });
                                                state.feedback = Some(
                                                    "请核实服务器主机指纹，再次点击同一服务器按钮确认。"
                                                        .into(),
                                                );
                                            }
                                            Err(SessionConnectError::Message(error)) => {
                                                state.feedback = Some(error)
                                            }
                                        }
                                        cx.notify();
                                    });
                                })
                                .detach();
                            });
                        }),
                )
                .child(
                    KitButton::new(format!("edit-{delete_profile_id}"))
                        .ghost()
                        .label("编辑")
                        .on_click(move |_, window, cx| {
                            let server = edit_server.clone();
                            edit_view.update(cx, |state, cx| {
                                state.profile_form_open = true;
                                state.profile_editing_id = Some(server.id.clone());
                                state.profile_name_input.update(cx, |input, cx| {
                                    input.set_value(server.name.clone(), window, cx)
                                });
                                state.profile_host_input.update(cx, |input, cx| {
                                    input.set_value(server.host.clone(), window, cx)
                                });
                                state.profile_user_input.update(cx, |input, cx| {
                                    input.set_value(server.user.clone(), window, cx)
                                });
                                state.profile_port_input.update(cx, |input, cx| {
                                    input.set_value(server.port.to_string(), window, cx)
                                });
                                state.feedback = Some("已载入服务器配置，可修改后保存。".into());
                                cx.notify();
                            });
                        }),
                )
                .child(
                    KitButton::new(format!("delete-{delete_profile_id}"))
                        .ghost()
                        .label(if is_delete_pending { "确认删除" } else { "删除" })
                        .on_click({
                            let view = delete_view;
                            move |_, _, cx| {
                                let profile_id = delete_profile_id.clone();
                                view.update(cx, |state, cx| {
                                    if is_delete_pending {
                                        let base = state.snapshot.base.clone();
                                        let token = state.token.clone();
                                        state.feedback = Some("正在回收服务器配置…".into());
                                        cx.notify();
                                        cx.spawn(async move |this, cx| {
                                            let result = cx
                                                .background_executor()
                                                .spawn(async move {
                                                    delete_profile_and_reload(
                                                        &base, &token, &profile_id,
                                                    )
                                                })
                                                .await;
                                            let _ = this.update(cx, |state, cx| {
                                                match result {
                                                    Ok(snapshot) => {
                                                        state.snapshot = snapshot;
                                                        state.pending_profile_delete = None;
                                                        state.feedback =
                                                            Some("服务器配置已移入回收站。".into());
                                                    }
                                                    Err(error) => state.feedback = Some(error),
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .detach();
                                    } else {
                                        state.pending_profile_delete = Some(profile_id);
                                        state.feedback =
                                            Some("再次点击删除按钮确认移入回收站。".into());
                                        cx.notify();
                                    }
                                });
                            }
                        }),
                ),
        )
}

fn main() {
    let args = Arguments::parse().unwrap_or_else(|error| {
        eprintln!("DevShell native UI: {error}");
        process::exit(2);
    });
    let snapshot = BackendSnapshot::load(&args.base, &args.token);

    gpui_kit::application()
        .with_assets(ElyAssets::before(gpui_kit::assets::Assets))
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            ely_gpui_component::init(cx).expect("Ely failed to start");
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    window.focus(&focus, cx);
                    let command_input = cx
                        .new(|cx| InputState::new(window, cx).placeholder("终端输入，Enter 发送"));
                    let file_name_input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("目录名称"));
                    let secret_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder("SSH 密码或私钥口令")
                            .masked(true)
                    });
                    let profile_name_input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("服务器名称"));
                    let profile_host_input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("服务器地址"));
                    let profile_user_input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("用户名"));
                    let profile_port_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value("22")
                            .placeholder("端口")
                    });
                    DevShellWindow {
                        focus,
                        snapshot,
                        token: args.token.clone(),
                        settings_visible: false,
                        update_status: None,
                        update_checking: false,
                        profile_form_open: false,
                        profile_saving: false,
                        pending_profile_delete: None,
                        profile_editing_id: None,
                        connecting: None,
                        feedback: None,
                        terminal: None,
                        command_input,
                        file_name_input,
                        local_path_input: cx
                            .new(|cx| InputState::new(window, cx).placeholder("本地文件路径")),
                        secret_input,
                        profile_name_input,
                        profile_host_input,
                        profile_user_input,
                        profile_port_input,
                        pending_host_key: None,
                        files: None,
                        monitor: None,
                        pending_delete: None,
                        pending_rename: None,
                        preview: None,
                        transfers: Vec::new(),
                    }
                })
            })
            .expect("failed to open DevShell native window");
        });
}

struct Arguments {
    base: String,
    token: String,
}

impl Arguments {
    fn parse() -> Result<Self, String> {
        let mut base = None;
        let mut token = None;
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--base" => base = Some(args.next().ok_or("--base 缺少地址")?),
                "--token" => token = Some(args.next().ok_or("--token 缺少凭据")?),
                "--help" | "-h" => {
                    println!("用法：DevShellUI --base http://127.0.0.1:端口 --token 本地凭据");
                    process::exit(0);
                }
                other => return Err(format!("未知参数：{other}")),
            }
        }
        let base = base.ok_or("缺少 --base")?;
        let token = token.ok_or("缺少 --token")?;
        if !base.starts_with("http://127.0.0.1:") && !base.starts_with("http://[::1]:") {
            return Err("--base 只允许本机回环地址".into());
        }
        Ok(Self { base, token })
    }
}

impl BackendSnapshot {
    fn load(base: &str, token: &str) -> Self {
        let mut snapshot = Self {
            base: base.to_string(),
            version: "未知版本".into(),
            build: "未知构建".into(),
            servers: Vec::new(),
            groups: 0,
            locked: false,
            error: None,
            config_dir: None,
            config_format: None,
            security_summary: None,
        };
        match get_json(base, token, "/boot.js") {
            Ok(value) => {
                snapshot.version = value
                    .get("displayVersion")
                    .or_else(|| value.get("version"))
                    .and_then(Value::as_str)
                    .unwrap_or("未知版本")
                    .to_string();
                snapshot.build = value
                    .get("build")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "未知构建".into())
                    .trim_matches('"')
                    .to_string();
            }
            Err(error) => snapshot.error = Some(error),
        }
        match get_json(base, token, "/api/config") {
            Ok(value) => {
                snapshot.servers = value
                    .get("servers")
                    .and_then(Value::as_array)
                    .map(|servers| {
                        servers
                            .iter()
                            .filter_map(|server| {
                                Some(ServerSummary {
                                    id: server.get("id")?.as_str()?.to_string(),
                                    name: server.get("name")?.as_str()?.to_string(),
                                    host: server.get("host")?.as_str()?.to_string(),
                                    port: server.get("port").and_then(Value::as_u64).unwrap_or(22)
                                        as u16,
                                    user: server
                                        .get("user")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                snapshot.groups = value
                    .get("groupNodes")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
            }
            Err(error) if error.contains("423") => snapshot.locked = true,
            Err(error) if snapshot.error.is_none() => snapshot.error = Some(error),
            Err(_) => {}
        }
        if let Ok(value) = get_json(base, token, "/api/config/storage") {
            snapshot.config_dir = value
                .get("configDir")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            snapshot.config_format = value
                .get("format")
                .and_then(Value::as_str)
                .map(ToString::to_string);
        }
        if let Ok(value) = get_json(base, token, "/api/security-lock/status") {
            let enabled = value
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let locked = value
                .get("locked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let password = value
                .get("passwordConfigured")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let two_factor = value
                .get("twoFactorConfigured")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            snapshot.security_summary = Some(if !enabled {
                "未启用".into()
            } else if locked {
                format!(
                    "已锁定 · 密码：{} · 验证器：{}",
                    if password { "已配置" } else { "未配置" },
                    if two_factor { "已配置" } else { "未配置" }
                )
            } else {
                format!(
                    "已启用 · 当前已解锁 · 密码：{} · 验证器：{}",
                    if password { "已配置" } else { "未配置" },
                    if two_factor { "已配置" } else { "未配置" }
                )
            });
        }
        snapshot
    }
}

fn post_session(
    base: &str,
    token: &str,
    profile_id: &str,
    secret: &str,
    approval: Option<&HostKeyChallenge>,
) -> Result<String, SessionConnectError> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream = TcpStream::connect(authority).map_err(|error| format!("连接失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(25)))
        .map_err(|error| format!("设置超时失败：{error}"))?;
    let host_key_approval = approval.map(|challenge| {
        serde_json::json!({
            "host": challenge.host.clone(),
            "fingerprint": challenge.fingerprint.clone(),
            "previousFingerprint": challenge.previous_fingerprint.clone(),
        })
    });
    let body = serde_json::json!({
        "profileId": profile_id,
        "secret": secret,
        "hostKeyApproval": host_key_approval,
    })
    .to_string();
    let request = format!(
        "POST /api/sessions HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送连接请求失败：{error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取连接响应失败：{error}"))?;
    let text = String::from_utf8_lossy(&response);
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "后端响应格式无效".to_string())?;
    let status = headers.lines().next().unwrap_or_default();
    let value: Value =
        serde_json::from_str(body.trim()).map_err(|error| format!("解析连接响应失败：{error}"))?;
    if !status.contains(" 200 ") {
        if let Some(host_key) = value.get("hostKey") {
            let fingerprint = host_key
                .get("fingerprint")
                .and_then(Value::as_str)
                .unwrap_or("未知")
                .to_string();
            return Err(SessionConnectError::HostKey(HostKeyChallenge {
                host: host_key
                    .get("host")
                    .and_then(Value::as_str)
                    .unwrap_or("未知主机")
                    .to_string(),
                fingerprint,
                previous_fingerprint: host_key
                    .get("previousFingerprint")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                algorithm: host_key
                    .get("algorithm")
                    .and_then(Value::as_str)
                    .unwrap_or("未知算法")
                    .to_string(),
            }));
        }
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or(status)
            .to_string()
            .into());
    }
    value
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| "连接成功但后端没有返回会话 ID".to_string().into())
}

fn load_files(
    base: &str,
    token: &str,
    session_id: &str,
    path: &str,
) -> Result<FileListState, String> {
    let value = get_json(
        base,
        token,
        &format!(
            "/api/sessions/{session_id}/files?path={}",
            query_escape(path)
        ),
    )?;
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    Some(RemoteEntry {
                        name: entry.get("name")?.as_str()?.to_string(),
                        kind: entry.get("kind")?.as_str()?.to_string(),
                        bytes: entry.get("bytes").and_then(Value::as_i64).unwrap_or(0),
                        owner: entry
                            .get("owner")
                            .and_then(Value::as_str)
                            .unwrap_or("未知属主")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(FileListState {
        path: value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(path)
            .to_string(),
        entries,
        status: "已加载".into(),
    })
}

fn load_preview(
    base: &str,
    token: &str,
    session_id: &str,
    path: &str,
) -> Result<FilePreviewState, String> {
    let value = get_json(
        base,
        token,
        &format!(
            "/api/sessions/{session_id}/file-content?path={}&encoding=auto",
            query_escape(path)
        ),
    )?;
    Ok(FilePreviewState {
        path: value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(path)
            .to_string(),
        encoding: value
            .get("encoding")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        text: value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        status: "只读预览".into(),
    })
}

fn load_monitor(base: &str, token: &str, session_id: &str) -> Result<MonitorState, String> {
    let value = get_json(base, token, &format!("/api/sessions/{session_id}/stats"))?;
    Ok(MonitorState {
        cpu: value.get("cpu").and_then(Value::as_f64).unwrap_or(0.0),
        memory_used: value.get("memoryUsed").and_then(Value::as_u64).unwrap_or(0),
        memory_total: value
            .get("memoryTotal")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        load: value
            .get("load")
            .and_then(Value::as_str)
            .unwrap_or("未知")
            .to_string(),
        sampled_at: value
            .get("sampledAt")
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_string(),
        status: if value
            .get("sampleReady")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "已采样".into()
        } else {
            "等待采样".into()
        },
    })
}

fn load_transfers(base: &str, token: &str, session_id: &str) -> Result<Vec<TransferState>, String> {
    let value = get_json(base, token, "/api/transfers")?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|transfer| {
            transfer
                .get("sessionId")
                .and_then(Value::as_str)
                .map(|value| value == session_id)
                .unwrap_or(false)
        })
        .map(|transfer| TransferState {
            id: transfer
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            name: transfer
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("传输")
                .to_string(),
            status: transfer
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("未知")
                .to_string(),
            done: transfer.get("done").and_then(Value::as_i64).unwrap_or(0),
            total: transfer.get("total").and_then(Value::as_i64).unwrap_or(0),
            error: transfer
                .get("error")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            retryable: transfer
                .get("retryable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect())
}

fn request_transfer_action(
    base: &str,
    token: &str,
    transfer_id: &str,
    method: &str,
) -> Result<(), String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream =
        TcpStream::connect(authority).map_err(|error| format!("连接传输服务失败：{error}"))?;
    let request = format!(
        "{method} /api/transfers/{transfer_id}{} HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nConnection: close\r\n\r\n",
        if method == "POST" { "/retry" } else { "" }
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送传输操作失败：{error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取传输操作响应失败：{error}"))?;
    let text = String::from_utf8_lossy(&response);
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "传输操作响应格式无效".to_string())?;
    if !headers.lines().next().unwrap_or_default().contains(" 200 ") {
        let value: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("传输操作失败")
            .to_string());
    }
    Ok(())
}

fn post_file_action(
    base: &str,
    token: &str,
    session_id: &str,
    action: &str,
    path: &str,
    name: &str,
) -> Result<(), String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream =
        TcpStream::connect(authority).map_err(|error| format!("连接文件服务失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|error| format!("设置文件请求超时失败：{error}"))?;
    let body = serde_json::json!({
        "action": action,
        "path": path,
        "name": name,
    })
    .to_string();
    let request = format!(
        "POST /api/sessions/{session_id}/file-action HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送文件操作失败：{error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取文件操作响应失败：{error}"))?;
    let text = String::from_utf8_lossy(&response);
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "文件操作响应格式无效".to_string())?;
    if !headers.lines().next().unwrap_or_default().contains(" 200 ") {
        let value: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("文件操作失败")
            .to_string());
    }
    Ok(())
}

fn post_upload_local(
    base: &str,
    token: &str,
    session_id: &str,
    directory: &str,
    local_path: &str,
) -> Result<String, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream =
        TcpStream::connect(authority).map_err(|error| format!("连接上传服务失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|error| format!("设置上传超时失败：{error}"))?;
    let body = serde_json::json!({
        "paths": [local_path],
        "directory": directory,
        "overwrite": false,
        "overwriteTargets": [],
        "skipTargets": [],
    })
    .to_string();
    let request = format!(
        "POST /api/sessions/{session_id}/upload-local HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送上传请求失败：{error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取上传响应失败：{error}"))?;
    let text = String::from_utf8_lossy(&response);
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "上传响应格式无效".to_string())?;
    let value: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
    if !headers.lines().next().unwrap_or_default().contains(" 200 ") {
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("上传失败")
            .to_string());
    }
    Ok("上传已加入传输队列".into())
}

fn delete_session(base: &str, token: &str, session_id: &str) -> Result<(), String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream =
        TcpStream::connect(authority).map_err(|error| format!("连接会话服务失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| format!("设置断开超时失败：{error}"))?;
    let request = format!(
        "DELETE /api/sessions/{session_id} HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送断开请求失败：{error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取断开响应失败：{error}"))?;
    let text = String::from_utf8_lossy(&response);
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "断开响应格式无效".to_string())?;
    if !headers.lines().next().unwrap_or_default().contains(" 200 ") {
        let value: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("断开会话失败")
            .to_string());
    }
    Ok(())
}

fn download_file(
    base: &str,
    token: &str,
    session_id: &str,
    remote_path: &str,
    name: &str,
) -> Result<PathBuf, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream =
        TcpStream::connect(authority).map_err(|error| format!("连接下载服务失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|error| format!("设置下载超时失败：{error}"))?;
    let request = format!(
        "GET /api/sessions/{session_id}/download?path={} HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nConnection: close\r\n\r\n",
        query_escape(remote_path)
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送下载请求失败：{error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取下载内容失败：{error}"))?;
    let marker = b"\r\n\r\n";
    let boundary = response
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or_else(|| "下载响应格式无效".to_string())?;
    let headers = &response[..boundary];
    let body = &response[boundary + marker.len()..];
    let header_text = String::from_utf8_lossy(headers);
    if !header_text
        .lines()
        .next()
        .unwrap_or_default()
        .contains(" 200 ")
    {
        let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("下载失败")
            .to_string());
    }
    let safe_name = PathBuf::from(name)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("download");
    let directory = env::temp_dir().join("DevShellDownloads");
    fs::create_dir_all(&directory).map_err(|error| format!("创建下载目录失败：{error}"))?;
    let target = directory.join(safe_name);
    fs::write(&target, body).map_err(|error| format!("保存下载文件失败：{error}"))?;
    Ok(target)
}

fn query_escape(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}

fn terminal_reader(
    base: &str,
    token: &str,
    session_id: &str,
    sender: &mpsc::Sender<TerminalEvent>,
    input_rx: mpsc::Receiver<String>,
) -> Result<(), String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream =
        TcpStream::connect(authority).map_err(|error| format!("连接终端失败：{error}"))?;
    let mut key = [0u8; 16];
    for (index, byte) in session_id.bytes().chain(token.bytes()).enumerate() {
        key[index % key.len()] ^= byte.wrapping_add(index as u8);
    }
    let websocket_key = base64::engine::general_purpose::STANDARD.encode(key);
    let path = format!("/api/sessions/{session_id}/terminal?token={token}&cols=120&rows=30");
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {websocket_key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送终端握手失败：{error}"))?;
    let headers = read_http_headers(&mut stream)?;
    if !headers.starts_with("HTTP/1.1 101") {
        return Err(format!(
            "终端握手失败：{}",
            headers.lines().next().unwrap_or_default()
        ));
    }
    let mut writer = stream
        .try_clone()
        .map_err(|error| format!("复制终端通道失败：{error}"))?;
    thread::spawn(move || {
        while let Ok(text) = input_rx.recv() {
            let payload = serde_json::json!({"type":"input","data":text}).to_string();
            if write_ws_text(&mut writer, &payload).is_err() {
                break;
            }
        }
    });
    write_ws_text(&mut stream, r#"{"type":"resize","cols":120,"rows":30}"#)?;
    let _ = sender.send(TerminalEvent::Info("终端已连接".into()));

    loop {
        let (opcode, payload) = read_ws_frame(&mut stream)?;
        match opcode {
            0x1 => {
                if let Ok(value) = serde_json::from_slice::<Value>(&payload) {
                    match value.get("type").and_then(Value::as_str) {
                        Some("ready") => {
                            let _ = sender.send(TerminalEvent::Info("远端 Shell 已就绪".into()));
                        }
                        Some("error") => {
                            let message = value
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("终端启动失败");
                            let _ = sender.send(TerminalEvent::Info(message.to_string()));
                        }
                        Some("exit") => {
                            let message = value
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("远端 Shell 已退出");
                            let _ = sender.send(TerminalEvent::Info(message.to_string()));
                        }
                        _ => {}
                    }
                }
            }
            0x2 => {
                let _ = sender.send(TerminalEvent::Output(
                    String::from_utf8_lossy(&payload).into_owned(),
                ));
                write_ws_text(&mut stream, r#"{"type":"ack"}"#)?;
            }
            0x8 => return Ok(()),
            0x9 => write_ws_control(&mut stream, 0xA, &payload)?,
            _ => {}
        }
    }
}

fn read_http_headers(stream: &mut TcpStream) -> Result<String, String> {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while bytes.len() < 64 * 1024 {
        stream
            .read_exact(&mut byte)
            .map_err(|error| format!("读取终端握手失败：{error}"))?;
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            return String::from_utf8(bytes).map_err(|error| format!("终端握手响应无效：{error}"));
        }
    }
    Err("终端握手响应过大".into())
}

fn read_ws_frame(stream: &mut TcpStream) -> Result<(u8, Vec<u8>), String> {
    let mut header = [0u8; 2];
    stream
        .read_exact(&mut header)
        .map_err(|error| format!("读取终端帧失败：{error}"))?;
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    let mut length = u64::from(header[1] & 0x7f);
    if length == 126 {
        let mut bytes = [0u8; 2];
        stream
            .read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        length = u64::from(u16::from_be_bytes(bytes));
    } else if length == 127 {
        let mut bytes = [0u8; 8];
        stream
            .read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        length = u64::from_be_bytes(bytes);
    }
    if length > 2 * 1024 * 1024 {
        return Err("终端帧过大".into());
    }
    let mut mask = [0u8; 4];
    if masked {
        stream
            .read_exact(&mut mask)
            .map_err(|error| error.to_string())?;
    }
    let mut payload = vec![0u8; length as usize];
    stream
        .read_exact(&mut payload)
        .map_err(|error| format!("读取终端帧内容失败：{error}"))?;
    if masked {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
    }
    Ok((opcode, payload))
}

fn write_ws_text(stream: &mut TcpStream, text: &str) -> Result<(), String> {
    write_ws_control(stream, 0x1, text.as_bytes())
}

fn write_ws_control(stream: &mut TcpStream, opcode: u8, payload: &[u8]) -> Result<(), String> {
    if payload.len() >= 126 {
        return Err("终端控制帧过大".into());
    }
    let mask = [0x52, 0x41, 0x57, 0x01];
    let mut frame = Vec::with_capacity(payload.len() + 6);
    frame.push(0x80 | opcode);
    frame.push(0x80 | payload.len() as u8);
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    stream
        .write_all(&frame)
        .map_err(|error| format!("发送终端控制帧失败：{error}"))
}

fn check_updates(base: &str, token: &str) -> Result<String, String> {
    let value = post_json(
        base,
        token,
        "/api/updates/check",
        serde_json::json!({"channel": "release"}),
    )?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if status == "available" {
        let version = value
            .get("latestVersion")
            .and_then(Value::as_str)
            .unwrap_or("新版本");
        let build = value
            .get("latestBuild")
            .and_then(Value::as_u64)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "未知构建".into());
        return Ok(format!(
            "发现 {version}（构建 {build}），可通过旧版界面完成下载和安装。"
        ));
    }
    if status == "none" {
        return Ok(format!(
            "当前已是最新版本（{}），{}。",
            value
                .get("currentVersion")
                .and_then(Value::as_str)
                .unwrap_or("当前版本"),
            value
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("没有可用更新")
        ));
    }
    Err(value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("更新服务返回未知状态")
        .to_string())
}

fn save_profile_and_reload(
    base: &str,
    token: &str,
    profile_id: Option<&str>,
    name: &str,
    host: &str,
    port: u16,
    user: &str,
    secret: &str,
) -> Result<BackendSnapshot, String> {
    let mut profile = serde_json::json!({
        "id": profile_id.unwrap_or_default(),
        "name": name,
        "host": host,
        "port": port,
        "user": user,
        "protocol": "ssh",
        "auth": "password",
        "groupId": "",
    });
    if !secret.is_empty() {
        profile["secret"] = Value::String(secret.to_string());
    }
    post_json(base, token, "/api/profiles", profile)?;
    Ok(BackendSnapshot::load(base, token))
}

fn delete_profile_and_reload(
    base: &str,
    token: &str,
    profile_id: &str,
) -> Result<BackendSnapshot, String> {
    delete_json(base, token, &format!("/api/profiles/{profile_id}"))?;
    Ok(BackendSnapshot::load(base, token))
}

fn post_json(base: &str, token: &str, path: &str, body: Value) -> Result<Value, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream = TcpStream::connect(authority).map_err(|error| format!("连接失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(12)))
        .map_err(|error| format!("设置超时失败：{error}"))?;
    let body = body.to_string();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送请求失败：{error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取响应失败：{error}"))?;
    let text = String::from_utf8_lossy(&response);
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "后端响应格式无效".to_string())?;
    let status = headers.lines().next().unwrap_or_default();
    let value: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
    if !status.contains(" 200 ") {
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or(status)
            .to_string());
    }
    Ok(value)
}

fn delete_json(base: &str, token: &str, path: &str) -> Result<Value, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream = TcpStream::connect(authority).map_err(|error| format!("连接失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(12)))
        .map_err(|error| format!("设置超时失败：{error}"))?;
    let request = format!(
        "DELETE {path} HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送删除请求失败：{error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取删除响应失败：{error}"))?;
    let text = String::from_utf8_lossy(&response);
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "删除响应格式无效".to_string())?;
    let status = headers.lines().next().unwrap_or_default();
    let value: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
    if !status.contains(" 200 ") {
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or(status)
            .to_string());
    }
    Ok(value)
}

fn get_json(base: &str, token: &str, path: &str) -> Result<Value, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream = TcpStream::connect(authority).map_err(|error| format!("连接失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| format!("设置超时失败：{error}"))?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送请求失败：{error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取响应失败：{error}"))?;
    let text = String::from_utf8_lossy(&response);
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "后端响应格式无效".to_string())?;
    let status = headers.lines().next().unwrap_or_default();
    if !status.contains(" 200 ") {
        return Err(format!("后端返回 {status}"));
    }
    let body = if path == "/boot.js" {
        body.trim()
            .strip_prefix("window.DEVSHELL = ")
            .and_then(|value| value.strip_suffix(';'))
            .ok_or_else(|| "boot.js 响应格式无效".to_string())?
    } else {
        body
    };
    serde_json::from_str(body.trim()).map_err(|error| format!("解析后端响应失败：{error}"))
}
