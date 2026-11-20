//! 后端 WebUI v1 调度器控制通道。
//!
//! 仅服务于托盘菜单：向本机后端建立一条短连接至
//! `ws://127.0.0.1:<port>/api/v1/ws?channel=control`，复用后端已有的
//! `instances.list` / `scheduler.start` / `scheduler.stop`。tungstenite 的
//! 客户端请求不带 Origin 头，后端据此判定为免密本机直连，因此无需访问密码。
//!
//! 每条调用独立握手、请求、读响应后关闭；托盘使用频率很低，省去长连接的
//! 心跳/重连状态。分片响应按 v1 约定（首字节 0=还有后续、1=末片）重组。
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    time::{Duration, Instant},
};
use tungstenite::{client::IntoClientRequest, protocol::WebSocketConfig, Message, WebSocket};

/// 单次请求等待响应的总时限，避免后端异常时托盘弹窗无限挂起。
const CONTROL_CALL_TIMEOUT: Duration = Duration::from_secs(3);

/// `instances.list` 返回的最小字段集；其余字段无需关心。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct InstanceInfo {
    pub name: String,
    #[serde(default)]
    pub status: String,
}

impl InstanceInfo {
    #[cfg(target_os = "macos")]
    pub(crate) fn running(&self) -> bool {
        self.status == "running"
    }
}

/// 批量操作结果：成功数 + 失败明细（实例名、错误原因）。
#[derive(Clone, Debug, Serialize)]
pub(crate) struct BatchResult {
    pub succeeded: usize,
    pub failed: Vec<(String, String)>,
}

struct Connection {
    socket: WebSocket<TcpStream>,
    counter: u64,
}

impl Connection {
    fn open(port: u16) -> Result<Self> {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .context("connect backend control channel")?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let request = format!("ws://127.0.0.1:{port}/api/v1/ws?channel=control")
            .into_client_request()
            .context("build backend control request")?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(1024 * 1024))
            .max_frame_size(Some(1024 * 1024));
        let (mut socket, _) = tungstenite::client::client_with_config(request, stream, Some(config))
            .context("backend control handshake failed")?;
        socket
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(500)))?;
        Ok(Self { socket, counter: 0 })
    }

    fn recv_json(&mut self, deadline: Instant) -> Result<Value> {
        let mut chunks: Vec<u8> = Vec::new();
        loop {
            if Instant::now() >= deadline {
                bail!("backend control response timed out");
            }
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    return serde_json::from_str(&text).context("invalid backend control message")
                }
                Ok(Message::Binary(data)) => {
                    let (last, payload) = data
                        .split_first()
                        .context("empty backend control fragment")?;
                    chunks.extend_from_slice(payload);
                    if *last == 1 {
                        return serde_json::from_slice(&chunks)
                            .context("invalid backend control fragments");
                    }
                    if *last != 0 {
                        bail!("invalid backend control fragment marker");
                    }
                }
                Ok(Message::Ping(_)) => self.socket.flush()?,
                Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                Ok(Message::Close(_)) => bail!("backend control connection closed"),
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    continue
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.counter += 1;
        let id = format!("tray-{}", self.counter);
        let request = json!({"v":1, "type":"request", "id":id, "method":method, "params":params});
        self.socket.send(Message::Text(request.to_string().into()))?;
        let deadline = Instant::now() + CONTROL_CALL_TIMEOUT;
        loop {
            let message = self.recv_json(deadline)?;
            if message.get("type").and_then(Value::as_str) != Some("response") {
                continue;
            }
            if message.get("id").and_then(Value::as_str) != Some(id.as_str()) {
                continue;
            }
            if message.get("ok") == Some(&Value::Bool(true)) {
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            let error = message.get("error");
            let code = error
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("UNKNOWN");
            let detail = error
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("");
            bail!("backend control error {code}: {detail}");
        }
    }

    fn list(&mut self) -> Result<Vec<InstanceInfo>> {
        let result = self.call("instances.list", json!({}))?;
        serde_json::from_value(result).context("invalid instances.list result")
    }
}

pub(crate) fn instances(port: u16) -> Result<Vec<InstanceInfo>> {
    Connection::open(port)?.list()
}

pub(crate) fn set_scheduler(port: u16, instance: &str, start: bool) -> Result<()> {
    if instance.is_empty() || instance.len() > 64 {
        return Err(anyhow!("invalid instance name"));
    }
    let mut connection = Connection::open(port)?;
    let method = if start { "scheduler.start" } else { "scheduler.stop" };
    connection.call(method, json!({"instance": instance}))?;
    Ok(())
}

pub(crate) fn set_all(port: u16, start: bool) -> Result<BatchResult> {
    let mut connection = Connection::open(port)?;
    let instances = connection.list()?;
    let method = if start { "scheduler.start" } else { "scheduler.stop" };
    let mut failed = Vec::new();
    let mut succeeded = 0;
    for item in instances {
        match connection.call(method, json!({"instance": item.name})) {
            Ok(_) => succeeded += 1,
            Err(error) => failed.push((item.name, format!("{error:#}"))),
        }
    }
    Ok(BatchResult { succeeded, failed })
}
