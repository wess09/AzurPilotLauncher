//! 反向控制流模块：接收 WebUI 后端经 SSE 下发的启动器控制指令并回报结果。
//!
//! 后端通过 `http://127.0.0.1:{port}/api/launcher/stream` 推送指令
//! （查询/设置开机自启等），本模块在独立线程中维持长连接、解析 SSE 事件、
//! 执行指令并把执行结果 POST 回 `/api/launcher/report`，形成命令-应答闭环。
//! `allow_exit` 标志用于在启动器关闭流程中优雅终止此线程。

use std::{
    io::{BufRead, BufReader},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::autostart;

/// 后端下发的单条启动器控制指令。
///
/// `type` 为指令类型（如 `startup.query` / `startup.set`），
/// `payload` 为可选的指令参数；`id` 用于应答时与请求配对。
#[derive(Debug, Deserialize)]
struct LauncherCommand {
    id: String,
    #[serde(rename = "type")]
    command_type: String,
    payload: Option<Value>,
}

/// 启动反向控制流的后台线程，持续接收并处理启动器控制指令。
///
/// 线程内部带 3 秒间隔的自动重连：连接异常断开时只要启动器尚未进入
/// 退出流程（`allow_exit` 为 false）就会重新连接，保证指令通道可用。
/// `allow_exit` 由启动器的退出流程置位，用于让此线程尽快结束。
pub fn start_launcher_control_stream(
    port: u16,
    allow_exit: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let stream_url = format!("http://127.0.0.1:{port}/api/launcher/stream");
        let report_url = format!("http://127.0.0.1:{port}/api/launcher/report");
        // 显式禁用系统代理并限制连接超时：控制流是本机回环通信，
        // 走代理既无意义，还可能因系统代理故障而中断
        let client = match Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(None)
            .no_proxy()
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                warn!("Unable to create launcher control client: {e}");
                return;
            }
        };

        while !allow_exit.load(Ordering::SeqCst) {
            info!("Connecting to launcher control stream: {stream_url}");
            match read_launcher_control_stream(&client, &stream_url, &report_url, &allow_exit) {
                Ok(()) => debug!("Launcher control stream ended"),
                Err(e) => warn!("Launcher control stream disconnected: {e}"),
            }

            // 断开后稍作退避再重连；启动器已进入退出流程时立即结束循环
            if !allow_exit.load(Ordering::SeqCst) {
                for _ in 0..30 {
                    if allow_exit.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    })
}

/// 建立一次 SSE 长连接并循环读取事件，直到流结束或启动器开始退出。
///
/// 按 SSE 规范解析：`data:` 前缀行累积为事件数据，空行表示一个事件
/// 结束并触发分发。函数在流正常结束（对端关闭、读到 0 字节）或
/// `allow_exit` 置位时返回。
///
/// # Errors
/// 连接失败、非 2xx 响应或读取流时发生 IO 错误时返回 Err，
/// 由调用方记录日志后重连。
fn read_launcher_control_stream(
    client: &Client,
    stream_url: &str,
    report_url: &str,
    allow_exit: &Arc<AtomicBool>,
) -> Result<()> {
    let response = crate::cancel::request(
        client.get(stream_url).header(ACCEPT, "text/event-stream"),
        allow_exit,
    )?;

    if !response.status().is_success() {
        return Err(anyhow!("server returned {}", response.status()));
    }

    let mut reader = BufReader::new(crate::cancel::body(response, allow_exit.clone()));
    let mut data_lines = Vec::new();

    while !allow_exit.load(Ordering::SeqCst) {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }

        let line = line.trim_end_matches(['\r', '\n']);
        // SSE 协议：空行标志一个事件的边界，把此前累积的 data 行整体分发
        if line.is_empty() {
            if !allow_exit.load(Ordering::SeqCst) {
                dispatch_sse_data(&mut data_lines, client, report_url);
            }
            continue;
        }

        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_owned());
        }
    }

    // 流结束时若仍有未分发的残留事件（缺空行结尾），补一次分发避免丢失
    if !allow_exit.load(Ordering::SeqCst) {
        dispatch_sse_data(&mut data_lines, client, report_url);
    }
    Ok(())
}

/// 把累积的一批 SSE data 行合并为完整事件并交给指令处理器。
///
/// 多行 data 按 SSE 规范用换行符拼接；非 JSON 格式的事件只记录
/// 警告并丢弃，不中断连接。
fn dispatch_sse_data(data_lines: &mut Vec<String>, client: &Client, report_url: &str) {
    if data_lines.is_empty() {
        return;
    }

    let data = data_lines.join("\n");
    data_lines.clear();

    match serde_json::from_str::<LauncherCommand>(&data) {
        Ok(command) => handle_command(client, report_url, command),
        Err(e) => warn!("Ignoring invalid launcher command: {e}; payload={data}"),
    }
}

/// 执行单条控制指令，并把执行结果回报给后端。
///
/// 目前支持 `startup.query`（查询开机自启状态）与 `startup.set`
/// （设置开机自启，payload 需含 `enabled` 布尔字段）；未知指令
/// 返回错误应答而不是 panic，保证流通道不受个别坏指令影响。
/// 回报失败只记警告——指令本身已执行，后端有超时兜底。
fn handle_command(client: &Client, report_url: &str, command: LauncherCommand) {
    let report = match command.command_type.as_str() {
        "startup.query" => match autostart::query() {
            Ok(status) => success_report(&command.id, status),
            Err(e) => error_report(&command.id, e.to_string()),
        },
        "startup.set" => {
            let enabled = command
                .payload
                .as_ref()
                .and_then(|payload| payload.get("enabled"))
                .and_then(Value::as_bool);
            match enabled {
                Some(enabled) => match autostart::set_enabled(enabled) {
                    Ok(status) => success_report(&command.id, status),
                    Err(e) => error_report(&command.id, e.to_string()),
                },
                None => error_report(&command.id, "missing enabled".to_owned()),
            }
        }
        other => error_report(&command.id, format!("unknown command: {other}")),
    };

    if let Err(e) = client
        .post(report_url)
        .timeout(Duration::from_secs(5))
        .header(CONTENT_TYPE, "application/json")
        .body(report.to_string())
        .send()
    {
        warn!("Failed to report launcher command result: {e}");
    }
}

/// 构造执行成功的应答 JSON，`data` 为指令的业务结果。
fn success_report<T: serde::Serialize>(id: &str, data: T) -> Value {
    json!({
        "id": id,
        "success": true,
        "data": data,
    })
}

/// 构造执行失败的应答 JSON，`error` 为可读的错误描述。
fn error_report(id: &str, error: String) -> Value {
    json!({
        "id": id,
        "success": false,
        "error": error,
    })
}
