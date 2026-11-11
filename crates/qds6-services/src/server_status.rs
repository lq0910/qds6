//! 功能：独立 SSH 后台采集 Linux 基本状态，有限快照、不申请提权或安装软件；时间：2026-11-11；作者：lq。

use crate::ssh::TunnelLease;
use ssh2::Session;
use std::{
    collections::BTreeMap,
    io::Read,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

/// 功能：固定只读命令使用分段协议，不拼接用户输入，限制进程行数；时间：2026-11-11；作者：lq。
const SAMPLE_COMMAND: &str = r#"export LC_ALL=C
if [ ! -r /proc/stat ] || [ ! -r /proc/meminfo ]; then
  printf 'QDS6_UNSUPPORTED\n'; exit 0
fi
printf 'QDS6_UPTIME\n'; cat /proc/uptime
printf 'QDS6_LOAD\n'; cat /proc/loadavg
printf 'QDS6_CPU\n'; head -n 1 /proc/stat
printf 'QDS6_MEMORY\n'; cat /proc/meminfo
printf 'QDS6_NETWORK\n'; cat /proc/net/dev
printf 'QDS6_PROCESSES\n'; ps -eo rss=,pcpu=,comm= --sort=-pcpu 2>/dev/null | head -n 5
printf 'QDS6_END\n'
"#;

/// 功能：进程只保存内存、CPU 和命令名，避免采集命令参数中的凭据；时间：2026-11-11；作者：lq。
#[derive(Clone, Debug)]
pub struct Process {
    pub memory_bytes: u64,
    pub cpu_percent: f64,
    pub command: String,
}

/// 功能：按接口保存累计计数，速率必须由两次实际采样差值获得；时间：2026-11-11；作者：lq。
#[derive(Clone, Debug, Default)]
pub struct Network {
    pub received: u64,
    pub sent: u64,
    pub receive_rate: Option<f64>,
    pub send_rate: Option<f64>,
}

/// 功能：一个服务器的不可变状态快照，缺失速率或 CPU 基线使用 None；时间：2026-11-11；作者：lq。
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub uptime_seconds: f64,
    pub load: [f64; 3],
    pub cpu_percent: Option<f64>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub processes: Vec<Process>,
    pub networks: BTreeMap<String, Network>,
    cpu_total: u64,
    cpu_idle: u64,
}

/// 功能：采集失败保留最后数据但显式标记过期，UI 不将旧值当作实时值；时间：2026-11-11；作者：lq。
#[derive(Clone, Default)]
pub struct State {
    pub snapshot: Option<Snapshot>,
    pub error: Option<String>,
    pub revision: u64,
    pub sampled_at: Option<Instant>,
}

/// 功能：生命周期随服务器标签，释放发送端立即唤醒并停止后台轮询；时间：2026-11-11；作者：lq。
pub struct Monitor {
    state: Arc<Mutex<State>>,
    _stop: mpsc::Sender<()>,
}

impl Monitor {
    /// 功能：独占第三条 SSH 传输，超时四秒，每三秒采样，隧道随采集连接保活；时间：2026-11-11；作者：lq。
    pub fn start(session: Session, tunnel: Option<Arc<TunnelLease>>) -> Self {
        session.set_timeout(4_000);
        let state = Arc::new(Mutex::new(State::default()));
        let worker_state = state.clone();
        let (stop, stopped) = mpsc::channel();
        thread::spawn(move || {
            let _tunnel = tunnel;
            let mut previous = None;
            loop {
                let result = collect(&session, previous.as_ref());
                let failed = result.is_err();
                if let Ok(mut state) = worker_state.lock() {
                    state.revision += 1;
                    match result {
                        Ok(sample) => {
                            previous = Some(sample.clone());
                            state.snapshot = Some(sample);
                            state.sampled_at = Some(Instant::now());
                            state.error = None;
                        }
                        Err(error) => state.error = Some(error),
                    }
                }
                // 功能：断线、拒绝 exec 或不支持 /proc 时停止请求，避免持续干扰服务器；时间：2026-11-11；作者：lq。
                if failed
                    || stopped
                        .recv_timeout(Duration::from_secs(3))
                        .is_err_and(|e| e == mpsc::RecvTimeoutError::Disconnected)
                {
                    break;
                }
            }
        });
        Self { state, _stop: stop }
    }

