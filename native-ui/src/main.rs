use std::{
    env,
    io::{Read, Write},
    net::TcpStream,
    process,
    time::Duration,
};

use ely_gpui_component::{
    Assets as ElyAssets,
    buttons::Button as ElyButton,
    primitives::FocusScope,
    typography::{Heading, Paragraph},
};
use gpui_kit::{
    App, AppContext, Context, FocusHandle, IntoElement, ParentElement, Render, Styled, Window,
    WindowOptions,
    component::button::{Button as KitButton, ButtonVariants},
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
    name: String,
    host: String,
    port: u16,
}

struct DevShellWindow {
    focus: FocusHandle,
    snapshot: BackendSnapshot,
}

impl Render for DevShellWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.snapshot.clone();
        let focus = self.focus.clone();
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
                                                snapshot
                                                    .servers
                                                    .iter()
                                                    .take(8)
                                                    .cloned()
                                                    .map(server_row),
                                            ),
                                    )
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

fn server_row(server: ServerSummary) -> impl IntoElement {
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
        .child(Paragraph::new(format!("{}:{}", server.host, server.port)))
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
                    DevShellWindow { focus, snapshot }
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
