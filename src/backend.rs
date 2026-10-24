//! 后端进程托管模块：负责 gui.py（AzurPilot WebUI）子进程的完整生命周期。
//! 启动前先清理占用目标端口的残留进程，再以进程组方式拉起 Python 解释器，
//! 并注入 `AP_LAUNCHER_PID` / `ALAS_LAUNCHER_PID` 双标记，便于退出时兜底回收。
//! main.rs 在启动流程中调用 [`ManagedBackend::new_cancellable`] 拉起后端，退出时依赖
//! [`ManagedBackend`] 的 [`Drop`] 实现杀掉遗留子进程，防止进程泄漏。

use std::{
    collections::BTreeSet,
    io::Read as _,
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    thread::sleep,
    time::Duration,
};

use anyhow::{anyhow, Result};
use chrono::Local;
#[cfg(unix)]
use command_group::{CommandGroup, GroupChild as BackendChild};
use serde_json::Value as JsonValue;
use tracing::{info, warn};

#[cfg(windows)]
use crate::backend_process::BackendChild;
use crate::setup::{azurpilot_repo_dir, isolate_python_child_environment, venv_python};
use crate::window_util::CreateNoWindow as _;
use crate::{backend_env, launcher_trust_secret};

/// 等待后端端口就绪的总超时时长，超时后判定启动失败并交由上层恢复。
///
/// gui.py 首次启动可能包含依赖同步、前端构建等耗时步骤，因此留足 5 分钟；
/// 等待期间每 100 毫秒探测一次端口，并同时检查子进程是否已提前退出。
const BACKEND_STARTUP_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// gui.py 在启动失败时写下的日志标记。
const BACKEND_FAILURE_LOG_MARKER: &str = "[GUI] AzurPilot Web服务启动失败：";
/// 读取日志尾部的上限，失败原因位于文件末尾。
const BACKEND_FAILURE_LOG_TAIL_BYTES: u64 = 64 * 1024;

/// 等待后端端口就绪超时的专用错误类型。
///
/// 独立成类型而非普通字符串错误，便于上层用 [`is_backend_startup_timeout`]
/// 精确识别启动超时，并在错误页引导用户手动修复环境。
#[derive(Debug)]
pub(crate) struct BackendStartupTimeout {
    /// 尚未就绪的目标端口，仅用于错误信息展示。
    port: u16,
}

/// 按约定实现 `Display`，让超时错误在上层日志与界面中可读。
impl std::fmt::Display for BackendStartupTimeout {
    /// 输出形如 "Timeout waiting for port 22267 to be ready" 的英文提示。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Timeout waiting for port {} to be ready",
            self.port
        )
    }
}

/// 标记为标准错误类型，使其能被 `anyhow` 的 downcast 机制识别。
impl std::error::Error for BackendStartupTimeout {}

/// 判断任意 `anyhow` 错误是否为本模块产生的启动超时错误。
///
/// 保留专用错误类型的回归检查；超时不再自动重建环境。
#[cfg(test)]
pub(crate) fn is_backend_startup_timeout(error: &anyhow::Error) -> bool {
    error.downcast_ref::<BackendStartupTimeout>().is_some()
}

/// 一次 WebUI 启动的完整参数，来源为 AzurPilot 的 deploy 配置或保守默认值。
///
/// 各字段与 gui.py 的命令行开关一一对应，由 [`WebuiLaunchConfig::args`]
/// 负责翻译成实际的参数列表。
#[derive(Clone, Debug)]
pub struct WebuiLaunchConfig {
    /// WebUI 监听地址，默认 `127.0.0.1`，仅允许本机访问。
    pub host: String,
    /// WebUI 监听端口，默认 22267，与 AzurPilot 上游约定一致。
    pub port: u16,
    /// WebUI 访问密码；缺失时后端以无密码模式运行。
    pub password: Option<String>,
    /// 是否启用 CDN 加速静态资源加载。
    pub cdn: bool,
    /// HTTPS 私钥路径；与 `ssl_cert` 成对配置时以 HTTPS 启动。
    pub ssl_key: Option<String>,
    /// HTTPS 证书路径。
    pub ssl_cert: Option<String>,
    /// 启动后立即执行的 AzurPilot 任务名列表，经 `--run` 透传给 gui.py。
    pub run: Vec<String>,
}

