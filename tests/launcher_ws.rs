//! 无 WebView 的真实回环 WS 回归；同时验证协议和可取消的会话生命周期。
#[path = "../src/launcher_control.rs"]
mod launcher_control;
#[path = "../src/launcher_protocol.rs"]
mod launcher_protocol;

use anyhow::Result;
use launcher_control::{start_launcher_channel, ChannelHandler};
use serde_json::{json, Value};
use std::{
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{
    handshake::server::{Request, Response},
    Message, WebSocket,
};

#[derive(Default)]
struct Handler {
    writes: AtomicUsize,
    notices: AtomicUsize,
}

impl ChannelHandler for Handler {
    fn autostart(&self, enabled: Option<bool>) -> Result<Value> {
        if enabled.is_some() {
            self.writes.fetch_add(1, Ordering::SeqCst);
        }
        Ok(json!({"supported":true,"enabled":enabled.unwrap_or(false)}))
    }

    fn notification(&self, _data: Value) -> Result<()> {
        self.notices.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").unwrap()
}

fn accept(listener: &TcpListener) -> WebSocket<TcpStream> {
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut socket = tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
        assert_eq!(request.uri().path(), "/api/v2/launcher/ws");
        assert_eq!(request.headers()["authorization"], "Bearer fixture-secret");
        Ok(response)
    })
    .unwrap();
    let hello = read(&mut socket);
    assert_eq!(hello["method"], "session.hello");
    assert_eq!(hello["params"]["protocolVersion"], 2);
    assert_eq!(hello["params"]["launcherVersion"], env!("CARGO_PKG_VERSION"));
    write(
        &mut socket,
        json!({"type":"response","id":"hello","ok":true,
                              "result":{"protocolVersion":2,"heartbeatSeconds":15}}),
    );
    socket
}

fn read(socket: &mut WebSocket<TcpStream>) -> Value {
    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap()
}

fn write(socket: &mut WebSocket<TcpStream>, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .unwrap();
}

fn response(socket: &mut WebSocket<TcpStream>, id: &str, login: &mut bool) -> Value {
    loop {
        let message = read(socket);
        if message["type"] == "request" && message["method"] == "login.issue" {
            write(
                socket,
                json!({"type":"response","id":message["id"],"ok":true,
                                 "result":{"ticket":"safe_ticket","ttl":60}}),
            );
            *login = true;
        } else {
            assert_eq!(message["id"], id);
            return message;
        }
    }
}

#[test]
fn commands_are_strict_correlated_and_deduplicated_and_login_uses_same_socket() {
    let listener = listener();
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(Handler::default());
    let checked = handler.clone();
    let server = thread::spawn(move || {
        let mut socket = accept(&listener);
        let mut login = false;
        let command = json!({"type":"request","id":"command-1","method":"autostart.set",
                             "params":{"enabled":true}});
        write(&mut socket, command.clone());
        let reply = response(&mut socket, "command-1", &mut login);
        assert_eq!(reply["id"], "command-1");
        assert_eq!(reply["result"]["enabled"], true);
        write(&mut socket, command);
        assert_eq!(response(&mut socket, "command-1", &mut login), reply);
        assert_eq!(checked.writes.load(Ordering::SeqCst), 1);
        write(
            &mut socket,
            json!({"type":"request","id":"bad","method":"autostart.set","params":{"enabled":"yes"}}),
        );
        assert_eq!(
            response(&mut socket, "bad", &mut login)["error"]["code"],
            "INVALID_PARAMS"
        );
        write(
            &mut socket,
            json!({"type":"request","id":"unknown","method":"__dict__","params":{}}),
        );
        assert_eq!(
            response(&mut socket, "unknown", &mut login)["error"]["code"],
            "METHOD_NOT_FOUND"
        );
        write(
            &mut socket,
            json!({"type":"request","id":"expired","method":"autostart.set",
                                 "params":{"enabled":false},"deadline":1.0}),
        );
        assert_eq!(
            response(&mut socket, "expired", &mut login)["error"]["code"],
            "TIMEOUT"
        );
        if !login {
            let request = read(&mut socket);
            assert_eq!(request["method"], "login.issue");
            write(
                &mut socket,
                json!({"type":"response","id":request["id"],"ok":true,
                                     "result":{"ticket":"safe_ticket","ttl":60}}),
            );
        }
    });
    let (channel, worker) = start_launcher_channel(
        port,
        "fixture-secret".into(),
        Arc::new(AtomicBool::new(false)),
        handler.clone(),
    );
    assert_eq!(channel.login_ticket().unwrap(), "safe_ticket");
    server.join().unwrap();
    let started = Instant::now();
    channel.close();
    worker.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(handler.writes.load(Ordering::SeqCst), 1);
}

