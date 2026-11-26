//! 启动器自身的 HTTPS 传输：QUIC 优先、TCP 回退、有限重试和安全重定向。
//! 对外保留同步、流式接口；异步请求在共用 runtime 中运行，超时/取消会丢弃
//! 请求 future，不遗留等待整个下载超时的探测线程。Git/uv/npm 不经过本模块。
use anyhow::{bail, Result};
use bytes::Bytes;
use rand::Rng;
use reqwest::{header, Method, StatusCode, Url, Version};
use rust_i18n::t;
use std::{
    collections::HashMap,
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

const H3_HEADER_TIMEOUT: Duration = Duration::from_secs(2);
const H3_COOLDOWN: Duration = Duration::from_secs(5 * 60);
// rustls 的限制包含 5 字节 TLS 记录头；ClientHello 拆为多个合法记录。
// 此策略针对 SNI 识别触发的重置，不修改握手内容、SNI 或证书验证。
const TLS_FRAGMENT_SIZE: usize = 64;
const MAX_ORIGINS: usize = 256;
const MAX_TRANSPORT_RETRIES: usize = 2;
static NEVER_CANCELLED: AtomicBool = AtomicBool::new(false);

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("launcher-http")
            .enable_all()
            .build()
            .expect("create launcher HTTP runtime")
    })
}

fn is_loopback(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn validate_url(url: &Url) -> Result<()> {
    let local_http =
        cfg!(any(test, debug_assertions)) && url.scheme() == "http" && is_loopback(url);
    if (url.scheme() != "https" && !local_http)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!(t!("network.https_required"));
    }
    Ok(())
}

/// 发生断连后轮换 DNS 返回地址，重新建连时避免始终命中同一个故障节点。
#[derive(Default, Debug)]
struct Resolver {
    rotations: Mutex<HashMap<String, usize>>,
}

impl Resolver {
    fn rotate(&self, host: &str) {
        let mut rotations = self.rotations.lock().unwrap_or_else(|e| e.into_inner());
        if rotations.len() >= MAX_ORIGINS {
            rotations.clear();
        }
        let offset = rotations.entry(host.to_owned()).or_default();
        *offset = offset.wrapping_add(1);
    }
}

