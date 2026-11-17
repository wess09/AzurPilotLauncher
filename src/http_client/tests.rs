use super::*;
use std::{
    io::Write,
    net::{SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::atomic::AtomicUsize,
    thread,
};

const HOST: &str = "launcher-http.test";

struct Pki {
    root: rustls::pki_types::CertificateDer<'static>,
    leaf: rustls::pki_types::CertificateDer<'static>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
    identity_pem: Vec<u8>,
}

impl Pki {
    fn new() -> Self {
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params =
            rcgen::CertificateParams::new(vec![HOST.into(), "localhost".into()]).unwrap();
        params.extended_key_usages = vec![
            rcgen::ExtendedKeyUsagePurpose::ServerAuth,
            rcgen::ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let leaf = params.signed_by(&key, &ca, &ca_key).unwrap();
        Self {
            root: ca.der().clone(),
            leaf: leaf.der().clone(),
            key: rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            identity_pem: format!("{}{}", leaf.pem(), key.serialize_pem()).into_bytes(),
        }
    }

    fn tls(&self, mtls: bool) -> rustls::ServerConfig {
        let builder = rustls::ServerConfig::builder();
        let builder = if mtls {
            let mut roots = rustls::RootCertStore::empty();
            roots.add(self.root.clone()).unwrap();
            builder.with_client_cert_verifier(
                rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
                    .build()
                    .unwrap(),
            )
        } else {
            builder.with_no_client_auth()
        };
        builder
            .with_single_cert(vec![self.leaf.clone()], self.key.clone_key())
            .unwrap()
    }

    fn client(&self, address: SocketAddr) -> ClientBuilder {
        let mut builder = Client::builder().no_proxy().timeout(Duration::from_secs(8));
        let root = reqwest::Certificate::from_der(&self.root).unwrap();
        builder.extra_roots.push(self.root.clone());
        builder.tcp = builder
            .tcp
            .add_root_certificate(root.clone())
            .resolve(HOST, address);
        builder.plain_tcp = builder.plain_tcp.resolve(HOST, address);
        builder.quic = builder
            .quic
            .add_root_certificate(root)
            .resolve(HOST, address);
        builder
    }
}

fn request_head(stream: &mut impl Read) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        assert!(bytes.len() < 16384);
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
    }
    String::from_utf8(bytes).unwrap()
}

/// 观察真实 socket 上的记录边界，随后将原始字节交还标准 TLS 服务端。
fn capture_client_hello(stream: &mut TcpStream) -> Vec<Vec<u8>> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut records = Vec::new();
    let mut handshake = Vec::new();
    loop {
        let mut header = [0; 5];
        stream.read_exact(&mut header).unwrap();
        assert_eq!(
            header[0], 22,
            "expected an unencrypted TLS handshake record"
        );
        let length = u16::from_be_bytes([header[3], header[4]]) as usize;
        assert!((1..=16384).contains(&length));
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload).unwrap();
        handshake.extend_from_slice(&payload);
        records.push([header.to_vec(), payload].concat());
        assert!(handshake.len() < 65536);
        if handshake.len() >= 4 {
            assert_eq!(handshake[0], 1, "expected ClientHello");
            let length = u32::from_be_bytes([0, handshake[1], handshake[2], handshake[3]]) as usize;
            if handshake.len() >= 4 + length {
                return records;
            }
        }
    }
}

struct ReplayedStream {
    prefix: io::Cursor<Vec<u8>>,
    socket: TcpStream,
}

impl Read for ReplayedStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.prefix.read(buffer)?;
        if read != 0 {
            Ok(read)
        } else {
            self.socket.read(buffer)
        }
    }
}

impl Write for ReplayedStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.socket.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

