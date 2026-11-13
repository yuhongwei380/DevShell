//! Native port of v26.11.3-rc.3's trafficChartModel/trafficSamplesContinuous
//! and DengLatencyChart.{range,curve,model}. Sampling stays in the Go backend.
use super::*;
use crate::{forms::Choice, workspace::Palette};
use gpui_kit::{
    Div,
    component::{
        IndexPath,
        select::{Select, SelectEvent, SelectState},
    },
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Axis {
    pub low: f64,
    pub high: f64,
    pub step: f64,
    pub unit: String,
}
#[derive(Clone, Default)]
pub(super) struct Scale {
    pub axis: Option<Axis>,
    pending: Option<(i64, Axis)>,
}
#[derive(Clone, Debug)]
pub(super) struct Sample {
    pub time: i64,
    pub value: Option<f64>,
    pub other: Option<f64>,
    pub rx_bytes: Option<f64>,
    pub tx_bytes: Option<f64>,
    pub elapsed: Option<f64>,
    pub sequence: Option<u64>,
    pub status: String,
}
#[derive(Clone, Default)]
pub(super) struct Charts {
    pub network: Value,
    pub latency: Value,
    pub network_error: Option<String>,
    pub latency_error: Option<String>,
    pub interface: String,
    pub manual: bool,
    pub scale: Scale,
    pub started: bool,
    pub generation: u64,
    focus_network: Option<FocusHandle>,
    focus_latency: Option<FocusHandle>,
    choices: Option<Entity<SelectState<Vec<Choice>>>>,
    signature: String,
    pub hover: Option<Hover>,
    pub fixed_now: Option<i64>,
    pub process_sort: &'static str,
}
#[derive(Clone)]
pub(super) struct Hover {
    pub point: gpui_kit::Point<gpui_kit::Pixels>,
    pub description: String,
    pub series: &'static str,
    pub index: usize,
}

pub(super) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
fn stamp(value: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|time| time.timestamp_millis())
}
fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite() && *n >= 0.)
}

pub(super) fn latency_axis(values: &[f64]) -> Option<Axis> {
    if values.is_empty() {
        return None;
    }
    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let high = values.iter().copied().fold(0., f64::max);
    if high == 0. {
        return Some(Axis {
            low: 0.,
            high: 1.,
            step: 0.5,
            unit: "ms".into(),
        });
    }
    let span = (high - low).max(high * 0.08).max(0.02);
    let unit = 10f64.powf((span / 2.).log10().floor());
    for factor in [1., 2., 2.5, 5., 10., 20., 25., 50.] {
        let step = unit * factor;
        let base = (((low + high) / 2. - step) / step + 1e-9).floor().max(0.) * step;
        let mut bottoms = [base, base + step];
        bottoms.sort_by(|a, b| {
            (a + step - (low + high) / 2.)
                .abs()
                .total_cmp(&(b + step - (low + high) / 2.).abs())
        });
        for bottom in bottoms {
            if bottom <= low
                && bottom + step * 2. >= high
                && (low != high || bottom < low || low == 0.)
            {
                return Some(Axis {
                    low: bottom,
                    high: bottom + step * 2.,
                    step,
                    unit: "ms".into(),
                });
            }
        }
    }
    Some(Axis {
        low: 0.,
        high,
        step: high / 2.,
        unit: "ms".into(),
    })
}

pub(super) fn traffic_axis(values: &[f64]) -> Option<Axis> {
    if values.is_empty() {
        return None;
    }
    let peak = values.iter().copied().fold(0., f64::max);
    let index = ((peak.max(1.).log(1024.)).floor() as usize).min(3);
    let divisor = 1024f64.powi(index as i32);
    let padded = (peak * 1.1).max(1.) / divisor;
    let base = 10f64.powf(padded.log10().floor());
    let factor = [1., 1.2, 1.5, 2., 2.5, 3., 4., 5., 6., 8., 10.]
        .into_iter()
        .find(|factor| factor * base >= padded)
        .unwrap_or(10.);
    let high = factor * base * divisor;
    Some(Axis {
        low: 0.,
        high,
        step: high / 2.,
        unit: ["B/s", "KiB/s", "MiB/s", "GiB/s"][index].into(),
    })
}
impl Scale {
    pub fn update(&mut self, candidate: Option<Axis>, now: i64) {
        let Some(candidate) = candidate else {
            self.axis = None;
            self.pending = None;
            return;
        };
        if self
            .axis
            .as_ref()
            .is_none_or(|old| candidate.high > old.high)
        {
            self.axis = Some(candidate);
            self.pending = None;
        } else if self
            .axis
            .as_ref()
            .is_some_and(|old| candidate.high < old.high)
        {
            match &mut self.pending {
                None => self.pending = Some((now, candidate)),
                Some((since, pending)) => {
                    if candidate.high > pending.high {
                        *pending = candidate;
                    }
                    if now - *since >= 5000 {
                        self.axis = Some(pending.clone());
                        self.pending = None;
                    }
                }
            }
        } else {
            self.pending = None;
        }
    }
}