impl reqwest::dns::Resolve for Resolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        let offset = self
            .rotations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&host)
            .copied()
            .unwrap_or_default();
        Box::pin(async move {
            let mut addresses: Vec<_> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            // 保留系统解析顺序（包括 IPv4/IPv6），只在断连后移动起点。
            if !addresses.is_empty() {
                let start = offset % addresses.len();
                addresses.rotate_left(start);
            }
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Client {
    tcp: reqwest::Client,
    plain_tcp: reqwest::Client,
    quic: Option<reqwest::Client>,
    timeout: Duration,
    redirect_limit: usize,
    h3_failures: Arc<Mutex<HashMap<String, Instant>>>,
    tls_compatibility: Arc<Mutex<HashMap<String, Instant>>>,
    resolver: Arc<Resolver>,
}

pub(crate) struct ClientBuilder {
    tcp: reqwest::ClientBuilder,
    plain_tcp: reqwest::ClientBuilder,
    quic: reqwest::ClientBuilder,
    timeout: Duration,
    redirect_limit: usize,
    resolver: Arc<Resolver>,
    extra_roots: Vec<rustls::pki_types::CertificateDer<'static>>,
    client_auth: Option<(
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    )>,
}

impl Client {
    pub(crate) fn builder() -> ClientBuilder {
        let resolver = Arc::new(Resolver::default());
        let common = || {
            reqwest::Client::builder()
                .dns_resolver(resolver.clone())
                .redirect(reqwest::redirect::Policy::none())
                .referer(false)
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(30))
                .tcp_keepalive(Duration::from_secs(30))
                .tcp_nodelay(true)
                .pool_idle_timeout(Duration::from_secs(30))
        };
        ClientBuilder {
            tcp: common().connect_timeout(Duration::from_secs(3)),
            plain_tcp: common(),
            // QUIC 直连 UDP；不经过仅支持 TCP 的系统 HTTP 代理。
            quic: common()
                .no_proxy()
                .http3_prior_knowledge()
                .http3_max_idle_timeout(Duration::from_secs(30)),
            timeout: Duration::from_secs(30),
            redirect_limit: 10,
            resolver,
            extra_roots: Vec::new(),
            client_auth: None,
        }
    }

    pub(crate) fn get(&self, url: impl reqwest::IntoUrl) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    pub(crate) fn head(&self, url: impl reqwest::IntoUrl) -> RequestBuilder {
        self.request(Method::HEAD, url)
    }

    fn request(&self, method: Method, url: impl reqwest::IntoUrl) -> RequestBuilder {
        let url = url.into_url();
        // reqwest 会把 URL 用户信息转成 Authorization 并清空 userinfo，
        // 因而必须在交给 reqwest 之前校验原始地址。
        let validation_error = match &url {
            Ok(url) => validate_url(url).err().map(|error| error.to_string()),
            Err(error) => Some(error.to_string()),
        };
        RequestBuilder {
            inner: self
                .tcp
                .request(method, url.as_ref().map_or("", Url::as_str)),
            client: self.clone(),
            validation_error,
        }
    }

    fn try_h3(&self, url: &Url) -> bool {
        if self.quic.is_none() || is_loopback(url) || url.scheme() != "https" {
            return false;
        }
        let mut failures = self.h3_failures.lock().unwrap_or_else(|e| e.into_inner());
        failures.retain(|_, failed_at| failed_at.elapsed() < H3_COOLDOWN);
        !failures.contains_key(&url.origin().ascii_serialization())
    }

    fn h3_failed(&self, url: &Url) {
        let mut failures = self.h3_failures.lock().unwrap_or_else(|e| e.into_inner());
        if failures.len() >= MAX_ORIGINS {
            failures.clear();
        }
        failures.insert(url.origin().ascii_serialization(), Instant::now());
    }

    fn fragment_tls(&self, url: &Url) -> bool {
        if url.scheme() != "https" || is_loopback(url) {
            return false;
        }
        let mut failures = self
            .tls_compatibility
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        failures.retain(|_, failed_at| failed_at.elapsed() < H3_COOLDOWN);
        !failures.contains_key(&url.origin().ascii_serialization())
    }

    fn use_compatible_tls(&self, url: &Url) {
        let mut failures = self
            .tls_compatibility
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if failures.len() >= MAX_ORIGINS {
            failures.clear();
        }
        failures.insert(url.origin().ascii_serialization(), Instant::now());
    }

    fn interrupted(&self, url: &Url) {
        // TCP 重置不能永久把请求压在 TCP 上：清除 QUIC 失败记忆，
        // 下一次请求/重试在重新解析后的另一个地址再次先走 QUIC。
        self.h3_failures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&url.origin().ascii_serialization());
        if let Some(host) = url.host_str() {
            self.resolver.rotate(host);
        }
    }

    async fn execute(&self, mut request: reqwest::Request) -> Result<Response> {
        validate_url(request.url())?;
        let started = Instant::now();
        let timeout = request.timeout().copied().unwrap_or(self.timeout);
        let mut redirects = 0;
        let mut retries = 0;
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                bail!(t!("network.deadline"));
            }
            *request.timeout_mut() = Some(remaining);
            let mut response = None;
            if self.try_h3(request.url()) {
                let mut quic_request = request.try_clone().expect("GET/HEAD has no body");
                *quic_request.version_mut() = Version::HTTP_3;
                let result = tokio::time::timeout(
                    H3_HEADER_TIMEOUT.min(remaining),
                    self.quic.as_ref().unwrap().execute(quic_request),
                )
                .await;
                match result {
                    Ok(Ok(value)) => response = Some(value),
                    _ => {
                        self.h3_failed(request.url());
                        // 不打印 URL，避免查询参数/凭据进入日志。
                        tracing::debug!(
                            host = request.url().host_str(),
                            "HTTP/3 unavailable; using HTTPS over TCP"
                        );
                    }
                }
            }
            let result = match response {
                Some(response) => Ok(response),
                None => {
                    let remaining = timeout.saturating_sub(started.elapsed());
                    if remaining.is_zero() {
                        bail!(t!("network.deadline"));
                    }
                    let mut tcp_request = request.try_clone().expect("GET/HEAD has no body");
                    *tcp_request.timeout_mut() = Some(remaining);
                    let fragmented = self.fragment_tls(request.url());
                    let tcp = if fragmented {
                        &self.tcp
                    } else {
                        &self.plain_tcp
                    };
                    let result = tcp.execute(tcp_request).await;
                    if fragmented
                        && result.as_ref().is_err_and(tls_compatibility_error)
                        && retries < MAX_TRANSPORT_RETRIES
                    {
                        self.use_compatible_tls(request.url());
                        self.interrupted(request.url());
                        tracing::debug!(host = request.url().host_str(), "Fragmented TLS handshake rejected; trying standard TLS with the same certificate policy");
                        retries += 1;
                        continue;
                    }
                    result
                }
            };
            let response = match result {
                Ok(response) => response,
                Err(error) if retries < MAX_TRANSPORT_RETRIES && retryable(&error) => {
                    self.interrupted(request.url());
                    // 仅重发幂等 GET/HEAD；不重试 HTTP 状态码，由下载层处理
                    // Retry-After、镜像切换和已校验的断点续传。
                    let delay = retry_delay(retries);
                    retries += 1;
                    if started.elapsed().saturating_add(delay) >= timeout {
                        return Err(error.without_url().into());
                    }
                    tracing::debug!(
                        host = request.url().host_str(),
                        ?delay,
                        "Retrying interrupted HTTPS request"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(error) => return Err(error.without_url().into()),
            };
            if matches!(
                response.status(),
                StatusCode::MOVED_PERMANENTLY
                    | StatusCode::FOUND
                    | StatusCode::SEE_OTHER
                    | StatusCode::TEMPORARY_REDIRECT
                    | StatusCode::PERMANENT_REDIRECT
            ) {
                if let Some(location) = response.headers().get(header::LOCATION) {
                    let target = response.url().join(location.to_str()?)?;
                    validate_url(&target)?;
                    // HTTPS 响应即使指向本机，也禁止降级为明文 HTTP。
                    if request.url().scheme() == "https" && target.scheme() != "https" {
                        bail!(t!("network.https_downgrade"));
                    }
                    if redirects >= self.redirect_limit {
                        bail!(t!("network.redirect_limit"));
                    }
                    redirects += 1;
                    if target.origin() != request.url().origin() {
                        for name in [
                            header::AUTHORIZATION,
                            header::COOKIE,
                            header::PROXY_AUTHORIZATION,
                        ] {
                            request.headers_mut().remove(name);
                        }
                    }
                    *request.url_mut() = target;
                    // 每一个重定向目标独立选择协议，不继承上一主机的 HTTP/3 能力。
                    continue;
                }
            }
            tracing::debug!(host = response.url().host_str(), version = ?response.version(), "Launcher HTTP response");
            return Ok(Response {
                inner: response,
                chunk: Bytes::new(),
                client: self.clone(),
            });
        }
    }
}