/// `WebuiLaunchConfig` 的构造与命令行翻译逻辑。
impl WebuiLaunchConfig {
    /// 从 AzurPilot 的 deploy 配置中提取 WebUI 参数。
    ///
    /// `config` 为 `None` 或缺少 `Deploy.Webui` 节点时，各字段独立回退到
    /// 默认值，因此即使配置部分损坏也能以保守默认值启动后端。
    pub fn from_deploy_config(config: Option<&JsonValue>) -> Self {
        // 配置路径固定为 config["Deploy"]["Webui"]，任一层缺失都会得到 None；
        // 之后每个字段独立判空并回退默认值，保证部分缺失的配置仍可启动。
        let webui = config
            .and_then(|config| config.get("Deploy"))
            .and_then(|deploy| deploy.get("Webui"));

        Self {
            host: webui
                .and_then(|webui| webui.get("WebuiHost"))
                .and_then(value_as_string)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "127.0.0.1".to_owned()),
            port: webui
                .and_then(|webui| webui.get("WebuiPort"))
                .and_then(value_as_u16)
                .unwrap_or(22267),
            password: webui
                .and_then(|webui| webui.get("Password"))
                .and_then(value_as_string)
                .filter(|value| !value.trim().is_empty()),
            cdn: webui
                .and_then(|webui| webui.get("CDN"))
                .and_then(value_as_bool)
                .unwrap_or(false),
            ssl_key: webui
                .and_then(|webui| webui.get("WebuiSSLKey"))
                .and_then(value_as_string)
                .filter(|value| !value.trim().is_empty()),
            ssl_cert: webui
                .and_then(|webui| webui.get("WebuiSSLCert"))
                .and_then(value_as_string)
                .filter(|value| !value.trim().is_empty()),
            run: webui
                .and_then(|webui| webui.get("Run"))
                .map(value_as_string_list)
                .unwrap_or_default(),
        }
    }

    /// 把配置翻译为传给 Python 解释器的 gui.py 参数列表（不含解释器本身）。
    ///
    /// `--host`/`--port` 恒定存在；其余开关仅在对应字段有值时追加，
    /// 保证未配置的选项不会以空值形式传给后端。
    fn args(&self) -> Vec<String> {
        // 基础参数：gui.py 入口与监听地址、端口，决定 WebUI 的访问位置。
        let mut args = vec![
            "gui.py".to_owned(),
            "--host".to_owned(),
            self.host.clone(),
            "--port".to_owned(),
            self.port.to_string(),
        ];

        if let Some(password) = &self.password {
            args.push("--key".to_owned());
            args.push(password.clone());
        }
        if self.cdn {
            args.push("--cdn".to_owned());
        }
        if let Some(ssl_key) = &self.ssl_key {
            args.push("--ssl-key".to_owned());
            args.push(ssl_key.clone());
        }
        if let Some(ssl_cert) = &self.ssl_cert {
            args.push("--ssl-cert".to_owned());
            args.push(ssl_cert.clone());
        }
        if !self.run.is_empty() {
            args.push("--run".to_owned());
            args.extend(self.run.iter().cloned());
        }

        args
    }
}

/// 把 JSON 值宽松地转换为字符串：字符串原样返回，`null` 视为缺失，
/// 其余类型（数字、布尔等）经 `Display` 字符串化，兼容配置中的异构写法。
fn value_as_string(value: &JsonValue) -> Option<String> {
    if let Some(value) = value.as_str() {
        Some(value.to_owned())
    } else if value.is_null() {
        None
    } else {
        Some(value.to_string())
    }
}

/// 把 JSON 值转换为 `u16` 端口号：优先按数字解析，其次按字符串解析；
/// 超出 `u16` 范围或无法解析时返回 `None`，由调用方回退默认端口。
fn value_as_u16(value: &JsonValue) -> Option<u16> {
    if let Some(value) = value.as_u64() {
        u16::try_from(value).ok()
    } else {
        value.as_str()?.parse::<u16>().ok()
    }
}

