#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod assets;
mod appearance;
mod appearance_panels;
mod ansi_schemes;
mod key_panels;
mod security_panel;
mod file_browser;
mod file_view;
mod file_actions;
mod file_upload;
mod monitor_charts;
mod monitor_panel;
mod collation;
mod forms;
mod fonts;
mod http;
mod integration;
mod help;
mod server_manager;
mod settings;
mod sessions;
mod ui_scale;
#[cfg(feature="visual-test")]
mod visual_tests;
mod remote_desktop;
mod screen;
mod workspace;
#[cfg(test)]
mod workspace_tests;
mod workspace_tools;

use std::{
    env, fs,
    io::{Read, Write},
    net::TcpStream,
    process,
    sync::mpsc,
    thread,
    time::Duration,
};

use base64::Engine;
use gpui_kit::{
    App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window, WindowOptions,
    base::Disableable,
    component::{
        button::{Button as KitButton, ButtonVariants},
        input::{Input, InputState, Textarea, TextareaState},
    },
    div,
    prelude::FluentBuilder,
};
use ui_scale::pixels as px;
use serde_json::Value;

#[derive(Clone, Debug)]
struct BackendSnapshot {
    base: String,
    version: String,
    build: String,
    servers: Vec<ServerSummary>,
    groups: usize,
    group_names: Vec<String>,
    locked: bool,
    error: Option<String>,
    config_dir: Option<String>,
    config_format: Option<String>,
    security_summary: Option<String>,
    proxies: Vec<ProxySummary>,
    appearance: Value,
    commands: Vec<Value>,
    keys: Vec<Value>,
    config: Value,
    security: Value,
    ui_font_runtime:Value,
}

#[derive(Clone, Debug)]
struct ServerSummary {
    raw:Value,protocol:String,id:String,name:String,host:String,port:u16,user:String,
}
impl ServerSummary {
    fn from_value(profile:&Value)->Option<Self>{
        Some(Self{raw:profile.clone(),protocol:profile["protocol"].as_str().unwrap_or("ssh").into(),id:profile["id"].as_str()?.into(),name:profile["name"].as_str().unwrap_or("连接").into(),host:profile["host"].as_str().unwrap_or_default().into(),port:profile["port"].as_u64().unwrap_or(22) as u16,user:profile["user"].as_str().unwrap_or_default().into()})
    }
}

#[derive(Clone, Debug)]
struct ProxySummary {
    id: String,
    name: String,
    kind: String,
    host: String,
    port: u16,
    user: String,
}

#[derive(Clone)]
struct PendingHostKey {
    profile_id: String,
    challenge: HostKeyChallenge,
    secret: String,
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
    ui: workspace::WorkspaceUi,
    workspace: SessionWorkspace,
    parked: Vec<SessionWorkspace>,
    update_status: Option<String>,
    update_hash: Option<String>,
    update_checking: bool,
    update_download: Option<UpdateDownloadState>,
    connecting: Option<String>,
    feedback: Option<String>,
    command_input: Entity<InputState>,
    proxy_form_open: bool,
    proxy_saving: bool,
    proxy_name_input: Entity<InputState>,
    proxy_host_input: Entity<InputState>,
    proxy_port_input: Entity<InputState>,
    proxy_user_input: Entity<InputState>,
    proxy_password_input: Entity<InputState>,
    proxy_kind: String,
    proxy_editing_id: Option<String>,
    proxy_original_signature: Option<String>,
    pending_proxy_delete: Option<String>,
    pending_host_key: Option<PendingHostKey>,
    pending_credential: Option<ServerSummary>,
    temporary_profiles:Vec<ServerSummary>,
    pending_dialog:Option<(&'static str,Value)>,
    preview_input: Entity<ely_gpui_component::editor::CodeEditor>,
    form: Option<forms::NativeForm>,
    fonts:fonts::NativeFonts,
}

#[derive(Clone, Default)]
struct SessionWorkspace {
    terminal: Option<TerminalState>,
    files: Option<FileListState>,
    file_ui:file_view::FileUi,
    monitor: Option<MonitorState>,
    charts:monitor_charts::Charts,
    preview: Option<FilePreviewState>,
    preview_sync: Option<String>,
    editor:Option<Entity<ely_gpui_component::editor::CodeEditor>>,
    transfers: Vec<TransferState>,
    selected_file: Option<RemoteEntry>,
    command_history: Vec<String>,
    history_cursor: Option<usize>,
    history_draft: String,
    rdp: Option<Value>,
    auto_commands: Vec<String>,
    integration:integration::ShellIntegration,
    terminal_ready:bool,
    auto_next:Option<std::time::Instant>,
}

impl DevShellWindow {
    fn session_mut(&mut self, id: &str) -> Option<&mut SessionWorkspace> {
        find_session(&mut self.workspace, &mut self.parked, id)
    }

    fn park_session(&mut self) {
        if self.workspace.terminal.is_some() {
            self.parked.push(std::mem::take(&mut self.workspace));
        }
    }

    fn switch_session(&mut self, id: &str, cx: &App) {
        if let Some(preview) = &mut self.workspace.preview {
            preview.text = self.preview_input.read(cx).text().to_string();
        }
        if select_session(&mut self.workspace, &mut self.parked, id) {
            self.ui.active_start = None;
            if let Some(preview) = &self.workspace.preview {
                self.workspace.preview_sync = Some(preview.text.clone());
            }
        }
    }
}

fn find_session<'a>(
    active: &'a mut SessionWorkspace,
    parked: &'a mut [SessionWorkspace],
    id: &str,
) -> Option<&'a mut SessionWorkspace> {
    if active
        .terminal
        .as_ref()
        .is_some_and(|terminal| terminal.session_id == id)
    {
        Some(active)
    } else {
        parked.iter_mut().find(|session| {
            session
                .terminal
                .as_ref()
                .is_some_and(|terminal| terminal.session_id == id)
        })
    }
}

fn select_session(
    active: &mut SessionWorkspace,
    parked: &mut Vec<SessionWorkspace>,
    id: &str,
) -> bool {
    let Some(index) = parked.iter().position(|session| {
        session
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal.session_id == id)
    }) else {
        return false;
    };
    let selected = parked.remove(index);
    if active.terminal.is_some() {
        parked.push(std::mem::take(active));
    }
    *active = selected;
    true
}

#[derive(Clone, Debug)]
struct TerminalState {
    protocol: String,
    profile_id: String,
    name: String,
    host: String,
    session_id: String,
    screen: std::sync::Arc<std::sync::Mutex<screen::TerminalScreen>>,
    status: String,
    input_tx: Option<mpsc::Sender<TerminalInput>>,
    view: Option<Entity<ely_gpui_component::terminal::Terminal>>,
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
    link: bool,
    bytes: i64,
    owner: String,
    modified: String,
    modified_at: i64,
    mode: String,
}

#[derive(Clone, Debug)]
struct FilePreviewState {
    saved_text:String,
    path: String,
    encoding: String,
    sha256: String,
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
    raw: Value,
    load: String,
    status: String,
}

