use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

#[test]
fn ownership_lookup_matches_only_the_actual_socket_owner() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let connection = Connection {
        local: stream.local_addr().unwrap(),
        remote: stream.peer_addr().unwrap(),
    };
    assert!(owns_connection(connection, std::process::id()).unwrap());
    assert!(!owns_connection(connection, u32::MAX).unwrap());
    drop(stream);
}

#[test]
fn windivert_address_matches_the_native_abi() {
    assert_eq!(mem::size_of::<Address>(), 80);
    assert_eq!(mem::align_of::<Address>(), 8);
}

#[test]
fn native_ipv6_rows_keep_ports_and_reject_other_pids() {
    let mut row = vec![0u8; 56];
    row[..16].copy_from_slice(&"2001:db8::2".parse::<Ipv6Addr>().unwrap().octets());
    row[24..40].copy_from_slice(&"2606:4700::1111".parse::<Ipv6Addr>().unwrap().octets());
    row[20..24].copy_from_slice(&u32::from(55000u16.to_be()).to_ne_bytes());
    row[44..48].copy_from_slice(&u32::from(443u16.to_be()).to_ne_bytes());
    row[52..56].copy_from_slice(&1234u32.to_ne_bytes());
    let connection = owned_row(&row, true, 1234).unwrap().unwrap();
    assert_eq!(connection.local, "[2001:db8::2]:55000".parse().unwrap());
    assert_eq!(connection.remote, "[2606:4700::1111]:443".parse().unwrap());
    assert!(owned_row(&row, true, 4321).unwrap().is_none());
}

#[test]
fn embedded_driver_files_are_verified_before_loading() {
    if DLL.is_empty() {
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    prepare_files(temporary.path()).unwrap();
    assert_eq!(
        fs::read(temporary.path().join("WinDivert.dll")).unwrap(),
        DLL
    );
    fs::write(temporary.path().join("WinDivert.dll"), b"corrupted").unwrap();
    prepare_files(temporary.path()).unwrap();
    assert_eq!(
        fs::read(temporary.path().join("WinDivert.dll")).unwrap(),
        DLL
    );
}

#[test]
#[ignore = "loads the signed WinDivert driver; requires an administrator and network access"]
fn rst_filter_live() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init();
    let executable = std::env::current_exe().unwrap();
    let directory = executable
        .parent()
        .unwrap()
        .join(".azurpilot/tools/windivert-2.2.2-A");
    prepare_files(&directory).unwrap();
    let api = Api::load(&directory).unwrap();
    let url = reqwest::Url::parse(crate::updater::LAUNCHER_UPDATE_URL).unwrap();
    let host = url.host_str().unwrap();
    let target = (host, url.port_or_known_default().unwrap())
        .to_socket_addrs()
        .unwrap()
        .find(|address| address.is_ipv4())
        .unwrap();

    // 无过滤时，正确序列号的注入 RST 必须真的让 TCP 栈断开连接。
    let (mut baseline, ack, address) = capture_ack(&api, target);
    inject_rst(&api, &ack, address);
    baseline
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let error = baseline.read(&mut [0; 1]).unwrap_err();
    assert!(
        matches!(
            error.kind(),
            io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
        ),
        "injected RST was not accepted by the TCP stack: {error}"
    );
    drop(baseline);

    let guard = Guard::start().unwrap();
    assert!(guard.active.load(Ordering::SeqCst));
    let (mut protected, ack, address) = capture_ack(&api, target);
    inject_rst(&api, &ack, address);
    let started = Instant::now();
    while guard.dropped.load(Ordering::SeqCst) == 0 && started.elapsed() < Duration::from_secs(3) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        guard.dropped.load(Ordering::SeqCst) > 0,
        "injected RST was not intercepted"
    );
    protected
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    let error = protected.read(&mut [0; 1]).unwrap_err();
    assert!(
        matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ),
        "protected connection was reset: {error}"
    );

    // 在同一个被注入 RST 的 socket 上完成 TLS 并读 HTTP，证明连接确实存活。
    protected
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    let session =
        rustls::ClientConnection::new(Arc::new(tls), host.to_owned().try_into().unwrap()).unwrap();
    let mut stream = rustls::StreamOwned::new(session, protected);
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
        url.path()
    )
    .unwrap();
    stream.flush().unwrap();
    let mut response = [0u8; 12];
    stream.read_exact(&mut response).unwrap();
    assert_eq!(&response, b"HTTP/1.1 200");
    drop(stream);

    // 另一个进程的 RST 必须仍由系统接收，不能被此过滤器吞掉。
    verify_other_process_rst(&api, target);

    // 过滤器开启期间，启动器 HTTP 客户端仍能完整获取清单。
    let client = crate::http_client::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let bytes = crate::http_client::read_limited(
        client.get(crate::updater::LAUNCHER_UPDATE_URL),
        1024 * 1024,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["version"].is_string());
    drop(guard);
}

struct TestHandle {
    api: Arc<Api>,
    handle: isize,
}
impl Drop for TestHandle {
    fn drop(&mut self) {
        unsafe {
            (self.api.close)(self.handle);
        }
    }
}

fn open_test_handle(api: &Arc<Api>, filter: &str, priority: i16, flags: u64) -> TestHandle {
    let filter = std::ffi::CString::new(filter).unwrap();
    let handle = unsafe { (api.open)(filter.as_ptr(), 0, priority, flags) };
    assert_ne!(
        handle,
        -1,
        "WinDivertOpen failed: {}",
        io::Error::last_os_error()
    );
    TestHandle {
        api: api.clone(),
        handle,
    }
}

