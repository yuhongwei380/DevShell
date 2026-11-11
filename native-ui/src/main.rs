use std::{
    env,
    io::{Read, Write},
    net::TcpStream,
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
}

#[derive(Clone, Debug)]
struct ServerSummary {
    id: String,
    name: String,
    host: String,
    port: u16,
}

struct DevShellWindow {
    focus: FocusHandle,
    snapshot: BackendSnapshot,
    token: String,
    connecting: Option<String>,
    feedback: Option<String>,
    terminal: Option<TerminalState>,
    command_input: Entity<InputState>,
    file_name_input: Entity<InputState>,
    files: Option<FileListState>,
    monitor: Option<MonitorState>,
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
                                    .on_click(|_, _, _| println!("settings: native view pending")),
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
                                    .child(sidebar_button("设置", false)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .p_8()
                                    .flex()
                                    .flex_col()
                                    .gap_6()
                                    .child(Paragraph::new("原生工作区"))
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
                                            .child(Paragraph::new("最近配置的服务器"))
                                            .children(
                                                snapshot.servers.iter().take(8).cloned().map(
                                                    |server| {
                                                        server_row(
                                                            view.clone(),
                                                            server,
                                                            self.connecting.as_deref(),
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
                                        ))
                                    })
                                    .when_some(self.monitor.clone(), |workspace, monitor| {
                                        workspace.child(monitor_panel(monitor))
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
                                            .child(
                                                ElyButton::new("refresh", "刷新后端状态").on_click(
                                                    |_, _, _| {
                                                        println!("refresh: native view pending")
                                                    },
                                                ),
                                            ),
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
        .child(Paragraph::new(format!(
            "终端 · {} · {}",
            terminal.session_id, terminal.status
        )))
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
                }),
        )
        .children(files.entries.into_iter().take(12).map(|entry| {
            let is_folder = entry.kind == "folder";
            let name = entry.name.clone();
            let target = join_remote_path(&current_path, &name);
            let view = view.clone();
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
                })
                .child(Paragraph::new(entry.owner))
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
) -> impl IntoElement {
    let server_id = server.id.clone();
    let is_connecting = connecting == Some(server.id.as_str());
    let label = if is_connecting {
        "连接中…"
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
                                state.connecting = Some(profile_id.clone());
                                state.feedback = Some("正在连接服务器…".into());
                                cx.notify();
                                cx.spawn(async move |this, cx| {
                                    let result = cx
                                        .background_executor()
                                        .spawn(
                                            async move { post_session(&base, &token, &profile_id) },
                                        )
                                        .await;
                                    let _ = this.update(cx, |state, cx| {
                                        state.connecting = None;
                                        match result {
                                            Ok(session_id) => {
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
                                            }
                                            Err(error) => state.feedback = Some(error),
                                        }
                                        cx.notify();
                                    });
                                })
                                .detach();
                            });
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
                    DevShellWindow {
                        focus,
                        snapshot,
                        token: args.token.clone(),
                        connecting: None,
                        feedback: None,
                        terminal: None,
                        command_input,
                        file_name_input,
                        files: None,
                        monitor: None,
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
        snapshot
    }
}

fn post_session(base: &str, token: &str, profile_id: &str) -> Result<String, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| "后端地址必须使用 http://".to_string())?;
    let mut stream = TcpStream::connect(authority).map_err(|error| format!("连接失败：{error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(25)))
        .map_err(|error| format!("设置超时失败：{error}"))?;
    let body = serde_json::json!({"profileId": profile_id, "secret": ""}).to_string();
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
                .unwrap_or("未知");
            return Err(format!("需要确认 SSH 主机指纹：{fingerprint}"));
        }
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or(status)
            .to_string());
    }
    value
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| "连接成功但后端没有返回会话 ID".into())
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