fn retryable(error: &reqwest::Error) -> bool {
    // 证书、URL、重定向和协议校验错误不属于可重试的断连。
    if error.is_timeout() {
        return true;
    }
    let mut source = std::error::Error::source(error);
    while let Some(error) = source {
        if let Some(error) = error.downcast_ref::<hyper::Error>() {
            if error.is_incomplete_message() || error.is_closed() {
                return true;
            }
        }
        if let Some(error) = error.downcast_ref::<io::Error>() {
            if interrupted_io(error) {
                return true;
            }
        }
        source = error.source();
    }
    false
}

fn interrupted_io(mut error: &io::Error) -> bool {
    loop {
        if matches!(
            error.kind(),
            io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::UnexpectedEof
                | io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionRefused
        ) {
            return true;
        }
        // TLS 握手会把底层 socket 错误包进 ErrorKind::Other。
        // io::Error::source() 会跳过内层 io::Error，需显式展开 get_ref()。
        match error
            .get_ref()
            .and_then(|source| source.downcast_ref::<io::Error>())
        {
            Some(inner) => error = inner,
            None => return false,
        }
    }
}

fn tls_compatibility_error(error: &reqwest::Error) -> bool {
    if retryable(error) {
        return true;
    }
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = cause {
        if let Some(error) = error.downcast_ref::<rustls::Error>() {
            return matches!(
                error,
                rustls::Error::InvalidMessage(_)
                    | rustls::Error::PeerIncompatible(_)
                    | rustls::Error::PeerMisbehaved(_)
                    | rustls::Error::AlertReceived(
                        rustls::AlertDescription::DecodeError
                            | rustls::AlertDescription::UnexpectedMessage
                            | rustls::AlertDescription::HandshakeFailure
                            | rustls::AlertDescription::IllegalParameter
                            | rustls::AlertDescription::RecordOverflow
                    )
            );
        }
        cause = match error.downcast_ref::<io::Error>() {
            Some(error) if error.get_ref().is_some() => error.get_ref().map(|inner| inner as _),
            _ => error.source(),
        };
    }
    false
}

