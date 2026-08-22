// No default console window createion on Windows
#![windows_subsystem = "windows"]

mod autostart;
mod backend;
mod i18n;
mod launcher_control;
mod notify;
mod setup;
mod window_util;

#[macro_use]
extern crate rust_i18n;
i18n!("locales", fallback = "en");

use std::{
    cell::Cell,
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self},
    time::{Duration, Instant},
};

use crate::{
    backend::{is_backend_startup_timeout, ManagedBackend, WebuiLaunchConfig},
    launcher_control::start_launcher_control_stream,
    notify::{start_notify_stream, NotificationClickHandler},
    setup::{
        get_deploy_config, rebuild_venv_and_sync_dependencies, setup_alas_repo, setup_environment,
        SplashUpdate,
    },
};
use anyhow::{anyhow, bail, Context, Result};
use base64::{prelude::BASE64_STANDARD, Engine};
use chrono::{DateTime, FixedOffset, Local, Utc};
use reqwest::{
    blocking::Client,
    header::{
        HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONTENT_RANGE, DATE, RANGE, USER_AGENT,
    },
    StatusCode,
};
use rust_i18n::t;
use serde::Deserialize;
use serde_json::to_string;
use sha2::{Digest, Sha256};
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    webview::{PageLoadEvent, PageLoadPayload},
    Manager, State, Url, WebviewWindow,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_dialog::FilePath;
use tempfile::Builder as TempDirBuilder;
use tracing::{debug, error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

#[cfg(target_os = "macos")]
const MENUBAR_ICON_2X: &[u8] = include_bytes!("../icons/menubar@2x.png");
#[cfg(target_os = "macos")]
const MENUBAR_ICON_1X: &[u8] = include_bytes!("../icons/menubar.png");
#[cfg(windows)]
const WINDOWS_TRAY_ICON: &[u8] = include_bytes!("../icons/icon.png");
const SPLASH_BG_VIDEO: &[u8] = include_bytes!("../bg/bg.mp4");
const MI_SANS_FONT: &[u8] = include_bytes!("../fonts/MiSansLauncher.ttf");
const BACKEND_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const BACKEND_NAVIGATION_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(any(windows, target_os = "android"))]
const BACKEND_ERROR_URL_BASE: &str = "http://alas-error.localhost/backend";
#[cfg(not(any(windows, target_os = "android")))]
const BACKEND_ERROR_URL_BASE: &str = "alas-error://localhost/backend";
#[cfg(any(windows, target_os = "android"))]
const SPLASH_URL: &str = "http://alas-splash.localhost/";
#[cfg(not(any(windows, target_os = "android")))]
const SPLASH_URL: &str = "alas-splash://localhost/";
const TIME_BOMB_CONFIG_SOURCE: &str = include_str!("../Cargo.toml");
#[cfg(test)]
const TAURI_CONFIG_SOURCE: &str = include_str!("../tauri.conf.json");
const LAUNCHER_UPDATE_URL: &str = env!("LAUNCHER_UPDATE_URL");
const LAUNCHER_UPDATE_FALLBACK_URL: &str =
    "https://ap.launcher-update.nanoda.work/updata/stable.json";
const LAUNCHER_UPDATE_SKIP_ENV: &str = "AZURPILOT_SKIP_LAUNCHER_UPDATE";
const MINI_LAUNCHER_VERSION: &str = "0.0.1";
const LAUNCHER_UPDATE_MTLS_IDENTITY: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/launcher_mtls_identity.pem"));
const LAUNCHER_UPDATE_BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36 AZURPILOT_LAUNCHER_UPDATE/2.0.4";
const LAUNCHER_UPDATE_MAX_CONNECTIONS: usize = 8;
const LAUNCHER_UPDATE_MIN_CHUNK_BYTES: u64 = 1024 * 1024;
const LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START: u8 = 8;
const LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_END: u8 = 88;
#[cfg(windows)]
const LAUNCHER_UPDATE_NO_CONSOLE_ENV: &str = "AZURPILOT_NO_ATTACH_CONSOLE";
#[cfg(windows)]
const LAUNCHER_UPDATE_APPLY_ARG: &str = "--apply-launcher-update";
const PREVIEW_NO_UPDATE_ARGS: &[&str] = &[
    "--preview-no-update",
    "--skip-update",
    "--no-update",
    "--disable-update",
    "/preview-no-update",
    "/skip-update",
    "/no-update",
];
const PREVIEW_CRASH_ARGS: &[&str] = &[
    "--preview-crash",
    "--preview-error",
    "--crash-preview",
    "--error-preview",
    "/preview-crash",
    "/preview-error",
];
const START_MINIMIZED_ARGS: &[&str] = &["--start-minimized", "/start-minimized"];

struct ExitControl(Arc<AtomicBool>);

#[derive(Clone, Debug)]
struct TimeBombConfig {
    expires_at: DateTime<FixedOffset>,
    network_time_url: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct LauncherUpdateManifest {
    version: String,
    platforms: HashMap<String, LauncherUpdatePlatform>,
}

#[derive(Debug, Deserialize)]
struct LauncherUpdatePlatform {
    url: String,
    sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LauncherUpdateByteRange {
    start: u64,
    end: u64,
}

#[cfg(target_os = "macos")]
fn tray_icon_for_platform() -> Image<'static> {
    info!("Loading macOS tray icon from embedded bytes...");
    let result = Image::from_bytes(MENUBAR_ICON_2X)
        .or_else(|_| {
            info!("2x icon failed, trying 1x...");
            Image::from_bytes(MENUBAR_ICON_1X)
        })
        .unwrap_or_else(|err| {
            error!(
                ?err,
                "Failed to load tray icon from embedded menubar icon bytes (2x and 1x)."
            );
            panic!("Failed to load tray icon from embedded menubar icon bytes: {err}");
        });
    info!("Tray icon loaded successfully");
    result
}

#[cfg(windows)]
fn tray_icon_for_platform() -> Image<'static> {
    Image::from_bytes(WINDOWS_TRAY_ICON).unwrap_or_else(|err| {
        error!(?err, "Failed to load tray icon from embedded icon bytes.");
        panic!("Failed to load tray icon from embedded icon bytes: {err}");
    })
}

fn begin_startup_shutdown(
    app_handle: tauri::AppHandle,
    allow_exit: Arc<AtomicBool>,
    setup_cancel_requested: Arc<AtomicBool>,
    setup_running: Arc<AtomicBool>,
    startup_shutdown_started: Arc<AtomicBool>,
) {
    if startup_shutdown_started
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    setup_cancel_requested.store(true, Ordering::SeqCst);
    info!("Startup cancellation requested; preserving repository and runtime for diagnostics");

    thread::spawn(move || {
        let started_at = Instant::now();
        while setup_running.load(Ordering::SeqCst) && started_at.elapsed() < Duration::from_secs(30)
        {
            thread::sleep(Duration::from_millis(100));
        }

        if setup_running.load(Ordering::SeqCst) {
            warn!("Setup thread did not stop before startup shutdown timeout");
        }

        allow_exit.store(true, Ordering::SeqCst);
        app_handle.exit(0);
    });
}

fn time_bomb_config() -> Result<Option<TimeBombConfig>> {
    let Some(section) = cargo_toml_section("package.metadata.alas-launcher.time-bomb") else {
        return Ok(None);
    };
    let enabled = cargo_toml_value(section, "enabled")
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or(false);
    if !enabled {
        return Ok(None);
    }

    let expires_at = cargo_toml_value(section, "expires-at")
        .ok_or_else(|| anyhow!(t!("errors.time_bomb_not_configured")))?;
    let expires_at = DateTime::parse_from_rfc3339(&expires_at)
        .map_err(|err| anyhow!(t!("errors.time_bomb_format_error", error = err.to_string())))?;
    let network_time_url = cargo_toml_value(section, "network-time-url")
        .unwrap_or_else(|| "http://www.gstatic.com/generate_204".to_owned());
    let message = cargo_toml_value(section, "message")
        .unwrap_or_else(|| t!("errors.time_bomb_expired").to_string());

    Ok(Some(TimeBombConfig {
        expires_at,
        network_time_url,
        message,
    }))
}

fn cargo_toml_section(section_name: &str) -> Option<&'static str> {
    let header = format!("[{section_name}]");
    let start = TIME_BOMB_CONFIG_SOURCE.find(&header)? + header.len();
    let rest = &TIME_BOMB_CONFIG_SOURCE[start..];
    let end = rest.find("\n[").unwrap_or(rest.len());
    Some(&rest[..end])
}

fn cargo_toml_value(section: &str, key: &str) -> Option<String> {
    for line in section.lines() {
        let line = line
            .split_once('#')
            .map(|(left, _)| left)
            .unwrap_or(line)
            .trim();
        let Some((left, right)) = line.split_once('=') else {
            continue;
        };
        if left.trim() != key {
            continue;
        }
        let value = right.trim();
        return Some(
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value)
                .to_owned(),
        );
    }
    None
}

fn time_bomb_expiration_message() -> Result<Option<String>> {
    let Some(config) = time_bomb_config()? else {
        return Ok(None);
    };
    let network_time = fetch_network_time(&config.network_time_url)?;
    if network_time >= config.expires_at.with_timezone(&Utc) {
        Ok(Some(config.message))
    } else {
        Ok(None)
    }
}

fn fetch_network_time(url: &str) -> Result<DateTime<Utc>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .build()?;
    let response = client.get(url).send()?;
    let date_header = response
        .headers()
        .get(DATE)
        .ok_or_else(|| anyhow!(t!("errors.network_time_missing")))?
        .to_str()?;
    Ok(DateTime::parse_from_rfc2822(date_header)?.with_timezone(&Utc))
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

fn launcher_update_http_client(
    timeout: Option<Duration>,
    with_mtls_identity: bool,
) -> Result<Client> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .no_proxy()
        .default_headers(launcher_update_browser_headers());
    if with_mtls_identity && !LAUNCHER_UPDATE_MTLS_IDENTITY.is_empty() {
        builder = builder.identity(reqwest::Identity::from_pem(LAUNCHER_UPDATE_MTLS_IDENTITY)?);
    }
    builder = match timeout {
        Some(timeout) => builder.timeout(timeout),
        None => builder.timeout(None),
    };
    Ok(builder.build()?)
}

fn fetch_launcher_update_manifest(client: &Client) -> Result<LauncherUpdateManifest> {
    fetch_launcher_update_manifest_from_urls(
        client,
        &[LAUNCHER_UPDATE_URL, LAUNCHER_UPDATE_FALLBACK_URL],
    )
}

fn fetch_launcher_update_manifest_from_urls(
    client: &Client,
    urls: &[&str],
) -> Result<LauncherUpdateManifest> {
    let mut failures = Vec::new();

    for (index, url) in urls.iter().enumerate() {
        if urls[..index].iter().any(|previous| previous == url) {
            continue;
        }

        match fetch_launcher_update_manifest_from_url(client, url) {
            Ok(manifest) => {
                if index > 0 {
                    info!("Using fallback launcher update manifest: {url}");
                }
                return Ok(manifest);
            }
            Err(error) => {
                warn!("Unable to fetch launcher update manifest from {url}: {error:#}");
                failures.push(format!("{url}: {error:#}"));
            }
        }
    }

    bail!(
        "Unable to fetch launcher update manifest from all configured URLs: {}",
        failures.join("; ")
    )
}

fn fetch_launcher_update_manifest_from_url(
    client: &Client,
    url: &str,
) -> Result<LauncherUpdateManifest> {
    let manifest_text = client
        .get(url)
        .send()
        .with_context(|| format!("request launcher update manifest from {url}"))?
        .error_for_status()
        .with_context(|| format!("validate launcher update manifest response from {url}"))?
        .text()
        .with_context(|| format!("read launcher update manifest from {url}"))?;
    serde_json::from_str(&manifest_text)
        .with_context(|| format!("parse launcher update manifest from {url}"))
}

fn launcher_version_is_mini(version: &str) -> bool {
    version.strip_prefix('v').unwrap_or(version) == MINI_LAUNCHER_VERSION
}

fn check_launcher_update_and_restart(mut status_updater: impl FnMut(SplashUpdate)) -> Result<bool> {
    if std::env::var_os(LAUNCHER_UPDATE_SKIP_ENV).is_some() {
        info!("Skipping launcher update check after restart");
        std::env::remove_var(LAUNCHER_UPDATE_SKIP_ENV);
        return Ok(false);
    }

    let current_version = env!("CARGO_PKG_VERSION");
    let mini_launcher = launcher_version_is_mini(current_version);
    let platform_key = launcher_update_platform_key();
    let manifest_client = match launcher_update_http_client(Some(Duration::from_secs(10)), false) {
        Ok(client) => client,
        Err(err) => {
            warn!("Unable to create launcher update client: {err:#}");
            return Err(anyhow!(t!(
                "launcher_update.check_failed",
                error = format!("{err:#}")
            )));
        }
    };
    let manifest = match fetch_launcher_update_manifest(&manifest_client) {
        Ok(manifest) => manifest,
        Err(err) => {
            return Err(anyhow!(t!(
                "launcher_update.check_failed",
                error = format!("{err:#}")
            )));
        }
    };
    let update_available = launcher_version_is_newer(current_version, &manifest.version)
        .ok_or_else(|| {
            warn!(
                "Launcher update manifest contains an invalid version: {}",
                manifest.version
            );
            anyhow!(t!(
                "launcher_update.invalid_manifest_version",
                version = manifest.version.clone()
            ))
        })?;
    if !update_available {
        info!(
            "Launcher is up to date: current={}, latest={}",
            current_version, manifest.version
        );
        if mini_launcher {
            return Err(anyhow!(t!(
                "launcher_update.mini_update_missing",
                current = current_version,
                latest = manifest.version
            )));
        }
        return Ok(false);
    }

    let Some(platform) = manifest.platforms.get(platform_key) else {
        warn!("No launcher update payload for platform {platform_key}");
        return Err(anyhow!(t!(
            "launcher_update.payload_missing",
            platform = platform_key
        )));
    };

    info!(
        "Launcher update available: {} -> {}",
        current_version, manifest.version
    );
    status_updater(
        SplashUpdate::loading(
            t!("launcher_update.updating"),
            t!(
                "launcher_update.available_detail",
                version = manifest.version.clone()
            ),
            8,
        )
        .with_subtitle(t!("launcher_update.status")),
    );

    let current_exe = std::env::current_exe()?;
    let update_path = launcher_update_temp_path(&current_exe);
    if let Err(err) = download_launcher_update(
        &platform.url,
        &update_path,
        &platform.sha256,
        &mut status_updater,
    ) {
        warn!("Launcher update download failed: {err:#}");
        return Err(err);
    }
    make_executable(&update_path)?;
    status_updater(
        SplashUpdate::loading(
            t!("launcher_update.restart_title"),
            t!("launcher_update.restarting_detail"),
            100,
        )
        .with_subtitle(t!("launcher_update.restart_status")),
    );
    if let Err(err) = replace_launcher_and_restart(&current_exe, &update_path) {
        warn!("Launcher update replacement failed: {err:#}");
        return Err(err);
    }
    Ok(true)
}