/// 把 JSON 值宽松地转换为布尔值：除原生布尔外还接受常见的字符串写法
/// （"true"/"1"/"yes"/"on" 等），其余一律返回 `None` 以便回退默认值。
fn value_as_bool(value: &JsonValue) -> Option<bool> {
    if let Some(value) = value.as_bool() {
        Some(value)
    } else {
        match value.as_str()?.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        }
    }
}

/// 把 JSON 值转换为非空字符串列表：数组逐元素转换并丢弃空串；
/// 非数组值按单个字符串处理，兼容配置里把列表写成单值的情况。
fn value_as_string_list(value: &JsonValue) -> Vec<String> {
    match value {
        JsonValue::Array(values) => values
            .iter()
            .filter_map(value_as_string)
            .filter(|value| !value.trim().is_empty())
            .collect(),
        _ => value_as_string(value)
            .filter(|value| !value.trim().is_empty())
            .into_iter()
            .collect(),
    }
}

/// 托管中的 gui.py 子进程句柄。
///
/// 持有后端进程组，使终止操作能连同 gui.py 派生的孙进程
/// （例如 uvicorn worker）一并结束；子进程的归属另通过 AP / ALAS 双 PID 标记
/// 环境变量在进程表层面标记，供 `Drop` 兜底回收使用。Windows 模拟器由
/// AzurPilot 显式脱离 Job，并清除上述归属标记，从而独立于后端存活。
pub struct ManagedBackend {
    /// 进程组子进程句柄；`None` 表示已被 [`ManagedBackend::terminate`]
    /// 取走或从未成功拉起，此时 `Drop` 不再重复强杀。
    child: Option<BackendChild>,
}