fn serve_captured_manifest(
    stream: TcpStream,
    records: Vec<Vec<u8>>,
    tls: Arc<rustls::ServerConfig>,
) {
    let mut stream = rustls::StreamOwned::new(
        rustls::ServerConnection::new(tls).unwrap(),
        ReplayedStream {
            prefix: io::Cursor::new(records.concat()),
            socket: stream,
        },
    );
    assert!(request_head(&mut stream).starts_with("GET /updata/stable.json "));
    assert!(stream
        .conn
        .peer_certificates()
        .is_some_and(|chain| !chain.is_empty()));
    let payload = br#"{"version":"3.0.7"}"#;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    )
    .unwrap();
    stream.write_all(payload).unwrap();
    stream.flush().unwrap();
}

#[test]
fn tls_record_fragmentation_avoids_a_client_hello_rst_rule() {
    let pki = Pki::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let tls = Arc::new(pki.tls(true));
    let server = thread::spawn(move || {
        let (mut ordinary, _) = listener.accept().unwrap();
        let baseline = capture_client_hello(&mut ordinary);
        assert_eq!(baseline.len(), 1);
        // 模拟根据完整 ClientHello 识别 SNI 后注入 RST 的设备。
        socket2::SockRef::from(&ordinary)
            .set_linger(Some(Duration::ZERO))
            .unwrap();
        drop(ordinary);

        let (mut fragmented, _) = listener.accept().unwrap();
        let records = capture_client_hello(&mut fragmented);
        assert!(
            records.len() > 1,
            "the actual ClientHello must span TLS records"
        );
        assert!(records
            .iter()
            .all(|record| record.len() <= TLS_FRAGMENT_SIZE));
        let hello: Vec<u8> = records
            .iter()
            .flat_map(|record| record[5..].iter().copied())
            .collect();
        assert!(
            hello
                .windows(HOST.len())
                .any(|part| part == HOST.as_bytes()),
            "SNI must be preserved"
        );
        serve_captured_manifest(fragmented, records, tls);
    });
    let mut client = pki
        .client(address)
        .identity_pem(&pki.identity_pem)
        .unwrap()
        .build()
        .unwrap();
    client.quic = None;
    let url = format!("https://{HOST}:{}/updata/stable.json", address.port());
    // 普通 TLS 对同一规则失败，开启记录分片后成功；证书、mTLS 和 URL 相同。
    assert!(runtime()
        .block_on(async { client.plain_tcp.get(&url).send().await })
        .is_err());
    let bytes = read_limited(client.get(&url), 1024, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["version"],
        "3.0.7"
    );
    server.join().unwrap();
}

#[test]
fn incompatible_tls_fragmentation_falls_back_and_preserves_mtls() {
    let pki = Pki::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let tls = Arc::new(pki.tls(true));
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert!(capture_client_hello(&mut stream).len() > 1);
        // 合法的 fatal unexpected_message，模拟不接受 ClientHello 分片的网关。
        stream.write_all(&[21, 3, 3, 0, 2, 2, 10]).unwrap();
        drop(stream);
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let records = capture_client_hello(&mut stream);
            assert_eq!(records.len(), 1, "compatibility fallback uses ordinary TLS");
            serve_captured_manifest(stream, records, tls.clone());
        }
    });
    let mut client = pki
        .client(address)
        .identity_pem(&pki.identity_pem)
        .unwrap()
        .build()
        .unwrap();
    client.quic = None;
    let url = format!("https://{HOST}:{}/updata/stable.json", address.port());
    for _ in 0..2 {
        let bytes = read_limited(client.get(&url), 1024, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["version"],
            "3.0.7"
        );
    }
    server.join().unwrap();
}