fn download_launcher_update(
    url: &str,
    update_path: &Path,
    expected_sha256: &str,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<()> {
    validate_launcher_update_payload(url, expected_sha256)?;

    // The public manifest supplies the payload URL; ESA requires mTLS for the payload itself.
    let client = launcher_update_http_client(None, true)?;
    let part_path = launcher_update_part_path(update_path);
    remove_launcher_update_file_if_exists(&part_path)?;
    remove_launcher_update_file_if_exists(update_path)?;

    info!("Downloading launcher update from {url}");
    let range_total = launcher_update_range_total(&client, url)?;
    let download_result = match range_total {
        Some(total_bytes) => {
            let ranges = launcher_update_byte_ranges(total_bytes);
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
                )
            } else {
                info!("Launcher update payload is too small for parallel download");
                download_launcher_update_sequential(
                    &client,
                    url,
                    &part_path,
                    Some(total_bytes),
                    &mut status_updater,
                )
            }
        }
        None => {
            info!("Launcher update server does not support HTTP byte ranges; using one connection");
            download_launcher_update_sequential(&client, url, &part_path, None, &mut status_updater)
        }
    };
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

    let downloaded =
        match verify_and_promote_launcher_update(&part_path, update_path, expected_sha256) {
            Ok(downloaded) => downloaded,
            Err(err) => {
                cleanup_launcher_update_download_files(&part_path, update_path);
                return Err(err);
            }
        };

    info!(
        "Launcher update downloaded: {} bytes -> {}",
        downloaded,
        update_path.display()
    );
    Ok(())
}

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

fn launcher_update_sha256_is_valid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn launcher_update_range_total(client: &Client, url: &str) -> Result<Option<u64>> {
    let response = client
        .get(url)
        .header(RANGE, "bytes=0-0")
        .send()?
        .error_for_status()?;
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
    if start != 0 || end != 0 || total == 0 {
        return Ok(None);
    }
    Ok(Some(total))
}

fn parse_launcher_update_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.trim().strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    let total = total.parse().ok()?;
    (start <= end && end < total).then_some((start, end, total))
}