/// `ManagedBackend` 的启动与终止逻辑。
impl ManagedBackend {
    /// 按配置启动 gui.py 并阻塞等待其 WebUI 端口就绪。
    ///
    /// 流程依次为：清理占用目标端口的残留进程、向后端命令注入双名称环境变量、
    /// 以进程组拉起子进程、轮询 TCP 连接直至就绪或超时。超时路径
    /// 会先主动终止刚拉起的子进程再返回错误，避免遗留半死进程。
    ///
    /// # Errors
    ///
    /// 端口清理或进程拉起失败、等待期间子进程提前退出（此时会尝试从
    /// gui 日志提取可读原因）时返回错误；等待超过 [`BACKEND_STARTUP_TIMEOUT`]
    /// 时返回 [`BackendStartupTimeout`]。
    ///
    /// # Panics
    ///
    /// 仅当 `127.0.0.1:{port}` 无法解析为 `SocketAddr` 时 `unwrap` 会 panic；
    /// `u16` 端口拼上固定回环地址恒可解析，该不变量由类型系统保证。
    pub fn new_cancellable(
        config: &WebuiLaunchConfig,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Self> {
        crate::launcher::ensure_not_cancelled(cancel)?;
        kill_processes_using_port(config.port)?;

        // 使用仓库 venv 内的 Python 解释器，避免依赖系统全局 Python 环境。
        let mut command = Command::new(venv_python());
        command.args(config.args());
        // 同时注入 AP / ALAS 的归属标记与信任密钥，兼容新旧 Python 后端。
        backend_env::configure(&mut command, std::process::id(), launcher_trust_secret());
        isolate_python_child_environment(&mut command);
        // 以进程组方式拉起：terminate/drop 时可整组回收，防止孙进程残留。
        #[cfg(unix)]
        let child = command.group().create_no_window().spawn()?;
        #[cfg(windows)]
        let child = BackendChild::spawn(&mut command)?;
        let mut res = Self { child: Some(child) };

        // 就绪判定：对 127.0.0.1:{port} 的 TCP 三次握手成功即认为 WebUI 可用。
        let address = format!("127.0.0.1:{}", config.port).parse().unwrap();
        let start_time = std::time::Instant::now();
        // 每 100 毫秒探测一次；探测间隔同时决定了感知子进程退出的最大延迟。
        while start_time.elapsed() < BACKEND_STARTUP_TIMEOUT {
            crate::launcher::ensure_not_cancelled(cancel)?;
            if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
                return Ok(res);
            }
            // 端口未就绪但子进程已退出时立即报错，避免傻等满超时。
            if let Some(child) = res.child.as_mut() {
                if let Some(status) = child.try_wait()? {
                    return Err(backend_exit_error(config, &status));
                }
            }
            sleep(Duration::from_millis(100));
        }
        // 超时路径：先回收刚拉起的子进程，再向上报告超时，保证不留孤儿进程。
        res.terminate().map_err(|error| {
            anyhow!(
                "Failed to stop timed out backend on port {} before recovery: {error:#}",
                config.port
            )
        })?;
        Err(BackendStartupTimeout { port: config.port }.into())
    }

    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => Ok(None),
        }
    }

    /// 终止托管的 gui.py 进程组并等待其退出。
    ///
    /// Unix 上先向进程组发送 `SIGTERM` 并给予 500 毫秒优雅退出窗口，仍未
    /// 退出才强杀；Windows 没有 SIGTERM 语义，直接强杀。子进程句柄已被取走
    /// 或从未拉起时，返回默认的 `ExitStatus`（视同正常退出）。
    ///
    /// # Errors
    ///
    /// 强杀或回收退出状态失败（例如句柄已失效、等待被中断）时返回错误。
    pub fn terminate(&mut self) -> Result<ExitStatus> {
        // take() 让 self.child 变为 None：无论后续成败，本函数只会执行一次，
        // Drop 也不会再对同一进程重复 kill。
        if let Some(mut child) = self.child.take() {
            #[cfg(unix)]
            {
                use command_group::{Signal, UnixChildExt};
                // 先礼后兵：SIGTERM 让 uvicorn 有机会清理子资源；忽略发送
                // 失败（进程可能已退出），由下方的强杀兜底。
                let _ = child.signal(Signal::SIGTERM);
                let start_time = std::time::Instant::now();
                while start_time.elapsed() < Duration::from_millis(500) {
                    if let Ok(Some(exit_status)) = child.try_wait() {
                        return Ok(exit_status);
                    }
                    sleep(Duration::from_millis(100));
                }
                warn!("gui.py didn't exit, killing it...");
            }
            // 兜底强杀：SIGTERM 窗口超时，或平台本就没有优雅退出语义。
            child.kill()?;
            Ok(child.wait()?)
        } else {
            // 没有可终止的子进程（未启动或已回收），按"正常退出"处理。
            Ok(ExitStatus::default())
        }
    }
}

/// 清理占用指定 TCP 端口的所有进程，并等待端口真正释放。
///
/// AzurPilot 非正常退出时常遗留占用 WebUI 端口的 Python 进程，不清掉会导致
/// 新后端绑定端口失败。扫描失败视为可容忍路径（仅告警后放行），因为端口
/// 本就可能无人占用；同时跳过 PID 0 与启动器自身，避免误杀。
///
/// # Errors
///
/// 当前实现把扫描失败一律降级为告警并返回 `Ok(())`，实际不会向调用方
/// 传递 `Err`；保留 `Result` 签名以便未来收紧失败策略。
fn kill_processes_using_port(port: u16) -> Result<()> {
    // 扫描失败不算致命：端口很可能本来就没被占用，直接放行启动流程。
    let pids = match pids_using_tcp_port(port) {
        Ok(pids) => pids,
        Err(e) => {
            warn!("Unable to scan processes using port {}: {}", port, e);
            return Ok(());
        }
    };
    if pids.is_empty() {
        return Ok(());
    }

    let current_pid = std::process::id();
    let sys = sysinfo::System::new_all();
    // PID 0 是系统空闲进程；current_pid 是启动器自身（自身也可能出现在
    // 监听列表的瞬时快照中），二者都绝不能杀。
    for pid in pids {
        if pid == 0 || pid == current_pid {
            continue;
        }

        let sys_pid = sysinfo::Pid::from_u32(pid);
        match sys.process(sys_pid) {
            Some(process) => {
                info!(
                    "Killing process {} ({}) using configured WebUI port {}",
                    pid,
                    process.name().to_string_lossy(),
                    port
                );
                // sysinfo 的 kill 跨平台一致（Windows TerminateProcess，
                // Unix SIGKILL），无需分支处理。
                if !process.kill() {
                    warn!("Failed to kill process {} using port {}", pid, port);
                }
            }
            None => {
                // 快照建立后目标进程刚好自行退出，无需处理。
                warn!(
                    "Process {} was using port {}, but exited before it could be killed",
                    pid, port
                );
            }
        }
    }

    // 等待端口真正释放后再返回：内核回收监听 socket 有延迟，立即启动
    // 新后端可能绑定失败；5 秒上限防止极端残留拖死启动流程。
    let start_time = std::time::Instant::now();
    while start_time.elapsed() < Duration::from_secs(5) {
        match pids_using_tcp_port(port) {
            Ok(pids) if pids.is_empty() => return Ok(()),
            Ok(_) => sleep(Duration::from_millis(100)),
            Err(e) => {
                warn!("Unable to verify port {} was released: {}", port, e);
                return Ok(());
            }
        }
    }

    warn!("Timed out waiting for port {} to be released", port);
    Ok(())
}