#[test]
fn notification_is_acknowledged_and_not_displayed_twice_after_reconnect() {
    let listener = listener();
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(Handler::default());
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let mut socket = accept(&listener);
            write(
                &mut socket,
                json!({"type":"request","id":"replayed-command",
                                     "method":"autostart.set","params":{"enabled":true}}),
            );
            assert_eq!(read(&mut socket)["ok"], true);
            write(
                &mut socket,
                json!({"type":"event","id":"notification-1","topic":"notification",
                                     "data":{"instance":"fixture","title":"title","content":"body"}}),
            );
            let ack = read(&mut socket);
            assert_eq!(ack["method"], "notifications.ack");
            assert_eq!(ack["params"]["ids"], json!(["notification-1"]));
        }
    });
    let (channel, worker) = start_launcher_channel(
        port,
        "fixture-secret".into(),
        Arc::new(AtomicBool::new(false)),
        handler.clone(),
    );
    server.join().unwrap();
    channel.close();
    worker.join().unwrap();
    assert_eq!(handler.notices.load(Ordering::SeqCst), 1);
    assert_eq!(handler.writes.load(Ordering::SeqCst), 1);
}

#[test]
fn malformed_protocol_fields_and_unsafe_tickets_are_rejected() {
    assert!(
        serde_json::from_value::<launcher_protocol::Incoming>(json!({
            "type":"request","id":"x","method":"autostart.get","params":{},"unknown":true
        }))
        .is_err()
    );
    assert!(!launcher_protocol::valid_ticket("secret&password=leak"));
    assert!(!launcher_protocol::valid_ticket(""));
    assert!(launcher_protocol::valid_ticket("opaque_ticket-123"));
}

#[test]
fn idle_connection_is_cancelled_without_waiting_for_server_data() {
    let listener = listener();
    let port = listener.local_addr().unwrap().port();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let mut socket = accept(&listener);
        ready_tx.send(()).unwrap();
        let _ = socket.read();
    });
    let (channel, worker) = start_launcher_channel(
        port,
        "fixture-secret".into(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(Handler::default()),
    );
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let started = Instant::now();
    channel.close();
    worker.join().unwrap();
    server.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
#[ignore = "requires the Python launcher API fixture; run with AZURPILOT_TEST_BACKEND_PORT"]
fn python_backend_interop() {
    let port = std::env::var("AZURPILOT_TEST_BACKEND_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let handler = Arc::new(Handler::default());
    let (channel, worker) = start_launcher_channel(
        port,
        "fixture-secret".into(),
        Arc::new(AtomicBool::new(false)),
        handler.clone(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while handler.writes.load(Ordering::SeqCst) == 0 || handler.notices.load(Ordering::SeqCst) == 0
    {
        if Instant::now() >= deadline {
            channel.close();
            worker.join().unwrap();
            panic!("Python backend did not deliver command and notification");
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(launcher_protocol::valid_ticket(
        &channel.login_ticket().unwrap()
    ));
    channel.close();
    worker.join().unwrap();
}