fn capture_ack(api: &Arc<Api>, target: SocketAddr) -> (TcpStream, Vec<u8>, Address) {
    let watcher = ack_watcher(api, target);
    let stream = TcpStream::connect_timeout(&target, Duration::from_secs(5)).unwrap();
    let (packet, address) = receive_ack(&watcher, stream.local_addr().unwrap().port());
    (stream, packet, address)
}

fn ack_watcher(api: &Arc<Api>, target: SocketAddr) -> TestHandle {
    open_test_handle(api, &format!("outbound and ip and tcp and ip.DstAddr == {} and tcp.DstPort == {} and tcp.Ack and !tcp.Syn and tcp.PayloadLength == 0", target.ip(), target.port()), 100, 1 | 4)
}

fn receive_ack(watcher: &TestHandle, port: u16) -> (Vec<u8>, Address) {
    let api = watcher.api.clone();
    let handle = watcher.handle;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let mut packet = vec![0u8; 65575];
        loop {
            let mut length = 0u32;
            let mut address = Address::default();
            if unsafe {
                (api.recv)(
                    handle,
                    packet.as_mut_ptr().cast(),
                    packet.len() as u32,
                    &mut length,
                    &mut address,
                )
            } == 0
            {
                break;
            }
            let offset = usize::from(packet[0] & 15) * 4;
            if length as usize >= offset + 20
                && u16::from_be_bytes([packet[offset], packet[offset + 1]]) == port
            {
                let _ = sender.send((packet[..length as usize].to_vec(), address));
                break;
            }
        }
    });
    let result = receiver.recv_timeout(Duration::from_secs(5));
    unsafe {
        (watcher.api.shutdown)(watcher.handle, 1);
    }
    worker.join().unwrap();
    result.expect("outbound TCP ACK was not observed")
}

fn inject_rst(api: &Arc<Api>, ack: &[u8], mut address: Address) {
    let tcp = usize::from(ack[0] & 15) * 4;
    let mut packet = vec![0u8; 40];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&40u16.to_be_bytes());
    packet[8] = 64;
    packet[9] = 6;
    packet[12..16].copy_from_slice(&ack[16..20]);
    packet[16..20].copy_from_slice(&ack[12..16]);
    packet[20..22].copy_from_slice(&ack[tcp + 2..tcp + 4]);
    packet[22..24].copy_from_slice(&ack[tcp..tcp + 2]);
    packet[24..28].copy_from_slice(&ack[tcp + 8..tcp + 12]);
    packet[28..32].copy_from_slice(&ack[tcp + 4..tcp + 8]);
    packet[32] = 0x50;
    packet[33] = 0x14;
    address.flags &= !((1 << 16) | (1 << 17) | (1 << 18));
    type Checksum = unsafe extern "system" fn(*mut c_void, u32, *mut Address, u64) -> i32;
    let checksum = unsafe {
        api._library
            .get::<Checksum>(b"WinDivertHelperCalcChecksums\0")
            .unwrap()
    };
    assert_ne!(
        unsafe {
            checksum(
                packet.as_mut_ptr().cast(),
                packet.len() as u32,
                &mut address,
                0,
            )
        },
        0
    );
    let sender = open_test_handle(api, "false", 1000, 8);
    assert_ne!(
        unsafe {
            (api.send)(
                sender.handle,
                packet.as_ptr().cast(),
                packet.len() as u32,
                ptr::null_mut(),
                &address,
            )
        },
        0,
        "inject TCP RST failed: {}",
        io::Error::last_os_error()
    );
}

fn verify_other_process_rst(api: &Arc<Api>, target: SocketAddr) {
    use crate::window_util::CreateNoWindow;
    let temporary = tempfile::tempdir().unwrap();
    let watcher = ack_watcher(api, target);
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "rst_filter::windows::tests::rst_external_owner_fixture",
            "--exact",
            "--nocapture",
        ])
        .env("AZURPILOT_RST_FIXTURE", temporary.path())
        .env("AZURPILOT_RST_TARGET", target.to_string())
        .create_no_window();
    let mut child = command.spawn().unwrap();
    let started = Instant::now();
    let ready = temporary.path().join("ready");
    while !ready.exists() && started.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(10));
    }
    let port = fs::read_to_string(ready)
        .expect("child socket was not created")
        .parse::<u16>()
        .unwrap();
    let (packet, address) = receive_ack(&watcher, port);
    inject_rst(api, &packet, address);
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if started.elapsed() > Duration::from_secs(6) {
            let _ = child.kill();
            panic!("RST for another process was blocked");
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        fs::read_to_string(temporary.path().join("result")).unwrap(),
        "reset"
    );
}

#[test]
fn rst_external_owner_fixture() {
    let Some(directory) = std::env::var_os("AZURPILOT_RST_FIXTURE").map(std::path::PathBuf::from)
    else {
        return;
    };
    let target = std::env::var("AZURPILOT_RST_TARGET")
        .unwrap()
        .parse::<SocketAddr>()
        .unwrap();
    let mut stream = TcpStream::connect_timeout(&target, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    fs::write(
        directory.join("ready.tmp"),
        stream.local_addr().unwrap().port().to_string(),
    )
    .unwrap();
    fs::rename(directory.join("ready.tmp"), directory.join("ready")).unwrap();
    let error = stream.read(&mut [0; 1]).unwrap_err();
    assert!(matches!(
        error.kind(),
        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
    ));
    fs::write(directory.join("result"), "reset").unwrap();
}