/// 列出当前监听指定 TCP 端口的进程 PID 集合（Windows 实现）。
///
/// 选用 `netstat -ano -p tcp`：Windows 自带、无需第三方依赖；
/// Unix 平台的同名函数改用 `lsof`，见 `#[cfg(unix)]` 版本。
///
/// # Errors
///
/// `netstat` 无法启动（不在 PATH 中）或以非零状态退出时返回错误。
#[cfg(windows)]
fn pids_using_tcp_port(port: u16) -> Result<BTreeSet<u32>> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .create_no_window()
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("netstat failed with status {}", output.status));
    }

    Ok(parse_windows_netstat_pids(&output.stdout, port))
}

/// 解析 `netstat -ano -p tcp` 的输出，返回监听目标端口的所有 PID。
///
/// netstat 的数据行格式为 `协议 本地地址 外部地址 状态 PID`（至少 5 列，
/// 另有表头与分隔行），因此必须同时满足：首列为 `TCP`、状态列为
/// `LISTENING`、本地地址列（第 2 列）的端口与目标一致，此时末列才是
/// 可解析的 PID；`-a` 会列出全部连接，必须靠状态列过滤。
#[cfg(windows)]
fn parse_windows_netstat_pids(output: &[u8], port: u16) -> BTreeSet<u32> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| {
            // netstat 行形如 "TCP    0.0.0.0:22267   0.0.0.0:0  LISTENING  1234"；
            // 表头行、分隔行与非 LISTENING 行都不满足下列条件，直接过滤。
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() < 5
                || !parts[0].eq_ignore_ascii_case("TCP")
                || !parts[3].eq_ignore_ascii_case("LISTENING")
                || !local_address_uses_port(parts[1], port)
            {
                return None;
            }
            // 通过全部校验后，末列即 PID。
            parts.last()?.parse::<u32>().ok()
        })
        .collect()
}

/// 列出当前监听指定 TCP 端口的进程 PID 集合（Unix 实现）。
///
/// 选用 `lsof -nP -iTCP:{port} -sTCP:LISTEN -t`：`-t` 只输出 PID（省去
/// 表头）、`-nP` 跳过主机名与端口名的反解以加速；Windows 上 lsof 不可用，
/// 故同名函数基于 `netstat` 文本解析实现。
///
/// # Errors
///
/// `lsof` 命令无法启动时返回错误；注意 lsof 在"无进程监听该端口"时也会
/// 以非零状态退出且输出为空，这是正常情况，此处返回空集合而非错误。
#[cfg(unix)]
fn pids_using_tcp_port(port: u16) -> Result<BTreeSet<u32>> {
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"])
        .create_no_window()
        .output()?;
    // 非零退出码 + 空 stdout 是 lsof 表示"没有匹配进程"的方式，不是故障。
    if !output.status.success() && output.stdout.is_empty() {
        return Ok(BTreeSet::new());
    }

    // 输出每行一个 PID；空行与无法解析的行直接忽略。
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect())
}