    /// 功能：只在内存中短暂加锁，不在 UI 线程发起 SSH 请求；时间：2026-11-11；作者：lq。
    pub fn state(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// 功能：有界读取独立 exec 通道，完整结束标志与退出码都通过才发布新值；时间：2026-11-11；作者：lq。
fn collect(session: &Session, previous: Option<&Snapshot>) -> Result<Snapshot, String> {
    let mut channel = session
        .channel_session()
        .map_err(|_| "状态采集通道不可用，请重新连接")?;
    channel
        .exec(SAMPLE_COMMAND)
        .map_err(|_| "服务器拒绝状态采集命令")?;
    let mut bytes = Vec::new();
    channel
        .by_ref()
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "状态采集超时或连接已断开")?;
    if bytes.len() > 128 * 1024 {
        return Err("状态采集结果超过限制".into());
    }
    channel.wait_close().map_err(|_| "状态采集连接已断开")?;
    let output = std::str::from_utf8(&bytes).map_err(|_| "状态采集返回了无效文本")?;
    if channel.exit_status().unwrap_or(-1) != 0 || !output.lines().any(|s| s == "QDS6_END") {
        return Err(if output.contains("QDS6_UNSUPPORTED") {
            "当前服务器暂不支持状态采集（需 Linux /proc）"
        } else {
            "无法读取服务器状态（需允许只读 shell 命令）"
        }
        .into());
    }
    parse(output, previous)
}

/// 功能：严格验证关键数值，兼容无 swap、旧内核 MemAvailable 和接口重置；时间：2026-11-11；作者：lq。
fn parse(output: &str, previous: Option<&Snapshot>) -> Result<Snapshot, String> {
    let mut sample = Snapshot::default();
    let mut section = "";
    let mut memory = BTreeMap::new();
    for line in output.lines() {
        if line.starts_with("QDS6_") {
            section = line;
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        match section {
            "QDS6_UPTIME" => {
                sample.uptime_seconds = fields.first().and_then(|s| s.parse().ok()).unwrap_or(0.)
            }
            "QDS6_LOAD" if fields.len() >= 3 => {
                for (index, value) in fields.iter().take(3).enumerate() {
                    sample.load[index] = value.parse().map_err(|_| "无效系统负载")?;
                }
            }
            "QDS6_CPU" if fields.first() == Some(&"cpu") => {
                let values: Vec<u64> = fields
                    .iter()
                    .skip(1)
                    .take(8)
                    .map(|s| s.parse::<u64>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| "无效 CPU 计数")?;
                if values.len() < 4 {
                    return Err("CPU 计数不完整".into());
                }
                sample.cpu_total = values.iter().copied().sum();
                sample.cpu_idle = values[3] + values.get(4).copied().unwrap_or(0);
            }
            "QDS6_MEMORY" if fields.len() >= 2 => {
                if let Ok(value) = fields[1].parse::<u64>() {
                    memory.insert(
                        fields[0].trim_end_matches(':').to_string(),
                        value.saturating_mul(1024),
                    );
                }
            }
            "QDS6_NETWORK" => {
                if let Some((name, values)) = line.split_once(':') {
                    let values: Vec<_> = values.split_whitespace().collect();
                    if name.trim() != "lo"
                        && values.len() >= 9
                        && let (Ok(received), Ok(sent)) = (values[0].parse(), values[8].parse())
                    {
                        sample.networks.insert(
                            name.trim().chars().take(64).collect(),
                            Network {
                                received,
                                sent,
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            "QDS6_PROCESSES" if fields.len() >= 3 && sample.processes.len() < 5 => {
                if let (Ok(memory), Ok(cpu)) = (fields[0].parse::<u64>(), fields[1].parse::<f64>())
                {
                    sample.processes.push(Process {
                        memory_bytes: memory.saturating_mul(1024),
                        cpu_percent: cpu,
                        command: fields[2..]
                            .join(" ")
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(80)
                            .collect(),
                    });
                }
            }
            _ => {}
        }
    }
    sample.memory_total = *memory.get("MemTotal").ok_or("缺少内存信息")?;
    let available = memory.get("MemAvailable").copied().unwrap_or_else(|| {
        ["MemFree", "Buffers", "Cached", "SReclaimable"]
            .iter()
            .map(|key| memory.get(*key).copied().unwrap_or(0))
            .sum::<u64>()
            .saturating_sub(memory.get("Shmem").copied().unwrap_or(0))
    });
    sample.memory_used = sample.memory_total.saturating_sub(available);
    sample.swap_total = memory.get("SwapTotal").copied().unwrap_or(0);
    sample.swap_used = sample
        .swap_total
        .saturating_sub(memory.get("SwapFree").copied().unwrap_or(0));
    if sample.memory_total == 0
        || sample.cpu_total == 0
        || !sample.uptime_seconds.is_finite()
        || sample.uptime_seconds <= 0.
        || sample.load.iter().any(|n| !n.is_finite() || *n < 0.)
    {
        return Err("服务器状态数据不完整".into());
    }
    if let Some(old) = previous.filter(|old| sample.uptime_seconds > old.uptime_seconds) {
        if let (Some(total), Some(idle)) = (
            sample.cpu_total.checked_sub(old.cpu_total),
            sample.cpu_idle.checked_sub(old.cpu_idle),
        ) && total > 0
            && idle <= total
        {
            sample.cpu_percent = Some((total - idle) as f64 / total as f64 * 100.);
        }
        let elapsed = sample.uptime_seconds - old.uptime_seconds;
        for (name, network) in &mut sample.networks {
            if let Some(before) = old.networks.get(name) {
                network.receive_rate = network
                    .received
                    .checked_sub(before.received)
                    .map(|n| n as f64 / elapsed);
                network.send_rate = network
                    .sent
                    .checked_sub(before.sent)
                    .map(|n| n as f64 / elapsed);
            }
        }
    }
    Ok(sample)
}

// 功能：验证增量采样、首次未知值、零交换分区、接口重置及不完整响应；时间：2026-11-11；作者：lq。
#[cfg(test)]
mod tests {
    use super::*;
    /// 功能：构造固定 proc 文本，不依赖本机系统或真实业务服务器；时间：2026-11-11；作者：lq。
    fn fixture(uptime: u64, cpu: &str, rx: u64) -> String {
        format!(
            "QDS6_UPTIME\n{uptime} 0\nQDS6_LOAD\n0.29 0.13 0.04 1/1 1\nQDS6_CPU\ncpu {cpu}\nQDS6_MEMORY\nMemTotal: 8192 kB\nMemAvailable: 2048 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\nQDS6_NETWORK\n eth0: {rx} 0 0 0 0 0 0 0 {rx} 0 0 0 0 0 0 0\nQDS6_PROCESSES\n 1024 3.7 k3s-server\nQDS6_END\n"
        )
    }
    #[test]
    fn samples_use_deltas_and_handle_reset() {
        let first = parse(&fixture(100, "10 0 10 80 0 0 0 0 99 99", 1000), None).unwrap();
        assert!(first.cpu_percent.is_none());
        assert!(first.networks["eth0"].receive_rate.is_none());
        assert_eq!(first.memory_used, 6144 * 1024);
        assert_eq!(first.swap_used, 0);
        let next = parse(&fixture(103, "20 0 20 160 0 0 0 0", 1600), Some(&first)).unwrap();
        assert_eq!(next.cpu_percent, Some(20.));
        assert_eq!(next.networks["eth0"].receive_rate, Some(200.));
        assert_eq!(next.processes[0].command, "k3s-server");
        let reset = parse(&fixture(106, "30 0 30 240 0 0 0 0", 10), Some(&next)).unwrap();
        assert!(reset.networks["eth0"].receive_rate.is_none());
        assert!(parse("QDS6_END", None).is_err());
        let legacy = fixture(100, "10 0 10 80", 1)
            .replace("MemAvailable: 2048 kB", "MemFree: 1024 kB\nCached: 1024 kB");
        assert_eq!(parse(&legacy, None).unwrap().memory_used, 6144 * 1024);
    }
}