fn retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(rand::thread_rng().gen_range(150..=300) * (1 << attempt.min(2)))
}

impl ClientBuilder {
    pub(crate) fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub(crate) fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.tcp = self
            .tcp
            .connect_timeout(timeout.min(Duration::from_secs(3)));
        self.plain_tcp = self.plain_tcp.connect_timeout(timeout);
        self.quic = self.quic.connect_timeout(timeout);
        self
    }

    pub(crate) fn no_proxy(mut self) -> Self {
        self.tcp = self.tcp.no_proxy();
        self.plain_tcp = self.plain_tcp.no_proxy();
        self
    }

    pub(crate) fn https_only(mut self, enabled: bool) -> Self {
        self.tcp = self.tcp.https_only(enabled);
        self.plain_tcp = self.plain_tcp.https_only(enabled);
        self.quic = self.quic.https_only(enabled);
        self
    }

    pub(crate) fn default_headers(mut self, headers: header::HeaderMap) -> Self {
        self.tcp = self.tcp.default_headers(headers.clone());
        self.plain_tcp = self.plain_tcp.default_headers(headers.clone());
        self.quic = self.quic.default_headers(headers);
        self
    }

    pub(crate) fn user_agent(mut self, value: &'static str) -> Self {
        self.tcp = self.tcp.user_agent(value);
        self.plain_tcp = self.plain_tcp.user_agent(value);
        self.quic = self.quic.user_agent(value);
        self
    }

    pub(crate) fn identity_pem(mut self, pem: &[u8]) -> Result<Self> {
        self.quic = self.quic.identity(reqwest::Identity::from_pem(pem)?);
        let certs =
            rustls_pemfile::certs(&mut io::Cursor::new(pem)).collect::<io::Result<Vec<_>>>()?;
        let key = rustls_pemfile::private_key(&mut io::Cursor::new(pem))?
            .ok_or_else(|| anyhow::anyhow!("mTLS identity contains no private key"))?;
        self.client_auth = Some((certs, key));
        Ok(self)
    }

    pub(crate) fn redirect_limit(mut self, limit: usize) -> Self {
        self.redirect_limit = limit;
        self
    }

    pub(crate) fn build(self) -> Result<Client> {
        let _guard = runtime().enter();
        let mut roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for certificate in self.extra_roots {
            roots.add(certificate)?;
        }
        let tls = rustls::ClientConfig::builder().with_root_certificates(roots);
        let mut tls = match self.client_auth {
            Some((certs, key)) => tls.with_client_auth_cert(certs, key)?,
            None => tls.with_no_client_auth(),
        };
        tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let mut fragmented_tls = tls.clone();
        fragmented_tls.max_fragment_size = Some(TLS_FRAGMENT_SIZE);
        let tcp = self.tcp.use_preconfigured_tls(fragmented_tls).build()?;
        let plain_tcp = self.plain_tcp.use_preconfigured_tls(tls).build()?;
        let quic = match self.quic.build() {
            Ok(client) => Some(client),
            Err(error) => {
                // 禁用 IPv6/UDP 等系统不具备 QUIC socket 时仍可使用 TCP。
                tracing::warn!("Cannot initialize HTTP/3 transport: {error}");
                None
            }
        };
        Ok(Client {
            tcp,
            plain_tcp,
            quic,
            timeout: self.timeout,
            redirect_limit: self.redirect_limit,
            h3_failures: Arc::new(Mutex::new(HashMap::new())),
            tls_compatibility: Arc::new(Mutex::new(HashMap::new())),
            resolver: self.resolver,
        })
    }
}

pub(crate) struct RequestBuilder {
    inner: reqwest::RequestBuilder,
    client: Client,
    validation_error: Option<String>,
}

impl RequestBuilder {
    fn try_clone(&self) -> Self {
        Self {
            inner: self.inner.try_clone().expect("GET/HEAD has no body"),
            client: self.client.clone(),
            validation_error: self.validation_error.clone(),
        }
    }
    pub(crate) fn header(mut self, key: header::HeaderName, value: &str) -> Self {
        self.inner = self.inner.header(key, value);
        self
    }

    pub(crate) fn timeout(mut self, timeout: Duration) -> Self {
        self.inner = self.inner.timeout(timeout);
        self
    }