/// 判断 netstat 的"本地地址"列是否指向目标端口。
///
/// 用 `rsplit_once(':')` 从右侧切出端口：IPv6 监听地址形如 `[::]:22267`，
/// 只有从右往左切才能正确越过方括号内的冒号。
#[cfg(windows)]
fn local_address_uses_port(address: &str, port: u16) -> bool {
    address
        .rsplit_once(':')
        .and_then(|(_, port_part)| port_part.parse::<u16>().ok())
        == Some(port)
}

/// 兜底回收：无论 `ManagedBackend` 因何种路径被丢弃，都确保 gui.py 不残留。
///
/// `Drop` 在正常返回、错误传播与 panic 展开等所有路径上都会执行，因此即使
/// 启动中途失败（例如端口等待超时、调用方提前丢弃句柄），已拉起的子进程
/// 也会在此被终止。除直接 `kill` 进程组外，还会扫描全系统进程表，按
/// AP / ALAS 双 PID 标记找出"归属当前启动器、但句柄已随失败路径
/// 丢失"的子进程一并回收，这是防止进程泄漏的最后防线。
impl Drop for ManagedBackend {
    fn drop(&mut self) {
        // child 仍为 Some 说明尚未走过 terminate（terminate 会 take 掉句柄），
        // 直接强杀进程组即可；失败仅告警，不阻断后续的泄漏扫描。
        if let Some(mut child) = self.child.take() {
            match child.kill() {
                Ok(_) => {}
                Err(e) => warn!("Failed to kill gui.py process: {:?}", e),
            }
        }
        // 双归属标记均匹配时才补杀；任一标记被后端清除都视为显式脱离。
        let sys = sysinfo::System::new_all();
        for (pid, process) in sys.processes() {
            if pid.as_u32() != std::process::id()
                && backend_env::is_managed_child(process.environ(), std::process::id())
            {
                process.kill();
            }
        }
    }
}

/// 将子进程异常退出表述为可读原因：优先取 gui.py 写在日志里的失败原因，
/// 取不到时退回端口与退出码。
fn backend_exit_error(config: &WebuiLaunchConfig, status: &ExitStatus) -> anyhow::Error {
    // 非零退出码即 Python 侧的分类错误码（deploy ErrorCodes / gui EXIT_*），
    // 以 AZURPILOT_ERROR 前缀携带，由 launcher::split_error_code 提取。
    let code_prefix = status
        .code()
        .filter(|code| *code != 0)
        .map(|code| format!("AZURPILOT_ERROR:{code}: "))
        .unwrap_or_default();
    match read_backend_failure_reason() {
        Some(reason) => anyhow!("{code_prefix}{reason}（端口 {}，{status}）", config.port),
        None => anyhow!(
            "{code_prefix}后端在端口 {} 就绪前退出：{status}（未能从日志读取失败原因）",
            config.port
        ),
    }
}

/// 读取当日 gui 日志尾部记录的失败原因。日志尚未写出时返回 None，由调用方退回通用提示。
pub(crate) fn read_backend_failure_reason() -> Option<String> {
    // 优先读当日日志；跨零点启动（如启动器挂到次日）当日文件尚未建立时，
    // 退回目录中修改时间最新的一份。
    let log_directory = azurpilot_repo_dir().join("log");
    let today = log_directory.join(format!("{}_gui.txt", Local::now().format("%Y-%m-%d")));
    let log_path = if today.is_file() {
        today
    } else {
        newest_gui_log(&log_directory)?
    };
    read_log_tail(&log_path).and_then(|tail| extract_failure_reason(&tail))
}

/// 取目录中最新的一份 gui 日志；跨零点启动时当日文件尚未建立。
fn newest_gui_log(directory: &Path) -> Option<PathBuf> {
    std::fs::read_dir(directory)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_gui.txt"))
        })
        .max_by_key(|path| path.metadata().and_then(|meta| meta.modified()).ok())
}

