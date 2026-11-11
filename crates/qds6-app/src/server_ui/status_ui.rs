//! 功能：原生服务器状态侧栏，采样值、进程和按网卡流量历史；时间：2026-11-11；作者：lq。

use super::*;
use qds6_services::server_status::{Monitor, State};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

// 功能：真实服务与原生布局验收入口仅在测试构建中可用；时间：2026-11-11；作者：lq。
#[cfg(feature = "ui-tests")]
pub(crate) mod smoke;

/// 功能：每个标签保留最多 40 个流量点，采集线程和 UI 定时器随实体释放；时间：2026-11-11；作者：lq。
pub(super) struct StatusUi {
    pub visible: bool,
    monitor: Option<Monitor>,
    state: State,
    task: Option<Task<()>>,
    interface: String,
    history: BTreeMap<String, VecDeque<(f64, f64)>>,
}

impl Default for StatusUi {
    /// 功能：真实连接默认展开，离线布局样例不触发网络请求；时间：2026-11-11；作者：lq。
    fn default() -> Self {
        Self {
            visible: false,
            monitor: None,
            state: State::default(),
            task: None,
            interface: String::new(),
            history: BTreeMap::new(),
        }
    }
}

/// 功能：以二进制单位统一展示内存和每秒流量；时间：2026-11-11；作者：lq。
fn bytes(value: f64) -> String {
    if value >= 1024. * 1024. * 1024. {
        format!("{:.1} GiB", value / (1024. * 1024. * 1024.))
    } else if value >= 1024. * 1024. {
        format!("{:.1} MiB", value / (1024. * 1024.))
    } else if value >= 1024. {
        format!("{:.1} KiB", value / 1024.)
    } else {
        format!("{value:.0} B")
    }
}