    pub(crate) fn send(self) -> Result<Response> {
        self.send_with_cancel(&NEVER_CANCELLED)
    }

    pub(crate) fn send_with_cancel(self, cancel: &AtomicBool) -> Result<Response> {
        crate::launcher::ensure_not_cancelled(cancel)?;
        if let Some(error) = self.validation_error {
            bail!(error);
        }
        let request = self.inner.build()?;
        runtime().block_on(async {
            tokio::select! {
                result = self.client.execute(request) => result,
                _ = async {
                    while !cancel.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                } => {
                    crate::launcher::ensure_not_cancelled(cancel)?;
                    unreachable!("cancellation future only completes when cancelled")
                },
            }
        })
    }
}

#[derive(Debug)]
pub(crate) struct Response {
    inner: reqwest::Response,
    chunk: Bytes,
    client: Client,
}

impl Response {
    pub(crate) fn status(&self) -> StatusCode {
        self.inner.status()
    }
    pub(crate) fn headers(&self) -> &header::HeaderMap {
        self.inner.headers()
    }
    pub(crate) fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }
    pub(crate) fn error_for_status(self) -> reqwest::Result<Self> {
        self.inner.error_for_status_ref()?;
        Ok(self)
    }
}

impl Read for Response {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        while self.chunk.is_empty() {
            let chunk = runtime().block_on(self.inner.chunk()).map_err(|error| {
                if retryable(&error) || error.is_body() {
                    self.client.interrupted(self.inner.url());
                }
                io::Error::other(error.without_url())
            })?;
            match chunk {
                Some(chunk) => self.chunk = chunk,
                None => return Ok(0),
            }
        }
        let size = self.chunk.len().min(buffer.len());
        buffer[..size].copy_from_slice(&self.chunk[..size]);
        self.chunk = self.chunk.slice(size..);
        Ok(size)
    }
}

/// 清单和小型引导数据必须完整读完才可使用；响应体被重置时从头重新
/// 获取，协议和地址选择同样先于 JSON 解析，不依赖清单中的下载源。
pub(crate) fn read_limited(
    request: RequestBuilder,
    limit: usize,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<u8>> {
    let started = Instant::now();
    let built = request
        .inner
        .try_clone()
        .expect("GET/HEAD has no body")
        .build()?;
    let timeout = built.timeout().copied().unwrap_or(request.client.timeout);
    for attempt in 0..=MAX_TRANSPORT_RETRIES {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            bail!(t!("network.deadline"));
        }
        // 连接池复用半关闭连接时，请求阶段也会断连；这类传输错误与原
        // body 读取错误同样可重试。HTTP 状态码由 error_for_status 原样返回，
        // 交给上层入口回退，绝不在这里重试。
        let response = match crate::cancel::request(
            request.try_clone().timeout(remaining),
            &cancel,
        ) {
            Ok(response) => response,
            Err(error) => {
                if !retryable_anyhow(&error) || attempt == MAX_TRANSPORT_RETRIES {
                    return Err(error);
                }
                backoff(attempt, &cancel, timeout, started)?;
                continue;
            }
        };
        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        match crate::cancel::body(response, cancel.clone())
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
        {
            Ok(_) if bytes.len() <= limit => return Ok(bytes),
            Ok(_) => bail!(t!("network.response_limit")),
            Err(error) => {
                crate::launcher::ensure_not_cancelled(&cancel)?;
                let retry = error
                    .get_ref()
                    .and_then(|e| e.downcast_ref::<reqwest::Error>())
                    .is_some_and(|e| retryable(e) || e.is_body());
                if !retry || attempt == MAX_TRANSPORT_RETRIES {
                    return Err(error.into());
                }
                backoff(attempt, &cancel, timeout, started)?;
            }
        }
    }
    unreachable!()
}

/// 判断 anyhow 包装的请求错误是否为可重试的传输层断连。
fn retryable_anyhow(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<reqwest::Error>()
        .is_some_and(retryable)
}

fn backoff(
    attempt: usize,
    cancel: &AtomicBool,
    timeout: Duration,
    started: Instant,
) -> Result<()> {
    let delay = retry_delay(attempt).min(timeout.saturating_sub(started.elapsed()));
    let backoff_started = Instant::now();
    while backoff_started.elapsed() < delay {
        crate::launcher::ensure_not_cancelled(cancel)?;
        std::thread::sleep(
            delay
                .saturating_sub(backoff_started.elapsed())
                .min(Duration::from_millis(50)),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