/// 日志为 UTF-8，从尾部按字符边界回溯，避免截断多字节字符。
fn read_log_tail(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(BACKEND_FAILURE_LOG_TAIL_BYTES);
    if start > 0 {
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(start)).ok()?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    if start == 0 {
        return Some(text);
    }
    // 起始偏移可能落在字符中间，丢掉不完整的首行。
    Some(text.split_once('\n').map(|(_, tail)| tail.to_owned())?)
}

/// 日志行形如「时间 | 级别 | [GUI] AzurPilot Web服务启动失败：原因」，取最后一次的原因。
fn extract_failure_reason(log_tail: &str) -> Option<String> {
    log_tail
        .lines()
        .rev()
        .find_map(|line| line.split_once(BACKEND_FAILURE_LOG_MARKER))
        .map(|(_, reason)| reason.trim().to_owned())
        .filter(|reason| !reason.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 启动超时阈值应为 5 分钟，覆盖依赖同步等慢启动场景。
    #[test]
    fn backend_startup_timeout_is_five_minutes() {
        assert_eq!(BACKEND_STARTUP_TIMEOUT, Duration::from_secs(5 * 60));
    }

    /// 超时错误可被精确识别，且不会误判其他 `anyhow` 错误。
    #[test]
    fn backend_startup_timeout_is_identified_without_matching_other_errors() {
        let timeout: anyhow::Error = BackendStartupTimeout { port: 22267 }.into();

        assert!(is_backend_startup_timeout(&timeout));
        assert!(!is_backend_startup_timeout(&anyhow!("other startup error")));
    }
    /// 能从真实格式的 gui 日志行中提取失败原因。
    #[test]
    fn failure_reason_is_extracted_from_a_real_gui_log_line() {
        // 与 rich 日志实际落盘格式一致：时间 | 级别 | 消息
        let line = format!(
            "2026-09-21 20:14:40.595 | ERROR | {BACKEND_FAILURE_LOG_MARKER}React 前端构建失败\r"
        );

        assert_eq!(
            extract_failure_reason(&line).as_deref(),
            Some("React 前端构建失败")
        );
    }

    /// 多条失败记录并存时取最后一次（最新）的原因。
    #[test]
    fn failure_reason_uses_the_last_recorded_failure() {
        let tail =
            format!("{BACKEND_FAILURE_LOG_MARKER}旧原因\n{BACKEND_FAILURE_LOG_MARKER}新原因\n");

        assert_eq!(extract_failure_reason(&tail).as_deref(), Some("新原因"));
    }

    /// 无标记行、或标记后原因为空时，均不产生失败原因。
    #[test]
    fn failure_reason_is_absent_without_the_marker() {
        assert!(extract_failure_reason("2026-09-21 20:14:40 | INFO | 普通日志\n").is_none());
        // 标记出现但原因为空时同样不作为原因
        assert!(extract_failure_reason(&format!("{BACKEND_FAILURE_LOG_MARKER}\n")).is_none());
    }

    /// 从文件中部回溯读取后，落在多字节字符中间的不完整首行应被丢弃。
    #[test]
    fn log_tail_drops_the_incomplete_first_line_after_seeking() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("2026-09-21_gui.txt");
        // 超过读取上限，迫使实现从文件中部定位
        let mut content = "x".repeat(BACKEND_FAILURE_LOG_TAIL_BYTES as usize + 64);
        content.push_str("\n第一行完整\n");
        content.push_str(&format!("{BACKEND_FAILURE_LOG_MARKER}依赖同步失败\n"));
        std::fs::write(&path, content).expect("write log");

        let tail = read_log_tail(&path).expect("tail");

        assert!(!tail.starts_with("xxxx"), "首行应从字符边界之后开始");
        assert_eq!(
            extract_failure_reason(&tail).as_deref(),
            Some("依赖同步失败")
        );
    }

    /// 日志文件不存在时返回 `None` 而非报错。
    #[test]
    fn log_tail_is_absent_for_a_missing_file() {
        assert!(read_log_tail(Path::new("does-not-exist_gui.txt")).is_none());
    }
}
