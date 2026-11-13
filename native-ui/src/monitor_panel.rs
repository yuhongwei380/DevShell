use super::*;
use crate::workspace::{Palette, icon};
use gpui_kit::Div;

fn text(value: impl Into<gpui_kit::SharedString>, size: f32) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size * 1.4))
        .child(value.into())
}
fn bytes(value: f64) -> String {
    for (divisor, suffix) in [(1073741824., "GB"), (1048576., "MB"), (1024., "KB")] {
        if value >= divisor {
            return format!("{:.1} {suffix}", value / divisor);
        }
    }
    format!("{value:.0} B")
}
fn percent(used: f64, total: f64) -> f64 {
    if total > 0. {
        (used / total * 100.).clamp(0., 100.)
    } else {
        0.
    }
}

impl DevShellWindow {
    pub(super) fn original_monitor_sidebar(
        &self,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let monitor = self.workspace.monitor.as_ref();
        let empty = Value::Null;
        let raw = monitor.map(|m| &m.raw).unwrap_or(&empty);
        let get = |key: &str| raw[key].as_str().unwrap_or("—").to_owned();
        let number = |key: &str| workspace::number(raw, key);
        let ready = raw["sampleReady"] == true;
        let short = self.ui.viewport.1 <= 850.;
        let meter_height = if short { 19. } else { 21. };
        let gap = if short { 5. } else { 7. };
        let uptime = if monitor.is_some() {
            format!(
                "{} 天 {} 时",
                (number("uptime") / 86400.) as u64,
                (number("uptime") % 86400. / 3600.) as u64
            )
        } else {
            "—".into()
        };
        let os = get("os");
        let logo = system_logo(&os);
        let sort = if self.workspace.charts.process_sort=="memory" {"memory"} else {"cpu"};
        let source = if sort=="memory" {raw.get("processMemoryTop").unwrap_or(&raw["processes"])} else {&raw["processes"]};
        let mut process = source.as_array().cloned().unwrap_or_default();
        process.sort_by(|a,b|workspace::number(b,sort).total_cmp(&workspace::number(a,sort)));
        div()
            .id("monitor-sidebar")
            .w(px(304.))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .child(
                div()
                    .rounded(px(16.))
                    .bg(gpui_kit::rgb(p.surface))
                    .px(px(15.))
                    .pt(px(if short { 12. } else { 15. }))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(20.))
                            .mb(px(9.))
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .child(icon("cpu", 15.))
                            .child(
                                text("系统信息", 12.).font_weight(gpui_kit::FontWeight::SEMIBOLD),
                            )
                            .child(div().flex_1())
                            .when_some(logo, |d, logo| {
                                d.child(gpui_kit::img(format!("system/{logo}.svg")).size(px(20.)))
                            })
                            .child(
                                text(os, 11.5)
                                    .max_w(px(150.))
                                    .overflow_hidden()
                                    .text_color(gpui_kit::rgb(p.muted)),
                            ),
                    )
                    .child(
                        div()
                            .h(px(14.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_color(gpui_kit::rgb(p.muted))
                            .child(text(format!("运行 {uptime}"), 9.))
                            .child(text(
                                format!("负载 {}", monitor.map(|m| m.load.as_str()).unwrap_or("—")),
                                9.,
                            )),
                    )
                    .child(
                        div()
                            .h(px(14.))
                            .mt(px(5.))
                            .mb(px(8.))
                            .flex()
                            .justify_between()
                            .text_color(gpui_kit::rgb(p.muted))
                            .child(
                                text(get("cpuModel"), 10.)
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden(),
                            )
                            .child(text(
                                if monitor.is_some() {
                                    format!("{} vCPU", number("cores") as u64)
                                } else {
                                    "—".into()
                                },
                                10.,
                            )),
                    )
                    .child(div().h(px(29.)).mt(px(2.)).flex().gap(px(8.)).children(
                        [("IPv4", "serverIPv4"), ("IPv6", "serverIPv6")].map(|(label, key)| {
                            let addresses = raw[key]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", ");
                            let copy = addresses.clone();
                            div()
                                .id(format!("monitor-address-{key}"))
                                .flex_1()
                                .min_w_0()
                                .relative()
                                .rounded(px(7.))
                                .border_1()
                                .border_color(gpui_kit::rgb(p.border))
                                .bg(gpui_kit::rgb(if self.ui.dark {
                                    0x131a21
                                } else {
                                    0xf5f8fa
                                }))
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(move |_, _, cx| {
                                    if !copy.is_empty() {
                                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                            copy.clone(),
                                        ));
                                    }
                                })
                                .child(
                                    text(label, 8.)
                                        .absolute()
                                        .top(px(-6.))
                                        .left(px(7.))
                                        .px(px(3.))
                                        .text_color(gpui_kit::rgb(p.muted))
                                        .bg(gpui_kit::rgb(p.surface)),
                                )
                                .child(
                                    text(
                                        if addresses.is_empty() {
                                            "-".into()
                                        } else {
                                            addresses
                                        },
                                        11.,
                                    )
                                    .min_w_0()
                                    .overflow_hidden(),
                                )
                        }),
                    ))
                    .child(
                        meter(
                            p,
                            "CPU",
                            if ready {
                                format!("{:.1}%", number("cpu"))
                            } else {
                                "—".into()
                            },
                            if ready {
                                format!("{} 核心", number("cores") as u64)
                            } else {
                                "—".into()
                            },
                            number("cpu"),
                            0xc5e0d4,
                            meter_height,
                            27.,
                        )
                        .mt(px(8.)),
                    )
                    .child(
                        meter(
                            p,
                            "内存",
                            if ready {
                                format!(
                                    "{:.1}%",
                                    percent(number("memoryUsed"), number("memoryTotal"))
                                )
                            } else {
                                "—".into()
                            },
                            if ready {
                                format!(
                                    "{} / {}",
                                    bytes(number("memoryUsed")),
                                    bytes(number("memoryTotal"))
                                )
                            } else {
                                "—".into()
                            },
                            percent(number("memoryUsed"), number("memoryTotal")),
                            0xc8ddeb,
                            meter_height,
                            27.,
                        )
                        .mt(px(gap)),
                    )
                    .child(
                        meter(
                            p,
                            "交换",
                            if ready {
                                format!("{:.1}%", percent(number("swapUsed"), number("swapTotal")))
                            } else {
                                "—".into()
                            },
                            if ready {
                                format!(
                                    "{} / {}",
                                    bytes(number("swapUsed")),
                                    bytes(number("swapTotal"))
                                )
                            } else {
                                "—".into()
                            },
                            percent(number("swapUsed"), number("swapTotal")),
                            0xcddfeb,
                            meter_height,
                            27.,
                        )
                        .mt(px(gap)),
                    )
                    .children(
                        raw["disks"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|disk| workspace::number(disk, "total") > 0.)
                            .map(|disk| {
                                let total = workspace::number(disk, "total");
                                let used = workspace::number(disk, "used");
                                let percent = percent(used, total);
                                meter(
                                    p,
                                    disk["path"].as_str().unwrap_or("磁盘"),
                                    format!("{percent:.1}%"),
                                    format!("{} / {}", bytes(used), bytes(total)),
                                    percent,
                                    0xc8ddeb,
                                    meter_height,
                                    54.,
                                )
                                .mt(px(gap))
                            }),
                    )
                    .child(div().h(px(31.)).mt(px(10.)).flex().gap(px(7.)).children(
                        [("↓ 读取", "diskRead", true), ("↑ 写入", "diskWrite", false)].map(
                            |(label, key, read)| {
                                let dark = self.ui.dark;
                                let color = if read {
                                    if dark { 0xa1cbd9 } else { 0x4d8ca3 }
                                } else if dark {
                                    0xdfbf91
                                } else {
                                    0xae8554
                                };
                                let background = if read {
                                    if dark { 0x243e4c } else { 0xeaf4f8 }
                                } else if dark {
                                    0x42382b
                                } else {
                                    0xfaf1e4
                                };
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px(px(8.))
                                    .rounded(px(9.))
                                    .bg(gpui_kit::rgb(background))
                                    .text_color(gpui_kit::rgb(color))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        text(label, 10.)
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD),
                                    )
                                    .child(
                                        text(
                                            if ready {
                                                format!("{}/s", bytes(number(key)))
                                            } else {
                                                "—".into()
                                            },
                                            11.,
                                        )
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD),
                                    )
                            },
                        ),
                    ))
                    .child(self.native_monitor_charts(p, cx).mt(px(8.)))
                    .child(
                        div()
                            .mt(px(10.))
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(12.))
                            .bg(gpui_kit::rgb(if self.ui.dark {
                                0x131a21
                            } else {
                                0xf6f9fb
                            }))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(35.2))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        text("活跃进程", 12.)
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD),
                                    )
                                    .child(
                                        text(
                                            if raw.is_null() {
                                                monitor
                                                    .map(|m| m.status.clone())
                                                    .unwrap_or("等待连接".into())
                                            } else if raw["processSample"]["ready"] == true {
                                                format!("{} 个进程", process.len())
                                            } else {
                                                "采集中…".into()
                                            },
                                            10.,
                                        )
                                        .text_color(gpui_kit::rgb(p.muted)),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(7.))
                                    .h(px(25.))
                                    .flex()
                                    .items_center()
                                    .text_color(gpui_kit::rgb(p.muted))
                                    .children(
                                        [("内存", "memory", 0.31), ("CPU", "cpu", 0.24), ("进程", "name", 0.45)].map(
                                            |(name,key,width)| {
                                                KitButton::new(format!("process-sidebar-{key}")).ghost().p_0().h(px(25.)).w(gpui_kit::relative(width)).text_color(gpui_kit::rgb(p.muted)).child(text(format!("{name}{}",if key==sort {" ↓"} else {""}),10.)).on_click(cx.listener(move|state,_,_,cx|{if key!="name"{state.workspace.charts.process_sort=if state.workspace.charts.process_sort==key {"cpu"}else{key};cx.notify();}}))
                                            },
                                        ),
                                    ),
                            )
                            .children(process.iter().take(5).map(|process| {
                                div()
                                    .h(px(27.))
                                    .flex()
                                    .items_center()
                                    .border_t_1()
                                    .border_color(gpui_kit::rgb(p.border))
                                    .child(
                                        text(bytes(workspace::number(process, "memory")), 11.)
                                            .w(gpui_kit::relative(0.31)),
                                    )
                                    .child(
                                        text(
                                            format!("{:.1}%", workspace::number(process, "cpu")),
                                            11.,
                                        )
                                        .w(gpui_kit::relative(0.24)),
                                    )
                                    .child(
                                        text(
                                            process["name"].as_str().unwrap_or("—").to_owned(),
                                            11.,
                                        )
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden(),
                                    )
                            })),
                    ),
            )
    }
}