#[derive(Clone, Debug)]
struct UpdateDownloadState {
    id: String,
    status: String,
    received: i64,
    total: i64,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct UpdateCheckResult {
    summary: String,
    hash: Option<String>,
}

enum TerminalInput {
    Ack,
    Bytes(Vec<u8>),
    Text(String),
    Resize(usize, usize),
}

enum TerminalEvent {
    Ready(Value),
    Output(Vec<u8>),
    Info(String),
    Closed(String),
}

// Shell typography inherits the active workspace palette. ELY typography owns
// a separate palette and otherwise paints dark text in GPUI Kit dark dialogs.
fn paragraph(text: impl Into<gpui_kit::SharedString>) -> gpui_kit::Div {
    div()
        .text_size(px(12.))
        .line_height(gpui_kit::relative(1.6))
        .child(text.into())
}
fn heading(text: impl Into<gpui_kit::SharedString>) -> gpui_kit::Div {
    div()
        .text_size(px(17.))
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .child(text.into())
}

fn settings_panel(
    p: workspace::Palette,
    snapshot: BackendSnapshot,
    view: Entity<DevShellWindow>,
    update_status: Option<String>,
    update_hash: Option<String>,
    update_checking: bool,
    update_download: Option<UpdateDownloadState>,
    proxy_form_open: bool,
    proxy_saving: bool,
    proxy_name_input: Entity<InputState>,
    proxy_host_input: Entity<InputState>,
    proxy_port_input: Entity<InputState>,
    proxy_user_input: Entity<InputState>,
    proxy_password_input: Entity<InputState>,
    proxy_kind: String,
    pending_proxy_delete: Option<String>,
    proxy_editing_id: Option<String>,
    section: &'static str,
) -> impl IntoElement {
    let update_label = if update_checking {
        "正在检查更新…"
    } else {
        "检查更新"
    };
    let download_active = update_download
        .as_ref()
        .map(|job| matches!(job.status.as_str(), "downloading" | "queued"))
        .unwrap_or(false);
    div()
        .p_5()
        .rounded_lg()
        .bg(gpui_kit::rgb(p.surface))
        .border_1()
        .border_color(gpui_kit::rgb(p.border))
        .flex()
        .flex_col()
        .gap_2()
        .when(section == "updates", |panel| {
            panel.child(paragraph(format!(
                "{} · 构建 {}",
                snapshot.version, snapshot.build
            )))
        })
        .when(section == "proxy", |panel| {
            panel
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(paragraph("已保存代理"))
                        .child(
                            KitButton::new("new-proxy")
                                .secondary()
                                .label(if proxy_form_open {
                                    "收起新增"
                                } else {
                                    "新增代理"
                                })
                                .on_click({
                                    let view = view.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |state, cx| {
                                            let opening = !state.proxy_form_open;
                                            state.proxy_form_open = opening;
                                            if opening {
                                                state.proxy_editing_id = None;
                                                state.proxy_original_signature = None;
                                                state.proxy_name_input.update(cx, |input, cx| {
                                                    input.set_value("", window, cx)
                                                });
                                                state.proxy_host_input.update(cx, |input, cx| {
                                                    input.set_value("", window, cx)
                                                });
                                                state.proxy_port_input.update(cx, |input, cx| {
                                                    input.set_value("1080", window, cx)
                                                });
                                                state.proxy_user_input.update(cx, |input, cx| {
                                                    input.set_value("", window, cx)
                                                });
                                                state
                                                    .proxy_password_input
                                                    .update(cx, |input, cx| {
                                                        input.set_value("", window, cx)
                                                    });
                                                state.proxy_kind = "socks5".into();
                                            }
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                )
                .when(proxy_form_open, |panel| {
                    panel.child(proxy_form(
                        p,
                        view.clone(),
                        proxy_name_input.clone(),
                        proxy_host_input.clone(),
                        proxy_port_input.clone(),
                        proxy_user_input.clone(),
                        proxy_password_input.clone(),
                        proxy_kind.clone(),
                        proxy_editing_id.clone(),
                        proxy_saving,
                    ))
                })
                .children(snapshot.proxies.iter().map(|proxy| {
                    let proxy_id = proxy.id.clone();
                    let is_pending = pending_proxy_delete.as_deref() == Some(proxy.id.as_str());
                    let delete_view = view.clone();
                    let edit_view = view.clone();
                    let edit_id = proxy.id.clone();
                    let edit_name = proxy.name.clone();
                    let edit_kind = proxy.kind.clone();
                    let edit_host = proxy.host.clone();
                    let edit_port = proxy.port;
                    let edit_user = proxy.user.clone();
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(paragraph(format!(
                            "{} · {} {}:{}",
                            proxy.name, proxy.kind, proxy.host, proxy.port
                        )))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    KitButton::new(format!("edit-proxy-{edit_id}"))
                                        .ghost()
                                        .label("编辑")
                                        .on_click(move |_, window, cx| {
                                            edit_view.update(cx, |state, cx| {
                                                state.proxy_form_open = true;
                                                state.proxy_editing_id = Some(edit_id.clone());
                                                state.proxy_original_signature = Some(format!(
                                                    "{}|{}|{}|{}",
                                                    edit_kind, edit_host, edit_port, edit_user
                                                ));
                                                state.proxy_name_input.update(cx, |input, cx| {
                                                    input.set_value(edit_name.clone(), window, cx)
                                                });
                                                state.proxy_host_input.update(cx, |input, cx| {
                                                    input.set_value(edit_host.clone(), window, cx)
                                                });
                                                state.proxy_port_input.update(cx, |input, cx| {
                                                    input.set_value(
                                                        edit_port.to_string(),
                                                        window,
                                                        cx,
                                                    )
                                                });
                                                state.proxy_user_input.update(cx, |input, cx| {
                                                    input.set_value(edit_user.clone(), window, cx)
                                                });
                                                state
                                                    .proxy_password_input
                                                    .update(cx, |input, cx| {
                                                        input.set_value("", window, cx)
                                                    });
                                                state.proxy_kind = edit_kind.clone();
                                                state.feedback =
                                                    Some("已载入代理配置，可修改后保存。".into());
                                                cx.notify();
                                            });
                                        }),
                                )
                                .child(
                                    KitButton::new(format!("delete-proxy-{proxy_id}"))
                                        .danger()
                                        .label(if is_pending { "确认删除" } else { "删除" })
                                        .on_click(move |_, _, cx| {
                                            if is_pending {
                                                delete_proxy_and_reload(
                                                    delete_view.clone(),
                                                    proxy_id.clone(),
                                                    cx,
                                                );
                                            } else {
                                                let _ = delete_view.update(cx, |state, cx| {
                                                    state.pending_proxy_delete =
                                                        Some(proxy_id.clone());
                                                    state.feedback = Some(
                                                        "再次点击删除按钮确认移除代理。".into(),
                                                    );
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                ),
                        )
                }))
        })
        .when(section == "updates", |panel| {
            panel
                .when_some(update_status, |card, status| {
                    card.child(paragraph(format!("更新状态：{status}")))
                })
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
                                            match result {
                                                Ok(result) => {
                                                    state.update_status = Some(result.summary);
                                                    state.update_hash = result.hash;
                                                    state.update_download = None;
                                                }
                                                Err(error) => {
                                                    state.update_status =
                                                        Some(format!("检查失败：{error}"));
                                                    state.update_hash = None;
                                                }
                                            }
                                            state.feedback = Some("更新检查完成。".into());
                                            cx.notify();
                                        });
                                    })
                                    .detach();
                                });
                            }
                        }),
                )
                .when_some(update_hash, |panel, hash| {
                    panel.child(
                        KitButton::new("download-update")
                            .secondary()
                            .label(if download_active {
                                "正在下载更新…"
                            } else {
                                "下载更新"
                            })
                            .disabled(download_active)
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    start_update_download(view.clone(), hash.clone(), cx)
                                }
                            }),
                    )
                })
                .when_some(update_download, |panel, download| {
                    let progress = if download.total > 0 {
                        format!(
                            "{:.0}% ({}/{})",
                            download.received as f64 / download.total as f64 * 100.0,
                            download.received,
                            download.total
                        )
                    } else {
                        format!("{} / {}", download.received, download.total)
                    };
                    panel.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(paragraph(format!(
                                "更新下载 · {} · {}{}",
                                download.status,
                                progress,
                                download
                                    .error
                                    .as_deref()
                                    .map(|error| format!(" · {error}"))
                                    .unwrap_or_default()
                            )))
                            .when(download_active, |row| {
                                let view = view.clone();
                                let job_id = download.id.clone();
                                row.child(
                                    KitButton::new("cancel-update-download")
                                        .danger()
                                        .label("取消下载")
                                        .on_click(move |_, _, cx| {
                                            cancel_update_download(view.clone(), job_id.clone(), cx)
                                        }),
                                )
                            })
                            .when(download.status == "ready", |row| {
                                let view = view.clone();
                                let job_id = download.id.clone();
                                row.child(
                                    KitButton::new("install-update")
                                        .primary()
                                        .label("安装更新")
                                        .on_click(move |_, _, cx| {
                                            install_update(view.clone(), job_id.clone(), cx)
                                        }),
                                )
                            }),
                    )
                })
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
        })
}