pub(super) fn continuous(a: &Sample, b: &Sample, latency: bool) -> bool {
    let gap = b.time - a.time;
    if latency {
        return gap > 0
            && gap <= 1600
            && match (a.sequence, b.sequence) {
                (Some(a), Some(b)) => b == a + 1,
                _ => true,
            };
    }
    if gap <= 0 || gap > 15500 {
        return false;
    }
    let Some(elapsed) = b.elapsed else {
        return gap <= 1800;
    };
    if elapsed <= 0. || elapsed > 15000. {
        return false;
    }
    if let (Some(rx), Some(tx), Some(arx), Some(atx), Some(brx), Some(btx)) = (
        b.value, b.other, a.rx_bytes, a.tx_bytes, b.rx_bytes, b.tx_bytes,
    ) {
        return [(arx, brx, rx), (atx, btx, tx)]
            .iter()
            .all(|&(previous, current, rate)| {
                let delta = current - previous;
                let expected = rate * elapsed / 1000.;
                delta >= 0.
                    && (delta - expected).abs()
                        <= 2f64
                            .max(expected.abs() * 1e-6)
                            .max(previous.max(current) * f64::EPSILON * 2.)
            });
    }
    elapsed <= 3000. && gap as f64 <= elapsed + 500.
}

pub(super) fn network_samples(data: &Value, interface: &str, now: i64) -> Vec<Sample> {
    let mut frames = BTreeMap::new();
    for frame in data["history"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(std::iter::once(data))
    {
        if let Some(time) = stamp(&frame["sampledAt"]) {
            if time > now - 60000 && time <= now {
                let iface = frame["interfaces"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|i| i["name"].as_str() == Some(interface));
                let ready = iface.is_some_and(|i| i["ready"] == true);
                frames.insert(
                    time,
                    Sample {
                        time,
                        value: iface.filter(|_| ready).and_then(|i| finite(&i["rx"])),
                        other: iface.filter(|_| ready).and_then(|i| finite(&i["tx"])),
                        rx_bytes: iface.and_then(|i| finite(&i["rxBytes"])),
                        tx_bytes: iface.and_then(|i| finite(&i["txBytes"])),
                        elapsed: finite(&frame["elapsedMilliseconds"]),
                        sequence: None,
                        status: if ready { "reply" } else { "unavailable" }.into(),
                    },
                );
            }
        }
    }
    frames.into_values().collect()
}
pub(super) fn ping_samples(data: &Value, now: i64) -> Vec<Sample> {
    let mut samples = data["ping"]["samples"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sample| {
            let time = stamp(&sample["sampledAt"])?;
            if time <= now - 60000 || time > now {
                return None;
            }
            let status = sample["status"].as_str().unwrap_or("unavailable");
            Some(Sample {
                time,
                value: if status == "reply" {
                    finite(&sample["milliseconds"])
                } else {
                    None
                },
                other: None,
                sequence: sample["sequence"].as_u64(),
                status: status.into(),
                rx_bytes: None,
                tx_bytes: None,
                elapsed: None,
            })
        })
        .collect::<Vec<_>>();
    samples.sort_by_key(|s| (s.time, s.sequence));
    samples
}

pub(super) fn preferred_interface(interfaces: &[Value]) -> String {
    let priority = |iface: &Value| {
        let name = iface["name"].as_str().unwrap_or("");
        let ethernet = name.starts_with("en") || name.starts_with("eth");
        let down = matches!(
            iface["state"].as_str(),
            Some("down" | "lowerlayerdown" | "notpresent")
        );
        if iface["loopback"] == true || name == "lo" {
            6
        } else if iface["up"] == true && !down {
            if ethernet { 0 } else { 1 }
        } else if !down {
            if ethernet { 2 } else { 3 }
        } else if ethernet {
            4
        } else {
            5
        }
    };
    interfaces
        .iter()
        .min_by(|a, b| {
            (
                priority(a),
                a["default"] != true,
                a["name"].as_str().unwrap_or(""),
            )
                .cmp(&(
                    priority(b),
                    b["default"] != true,
                    b["name"].as_str().unwrap_or(""),
                ))
        })
        .and_then(|v| v["name"].as_str())
        .unwrap_or("")
        .into()
}

/// Monotone cubic tangents from the original: preserve each measured extremum.
pub(super) fn tangents(points: &[(f32, f32)]) -> Vec<f32> {
    if points.len() < 2 {
        return vec![0.; points.len()];
    }
    let slopes = points
        .windows(2)
        .map(|p| (p[1].1 - p[0].1) / (p[1].0 - p[0].0))
        .collect::<Vec<_>>();
    let mut tangents = (0..points.len())
        .map(|i| {
            if i == 0 {
                slopes[0]
            } else if i == points.len() - 1 {
                slopes[i - 1]
            } else if slopes[i - 1] * slopes[i] <= 0. {
                0.
            } else {
                2. / (1. / slopes[i - 1] + 1. / slopes[i])
            }
        })
        .collect::<Vec<_>>();
    for (i, slope) in slopes.iter().enumerate() {
        if *slope == 0. {
            tangents[i] = 0.;
            tangents[i + 1] = 0.;
            continue;
        }
        let a = tangents[i] / slope;
        let b = tangents[i + 1] / slope;
        let length = a.hypot(b);
        if length > 3. {
            tangents[i] = 3. * a / length * slope;
            tangents[i + 1] = 3. * b / length * slope;
        }
    }
    tangents
}

impl DevShellWindow {
    pub(super) fn ensure_monitor_charts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.charts.focus_network.is_none() {
            self.workspace.charts.focus_network = Some(cx.focus_handle());
            self.workspace.charts.focus_latency = Some(cx.focus_handle());
        }
        let interfaces = self.workspace.charts.network["interfaces"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let signature = serde_json::to_string(
            &interfaces
                .iter()
                .map(|i| (&i["name"], &i["state"], &i["default"]))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        let charts = &mut self.workspace.charts;
        if (!charts.manual
            || !interfaces
                .iter()
                .any(|i| i["name"].as_str() == Some(&charts.interface)))
            && !interfaces.is_empty()
        {
            let preferred = preferred_interface(&interfaces);
            if charts.interface != preferred {
                charts.interface = preferred;
                charts.scale = Scale::default();
            }
        }
        if charts.signature != signature || charts.choices.is_none()
            || (!interfaces.is_empty() && charts.choices.as_ref().is_some_and(|choices| {
                choices.read(cx).selected_value().map(String::as_str) != Some(charts.interface.as_str())
            })) {
            charts.signature = signature;
            let selected = interfaces
                .iter()
                .position(|i| i["name"].as_str() == Some(&charts.interface))
                .unwrap_or(0);
            let choices = interfaces
                .iter()
                .map(|i| Choice {
                    value: i["name"].as_str().unwrap_or("").into(),
                    label: format!(
                        "{}{}{}",
                        i["name"].as_str().unwrap_or(""),
                        if i["default"] == true {
                            " · 默认路由"
                        } else {
                            ""
                        },
                        if i["state"] == "down" {
                            " · 关闭"
                        } else {
                            ""
                        }
                    )
                    .into(),
                })
                .collect::<Vec<_>>();
            let choices = if choices.is_empty() {
                vec![Choice {
                    value: String::new(),
                    label: "未连接".into(),
                }]
            } else {
                choices
            };
            let choices =
                cx.new(|cx| SelectState::new(choices, Some(IndexPath::new(selected)), window, cx));
            cx.subscribe_in(
                &choices,
                window,
                |state, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    if let SelectEvent::Confirm(Some(name)) = event {
                        state.workspace.charts.interface = name.clone();
                        state.workspace.charts.manual = true;
                        state.workspace.charts.scale = Scale::default();
                        cx.notify();
                    }
                },
            )
            .detach();
            charts.choices = Some(choices);
        }
        let time = charts.fixed_now.unwrap_or_else(now);
        let points = network_samples(&charts.network, &charts.interface, time);
        let values = points
            .iter()
            .flat_map(|p| [p.value, p.other])
            .flatten()
            .collect::<Vec<_>>();
        charts.scale.update(traffic_axis(&values), time);
        if self.workspace.charts.started || self.snapshot.locked {
            return;
        }
        let Some(id) = self
            .workspace
            .terminal
            .as_ref()
            .filter(|t| t.protocol == "ssh" && t.input_tx.is_some())
            .map(|t| t.session_id.clone())
        else {
            return;
        };
        self.workspace.charts.started = true;
        self.workspace.charts.generation = self.workspace.charts.generation.wrapping_add(1);
        let generation = self.workspace.charts.generation;
        for route in ["network", "latency"] {
            let base = self.snapshot.base.clone();
            let token = self.token.clone();
            let id = id.clone();
            cx.spawn(async move |this, cx| {
                loop {
                    let allowed = this
                        .update(cx, |state, _| {
                            if state.snapshot.locked {
                                if let Some(s) = state.session_mut(&id) {
                                    s.charts.started = false;
                                    s.charts.generation = s.charts.generation.wrapping_add(1);
                                }
                                return false;
                            }
                            state.session_mut(&id).is_some_and(|s| {
                                s.charts.generation == generation
                                    && s.terminal.as_ref().is_some_and(|t| t.input_tx.is_some())
                            })
                        })
                        .unwrap_or(false);
                    if !allowed {
                        break;
                    }
                    let req_base = base.clone();
                    let req_token = token.clone();
                    let req_id = id.clone();
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            get_json(
                                &req_base,
                                &req_token,
                                &format!("/api/sessions/{req_id}/{route}"),
                            )
                        })
                        .await;
                    if this
                        .update(cx, |state, cx| {
                            let Some(s) = state.session_mut(&id) else {
                                return false;
                            };
                            if s.charts.generation != generation {
                                return false;
                            }
                            if route == "network" {
                                match result {
                                    Ok(data) => {
                                        s.charts.network = data;
                                        s.charts.network_error = None;
                                    }
                                    Err(error) => s.charts.network_error = Some(error),
                                }
                            } else {
                                match result {
                                    Ok(data) => {
                                        s.charts.latency = data;
                                        s.charts.latency_error = None;
                                    }
                                    Err(error) => s.charts.latency_error = Some(error),
                                }
                            }
                            cx.notify();
                            true
                        })
                        .unwrap_or(false)
                        == false
                    {
                        break;
                    }
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                }
            })
            .detach();
        }
    }

    pub(super) fn native_monitor_charts(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let c = &self.workspace.charts;
        let time = c.fixed_now.unwrap_or_else(now);
        let network = network_samples(&c.network, &c.interface, time);
        let latency = ping_samples(&c.latency, time);
        let ping = &c.latency["ping"];
        let fresh = stamp(&c.network["sampledAt"]).is_some_and(|t| time - t < 2500)
            && c.network_error.is_none()
            && c.network["sampleError"].as_str().is_none_or(str::is_empty);
        let iface = c.network["interfaces"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|i| i["name"].as_str() == Some(&c.interface));
        let rate = |key: &str| {
            if fresh {
                iface
                    .and_then(|i| finite(&i[key]))
                    .map(format_rate)
                    .unwrap_or("—".into())
            } else {
                "—".into()
            }
        };
        let upload = style(self, "upload", cx);
        let download = style(self, "download", cx);
        let latency_style = style(self, "latency", cx);
        let ping_value = if ping["ready"] == true
            && ping["status"] == "reply"
            && stamp(&ping["sampledAt"]).is_some_and(|t| time - t < 3500)
        {
            finite(&ping["milliseconds"])
                .map(|n| {
                    if n == 0. {
                        "<1 ms".into()
                    } else {
                        format!("{n:.2} ms")
                    }
                })
                .unwrap_or("—".into())
        } else {
            match ping["status"].as_str() {
                Some("timeout") => "回包超时",
                Some("unreachable") => "目标不可达",
                Some("pending") => "等待回包",
                _ => "—",
            }
            .into()
        };
        let range = latency_axis(&latency.iter().filter_map(|s| s.value).collect::<Vec<_>>());
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .px(px(10.))
                    .pt(px(10.))
                    .pb(px(7.))
                    .rounded(px(11.))
                    .bg(gpui_kit::rgb(if self.ui.dark {
                        0x131a21
                    } else {
                        0xf6f9fb
                    }))
                    .child(
                        div()
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(label("网络流量", 10., p.muted))
                            .when_some(c.choices.as_ref(), |d, choices| {
                                d.child(
                                    div().w(px(108.)).h(px(22.)).child(
                                        Select::new(choices)
                                            .h(px(22.))
                                            .px_0()
                                            .appearance(false)
                                            .text_size(px(9.))
                                            .disabled(iface.is_none()),
                                    ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .h(px(20.))
                            .flex()
                            .justify_between()
                            .mt(px(3.))
                            .child(label(format!("— 上行 {}", rate("tx")), 11., upload.0))
                            .child(label(format!("┄ 下行 {}", rate("rx")), 11., download.0)),
                    )
                    .child(graph(
                        "network",
                        network,
                        c.scale.axis.clone(),
                        time,
                        p,
                        vec![upload, download],
                        c.hover.as_ref(),
                        c.focus_network.as_ref(),
                        cx,
                    )),
            )
            .child(
                div()
                    .px(px(10.))
                    .pt(px(8.))
                    .pb(px(6.))
                    .rounded(px(11.))
                    .bg(gpui_kit::rgb(if self.ui.dark {
                        0x131a21
                    } else {
                        0xf6f9fb
                    }))
                    .child(
                        div()
                            .min_h(px(18.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(label("当前延迟", 10., p.muted))
                            .child(
                                label(
                                    ping_value,
                                    14.,
                                    if self.ui.dark { 0xa1cdb7 } else { 0x35684e },
                                )
                                .line_height(px(16.1)),
                            ),
                    )
                    .child(graph(
                        "latency",
                        latency,
                        range,
                        time,
                        p,
                        vec![latency_style],
                        c.hover.as_ref(),
                        c.focus_latency.as_ref(),
                        cx,
                    ))
                    .child(
                        div().flex().items_center().justify_center().gap(px(6.)).child(label("1分钟内丢包率",10.,p.muted)).child(label(
                                if ping["windowReady"] == true
                                    || ping["windowSent"].as_u64().unwrap_or(0) > 0
                                {
                                    ping["windowLossPercent"]
                                        .as_f64()
                                        .map(|v| format!("{v:.2}%"))
                                        .unwrap_or("—".into())
                                } else {
                                    "—".into()
                                },
                            10.,
                            if ping["windowLossPercent"].as_f64().is_some_and(|loss| loss>0.) {if self.ui.dark{0xdf9690}else{0xa33d48}} else {p.muted},
                        ).font_weight(gpui_kit::FontWeight(550.)))
                        .h(px(16.))
                        .mt(px(2.))
                        .text_center(),
                    ),
            )
    }

    pub(super) fn monitor_chart_tooltip(&self, p: Palette, window: &Window) -> Div {
        let h = self.workspace.charts.hover.as_ref().unwrap();
        div()
            .absolute()
            .left(
                (h.point.x - px(70.))
                    .max(px(6.))
                    .min(window.viewport_size().width - px(210.)),
            )
            .top((h.point.y - px(74.)).max(px(6.)))
            .w(px(200.))
            .p(px(10.))
            .rounded(px(10.))
            .border_1()
            .border_color(gpui_kit::rgb(p.border))
            .bg(gpui_kit::rgb(p.surface))
            .shadow_lg()
            .text_color(gpui_kit::rgb(p.text))
            .text_size(px(11.))
            .line_height(px(16.))
            .child(h.description.clone())
    }
}

fn label(value: impl Into<gpui_kit::SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size * 1.5))
        .text_color(gpui_kit::rgb(color))
        .child(value.into())
}
pub(super) fn format_rate(value: f64) -> String {
    for (divisor, unit) in [
        (1073741824., "GiB/s"),
        (1048576., "MiB/s"),
        (1024., "KiB/s"),
    ] {
        if value >= divisor {
            return format!("{:.1} {unit}", value / divisor);
        }
    }
    format!("{value:.0} B/s")
}
fn style(owner: &DevShellWindow, key: &str, cx: &App) -> (u32, f32, bool) {
    let dark = owner.ui.dark;
    let default = match key {
        "upload" => (if dark { 0xf5b36c } else { 0xac5418 }, 2.3, false),
        "download" => (if dark { 0x73d2ec } else { 0x087b99 }, 1.8, true),
        _ => (if dark { 0x9bc4b3 } else { 0x77a88f }, 1.65, false),
    };
    let value = &owner.snapshot.appearance["chartStyles"][key];
    if let Some(f) = owner.form.as_ref().filter(|f| f.kind == "charts") {
        return (
            if f.flag(&format!("{key}Theme")) {
                default.0
            } else {
                appearance::color(&Value::from(f.text(&format!("{key}Color"), cx)))
                    .unwrap_or(default.0)
            },
            f.text(&format!("{key}Width"), cx)
                .parse::<f32>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.5 && *n <= 6.)
                .unwrap_or(default.1),
            key != "latency" && f.selected(&format!("{key}Dashed"), cx) == "dashed",
        );
    }
    (
        appearance::color(&value["color"]).unwrap_or(default.0),
        value["width"]
            .as_f64()
            .filter(|n| *n >= 0.5 && *n <= 6.)
            .unwrap_or(default.1 as f64) as f32,
        key != "latency" && value["dashed"].as_bool().unwrap_or(default.2),
    )
}

fn graph(
    kind: &'static str,
    samples: Vec<Sample>,
    axis: Option<Axis>,
    now: i64,
    p: Palette,
    styles: Vec<(u32, f32, bool)>,
    hover: Option<&Hover>,
    focus: Option<&FocusHandle>,
    cx: &mut Context<DevShellWindow>,
) -> impl IntoElement + use<> {
    let latency = kind == "latency";
    let height = if latency { 54. } else { 65. };
    let top = if latency { 5. } else { 19. };
    let bottom = if latency { 36. } else { 47. };
    let divisor = axis
        .as_ref()
        .map(|a| {
            if latency {
                1.
            } else {
                match a.unit.as_str() {
                    "KiB/s" => 1024.,
                    "MiB/s" => 1048576.,
                    "GiB/s" => 1073741824.,
                    _ => 1.,
                }
            }
        })
        .unwrap_or(1.);
    let labels = axis
        .as_ref()
        .map(|a| {
            [a.high, a.low + a.step, a.low].map(|n| {
                if latency {
                    let precision = ((-a.step.log10()).ceil() as i32 + 1).clamp(0, 6) as usize;
                    trim_number(n, precision)
                } else {
                    trim_number(n / divisor, 3)
                }
            })
        })
        .unwrap_or(["—".into(), "—".into(), "—".into()]);
    let left = labels
        .iter()
        .map(|s| s.len() as f32 * if latency { 5.8 } else { 5.6 } + if latency { 11. } else { 10. })
        .fold(if latency { 29. } else { 35. }, f32::max);
    let unit = axis
        .as_ref()
        .map(|a| a.unit.clone())
        .unwrap_or("B/s".into());
    let has_data = axis.is_some();
    let bounds = std::sync::Arc::new(std::sync::Mutex::new(gpui_kit::Bounds::default()));
    let paint_bounds = bounds.clone();
    let hit_bounds = bounds.clone();
    let samples = std::sync::Arc::new(samples);
    let hit_samples = samples.clone();
    let key_bounds = bounds.clone();
    let key_samples = samples.clone();
    let click_focus = focus.cloned();
    let selected = hover.filter(|h| h.series == kind).map(|h| h.index);
    let moving_samples = samples.clone();
    let mut graph = div()
        .id(format!("monitor-{kind}-graph"))
        .when_some(focus, |d, focus| d.track_focus(focus))
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
            if let Some(focus) = &click_focus {
                window.focus(focus, cx);
            }
        })
        .on_key_down(
            cx.listener(move |state, event: &gpui_kit::KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                if key == "escape" {
                    state.workspace.charts.hover = None;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if !matches!(key, "left" | "right" | "home" | "end") || key_samples.is_empty() {
                    return;
                }
                let last = key_samples.len() - 1;
                let old = state
                    .workspace
                    .charts
                    .hover
                    .as_ref()
                    .filter(|h| h.series == kind)
                    .map(|h| h.index)
                    .unwrap_or(last);
                let index = match key {
                    "home" => 0,
                    "end" => last,
                    "left" => old.saturating_sub(1),
                    _ => old.saturating_add(1).min(last),
                };
                let sample = &key_samples[index];
                let bounds = *key_bounds.lock().unwrap();
                let width = f32::from(bounds.size.width) / crate::ui_scale::current();
                let x = left + (sample.time - (now - 60000)) as f32 / 60000. * (width - 3. - left);
                state.workspace.charts.hover = Some(Hover {
                    point: gpui_kit::point(bounds.origin.x + px(x), bounds.origin.y),
                    description: sample_description(sample, latency),
                    series: kind,
                    index,
                });
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .relative()
        .w_full()
        .h(px(height))
        .mt(px(5.))
        .on_mouse_move(
            cx.listener(move |state, event: &gpui_kit::MouseMoveEvent, _, cx| {
                let bounds = *hit_bounds.lock().unwrap();
                let width = f32::from(bounds.size.width) / crate::ui_scale::current();
                let x = f32::from(event.position.x - bounds.origin.x) / crate::ui_scale::current();
                let positions = hit_samples
                    .iter()
                    .map(|s| left + (s.time - (now - 60000)) as f32 / 60000. * (width - 3. - left))
                    .collect::<Vec<_>>();
                let nearest = positions
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| (**a - x).abs().total_cmp(&(**b - x).abs()));
                if let Some((index, pos)) =
                    nearest.filter(|(_, pos)| (**pos - x).abs() <= 8f32.max(width / 60. * 1.5))
                {
                    let s = &hit_samples[index];
                    let description = sample_description(s, latency);
                    state.workspace.charts.hover = Some(Hover {
                        point: gpui_kit::point(bounds.origin.x + px(*pos), event.position.y),
                        description,
                        series: kind,
                        index,
                    });
                } else {
                    state.workspace.charts.hover = None;
                }
                cx.notify();
            }),
        )
        .on_hover(cx.listener(|state, over: &bool, _, cx| {
            if !over {
                state.workspace.charts.hover = None;
                cx.notify();
            }
        }));
    graph = graph.child(
        gpui_kit::canvas(
            move |bounds, _, _| {
                *paint_bounds.lock().unwrap() = bounds;
            },
            move |bounds, _, window, _| {
                let scale = crate::ui_scale::current();
                let width = f32::from(bounds.size.width) / scale;
                let right = width - 3.;
                let point = |x: f32, y: f32| {
                    gpui_kit::point(bounds.origin.x + px(x), bounds.origin.y + px(y))
                };
                for (index, y) in [top, (top + bottom) / 2., bottom].into_iter().enumerate() {
                    let mut line = gpui_kit::PathBuilder::stroke(px(0.65));
                    if index != 2 {
                        line = line.dash_array(&[px(2.), px(3.)]);
                    }
                    line.move_to(point(left, y));
                    line.line_to(point(right, y));
                    if let Ok(line) = line.build() {
                        window.paint_path(line, gpui_kit::rgba((p.border << 8) | 0x90));
                    }
                }
                let Some(axis) = &axis else {
                    return;
                };
                let x = |t: i64| left + (t - (now - 60000)) as f32 / 60000. * (right - left);
                let y = |v: f64| {
                    bottom - ((v - axis.low) / (axis.high - axis.low)) as f32 * (bottom - top)
                };
                if latency && axis.low > 0. {
                    let mut path = gpui_kit::PathBuilder::stroke(px(0.8));
                    for (i, (x, y)) in [
                        (left - 4., bottom - 2.),
                        (left - 2., bottom - 4.),
                        (left, bottom - 2.),
                        (left + 2., bottom - 4.),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if i == 0 {
                            path.move_to(point(x, y));
                        } else {
                            path.line_to(point(x, y));
                        }
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, gpui_kit::rgb(p.muted));
                    }
                }
                for (series, (color, width, dashed)) in styles.iter().enumerate() {
                    let mut runs = Vec::<Vec<(f32, f32)>>::new();
                    let mut run = Vec::new();
                    let mut previous = None;
                    for sample in samples.iter() {
                        let value = if !latency && series == 0 {
                            sample.other
                        } else {
                            sample.value
                        };
                        if value.is_none()
                            || previous
                                .is_some_and(|previous| !continuous(previous, sample, latency))
                        {
                            if !run.is_empty() {
                                runs.push(std::mem::take(&mut run));
                            }
                        }
                        if let Some(value) = value {
                            run.push((x(sample.time), y(value)));
                        }
                        previous = Some(sample);
                    }
                    if !run.is_empty() {
                        runs.push(run);
                    }
                    for points in runs {
                        if points.len() == 1 {
                            let at = point(points[0].0, points[0].1);
                            window.paint_quad(gpui_kit::fill(
                                gpui_kit::Bounds::new(
                                    at - gpui_kit::point(px(1.5), px(1.5)),
                                    gpui_kit::size(px(3.), px(3.)),
                                ),
                                gpui_kit::rgb(*color),
                            ));
                            continue;
                        }
                        let tangents = tangents(&points);
                        let mut line = gpui_kit::PathBuilder::stroke(px(*width));
                        if *dashed {
                            line = line.dash_array(&[px(5.), px(4.)]);
                        }
                        let mut area = gpui_kit::PathBuilder::fill();
                        line.move_to(point(points[0].0, points[0].1));
                        area.move_to(point(points[0].0, points[0].1));
                        for (i, pair) in points.windows(2).enumerate() {
                            let (a, b) = (pair[0], pair[1]);
                            let dx = (b.0 - a.0) / 3.;
                            for path in [&mut line, &mut area] {
                                path.cubic_bezier_to(
                                    point(b.0, b.1),
                                    point(a.0 + dx, a.1 + tangents[i] * dx),
                                    point(b.0 - dx, b.1 - tangents[i + 1] * dx),
                                );
                            }
                        }
                        area.line_to(point(points.last().unwrap().0, bottom));
                        area.line_to(point(points[0].0, bottom));
                        area.close();
                        if let Ok(area) = area.build() {
                            if !latency {
                                window.paint_path(area, gpui_kit::rgba((*color << 8) | 0x13));
                            } else {
                                window.paint_path(
                                    area,
                                    gpui_kit::linear_gradient(
                                        180.,
                                        gpui_kit::linear_color_stop(
                                            gpui_kit::rgba(
                                                (*color << 8) | if latency { 0x45 } else { 0x19 },
                                            ),
                                            0.,
                                        ),
                                        gpui_kit::linear_color_stop(
                                            gpui_kit::rgba((*color << 8) | 0x06),
                                            1.,
                                        ),
                                    ),
                                );
                            }
                        }
                        if let Ok(line) = line.build() {
                            window.paint_path(line, gpui_kit::rgb(*color));
                        }
                    }
                }
                if latency {
                    for sample in samples.iter().filter(|s| s.value.is_none()) {
                        let at = x(sample.time);
                        let mut mark = gpui_kit::PathBuilder::stroke(px(1.5));
                        if matches!(sample.status.as_str(), "timeout" | "unreachable") {
                            mark.move_to(point(at - 2., bottom - 4.));
                            mark.line_to(point(at + 2., bottom));
                            mark.move_to(point(at - 2., bottom));
                            mark.line_to(point(at + 2., bottom - 4.));
                            if let Ok(mark) = mark.build() {
                                window.paint_path(mark, gpui_kit::rgb(0xd57970));
                            }
                        } else {
                            window.paint_quad(gpui_kit::fill(
                                gpui_kit::Bounds::new(
                                    point(at - 1., bottom - 3.),
                                    gpui_kit::size(px(2.), px(2.)),
                                ),
                                gpui_kit::rgb(p.muted),
                            ));
                        }
                    }
                }
                if let Some(sample) = selected.and_then(|i| moving_samples.get(i)) {
                    let mut cursor =
                        gpui_kit::PathBuilder::stroke(px(0.75)).dash_array(&[px(2.), px(2.)]);
                    cursor.move_to(point(x(sample.time), top));
                    cursor.line_to(point(x(sample.time), bottom));
                    if let Ok(cursor) = cursor.build() {
                        window.paint_path(cursor, gpui_kit::rgb(p.muted));
                    }
                }
            },
        )
        .absolute()
        .inset_0(),
    );
    for (index, value) in labels.into_iter().enumerate() {
        graph = graph.child(
            label(value, 9., p.muted)
                .absolute()
                .left_0()
                .top(px([top, (top + bottom) / 2., bottom][index] - 5.))
                .w(px(left - 8.))
                .text_right(),
        );
    }
    if !latency {
        graph = graph.child(
            label(unit, 8., p.muted)
                .absolute()
                .left_0()
                .top_0()
                .w(px(left - 8.))
                .text_right(),
        );
    }
    if !has_data {
        graph = graph.child(
            label(
                if latency {
                    "等待探测结果"
                } else {
                    "等待流量采样"
                },
                10.,
                p.muted,
            )
            .absolute()
            .left(px(left))
            .right(px(3.))
            .top(px(if latency { 17. } else { 25. }))
            .text_center(),
        );
    }
    graph
        .child(
            label(if latency { "60 秒前" } else { "60秒前" }, 8., p.muted)
                .absolute()
                .left(px(left))
                .top(px(height - 13.)),
        )
        .child(
            label("现在", 8., p.muted)
                .absolute()
                .right(px(3.))
                .top(px(height - 13.)),
        )
}
fn trim_number(value: f64, precision: usize) -> String {
    let value = format!("{value:.precision$}");
    if value.contains('.') {
        value.trim_end_matches('0').trim_end_matches('.').into()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn point(time: i64, value: Option<f64>) -> Sample {
        Sample {
            time,
            value,
            other: value,
            rx_bytes: None,
            tx_bytes: None,
            elapsed: None,
            sequence: None,
            status: "reply".into(),
        }
    }
    #[test]
    fn latency_constant_values_keep_headroom_and_exact_zero_is_valid() {
        let a = latency_axis(&[42., 42.]).unwrap();
        assert!(a.low < 42. && a.high >= 42.);
        assert_eq!(latency_axis(&[0.]).unwrap().high, 1.);
        assert!(latency_axis(&[]).is_none());
    }
    #[test]
    fn real_counter_intervals_allow_slow_delivery_but_never_invent_a_gap() {
        let mut a = point(0, Some(2.));
        a.rx_bytes = Some(100.);
        a.tx_bytes = Some(100.);
        let mut b = point(10000, Some(2.));
        b.elapsed = Some(10000.);
        b.rx_bytes = Some(120.);
        b.tx_bytes = Some(120.);
        assert!(continuous(&a, &b, false));
        b.tx_bytes = Some(119.);
        assert!(continuous(&a, &b, false));
        b.tx_bytes = Some(110.);
        assert!(!continuous(&a, &b, false));
        b.elapsed = None;
        assert!(!continuous(&a, &b, false));
        a.sequence = Some(1);
        b.sequence = Some(3);
        b.time = 1000;
        assert!(!continuous(&a, &b, true));
    }
    #[test]
    fn traffic_axis_expands_immediately_and_shrinks_after_five_seconds() {
        let high = traffic_axis(&[1048576.]).unwrap();
        let low = traffic_axis(&[1024.]).unwrap();
        let mut scale = Scale::default();
        scale.update(Some(high.clone()), 0);
        scale.update(Some(low.clone()), 1);
        scale.update(Some(low.clone()), 5000);
        assert_eq!(scale.axis, Some(high));
        scale.update(Some(low.clone()), 5001);
        assert_eq!(scale.axis, Some(low));
        let peak = traffic_axis(&[1073741824.]).unwrap();
        scale.update(Some(peak.clone()), 5002);
        assert_eq!(scale.axis, Some(peak));
    }
    #[test]
    fn curve_keeps_measured_peaks_and_flat_intervals() {
        let points = [(0., 0.), (1., 10.), (2., 0.), (3., 0.)];
        let tangents = tangents(&points);
        assert_eq!(tangents[1], 0.);
        assert_eq!(tangents[2], 0.);
        assert_eq!(tangents[3], 0.);
        for (i, pair) in points.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            for step in 0..=100 {
                let t = step as f32 / 100.;
                let u = 1. - t;
                let dx = (b.0 - a.0) / 3.;
                let value = u * u * u * a.1
                    + 3. * u * u * t * (a.1 + tangents[i] * dx)
                    + 3. * u * t * t * (b.1 - tangents[i + 1] * dx)
                    + t * t * t * b.1;
                assert!(value >= a.1.min(b.1) - 0.001 && value <= a.1.max(b.1) + 0.001);
            }
        }
    }
    #[test]
    fn unavailable_network_frames_remain_gaps_instead_of_zero_measurements() {
        let data = serde_json::json!({"history":[{"sampledAt":"2026-11-03T00:00:00Z","interfaces":[{"name":"eth0","ready":true,"rx":10,"tx":20}]},{"sampledAt":"2026-11-03T00:00:01Z","interfaces":[]},{"sampledAt":"2026-11-03T00:00:02Z","interfaces":[{"name":"eth0","ready":true,"rx":11,"tx":21}]}]});
        let samples = network_samples(
            &data,
            "eth0",
            chrono::DateTime::parse_from_rfc3339("2026-11-03T00:00:03Z")
                .unwrap()
                .timestamp_millis(),
        );
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[1].value, None);
        assert_eq!(samples[2].value, Some(11.));
    }
    #[test]
    fn interface_choice_follows_link_and_route_metadata_instead_of_rate() {
        let data = serde_json::json!([{"name":"lo","up":true,"rx":99999999},{"name":"eth1","up":true},{"name":"eth0","up":true,"default":true},{"name":"enp2s0","state":"down"}]);
        assert_eq!(preferred_interface(data.as_array().unwrap()), "eth0");
    }
}

fn sample_description(s: &Sample, latency: bool) -> String {
    let clock = chrono::DateTime::from_timestamp_millis(s.time)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default();
    let value = if latency {
        s.value
            .map(|v| {
                if v == 0. {
                    "<1 ms".into()
                } else {
                    format!("{v:.2} ms")
                }
            })
            .unwrap_or_else(|| {
                match s.status.as_str() {
                    "timeout" => "回包超时 · 延迟未知",
                    "unreachable" => "目标不可达 · 延迟未知",
                    "pending" => "等待回包 · 暂不计丢包",
                    _ => "本机探测不可用 · 不计丢包",
                }
                .into()
            })
    } else {
        format!(
            "上行 {}\n下行 {}",
            s.other.map(format_rate).unwrap_or("—".into()),
            s.value.map(format_rate).unwrap_or("—".into())
        )
    };
    format!("{clock}\n{value}")
}