fn quic_server(
    address: SocketAddr,
    mut tls: rustls::ServerConfig,
    handler: impl Fn(http::Request<()>) -> (http::Response<()>, Bytes) + Send + Sync + 'static,
) -> (quinn::Endpoint, tokio::task::JoinHandle<()>) {
    tls.alpn_protocols = vec![b"h3".to_vec()];
    let config = quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap(),
    ));
    let _guard = runtime().enter();
    let endpoint = quinn::Endpoint::server(config, address).unwrap();
    let server = endpoint.clone();
    let handler = Arc::new(handler);
    let task = runtime().spawn(async move {
        while let Some(incoming) = server.accept().await {
            let handler = handler.clone();
            tokio::spawn(async move {
                let Ok(connection) = incoming.await else {
                    return;
                };
                let mut connection =
                    h3::server::Connection::<_, Bytes>::new(h3_quinn::Connection::new(connection))
                        .await
                        .unwrap();
                while let Ok(Some((request, mut stream))) = connection.accept().await {
                    let handler = handler.clone();
                    tokio::spawn(async move {
                        let (response, bytes) = handler(request);
                        stream.send_response(response).await.unwrap();
                        if !bytes.is_empty() {
                            stream.send_data(bytes).await.unwrap();
                        }
                        stream.finish().await.unwrap();
                    });
                }
            });
        }
    });
    (endpoint, task)
}

#[test]
fn quic_is_preferred_and_streams_authenticated_payloads() {
    let pki = Pki::new();
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    tcp.set_nonblocking(true).unwrap();
    let address = tcp.local_addr().unwrap();
    let (endpoint, task) = quic_server(address, pki.tls(true), |request| {
        assert_eq!(request.headers()[header::RANGE], "bytes=0-262143");
        assert_eq!(
            request.headers()[header::USER_AGENT],
            "launcher transport test"
        );
        (
            http::Response::builder().status(206).body(()).unwrap(),
            Bytes::from(vec![0x5a; 262144]),
        )
    });
    let client = pki
        .client(address)
        .identity_pem(&pki.identity_pem)
        .unwrap()
        .user_agent("launcher transport test")
        .build()
        .unwrap();
    let mut response = client
        .get(format!("https://{HOST}:{}/payload", address.port()))
        .header(header::RANGE, "bytes=0-262143")
        .send()
        .unwrap();
    assert_eq!(response.inner.version(), Version::HTTP_3);
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    let mut bytes = Vec::new();
    response.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, vec![0x5a; 262144]);
    assert_eq!(tcp.accept().unwrap_err().kind(), io::ErrorKind::WouldBlock);
    endpoint.close(0u32.into(), b"done");
    task.abort();
}

#[test]
fn blocked_udp_falls_back_and_caches_failure_without_losing_headers_or_mtls() {
    let pki = Pki::new();
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = tcp.local_addr().unwrap();
    // 保持 UDP 端口打开但不应答，模拟防火墙静默丢弃 QUIC。
    let _blackhole = UdpSocket::bind(address).unwrap();
    let tls = Arc::new(pki.tls(true));
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (stream, _) = tcp.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut stream = rustls::StreamOwned::new(
                rustls::ServerConnection::new(tls.clone()).unwrap(),
                stream,
            );
            let request = request_head(&mut stream).to_ascii_lowercase();
            assert!(request.contains("range: bytes=5-8"));
            assert!(request.contains("user-agent: fallback-test"));
            assert!(stream
                .conn
                .peer_certificates()
                .is_some_and(|chain| !chain.is_empty()));
            stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata").unwrap();
            stream.flush().unwrap();
        }
    });
    let client = pki
        .client(address)
        .identity_pem(&pki.identity_pem)
        .unwrap()
        .user_agent("fallback-test")
        .build()
        .unwrap();
    let url = format!("https://{HOST}:{}/payload", address.port());
    for attempt in 0..2 {
        let started = Instant::now();
        let mut response = client
            .get(&url)
            .header(header::RANGE, "bytes=5-8")
            .send()
            .unwrap();
        assert_eq!(response.inner.version(), Version::HTTP_11);
        let mut bytes = Vec::new();
        response.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"data");
        if attempt == 0 {
            assert!(started.elapsed() < Duration::from_secs(4));
        } else {
            assert!(started.elapsed() < H3_HEADER_TIMEOUT);
        }
    }
    server.join().unwrap();
}

