//! 单条本机 WS 承载命令、通知、状态和免密票据；不使用系统代理。
use crate::launcher_protocol::{valid_id, valid_ticket, AutostartParams, Incoming};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tracing::{debug, warn};
use tungstenite::{client::IntoClientRequest, protocol::WebSocketConfig, Message, WebSocket};

pub trait ChannelHandler: Send + Sync + 'static {
    fn autostart(&self, enabled: Option<bool>) -> Result<Value>;
    fn notification(&self, data: Value) -> Result<()>;
}

struct Outgoing {
    id: String,
    method: &'static str,
    reply: mpsc::Sender<Result<Value>>,
    deadline: Instant,
}

pub struct LauncherChannel {
    sender: mpsc::SyncSender<Outgoing>,
    stop: Arc<AtomicBool>,
    socket: Mutex<Option<TcpStream>>,
    counter: AtomicU64,
}

impl LauncherChannel {
    pub fn login_ticket(&self) -> Result<String> {
        let (reply, receiver) = mpsc::channel();
        let request = Outgoing {
            id: format!("launcher-{}", self.counter.fetch_add(1, Ordering::Relaxed)),
            method: "login.issue",
            reply,
            deadline: Instant::now() + Duration::from_secs(3),
        };
        if self.stop.load(Ordering::SeqCst) {
            bail!("launcher session stopped");
        }
        self.sender
            .try_send(request)
            .context("launcher request queue unavailable")?;
        let result = receiver
            .recv_timeout(Duration::from_secs(3))
            .context("launcher login timed out")??;
        let ticket = result
            .get("ticket")
            .and_then(Value::as_str)
            .filter(|value| valid_ticket(value))
            .context("invalid login ticket")?;
        Ok(ticket.to_owned())
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(socket) = self.socket.lock().unwrap().as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

pub fn start_launcher_channel(
    port: u16,
    secret: String,
    stop: Arc<AtomicBool>,
    handler: Arc<dyn ChannelHandler>,
) -> (Arc<LauncherChannel>, thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::sync_channel(32);
    let channel = Arc::new(LauncherChannel {
        sender,
        stop,
        socket: Mutex::new(None),
        counter: AtomicU64::new(1),
    });
    let worker = channel.clone();
    let thread = thread::spawn(move || {
        let mut attempts = 0usize;
        let mut notifications = VecDeque::new();
        let mut completed = VecDeque::new();
        while !worker.stop.load(Ordering::SeqCst) {
            let started = Instant::now();
            let result = run_connection(
                port,
                &secret,
                &worker,
                &receiver,
                handler.as_ref(),
                &mut notifications,
                &mut completed,
            );
            *worker.socket.lock().unwrap() = None;
            if worker.stop.load(Ordering::SeqCst) {
                break;
            }
            if let Err(error) = result {
                warn!("Launcher WS disconnected: {error}");
            }
            if started.elapsed() >= Duration::from_secs(45) {
                attempts = 0;
            }
            let delay = [1, 2, 5, 10, 30][attempts.min(4)];
            attempts += 1;
            for _ in 0..delay * 10 {
                if worker.stop.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        while let Ok(request) = receiver.try_recv() {
            let _ = request.reply.send(Err(anyhow!("launcher session stopped")));
        }
        debug!("Launcher WS worker stopped");
    });
    (channel, thread)
}

fn send(socket: &mut WebSocket<TcpStream>, message: Value) -> Result<()> {
    socket.send(Message::Text(message.to_string().into()))?;
    Ok(())
}

fn request(id: &str, method: &str, params: Value) -> Value {
    json!({"type":"request", "id":id, "method":method, "params":params})
}

fn failure(id: &str, code: &str, message: &str) -> Value {
    json!({"type":"response", "id":id, "ok":false, "error":{"code":code,"message":message}})
}

fn run_connection(
    port: u16,
    secret: &str,
    channel: &LauncherChannel,
    receiver: &mpsc::Receiver<Outgoing>,
    handler: &dyn ChannelHandler,
    notifications: &mut VecDeque<String>,
    completed: &mut VecDeque<(String, String, Value)>,
) -> Result<()> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    *channel.socket.lock().unwrap() = Some(stream.try_clone()?);
    if channel.stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut upgrade = format!("ws://127.0.0.1:{port}/api/v2/launcher/ws").into_client_request()?;
    upgrade
        .headers_mut()
        .insert("Authorization", format!("Bearer {secret}").parse()?);
    let config = WebSocketConfig::default()
        .max_message_size(Some(65536))
        .max_frame_size(Some(65536));
    // 握手错误只记录状态码，不格式化请求（请求头包含信任密钥）。
    let (mut socket, _) = tungstenite::client::client_with_config(upgrade, stream, Some(config))
        .map_err(|_| anyhow!("launcher WS handshake failed"))?;
    socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(250)))?;
    let mut state = handler
        .autostart(None)
        .unwrap_or(json!({"supported":false,"enabled":null}));
    let capabilities = if state["supported"] == true {
        vec!["autostart", "notifications", "login"]
    } else {
        vec!["notifications", "login"]
    };
    let platform = if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    send(
        &mut socket,
        request(
            "hello",
            "session.hello",
            json!({
                "protocolVersion":2, "launcherVersion":env!("CARGO_PKG_VERSION"),
                "platform":platform, "capabilities":capabilities, "autostart":state,
            }),
        ),
    )?;
    let mut ready = false;
    let mut last_received = Instant::now();
    let mut heartbeat = Instant::now();
    let mut pending: HashMap<String, Outgoing> = HashMap::new();
    let result = (|| -> Result<()> {
        while !channel.stop.load(Ordering::SeqCst) {
            if !ready && last_received.elapsed() > Duration::from_secs(5) {
                bail!("launcher hello timed out");
            }
            if last_received.elapsed() > Duration::from_secs(45) {
                bail!("launcher heartbeat timed out");
            }
            if ready {
                while let Ok(outgoing) = receiver.try_recv() {
                    if outgoing.deadline <= Instant::now() {
                        continue;
                    }
                    send(
                        &mut socket,
                        request(&outgoing.id, outgoing.method, json!({})),
                    )?;
                    pending.insert(outgoing.id.clone(), outgoing);
                }
                pending.retain(|_, outgoing| {
                    if outgoing.deadline > Instant::now() {
                        true
                    } else {
                        let _ = outgoing
                            .reply
                            .send(Err(anyhow!("launcher request timed out")));
                        false
                    }
                });
                if heartbeat.elapsed() >= Duration::from_secs(15) {
                    let id = format!("ping-{}", channel.counter.fetch_add(1, Ordering::Relaxed));
                    send(&mut socket, request(&id, "system.ping", json!({})))?;
                    if let Ok(current) = handler.autostart(None) {
                        if current != state {
                            state = current;
                            let id = format!(
                                "state-{}",
                                channel.counter.fetch_add(1, Ordering::Relaxed)
                            );
                            send(
                                &mut socket,
                                request(&id, "state.report", json!({"autostart":state})),
                            )?;
                        }
                    }
                    heartbeat = Instant::now();
                }
            }
            let message = match socket.read() {
                Ok(message) => message,
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    continue
                }
                Err(error) => return Err(error.into()),
            };
            last_received = Instant::now();
            match message {
                Message::Text(text) => {
                    let incoming: Incoming =
                        serde_json::from_str(&text).context("invalid launcher WS message")?;
                    match incoming {
                        Incoming::Response {
                            id,
                            ok,
                            result,
                            error,
                        } => {
                            if id == "hello" {
                                if !ok
                                    || result
                                        .as_ref()
                                        .and_then(|v| v.get("protocolVersion"))
                                        .and_then(Value::as_u64)
                                        != Some(2)
                                {
                                    bail!("launcher protocol rejected");
                                }
                                ready = true;
                            } else if let Some(outgoing) = pending.remove(&id) {
                                let result = if ok && error.is_none() {
                                    result.context("missing launcher result")
                                } else {
                                    Err(error
                                        .map(|error| anyhow!("{}: {}", error.code, error.message))
                                        .unwrap_or_else(|| anyhow!("launcher request rejected")))
                                };
                                let _ = outgoing.reply.send(result);
                            }
                        }
                        Incoming::Request {
                            id,
                            method,
                            params,
                            deadline,
                        } => {
                            if !ready || !valid_id(&id) {
                                bail!("invalid launcher request");
                            }
                            let fingerprint = format!("{method}:{params}");
                            if let Some((_, previous, response)) =
                                completed.iter().find(|(key, _, _)| key == &id)
                            {
                                send(
                                    &mut socket,
                                    if previous == &fingerprint {
                                        response.clone()
                                    } else {
                                        failure(&id, "DUPLICATE_REQUEST", "request ID reused")
                                    },
                                )?;
                                continue;
                            }
                            let expired = deadline
                                .map(|deadline| {
                                    deadline
                                        <= SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs_f64()
                                })
                                .unwrap_or(false);
                            let response = if expired {
                                failure(&id, "TIMEOUT", "command expired")
                            } else {
                                execute(handler, &id, &method, params)
                            };
                            send(&mut socket, response.clone())?;
                            completed.push_back((id, fingerprint, response));
                            if completed.len() > 128 {
                                completed.pop_front();
                            }
                        }
                        Incoming::Event { id, topic, data } => {
                            if !ready || !valid_id(&id) || topic != "notification" {
                                bail!("invalid launcher event");
                            }
                            if !notifications.contains(&id) {
                                handler.notification(data)?;
                                notifications.push_back(id.clone());
                                if notifications.len() > 128 {
                                    notifications.pop_front();
                                }
                            }
                            let request_id =
                                format!("ack-{}", channel.counter.fetch_add(1, Ordering::Relaxed));
                            send(
                                &mut socket,
                                request(&request_id, "notifications.ack", json!({"ids":[id]})),
                            )?;
                        }
                    }
                }
                Message::Ping(_) => {
                    socket.flush()?;
                }
                Message::Close(_) => break,
                Message::Pong(_) => {}
                _ => bail!("unsupported launcher WS message"),
            }
        }
        let _ = socket.close(None);
        Ok(())
    })();
    for (_, outgoing) in pending {
        let _ = outgoing.reply.send(Err(anyhow!("launcher disconnected")));
    }
    result
}

fn execute(handler: &dyn ChannelHandler, id: &str, method: &str, params: Value) -> Value {
    let result = match method {
        "autostart.get" if params == json!({}) => handler.autostart(None),
        "autostart.set" => match serde_json::from_value::<AutostartParams>(params) {
            Ok(params) => handler.autostart(Some(params.enabled)),
            Err(_) => return failure(id, "INVALID_PARAMS", "enabled must be a boolean"),
        },
        "autostart.get" => return failure(id, "INVALID_PARAMS", "unexpected parameters"),
        _ => return failure(id, "METHOD_NOT_FOUND", "unknown launcher method"),
    };
    match result {
        Ok(result) => json!({"type":"response","id":id,"ok":true,"result":result}),
        Err(_) => failure(
            id,
            "AUTOSTART_FAILED",
            "unable to change or query autostart",
        ),
    }
}