fn proxy_form(
    p: workspace::Palette,
    view: Entity<DevShellWindow>,
    name_input: Entity<InputState>,
    host_input: Entity<InputState>,
    port_input: Entity<InputState>,
    user_input: Entity<InputState>,
    password_input: Entity<InputState>,
    kind: String,
    editing_id: Option<String>,
    saving: bool,
) -> impl IntoElement {
    let socks_button = KitButton::new("proxy-kind-socks5");
    let socks_button = if kind == "socks5" {
        socks_button.primary()
    } else {
        socks_button.secondary()
    };
    let http_button = KitButton::new("proxy-kind-http");
    let http_button = if kind == "http" {
        http_button.primary()
    } else {
        http_button.secondary()
    };
    div()
        .p_4()
        .rounded_md()
        .bg(gpui_kit::rgb(p.surface))
        .border_1()
        .border_color(gpui_kit::rgb(p.border))
        .flex()
        .flex_col()
        .gap_2()
        .child(paragraph(if editing_id.is_some() {
            "编辑连接代理"
        } else {
            "新增连接代理"
        }))
        .child(Input::new(&name_input))
        .child(
            div()
                .flex()
                .gap_2()
                .child(socks_button.label("SOCKS5").on_click({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |state, cx| {
                            state.proxy_kind = "socks5".into();
                            cx.notify();
                        });
                    }
                }))
                .child(http_button.label("HTTP CONNECT").on_click({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |state, cx| {
                            state.proxy_kind = "http".into();
                            cx.notify();
                        });
                    }
                })),
        )
        .child(Input::new(&host_input))
        .child(Input::new(&port_input))
        .child(Input::new(&user_input))
        .child(Input::new(&password_input).mask_toggle())
        .child(
            KitButton::new("save-proxy")
                .secondary()
                .label(if saving {
                    "保存中…"
                } else if editing_id.is_some() {
                    "保存修改"
                } else {
                    "保存代理"
                })
                .disabled(saving)
                .on_click(move |_, _, cx| {
                    view.update(cx, |state, cx| {
                        let name = state.proxy_name_input.read(cx).value().trim().to_string();
                        let host = state.proxy_host_input.read(cx).value().trim().to_string();
                        let port_text = state.proxy_port_input.read(cx).value().trim().to_string();
                        let user = state.proxy_user_input.read(cx).value().trim().to_string();
                        let password = state.proxy_password_input.read(cx).value().to_string();
                        if name.is_empty() || host.is_empty() {
                            state.feedback = Some("请填写代理名称和地址。".into());
                            cx.notify();
                            return;
                        }
                        let port = match port_text.parse::<u16>() {
                            Ok(port) if port > 0 => port,
                            _ => {
                                state.feedback = Some("代理端口必须是 1–65535 的数字。".into());
                                cx.notify();
                                return;
                            }
                        };
                        let base = state.snapshot.base.clone();
                        let token = state.token.clone();
                        let kind = state.proxy_kind.clone();
                        let editing_id = state.proxy_editing_id.clone();
                        let signature = format!("{}|{}|{}|{}", kind, host, port, user);
                        if editing_id.is_some()
                            && password.is_empty()
                            && state.proxy_original_signature.as_deref() != Some(signature.as_str())
                        {
                            state.feedback =
                                Some("修改代理地址或认证信息时，请重新填写代理密码。".into());
                            cx.notify();
                            return;
                        }
                        state.proxy_saving = true;
                        state.feedback = Some("正在保存代理配置…".into());
                        cx.notify();
                        cx.spawn(async move |this, cx| {
                            let result = cx
                                .background_executor()
                                .spawn(async move {
                                    save_proxy_and_reload(
                                        &base,
                                        &token,
                                        editing_id.as_deref(),
                                        &name,
                                        &kind,
                                        &host,
                                        port,
                                        &user,
                                        &password,
                                    )
                                })
                                .await;
                            let _ = this.update(cx, |state, cx| {
                                state.proxy_saving = false;
                                match result {
                                    Ok(snapshot) => {
                                        state.snapshot = snapshot;
                                        state.proxy_form_open = false;
                                        state.proxy_editing_id = None;
                                        state.proxy_original_signature = None;
                                        state.feedback = Some("代理配置已保存。".into());
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

fn file_preview_panel(
    p: workspace::Palette,
    preview: FilePreviewState,
    preview_input: Entity<ely_gpui_component::editor::CodeEditor>,
    view: Entity<DevShellWindow>,
) -> impl IntoElement {
    let save_view = view.clone();
    let close_view = view.clone();
    div()
        .absolute().left(px(32.)).right(px(32.)).top(px(68.)).bottom(px(24.)).occlude().shadow_lg()
        .p_4()
        .rounded_lg()
        .bg(gpui_kit::rgb(p.inset))
        .border_1()
        .border_color(gpui_kit::rgb(p.border))
        .flex()
        .flex_col()
        .gap_2()
        .child(paragraph(format!(
            "预览 · {} · 编码 {} · {}",
            preview.path, preview.encoding, preview.status
        )))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(paragraph("保存前会校验远端文件指纹，冲突时保留远端版本。"))
                .child(
                    KitButton::new("preview-save")
                        .primary()
                        .label("保存文件")
                        .on_click(move |_, _, cx| save_preview(save_view.clone(), cx)),
                )
                .child(
                    KitButton::new("preview-close")
                        .ghost()
                        .label("关闭预览")
                        .on_click(move |_, _, cx| {
                            close_view.update(cx, |state, cx| {
                                if state.workspace.preview.as_ref().is_some_and(|preview|state.preview_input.read(cx).text()!=preview.saved_text){state.pending_dialog=Some(("close-preview",serde_json::json!({})));}
                                else{state.workspace.preview=None;state.workspace.preview_sync=None;state.workspace.editor=None;}
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(div().flex_1().min_h_0().child(preview_input))
}

fn transfer_panel(
    p: workspace::Palette,
    transfers: Vec<TransferState>,
    view: Entity<DevShellWindow>,
) -> impl IntoElement {
    let clear_view = view.clone();
    div()
        .p_4()
        .rounded_lg()
        .bg(gpui_kit::rgb(p.surface))
        .border_1()
        .border_color(gpui_kit::rgb(p.border))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(paragraph("传输队列"))
                .child(
                    KitButton::new("transfer-clear-completed")
                        .secondary()
                        .label("清理已完成")
                        .on_click(move |_, _, cx| {
                            clear_completed_transfers(clear_view.clone(), cx)
                        }),
                ),
        )
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
                .child(paragraph(format!(
                    "{} · {} · {}",
                    transfer.name, transfer.status, progress
                )))
                .child(paragraph(transfer.error.unwrap_or_else(|| {
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

fn navigate_files_for(view: Entity<DevShellWindow>,session_id:String,path:String,cx:&mut App){
    navigate_files_for_mode(view,session_id,path,false,cx);
}
fn navigate_files_for_mode(view: Entity<DevShellWindow>,session_id:String,path:String,preview_on_error:bool,cx:&mut App){
    cx.defer(move|cx|{
    let _=view.update(cx,|state,cx|{
        if state.snapshot.locked{return;}

        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some(format!("正在读取远端目录 {}…", path));
        let Some(owner)=state.session_mut(&session_id)else{return;};owner.file_ui.navigation=owner.file_ui.navigation.wrapping_add(1);let navigation=owner.file_ui.navigation;
        let result_session=Some(session_id.clone());
        let view = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { load_files(&base, &token, &session_id, &path) }
                })
                .await;
            let _ = view.update(cx, |state, cx| {
                let Some(session) = result_session
                    .as_deref()
                    .and_then(|id| state.session_mut(id))
                else {
                    return;
                };
                let preview = preview_on_error && result.is_err() && session.file_ui.navigation == navigation;
                let target = path.clone();
                if !session.accept_listing(navigation,path,result){return;}
                if preview && state.workspace.terminal.as_ref().is_some_and(|t|Some(t.session_id.as_str())==result_session.as_deref()) { state.preview_file(target,cx); }
                cx.notify();
            });
        })
        .detach();
    });
    });
}

fn preview_remote_for(view: Entity<DevShellWindow>,owner:String, path: String, cx: &mut App) {
    cx.defer(move|cx|{
    let _ = view.update(cx, |state, cx| {
        if state.snapshot.locked||!state.workspace.terminal.as_ref().is_some_and(|t|t.session_id==owner&&t.input_tx.is_some()){return;}

        if state.workspace.preview.as_ref().is_some_and(|preview|state.preview_input.read(cx).text()!=preview.saved_text){state.pending_dialog=Some(("open-preview",serde_json::json!({"path":path})));cx.notify();return;}
        let Some(session_id) = state
            .workspace
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some(format!("正在读取文件 {}…", path));
        let result_session = state
            .workspace
            .terminal
            .as_ref()
            .map(|t| t.session_id.clone());
        let view = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_preview(&base, &token, &session_id, &path) })
                .await;
            let _ = view.update(cx, |state, cx| {
                let Some(session) = result_session
                    .as_deref()
                    .and_then(|id| state.session_mut(id))
                else {
                    return;
                };
                match result {
                    Ok(preview) => {
                        let text = preview.text.clone();
                        session.editor=None;
                        session.preview = Some(preview);
                        session.preview_sync = Some(text);
                        state.feedback = Some("文件已加载".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
    });
}

fn save_preview(view: Entity<DevShellWindow>, cx: &mut App) {
    cx.defer(move|cx|{
    let _ = view.update(cx, |state, cx| {
        let Some(preview) = state.workspace.preview.clone() else {
            return;
        };
        let Some(session_id) = state
            .workspace
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_id.clone())
        else {
            return;
        };
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        let text = state.preview_input.read(cx).text().to_string();
        state.feedback = Some(format!("正在保存文件 {}…", preview.path));
        let result_session = state
            .workspace
            .terminal
            .as_ref()
            .map(|t| t.session_id.clone());
        let view = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(
                    async move { save_file_content(&base, &token, &session_id, &preview, &text) },
                )
                .await;
            let _ = view.update(cx, |state, cx| {
                let Some(session) = result_session
                    .as_deref()
                    .and_then(|id| state.session_mut(id))
                else {
                    return;
                };
                match result {
                    Ok(saved) => {
                        session.preview = Some(saved);
                        state.feedback = Some("文件已保存".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
    });
}

fn retry_transfer(view: Entity<DevShellWindow>, transfer_id: String, cx: &mut App) {
    transfer_action(view, transfer_id, "POST", "retry", cx);
}

fn cancel_transfer(view: Entity<DevShellWindow>, transfer_id: String, cx: &mut App) {
    transfer_action(view, transfer_id, "DELETE", "cancel", cx);
}

fn clear_completed_transfers(view: Entity<DevShellWindow>, cx: &mut App) {
    cx.defer(move|cx|{
    let _ = view.update(cx, |state, cx| {
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some("正在清理已完成传输…".into());
        let result_session = state
            .workspace
            .terminal
            .as_ref()
            .map(|t| t.session_id.clone());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_json(
                        &base,
                        &token,
                        "/api/transfers/clear-completed",
                        serde_json::json!({}),
                    )
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                let Some(session) = result_session
                    .as_deref()
                    .and_then(|id| state.session_mut(id))
                else {
                    return;
                };
                match result {
                    Ok(_) => {
                        session.transfers.retain(|transfer| {
                            matches!(transfer.status.as_str(), "queued" | "uploading")
                        });
                        state.feedback = Some("已清理完成传输。".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
    });
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
    cx.defer(move|cx|{
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
        let result_session = state
            .workspace
            .terminal
            .as_ref()
            .map(|t| t.session_id.clone());
        let view = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { request_transfer_action(&base, &token, &transfer_id, &method) })
                .await;
            let _ = view.update(cx, |state, cx| {
                let Some(_session) = result_session
                    .as_deref()
                    .and_then(|id| state.session_mut(id))
                else {
                    return;
                };
                state.feedback = Some(match result {
                    Ok(()) => "传输操作已提交".into(),
                    Err(error) => error,
                });
                cx.notify();
            });
        })
        .detach();
    });
    });
}

fn disconnect_session(view: Entity<DevShellWindow>, session_id: String, cx: &mut App) {
    cx.defer(move|cx|{
    let _ = view.update(cx, |state, cx| {
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some("正在断开会话…".into());
        let protocol=state.session_mut(&session_id).and_then(|session|session.terminal.as_ref()).map(|t|t.protocol.clone()).unwrap_or_else(||"ssh".into());
        let result_session = session_id.clone();
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { delete_json(&base,&token,&format!("{}{session_id}",remote_desktop::session_root(&protocol))).map(|_|()) })
                .await;
            let _ = view.update(cx, |state, cx| {
                match result {
                    Ok(()) => {
                        if let Some(session) = state.session_mut(&result_session) {
                            if let Some(terminal) = &mut session.terminal {
                                terminal.input_tx = None;
                                terminal.status = "已断开".into();
                            }
                        }
                        if state.ui.closing_sessions.contains(&result_session) {
                            state.ui.closing_sessions.retain(|id| id != &result_session);
                            state.ui.session_order.retain(|id| id != &result_session);
                            state.parked.retain(|s| {
                                !s.terminal
                                    .as_ref()
                                    .is_some_and(|t| t.session_id == result_session)
                            });
                            if state
                                .workspace
                                .terminal
                                .as_ref()
                                .is_some_and(|t| t.session_id == result_session)
                            {
                                state.workspace = state.parked.pop().unwrap_or_default();
                                if state.workspace.terminal.is_none() {
                                    state.ui.active_start = state.ui.start_tabs.last().copied();
                                }
                            }
                        }
                        state.feedback = Some("会话已断开".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
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

fn main() {
    let args = Arguments::parse().unwrap_or_else(|error| {
        eprintln!("DevShell native UI: {error}");
        process::exit(2);
    });
    let snapshot = BackendSnapshot::load(&args.base, &args.token);

    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            let _=cx.text_system().add_fonts(vec![
                std::borrow::Cow::Borrowed(include_bytes!("../../web/assets/ui-fonts/alibaba-puhuiti-3-55-regular.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/jetbrains-mono.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/fira-code.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/source-code-pro.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/ibm-plex-mono.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/ibm-plex-mono-bold.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/maple-mono-cn.ttf")),
            ]);
            let dark = match snapshot.appearance.get("theme").and_then(Value::as_str) {
                Some("light") => false,
                Some("dark") => true,
                _ => matches!(
                    cx.window_appearance(),
                    gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark
                ),
            };
            workspace::apply_theme(dark, None, cx);
            ely_gpui_component::init(cx).expect("Ely failed to start");
            ely_gpui_component::terminal::bind_keys(cx);
            ely_gpui_component::editor::bind_keys(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let options = WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(
                    gpui_kit::Bounds::centered(None, gpui_kit::size(px(1280.), px(800.)), cx),
                )),
                window_min_size: Some(gpui_kit::size(px(720.), px(480.))),
                titlebar: Some(gpui_kit::TitlebarOptions {
                    title: Some("DevShell".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                app_owns_titlebar_drag: true,
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                let entity=cx.new(|cx| {
                    let focus = cx.focus_handle();
                    window.focus(&focus, cx);
                    let command_input = cx.new(|cx| {
                        InputState::new(window, cx).placeholder("输入命令并回车发送 · ↑↓ 历史")
                    });

                    let proxy_name_input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("代理名称"));
                    let proxy_host_input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("代理地址"));
                    let proxy_port_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value("1080")
                            .placeholder("代理端口")
                    });
                    let proxy_user_input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("代理用户名"));
                    let proxy_password_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder("代理密码")
                            .masked(true)
                    });
                    let preview_input =
                        cx.new(|cx| ely_gpui_component::editor::CodeEditor::new("",window,cx));
                    cx.subscribe_in(
                        &command_input,
                        window,
                        |state: &mut DevShellWindow,
                         _,
                         event: &gpui_kit::base::input::InputEvent,
                         window,
                         cx| {
                            if matches!(event, gpui_kit::base::input::InputEvent::PressEnter { .. })
                            {
                                state.send_command(window, cx);
                            }
                        },
                    )
                    .detach();
                    let mut ui = workspace::WorkspaceUi::new(window, cx);
                    ui.restore(&snapshot.appearance, dark);
                    ui.rdp.parent=DevShellWindow::native_parent(window);
                    cx.subscribe(
                        &ui.file_filter,
                        |_, _, _: &gpui_kit::base::input::InputEvent, cx| cx.notify(),
                    )
                    .detach();
                    cx.subscribe_in(
                        &ui.file_path,
                        window,
                        |state: &mut DevShellWindow,
                         _,
                         event: &gpui_kit::base::input::InputEvent,
                         _,
                         cx| {
                            if matches!(event, gpui_kit::base::input::InputEvent::PressEnter { .. })
                            {
                                let path = state.ui.file_path.read(cx).value().to_string();
                                if !path.trim().is_empty() {
                                    state.browse_files(path,cx);
                                }
                            }
                        },
                    )
                    .detach();
                    cx.subscribe(
                        &ui.search,
                        |_, _, _: &gpui_kit::base::input::InputEvent, cx| cx.notify(),
                    )
                    .detach();
                    DevShellWindow {
                        focus,
                        snapshot,
                        token: args.token.clone(),
                        settings_visible: false,
                        ui,
                        workspace: SessionWorkspace::default(),
                        parked: Vec::new(),
                        update_status: None,
                        update_hash: None,
                        update_checking: false,
                        update_download: None,
                        connecting: None,
                        feedback: None,
                        command_input,
                        proxy_form_open: false,
                        proxy_saving: false,
                        proxy_name_input,
                        proxy_host_input,
                        proxy_port_input,
                        proxy_user_input,
                        proxy_password_input,
                        proxy_kind: "socks5".into(),
                        proxy_editing_id: None,
                        proxy_original_signature: None,
                        pending_proxy_delete: None,
                        pending_host_key: None,
                        pending_credential: None,
                        temporary_profiles:Vec::new(),
                        pending_dialog:None,
                        preview_input,
                        form: None,
                        fonts:fonts::NativeFonts::default(),
                    }
                });
                entity.update(cx,|state,cx|{state.poll_security(cx);state.poll_rdp(cx);});
                #[cfg(feature="visual-test")]
                if let Some(directory)=env::var_os("DEVSHELL_VISUAL_TEST_DIR"){visual_tests::start(directory.into(),&entity,window,cx);}
                entity
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
            group_names: Vec::new(),
            locked: false,
            error: None,
            config_dir: None,
            config_format: None,
            security_summary: None,
            proxies: Vec::new(),
            appearance: Value::Null,
            commands: Vec::new(),
            keys: Vec::new(),
            config: Value::Null,
            security: Value::Null,
            ui_font_runtime:Value::Null,
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
                snapshot.config = value.clone();
                snapshot.appearance = value.get("appearance").cloned().unwrap_or(Value::Null);
                snapshot.commands = value
                    .get("commands")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                snapshot.keys = value
                    .get("keys")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                snapshot.servers=value["servers"].as_array().map(|profiles|profiles.iter().filter_map(ServerSummary::from_value).collect()).unwrap_or_default();
                snapshot.groups = value
                    .get("groupNodes")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                snapshot.group_names = value
                    .get("groupNodes")
                    .and_then(Value::as_array)
                    .map(|groups| {
                        groups
                            .iter()
                            .filter_map(|group| group.get("name").and_then(Value::as_str))
                            .map(ToString::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                snapshot.proxies = value
                    .get("proxies")
                    .and_then(Value::as_array)
                    .map(|proxies| {
                        proxies
                            .iter()
                            .filter_map(|proxy| {
                                Some(ProxySummary {
                                    id: proxy.get("id")?.as_str()?.to_string(),
                                    name: proxy
                                        .get("name")
                                        .and_then(Value::as_str)
                                        .unwrap_or("代理")
                                        .to_string(),
                                    kind: proxy
                                        .get("proxy")
                                        .and_then(|value| value.get("type"))
                                        .and_then(Value::as_str)
                                        .unwrap_or("direct")
                                        .to_string(),
                                    host: proxy
                                        .get("proxy")
                                        .and_then(|value| value.get("host"))
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                    port: proxy
                                        .get("proxy")
                                        .and_then(|value| value.get("port"))
                                        .and_then(Value::as_u64)
                                        .unwrap_or(0)
                                        as u16,
                                    user: proxy
                                        .get("proxy")
                                        .and_then(|value| value.get("user"))
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Err(error) if error.contains("423") => snapshot.locked = true,
            Err(error) if snapshot.error.is_none() => snapshot.error = Some(error),
            Err(_) => {}
        }
        snapshot.ui_font_runtime=get_json(base,token,"/api/ui-fonts/runtime").unwrap_or_default();
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
            snapshot.security=value.clone();
            snapshot.locked=value.get("locked").and_then(Value::as_bool).unwrap_or(snapshot.locked);
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
    let response=http::decode_response(&response)?;
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
    Ok(parse_file_list(&value,path))
}

fn parse_file_list(value:&Value,path:&str)->FileListState {
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
                        link: entry.get("link").and_then(Value::as_bool).unwrap_or(false),
                        bytes: entry.get("bytes").and_then(Value::as_i64).unwrap_or(0),
                        owner: entry
                            .get("owner")
                            .and_then(Value::as_str)
                            .unwrap_or("未知属主")
                            .to_string(),
                        modified: entry
                            .get("time")
                            .and_then(Value::as_str)
                            .unwrap_or("—")
                            .to_owned(),
                        modified_at: entry.get("modifiedAt").and_then(Value::as_i64).unwrap_or(0),
                        mode: entry
                            .get("mode")
                            .and_then(Value::as_str)
                            .unwrap_or("—")
                            .to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    FileListState {
        path: value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(path)
            .to_string(),
        entries,
        status: "已加载".into(),
    }
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
        saved_text:value.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
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
        sha256: value
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        text: value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        status: "可编辑".into(),
    })
}

fn save_file_content(
    base: &str,
    token: &str,
    session_id: &str,
    preview: &FilePreviewState,
    text: &str,
) -> Result<FilePreviewState, String> {
    let value = post_json(
        base,
        token,
        &format!("/api/sessions/{session_id}/file-content"),
        serde_json::json!({
            "path": preview.path.clone(),
            "encoding": preview.encoding.clone(),
            "sha256": preview.sha256.clone(),
            "text": text,
        }),
    )?;
    Ok(FilePreviewState {
        saved_text:text.to_owned(),
        path: value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(&preview.path)
            .to_string(),
        encoding: value
            .get("encoding")
            .and_then(Value::as_str)
            .unwrap_or(&preview.encoding)
            .to_string(),
        sha256: value
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap_or(&preview.sha256)
            .to_string(),
        text: value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or(text)
            .to_string(),
        status: "已保存".into(),
    })
}

fn load_monitor(base: &str, token: &str, session_id: &str) -> Result<MonitorState, String> {
    let value = get_json(base, token, &format!("/api/sessions/{session_id}/stats"))?;
    Ok(MonitorState {
        raw: value.clone(),
        load: value
            .get("load")
            .and_then(Value::as_str)
            .unwrap_or("未知")
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
    let response=http::decode_response(&response)?;
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
    let response=http::decode_response(&response)?;
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
    protocol: &str,
    sender: &mpsc::Sender<TerminalEvent>,
    input_rx: mpsc::Receiver<TerminalInput>,
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
    let path = format!("{}{session_id}/terminal?token={token}&cols=120&rows=30",remote_desktop::session_root(protocol));
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
    let writer = std::sync::Arc::new(std::sync::Mutex::new(
        stream
            .try_clone()
            .map_err(|error| format!("复制终端通道失败：{error}"))?,
    ));
    let input_writer = writer.clone();
    thread::spawn(move || {
        while let Ok(input) = input_rx.recv() {
            let payloads=match input {
                TerminalInput::Ack=>vec![serde_json::json!({"type":"ack"}).to_string()],
                TerminalInput::Text(text)=>terminal_input_frames(text.as_bytes()),
                TerminalInput::Bytes(bytes)=>terminal_input_frames(&bytes),
                TerminalInput::Resize(cols,rows)=>vec![serde_json::json!({"type":"resize","cols":cols,"rows":rows}).to_string()],
            };
            for payload in payloads {
                let Ok(mut writer)=input_writer.lock()else{return;};
                if write_ws_text(&mut writer,&payload).is_err(){return;}
            }
        }
    });
    write_ws_text(
        &mut *writer.lock().map_err(|_| "终端发送锁失效")?,
        r#"{"type":"resize","cols":120,"rows":30}"#,
    )?;
    let _ = sender.send(TerminalEvent::Info("终端已连接".into()));

    loop {
        let (opcode, payload) = read_ws_frame(&mut stream)?;
        match opcode {
            0x1 => {
                if let Ok(value) = serde_json::from_slice::<Value>(&payload) {
                    match value.get("type").and_then(Value::as_str) {
                        Some("ready") => {
                            let _ = sender.send(TerminalEvent::Ready(value.get("integration").cloned().unwrap_or_default()));
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
            0x2 => {let _=sender.send(TerminalEvent::Output(payload));}
            0x8 => return Ok(()),
            0x9 => write_ws_control(
                &mut *writer.lock().map_err(|_| "终端发送锁失效")?,
                0xA,
                &payload,
            )?,
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
    let frame = websocket_frame(opcode, payload)?;
    stream
        .write_all(&frame)
        .map_err(|error| format!("发送终端帧失败：{error}"))
}

fn websocket_frame(opcode: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
    if opcode >= 0x8 && payload.len() >= 126 {
        return Err("终端控制帧过大".into());
    }
    let mask = [0x52, 0x41, 0x57, 0x01];
    let mut frame = Vec::with_capacity(payload.len() + 6);
    frame.push(0x80 | opcode);
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    Ok(frame)
}

fn check_updates(base: &str, token: &str) -> Result<UpdateCheckResult, String> {
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
        return Ok(UpdateCheckResult {
            summary: format!("发现 {version}（构建 {build}）。"),
            hash: value
                .get("package")
                .and_then(|package| package.get("sha256"))
                .and_then(Value::as_str)
                .map(ToString::to_string),
        });
    }
    if status == "none" {
        return Ok(UpdateCheckResult {
            summary: format!(
                "当前已是最新版本（{}），{}。",
                value
                    .get("currentVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("当前版本"),
                value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("没有可用更新")
            ),
            hash: None,
        });
    }
    Err(value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("更新服务返回未知状态")
        .to_string())
}

fn parse_update_download(value: &Value) -> UpdateDownloadState {
    UpdateDownloadState {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        status: value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        received: value.get("received").and_then(Value::as_i64).unwrap_or(0),
        total: value.get("total").and_then(Value::as_i64).unwrap_or(0),
        error: value
            .get("error")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    }
}

fn start_update_download(view: Entity<DevShellWindow>, hash: String, cx: &mut App) {
    cx.defer(move|cx|{
    let _ = view.update(cx, |state, cx| {
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some("正在启动更新下载…".into());
        cx.spawn(async move |this, cx| {
            let request_base = base.clone();
            let request_token = token.clone();
            let request_hash = hash.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_json(
                        &request_base,
                        &request_token,
                        "/api/updates/download",
                        serde_json::json!({"sha256": request_hash}),
                    )
                })
                .await;
            let value = match result {
                Ok(value) => value,
                Err(error) => {
                    let _ = this.update(cx, |state, cx| {
                        state.feedback = Some(error);
                        cx.notify();
                    });
                    return;
                }
            };
            let job = parse_update_download(&value);
            let job_id = job.id.clone();
            let active = matches!(job.status.as_str(), "downloading" | "queued");
            let _ = this.update(cx, |state, cx| {
                state.update_download = Some(job.clone());
                state.feedback = Some("更新下载已启动。".into());
                cx.notify();
            });
            if !active {
                return;
            }
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let poll = {
                    let base = base.clone();
                    let token = token.clone();
                    let job_id = job_id.clone();
                    cx.background_executor()
                        .spawn(async move {
                            get_json(&base, &token, &format!("/api/updates/download/{job_id}"))
                        })
                        .await
                };
                let value = match poll {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = this.update(cx, |state, cx| {
                            state.feedback = Some(error);
                            cx.notify();
                        });
                        break;
                    }
                };
                let job = parse_update_download(&value);
                let done = !matches!(job.status.as_str(), "downloading" | "queued");
                let _ = this.update(cx, |state, cx| {
                    state.update_download = Some(job.clone());
                    if done {
                        state.feedback = Some(if job.status == "ready" {
                            "更新已下载完成。".into()
                        } else {
                            "更新下载已结束。".into()
                        });
                    }
                    cx.notify();
                });
                if done {
                    break;
                }
            }
        })
        .detach();
    });
    });
}

fn cancel_update_download(view: Entity<DevShellWindow>, job_id: String, cx: &mut App) {
    cx.defer(move|cx|{
    let _ = view.update(cx, |state, cx| {
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some("正在取消更新下载…".into());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    delete_json(&base, &token, &format!("/api/updates/download/{job_id}"))
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                match result {
                    Ok(_) => {
                        if let Some(download) = state.update_download.as_mut() {
                            download.status = "cancelled".into();
                        }
                        state.feedback = Some("更新下载已取消。".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
    });
}

fn install_update(view: Entity<DevShellWindow>, job_id: String, cx: &mut App) {
    cx.defer(move|cx|{
    let _ = view.update(cx, |state, cx| {
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some("正在准备安装更新…".into());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_json(
                        &base,
                        &token,
                        "/api/updates/install",
                        serde_json::json!({"id": job_id}),
                    )
                })
                .await;
            match result {
                Ok(_) => {
                    let _ = this.update(cx, |state, cx| {
                        state.feedback = Some("安装助手已启动，窗口即将退出并重启。".into());
                        cx.notify();
                    });
                    process::exit(0);
                }
                Err(error) => {
                    let _ = this.update(cx, |state, cx| {
                        state.feedback = Some(error);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    });
    });
}

fn save_proxy_and_reload(
    base: &str,
    token: &str,
    proxy_id: Option<&str>,
    name: &str,
    kind: &str,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<BackendSnapshot, String> {
    post_json(
        base,
        token,
        "/api/proxies",
        serde_json::json!({
            "id": proxy_id.unwrap_or_default(),
            "name": name,
            "proxy": {
                "type": kind,
                "host": host,
                "port": port,
                "user": user,
                "password": password,
            },
        }),
    )?;
    Ok(BackendSnapshot::load(base, token))
}

fn delete_proxy_and_reload(view: Entity<DevShellWindow>, proxy_id: String, cx: &mut App) {
    cx.defer(move|cx|{
    let _ = view.update(cx, |state, cx| {
        let base = state.snapshot.base.clone();
        let token = state.token.clone();
        state.feedback = Some("正在删除代理配置…".into());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    delete_json(&base, &token, &format!("/api/proxies/{proxy_id}"))?;
                    Ok::<BackendSnapshot, String>(BackendSnapshot::load(&base, &token))
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                match result {
                    Ok(snapshot) => {
                        state.snapshot = snapshot;
                        state.pending_proxy_delete = None;
                        state.feedback = Some("代理配置已删除。".into());
                    }
                    Err(error) => state.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    });
    });
}

fn post_json(base:&str,token:&str,path:&str,body:Value)->Result<Value,String>{json_body_request("POST",base,token,path,body)}
fn json_body_request(method:&str,base: &str, token: &str, path: &str, body: Value) -> Result<Value, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream = TcpStream::connect(authority).map_err(|error| format!("连接失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(if path.ends_with("/upload-check")||path.ends_with("/upload-local"){60}else{12})))
        .map_err(|error| format!("设置超时失败：{error}"))?;
    let body = body.to_string();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
    let response=http::decode_response(&response)?;
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
        .set_read_timeout(Some(Duration::from_secs(if path.ends_with("/upload-check")||path.ends_with("/upload-local"){60}else{12})))
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
    let response=http::decode_response(&response)?;
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
    let response=http::decode_response(&response)?;
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

impl DevShellWindow {
    fn connect_profile(&mut self,server:ServerSummary,cx:&mut Context<Self>) {
        let secret=self.pending_host_key.as_ref().filter(|pending|pending.profile_id==server.id).map(|pending|pending.secret.clone()).unwrap_or_default();
        if secret.is_empty() && needs_credential(&server.raw) {self.pending_credential=Some(server);cx.notify();return;}
        self.connect_profile_with_secret(server,secret,cx);
    }
    fn connect_profile_with_secret(&mut self,server:ServerSummary,secret:String,cx:&mut Context<Self>) {
        if server.protocol=="rdp" {self.connect_rdp(server,secret,cx);return;}
        let protocol=if server.protocol=="serial" {"serial"}else{"ssh"}.to_string();
        let auto_commands=if self.snapshot.appearance["autoCommandsEnabled"].as_bool().unwrap_or(true)&&server.raw["autoRun"].as_bool().unwrap_or(false){server.raw.get("autoCommands").and_then(Value::as_array).map(|a|a.iter().filter_map(Value::as_str).filter(|s|!s.trim().is_empty()).map(ToString::to_string).collect()).unwrap_or_default()}else{Vec::new()};
        let pending_secret=secret.clone();
        let profile_id=server.id.clone();
        let state=self;

                                let base = state.snapshot.base.clone();
                                let token = state.token.clone();
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
                                let request_protocol=protocol.clone();
                                cx.spawn(async move |this, cx| {
                                    let result = cx
                                        .background_executor()
                                        .spawn(
                                            async move {
                                                if request_protocol=="serial" {
                                                    let value=post_json(&base,&token,"/api/serial/sessions",serde_json::json!({"profileId":request_profile_id})).map_err(SessionConnectError::Message)?;
                                                    value.get("id").and_then(Value::as_str).map(ToString::to_string).ok_or_else(||SessionConnectError::Message("串口会话没有返回 ID".into()))
                                                } else {
post_session(
                                                    &base,
                                                    &token,
                                                    &request_profile_id,
                                                    &secret,
                                                    request_approval.as_ref(),
                                                )
                                                }
                                            },
                                        )
                                        .await;
                                    let _ = this.update(cx, |state, cx| {
                                        state.connecting = None;
                                        match result {
                                            Ok(session_id)=>state.attach_terminal_session(server,session_id,protocol,auto_commands,cx),
                                            Err(SessionConnectError::HostKey(challenge)) => {
                                                state.pending_host_key = Some(PendingHostKey {
                                                    profile_id: profile_id.clone(),
                                                    challenge,
                                                    secret:pending_secret,
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

    }
}

fn needs_credential(profile:&Value)->bool {
    let protocol=forms::string(profile,"protocol");let auth=forms::string(profile,"auth");
    protocol!="serial" && !profile["hasSecret"].as_bool().unwrap_or(false) && (protocol=="rdp"||auth=="password"||auth.is_empty()||(auth=="key"&&forms::string(profile,"keyId").is_empty()))
}

fn terminal_input_frames(bytes:&[u8])->Vec<String> {
    bytes.chunks(32768).map(|chunk|serde_json::json!({"type":"input","dataBase64":base64::engine::general_purpose::STANDARD.encode(chunk)}).to_string()).collect()
}

impl DevShellWindow {
    fn find_profile(&self,id:&str)->Option<ServerSummary>{self.snapshot.servers.iter().chain(self.temporary_profiles.iter()).find(|server|server.id==id).cloned()}
    fn attach_terminal_session(&mut self,server:ServerSummary,session_id:String,protocol:String,auto_commands:Vec<String>,cx:&mut Context<Self>){
        let profile_id=server.id.clone();let session_name=server.name.clone();let session_host=if protocol=="serial"{forms::string(&server.raw,"serialPort")}else if protocol=="local"{"本机终端".into()}else{format!("{}@{}:{}",server.user,server.host,server.port)};let state=self;

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
                                                let reader_protocol=protocol.clone();
                                                thread::spawn(move || {
                                                    let event = match terminal_reader(
                                                        &base,
                                                        &token,
                                                        &terminal_id,
                                                        &reader_protocol,
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
                                                if let Some(preview) = &mut state.workspace.preview { preview.text = state.preview_input.read(cx).text().to_string(); }
                                                state.park_session();
                                                if let Some(index)=state.ui.active_start.take() {state.ui.start_tabs.retain(|&id|id!=index);}
                                                state.ui.session_order.push(session_id.clone());
                                                state.ui.connections_visible = false;
                                                let vt_sender=input_tx.clone();
                                                let terminal_view=cx.new(move|cx|ely_gpui_component::terminal::Terminal::remote(120,30,move|event|{
                                                    use ely_gpui_component::terminal::RemoteEvent;
                                                    let input=match event {RemoteEvent::Input(bytes)=>TerminalInput::Bytes(bytes),RemoteEvent::Resize(columns,rows)=>TerminalInput::Resize(columns,rows)};
                                                    let _=vt_sender.send(input);
                                                },cx));
                                                state.workspace.terminal = Some(TerminalState {
                                                    protocol: protocol.clone(),
                                                    profile_id: profile_id.clone(),
                                                    name: session_name,
                                                    host: session_host,
                                                    session_id: session_id.clone(),
                                                    screen: Default::default(),
                                                    status: "连接中".into(),
                                                    input_tx: Some(input_tx),
                                                    view: Some(terminal_view.clone()),
                                                });
                                                let input_session=session_id.clone();cx.subscribe(&terminal_view,move|state,_,event:&ely_gpui_component::terminal::TerminalEvent,cx|{match event{ely_gpui_component::terminal::TerminalEvent::Find=>{state.ui.terminal_search_visible=true;state.ui.terminal_search_focus=true;cx.notify();},ely_gpui_component::terminal::TerminalEvent::Input=>{if let Some(session)=state.session_mut(&input_session){session.integration.at_prompt=false;}},_=>{}}}).detach();
                                                state.workspace.auto_commands=auto_commands;
                                                state.workspace.command_history = state.snapshot.appearance.get("layout").and_then(|layout|layout.get(format!("dengshell.history.{profile_id}"))).and_then(Value::as_array).map(|items|items.iter().filter_map(Value::as_str).map(ToString::to_string).collect()).unwrap_or_default();
                                                let output_session = session_id.clone();
                                                let view = cx.entity().downgrade();
                                                cx.spawn(async move |_, cx| {
                                                    let mut closed = false;
                                                    while !closed {
                                                        if view.upgrade().is_none() { break; }
                                                        while let Ok(event) = receiver.try_recv() {
                                                            let _ = view.update(cx, |state, cx| {
                                                                closed=state.accept_terminal_event(&output_session,event,cx);
                                                                cx.notify();
                                                            });
                                                        }
                                                        let _=view.update(cx,|state,cx|state.advance_auto_commands(&output_session,cx));
                                                        if !closed {
                                                            cx.background_executor()
                                                                .timer(Duration::from_millis(50))
                                                                .await;
                                                        }
                                                    }
                                                })
                                                .detach();
                                                if protocol=="ssh" {
                                                let files_base = state.snapshot.base.clone();
                                                let files_token = state.token.clone();
                                                let files_session = session_id.clone();
                                                let files_result_session = session_id.clone();
                                                let files_view = cx.entity().downgrade();
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
                                                        let Some(session) = state.session_mut(&files_result_session) else { return; };
                                                        session.selected_file = None;
                session.files = Some(match result {
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
                                                let monitor_session = session_id.clone();
                                                let monitor_result_session = session_id.clone();
                                                let transfer_result_session = session_id;
                                                let monitor_view = cx.entity().downgrade();
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
                                                                let Some(session) = state.session_mut(&monitor_result_session) else { return false; };
                                                                session.monitor = Some(match result {
                                                                    Ok(monitor) => {
                                                                        monitor
                                                                    },
                                                                    Err(error) => MonitorState {
                                                                        raw: Value::Null,
                                                                        load: "未知".into(),
                                                                        status: error,
                                                                    },
                                                                });
                                                                cx.notify();
                                                                true
                                                            })
                                                            .unwrap_or(false) == false
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
                                                let transfer_view = cx.entity().downgrade();
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
                                                                let Some(session) = state.session_mut(&transfer_result_session) else { return false; };
                                                                let refresh=if let Ok(transfers) = result {
                                                                    let completed=transfers.iter().any(|t|t.status=="done"&&!session.transfers.iter().any(|old|old.id==t.id&&old.status=="done"));session.transfers=transfers;
                                                                    if completed{session.files.as_ref().map(|f|f.path.clone())}else{None}
                                                                }else{None};
                                                                if let Some(path)=refresh{navigate_files_for(cx.entity(),transfer_result_session.clone(),path,cx);}

                                                                cx.notify();
                                                                true
                                                            })
                                                            .unwrap_or(false) == false
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
    }
}