#[test]
fn redirects_reselect_quic_for_the_destination() {
    let pki = Pki::new();
    let (endpoint, task) = quic_server("127.0.0.1:0".parse().unwrap(), pki.tls(false), |request| {
        assert!(!request.headers().contains_key(header::AUTHORIZATION));
        (
            http::Response::new(()),
            Bytes::from_static(b"redirected over QUIC"),
        )
    });
    let address = endpoint.local_addr().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = request_head(&mut stream);
        write!(stream, "HTTP/1.1 302 Found\r\nLocation: https://{HOST}:{}/target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", address.port()).unwrap();
    });
    let client = pki.client(address).build().unwrap();
    let mut response = client
        .get(url)
        .header(header::AUTHORIZATION, "Bearer secret")
        .send()
        .unwrap();
    assert_eq!(response.inner.version(), Version::HTTP_3);
    let mut body = String::new();
    response.read_to_string(&mut body).unwrap();
    assert_eq!(body, "redirected over QUIC");
    server.join().unwrap();
    endpoint.close(0u32.into(), b"done");
    task.abort();
}

#[test]
fn untrusted_quic_and_tcp_certificates_are_rejected() {
    let pki = Pki::new();
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = tcp.local_addr().unwrap();
    let received_request = Arc::new(AtomicBool::new(false));
    let seen = received_request.clone();
    let (endpoint, task) = quic_server(address, pki.tls(false), move |_| {
        seen.store(true, Ordering::SeqCst);
        (http::Response::new(()), Bytes::new())
    });
    let tls = Arc::new(pki.tls(false));
    let server = thread::spawn(move || {
        let (stream, _) = tcp.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut stream =
            rustls::StreamOwned::new(rustls::ServerConnection::new(tls).unwrap(), stream);
        assert!(stream.read(&mut [0; 1024]).is_err());
    });
    let mut builder = Client::builder().no_proxy().timeout(Duration::from_secs(5));
    builder.tcp = builder.tcp.resolve(HOST, address);
    builder.plain_tcp = builder.plain_tcp.resolve(HOST, address);
    builder.quic = builder.quic.resolve(HOST, address);
    assert!(builder
        .build()
        .unwrap()
        .get(format!("https://{HOST}:{}/", address.port()))
        .send()
        .is_err());
    assert!(!received_request.load(Ordering::SeqCst));
    server.join().unwrap();
    endpoint.close(0u32.into(), b"done");
    task.abort();
}

#[test]
fn interrupted_get_is_retried_with_the_same_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = request_head(&mut stream).to_ascii_lowercase();
            assert!(request.contains("range: bytes=1-2"));
            if index == 1 {
                stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
            }
            // 第一个连接在响应头之前中断，第二个返回有效数据。
        }
    });
    let mut response = Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(url)
        .header(header::RANGE, "bytes=1-2")
        .send()
        .unwrap();
    let mut body = String::new();
    response.read_to_string(&mut body).unwrap();
    assert_eq!(body, "ok");
    server.join().unwrap();
}

#[test]
fn tcp_rst_reopens_quic_instead_of_staying_on_tcp() {
    let pki = Pki::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let blackhole = UdpSocket::bind(address).unwrap();
    let tls = pki.tls(false);
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        drop(blackhole);
        let server = quic_server(address, tls, |request| {
            assert_eq!(request.uri().path(), "/updata/stable.json");
            (http::Response::new(()), Bytes::from_static(b"{}"))
        });
        // SO_LINGER=0 在关闭 socket 时发送真正的 TCP RST。
        socket2::SockRef::from(&stream)
            .set_linger(Some(Duration::ZERO))
            .unwrap();
        drop(stream);
        server
    });
    let client = pki.client(address).build().unwrap();
    let mut response = client
        .get(format!(
            "https://{HOST}:{}/updata/stable.json",
            address.port()
        ))
        .send()
        .unwrap();
    assert_eq!(response.inner.version(), Version::HTTP_3);
    let mut bytes = Vec::new();
    response.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"{}");
    assert!(client
        .resolver
        .rotations
        .lock()
        .unwrap()
        .get(HOST)
        .is_some_and(|count| *count >= 1));
    let (endpoint, task) = server.join().unwrap();
    endpoint.close(0u32.into(), b"done");
    task.abort();
}

