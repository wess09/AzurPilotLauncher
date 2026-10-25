//! 启动器自更新：清单检查与显式更新分离，保留平台替换和下载校验。
use crate::{progress, setup::SplashUpdate};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONTENT_RANGE, RANGE, USER_AGENT},
    StatusCode,
};
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::AtomicBool, mpsc, Arc},
    thread,
    time::{Duration, Instant},
};
use tauri::Url;
use tempfile::Builder as TempDirBuilder;

/// 启动器更新清单主 URL：编译期由构建脚本经环境变量注入。
pub(crate) const LAUNCHER_UPDATE_URL: &str = env!("LAUNCHER_UPDATE_URL");
/// 主 URL 不可用时的更新清单回退 URL。
pub(crate) const LAUNCHER_UPDATE_FALLBACK_URL: &str =
    "https://ap.launcher-update.nanoda.work/updata/stable.json";
/// 存在该环境变量时跳过启动器更新检查（自更新重启后的新一轮启动会带上）。
const LAUNCHER_UPDATE_SKIP_ENV: &str = "AZURPILOT_SKIP_LAUNCHER_UPDATE";
/// 迷你版启动器的版本号：迷你版即使已是"最新"也必须升级到正式版。
const MINI_LAUNCHER_VERSION: &str = "0.0.1";
/// 下载更新载荷所需的 mTLS 客户端身份（PEM），构建脚本生成后内嵌；
/// 空内容表示未配置 mTLS。
const LAUNCHER_UPDATE_MTLS_IDENTITY: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/launcher_mtls_identity.pem"));
/// 更新请求使用的浏览器 User-Agent，附带 AZURPILOT_LAUNCHER_UPDATE 标识
/// 便于服务端识别流量来源。
const LAUNCHER_UPDATE_BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36 AZURPILOT_LAUNCHER_UPDATE/2.0.4";
/// 并行下载更新载荷的最大连接数。
const LAUNCHER_UPDATE_MAX_CONNECTIONS: usize = 8;
/// 并行分片的最小字节数：载荷不足单片时退回单连接顺序下载。
const LAUNCHER_UPDATE_MIN_CHUNK_BYTES: u64 = 1024 * 1024;
/// 更新下载占用 splash 进度条的起始百分比（8 之前留给更新前的检查）。
const LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START: u8 = 8;
/// 更新下载占用 splash 进度条的结束百分比（88 之后留给校验与重启）。
const LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_END: u8 = 88;
/// Windows 专用：存在该环境变量表示当前进程由更新助手拉起，启动时不
/// 附着父进程控制台（读取后立即删除，避免传递给子进程）。
#[cfg(windows)]
pub(crate) const LAUNCHER_UPDATE_NO_CONSOLE_ENV: &str = "AZURPILOT_NO_ATTACH_CONSOLE";
/// Windows 专用：更新助手模式的命令行标记，后跟目标 exe 路径与载荷路径。
#[cfg(windows)]
const LAUNCHER_UPDATE_APPLY_ARG: &str = "--apply-launcher-update";
#[derive(Debug, Deserialize)]
struct LauncherUpdateManifest {
    /// 清单声明的最新启动器版本。
    version: String,
    /// 平台标识（如 windows-x86_64）到对应载荷的映射。
    platforms: HashMap<String, LauncherUpdatePlatform>,
    /// 金丝雀通道载荷；与顶层正式版结构一致，缺失表示尚无金丝雀构建。
    #[serde(default)]
    canary: Option<LauncherUpdateChannelManifest>,
    #[serde(default)]
    runtime: Option<serde_json::Value>,
}

/// 金丝雀通道的版本与平台载荷，结构同清单顶层的正式版字段。
#[derive(Debug, Deserialize)]
struct LauncherUpdateChannelManifest {
    /// 金丝雀版本号（时间戳+提交哈希后缀，如 `3.0.2-canary.20261026.1750.9f3a2b1c`）。
    version: String,
    /// 平台标识到对应载荷的映射。
    platforms: HashMap<String, LauncherUpdatePlatform>,
}

/// 更新通道：正式版走清单顶层字段，金丝雀版走 `canary` 字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) enum UpdateChannel {
    /// 正式版（默认）。
    #[default]
    Stable,
    /// 金丝雀版：获取每次 main 分支构建的产物。
    Canary,
}

/// 更新下载源：决定载荷下载地址（url / github_url / cloudflare_url）
/// 的尝试顺序；清单始终从官方域名拉取，一份 json 包含全部源地址。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum UpdateSource {
    /// 自动（默认）：国内（简体中文+北京时间）官方源优先；海外
    /// GitHub 优先，不可达时回退 Cloudflare CDN，再回退官方源。
    #[default]
    Auto,
    /// 官方源：launcher-download.azurpilot.com（国内 CNB 加速）。
    Official,
    /// GitHub Release（海外）。
    Github,
    /// Cloudflare CDN 国际源。
    Cloudflare,
}

impl UpdateSource {
    /// 按下载源返回载荷 URL 的尝试顺序。
    fn order_payload_urls<'a>(
        self,
        url: &'a str,
        github_url: Option<&'a str>,
        cloudflare_url: Option<&'a str>,
    ) -> Vec<&'a str> {
        let mut official = vec![url];
        let github = github_url.filter(|candidate| *candidate != url);
        let cloudflare = cloudflare_url.filter(|candidate| *candidate != url);
        match self {
            UpdateSource::Official => UpdateSource::push_all(official, [github, cloudflare]),
            UpdateSource::Github => {
                let mut ordered = Vec::new();
                if let Some(candidate) = github {
                    ordered.push(candidate);
                }
                ordered.push(url);
                if let Some(candidate) = cloudflare {
                    ordered.push(candidate);
                }
                ordered
            }
            UpdateSource::Cloudflare => {
                let mut ordered = Vec::new();
                if let Some(candidate) = cloudflare {
                    ordered.push(candidate);
                }
                ordered.push(url);
                if let Some(candidate) = github {
                    ordered.push(candidate);
                }
                ordered
            }
            UpdateSource::Auto => {
                // 与组件下载源同一判定：简体中文且北京时间视为国内环境。
                if crate::sources::DownloadSource::Auto.prefer_cnb() {
                    UpdateSource::push_all(official, [github, cloudflare])
                } else {
                    official.clear();
                    if let Some(candidate) = github {
                        official.push(candidate);
                    }
                    if let Some(candidate) = cloudflare {
                        official.push(candidate);
                    }
                    official.push(url);
                    official
                }
            }
        }
    }

    fn push_all<'a>(
        mut urls: Vec<&'a str>,
        extra: [Option<&'a str>; 2],
    ) -> Vec<&'a str> {
        for candidate in extra.into_iter().flatten() {
            if !urls.contains(&candidate) {
                urls.push(candidate);
            }
        }
        urls
    }
}

/// 单个平台的更新载荷信息。
#[derive(Clone, Debug, Deserialize)]
struct LauncherUpdatePlatform {
    /// 载荷下载 URL（清单公开，载荷本身需 mTLS）。
    url: String,
    /// GitHub Release 的同一份载荷；兼容没有此字段的旧清单。
    #[serde(default)]
    github_url: Option<String>,
    /// Cloudflare CDN 的同一份载荷；兼容没有此字段的旧清单。
    #[serde(default)]
    cloudflare_url: Option<String>,
    /// 载荷的 SHA-256 十六进制摘要，下载后用于完整性校验。
    sha256: String,
}

/// 并行下载的一个字节区间（闭区间 [start, end]）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LauncherUpdateByteRange {
    /// 区间起始字节偏移（含）。
    start: u64,
    /// 区间结束字节偏移（含）。
    end: u64,
}