impl ServerWorkspace {
    /// 功能：接收后台监控句柄，仅在快照代次变化时刷新界面；时间：2026-11-11；作者：lq。
    pub(crate) fn attach_monitor(
        &mut self,
        monitor: Result<Monitor, String>,
        cx: &mut Context<Self>,
    ) {
        self.monitoring.visible = true;
        match monitor {
            Ok(monitor) => self.monitoring.monitor = Some(monitor),
            Err(error) => {
                self.monitoring.state.error = Some(format!("状态连接不可用：{error}"));
                return;
            }
        }
        self.monitoring.task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let result = this.update(cx, |this, cx| {
                    let Some(monitor) = &this.monitoring.monitor else {
                        return false;
                    };
                    let state = monitor.state();
                    if state.revision != this.monitoring.state.revision {
                        if state.error.is_none()
                            && let Some(sample) = &state.snapshot
                        {
                            // 功能：接口消失时丢弃历史，重新出现也不连接旧曲线；时间：2026-11-11；作者：lq。
                            this.monitoring
                                .history
                                .retain(|name, _| sample.networks.contains_key(name));
                            for (name, network) in &sample.networks {
                                if let (Some(rx), Some(tx)) =
                                    (network.receive_rate, network.send_rate)
                                {
                                    let history =
                                        this.monitoring.history.entry(name.clone()).or_default();
                                    if history.len() == 40 {
                                        history.pop_front();
                                    }
                                    history.push_back((rx, tx));
                                } else {
                                    this.monitoring.history.remove(name);
                                }
                            }
                            if !sample.networks.contains_key(&this.monitoring.interface) {
                                // 功能：默认选择已有流量的网卡；时间：2026-11-11；作者：lq。
                                this.monitoring.interface = sample
                                    .networks
                                    .iter()
                                    .max_by_key(|(_, n)| n.received.saturating_add(n.sent))
                                    .map(|(name, _)| name.clone())
                                    .unwrap_or_default();
                            }
                        }
                        this.monitoring.state = state;
                        cx.notify();
                    }
                    this.monitoring.state.error.is_none()
                });
                if !matches!(result, Ok(true)) {
                    break;
                }
            }
        }));
    }

    /// 功能：比例条明确显示未知值和零分母，超出范围不会撑破侧栏；时间：2026-11-11；作者：lq。
    fn status_meter(
        &self,
        name: &'static str,
        value: Option<f64>,
        detail: String,
        cx: &App,
    ) -> AnyElement {
        v_flex()
            .gap_1()
            .child(h_flex().justify_between().child(name).child(detail))
            .child(
                div()
                    .h(px(5.))
                    .w_full()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .child(
                        div()
                            .h_full()
                            .rounded_sm()
                            .bg(if value.is_some_and(|v| v >= 85.) {
                                cx.theme().danger
                            } else {
                                cx.theme().primary
                            })
                            .w(relative(
                                (value.unwrap_or(0.).clamp(0., 100.) / 100.) as f32,
                            )),
                    ),
            )
            .into_any_element()
    }

    /// 功能：侧栏滚动独立，不挤压下方文件列表高度，错误时保留值并标为过期；时间：2026-11-11；作者：lq。
    pub(super) fn server_status_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.monitoring.state;
        let mut panel = v_flex()
            .id("server-status-panel")
            .test_support()
            .w(px(248.))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scroll()
            .p_3()
            .gap_2()
            .text_xs()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_sm().font_semibold().child("服务器状态"))
                    .child(
                        div()
                            .text_color(if state.error.is_some() {
                                cx.theme().danger
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(if state.error.is_some() {
                                "已停止"
                            } else if state.snapshot.is_some() {
                                "每 3 秒"
                            } else {
                                "采集中…"
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(self.profile.host.clone()),
                    )
                    .child(
                        Button::new("copy-server-address")
                            .ghost()
                            .xsmall()
                            .icon(UiIcon::Copy)
                            .tooltip("复制服务器地址")
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    this.profile.host.clone(),
                                ))
                            })),
                    ),
            );
        if let Some(error) = &state.error {
            panel = panel
                .child(div().text_color(cx.theme().danger).child(error.clone()))
                .child(div().text_color(cx.theme().muted_foreground).child(
                    if let Some(at) = state.sampled_at {
                        format!(
                            "以下为最后成功数据，已过期 · {} 秒前；重新连接可恢复采集",
                            at.elapsed().as_secs()
                        )
                    } else {
                        "终端和文件可继续使用；重新连接可重试采集".into()
                    },
                ));
        }
        if let Some(sample) = &state.snapshot {
            let uptime = sample.uptime_seconds as u64;
            panel = panel
                .child(div().font_semibold().child("系统信息"))
                .child(div().child(format!(
                    "运行 {} 天 {} 小时 {} 分钟",
                    uptime / 86400,
                    uptime % 86400 / 3600,
                    uptime % 3600 / 60
                )))
                .child(div().child(format!(
                    "负载 {:.2}, {:.2}, {:.2}",
                    sample.load[0], sample.load[1], sample.load[2]
                )))
                .child(
                    self.status_meter(
                        "CPU",
                        sample.cpu_percent,
                        sample
                            .cpu_percent
                            .map(|n| format!("{n:.0}%"))
                            .unwrap_or_else(|| "采样中…".into()),
                        cx,
                    ),
                )
                .child(self.status_meter(
                    "内存",
                    Some(sample.memory_used as f64 / sample.memory_total as f64 * 100.),
                    format!(
                        "{:.0}% · {} / {}",
                        sample.memory_used as f64 / sample.memory_total as f64 * 100.,
                        bytes(sample.memory_used as f64),
                        bytes(sample.memory_total as f64)
                    ),
                    cx,
                ))
                .child(self.status_meter(
                    "交换",
                    if sample.swap_total > 0 {
                        Some(sample.swap_used as f64 / sample.swap_total as f64 * 100.)
                    } else {
                        None
                    },
                    if sample.swap_total == 0 {
                        "未启用".into()
                    } else {
                        format!(
                            "{} / {}",
                            bytes(sample.swap_used as f64),
                            bytes(sample.swap_total as f64)
                        )
                    },
                    cx,
                ))
                .child(div().mt_2().font_semibold().child("进程 · CPU 前 5"))
                .child(
                    h_flex()
                        .gap_2()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().w(px(68.)).child("内存"))
                        .child(div().w(px(38.)).child("CPU%"))
                        .child("命令"),
                );
            for process in &sample.processes {
                panel = panel.child(
                    h_flex()
                        .gap_2()
                        .child(div().w(px(68.)).child(bytes(process.memory_bytes as f64)))
                        .child(
                            div()
                                .w(px(38.))
                                .child(format!("{:.1}", process.cpu_percent)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .child(process.command.clone()),
                        ),
                );
            }
            if sample.processes.is_empty() {
                panel = panel.child("服务器未提供进程列表");
            }
            panel = panel
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("进程 CPU 为 ps 报告的平均占用"),
                )
                .child(div().mt_2().font_semibold().child("网络流量"));
            if sample.networks.is_empty() {
                panel = panel.child("未发现非回环网络接口");
            }
            if !sample.networks.is_empty() {
                let names: Vec<_> = sample.networks.keys().cloned().collect();
                panel = panel.child(
                    Button::new("status-next-interface")
                        .ghost()
                        .xsmall()
                        .label(format!("{} · 切换网卡", self.monitoring.interface))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let current = names
                                .iter()
                                .position(|name| name == &this.monitoring.interface)
                                .unwrap_or(0);
                            this.monitoring.interface = names[(current + 1) % names.len()].clone();
                            cx.notify();
                        })),
                );
            }
            if let Some(network) = sample.networks.get(&self.monitoring.interface) {
                let rate = |v: Option<f64>| {
                    v.map(|n| format!("{}/s", bytes(n)))
                        .unwrap_or_else(|| "采样中…".into())
                };
                panel = panel.child(
                    h_flex()
                        .justify_between()
                        .child(format!("↑ {}", rate(network.send_rate)))
                        .child(format!("↓ {}", rate(network.receive_rate))),
                );
                let history = self.monitoring.history.get(&self.monitoring.interface);
                let max = history
                    .into_iter()
                    .flatten()
                    .map(|(rx, tx)| rx.max(*tx))
                    .fold(1., f64::max);
                let mut chart = h_flex()
                    .h(px(64.))
                    .w_full()
                    .gap(px(1.))
                    .items_end()
                    .border_b_1()
                    .border_color(cx.theme().border);
                // 功能：固定四十点横轴，初始样本从右侧进入而不拉伸成大块；时间：2026-11-11；作者：lq。
                for _ in 0..40 - history.map_or(0, VecDeque::len) {
                    chart = chart.child(div().flex_1());
                }
                for (rx, tx) in history.into_iter().flatten() {
                    chart = chart.child(
                        h_flex()
                            .flex_1()
                            .h_full()
                            .items_end()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px((rx / max * 60.) as f32))
                                    .bg(rgb(0x3b82f6)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px((tx / max * 60.) as f32))
                                    .bg(cx.theme().muted_foreground),
                            ),
                    );
                }
                panel = panel.child(chart).child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("蓝色下载 · 灰色上传 · 最近 40 次采样"),
                );
            }
        }
        panel.into_any_element()
    }
}