#[test]
fn cancelling_a_quic_probe_never_starts_tcp_fallback() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let _udp = UdpSocket::bind(address).unwrap();
    let mut builder = Client::builder().no_proxy();
    builder.quic = builder.quic.resolve(HOST, address);
    builder.tcp = builder.tcp.resolve(HOST, address);
    builder.plain_tcp = builder.plain_tcp.resolve(HOST, address);
    let client = builder.build().unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        signal.store(true, Ordering::SeqCst);
    });
    let started = Instant::now();
    assert!(crate::cancel::request(
        client.get(format!("https://{HOST}:{}/", address.port())),
        &cancel
    )
    .is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    thread.join().unwrap();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn exhausted_request_budget_does_not_start_tcp_fallback() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let _udp = UdpSocket::bind(address).unwrap();
    let mut builder = Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(150));
    builder.quic = builder.quic.resolve(HOST, address);
    builder.tcp = builder.tcp.resolve(HOST, address);
    builder.plain_tcp = builder.plain_tcp.resolve(HOST, address);
    let started = Instant::now();
    assert!(builder
        .build()
        .unwrap()
        .get(format!("https://{HOST}:{}/", address.port()))
        .send()
        .is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn status_errors_are_left_to_existing_download_retry_policy() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        seen.fetch_add(1, Ordering::SeqCst);
        let _ = request_head(&mut stream);
        stream.write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 12\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let response = Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(url)
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "12");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.join().unwrap();
}

#[test]
fn cleartext_internet_and_credential_urls_are_rejected() {
    let client = Client::builder().build().unwrap();
    for url in ["http://example.com/", "https://user:password@example.com/"] {
        assert!(client.get(url).send().is_err());
    }
}

#[test]
fn manifest_is_refetched_after_response_body_is_interrupted() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/updata/stable.json",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = request_head(&mut stream);
            if attempt == 0 {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 128\r\nConnection: close\r\n\r\n{\"ver").unwrap();
            } else {
                let payload = br#"{"version":"3.0.7"}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                )
                .unwrap();
                stream.write_all(payload).unwrap();
            }
        }
    });
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let bytes = read_limited(client.get(url), 1024, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["version"],
        "3.0.7"
    );
    server.join().unwrap();
}

#[test]
#[ignore = "requires access to the official service"]
fn official_manifest_prefers_quic_and_allows_tcp() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init();
    let mut builder = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10));
    // 可用真实地址隔离本地代理的 fake-IP DNS；只影响此手动测试。
    if let Ok(address) = std::env::var("AZURPILOT_QUIC_TEST_ADDRESS") {
        let url = Url::parse(crate::updater::LAUNCHER_UPDATE_URL).unwrap();
        let address = address.parse::<SocketAddr>().unwrap();
        builder.tcp = builder.tcp.resolve(url.host_str().unwrap(), address);
        builder.plain_tcp = builder.plain_tcp.resolve(url.host_str().unwrap(), address);
        builder.quic = builder.quic.resolve(url.host_str().unwrap(), address);
    }
    let client = builder.build().unwrap();
    let mut response = client
        .get(crate::updater::LAUNCHER_UPDATE_URL)
        .send()
        .unwrap();
    assert!(matches!(
        response.inner.version(),
        Version::HTTP_3 | Version::HTTP_2 | Version::HTTP_11
    ));
    assert!(response.status().is_success());
    tracing::info!(
        protocol = ?response.inner.version(),
        tls_record_fragmentation = response.inner.version() != Version::HTTP_3
            && client.fragment_tls(response.inner.url()),
        "Fetched official manifest"
    );
    let mut body = Vec::new();
    response.read_to_end(&mut body).unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&body).unwrap()["version"].is_string());
}