fn launcher_update_byte_ranges(total_bytes: u64) -> Vec<LauncherUpdateByteRange> {
    if total_bytes == 0 {
        return Vec::new();
    }

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

fn download_launcher_update_parallel(
    client: &Client,
    url: &str,
    total_bytes: u64,
    ranges: &[LauncherUpdateByteRange],
    part_path: &Path,
    status_updater: &mut impl FnMut(SplashUpdate),
) -> Result<u64> {
    let temp_dir = TempDirBuilder::new()
        .prefix("azurpilot-launcher-update-")
        .tempdir()
        .context("create temporary launcher update download directory")?;
    let (progress_sender, progress_receiver) = mpsc::channel();
    let mut workers = Vec::with_capacity(ranges.len());
    let mut chunk_paths = Vec::with_capacity(ranges.len());

    for (index, range) in ranges.iter().copied().enumerate() {
        let chunk_path = temp_dir.path().join(format!("chunk-{index:02}"));
        let worker_client = client.clone();
        let worker_url = url.to_owned();
        let worker_path = chunk_path.clone();
        let worker_sender = progress_sender.clone();
        workers.push(thread::spawn(move || {
            download_launcher_update_range(
                &worker_client,
                &worker_url,
                range,
                &worker_path,
                &worker_sender,
            )
        }));
        chunk_paths.push(chunk_path);
    }
    drop(progress_sender);

    let started_at = Instant::now();
    let mut downloaded_so_far = 0u64;
    let mut last_reported_progress = LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START;
    let mut last_reported_at = Instant::now() - Duration::from_secs(1);
    while workers.iter().any(|worker| !worker.is_finished()) {
        match progress_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(downloaded) => {
                downloaded_so_far = downloaded_so_far.saturating_add(downloaded);
                report_launcher_update_download_progress(
                    status_updater,
                    downloaded_so_far,
                    total_bytes,
                    started_at,
                    &mut last_reported_progress,
                    &mut last_reported_at,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    while let Ok(downloaded) = progress_receiver.try_recv() {
        downloaded_so_far = downloaded_so_far.saturating_add(downloaded);
    }

    let mut completed_bytes = 0u64;
    for worker in workers {
        completed_bytes = completed_bytes.saturating_add(
            worker
                .join()
                .map_err(|_| anyhow!("launcher update download worker panicked"))??,
        );
    }
    if completed_bytes != total_bytes {
        bail!(
            "launcher update download incomplete: expected {} bytes, got {} bytes",
            total_bytes,
            completed_bytes
        );
    }

    report_launcher_update_download_progress(
        status_updater,
        total_bytes,
        total_bytes,
        started_at,
        &mut last_reported_progress,
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

fn download_launcher_update_range(
    client: &Client,
    url: &str,
    range: LauncherUpdateByteRange,
    chunk_path: &Path,
    progress_sender: &mpsc::Sender<u64>,
) -> Result<u64> {
    let requested_range = format!("bytes={}-{}", range.start, range.end);
    let mut response = client
        .get(url)
        .header(RANGE, requested_range)
        .send()?
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

fn report_launcher_update_download_progress(
    status_updater: &mut impl FnMut(SplashUpdate),
    downloaded: u64,
    total_bytes: u64,
    started_at: Instant,
    last_reported_progress: &mut u8,
    last_reported_at: &mut Instant,
) {
    let (progress, detail) =
        launcher_download_progress_detail(downloaded, Some(total_bytes), started_at);
    if progress > *last_reported_progress
        || last_reported_at.elapsed() >= Duration::from_millis(250)
    {
        *last_reported_progress = progress;
        *last_reported_at = Instant::now();
        status_updater(
            SplashUpdate::loading(t!("launcher_update.updating"), detail, progress)
                .with_subtitle(t!("launcher_update.status")),
        );
    }
}

fn verify_and_promote_launcher_update(
    part_path: &Path,
    update_path: &Path,
    expected_sha256: &str,
) -> Result<u64> {
    let digest_hex = sha256_file(part_path)?;
    if !digest_hex.eq_ignore_ascii_case(expected_sha256) {
        let _ = fs::remove_file(part_path);
        bail!(
            "launcher update sha256 mismatch: expected {}, got {}",
            expected_sha256,
            digest_hex
        );
    }

    let downloaded = fs::metadata(part_path)?.len();
    remove_launcher_update_file_if_exists(update_path)?;
    fs::rename(part_path, update_path).with_context(|| {
        format!(
            "promote verified launcher update from {} to {}",
            part_path.display(),
            update_path.display()
        )
    })?;
    Ok(downloaded)
}

fn launcher_update_part_path(update_path: &Path) -> PathBuf {
    let Some(file_name) = update_path.file_name() else {
        return update_path.with_extension("part");
    };
    let mut part_name = file_name.to_os_string();
    part_name.push(".part");
    update_path.with_file_name(part_name)
}

fn remove_launcher_update_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn cleanup_launcher_update_download_files(part_path: &Path, update_path: &Path) {
    for path in [part_path, update_path] {
        if let Err(error) = remove_launcher_update_file_if_exists(path) {
            warn!(
                "Unable to clean launcher update download {}: {error}",
                path.display()
            );
        }
    }
}

fn download_launcher_update_sequential(
    client: &Client,
    url: &str,
    update_path: &Path,
    expected_total_bytes: Option<u64>,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<u64> {
    let mut response = client.get(url).send()?.error_for_status()?;
    let total_bytes = expected_total_bytes.or_else(|| response.content_length());
    let mut file = fs::File::create(update_path).with_context(|| {
        t!(
            "errors.write_update_failed",
            error = update_path.display().to_string()
        )
    })?;
    let mut downloaded = 0u64;
    let mut buffer = [0u8; 128 * 1024];
    let mut last_reported_progress = LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START;
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

        let (progress, detail) =
            launcher_download_progress_detail(downloaded, total_bytes, download_started_at);
        if progress > last_reported_progress
            || last_reported_at.elapsed() >= Duration::from_millis(250)
        {
            last_reported_progress = progress;
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

    if let Some(total_bytes) = total_bytes {
        if downloaded != total_bytes {
            return Err(anyhow!(
                "launcher update download incomplete: expected {} bytes, got {} bytes",
                total_bytes,
                downloaded
            ));
        }
    }

    Ok(downloaded)
}

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

fn launcher_download_progress_detail(
    downloaded: u64,
    total_bytes: Option<u64>,
    started_at: Instant,
) -> (u8, String) {
    let speed = format_speed(download_speed_bytes_per_second(downloaded, started_at));
    if let Some(total) = total_bytes.filter(|total| *total > 0) {
        let percentage = (downloaded.min(total).saturating_mul(100) / total) as u8;
        let detail = t!(
            "launcher_update.downloading_detail",
            downloaded = format_bytes(downloaded),
            total = format_bytes(total),
            percent = percentage.to_string(),
            speed = speed
        )
        .to_string();
        let progress_span =
            LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_END - LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START;
        let progress = LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START
            + ((u16::from(percentage) * u16::from(progress_span)) / 100) as u8;
        return (progress, detail);
    }

    let mib_downloaded = downloaded / (1024 * 1024);
    let progress = (LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START
        + mib_downloaded.min(u64::from(
            LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_END - LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START,
        )) as u8)
        .min(LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_END);
    let detail = t!(
        "launcher_update.downloading_detail_unknown",
        downloaded = format_bytes(downloaded),
        speed = speed
    )
    .to_string();
    (progress, detail)
}

fn format_bytes(bytes: u64) -> String {
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

fn download_speed_bytes_per_second(downloaded: u64, started_at: Instant) -> f64 {
    let elapsed = started_at.elapsed().as_secs_f64().max(0.1);
    downloaded as f64 / elapsed
}

fn format_speed(bytes_per_second: f64) -> String {
    format_bytes(bytes_per_second.max(0.0).round() as u64)
}

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

fn launcher_version_is_newer(current: &str, latest: &str) -> Option<bool> {
    let current = parse_launcher_version(current)?;
    let latest = parse_launcher_version(latest)?;
    Some(latest > current)
}

fn parse_launcher_version(version: &str) -> Option<(u64, u64, u64, u64)> {
    let version = version.strip_prefix('v').unwrap_or(version);
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
    let suffix_rank = suffix
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>()
        .parse::<u64>()
        .unwrap_or(0);
    Some((major, minor, patch, suffix_rank))
}

fn valid_launcher_version_suffix(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

fn launcher_arg_present(flags: &[&str]) -> bool {
    std::env::args().skip(1).any(|arg| {
        let arg = arg.to_ascii_lowercase();
        flags.iter().any(|flag| arg == *flag)
    })
}

fn preview_no_update_arg_present() -> bool {
    launcher_arg_present(PREVIEW_NO_UPDATE_ARGS)
}

fn preview_crash_arg_present() -> bool {
    launcher_arg_present(PREVIEW_CRASH_ARGS)
}

fn start_minimized_arg_present() -> bool {
    launcher_arg_present(START_MINIMIZED_ARGS)
}

fn launcher_update_temp_path(current_exe: &Path) -> PathBuf {
    let file_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("alas-launcher");
    std::env::temp_dir().join(format!(
        "azurpilot-launcher-update-{}-{file_name}",
        std::process::id()
    ))
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

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

#[cfg(not(windows))]
fn replace_launcher_and_restart(current_exe: &Path, update_path: &Path) -> Result<()> {
    fs::rename(update_path, current_exe).with_context(|| {
        t!(
            "errors.replace_launcher_failed",
            error = current_exe.display().to_string()
        )
    })?;
    Command::new(current_exe)
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
    Command::new(&helper_path)
        .arg(LAUNCHER_UPDATE_APPLY_ARG)
        .arg(current_exe)
        .arg(update_path)
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

#[cfg(windows)]
fn try_apply_launcher_update_from_args() -> Result<bool> {
    use std::ffi::OsStr;

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

#[cfg(windows)]
fn apply_launcher_update_and_restart(target_path: PathBuf, update_path: PathBuf) -> Result<()> {
    let mut last_error = None;
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

#[cfg(windows)]
fn restart_launcher_after_update(target_path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use winapi::um::winbase::CREATE_NO_WINDOW;

    Command::new(target_path)
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

#[cfg(windows)]
fn schedule_file_delete_on_reboot(path: &Path) {
    use std::ptr;
    use winapi::um::winbase::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

    let path_wide = path_to_wide(path);
    let _ = unsafe { MoveFileExW(path_wide.as_ptr(), ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
}

#[cfg(windows)]
fn path_to_wide(path: &Path) -> Vec<u16> {
    use std::{iter, os::windows::ffi::OsStrExt};

    path.as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_bomb_config_parses_when_enabled() {
        let section =
            cargo_toml_section("package.metadata.alas-launcher.time-bomb").expect("section exists");
        let enabled = cargo_toml_value(section, "enabled").as_deref() == Some("true");
        let config = time_bomb_config().expect("time bomb config parses");
        assert_eq!(config.is_some(), enabled);
    }

    #[test]
    fn test_cargo_toml_value_reads_time_bomb_fields() {
        let section =
            cargo_toml_section("package.metadata.alas-launcher.time-bomb").expect("section exists");
        assert!(cargo_toml_value(section, "expires-at").is_some());
        assert_eq!(
            Some("测试已结束，请安装正式版".to_owned()),
            cargo_toml_value(section, "message")
        );
    }

    #[test]
    fn test_english_splash_i18n_uses_json_literals() {
        rust_i18n::set_locale("en");

        let html = splash_redesigned_shell_html("video", "font");

        assert!(html.contains(r#""defaultTip":"Sakura Empire's cherry blossoms"#));
        assert!(!html.contains("const defaultTip = '"));
        assert!(html.contains("window.__ALAS_SPLASH_READY = true;"));
        assert!(html.contains("data:video/mp4;base64,video"));
        assert!(html.contains("font-family: \"MiSans\""));
        assert!(html.contains("data:font/ttf;base64,font"));
        assert!(!html.contains("text-transform: uppercase;"));
    }

    #[test]
    fn test_splash_includes_optional_uv_progress() {
        let html = splash_redesigned_shell_html("video", "font");

        assert!(html.contains("id=\"uv-progress-container\""));
        assert!(html.contains("payload.uv_progress"));
        assert!(html.contains("id=\"uv-progress-detail\""));
    }

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

    #[test]
    fn test_titlebars_use_webview_draggable_regions_for_touch_dragging() {
        let splash_html = splash_redesigned_shell_html("video", "font");

        assert!(splash_html.contains("touch-action: none;"));
        assert!(splash_html.contains("addEventListener('pointerdown'"));
        assert!(splash_html.contains("-webkit-app-region: drag;"));
        assert!(splash_html.contains("-webkit-app-region: no-drag;"));
        assert!(splash_html.contains("webviewDraggableRegionsEnabled"));
        assert!(splash_html.contains("if (webviewDraggableRegionsEnabled) {"));
        assert!(!splash_html.contains("$NATIVE_TOUCH_DRAG"));

        #[cfg(windows)]
        assert!(splash_html.contains("const webviewDraggableRegionsEnabled = true;"));

        #[cfg(not(target_os = "macos"))]
        let titlebar_script = main_window_titlebar_injection_script();

        #[cfg(not(target_os = "macos"))]
        {
            assert!(titlebar_script.contains("touch-action:none"));
            assert!(titlebar_script.contains("addEventListener('pointerdown'"));
            assert!(titlebar_script.contains("-webkit-app-region:drag"));
            assert!(titlebar_script.contains("-webkit-app-region:no-drag"));
            assert!(titlebar_script.contains("webviewDraggableRegionsEnabled"));
            assert!(titlebar_script.contains("if (webviewDraggableRegionsEnabled)"));
            assert!(titlebar_script.contains("min-height:12px"));
            assert!(titlebar_script.contains("alas-close-menu"));
            assert!(!titlebar_script.contains("alas-close-optics"));
            assert!(!titlebar_script.contains("alas-island-open"));
            assert!(titlebar_script.contains("__ALAS_OPEN_CLOSE_PROMPT"));
            assert!(titlebar_script.contains("window_exit_application"));
            #[cfg(windows)]
            assert!(titlebar_script.contains("const webviewDraggableRegionsEnabled = true;"));
        }
    }

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
        let manifest = fetch_launcher_update_manifest_from_urls(&client, &[&primary, &fallback])
            .expect("fetch manifest from fallback");

        assert_eq!(manifest.version, "2.1.8");
        server.join().expect("manifest server completed");
    }

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
            launcher_update_range_total(&client, &url).expect("probe range support"),
            None
        );
        server.join().expect("range probe server completed");
    }

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
        let total_bytes = launcher_update_range_total(&client, &url)
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
        )
        .expect("parallel launcher download");

        assert_eq!(downloaded, payload.len() as u64);
        let merged = fs::read(&part_path).expect("read merged file");
        assert_eq!(merged.as_slice(), payload.as_slice());
        assert!(progress
            .iter()
            .any(|value| *value > LAUNCHER_UPDATE_DOWNLOAD_PROGRESS_START));
        server.join().expect("range server completed");
    }

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

/// Set macOS activation policy to Regular (show in dock) or Accessory (hide from dock).
#[cfg(target_os = "macos")]
fn set_macos_activation_policy(app: &tauri::AppHandle, regular: bool) {
    let policy = if regular {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    if let Err(e) = app.set_activation_policy(policy) {
        error!("Failed to set activation policy: {}", e);
    }
}

fn main() -> Result<()> {
    #[cfg(windows)]
    if try_apply_launcher_update_from_args()? {
        return Ok(());
    }

    #[cfg(windows)]
    unsafe {
        use crate::window_util::HAS_CONSOLE;
        use std::sync::atomic::Ordering;
        use winapi::um::wincon::{AttachConsole, ATTACH_PARENT_PROCESS};
        if std::env::var_os(LAUNCHER_UPDATE_NO_CONSOLE_ENV).is_some() {
            std::env::remove_var(LAUNCHER_UPDATE_NO_CONSOLE_ENV);
        } else {
            HAS_CONSOLE.store(AttachConsole(ATTACH_PARENT_PROCESS) != 0, Ordering::Relaxed);
        }
    }
    setup_environment()?;
    let _log_guard = initialize_logging()?;
    crate::i18n::init();
    let preview_crash = preview_crash_arg_present();
    let preview_no_update = preview_crash || preview_no_update_arg_present();
    let start_minimized = start_minimized_arg_present();

    info!("=== AzurPilot starting ===");
    info!("Launcher log file: log/{}", today_launcher_log_filename());
    if preview_no_update {
        info!("Preview no-update mode enabled; skipping launcher update check");
    }
    if preview_crash {
        info!("Preview crash mode enabled; splash will stop on an artificial error state");
    }
    if start_minimized {
        info!("Start minimized mode enabled; main window will stay in tray after backend is ready");
    }

    let deploy_config = get_deploy_config();
    let webui_config = WebuiLaunchConfig::from_deploy_config(deploy_config.as_ref());
    if deploy_config.is_none() {
        warn!("config/deploy.yaml not found or invalid, using default WebUI launch config");
    }
    let port = webui_config.port;

    let backend = Arc::new(Mutex::new(None));
    let allow_exit = Arc::new(AtomicBool::new(false));
    let launch_blocked = Arc::new(AtomicBool::new(false));
    let setup_cancel_requested = Arc::new(AtomicBool::new(false));
    let setup_running = Arc::new(AtomicBool::new(false));
    let setup_completed = Arc::new(AtomicBool::new(false));
    let startup_shutdown_started = Arc::new(AtomicBool::new(false));
    let recreating_main_window = Arc::new(AtomicBool::new(false));

    let allow_exit_for_setup = allow_exit.clone();
    let launch_blocked_for_setup = launch_blocked.clone();
    let recreating_main_window_for_single_instance = recreating_main_window.clone();
    let recreating_main_window_for_setup = recreating_main_window.clone();
    let recreating_main_window_for_run = recreating_main_window.clone();
    let launch_blocked_for_run = launch_blocked.clone();
    let start_minimized_for_run = start_minimized;

    info!("Starting Webview...");
    tauri::Builder::default()
        .register_uri_scheme_protocol("alas-error", |_ctx, request| {
            backend_error_response(request)
        })
        .register_uri_scheme_protocol("alas-splash", |_ctx, _request| splash_response())
        .manage(ExitControl(allow_exit.clone()))
        .invoke_handler(tauri::generate_handler![
            save_as,
            download_today_gui_log,
            download_today_launcher_log,
            retry_backend_connection,
            window_hide,
            window_minimize,
            window_toggle_maximize,
            window_close,
            window_exit_application,
            window_start_dragging,
            window_is_maximized
        ])
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(
            move |app, _argv, _cwd| {
                restore_main_window_from_tray(
                    app,
                    port,
                    recreating_main_window_for_single_instance.clone(),
                );
            },
        ))
        .setup(move |app| {
            match time_bomb_expiration_message() {
                Ok(Some(message)) => {
                    launch_blocked_for_setup.store(true, Ordering::SeqCst);
                    allow_exit_for_setup.store(true, Ordering::SeqCst);
                    let app_handle = app.handle().clone();
                    app.dialog()
                        .message(message)
                        .title(t!("dialog.test_ended"))
                        .show(move |_| {
                            app_handle.exit(0);
                        });
                    return Ok(());
                }
                Ok(None) => {}
                Err(err) => {
                    warn!("Unable to verify test expiration time: {:?}", err);
                }
            }

            create_main_window(&app.handle(), port)?;

            // Windows and macOS: create system tray
            #[cfg(any(windows, target_os = "macos"))]
            {
                info!("Creating system tray...");
                let allow_exit = allow_exit_for_setup.clone();
                let recreating_main_window_for_menu = recreating_main_window_for_setup.clone();
                #[cfg(windows)]
                let recreating_main_window_for_tray = recreating_main_window_for_setup.clone();
                let show_item = MenuItemBuilder::new(t!("tray.toggle_visibility"))
                    .id("toggle_visibility")
                    .build(app)?;
                let quit_item = MenuItemBuilder::new(t!("tray.quit"))
                    .id("quit")
                    .build(app)?;
                let tray_menu = MenuBuilder::new(app)
                    .item(&show_item)
                    .separator()
                    .item(&quit_item)
                    .build()?;

                info!("Tray menu created successfully");

                // Use embedded icon bytes so packaged apps always load the tray icon correctly.
                let icon = tray_icon_for_platform();

                info!("Building tray icon...");
                let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                    .icon(icon)
                    .tooltip("AzurPilot")
                    .menu(&tray_menu);

                // On Windows, show menu on right click
                #[cfg(windows)]
                {
                    tray_builder = tray_builder.show_menu_on_left_click(false);
                }

                // On macOS, show menu on left click
                #[cfg(target_os = "macos")]
                {
                    info!("Setting macOS tray to show menu on left click");
                    tray_builder = tray_builder.show_menu_on_left_click(true);
                }

                match tray_builder
                    .on_menu_event(move |app, event| {
                        debug!("Tray menu event: {:?}", event.id());
                        match event.id().as_ref() {
                            "toggle_visibility" => {
                                toggle_main_window_visibility(
                                    app,
                                    port,
                                    recreating_main_window_for_menu.clone(),
                                );
                            }
                            "quit" => {
                                allow_exit.store(true, Ordering::SeqCst);
                                app.exit(0);
                            }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(move |tray, event| {
                        #[cfg(windows)]
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            toggle_main_window_visibility(
                                &app,
                                port,
                                recreating_main_window_for_tray.clone(),
                            );
                        }

                        #[cfg(target_os = "macos")]
                        {
                            let _ = tray;
                            let _ = event;
                        }
                    })
                    .build(app)
                {
                    Ok(_) => {
                        info!("System tray created successfully!");
                    }
                    Err(e) => {
                        error!("Failed to create system tray: {:?}", e);
                        return Err(Box::new(e));
                    }
                }
            }

            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(move |app_handle, event| {
            match event {
                tauri::RunEvent::Ready => {
                    if launch_blocked_for_run.load(Ordering::SeqCst) {
                        debug!("Launch blocked by test expiration");
                        return;
                    }

                    debug!("RunEvent::Ready");
                    let allow_exit = allow_exit.clone();
                    let allow_exit_for_ctrlc = allow_exit.clone();
                    let handle1 = app_handle.clone();
                    ctrlc::set_handler(move || {
                        allow_exit_for_ctrlc.store(true, Ordering::SeqCst);
                        handle1.exit(0);
                    })
                    .expect("Error setting Ctrl-C handler");
                    let app_handle = app_handle.clone();
                    let backend = backend.clone();
                    let webui_config = webui_config.clone();
                    let setup_cancel_requested = setup_cancel_requested.clone();
                    let setup_running = setup_running.clone();
                    let setup_completed = setup_completed.clone();
                    let recreating_main_window_for_notify = recreating_main_window_for_run.clone();
                    let start_minimized = start_minimized_for_run;
                    thread::spawn(move || {
                        setup_running.store(true, Ordering::SeqCst);
                        let splash = app_handle.get_webview_window("splash").unwrap();
                        initialize_splash(&splash, !start_minimized);
                        let last_progress = Cell::new(0u8);
                        let mut status_updater = |mut update: SplashUpdate| {
                            update.progress = update.progress.max(last_progress.get());
                            last_progress.set(update.progress);
                            update_splash(&splash, &update);
                        };

                        status_updater(
                            SplashUpdate::loading(
                                t!("splash.starting"),
                                t!("splash.webui_init"),
                                4,
                            )
                            .with_subtitle(format!(
                                "{} | Tips:{}",
                                t!("splash.initializing"),
                                crate::setup::get_tip()
                            )),
                        );

                        if !preview_no_update {
                            let launcher_progress = Cell::new(0u8);
                            let mut launcher_status_updater = |mut update: SplashUpdate| {
                                update.progress = update.progress.max(launcher_progress.get());
                                launcher_progress.set(update.progress);
                                update_splash(&splash, &update);
                            };

                            match check_launcher_update_and_restart(&mut launcher_status_updater) {
                                Ok(true) => {
                                    info!("Launcher update installed, restarting");
                                    setup_completed.store(true, Ordering::SeqCst);
                                    setup_running.store(false, Ordering::SeqCst);
                                    allow_exit.store(true, Ordering::SeqCst);
                                    app_handle.exit(0);
                                    return;
                                }
                                Ok(false) => {}
                                Err(e) => {
                                    warn!("Required launcher update failed: {e:#}");
                                    if start_minimized {
                                        let _ = reveal_window(&splash);
                                    }
                                    launcher_status_updater(SplashUpdate::error(
                                        t!("launcher_update.failed"),
                                        t!(
                                            "launcher_update.failed_detail",
                                            error = format!("{e:#}")
                                        ),
                                        launcher_progress.get().max(8),
                                    ));
                                    setup_completed.store(true, Ordering::SeqCst);
                                    setup_running.store(false, Ordering::SeqCst);
                                    return;
                                }
                            }
                        }

                        if preview_crash {
                            if start_minimized {
                                let _ = reveal_window(&splash);
                            }
                            status_updater(
                                SplashUpdate::error(
                                    t!("dialog.startup_failed"),
                                    t!("splash.preview_crash_detail"),
                                    42,
                                )
                                .with_subtitle(format!(
                                    "{} | Tips：{}",
                                    t!("splash.preview_crash_mode"),
                                    crate::setup::get_tip()
                                )),
                            );
                            setup_completed.store(true, Ordering::SeqCst);
                            setup_running.store(false, Ordering::SeqCst);
                            return;
                        }
                        if let Err(e) = setup_alas_repo(
                            &mut status_updater,
                            setup_cancel_requested.clone(),
                            preview_no_update,
                        ) {
                            error!("{e}");
                            setup_running.store(false, Ordering::SeqCst);
                            if setup_cancel_requested.load(Ordering::SeqCst) {
                                return;
                            }
                            if start_minimized {
                                let _ = reveal_window(&splash);
                            }
                            status_updater(SplashUpdate::error(
                                t!("dialog.startup_failed"),
                                t!("dialog.repo_setup_failed", error = e.to_string()),
                                last_progress.get().max(8),
                            ));
                            return;
                        }
                        info!("Starting gui.py on http://127.0.0.1:{}/", port);
                        status_updater(
                            SplashUpdate::loading(
                                t!("splash.starting"),
                                t!("splash.webui_init_slow"),
                                97,
                            )
                            .with_subtitle(format!(
                                "{} | Tips:{}",
                                t!("splash.starting_backend"),
                                crate::setup::get_tip()
                            )),
                        );
                        let mut backend_recovery_used = false;
                        let backend_result = loop {
                            match ManagedBackend::new(&webui_config) {
                                Ok(backend) => break Ok(backend),
                                Err(error)
                                    if !backend_recovery_used
                                        && is_backend_startup_timeout(&error) =>
                                {
                                    backend_recovery_used = true;
                                    if setup_cancel_requested.load(Ordering::SeqCst) {
                                        break Err(error);
                                    }

                                    warn!(
                                        "Backend startup timed out; rebuilding .venv and retrying once"
                                    );
                                    if let Err(recovery_error) = rebuild_venv_and_sync_dependencies(
                                        &mut status_updater,
                                        setup_cancel_requested.clone(),
                                    ) {
                                        break Err(recovery_error.context(
                                            "Failed to rebuild .venv after backend startup timeout",
                                        ));
                                    }

                                    info!(
                                        "Retrying gui.py after rebuilding dependencies on http://127.0.0.1:{port}/"
                                    );
                                    status_updater(
                                        SplashUpdate::loading(
                                            t!("splash.starting"),
                                            t!("splash.webui_init_slow"),
                                            97,
                                        )
                                        .with_subtitle(format!(
                                            "{} | Tips:{}",
                                            t!("splash.starting_backend"),
                                            crate::setup::get_tip()
                                        )),
                                    );
                                }
                                Err(error) => break Err(error),
                            }
                        };
                        let b = match backend_result {
                            Ok(backend) => backend,
                            Err(e) => {
                                error!("{e}");
                                setup_running.store(false, Ordering::SeqCst);
                                if setup_cancel_requested.load(Ordering::SeqCst) {
                                    return;
                                }
                                if start_minimized {
                                    let _ = reveal_window(&splash);
                                }
                                status_updater(SplashUpdate::error(
                                    t!("dialog.startup_failed"),
                                    t!("dialog.backend_launch_failed", error = e.to_string()),
                                    last_progress.get().max(97),
                                ));
                                return;
                            }
                        };
                        *backend.lock().unwrap() = Some(b);
                        let notification_click: NotificationClickHandler = {
                            let app_handle = app_handle.clone();
                            let recreating_main_window = recreating_main_window_for_notify.clone();
                            Arc::new(move || {
                                restore_main_window_from_any_thread(
                                    app_handle.clone(),
                                    port,
                                    recreating_main_window.clone(),
                                );
                            })
                        };
                        start_notify_stream(
                            app_handle.clone(),
                            port,
                            allow_exit.clone(),
                            notification_click,
                        );
                        start_launcher_control_stream(port, allow_exit.clone());
                        status_updater(
                            SplashUpdate::loading(t!("splash.opening"), t!("splash.ready"), 100)
                                .with_subtitle(format!(
                                    "{} | Tips:{}",
                                    t!("splash.startup_complete"),
                                    crate::setup::get_tip()
                                )),
                        );
                        let _ = splash.destroy();
                        debug!("Destroyed splash window after startup");

                        info!("Webview is ready");
                        let window = app_handle.get_webview_window("main").unwrap();
                        window.set_resizable(true).unwrap();
                        if let Err(e) = navigate_backend_or_error(&window, port) {
                            error!("Failed to navigate main window: {:?}", e);
                        }
                        if start_minimized {
                            info!("Backend is ready; keeping main window hidden in tray");
                            let _ = window.hide();
                        } else {
                            reveal_window(&window).unwrap();
                        }
                        setup_completed.store(true, Ordering::SeqCst);
                        setup_running.store(false, Ordering::SeqCst);
                    });
                }
                tauri::RunEvent::ExitRequested { api, .. } => {
                    if !setup_completed.load(Ordering::SeqCst)
                        && !startup_shutdown_started.load(Ordering::SeqCst)
                    {
                        api.prevent_exit();
                        begin_startup_shutdown(
                            app_handle.clone(),
                            allow_exit.clone(),
                            setup_cancel_requested.clone(),
                            setup_running.clone(),
                            startup_shutdown_started.clone(),
                        );
                        return;
                    }

                    let should_allow = allow_exit.load(Ordering::SeqCst);
                    debug!("ExitRequested event: allow_exit={}", should_allow);

                    // Only exit if explicitly allowed (e.g., via tray menu Quit)
                    if !should_allow {
                        api.prevent_exit();
                        debug!("Minimizing main window to tray");
                        minimize_main_window_to_tray(&app_handle);
                        return;
                    }

                    debug!("allow_exit is TRUE, proceeding with app shutdown");
                    info!("App exit allowed, shutting down backend...");
                    if let Some(ref mut b) = *backend.lock().unwrap() {
                        if let Err(e) = b.terminate() {
                            warn!("Failed to terminate backend process: {:?}", e);
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => {
                    restore_main_window_from_any_thread(
                        app_handle.clone(),
                        port,
                        recreating_main_window_for_run.clone(),
                    );
                }
                tauri::RunEvent::WindowEvent {
                    label,
                    event: tauri::WindowEvent::CloseRequested { ref api, .. },
                    ..
                } => {
                    debug!("Window {} close requested", label);

                    if label == "splash" && !setup_completed.load(Ordering::SeqCst) {
                        api.prevent_close();
                        begin_startup_shutdown(
                            app_handle.clone(),
                            allow_exit.clone(),
                            setup_cancel_requested.clone(),
                            setup_running.clone(),
                            startup_shutdown_started.clone(),
                        );
                        return;
                    }

                    if label == "splash" && !allow_exit.load(Ordering::SeqCst) {
                        api.prevent_close();
                        allow_exit.store(true, Ordering::SeqCst);
                        app_handle.exit(0);
                        return;
                    }

                    // Windows: show the in-window close chooser instead of a native dialog.
                    #[cfg(windows)]
                    {
                        if label == "main" && !allow_exit.load(Ordering::SeqCst) {
                            api.prevent_close();
                            if let Some(main_window) = app_handle.get_webview_window("main") {
                                if let Err(err) = main_window.eval(
                                    "if (typeof window.__ALAS_OPEN_CLOSE_PROMPT !== 'function') { throw new Error('close prompt is unavailable'); } window.__ALAS_OPEN_CLOSE_PROMPT();",
                                ) {
                                    warn!("Unable to open close chooser: {err:?}");
                                    minimize_main_window_to_tray(&app_handle);
                                }
                            } else {
                                minimize_main_window_to_tray(&app_handle);
                            }
                            return;
                        }
                    }

                    // macOS: switch to Accessory policy so the app does not terminate
                    // when no Regular windows are visible.
                    #[cfg(target_os = "macos")]
                    {
                        if label == "main" && !allow_exit.load(Ordering::SeqCst) {
                            api.prevent_close();
                            minimize_main_window_to_tray(&app_handle);
                            return;
                        }
                    }

                    // Linux: just hide to tray
                    #[cfg(target_os = "linux")]
                    {
                        if label == "main" && !allow_exit.load(Ordering::SeqCst) {
                            api.prevent_close();
                            minimize_main_window_to_tray(&app_handle);
                            return;
                        }
                    }
                }

                _ => {}
            };
        });
    Ok(())
}

fn initialize_logging() -> Result<WorkerGuard> {
    let log_dir = Path::new("log");
    let log_filename = today_launcher_log_filename();
    truncate_log_file(log_dir, &log_filename)?;
    let file_appender = tracing_appender::rolling::never(log_dir, log_filename);
    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false)
        .with_target(false)
        .with_filter(tracing::level_filters::LevelFilter::DEBUG);
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(tracing::level_filters::LevelFilter::DEBUG);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .init();

    Ok(guard)
}

fn truncate_log_file(log_dir: &Path, filename: &str) -> Result<()> {
    fs::create_dir_all(log_dir)?;
    let path = log_dir.join(filename);
    fs::File::create(&path)
        .with_context(|| format!("truncate launcher log file {}", path.display()))?;
    Ok(())
}

#[tauri::command]
fn save_as(app_handle: tauri::AppHandle, filename: &str, data: &str) {
    match BASE64_STANDARD.decode(data) {
        Ok(decoded_data) => app_handle
            .dialog()
            .file()
            .set_file_name(filename)
            .save_file(move |path| {
                let result: Result<()> = (move || {
                    let file_path = path
                        .as_ref()
                        .and_then(FilePath::as_path)
                        .ok_or_else(|| anyhow!("Invalid file path {:?}", &path))?;
                    fs::write(file_path, &decoded_data)?;
                    info!("Saved file to {:?}", file_path);
                    Ok(())
                })();
                if let Err(e) = result {
                    error!("Failed to save file: {:?}", e);
                }
            }),
        Err(e) => {
            error!("Failed to decode file content: {:?}", e);
        }
    }
}

#[tauri::command]
fn download_today_gui_log(app_handle: tauri::AppHandle) -> std::result::Result<String, String> {
    download_log_file(app_handle, today_gui_log_filename(), "GUI")
}

#[tauri::command]
fn download_today_launcher_log(
    app_handle: tauri::AppHandle,
) -> std::result::Result<String, String> {
    download_log_file(app_handle, today_launcher_log_filename(), "launcher")
}

fn download_log_file(
    app_handle: tauri::AppHandle,
    filename: String,
    log_name: &str,
) -> std::result::Result<String, String> {
    let log_name = log_name.to_owned();
    let source_path = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("log")
        .join(&filename);
    let data = fs::read(&source_path).map_err(|e| {
        t!(
            "errors.read_log_file",
            path = source_path.to_string_lossy().to_string(),
            error = e.to_string()
        )
    })?;

    app_handle
        .dialog()
        .file()
        .set_file_name(&filename)
        .save_file(move |path| {
            let log_name_for_save = log_name.clone();
            let result: Result<()> = (move || {
                let file_path = path
                    .as_ref()
                    .and_then(FilePath::as_path)
                    .ok_or_else(|| anyhow!("Invalid file path {:?}", &path))?;
                fs::write(file_path, &data)?;
                info!("Saved {} log to {:?}", log_name_for_save, file_path);
                Ok(())
            })();
            if let Err(e) = result {
                error!("Failed to save {} log: {:?}", log_name, e);
            }
        });

    Ok(filename)
}

fn today_gui_log_filename() -> String {
    format!("{}_gui.txt", Local::now().format("%Y-%m-%d"))
}

fn today_launcher_log_filename() -> String {
    format!("{}_launcher.txt", Local::now().format("%Y-%m-%d"))
}

#[tauri::command]
fn window_hide(app_handle: tauri::AppHandle) -> tauri::Result<()> {
    minimize_main_window_to_tray(&app_handle);
    Ok(())
}

#[tauri::command]
fn window_minimize(window: WebviewWindow) -> tauri::Result<()> {
    window.minimize()
}

#[tauri::command]
fn window_toggle_maximize(window: WebviewWindow) -> tauri::Result<bool> {
    if window.is_maximized()? {
        window.unmaximize()?;
        Ok(false)
    } else {
        window.maximize()?;
        Ok(true)
    }
}

#[tauri::command]
fn window_close(window: WebviewWindow) -> tauri::Result<()> {
    window.close()
}

#[tauri::command]
fn window_exit_application(
    app_handle: tauri::AppHandle,
    exit_control: State<'_, ExitControl>,
) -> tauri::Result<()> {
    exit_control.0.store(true, Ordering::SeqCst);
    app_handle.exit(0);
    Ok(())
}

#[tauri::command]
fn window_start_dragging(window: WebviewWindow) -> tauri::Result<()> {
    window.start_dragging()
}

#[tauri::command]
fn window_is_maximized(window: WebviewWindow) -> tauri::Result<bool> {
    window.is_maximized()
}

#[tauri::command]
async fn retry_backend_connection(
    window: WebviewWindow,
    port: u16,
) -> std::result::Result<bool, String> {
    let connected = tauri::async_runtime::spawn_blocking(move || {
        wait_for_backend_connection(port, BACKEND_NAVIGATION_TIMEOUT).is_ok()
    })
    .await
    .map_err(|e| {
        error!("Backend retry task failed: {e:?}");
        e.to_string()
    })?;

    if !connected {
        return Ok(false);
    }

    let url = Url::parse(&backend_url(port)).map_err(|e| e.to_string())?;
    window.navigate(url).map_err(|e| {
        error!("Failed to navigate to reconnected backend: {e:?}");
        e.to_string()
    })?;
    Ok(true)
}

fn page_load_injector(webview: WebviewWindow, payload: PageLoadPayload<'_>) {
    if payload.event() == PageLoadEvent::Finished {
        info!(
            "Injecting saveFile function to loaded page: {}",
            payload.url()
        );
        let injected_js = r#"
if (!window.alas_launcher_injected) {
    window.alas_launcher_injected = true;
    (function () {
        // Prevent going back
        history.pushState(null, document.title, location.href);
        window.addEventListener('popstate', event => {
            history.pushState(null, document.title, location.href);
        });
        // Overwrite original saveAs function
        window.saveAs = function (blob, filename) {
            const reader = new FileReader();
            reader.onload = async () => {
                const data = reader.result.split(',')[1];
                console.log(data);
                const tauriInvoke =
                    (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
                    || (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
                if (typeof tauriInvoke === 'function') {
                    tauriInvoke('save_as', { filename, data });
                }
            };
            reader.readAsDataURL(blob);
        };
__ALAS_TITLEBAR_SCRIPT__
    })();
}
"#
        .replace(
            "__ALAS_TITLEBAR_SCRIPT__",
            &main_window_titlebar_injection_script(),
        );
        if let Err(e) = webview.eval(&injected_js) {
            error!("Failed to inject JS to webview: {:?}", e);
        }
    }
}

fn initialize_splash(splash: &WebviewWindow, show_window: bool) {
    match Url::parse(SPLASH_URL) {
        Ok(url) => {
            if let Err(e) = splash.navigate(url) {
                error!("Failed to navigate splash page: {:?}", e);
            }
            if !wait_for_splash_ready(splash, Duration::from_secs(2)) {
                warn!("Timed out waiting for splash page readiness; showing splash anyway");
            }
            if show_window {
                if let Err(e) = splash.show() {
                    error!("Failed to show splash window: {:?}", e);
                }
            }
        }
        Err(e) => {
            error!("Failed to parse splash URL: {:?}", e);
        }
    }
}

fn wait_for_splash_ready(splash: &WebviewWindow, timeout: Duration) -> bool {
    let started_at = Instant::now();
    while started_at.elapsed() < timeout {
        if splash
            .eval(
                r#"
                if (!window.__ALAS_SPLASH_READY) {
                    throw new Error("splash page is not ready");
                }
                "#,
            )
            .is_ok()
        {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    false
}

fn update_splash(splash: &WebviewWindow, update: &SplashUpdate) {
    let payload = to_string(update).unwrap();
    let script = format!("window.__ALAS_SPLASH_UPDATE && window.__ALAS_SPLASH_UPDATE({payload});");
    if let Err(e) = splash.eval(&script) {
        error!("Failed to update splash page: {:?}", e);
    }
}

fn backend_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

fn splash_response() -> tauri::http::Response<Vec<u8>> {
    let video_bg_b64 = BASE64_STANDARD.encode(SPLASH_BG_VIDEO);
    let mi_sans_font_b64 = BASE64_STANDARD.encode(MI_SANS_FONT);
    tauri::http::Response::builder()
        .header(
            tauri::http::header::CONTENT_TYPE,
            "text/html; charset=utf-8",
        )
        .body(splash_redesigned_shell_html(&video_bg_b64, &mi_sans_font_b64).into_bytes())
        .unwrap()
}

fn check_backend_connection(port: u16) -> Result<()> {
    let address: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    TcpStream::connect_timeout(&address, BACKEND_CONNECT_TIMEOUT)
        .map(|_| ())
        .map_err(|e| anyhow!("Unable to connect to local backend at {address}: {e}"))
}

fn wait_for_backend_connection(port: u16, timeout: Duration) -> Result<()> {
    let started_at = Instant::now();
    let mut last_error = None;
    while started_at.elapsed() < timeout {
        match check_backend_connection(port) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_error = Some(e);
                thread::sleep(Duration::from_millis(200));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!(t!("errors.backend_timeout"))))
}

fn navigate_backend_or_error(window: &WebviewWindow, port: u16) -> Result<bool> {
    match wait_for_backend_connection(port, BACKEND_NAVIGATION_TIMEOUT) {
        Ok(()) => {
            let url = backend_url(port);
            window.navigate(Url::parse(&url)?)?;
            Ok(true)
        }
        Err(e) => {
            warn!("Backend connection check failed before navigation: {:?}", e);
            navigate_to_backend_error(window, port, &e.to_string())?;
            Ok(false)
        }
    }
}

fn navigate_to_backend_error(window: &WebviewWindow, port: u16, error_detail: &str) -> Result<()> {
    let url = backend_error_url(port, error_detail)?;
    window.navigate(url)?;
    Ok(())
}

fn backend_error_url(port: u16, error_detail: &str) -> Result<Url> {
    let port = port.to_string();
    Ok(Url::parse_with_params(
        BACKEND_ERROR_URL_BASE,
        [("port", port.as_str()), ("detail", error_detail)],
    )?)
}

fn backend_error_response(
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let (port, detail) = backend_error_request_params(request.uri().to_string().as_str());
    let html = backend_error_html(port, &detail);

    tauri::http::Response::builder()
        .header(
            tauri::http::header::CONTENT_TYPE,
            "text/html; charset=utf-8",
        )
        .body(html.into_bytes())
        .unwrap()
}

fn backend_error_request_params(uri: &str) -> (u16, String) {
    let mut port = 22267;
    let mut detail = t!("error_page.unable_connect").to_string();

    if let Ok(url) = Url::parse(uri) {
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "port" => {
                    if let Ok(parsed_port) = value.parse::<u16>() {
                        port = parsed_port;
                    }
                }
                "detail" => detail = value.into_owned(),
                _ => {}
            }
        }
    }

    (port, detail)
}

fn handle_backend_navigation(app: tauri::AppHandle, port: u16, url: &Url) -> bool {
    if !is_backend_url(url, port) {
        return true;
    }

    match check_backend_connection(port) {
        Ok(()) => true,
        Err(e) => {
            let blocked_url = url.to_string();
            warn!(
                "Blocked navigation to unreachable backend {}: {:?}",
                blocked_url, e
            );
            let error_detail = e.to_string();
            thread::spawn(move || {
                if let Some(window) = app.get_webview_window("main") {
                    if let Err(e) = navigate_to_backend_error(&window, port, &error_detail) {
                        error!("Failed to show backend error page: {:?}", e);
                    }
                }
            });
            false
        }
    }
}

fn is_backend_url(url: &Url, port: u16) -> bool {
    matches!(url.scheme(), "http" | "https")
        && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
        && url.port_or_known_default() == Some(port)
}

fn escape_html(input: impl AsRef<str>) -> String {
    input
        .as_ref()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn backend_error_html(port: u16, error_detail: &str) -> String {
    let backend_url_json = to_string(&backend_url(port)).unwrap();
    let error_detail_json = to_string(error_detail).unwrap();
    let mi_sans_font_b64 = BASE64_STANDARD.encode(MI_SANS_FONT);
    let splash_video_b64 = BASE64_STANDARD.encode(SPLASH_BG_VIDEO);
    let titlebar_script = main_window_titlebar_injection_script();
    let i18n = serde_json::json!({
        "title": t!("error_page.title"),
        "heading": t!("error_page.heading"),
        "description": t!("error_page.description"),
        "address": t!("error_page.address"),
        "errorLabel": t!("error_page.error_label"),
        "retry": t!("error_page.retry"),
        "downloadGuiLog": t!("error_page.download_gui_log"),
        "downloadLauncherLog": t!("error_page.download_launcher_log"),
        "reconnecting": t!("error_page.reconnecting"),
        "stillFailed": t!("error_page.still_failed"),
        "retryFailed": t!("error_page.retry_failed"),
        "preparing": t!("error_page.preparing"),
        "saved": t!("error_page.saved"),
        "downloadFailed": t!("error_page.download_failed"),
    });
    let i18n_json = to_string(&i18n).unwrap();

    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  @font-face {{
    font-family: "MiSans";
    src: url(data:font/ttf;base64,{mi_sans_font_b64}) format("truetype");
    font-weight: 100 900;
    font-style: normal;
    font-display: swap;
  }}
  :root {{
    color-scheme: light;
    --bg: #f4f6f8;
    --surface: #ffffff;
    --surface-soft: #f8fafb;
    --line: #e5e9ee;
    --text: #17212b;
    --muted: #687582;
    --accent: #176b67;
    --accent-hover: #105854;
    --accent-soft: #e8f4f2;
    --danger: #b64545;
    --danger-soft: #fff1f0;
  }}
  * {{
    box-sizing: border-box;
  }}
  html, body {{
    width: 100%;
    min-height: 100%;
    margin: 0;
    font-family: "MiSans", sans-serif;
    font-weight: 420;
    font-synthesis: none;
    color: var(--text);
    background: #dfe7ea;
  }}
  body {{
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 72px 44px 44px;
    position: relative;
    isolation: isolate;
    overflow: hidden;
    background: transparent;
    animation: page-in 420ms cubic-bezier(0.22, 1, 0.36, 1) both;
  }}
  .error-background-video {{
    position: fixed;
    inset: 0;
    z-index: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0.9;
    pointer-events: none;
  }}
  .error-background-scrim {{
    position: fixed;
    inset: 0;
    z-index: 1;
    background: rgba(244, 247, 248, 0.36);
    pointer-events: none;
  }}
  .panel {{
    position: relative;
    z-index: 2;
    display: grid;
    grid-template-columns: 190px minmax(0, 1fr);
    width: min(820px, 100%);
    min-height: 390px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.76);
    backdrop-filter: blur(22px) saturate(1.08);
    box-shadow: 0 20px 48px rgba(23, 33, 43, 0.11), 0 2px 6px rgba(23, 33, 43, 0.04);
    animation: panel-in 520ms cubic-bezier(0.22, 1, 0.36, 1) 70ms both;
  }}
  .signal {{
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent);
    color: #fff;
  }}
  .signal::before, .signal::after {{
    content: "";
    position: absolute;
    border: 1px solid rgba(255, 255, 255, 0.17);
    border-radius: 50%;
    opacity: 0;
    animation: signal-expand 3.2s ease-out infinite;
  }}
  .signal::before {{ width: 76px; height: 76px; }}
  .signal::after {{ width: 76px; height: 76px; animation-delay: 1.6s; }}
  .signal-core {{
    position: relative;
    z-index: 1;
    width: 76px;
    height: 76px;
    display: grid;
    place-items: center;
    border: 1px solid rgba(255, 255, 255, 0.45);
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.12);
    animation: core-breathe 2.8s ease-in-out infinite;
  }}
  .signal-core svg {{
    width: 36px;
    height: 36px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 1.7;
  }}
  .content {{
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 38px 42px 32px;
    animation: content-in 500ms cubic-bezier(0.22, 1, 0.36, 1) 140ms both;
  }}
  .eyebrow {{
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 620;
    letter-spacing: 1.2px;
    text-transform: uppercase;
  }}
  .eyebrow::before {{
    content: "";
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--danger);
    box-shadow: 0 0 0 4px var(--danger-soft);
    animation: status-pulse 2s ease-in-out infinite;
  }}
  h1 {{
    max-width: 470px;
    margin: 16px 0 0;
    font-size: 30px;
    font-weight: 680;
    letter-spacing: -0.3px;
    line-height: 1.18;
  }}
  .lead {{
    max-width: 510px;
    margin: 12px 0 0;
    color: var(--muted);
    font-size: 14px;
    font-weight: 430;
    line-height: 1.65;
  }}
  .details {{
    margin: 24px 0 0;
    border: 1px solid var(--line);
    border-radius: 8px;
    overflow: hidden;
    background: var(--surface-soft);
  }}
  .row {{
    display: grid;
    grid-template-columns: 70px minmax(0, 1fr);
    gap: 14px;
    padding: 10px 13px;
    border-top: 1px solid var(--line);
    font-size: 12px;
    font-weight: 460;
    line-height: 1.5;
  }}
  .row:first-child {{ border-top: 0; }}
  .label {{ color: var(--muted); }}
  .value {{
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--text);
    font-family: inherit;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }}
  .actions {{
    display: flex;
    align-items: center;
    gap: 9px;
    flex-wrap: wrap;
    margin-top: auto;
    padding-top: 24px;
  }}
  button {{
    min-height: 36px;
    border: 1px solid transparent;
    border-radius: 7px;
    padding: 0 13px;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: background 140ms ease, border-color 140ms ease, color 140ms ease, opacity 140ms ease;
    will-change: transform;
  }}
  button:hover {{ transform: translateY(-1px); }}
  button:active {{ transform: translateY(0); }}
  .action-button {{
    color: #fff;
    background: var(--accent);
  }}
  .action-button:hover {{ background: var(--accent-hover); }}
  .secondary-button {{
    color: var(--accent);
    border-color: #c5dfdc;
    background: var(--accent-soft);
  }}
  .secondary-button:hover {{ background: #dcefeb; border-color: #a8d2cd; }}
  button:disabled {{ cursor: default; opacity: 0.55; }}
  button:disabled:hover {{ transform: none; }}
  .status {{
    flex: 1 1 100%;
    min-height: 18px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 460;
  }}
  .footer {{
    margin-top: 16px;
    color: #9aa5ae;
    font-size: 11px;
    font-weight: 430;
  }}
  @media (max-width: 680px) {{
    body {{ padding: 62px 18px 24px; align-items: flex-start; }}
    .panel {{ grid-template-columns: 1fr; min-height: 0; }}
    .signal {{ min-height: 120px; }}
    .signal::before {{ width: 76px; height: 76px; }}
    .signal::after {{ width: 76px; height: 76px; }}
    .content {{ padding: 28px 24px 24px; }}
    h1 {{ font-size: 25px; }}
    .actions {{ margin-top: 22px; }}
    button {{ flex: 1 1 auto; }}
  }}
  @media (max-width: 420px) {{
    .row {{ grid-template-columns: 1fr; gap: 3px; }}
    button {{ width: 100%; }}
  }}
  @keyframes page-in {{ from {{ opacity: 0; }} to {{ opacity: 1; }} }}
  @keyframes panel-in {{ from {{ opacity: 0; transform: translateY(12px) scale(0.985); }} to {{ opacity: 1; transform: translateY(0) scale(1); }} }}
  @keyframes content-in {{ from {{ opacity: 0; transform: translateX(10px); }} to {{ opacity: 1; transform: translateX(0); }} }}
  @keyframes signal-expand {{ 0% {{ opacity: 0.72; transform: scale(0.72); }} 68% {{ opacity: 0.12; }} 100% {{ opacity: 0; transform: scale(2.8); }} }}
  @keyframes core-breathe {{ 0%, 100% {{ transform: scale(1); }} 50% {{ transform: scale(1.045); }} }}
  @keyframes status-pulse {{ 0%, 100% {{ opacity: 0.62; }} 50% {{ opacity: 1; }} }}
  @media (prefers-reduced-motion: reduce) {{
    *, *::before, *::after {{ animation-duration: 0.01ms !important; animation-iteration-count: 1 !important; transition-duration: 0.01ms !important; }}
  }}
</style>
</head>
<body>
  <video class="error-background-video" autoplay muted loop playsinline preload="auto" aria-hidden="true">
    <source src="data:video/mp4;base64,{splash_video_b64}" type="video/mp4">
  </video>
  <div class="error-background-scrim" aria-hidden="true"></div>
  <main class="panel">
    <div class="signal" aria-hidden="true">
      <div class="signal-core">
        <svg viewBox="0 0 24 24"><path d="M12 8v4m0 4h.01"/><path d="M10.3 3.9 2.8 17a2 2 0 0 0 1.7 3h15a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z"/></svg>
      </div>
    </div>
    <div class="content">
      <div class="eyebrow">{error_label}</div>
      <h1>{heading}</h1>
      <p class="lead">{description}</p>
      <section class="details" aria-label="{connection_info}">
        <div class="row">
          <div class="label">{address}</div>
          <div id="backend-url" class="value"></div>
        </div>
        <div class="row">
          <div class="label">{error_label}</div>
          <div id="error-detail" class="value"></div>
        </div>
      </section>
      <div class="actions">
        <button id="retry-button" class="action-button" type="button">{retry}</button>
        <button id="gui-log-button" class="secondary-button" type="button">{download_gui_log}</button>
        <button id="launcher-log-button" class="secondary-button" type="button">{download_launcher_log}</button>
        <span id="retry-status" class="status"></span>
      </div>
      <div class="footer">AzurPilot · {connection_info}</div>
    </div>
  </main>
  <script>
    (function () {{
{titlebar_script}
    }})();

    const i18n = {i18n_json};
    const backendUrl = {backend_url_json};
    const errorDetail = {error_detail_json};
    const port = {port};
    const retryButton = document.getElementById('retry-button');
    const guiLogButton = document.getElementById('gui-log-button');
    const launcherLogButton = document.getElementById('launcher-log-button');
    const retryStatus = document.getElementById('retry-status');
    const invoke =
      (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
      || (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);

    document.getElementById('backend-url').textContent = backendUrl;
    document.getElementById('error-detail').textContent = errorDetail;

    retryButton.addEventListener('click', async () => {{
      retryButton.disabled = true;
      retryStatus.textContent = i18n.reconnecting;
      try {{
        if (typeof invoke !== 'function') {{
          throw new Error('Tauri invoke is unavailable');
        }}
        const connected = await invoke('retry_backend_connection', {{ port }});
        if (!connected) {{
          retryStatus.textContent = i18n.stillFailed;
          retryButton.disabled = false;
        }}
      }} catch (error) {{
        retryStatus.textContent = i18n.retryFailed + (error && error.message ? error.message : error);
        retryButton.disabled = false;
      }}
    }});

    async function downloadLog(button, command, label) {{
      button.disabled = true;
      retryStatus.textContent = i18n.preparing.replace('%{{label}}', label);
      try {{
        if (typeof invoke !== 'function') {{
          throw new Error('Tauri invoke is unavailable');
        }}
        const filename = await invoke(command);
        retryStatus.textContent = i18n.saved.replace('%{{filename}}', filename);
      }} catch (error) {{
        retryStatus.textContent = i18n.downloadFailed.replace('%{{label}}', label) + (error && error.message ? error.message : error);
      }} finally {{
        button.disabled = false;
      }}
    }}

    guiLogButton.addEventListener('click', () => {{
      downloadLog(guiLogButton, 'download_today_gui_log', '{gui_log_label}');
    }});

    launcherLogButton.addEventListener('click', () => {{
      downloadLog(launcherLogButton, 'download_today_launcher_log', '{launcher_log_label}');
    }});

    // 每秒尝试自动刷新（重试连接）
    setInterval(() => {{
      if (!retryButton.disabled) {{
        retryButton.click();
      }}
    }}, 1000);
  </script>
</body>
</html>"#,
        title = t!("error_page.title"),
        heading = t!("error_page.heading"),
        description = t!("error_page.description"),
        address = t!("error_page.address"),
        error_label = t!("error_page.error_label"),
        retry = t!("error_page.retry"),
        download_gui_log = t!("error_page.download_gui_log"),
        download_launcher_log = t!("error_page.download_launcher_log"),
        gui_log_label = t!("error_page.download_gui_log"),
        launcher_log_label = t!("error_page.download_launcher_log"),
        connection_info = t!("error_page.connection_info"),
    )
}

fn splash_redesigned_shell_html(video_bg_b64: &str, mi_sans_font_b64: &str) -> String {
    let i18n = serde_json::json!({
        "defaultTip": t!("tips.17"),
        "loading": t!("splash.loading_badge"),
        "webuiInit": t!("splash.webui_init"),
        "starting": t!("splash.starting"),
        "errorBadge": t!("splash.error_badge"),
        "initStopped": t!("splash.init_stopped"),
        "progressMetaReady": t!("splash.progress_meta_ready"),
        "preparingLog": t!("splash.preparing_log"),
        "logSavedPrefix": t!("splash.log_saved_prefix"),
        "logFailed": t!("splash.log_failed"),
    });
    let i18n_json = to_string(&i18n).unwrap();

    r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  @font-face {
    font-family: "MiSans";
    src: url(data:font/ttf;base64,$MI_SANS_FONT) format("truetype");
    font-weight: 100 900;
    font-style: normal;
    font-display: swap;
  }
  :root {
    --primary-color: #4facfe;
    --secondary-color: #00f2fe;
    --text-main: #ffffff;
    --text-sub: rgba(255, 255, 255, 0.76);
    --text-muted: rgba(255, 255, 255, 0.52);
    --surface-soft: rgba(255, 255, 255, 0.16);
    --surface-border: rgba(255, 255, 255, 0.15);
    --danger: #ff5f57;
    --warning: #ffbd2e;
  }
  * {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
    user-select: none;
  }
  html,
  body {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #111827;
  }
  body {
    font-family: "MiSans", sans-serif;
    font-weight: 420;
    font-synthesis: none;
    color: var(--text-main);
  }
  button {
    font: inherit;
  }
  .launcher-window {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    border-radius: 0;
    background: #111827;
    box-shadow: none;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
  }
  .splash-background-video {
    position: absolute;
    inset: 0;
    z-index: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    pointer-events: none;
  }
  .launcher-window::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 1;
    background:
      linear-gradient(to bottom, rgba(0, 0, 0, 0.05) 0%, rgba(0, 0, 0, 0.03) 42%, rgba(0, 0, 0, 0.28) 100%),
      linear-gradient(115deg, rgba(12, 30, 72, 0.10), rgba(255, 126, 117, 0.05));
    pointer-events: none;
  }
  body.error-state .launcher-window::before {
    background:
      linear-gradient(to bottom, rgba(56, 0, 10, 0.28) 0%, rgba(78, 0, 13, 0.18) 42%, rgba(60, 0, 12, 0.68) 100%),
      linear-gradient(115deg, rgba(255, 95, 87, 0.34), rgba(255, 189, 46, 0.08));
  }
  .top-bar {
    position: relative;
    z-index: 2;
    display: flex;
    justify-content: space-between;
    align-items: center;
    min-height: 60px;
    padding: 18px 24px;
    touch-action: none;
    app-region: drag;
    -webkit-app-region: drag;
  }
  .brand-zone {
    display: flex;
    align-items: center;
    min-width: 0;
    gap: 10px;
  }
  .app-title {
    color: var(--text-main);
    font-size: 18px;
    font-weight: 610;
    letter-spacing: 0;
    text-shadow: 0 2px 6px rgba(0, 0, 0, 0.22);
  }
  .app-version {
    color: var(--text-sub);
    font-size: 12px;
    font-weight: 460;
    line-height: 1;
    background: rgba(255, 255, 255, 0.14);
    border: 1px solid rgba(255, 255, 255, 0.11);
    padding: 4px 9px;
    border-radius: 999px;
    backdrop-filter: blur(8px);
  }
  .top-right {
    display: flex;
    align-items: center;
    gap: 18px;
    min-width: 0;
  }
  .status-badge {
    max-width: 260px;
    min-height: 28px;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    border-radius: 999px;
    padding: 6px 14px;
    color: var(--text-main);
    background: var(--surface-soft);
    border: 1px solid var(--surface-border);
    backdrop-filter: blur(12px);
    box-shadow: 0 10px 24px rgba(0, 0, 0, 0.12);
    font-size: 12px;
    font-weight: 460;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    animation: pulse 2.2s ease-in-out infinite;
  }
  .status-badge::before {
    content: "";
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--secondary-color);
    box-shadow: 0 0 12px rgba(0, 242, 254, 0.7);
    flex: 0 0 auto;
  }
  .window-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 0 auto;
    app-region: no-drag;
    -webkit-app-region: no-drag;
  }
  .window-controls * {
    app-region: no-drag;
    -webkit-app-region: no-drag;
  }
  .win-btn {
    width: 13px;
    height: 13px;
    border: 0;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
    transition: filter 140ms ease, transform 140ms ease;
  }
  .win-btn:hover {
    filter: brightness(1.07);
    transform: scale(1.04);
  }
  .win-btn:active {
    filter: brightness(0.9);
    transform: scale(0.97);
  }
  .win-btn svg {
    width: 7px;
    height: 7px;
    stroke: rgba(50, 42, 35, 0.72);
    stroke-width: 1.45;
    stroke-linecap: round;
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .window-controls:hover .win-btn svg {
    opacity: 1;
  }
  .win-btn.minimize {
    background: var(--warning);
    box-shadow: 0 0 0 0.5px rgba(156, 110, 6, 0.55);
  }
  .win-btn.close {
    background: var(--danger);
    box-shadow: 0 0 0 0.5px rgba(160, 32, 28, 0.55);
  }
  .main-content {
    position: relative;
    z-index: 2;
    padding: 0 40px 35px;
  }
  .update-status {
    margin-bottom: 25px;
    max-width: min(650px, 100%);
  }
  .title-group {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 8px;
  }
  .spinner {
    width: 22px;
    height: 22px;
    border: 2.5px solid rgba(255, 255, 255, 0.24);
    border-top-color: var(--text-main);
    border-radius: 50%;
    animation: spin 0.9s linear infinite;
    flex: 0 0 auto;
  }
  .err-dot {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: #ffffff;
    color: #c73532;
    align-items: center;
    justify-content: center;
    font-size: 14px;
    font-weight: 800;
    box-shadow: 0 5px 16px rgba(0, 0, 0, 0.2);
    flex: 0 0 auto;
  }
  .main-action-text {
    min-width: 0;
    color: var(--text-main);
    font-size: 24px;
    line-height: 1.2;
    font-weight: 620;
    letter-spacing: 0;
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.32);
  }
  .sub-action-text {
    color: var(--text-sub);
    font-size: 12px;
    font-weight: 480;
    letter-spacing: 1.2px;
    line-height: 1.45;
    margin: 0;
    max-width: min(650px, 100%);
    max-height: 54px;
    overflow: hidden;
    text-shadow: 0 1px 5px rgba(0, 0, 0, 0.28);
    white-space: pre-line;
  }
  .progress-container {
    position: relative;
    margin-bottom: 15px;
  }
  .progress-bar-bg {
    width: 100%;
    height: 6px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.22);
    overflow: hidden;
    backdrop-filter: blur(5px);
  }
  .progress-bar-fill {
    width: 4%;
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, var(--primary-color), var(--secondary-color));
    box-shadow: 0 0 14px rgba(0, 242, 254, 0.5);
    position: relative;
    overflow: hidden;
    transition: width 0.35s ease, background 0.2s ease;
  }
  .progress-bar-fill::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.48), transparent);
    transform: translateX(-100%);
    animation: sweep 2s ease-in-out infinite;
  }
  .progress-bar-fill-error {
    background: linear-gradient(90deg, #ff5f57, #ffbd2e);
    box-shadow: 0 0 14px rgba(255, 95, 87, 0.46);
  }
  .progress-bar-fill-error::after {
    display: none;
  }
  .progress-percentage {
    position: absolute;
    right: 0;
    top: -25px;
    color: var(--text-main);
    font-size: 14px;
    font-weight: 680;
    font-variant-numeric: tabular-nums;
    text-shadow: 0 2px 6px rgba(0, 0, 0, 0.32);
  }
  .uv-progress-container {
    display: none;
    margin-top: -3px;
    margin-bottom: 15px;
  }
  .uv-progress-container.is-visible {
    display: block;
  }
  .uv-progress-header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 6px;
    color: var(--text-sub);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .uv-progress-detail {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-align: right;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .uv-progress-bar-bg {
    width: 100%;
    height: 4px;
    overflow: hidden;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.16);
  }
  .uv-progress-bar-fill {
    position: relative;
    width: 2%;
    height: 100%;
    overflow: hidden;
    border-radius: inherit;
    background: linear-gradient(90deg, #77e7a4, #47b8ff);
    box-shadow: 0 0 10px rgba(71, 184, 255, 0.42);
    transition: width 0.45s ease;
  }
  .uv-progress-bar-fill::after {
    position: absolute;
    inset: 0;
    content: "";
    background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.42), transparent);
    transform: translateX(-100%);
    animation: sweep 1.8s ease-in-out infinite;
  }
  .footer-info {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    min-height: 28px;
    font-size: 12px;
  }
  .tip-text {
    min-width: 0;
    max-width: 520px;
    color: var(--text-sub);
    background: rgba(0, 0, 0, 0.16);
    border-left: 3px solid var(--primary-color);
    border-radius: 4px;
    padding: 5px 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 460;
    backdrop-filter: blur(7px);
  }
  .footer-right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    flex: 0 0 auto;
  }
  .notice-text {
    color: var(--text-muted);
    white-space: nowrap;
    font-weight: 450;
  }
  .splash-actions {
    display: none;
  }
  .splash-actions-err {
    display: block;
  }
  .splash-log-button {
    min-height: 28px;
    border: 1px solid rgba(255, 255, 255, 0.28);
    border-radius: 6px;
    padding: 0 11px;
    color: var(--text-main);
    background: rgba(255, 255, 255, 0.14);
    backdrop-filter: blur(10px);
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
  }
  .splash-log-button:hover {
    background: rgba(255, 255, 255, 0.23);
  }
  .splash-log-button:disabled {
    cursor: default;
    opacity: 0.65;
  }
  body.error-state .status-badge {
    background: rgba(255, 255, 255, 0.18);
    animation: none;
  }
  body.error-state .status-badge::before {
    background: #ff5f57;
    box-shadow: 0 0 12px rgba(255, 95, 87, 0.76);
  }
  body.error-state .tip-text {
    border-left-color: #ffbd2e;
  }
  @media (max-width: 720px) {
    .top-bar {
      padding: 16px 20px;
    }
    .status-badge {
      max-width: 180px;
    }
    .main-content {
      padding: 0 28px 28px;
    }
    .main-action-text {
      font-size: 22px;
    }
  }
  @media (max-width: 560px), (max-height: 340px) {
    .top-right {
      gap: 12px;
    }
    .status-badge {
      display: none;
    }
    .footer-info {
      flex-direction: column;
      align-items: flex-start;
      gap: 8px;
    }
    .footer-right {
      width: 100%;
      justify-content: space-between;
    }
    .tip-text {
      max-width: 100%;
    }
  }
  @media (max-height: 340px) {
    .main-content {
      padding-bottom: 24px;
    }
    .update-status {
      margin-bottom: 18px;
    }
    .sub-action-text {
      max-height: 36px;
    }
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.9; transform: scale(1); }
    50% { opacity: 1; transform: scale(1.015); box-shadow: 0 0 18px rgba(255, 255, 255, 0.18); }
  }
  @keyframes sweep {
    to { transform: translateX(200%); }
  }
</style>
</head>
<body>
  <div class="launcher-window">
    <video class="splash-background-video" autoplay muted loop playsinline preload="auto" aria-hidden="true">
      <source src="data:video/mp4;base64,$VIDEO_BG" type="video/mp4">
    </video>
    <div id="splash-drag-region" class="top-bar">
      <div class="brand-zone">
        <span class="app-title">AzurPilot</span>
        <span class="app-version">v$LAUNCHER_VERSION</span>
      </div>
      <div class="top-right">
        <div id="badge" class="status-badge">
          <span id="badge-text">$I18N_INITIALIZING</span>
        </div>
        <div class="window-controls">
          <button id="window-minimize" class="win-btn minimize" type="button" aria-label="$I18N_MINIMIZE" title="$I18N_MINIMIZE">
            <svg viewBox="0 0 8 8" aria-hidden="true"><line x1="2" y1="4" x2="6" y2="4"></line></svg>
          </button>
          <button id="window-close" class="win-btn close" type="button" aria-label="$I18N_CLOSE" title="$I18N_CLOSE">
            <svg viewBox="0 0 8 8" aria-hidden="true"><line x1="2" y1="2" x2="6" y2="6"></line><line x1="6" y1="2" x2="2" y2="6"></line></svg>
          </button>
        </div>
      </div>
    </div>

    <div class="main-content">
      <div class="update-status">
        <div class="title-group">
          <div id="spinner" class="spinner"></div>
          <div id="error-dot" class="err-dot" style="display:none;">!</div>
          <h1 id="title" class="main-action-text">$I18N_STARTING</h1>
        </div>
        <p id="detail" class="sub-action-text">$I18N_WEBUI_INIT</p>
      </div>

      <div class="progress-container">
        <div id="progress-pct" class="progress-percentage">4%</div>
        <div class="progress-bar-bg">
          <div id="progress-fill" class="progress-bar-fill" style="width: 4%;"></div>
        </div>
      </div>

      <div id="uv-progress-container" class="uv-progress-container" aria-hidden="true">
        <div class="uv-progress-header">
          <span id="uv-progress-detail" class="uv-progress-detail"></span>
          <span id="uv-progress-pct">0%</span>
        </div>
        <div class="uv-progress-bar-bg">
          <div id="uv-progress-fill" class="uv-progress-bar-fill" style="width: 2%;"></div>
        </div>
      </div>

      <div class="footer-info">
        <div id="tip-text" class="tip-text">Tips: $I18N_DEFAULT_TIP</div>
        <div class="footer-right">
          <div id="progress-meta" class="notice-text">$I18N_PROGRESS_META</div>
          <div id="splash-actions" class="splash-actions">
            <button id="splash-log-button" class="splash-log-button" type="button">$I18N_DOWNLOAD_LOG</button>
          </div>
        </div>
      </div>
    </div>
  </div>

  <script>
    const i18n = $I18N_JSON;
    const defaultTip = i18n.defaultTip;
    const invoke =
      (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
      || (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
    const webviewDraggableRegionsEnabled = $NATIVE_TOUCH_DRAG;

    window.addEventListener('contextmenu', event => {
      event.preventDefault();
    }, { capture: true });

    function splitSubtitle(value) {
      const text = String(value || '').trim();
      if (!text) {
        return { status: i18n.loading, tip: defaultTip };
      }
      const match = text.match(/^(.*?)\s*\|\s*Tips[:：]\s*(.*)$/);
      if (!match) {
        return { status: text, tip: defaultTip };
      }
      return {
        status: match[1].trim() || i18n.loading,
        tip: match[2].trim() || defaultTip,
      };
    }

    function normalizeDetail(value) {
      const text = String(value || '').trim();
      return text || i18n.webuiInit;
    }

    window.__ALAS_SPLASH_UPDATE = function (payload) {
      const badge = document.getElementById('badge');
      const badgeText = document.getElementById('badge-text');
      const spinner = document.getElementById('spinner');
      const errorDot = document.getElementById('error-dot');
      const progressFill = document.getElementById('progress-fill');
      const progressPct = document.getElementById('progress-pct');
      const uvProgressContainer = document.getElementById('uv-progress-container');
      const uvProgressFill = document.getElementById('uv-progress-fill');
      const uvProgressPct = document.getElementById('uv-progress-pct');
      const uvProgressDetail = document.getElementById('uv-progress-detail');
      const progressMeta = document.getElementById('progress-meta');
      const splashActions = document.getElementById('splash-actions');
      const subtitle = splitSubtitle(payload.subtitle);

      badgeText.textContent = payload.is_error ? i18n.errorBadge : subtitle.status;
      document.getElementById('tip-text').textContent = 'Tips: ' + subtitle.tip;
      document.getElementById('title').textContent = payload.title || i18n.starting;
      document.getElementById('detail').textContent = normalizeDetail(payload.detail);
      progressMeta.textContent = payload.is_error
        ? i18n.initStopped
        : i18n.progressMetaReady;

      const progress = Math.max(0, Math.min(100, Number(payload.progress || 0)));
      progressFill.style.width = progress + '%';
      progressPct.textContent = progress + '%';

      const uvState = payload.uv_progress;
      const hasUvProgress = !payload.is_error
        && uvState
        && Number.isFinite(Number(uvState.progress));
      uvProgressContainer.classList.toggle('is-visible', Boolean(hasUvProgress));
      uvProgressContainer.setAttribute('aria-hidden', String(!hasUvProgress));
      if (hasUvProgress) {
        const uvProgress = Math.max(0, Math.min(99, Number(uvState.progress)));
        uvProgressFill.style.width = uvProgress + '%';
        uvProgressPct.textContent = uvProgress + '%';
        uvProgressDetail.textContent = String(uvState.detail || '');
      }

      if (payload.is_error) {
        document.body.classList.add('error-state');
        badge.className = 'status-badge status-badge-err';
        spinner.style.display = 'none';
        errorDot.style.display = 'flex';
        progressFill.className = 'progress-bar-fill progress-bar-fill-error';
        splashActions.className = 'splash-actions splash-actions-err';
      } else {
        document.body.classList.remove('error-state');
        badge.className = 'status-badge';
        spinner.style.display = 'block';
        errorDot.style.display = 'none';
        progressFill.className = 'progress-bar-fill';
        splashActions.className = 'splash-actions';
      }
    };

    const splashDragRegion = document.getElementById('splash-drag-region');
    splashDragRegion.addEventListener('pointerdown', event => {
      if (!event.isPrimary || event.button !== 0 || event.target.closest('button')) {
        return;
      }
      if (webviewDraggableRegionsEnabled) {
        return;
      }
      event.preventDefault();
      if (typeof invoke !== 'function') {
        return;
      }
      invoke('window_start_dragging').catch(error => {
        console.error('Failed to drag splash window', error);
      });
    });

    document.getElementById('window-minimize').addEventListener('click', event => {
      event.stopPropagation();
      if (typeof invoke === 'function') {
        invoke('window_minimize').catch(error => {
          console.error('Failed to minimize splash window', error);
        });
      }
    });

    document.getElementById('window-close').addEventListener('click', event => {
      event.stopPropagation();
      if (typeof invoke === 'function') {
        invoke('window_close').catch(error => {
          console.error('Failed to close splash window', error);
        });
      }
    });

    document.getElementById('splash-log-button').addEventListener('click', async () => {
      const button = document.getElementById('splash-log-button');
      const progressMeta = document.getElementById('progress-meta');
      button.disabled = true;
      progressMeta.textContent = i18n.preparingLog;
      try {
        if (typeof invoke !== 'function') {
          throw new Error('Tauri invoke is unavailable');
        }
        const filename = await invoke('download_today_launcher_log');
        progressMeta.textContent = i18n.logSavedPrefix + filename;
      } catch (error) {
        progressMeta.textContent = i18n.logFailed + (error && error.message ? error.message : error);
      } finally {
        button.disabled = false;
      }
    });

    window.__ALAS_SPLASH_READY = true;
  </script>
</body>
</html>"#
    .replace("$VIDEO_BG", video_bg_b64)
    .replace("$MI_SANS_FONT", mi_sans_font_b64)
    .replace("$LAUNCHER_VERSION", env!("CARGO_PKG_VERSION"))
    .replace("$I18N_JSON", &i18n_json)
    .replace("$NATIVE_TOUCH_DRAG", if cfg!(windows) { "true" } else { "false" })
    .replace("$I18N_INITIALIZING", &escape_html(t!("splash.initializing")))
    .replace("$I18N_MINIMIZE", &escape_html(t!("titlebar.minimize")))
    .replace("$I18N_CLOSE", &escape_html(t!("titlebar.close")))
    .replace("$I18N_STARTING", &escape_html(t!("splash.starting")))
    .replace("$I18N_WEBUI_INIT", &escape_html(t!("splash.webui_init")))
    .replace("$I18N_DEFAULT_TIP", &escape_html(t!("tips.17")))
    .replace("$I18N_PROGRESS_META", &escape_html(t!("splash.progress_meta_ready")))
    .replace("$I18N_DOWNLOAD_LOG", &escape_html(t!("splash.download_log")))
}

fn create_main_window(app: &tauri::AppHandle, port: u16) -> Result<WebviewWindow> {
    let main_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .ok_or_else(|| anyhow!("Main window config not found"))?;

    let app_for_navigation = app.clone();
    let main_window = tauri::WebviewWindowBuilder::from_config(app, main_config)?
        .on_navigation(move |url| handle_backend_navigation(app_for_navigation.clone(), port, url))
        .on_page_load(page_load_injector)
        .build()?;
    main_window.set_resizable(true)?;

    // Windows/Linux: remove native decorations for the main window as well.
    // Splash is configured as borderless in tauri.conf.json.
    #[cfg(not(target_os = "macos"))]
    {
        main_window.set_decorations(false)?;
    }

    Ok(main_window)
}

fn reveal_window(window: &WebviewWindow) -> tauri::Result<()> {
    if window.is_minimized()? {
        window.unminimize()?;
    }
    window.show()?;
    window.set_focus()?;
    Ok(())
}

fn minimize_main_window_to_tray(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        if let Some(window) = app.get_webview_window("main") {
            info!("Destroying main window to release WebView resources while trayed");
            if let Err(e) = window.destroy() {
                warn!("Failed to destroy main window for tray mode: {:?}", e);
            }
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    }

    #[cfg(target_os = "macos")]
    {
        set_macos_activation_policy(app, false);
    }
}

fn restore_main_window_from_any_thread(
    app: tauri::AppHandle,
    port: u16,
    recreating_main_window: Arc<AtomicBool>,
) {
    let app_for_restore = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        restore_main_window_from_tray(&app_for_restore, port, recreating_main_window);
    }) {
        warn!("Failed to schedule main window restore: {:?}", e);
    }
}

fn restore_main_window_from_tray(
    app: &tauri::AppHandle,
    port: u16,
    recreating_main_window: Arc<AtomicBool>,
) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        set_macos_activation_policy(app, true);
        let _ = reveal_window(&window);
        return;
    }

    if recreating_main_window
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        debug!("Main window recreation already in progress");
        return;
    }

    let app_handle = app.clone();
    thread::spawn(move || {
        #[cfg(target_os = "macos")]
        set_macos_activation_policy(&app_handle, true);

        let result = (|| -> Result<()> {
            let window = create_main_window(&app_handle, port)?;
            navigate_backend_or_error(&window, port)?;
            reveal_window(&window)?;
            Ok(())
        })();

        recreating_main_window.store(false, Ordering::SeqCst);

        if let Err(e) = result {
            error!("Failed to recreate main window from tray: {:?}", e);
        }
    });
}

fn toggle_main_window_visibility(
    app: &tauri::AppHandle,
    port: u16,
    recreating_main_window: Arc<AtomicBool>,
) {
    if let Some(window) = app.get_webview_window("main") {
        let is_visible = window.is_visible().unwrap_or(false);
        let is_minimized = window.is_minimized().unwrap_or(false);
        if is_visible && !is_minimized {
            minimize_main_window_to_tray(app);
        } else {
            restore_main_window_from_tray(app, port, recreating_main_window);
        }
    } else {
        restore_main_window_from_tray(app, port, recreating_main_window);
    }
}

fn main_window_titlebar_injection_script() -> String {
    #[cfg(target_os = "macos")]
    {
        String::new()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let i18n = serde_json::json!({
            "hideLabel": t!("titlebar.minimize_to_tray"),
            "minimizeLabel": t!("titlebar.minimize_window"),
            "minimizeTitle": t!("titlebar.minimize"),
            "maximizeLabel": t!("titlebar.maximize_restore_window"),
            "maximizeTitle": t!("titlebar.maximize"),
            "closeLabel": t!("titlebar.close_window"),
            "closeTitle": t!("titlebar.close"),
            "restoreTitle": t!("titlebar.restore"),
            "maximizeActionTitle": t!("titlebar.maximize_action"),
            "restoreLabel": t!("titlebar.restore_window"),
            "maximizeLabelText": t!("titlebar.maximize_window"),
            "closePrompt": t!("dialog.confirm_exit"),
            "exitAction": t!("dialog.exit"),
            "minimizeToTrayAction": t!("dialog.minimize_to_tray"),
        });
        let i18n_json = serde_json::to_string(&i18n).unwrap();
        let mut s = String::with_capacity(8192);
        s.push_str("const i18n = ");
        s.push_str(&i18n_json);
        s.push_str(if cfg!(windows) {
            ";const webviewDraggableRegionsEnabled = true;const closePromptEnabled = true;"
        } else {
            ";const webviewDraggableRegionsEnabled = false;const closePromptEnabled = false;"
        });
        s.push_str(r#";
        const invoke =
            (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
            || (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
        if (typeof invoke !== 'function') {
            return;
        }
        const ensureTitlebar = () => {
            if (!document.body || document.getElementById('alas-launcher-titlebar')) {
                return;
            }
            if (!document.getElementById('alas-launcher-titlebar-style')) {
                const style = document.createElement('style');
                style.id = 'alas-launcher-titlebar-style';
                style.textContent = ':root{--alas-titlebar-height:44px}#alas-launcher-titlebar{position:fixed;top:0;left:0;right:0;height:var(--alas-titlebar-height);z-index:2147483647;user-select:none;pointer-events:none;background:transparent}#alas-launcher-titlebar *{box-sizing:border-box}.alas-titlebar-drag-zone{position:absolute;inset:0 120px 0 0;height:100%;pointer-events:auto;background:transparent;touch-action:none;app-region:drag;-webkit-app-region:drag}.header-icon,.header-icon *{app-region:no-drag;-webkit-app-region:no-drag}.header-icon{display:flex;align-items:center;gap:8px;padding:0 12px;position:absolute;top:0;right:0;height:100%;pointer-events:auto}.icon{width:12px;height:12px;min-width:12px;min-height:12px;margin:0;padding:0;line-height:1;border-radius:50%;border:none;cursor:pointer;flex:0 0 auto;position:relative;transition:filter 120ms ease;display:inline-flex;align-items:center;justify-content:center}.icon:active{filter:brightness(0.85)}.icon-hide{background:#3b82f6;box-shadow:0 0 0 .5px #2563eb}.icon-close{background:#ff5f57;box-shadow:0 0 0 .5px #e0443e}.icon-minimize{background:#febc2e;box-shadow:0 0 0 .5px #d4a017}.icon-maximize{background:#28c840;box-shadow:0 0 0 .5px #14ae35}.icon svg{width:7px;height:7px;stroke:rgba(0,0,0,.72);fill:none;stroke-width:1.35;stroke-linecap:round;stroke-linejoin:round;opacity:0;transition:opacity 150ms ease}.header-icon:hover .icon svg{opacity:1}@media(max-width:680px){.alas-titlebar-drag-zone{inset-right:88px}}';
                style.textContent += '#alas-close-menu{position:fixed;top:8px;right:8px;z-index:2147483647;width:244px;padding:11px;border:1px solid rgba(255,255,255,.16);border-radius:18px;background:rgba(22,25,31,.92);box-shadow:0 18px 46px rgba(0,0,0,.3);backdrop-filter:blur(18px) saturate(1.25);-webkit-backdrop-filter:blur(18px) saturate(1.25);color:#fff;opacity:0;pointer-events:none;transform:translateY(-14px) scale(.72);transform-origin:calc(100% - 16px) 18px;transition:opacity 160ms ease,transform 220ms cubic-bezier(.2,.9,.25,1);app-region:no-drag;-webkit-app-region:no-drag}#alas-close-menu.is-open{opacity:1;pointer-events:auto;transform:translateY(0) scale(1)}#alas-close-menu *{box-sizing:border-box;app-region:no-drag;-webkit-app-region:no-drag}#alas-close-menu-title{margin:0 0 10px;font:500 12px/1.45 "MiSans",sans-serif;color:rgba(255,255,255,.78)}#alas-close-menu-actions{display:grid;grid-template-columns:1fr 1fr;gap:7px}#alas-close-menu button{display:flex;align-items:center;justify-content:center;min-width:0;min-height:34px;margin:0;padding:0 10px;border:1px solid rgba(255,255,255,.14);border-radius:10px;background:rgba(255,255,255,.1);color:#fff;font:600 12px/1 "MiSans",sans-serif;cursor:pointer;transition:background 120ms ease,transform 120ms ease}#alas-close-menu button:hover{transform:translateY(-1px);background:rgba(255,255,255,.18)}#alas-close-menu button:active{transform:translateY(0)}#alas-close-menu button:disabled{opacity:.55;cursor:default;transform:none}#alas-close-menu .alas-close-confirm{border-color:rgba(255,113,106,.38);background:rgba(202,56,52,.82)}#alas-close-menu .alas-close-confirm:hover{background:rgba(225,68,63,.94)}';
                document.head.appendChild(style);
            }
            const titlebar = document.createElement('div');
            titlebar.id = 'alas-launcher-titlebar';
            titlebar.innerHTML = '<div class="alas-titlebar-drag-zone" aria-hidden="true"></div><div class="header-icon"><button type="button" class="icon icon-hide" data-action="hide" aria-label="'+i18n.hideLabel+'" title="'+i18n.hideLabel+'"><svg viewBox="0 0 6 6"><rect x="1" y="1" width="4" height="4" rx="1"/><path d="M2 3h2"/></svg></button><button type="button" class="icon icon-minimize" data-action="minimize" aria-label="'+i18n.minimizeLabel+'" title="'+i18n.minimizeTitle+'"><svg viewBox="0 0 6 6"><line x1="1" y1="3" x2="5" y2="3"/></svg></button><button type="button" class="icon icon-maximize" data-action="maximize" aria-label="'+i18n.maximizeLabel+'" title="'+i18n.maximizeTitle+'"><svg viewBox="0 0 6 6" class="svg-restore" style="display:none"><polyline points="1,3 1,1 3,1"/><polyline points="3,5 5,5 5,3"/></svg><svg viewBox="0 0 6 6" class="svg-maximize"><polyline points="1,2.5 1,1 2.5,1"/><polyline points="3.5,5 5,5 5,3.5"/></svg></button><button type="button" class="icon icon-close" data-action="close" aria-label="'+i18n.closeLabel+'" title="'+i18n.closeTitle+'"><svg viewBox="0 0 6 6"><line x1="1" y1="1" x2="5" y2="5"/><line x1="5" y1="1" x2="1" y2="5"/></svg></button></div>';
            document.body.dataset.alasCustomTitlebar = 'true';
            document.body.prepend(titlebar);
            const dragZone = titlebar.querySelector('.alas-titlebar-drag-zone');
            const maximizeButton = titlebar.querySelector('[data-action="maximize"]');
            let closeMenu = document.getElementById('alas-close-menu');
            if (!closeMenu) {
                closeMenu = document.createElement('div');
                closeMenu.id = 'alas-close-menu';
                closeMenu.setAttribute('role', 'dialog');
                closeMenu.setAttribute('aria-modal', 'false');
                closeMenu.innerHTML = '<p id="alas-close-menu-title"></p><div id="alas-close-menu-actions"><button type="button" data-close-action="minimize"></button><button type="button" class="alas-close-confirm" data-close-action="exit"></button></div>';
                closeMenu.querySelector('#alas-close-menu-title').textContent = i18n.closePrompt;
                closeMenu.querySelector('[data-close-action="minimize"]').textContent = i18n.minimizeToTrayAction;
                closeMenu.querySelector('[data-close-action="exit"]').textContent = i18n.exitAction;
                closeMenu.addEventListener('pointerdown', event => event.stopPropagation());
                document.body.appendChild(closeMenu);
            }
            const setCloseMenuOpen = open => {
                closeMenu.classList.toggle('is-open', open);
                if (open) closeMenu.querySelector('[data-close-action="minimize"]').focus({ preventScroll: true });
            };
            const showClosePrompt = () => {
                if (!closePromptEnabled) {
                    invoke('window_close').catch(error => console.error('Failed to close window', error));
                    return;
                }
                setCloseMenuOpen(true);
            };
            window.__ALAS_OPEN_CLOSE_PROMPT = showClosePrompt;
            closeMenu.querySelector('[data-close-action="minimize"]').addEventListener('click', async () => {
                setCloseMenuOpen(false);
                try { await invoke('window_hide'); }
                catch (error) { console.error('Failed to minimize window to tray', error); }
            });
            closeMenu.querySelector('[data-close-action="exit"]').addEventListener('click', async () => {
                closeMenu.querySelectorAll('button').forEach(button => { button.disabled = true; });
                try { await invoke('window_exit_application'); }
                catch (error) {
                    closeMenu.querySelectorAll('button').forEach(button => { button.disabled = false; });
                    console.error('Failed to exit application', error);
                }
            });
            document.addEventListener('pointerdown', event => {
                if (closeMenu.classList.contains('is-open') && !closeMenu.contains(event.target)) setCloseMenuOpen(false);
            });
            document.addEventListener('keydown', event => {
                if (event.key === 'Escape' && closeMenu.classList.contains('is-open')) setCloseMenuOpen(false);
            });
            const syncMaximizeState = async () => {
                if (!maximizeButton) return;
                try {
                    const maximized = await invoke('window_is_maximized');
                    maximizeButton.dataset.maximized = maximized ? 'true' : 'false';
                    maximizeButton.title = maximized ? i18n.restoreTitle : i18n.maximizeActionTitle;
                    maximizeButton.setAttribute('aria-label', maximized ? i18n.restoreLabel : i18n.maximizeLabelText);
                    maximizeButton.querySelector('.svg-maximize').style.display = maximized ? 'none' : '';
                    maximizeButton.querySelector('.svg-restore').style.display = maximized ? '' : 'none';
                } catch (e) {
                    console.error('Failed to sync maximize state', e);
                }
            };
            titlebar.querySelectorAll('button[data-action]').forEach(button => {
                button.addEventListener('click', async event => {
                    event.stopPropagation();
                    try {
                        switch (button.dataset.action) {
                            case 'hide': await invoke('window_hide'); break;
                            case 'minimize': await invoke('window_minimize'); break;
                            case 'maximize': await invoke('window_toggle_maximize'); await syncMaximizeState(); break;
                            case 'close': showClosePrompt(); break;
                        }
                    } catch (error) {
                        console.error('Failed to handle ' + button.dataset.action + ' window action', error);
                    }
                });
            });
            dragZone.addEventListener('pointerdown', event => {
                if (!event.isPrimary || event.button !== 0 || event.target.closest('button')) return;
                if (webviewDraggableRegionsEnabled) return;
                event.preventDefault();
                invoke('window_start_dragging').catch(error => { console.error('Failed to start dragging from titlebar', error); });
            });
            dragZone.addEventListener('dblclick', async event => {
                if (event.target.closest('button')) return;
                try { await invoke('window_toggle_maximize'); await syncMaximizeState(); }
                catch (error) { console.error('Failed to toggle maximize from titlebar', error); }
            });
            window.addEventListener('resize', () => { void syncMaximizeState(); });
            void syncMaximizeState();
        };
        ensureTitlebar();
        if (!document.body) {
            window.addEventListener('DOMContentLoaded', ensureTitlebar, { once: true });
        }
        "#);
        s
    }
}