fn launcher_update_browser_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(LAUNCHER_UPDATE_BROWSER_UA),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/octet-stream,application/json,text/plain,*/*;q=0.8"),
    );
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
    );
    headers
}

/// 构造启动器更新专用的 reqwest 阻塞客户端。
///
/// # Errors
/// 客户端构建失败（如 mTLS 身份解析出错）时返回 Err。
fn launcher_update_http_client(
    timeout: Option<Duration>,
    with_mtls_identity: bool,
) -> Result<Client> {
    // 15 秒连接超时兜底；整体超时由调用方按用途传入。
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .no_proxy()
        .default_headers(launcher_update_browser_headers());
    // 仅请求更新载荷时携带 mTLS 身份；清单是公开数据，无需证书。
    if with_mtls_identity && !LAUNCHER_UPDATE_MTLS_IDENTITY.is_empty() {
        builder = builder.identity(reqwest::Identity::from_pem(LAUNCHER_UPDATE_MTLS_IDENTITY)?);
    }
    // 载荷总时限 15 分钟；任务取消会立即停止文件写入，不等待 socket 超时。
    builder = match timeout {
        Some(timeout) => builder.timeout(timeout),
        None => builder.timeout(Duration::from_secs(15 * 60)),
    };
    Ok(builder.build()?)
}

/// 拉取启动器更新清单：先试主 URL，失败后回退备用 URL。
///
/// # Errors
/// 所有 URL 均失败时返回 Err。
fn fetch_launcher_update_manifest(
    client: &Client,
    cancel: &Arc<AtomicBool>,
) -> Result<LauncherUpdateManifest> {
    fetch_launcher_update_manifest_from_urls(
        client,
        &[LAUNCHER_UPDATE_URL, LAUNCHER_UPDATE_FALLBACK_URL],
        cancel,
    )
}

/// 依次尝试给定的清单 URL 列表，取第一个成功的结果。
///
/// # Errors
/// 全部 URL 都失败时返回 Err，错误信息累积了各 URL 的失败详情，便于
/// 同时排查主/备两个源的问题。
fn fetch_launcher_update_manifest_from_urls(
    client: &Client,
    urls: &[&str],
    cancel: &Arc<AtomicBool>,
) -> Result<LauncherUpdateManifest> {
    let mut failures = Vec::new();

    for (index, url) in urls.iter().enumerate() {
        // 跳过重复 URL（主备地址可能配置成同一个）。
        if urls[..index].iter().any(|previous| previous == url) {
            continue;
        }

        crate::launcher::ensure_not_cancelled(cancel)?;
        match fetch_launcher_update_manifest_from_url(client, url, cancel) {
            Ok(manifest) => {
                if index > 0 {
                    tracing::info!("Using fallback launcher update manifest: {url}");
                }
                return Ok(manifest);
            }
            Err(error) => {
                tracing::warn!("Unable to fetch launcher update manifest from {url}: {error:#}");
                failures.push(format!("{url}: {error:#}"));
            }
        }
    }

    bail!(
        "Unable to fetch launcher update manifest from all configured URLs: {}",
        failures.join("; ")
    )
}

/// 从单个 URL 拉取并解析启动器更新清单 JSON。
///
/// # Errors
/// 请求失败、非 2xx 状态、读取响应体或 JSON 解析任一步失败时返回 Err。
fn fetch_launcher_update_manifest_from_url(
    client: &Client,
    url: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<LauncherUpdateManifest> {
    let response = crate::cancel::request(client.get(url), cancel)?
        .error_for_status()
        .with_context(|| format!("validate launcher update manifest response from {url}"))?;
    let mut manifest_text = String::new();
    crate::cancel::body(response, cancel.clone())
        .take(1024 * 1024)
        .read_to_string(&mut manifest_text)?;
    serde_json::from_str(&manifest_text)
        .with_context(|| format!("parse launcher update manifest from {url}"))
}

/// 判断给定版本号是否为迷你版启动器（仅版本 0.0.1，忽略 v 前缀）。
fn launcher_version_is_mini(version: &str) -> bool {
    version.strip_prefix('v').unwrap_or(version) == MINI_LAUNCHER_VERSION
}

/// 仅检查清单与当前平台载荷；发现新版返回待用户确认的更新信息。
/// 检查失败阻止启动；下载、校验与替换由 install_update 单独执行。
///
/// `channel` 决定读取清单的哪一组版本与载荷：正式版走顶层字段，
/// 金丝雀版走 `canary` 字段（缺失视为暂无构建，等同“已是最新”）。
pub(crate) fn check_update(
    cancel: &Arc<AtomicBool>,
    channel: UpdateChannel,
) -> Result<Option<AvailableUpdate>> {
    crate::launcher::ensure_not_cancelled(cancel)?;
    // 自更新重启后的新一轮启动带有跳过标记：取下标记并直接放行。
    if std::env::var_os(LAUNCHER_UPDATE_SKIP_ENV).is_some() {
        tracing::info!("Skipping launcher update check after restart");
        std::env::remove_var(LAUNCHER_UPDATE_SKIP_ENV);
        return Ok(None);
    }

    // 当前版本、迷你版标记与平台标识共同决定检查结果与下载载荷。
    let current_version = env!("CARGO_PKG_VERSION");
    let mini_launcher = launcher_version_is_mini(current_version);
    let platform_key = launcher_update_platform_key();
    let manifest_client = match launcher_update_http_client(Some(Duration::from_secs(10)), false) {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!("Unable to create launcher update client: {err:#}");
            return Err(anyhow!(t!(
                "launcher_update.check_failed",
                error = format!("{err:#}")
            )));
        }
    };
    let manifest = match fetch_launcher_update_manifest(&manifest_client, cancel) {
        Ok(manifest) => manifest,
        Err(err) => {
            return Err(anyhow!(t!(
                "launcher_update.check_failed",
                error = format!("{err:#}")
            )));
        }
    };
    crate::launcher::ensure_not_cancelled(cancel)?;
    if let Some(runtime) = manifest.runtime.clone() {
        if let Err(error) =
            crate::components::remember_manifest(&crate::setup::azurpilot_repo_dir(), runtime)
        {
            tracing::warn!("Unable to cache runtime file manifest: {error:#}");
        }
    }
    let (latest_version, latest_platforms) = match channel {
        UpdateChannel::Stable => (manifest.version.clone(), &manifest.platforms),
        UpdateChannel::Canary => {
            let Some(canary) = manifest.canary.as_ref() else {
                tracing::info!("No canary payload in update manifest; treating as up to date");
                if mini_launcher {
                    return Err(anyhow!(t!(
                        "launcher_update.mini_update_missing",
                        current = current_version,
                        latest = manifest.version.clone()
                    )));
                }
                return Ok(None);
            };
            (canary.version.clone(), &canary.platforms)
        }
    };
    let update_available = launcher_version_is_newer(current_version, &latest_version)
        .ok_or_else(|| {
            tracing::warn!(
                "Launcher update manifest contains an invalid version: {}",
                latest_version
            );
            anyhow!(t!(
                "launcher_update.invalid_manifest_version",
                version = latest_version.clone()
            ))
        })?;
    if !update_available {
        tracing::info!(
            "Launcher is up to date: current={}, latest={}",
            current_version,
            latest_version
        );
        // 迷你版不允许停留：即使无更新也报错，提示安装正式版。
        if mini_launcher {
            return Err(anyhow!(t!(
                "launcher_update.mini_update_missing",
                current = current_version,
                latest = latest_version
            )));
        }
        return Ok(None);
    }

    // 清单中没有当前平台的载荷：无法更新，报错提示。
    let Some(platform) = latest_platforms.get(platform_key) else {
        tracing::warn!("No launcher update payload for platform {platform_key}");
        return Err(anyhow!(t!(
            "launcher_update.payload_missing",
            platform = platform_key
        )));
    };

    validate_launcher_update_payload(&platform.url, &platform.sha256)?;
    for url in [&platform.github_url, &platform.cloudflare_url]
        .into_iter()
        .flatten()
    {
        validate_launcher_update_payload(url, &platform.sha256)?;
    }
    Ok(Some(AvailableUpdate {
        version: latest_version,
        payload: platform.clone(),
    }))
}

#[derive(Clone, Debug)]
pub(crate) struct AvailableUpdate {
    pub version: String,
    payload: LauncherUpdatePlatform,
}

#[cfg(test)]
mod source_tests {
    use super::*;
    #[test]
    fn github_payload_is_used_after_primary_failure_and_old_manifests_still_work() {
        let payload: LauncherUpdatePlatform = serde_json::from_value(serde_json::json!({
            "url": "https://launcher-download.azurpilot.com/a.exe",
            "github_url": "https://github.com/wess09/AzurPilotLauncher/releases/download/v3/a.exe",
            "cloudflare_url": "https://launcher-cloudflare-cdn-download.azurpilot.com/a.exe",
            "sha256": "0".repeat(64)
        }))
        .unwrap();
        let mut requested = Vec::new();
        download_from_sources(&payload, &AtomicBool::new(false), UpdateSource::Official, |url| {
            requested.push(url.to_owned());
            if requested.len() == 1 {
                bail!("primary unavailable");
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(requested.len(), 2);
        assert_eq!(Some(requested[1].as_str()), payload.github_url.as_deref());
        let old: LauncherUpdatePlatform = serde_json::from_value(
            serde_json::json!({"url": "https://example.com/a", "sha256": "0".repeat(64)}),
        )
        .unwrap();
        assert!(old.github_url.is_none());
        assert!(old.cloudflare_url.is_none());
    }
    #[test]
    fn cancelling_primary_does_not_start_github_request() {
        let payload = LauncherUpdatePlatform {
            url: "https://example.com/a".into(),
            github_url: Some("https://github.com/a".into()),
            cloudflare_url: None,
            sha256: "0".repeat(64),
        };
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        assert!(download_from_sources(&payload, &cancel, UpdateSource::Official, |_| {
            calls += 1;
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            bail!("cancelled")
        })
        .is_err());
        assert_eq!(calls, 1);
    }

    /// 下载源排序：GitHub / Cloudflare / 海外自动按各自优先级排列载荷地址。
    #[test]
    fn payload_url_order_follows_update_source() {
        let url = "https://launcher-download.azurpilot.com/a.exe";
        let github = "https://github.com/wess09/AzurPilotLauncher/releases/download/v3/a.exe";
        let cloudflare = "https://launcher-cloudflare-cdn-download.azurpilot.com/a.exe";
        let order = |source: UpdateSource| {
            source.order_payload_urls(url, Some(github), Some(cloudflare))
        };
        assert_eq!(
            order(UpdateSource::Official),
            vec![url, github, cloudflare]
        );
        assert_eq!(
            order(UpdateSource::Github),
            vec![github, url, cloudflare]
        );
        assert_eq!(
            order(UpdateSource::Cloudflare),
            vec![cloudflare, url, github]
        );
        // 自动模式按地区分流（本机与 CI 的地区不同），只断言全部地址
        // 都在候选里且无重复。
        let auto = order(UpdateSource::Auto);
        assert_eq!(auto.len(), 3);
        assert!(auto.contains(&url));
        assert!(auto.contains(&github));
        assert!(auto.contains(&cloudflare));
    }
}

pub(crate) fn install_update(
    update: &AvailableUpdate,
    cancel: Arc<AtomicBool>,
    mut status_updater: impl FnMut(SplashUpdate),
    before_commit: impl FnOnce() -> Result<()>,
    source: UpdateSource,
) -> Result<()> {
    let current_exe = std::env::current_exe()?;
    let update_path = launcher_update_temp_path(&current_exe);
    download_from_sources(&update.payload, &cancel, source, |url| {
        download_launcher_update(
            url,
            &update_path,
            &update.payload.sha256,
            cancel.clone(),
            &mut status_updater,
        )
    })?;
    crate::launcher::ensure_not_cancelled(&cancel)?;
    make_executable(&update_path)?;
    before_commit()?;
    status_updater(SplashUpdate::loading(
        t!("launcher_update.restart_title"),
        t!("launcher_update.restarting_detail"),
        100,
    ));
    replace_launcher_and_restart(&current_exe, &update_path)
}

/// 按下载源顺序尝试各载荷地址，全部失败时汇总报错。
fn download_from_sources(
    payload: &LauncherUpdatePlatform,
    cancel: &AtomicBool,
    source: UpdateSource,
    mut download: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let mut failures = Vec::new();
    let sources = source.order_payload_urls(
        &payload.url,
        payload.github_url.as_deref(),
        payload.cloudflare_url.as_deref(),
    );
    for url in sources {
        crate::launcher::ensure_not_cancelled(cancel)?;
        match download(url) {
            Ok(()) => return Ok(()),
            Err(error) => {
                tracing::warn!("Launcher payload {url}: {error:#}");
                failures.push(format!("{error:#}"));
            }
        }
    }
    crate::launcher::ensure_not_cancelled(cancel)?;
    bail!("{}", failures.join("; "))
}

/// 下载启动器更新载荷到本地临时路径，并校验 SHA-256。
///
/// 先探测服务器是否支持 HTTP Range：支持且载荷足够大时多线程并行分片
/// 下载，否则退回单连接顺序下载。下载或校验失败时清理残留文件。
///
/// # Errors
/// 载荷 URL 校验失败、下载中断、合并后字节数不符或 SHA-256 不匹配时
/// 返回 Err。
fn download_launcher_update(
    url: &str,
    update_path: &Path,
    expected_sha256: &str,
    cancel: Arc<AtomicBool>,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<()> {
    validate_launcher_update_payload(url, expected_sha256)?;

    // 载荷 URL 来自公开清单；ESA 边缘节点要求载荷请求携带 mTLS 客户端证书。
    let with_mtls = Url::parse(url)?.host_str() != Some("github.com");
    let client = launcher_update_http_client(None, with_mtls)?;
    // 预清理残留的 .part 与目标文件，保证本次下载从零开始。
    let part_path = launcher_update_part_path(update_path);
    remove_launcher_update_file_if_exists(&part_path)?;
    remove_launcher_update_file_if_exists(update_path)?;

    tracing::info!("Downloading launcher update from {url}");
    // 先用 bytes=0-0 探测 Range 支持并获取总字节数，再决定下载策略。
    let range_total = launcher_update_range_total(&client, url, &cancel)?;
    let download_result = match range_total {
        Some(total_bytes) => {
            let ranges = launcher_update_byte_ranges(total_bytes);
            // 载荷大于一个分片才值得并行，小文件单连接更快更省事。
            if ranges.len() > 1 {
                status_updater(
                    SplashUpdate::loading(
                        t!("launcher_update.updating"),
                        t!(
                            "launcher_update.parallel_downloading_detail",
                            connections = ranges.len().to_string()
                        ),
                        LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START,
                    )
                    .with_subtitle(t!("launcher_update.status")),
                );
                download_launcher_update_parallel(
                    &client,
                    url,
                    total_bytes,
                    &ranges,
                    &part_path,
                    &mut status_updater,
                    &cancel,
                )
            } else {
                tracing::info!("Launcher update payload is too small for parallel download");
                download_launcher_update_sequential(
                    &client,
                    url,
                    &part_path,
                    Some(total_bytes),
                    &mut status_updater,
                    &cancel,
                )
            }
        }
        None => {
            tracing::info!(
                "Launcher update server does not support HTTP byte ranges; using one connection"
            );
            download_launcher_update_sequential(
                &client,
                url,
                &part_path,
                None,
                &mut status_updater,
                &cancel,
            )
        }
    };
    // 下载失败时清理 .part 与目标文件，避免留下损坏的半成品。
    let _downloaded = match download_result {
        Ok(downloaded) => downloaded,
        Err(err) => {
            cleanup_launcher_update_download_files(&part_path, update_path);
            return Err(err);
        }
    };

    status_updater(
        SplashUpdate::loading(
            t!("launcher_update.updating"),
            t!("launcher_update.verifying_detail"),
            92,
        )
        .with_subtitle(t!("launcher_update.status")),
    );

    crate::launcher::ensure_not_cancelled(&cancel)?;
    // 下载完成后进入 SHA-256 校验与落地阶段（.part 重命名为正式文件）。
    let downloaded =
        match verify_and_promote_launcher_update(&part_path, update_path, expected_sha256) {
            Ok(downloaded) => downloaded,
            Err(err) => {
                cleanup_launcher_update_download_files(&part_path, update_path);
                return Err(err);
            }
        };

    tracing::info!(
        "Launcher update downloaded: {} bytes -> {}",
        downloaded,
        update_path.display()
    );
    Ok(())
}

/// 校验更新载荷 URL 与摘要格式：仅接受带主机的 HTTPS URL 与 64 位十六进制
/// SHA-256 摘要，防止清单被篡改成明文 HTTP 或本地地址。
///
/// # Errors
/// URL 不是 https、缺少主机名或摘要格式非法时返回 Err。
fn validate_launcher_update_payload(url: &str, expected_sha256: &str) -> Result<()> {
    let parsed_url =
        Url::parse(url).with_context(|| format!("invalid launcher update URL: {url}"))?;
    if parsed_url.scheme() != "https" || parsed_url.host_str().is_none() {
        bail!("launcher update URL must use HTTPS and include a host: {url}");
    }
    if !launcher_update_sha256_is_valid(expected_sha256) {
        bail!("launcher update manifest contains an invalid SHA-256 digest");
    }
    Ok(())
}

/// 判断字符串是否为合法的 SHA-256 十六进制摘要（64 个十六进制字符）。
fn launcher_update_sha256_is_valid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 探测更新载荷服务器是否支持 HTTP Range，并取得载荷总字节数。
///
/// 发送 bytes=0-0 的探测请求；仅当响应为 206 且 Content-Range 表明总大小
/// 有效时返回 Some(total)。
///
/// # Errors
/// 探测请求本身失败（网络错误、非 2xx）时返回 Err；服务器不支持 Range
/// 时返回 Ok(None)。
fn launcher_update_range_total(
    client: &Client,
    url: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<Option<u64>> {
    let response = crate::cancel::request(client.get(url).header(RANGE, "bytes=0-0"), cancel)?
        .error_for_status()?;
    // 非 206 说明服务器忽略了 Range，无法并行，也拿不到可靠总大小。
    if response.status() != StatusCode::PARTIAL_CONTENT {
        return Ok(None);
    }

    let Some((start, end, total)) = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_launcher_update_content_range)
    else {
        return Ok(None);
    };
    // 探测请求只取 1 字节：start/end 必须都是 0 才能据此信任 total。
    if start != 0 || end != 0 || total == 0 {
        return Ok(None);
    }
    Ok(Some(total))
}

/// 解析 Content-Range 头（形如 `bytes 10-19/42`）。
///
/// # Returns
/// 合法且满足 start <= end < total 时返回 Some((start, end, total))；
/// 其余情况（含通配 `*`）返回 None。
fn parse_launcher_update_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.trim().strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    let total = total.parse().ok()?;
    (start <= end && end < total).then_some((start, end, total))
}

/// 把载荷总字节数均匀切分为若干连续字节区间。
///
/// 区间数量按"每片至少 LAUNCHER_UPDATE_MIN_CHUNK_BYTES"向上取整，并夹在
/// 1 与 LAUNCHER_UPDATE_MAX_CONNECTIONS 之间；整除余数逐片分摊给前面的
/// 区间，保证各区间首尾相接且恰好覆盖整个载荷。
fn launcher_update_byte_ranges(total_bytes: u64) -> Vec<LauncherUpdateByteRange> {
    if total_bytes == 0 {
        return Vec::new();
    }

    // 分片数 = 向上取整(总量/单片最小值)，并夹在 [1, MAX_CONNECTIONS] 之间。
    let range_count = total_bytes
        .saturating_add(LAUNCHER_UPDATE_MIN_CHUNK_BYTES - 1)
        .checked_div(LAUNCHER_UPDATE_MIN_CHUNK_BYTES)
        .unwrap_or(1)
        .clamp(1, LAUNCHER_UPDATE_MAX_CONNECTIONS as u64) as usize;
    let base_size = total_bytes / range_count as u64;
    let extra_bytes = total_bytes % range_count as u64;
    let mut start = 0;
    let mut ranges = Vec::with_capacity(range_count);

    for index in 0..range_count {
        let size = base_size + u64::from(index < extra_bytes as usize);
        let end = start + size - 1;
        ranges.push(LauncherUpdateByteRange { start, end });
        start = end + 1;
    }
    ranges
}

/// 多线程并行下载各字节区间到临时目录的分片文件，随后按序合并。
///
/// 每个分片由独立线程下载，经 mpsc 通道上报进度；主循环每 100ms 汇聚一次
/// 增量并节流刷新 splash。全部完成后校验总字节数一致才执行合并。
///
/// # Errors
/// 任一 worker panic、分片字节数合计与总大小不符或合并失败时返回 Err。
fn download_launcher_update_parallel(
    client: &Client,
    url: &str,
    total_bytes: u64,
    ranges: &[LauncherUpdateByteRange],
    part_path: &Path,
    status_updater: &mut impl FnMut(SplashUpdate),
    cancel: &Arc<AtomicBool>,
) -> Result<u64> {
    // 分片先写入独立临时目录，全部校验通过后才合并到正式 .part 路径。
    let temp_dir = TempDirBuilder::new()
        .prefix("azurpilot-launcher-update-")
        .tempdir()
        .context("create temporary launcher update download directory")?;
    let (progress_sender, progress_receiver) = mpsc::channel();
    let mut workers = Vec::with_capacity(ranges.len());
    let mut chunk_paths = Vec::with_capacity(ranges.len());

    // 每个区间一个 worker 线程，独立持有客户端克隆、URL 与分片路径。
    for (index, range) in ranges.iter().copied().enumerate() {
        let chunk_path = temp_dir.path().join(format!("chunk-{index:02}"));
        let worker_client = client.clone();
        let worker_url = url.to_owned();
        let worker_path = chunk_path.clone();
        let worker_sender = progress_sender.clone();
        let worker_cancel = cancel.clone();
        workers.push(thread::spawn(move || {
            download_launcher_update_range(
                &worker_client,
                &worker_url,
                range,
                &worker_path,
                &worker_sender,
                &worker_cancel,
            )
        }));
        chunk_paths.push(chunk_path);
    }
    // 主动丢弃主发送端：worker 全部结束后通道断开，主循环得以退出。
    drop(progress_sender);

    let started_at = Instant::now();
    let mut downloaded_so_far = 0u64;
    let mut last_reported_at = Instant::now() - Duration::from_secs(1);
    // 每 100ms 醒一次：汇总各 worker 上报的进度增量并节流刷新 splash。
    while workers.iter().any(|worker| !worker.is_finished()) {
        match progress_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(downloaded) => {
                downloaded_so_far = downloaded_so_far.saturating_add(downloaded);
                report_launcher_update_download_progress(
                    status_updater,
                    downloaded_so_far,
                    total_bytes,
                    started_at,
                    &mut last_reported_at,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // worker 收尾后再清空一次通道，避免漏计尾部进度。
    while let Ok(downloaded) = progress_receiver.try_recv() {
        downloaded_so_far = downloaded_so_far.saturating_add(downloaded);
    }

    // join 各 worker 并累计字节数：任一 worker panic 视为整体失败。
    let mut completed_bytes = 0u64;
    let mut failure = None;
    for worker in workers {
        match worker.join() {
            Ok(Ok(bytes)) => completed_bytes = completed_bytes.saturating_add(bytes),
            Ok(Err(error)) => {
                if failure.is_none() {
                    failure = Some(error);
                }
            }
            Err(_) => {
                if failure.is_none() {
                    failure = Some(anyhow!("launcher update download worker panicked"));
                }
            }
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }
    crate::launcher::ensure_not_cancelled(cancel)?;
    if completed_bytes != total_bytes {
        bail!(
            "launcher update download incomplete: expected {} bytes, got {} bytes",
            total_bytes,
            completed_bytes
        );
    }

    // 合并前把进度刷满下载区间，避免 UI 停在中间值。
    report_launcher_update_download_progress(
        status_updater,
        total_bytes,
        total_bytes,
        started_at,
        &mut last_reported_at,
    );
    let merged_bytes = merge_launcher_update_chunks(&chunk_paths, part_path)?;
    if merged_bytes != total_bytes {
        bail!(
            "launcher update merge incomplete: expected {} bytes, got {} bytes",
            total_bytes,
            merged_bytes
        );
    }
    Ok(merged_bytes)
}

/// 下载单个字节区间到指定分片文件。
///
/// 严格校验响应：必须为 206、Content-Range 与请求区间一致、Content-Length
/// （如存在）等于区间长度，防止服务器忽略 Range 导致分片错位。
///
/// # Errors
/// 校验失败、写文件失败或实际下载数与区间长度不符时返回 Err。
fn download_launcher_update_range(
    client: &Client,
    url: &str,
    range: LauncherUpdateByteRange,
    chunk_path: &Path,
    progress_sender: &mpsc::Sender<u64>,
    cancel: &Arc<AtomicBool>,
) -> Result<u64> {
    let requested_range = format!("bytes={}-{}", range.start, range.end);
    let response = crate::cancel::request(client.get(url).header(RANGE, requested_range), cancel)?
        .error_for_status()?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        bail!(
            "launcher update server ignored byte range {}-{}",
            range.start,
            range.end
        );
    }

    let Some((start, end, _)) = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_launcher_update_content_range)
    else {
        bail!("launcher update range response is missing a valid Content-Range header");
    };
    if start != range.start || end != range.end {
        bail!(
            "launcher update range response does not match requested bytes {}-{}",
            range.start,
            range.end
        );
    }

    // 期望字节数即闭区间长度（两端均含）。
    let expected_bytes = range.end - range.start + 1;
    if response
        .content_length()
        .is_some_and(|content_length| content_length != expected_bytes)
    {
        bail!(
            "launcher update range response has wrong length for bytes {}-{}",
            range.start,
            range.end
        );
    }

    // 边读边写分片文件，并把每次写入的字节数经通道上报给进度汇总。
    let mut response = crate::cancel::body(response, cancel.clone());
    let mut file = fs::File::create(chunk_path)?;
    let mut downloaded = 0u64;
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let size = response.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        file.write_all(&buffer[..size])?;
        downloaded += size as u64;
        let _ = progress_sender.send(size as u64);
    }
    file.flush()?;

    // 最终核对：实际下载数必须与区间长度完全一致。
    if downloaded != expected_bytes {
        bail!(
            "launcher update range download incomplete for bytes {}-{}: expected {} bytes, got {} bytes",
            range.start,
            range.end,
            expected_bytes,
            downloaded
        );
    }
    Ok(downloaded)
}

/// 按顺序把各分片文件合并写入目标 .part 文件。
///
/// # Errors
/// 创建输出文件、读取分片或写盘失败时返回 Err。
fn merge_launcher_update_chunks(chunk_paths: &[PathBuf], part_path: &Path) -> Result<u64> {
    let mut output = fs::File::create(part_path).with_context(|| {
        t!(
            "errors.write_update_failed",
            error = part_path.display().to_string()
        )
    })?;
    let mut written = 0u64;
    for chunk_path in chunk_paths {
        let mut chunk = fs::File::open(chunk_path)?;
        written = written.saturating_add(std::io::copy(&mut chunk, &mut output)?);
    }
    output.flush().with_context(|| {
        t!(
            "errors.write_update_failed",
            error = part_path.display().to_string()
        )
    })?;
    Ok(written)
}

/// 汇总下载字节，以 100ms 节奏上报；保留小数且完成时立即上报。
fn report_launcher_update_download_progress(
    status_updater: &mut impl FnMut(SplashUpdate),
    downloaded: u64,
    total_bytes: u64,
    started_at: Instant,
    last_reported_at: &mut Instant,
) {
    if last_reported_at.elapsed() >= progress::UPDATE_INTERVAL || downloaded >= total_bytes {
        let (progress, detail) =
            launcher_download_progress_detail(downloaded, Some(total_bytes), started_at);
        *last_reported_at = Instant::now();
        status_updater(
            SplashUpdate::loading(t!("launcher_update.updating"), detail, progress)
                .with_subtitle(t!("launcher_update.status")),
        );
    }
}

/// 校验 .part 文件的 SHA-256，通过后重命名为正式更新文件。
///
/// # Errors
/// 摘要不匹配（同时删除 .part）、读取元数据或重命名失败时返回 Err。
fn verify_and_promote_launcher_update(
    part_path: &Path,
    update_path: &Path,
    expected_sha256: &str,
) -> Result<u64> {
    let digest_hex = sha256_file(part_path)?;
    if !digest_hex.eq_ignore_ascii_case(expected_sha256) {
        // 摘要不匹配：删除半成品，避免下次误用。
        let _ = fs::remove_file(part_path);
        bail!(
            "launcher update sha256 mismatch: expected {}, got {}",
            expected_sha256,
            digest_hex
        );
    }

    let downloaded = fs::metadata(part_path)?.len();
    remove_launcher_update_file_if_exists(update_path)?;
    // 同目录 rename 原子性较好：校验通过后"晋升"为正式更新文件。
    fs::rename(part_path, update_path).with_context(|| {
        format!(
            "promote verified launcher update from {} to {}",
            part_path.display(),
            update_path.display()
        )
    })?;
    Ok(downloaded)
}

/// 由最终更新文件路径推导下载中的临时 .part 路径（同名追加 .part 后缀）。
fn launcher_update_part_path(update_path: &Path) -> PathBuf {
    let Some(file_name) = update_path.file_name() else {
        return update_path.with_extension("part");
    };
    let mut part_name = file_name.to_os_string();
    part_name.push(".part");
    update_path.with_file_name(part_name)
}

/// 删除指定文件；文件不存在视为成功。
///
/// # Errors
/// 删除时发生非 NotFound 的 IO 错误时返回 Err。
fn remove_launcher_update_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// 清理下载失败后的 .part 与目标文件；清理失败仅记录告警，不中断调用方
/// 的错误处理流程。
fn cleanup_launcher_update_download_files(part_path: &Path, update_path: &Path) {
    for path in [part_path, update_path] {
        if let Err(error) = remove_launcher_update_file_if_exists(path) {
            tracing::warn!(
                "Unable to clean launcher update download {}: {error}",
                path.display()
            );
        }
    }
}

/// 单连接顺序下载更新载荷到指定路径，边下载边节流上报进度。
///
/// # Errors
/// 请求失败、写盘失败或与预期总字节数不符时返回 Err。
fn download_launcher_update_sequential(
    client: &Client,
    url: &str,
    update_path: &Path,
    expected_total_bytes: Option<u64>,
    mut status_updater: impl FnMut(SplashUpdate),
    cancel: &Arc<AtomicBool>,
) -> Result<u64> {
    let response = crate::cancel::request(client.get(url), cancel)?.error_for_status()?;
    // 优先使用 Range 探测得到的总大小，其次取响应 Content-Length。
    let total_bytes = expected_total_bytes.or_else(|| response.content_length());
    let mut response = crate::cancel::body(response, cancel.clone());
    let mut file = fs::File::create(update_path).with_context(|| {
        t!(
            "errors.write_update_failed",
            error = update_path.display().to_string()
        )
    })?;
    let mut downloaded = 0u64;
    let mut buffer = [0u8; 128 * 1024];
    let mut last_reported_at = Instant::now() - Duration::from_secs(1);
    let download_started_at = Instant::now();

    loop {
        let size = response
            .read(&mut buffer)
            .with_context(|| t!("errors.download_update_failed", url = url))?;
        if size == 0 {
            break;
        }
        file.write_all(&buffer[..size]).with_context(|| {
            t!(
                "errors.write_update_failed",
                error = update_path.display().to_string()
            )
        })?;
        downloaded += size as u64;

        if last_reported_at.elapsed() >= progress::UPDATE_INTERVAL {
            let (progress, detail) =
                launcher_download_progress_detail(downloaded, total_bytes, download_started_at);
            last_reported_at = Instant::now();
            status_updater(
                SplashUpdate::loading(t!("launcher_update.updating"), detail, progress)
                    .with_subtitle(t!("launcher_update.status")),
            );
        }
    }
    file.flush().with_context(|| {
        t!(
            "errors.write_update_failed",
            error = update_path.display().to_string()
        )
    })?;

    // 已知总大小时核对下载完整性。
    if let Some(total_bytes) = total_bytes {
        if downloaded != total_bytes {
            return Err(anyhow!(
                "launcher update download incomplete: expected {} bytes, got {} bytes",
                total_bytes,
                downloaded
            ));
        }
    }

    let (_, detail) =
        launcher_download_progress_detail(downloaded, total_bytes, download_started_at);
    status_updater(
        SplashUpdate::loading(
            t!("launcher_update.updating"),
            detail,
            LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_END,
        )
        .with_subtitle(t!("launcher_update.status")),
    );
    Ok(downloaded)
}

/// 分块计算指定文件的 SHA-256 摘要，返回小写十六进制字符串。
///
/// # Errors
/// 文件打开或读取失败时返回 Err。
fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];

    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hasher.update(&buffer[..size]);
    }

    let digest = hasher.finalize();
    Ok(bytes_to_hex(&digest))
}

/// 根据已下载字节数生成 splash 进度值与详情文案。
///
/// 已知总大小时把下载百分比线性映射进 [START, END] 进度区间；未知时按
/// 每 MiB 一格推进并封顶在 END，文案改用"已下载/未知总量"版本。
fn launcher_download_progress_detail(
    downloaded: u64,
    total_bytes: Option<u64>,
    started_at: Instant,
) -> (f64, String) {
    let speed = format_speed(download_speed_bytes_per_second(downloaded, started_at));
    if let Some(total) = total_bytes.filter(|total| *total > 0) {
        let percentage = downloaded.min(total) as f64 / total as f64 * 100.0;
        let detail = t!(
            "launcher_update.downloading_detail",
            downloaded = format_bytes(downloaded),
            total = format_bytes(total),
            percent = (percentage.floor() as u8).to_string(),
            speed = speed
        )
        .to_string();
        let progress_span =
            LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_END - LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START;
        let progress = f64::from(LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START)
            + percentage * f64::from(progress_span) / 100.0;
        return (progress, detail);
    }

    let mib_downloaded = downloaded as f64 / (1024.0 * 1024.0);
    let progress = (f64::from(LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START) + mib_downloaded)
        .min(f64::from(LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_END));
    let detail = t!(
        "launcher_update.downloading_detail_unknown",
        downloaded = format_bytes(downloaded),
        speed = speed
    )
    .to_string();
    (progress, detail)
}

/// 把字节数格式化为人类可读字符串（B/KiB/MiB/GiB，保留 1 位小数）。
pub(crate) fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.1} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.1} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// 计算平均下载速度（字节/秒）；耗时不足 0.1 秒时按 0.1 秒折算，避免除零。
pub(crate) fn download_speed_bytes_per_second(downloaded: u64, started_at: Instant) -> f64 {
    let elapsed = started_at.elapsed().as_secs_f64().max(0.1);
    downloaded as f64 / elapsed
}

/// 把速度（字节/秒）四舍五入后复用字节数格式化输出。
pub(crate) fn format_speed(bytes_per_second: f64) -> String {
    format_bytes(bytes_per_second.max(0.0).round() as u64)
}

/// 返回当前操作系统与架构对应的更新清单平台标识（如 windows-x86_64）。
/// 未识别的组合返回 "unknown"（清单中无该载荷，更新会报缺失）。
fn launcher_update_platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "x86") => "windows-i686",
        ("windows", "aarch64") => "windows-aarch64",
        _ => "unknown",
    }
}

/// 比较版本号，判断 latest 是否严格大于 current。
///
/// # Returns
/// 任一版本号无法解析时返回 None（调用方按无效清单处理）。
fn launcher_version_is_newer(current: &str, latest: &str) -> Option<bool> {
    let current = parse_launcher_version(current)?;
    let latest = parse_launcher_version(latest)?;
    Some(latest > current)
}

/// 解析启动器版本号为可比较的四元组 (major, minor, patch, 后缀序号)。
///
/// 支持 `v` 前缀、`-` 预发布后缀与 `+` 构建元数据；后缀序号取预发布段
/// 末尾的连续数字（如 build.3 -> 3，无数字视为 0），实现 alpha/beta 之类
/// 预发布版本间的次序比较。
///
/// # Returns
/// 格式非法（缺段、段非数字、后缀非法）时返回 None。
fn parse_launcher_version(version: &str) -> Option<(u64, u64, u64, u64)> {
    let version = version.strip_prefix('v').unwrap_or(version);
    // 构建元数据（+ 后段）只做合法性校验，不参与比较。
    let version = match version.split_once('+') {
        Some((version, build_metadata)) if valid_launcher_version_suffix(build_metadata) => version,
        Some(_) => return None,
        None => version,
    };
    let (core, suffix) = version.split_once('-').unwrap_or((version, ""));
    if version.contains('-') && !valid_launcher_version_suffix(suffix) {
        return None;
    }
    let mut nums = core.split('.');
    let major = nums.next()?.parse::<u64>().ok()?;
    let minor = nums.next()?.parse::<u64>().ok()?;
    let patch = nums.next()?.parse::<u64>().ok()?;
    if nums.next().is_some() {
        return None;
    }
    // 后缀序号：取预发布段末尾连续数字并倒序重组（如 rc12 -> 12）。
    // 正式版（无预发布段）取 u64::MAX，保证同 core 版本下
    // 3.0.2 > 3.0.2-canary.*（预发布不应高于正式发布，切换通道时能
    // 从金丝雀回到同号正式版）。
    let suffix_rank = if suffix.is_empty() {
        u64::MAX
    } else {
        suffix
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    };
    Some((major, minor, patch, suffix_rank))
}

/// 判断语义化版本后缀是否合法：非空且以 `.` 分隔的各段均由 ASCII 字母、
/// 数字或连字符组成。
fn valid_launcher_version_suffix(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

/// 检查命令行参数（跳过 argv[0]）中是否出现给定标志之一（忽略大小写）。
fn restart_arguments() -> Vec<std::ffi::OsString> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    restart_arguments_from(args)
}

fn restart_arguments_from(args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    if args
        .first()
        .is_some_and(|arg| arg == "--apply-launcher-update")
    {
        args.into_iter().skip(3).collect()
    } else {
        args
    }
}

fn launcher_update_temp_path(current_exe: &Path) -> PathBuf {
    let file_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("AzurPilot");
    std::env::temp_dir().join(format!(
        "azurpilot-launcher-update-{}-{file_name}",
        std::process::id()
    ))
}

/// 把字节序列格式化为小写十六进制字符串。
fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// 确保更新后的可执行文件具备执行权限。
///
/// 平台差异：Unix 需显式 chmod 0o755；Windows 的可执行性由 .exe 扩展名
/// 决定，无需处理。
///
/// # Errors
/// Unix 上读取或设置权限失败时返回 Err。
fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// 非 Windows 平台的自更新落地：直接把新可执行文件重命名覆盖旧文件，
/// 随后带"跳过更新"环境变量拉起新进程。
///
/// Unix 允许覆盖正在运行的程序文件（inode 已映射），因此无需助手进程。
///
/// # Errors
/// 重命名或拉起新进程失败时返回 Err。
#[cfg(not(windows))]
fn replace_launcher_and_restart(current_exe: &Path, update_path: &Path) -> Result<()> {
    fs::rename(update_path, current_exe).with_context(|| {
        t!(
            "errors.replace_launcher_failed",
            error = current_exe.display().to_string()
        )
    })?;
    Command::new(current_exe)
        .args(restart_arguments())
        .env(LAUNCHER_UPDATE_SKIP_ENV, "1")
        .spawn()
        .with_context(|| {
            t!(
                "errors.restart_launcher_failed",
                error = current_exe.display().to_string()
            )
        })?;
    Ok(())
}

/// Windows 平台的自更新落地：运行中的 exe 无法直接覆盖自身，因此先复制
/// 自身为临时助手进程，再由助手等待本进程退出后完成替换与重启。
///
/// # Errors
/// 复制自身或启动助手失败时返回 Err。
#[cfg(windows)]
fn replace_launcher_and_restart(current_exe: &Path, update_path: &Path) -> Result<()> {
    let helper_path = std::env::temp_dir().join(format!(
        "azurpilot-launcher-update-helper-{}.exe",
        std::process::id()
    ));

    fs::copy(current_exe, &helper_path).with_context(|| {
        t!(
            "errors.copy_file_failed",
            src = current_exe.display().to_string(),
            dest = helper_path.display().to_string()
        )
    })?;

    use std::os::windows::process::CommandExt;
    use winapi::um::winbase::CREATE_NO_WINDOW;
    // CREATE_NO_WINDOW：助手是控制台程序的副本，执行时避免闪出黑窗。
    Command::new(&helper_path)
        .arg(LAUNCHER_UPDATE_APPLY_ARG)
        .arg(current_exe)
        .arg(update_path)
        .args(std::env::args_os().skip(1))
        .env(LAUNCHER_UPDATE_SKIP_ENV, "1")
        .env(LAUNCHER_UPDATE_NO_CONSOLE_ENV, "1")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .with_context(|| {
            t!(
                "errors.start_update_script_failed",
                error = helper_path.display().to_string()
            )
        })?;
    Ok(())
}

/// Windows 更新助手模式入口：检查命令行是否携带 --apply-launcher-update。
///
/// 命中时按"目标 exe 路径 + 更新载荷路径"执行替换并重启，返回 Ok(true)
/// 表示本进程以助手身份完成使命，main 应直接返回、不再走正常启动流程。
///
/// # Errors
/// 有标记但缺少路径参数，或替换过程失败时返回 Err。
#[cfg(windows)]
pub(crate) fn try_apply_launcher_update_from_args() -> Result<bool> {
    use std::ffi::OsStr;

    // argv[1] 是模式标记，argv[2]/argv[3] 分别为目标 exe 与更新载荷路径。
    let mut args = std::env::args_os();
    let _ = args.next();
    let Some(mode) = args.next() else {
        return Ok(false);
    };
    if mode != OsStr::new(LAUNCHER_UPDATE_APPLY_ARG) {
        return Ok(false);
    }

    let target_path = args
        .next()
        .ok_or_else(|| anyhow!("missing launcher update target path"))?;
    let update_path = args
        .next()
        .ok_or_else(|| anyhow!("missing launcher update payload path"))?;
    apply_launcher_update_and_restart(PathBuf::from(target_path), PathBuf::from(update_path))?;
    Ok(true)
}

/// 助手核心逻辑：以 1 秒间隔至多重试 60 次替换目标 exe（等待旧进程退出并
/// 释放文件占用），成功后重启启动器并把助手自身的临时副本登记为重启后删除。
///
/// # Errors
/// 60 次重试后仍无法替换时返回最后一次的 Err。
#[cfg(windows)]
fn apply_launcher_update_and_restart(target_path: PathBuf, update_path: PathBuf) -> Result<()> {
    let mut last_error = None;
    // 每次失败休眠 1 秒再试：给旧进程留出退出并释放 exe 句柄的时间。
    for _ in 0..60 {
        match move_file_replace(&update_path, &target_path) {
            Ok(()) => {
                restart_launcher_after_update(&target_path)?;
                schedule_file_delete_on_reboot(&std::env::current_exe()?);
                return Ok(());
            }
            Err(err) => {
                last_error = Some(err);
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("launcher update replacement timed out")))
}

/// 调用 Win32 MoveFileExW 以"替换已存在 + 允许跨卷 + 直写磁盘"标志移动
/// 文件，实现覆盖式替换。
///
/// # Errors
/// API 返回 0（替换失败）时返回携带 last_os_error 的 Err。
#[cfg(windows)]
fn move_file_replace(from: &Path, to: &Path) -> Result<()> {
    use winapi::um::winbase::{
        MoveFileExW, MOVEFILE_COPY_ALLOWED, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let from_wide = path_to_wide(from);
    let to_wide = path_to_wide(to);
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH;
    let moved = unsafe { MoveFileExW(from_wide.as_ptr(), to_wide.as_ptr(), flags) };
    if moved == 0 {
        return Err(anyhow!(
            "{}: {}",
            t!(
                "errors.replace_launcher_failed",
                error = to.display().to_string()
            ),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// 更新落地后重启启动器：带"跳过更新"与"无控制台"环境变量拉起新 exe。
///
/// # Errors
/// 进程创建失败时返回 Err。
#[cfg(windows)]
fn restart_launcher_after_update(target_path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use winapi::um::winbase::CREATE_NO_WINDOW;

    Command::new(target_path)
        .args(restart_arguments())
        .env(LAUNCHER_UPDATE_SKIP_ENV, "1")
        .env(LAUNCHER_UPDATE_NO_CONSOLE_ENV, "1")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .with_context(|| {
            t!(
                "errors.restart_launcher_failed",
                error = target_path.display().to_string()
            )
        })?;
    Ok(())
}

/// 把指定文件登记为系统重启时删除（MOVEFILE_DELAY_UNTIL_REBOOT）。
/// 此处用于清理更新助手自身（旧启动器的临时副本）；登记失败无伤大雅，
/// 因此忽略返回值。
#[cfg(windows)]
fn schedule_file_delete_on_reboot(path: &Path) {
    use std::ptr;
    use winapi::um::winbase::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

    let path_wide = path_to_wide(path);
    let _ = unsafe { MoveFileExW(path_wide.as_ptr(), ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
}

/// 把路径编码为以空终止符结尾的 Windows 宽字符（UTF-16），供 Win32 API 使用。
#[cfg(windows)]
fn path_to_wide(path: &Path) -> Vec<u16> {
    use std::{iter, os::windows::ffi::OsStrExt};

    path.as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}

/// main.rs 纯函数逻辑的单元测试：覆盖时间炸弹配置解析、Cargo.toml 键值
#[cfg(test)]
mod tests {
    use super::*;
    use crate::TAURI_CONFIG_SOURCE;
    use crate::{
        cargo_toml_section, cargo_toml_value, time_bomb_config, truncate_log_file,
        windows::{backend_unready_error, main_window_titlebar_injection_script},
    };

    #[test]
    fn update_helper_preserves_locale_and_minimized_arguments() {
        let args = [
            "--apply-launcher-update",
            "target.exe",
            "update.exe",
            "--lang",
            "ja",
            "--start-minimized",
        ]
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect();
        assert_eq!(
            restart_arguments_from(args),
            ["--lang", "ja", "--start-minimized"].map(std::ffi::OsString::from)
        );
    }

    /// 时间炸弹段 enabled 时应能解析出配置（且与 enabled 值一致）。
    #[test]
    fn test_time_bomb_config_parses_when_enabled() {
        let section =
            cargo_toml_section("package.metadata.azurpilot.time-bomb").expect("section exists");
        let enabled = cargo_toml_value(section, "enabled").as_deref() == Some("true");
        let config = time_bomb_config().expect("time bomb config parses");
        assert_eq!(config.is_some(), enabled);
    }

    /// cargo_toml_value 应能读到 expires-at 与中文 message 字段。
    #[test]
    fn test_cargo_toml_value_reads_time_bomb_fields() {
        let section =
            cargo_toml_section("package.metadata.azurpilot.time-bomb").expect("section exists");
        assert!(cargo_toml_value(section, "expires-at").is_some());
        assert_eq!(
            Some("测试已结束，请安装正式版".to_owned()),
            cargo_toml_value(section, "message")
        );
    }

    /// backend_unready_error 携带日志失败原因时拼接展示，无原因时保持原样。
    #[test]
    fn backend_unready_error_carries_the_reason_from_the_log() {
        let with_reason = backend_unready_error(
            anyhow!("connect failed"),
            Some("React 前端构建失败".to_owned()),
        );
        assert!(format!("{with_reason:#}").contains("React 前端构建失败"));

        let without_reason = backend_unready_error(anyhow!("connect failed"), None);
        assert_eq!(format!("{without_reason:#}"), "connect failed");
    }

    /// 英文环境下 splash HTML 的文案应来自 JSON 字面量（而非 JS 单引号拼接）。

    /// splash HTML 应包含可选的 uv 安装进度子条相关标记与样式。

    /// 截断日志文件应把已存在的旧内容清空。
    #[test]
    fn test_truncate_log_file_replaces_existing_contents() {
        let temp_dir = TempDirBuilder::new()
            .prefix("launcher-log-truncate-test-")
            .tempdir()
            .expect("create temporary log directory");
        let filename = "launcher.txt";
        let path = temp_dir.path().join(filename);
        fs::write(&path, "old launcher log").expect("write old log");

        truncate_log_file(temp_dir.path(), filename).expect("truncate launcher log");

        assert_eq!(fs::read(&path).expect("read truncated log"), b"");
    }

    /// 标题栏应使用 WebView 可拖拽区域（pointerdown + app-region）支持触摸拖动。
    #[test]
    fn test_titlebars_use_webview_draggable_regions_for_touch_dragging() {
        #[cfg(not(target_os = "macos"))]
        let titlebar_script = main_window_titlebar_injection_script();

        #[cfg(not(target_os = "macos"))]
        {
            assert!(titlebar_script.contains("touch-action:none"));
            assert!(titlebar_script.contains("addEventListener('pointerdown'"));
            assert!(titlebar_script.contains("window_start_dragging"));
            assert!(titlebar_script.contains("-webkit-app-region:no-drag"));
            assert!(titlebar_script.contains(
                ".azurpilot-titlebar-drag-zone{position:absolute;inset:0 148px 0 0;height:100%;pointer-events:none"
            ));
            assert!(titlebar_script.contains("getComputedStyle(element).cursor === 'pointer'"));
            assert!(titlebar_script.contains("min-height:28px"));
            assert!(titlebar_script.contains("background:rgba(250,250,247,.78)"));
            assert!(titlebar_script.contains(".icon-close{color:#e64f58}"));
            assert!(titlebar_script.contains("--azurpilot-titlebar-height:56px"));
            assert!(titlebar_script.contains("transform:translateY(-6px) scale(.96)"));
            assert!(!titlebar_script.contains("scale(.72)"));
            assert!(titlebar_script.contains("azurpilot-close-menu"));
            assert!(!titlebar_script.contains("azurpilot-close-optics"));
            assert!(!titlebar_script.contains("azurpilot-island-open"));
            assert!(titlebar_script.contains("__AZURPILOT_OPEN_CLOSE_PROMPT"));
            assert!(titlebar_script.contains("window_exit_application"));
            assert!(!titlebar_script.contains("addEventListener('scroll'"));
            assert!(!titlebar_script.contains("MutationObserver"));
            assert!(titlebar_script.contains("data-theme*="));
            assert!(titlebar_script.contains("#azurpilot-launcher-titlebar.is-dark"));
        }
    }

    /// tauri.conf.json 中 Windows 窗口应启用 WebView2 拖拽区域等浏览器参数。
    #[test]
    fn test_windows_enable_webview_draggable_regions() {
        let config: serde_json::Value =
            serde_json::from_str(TAURI_CONFIG_SOURCE).expect("valid config");
        let windows = config["app"]["windows"].as_array().expect("window configs");

        for window in windows {
            let args = window["additionalBrowserArgs"]
                .as_str()
                .expect("draggable regions arguments");
            assert!(args.contains("msWebView2EnableDraggableRegions"));
            assert!(args.contains("ElasticOverscroll"));
            assert!(args.contains("msWebOOUI,msPdfOOUI,msSmartScreenProtection"));
            assert!(args.contains("--no-proxy-server"));
        }
    }

    /// 版本比较：合法版本可比较，非法版本返回 None。
    #[test]
    fn test_launcher_update_versions_must_be_valid() {
        assert_eq!(launcher_version_is_newer("2.1.6", "2.1.7"), Some(true));
        assert_eq!(
            launcher_version_is_newer("2.1.6", "2.1.6+build.1"),
            Some(false)
        );
        assert_eq!(launcher_version_is_newer("2.1.6", "not-a-version"), None);
        assert_eq!(launcher_version_is_newer("2.1", "2.1.7"), None);
        assert_eq!(launcher_version_is_newer("2.1.6", "2.1.7-"), None);
    }

    /// 金丝雀版本排序：时间戳后缀单调递增，正式版恒高于同 core 预发布。
    #[test]
    fn test_launcher_update_channel_version_ordering() {
        let newer = |current: &str, latest: &str| launcher_version_is_newer(current, latest);
        assert_eq!(
            newer(
                "3.0.2-canary.9f3a2b1c.20261026175030",
                "3.0.2-canary.9f3a2b1c.20261026175102"
            ),
            Some(true)
        );
        assert_eq!(
            newer(
                "3.0.2-canary.9f3a2b1c.20261026175102",
                "3.0.2-canary.0a1b2c3d.20261026175030"
            ),
            Some(false)
        );
        // 正式版高于金丝雀：切回正式通道后能从金丝雀回到同号正式版。
        assert_eq!(
            newer("3.0.2-canary.9f3a2b1c.20261026175102", "3.0.2"),
            Some(true)
        );
        assert_eq!(
            newer("3.0.2", "3.0.2-canary.9f3a2b1c.20261026175102"),
            Some(false)
        );
        // core 提升时先按 core 比较，不受通道后缀影响。
        assert_eq!(
            newer("3.0.2-canary.9f3a2b1c.20261026175102", "3.0.3"),
            Some(true)
        );
    }

    /// 主清单 URL 失败（503）时应回退到备用 URL 并成功解析。
    #[test]
    fn test_launcher_update_manifest_uses_fallback_url() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let server = std::thread::spawn(move || {
            let fallback_body = r#"{"version":"2.1.8","platforms":{}}"#;
            let responses = [
                (
                    "/primary",
                    "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_owned(),
                ),
                (
                    "/fallback",
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{fallback_body}",
                        fallback_body.len()
                    ),
                ),
            ];

            for (expected_path, response) in responses {
                let (mut stream, _) = listener.accept().expect("accept manifest request");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("set read timeout");
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0u8; 1024];
                    let read = stream.read(&mut buffer).expect("read manifest request");
                    assert!(read > 0, "manifest request ended before headers");
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request);
                assert!(request.starts_with(&format!("GET {expected_path} ")));
                stream
                    .write_all(response.as_bytes())
                    .expect("write manifest response");
            }
        });

        let client = Client::builder().no_proxy().build().expect("build client");
        let primary = format!("http://{address}/primary");
        let fallback = format!("http://{address}/fallback");
        let manifest = fetch_launcher_update_manifest_from_urls(
            &client,
            &[&primary, &fallback],
            &Arc::new(AtomicBool::new(false)),
        )
        .expect("fetch manifest from fallback");

        assert_eq!(manifest.version, "2.1.8");
        server.join().expect("manifest server completed");
    }

    /// 载荷校验：仅接受 HTTPS URL 与合法 SHA-256 摘要。
    #[test]
    fn test_launcher_update_payload_requires_https_and_sha256() {
        let digest = "a".repeat(64);

        assert!(
            validate_launcher_update_payload("https://updates.example/launcher", &digest).is_ok()
        );
        assert!(
            validate_launcher_update_payload("http://updates.example/launcher", &digest).is_err()
        );
        assert!(validate_launcher_update_payload(
            "https://updates.example/launcher",
            "not-a-digest"
        )
        .is_err());
    }

    /// 字节区间切分：区间数量、覆盖范围与无重叠应满足约定。
    #[test]
    fn test_launcher_update_byte_ranges_cover_payload_once() {
        let total_bytes = LAUNCHER_UPDATE_MIN_CHUNK_BYTES * 8 + 17;
        let ranges = launcher_update_byte_ranges(total_bytes);

        assert_eq!(ranges.len(), LAUNCHER_UPDATE_MAX_CONNECTIONS);
        assert_eq!(ranges.first().map(|range| range.start), Some(0));
        assert_eq!(ranges.last().map(|range| range.end), Some(total_bytes - 1));
        assert_eq!(
            ranges
                .iter()
                .map(|range| range.end - range.start + 1)
                .sum::<u64>(),
            total_bytes
        );
        assert!(ranges
            .windows(2)
            .all(|pair| pair[0].end + 1 == pair[1].start));
    }

    /// Content-Range 解析：合法值与非法值（通配、倒序、无前缀）的处理。
    #[test]
    fn test_launcher_update_content_range_parser() {
        assert_eq!(
            parse_launcher_update_content_range("bytes 10-19/42"),
            Some((10, 19, 42))
        );
        assert_eq!(parse_launcher_update_content_range("bytes 0-0/*"), None);
        assert_eq!(parse_launcher_update_content_range("bytes 19-10/42"), None);
        assert_eq!(parse_launcher_update_content_range("not-a-range"), None);
    }

    /// 服务器忽略 Range 直接返回 200 时，探测应返回 None 以走顺序下载。
    #[test]
    fn test_launcher_update_range_probe_falls_back_when_ignored() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept range probe");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set read timeout");
            let mut request = Vec::new();
            loop {
                let mut buffer = [0u8; 1024];
                let read = stream.read(&mut buffer).expect("read range probe");
                assert!(read > 0, "range probe ended before headers");
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            assert!(String::from_utf8_lossy(&request)
                .lines()
                .any(|line| line.eq_ignore_ascii_case("range: bytes=0-0")));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("write range probe response");
        });

        let client = Client::builder().no_proxy().build().expect("build client");
        let url = format!("http://{address}/launcher");
        assert_eq!(
            launcher_update_range_total(&client, &url, &Arc::new(AtomicBool::new(false)))
                .expect("probe range support"),
            None
        );
        server.join().expect("range probe server completed");
    }

    /// 并行下载应按 Range 取回各分片，并合并出与原载荷一致的文件。
    #[test]
    fn test_parallel_launcher_update_download_merges_ranges() {
        let payload: Vec<u8> = (0..(LAUNCHER_UPDATE_MIN_CHUNK_BYTES * 2 + 17))
            .map(|index| (index % 251) as u8)
            .collect();
        let payload = Arc::new(payload);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let request_count = launcher_update_byte_ranges(payload.len() as u64).len() + 1;
        let server_payload = Arc::clone(&payload);
        let server = std::thread::spawn(move || {
            for _ in 0..request_count {
                let (mut stream, _) = listener.accept().expect("accept range request");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("set read timeout");
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0u8; 1024];
                    let read = stream.read(&mut buffer).expect("read range request");
                    assert!(read > 0, "range request ended before headers");
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }

                let request = String::from_utf8_lossy(&request);
                let range = request
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        if name.eq_ignore_ascii_case("range") {
                            value.trim().strip_prefix("bytes=")
                        } else {
                            None
                        }
                    })
                    .expect("range request header");
                let (start, end) = range.split_once('-').expect("range bounds");
                let start: usize = start.parse().expect("range start");
                let end: usize = end.parse().expect("range end");
                assert!(start <= end && end < server_payload.len());
                let body = &server_payload[start..=end];
                let response = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                    body.len(),
                    start,
                    end,
                    server_payload.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write headers");
                stream.write_all(body).expect("write range body");
            }
        });

        let client = Client::builder().no_proxy().build().expect("build client");
        let url = format!("http://{address}/launcher");
        let temp_dir = TempDirBuilder::new()
            .prefix("launcher-range-test-")
            .tempdir()
            .unwrap();
        let part_path = temp_dir.path().join("launcher.part");
        let total_bytes =
            launcher_update_range_total(&client, &url, &Arc::new(AtomicBool::new(false)))
                .expect("probe range support")
                .expect("server supports ranges");
        let ranges = launcher_update_byte_ranges(total_bytes);
        let mut progress = Vec::new();

        let downloaded = download_launcher_update_parallel(
            &client,
            &url,
            total_bytes,
            &ranges,
            &part_path,
            &mut |update| progress.push(update.progress),
            &Arc::new(AtomicBool::new(false)),
        )
        .expect("parallel launcher download");

        assert_eq!(downloaded, payload.len() as u64);
        let merged = fs::read(&part_path).expect("read merged file");
        assert_eq!(merged.as_slice(), payload.as_slice());
        assert!(progress
            .iter()
            .any(|value| *value > f64::from(LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START)));
        server.join().expect("range server completed");
    }

    /// 校验晋升：摘要匹配才允许重命名落地，不匹配时清理半成品。
    #[test]
    fn test_launcher_update_promotion_requires_valid_digest() {
        let temp_dir = TempDirBuilder::new()
            .prefix("launcher-promotion-test-")
            .tempdir()
            .unwrap();
        let part_path = temp_dir.path().join("launcher.part");
        let update_path = temp_dir.path().join("launcher.update");
        fs::write(&part_path, b"verified update").expect("write update part");
        let digest = sha256_file(&part_path).expect("hash update part");

        assert_eq!(
            verify_and_promote_launcher_update(&part_path, &update_path, &digest)
                .expect("promote verified update"),
            b"verified update".len() as u64
        );
        assert!(!part_path.exists());
        assert_eq!(
            fs::read(&update_path).expect("read promoted update"),
            b"verified update"
        );

        let failed_part_path = temp_dir.path().join("failed.part");
        let failed_update_path = temp_dir.path().join("failed.update");
        fs::write(&failed_part_path, b"unverified update").expect("write failed update part");
        assert!(verify_and_promote_launcher_update(
            &failed_part_path,
            &failed_update_path,
            &"0".repeat(64),
        )
        .is_err());
        assert!(!failed_part_path.exists());
        assert!(!failed_update_path.exists());
    }
}