fn meter(
    p: Palette,
    name: &str,
    value: String,
    detail: String,
    percent: f64,
    color: u32,
    height: f32,
    label_width: f32,
) -> Div {
    div()
        .h(px(height))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            text(name.to_owned(), 11.)
                .w(px(label_width))
                .flex_shrink_0()
                .text_color(gpui_kit::rgb(p.muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .relative()
                .rounded(px(7.))
                .overflow_hidden()
                .bg(gpui_kit::rgb(if crate::ui_scale::dark() {
                    0x131a21
                } else {
                    0xf2f5f8
                }))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h_full()
                        .w(gpui_kit::relative(percent.clamp(0., 100.) as f32 / 100.))
                        .rounded(px(7.))
                        .bg(if crate::ui_scale::dark() {
                            gpui_kit::rgba((color << 8) | 0x30)
                        } else {
                            gpui_kit::rgb(color)
                        }),
                )
                .child(
                    div()
                        .relative()
                        .size_full()
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .child(text(value, 10.).flex_shrink_0())
                        .child(text(detail, 10.).min_w_0().overflow_hidden()),
                ),
        )
}
fn system_logo(os: &str) -> Option<&'static str> {
    let os = os.trim().to_lowercase();
    if os.is_empty() || os == "—" {
        return None;
    }
    Some(
        [
            ("alma", "almalinux"),
            ("rocky", "rockylinux"),
            ("linux mint", "linuxmint"),
            ("kali", "kalilinux"),
            ("ubuntu", "ubuntu"),
            ("debian", "debian"),
            ("centos", "centos"),
            ("red hat", "redhat"),
            ("rhel", "redhat"),
            ("fedora", "fedora"),
            ("alpine", "alpinelinux"),
            ("arch", "archlinux"),
            ("opensuse", "opensuse"),
            ("suse", "suse"),
            ("sles", "suse"),
        ]
        .into_iter()
        .find(|(prefix, _)| os.starts_with(prefix))
        .map(|(_, logo)| logo)
        .unwrap_or("linux"),
    )
}
