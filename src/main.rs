//! AzurPilot 桌面启动器入口与通用辅助功能。
//! React 页面通过 launcher 状态控制器驱动启动、停止、修复和显式自更新；
//! windows 保留 WebUI 标题栏与文件保存注入，updater 负责校验和平台替换。

// 在 Windows 上不创建默认控制台窗口
#![windows_subsystem = "windows"]

/// 开机自启动管理。
mod autostart;
/// gui.py 后端子进程的托管（启动、就绪探测、终止等）。
mod backend;
/// 后端环境协议的新旧名称与进程归属判断。
mod backend_env;
/// Windows 后端 Job：回收工作进程，同时允许模拟器显式脱离。
#[cfg(windows)]
mod backend_process;
mod cancel;
/// 多语言（i18n）初始化与本地化支持。
mod i18n;
mod launcher;
/// 启动器反向控制流：接收后端经 SSE 下发的退出等指令。
mod launcher_control;
/// Windows 运行时 Node.js 的检测与安装引导。
mod nodejs;
/// SSE 通知流：接收后端推送的系统通知与点击回调。
mod notify;
/// 统一的启动进度状态、快照与推送节奏。
mod progress;
mod task_process;
mod updater;
mod windows;
use windows::*;
/// 首次启动环境准备（deploy.yaml、仓库克隆/更新、依赖同步）。
mod setup;
/// 窗口工具（控制台附着状态等平台辅助）。
mod window_util;

#[macro_use]
extern crate rust_i18n;
i18n!("locales", fallback = "en");

#[cfg(windows)]
use crate::setup::SplashUpdate;
use anyhow::{anyhow, Context, Result};
use base64::{prelude::BASE64_STANDARD, Engine};
use chrono::{DateTime, FixedOffset, Local, Utc};
use reqwest::{blocking::Client, header::DATE};
use rust_i18n::t;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    fs,
    path::Path,
    sync::{Arc, OnceLock},
    time::Duration,
};
#[cfg(any(windows, target_os = "macos"))]
use tauri::image::Image;
use tauri::{
    webview::{PageLoadEvent, PageLoadPayload},
    Emitter, Manager, State, Url, WebviewWindow,
};
use tauri_plugin_dialog::{DialogExt, FilePath};
#[cfg(windows)]
use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogKind};
use tracing::{error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

/// macOS 托盘图标（2x 分辨率 PNG）：Retina 屏幕菜单栏使用的高清版本。
#[cfg(target_os = "macos")]
const MENUBAR_ICON_2X: &[u8] = include_bytes!("../icons/menubar@2x.png");
/// macOS 托盘图标（1x 分辨率 PNG）：2x 图标解码失败时的降级备选。
#[cfg(target_os = "macos")]
const MENUBAR_ICON_1X: &[u8] = include_bytes!("../icons/menubar.png");
/// Windows 系统托盘图标（PNG）。使用内嵌字节而非文件路径，保证打包后
/// 即使资源缺失托盘图标也能正常加载。
#[cfg(windows)]
const WINDOWS_TRAY_ICON: &[u8] = include_bytes!("../icons/icon.png");
const BACKEND_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const BACKEND_NAVIGATION_TIMEOUT: Duration = Duration::from_secs(10);
const TIME_BOMB_CONFIG_SOURCE: &str = include_str!("../Cargo.toml");
#[cfg(test)]
const TAURI_CONFIG_SOURCE: &str = include_str!("../tauri.conf.json");
/// 预览"跳过更新检查"模式的命令行参数：横线与斜杠两种风格都接受，
/// 便于测试时模拟"无更新可用"的启动路径。
const PREVIEW_NO_UPDATE_ARGS: &[&str] = &[
    "--preview-no-update",
    "--skip-update",
    "--no-update",
    "--disable-update",
    "/preview-no-update",
    "/skip-update",
    "/no-update",
];
/// 预览"启动即报错"模式的命令行参数，用于测试 splash 错误态的展示效果。
const PREVIEW_CRASH_ARGS: &[&str] = &[
    "--preview-crash",
    "--preview-error",
    "--crash-preview",
    "--error-preview",
    "/preview-crash",
    "/preview-error",
];
/// 请求"启动后最小化到托盘"的命令行参数。
const START_MINIMIZED_ARGS: &[&str] = &["--start-minimized", "/start-minimized"];

// ---------------------------------------------------------------------------
// 启动器信任免密登录
//
// 启动器为每次会话生成一个随机信任密钥，经 backend_env 的双名称环境变量注入到
// gui.py 子进程。WebUI 启动后会据当前 --key / deploy.yaml Password 登记该
// 密钥；启动器窗口导航前先向后端换发一次性令牌，再进入 /launcher-login 页面
// 预置登录态实现免密。信任密钥与会话密钥解耦，其它浏览器仍走原密码门禁。
// 手动 gui.py 启动时密钥未注入，WebUI 端整体关闭该通道。
// ---------------------------------------------------------------------------
/// 信任密钥的原始随机字节数（base64 编码后约 32 字符）。
const TRUST_SECRET_LENGTH: usize = 24;
/// 换发免密令牌的请求超时：失败即回退普通登录页，不能拖慢主窗口展示。
const TRUST_LOGIN_TIMEOUT: Duration = Duration::from_secs(3);

/// 本次会话的信任密钥缓存：首次调用时生成并全程复用，进程结束即失效。
static LAUNCHER_TRUST_SECRET: OnceLock<String> = OnceLock::new();

/// 生成（首次）并返回本次会话的启动器信任密钥。
pub(crate) fn launcher_trust_secret() -> &'static str {
    LAUNCHER_TRUST_SECRET.get_or_init(|| {
        use rand::RngCore;
        let mut bytes = [0u8; TRUST_SECRET_LENGTH];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        BASE64_STANDARD.encode(bytes)
    })
}

/// 计算主窗口应导航到的后端地址：若后端支持启动器免密（本机回环 + 密钥匹配），
/// 则返回带一次性令牌的 /launcher-login 页面；否则回退普通后端首页，维持原有
/// 登录行为。本函数不抛错，任何失败都静默回退。
fn webui_navigate_url(port: u16) -> String {
    let fallback = || backend_url(port);
    let secret = launcher_trust_secret();
    let client = match Client::builder().timeout(TRUST_LOGIN_TIMEOUT).build() {
        Ok(client) => client,
        Err(_) => return fallback(),
    };
    let response = client
        .post(format!(
            "http://127.0.0.1:{port}/api/launcher/trusted-login"
        ))
        .header("X-Webui-Launcher-Secret", secret)
        .send();
    let response = match response {
        Ok(response) if response.status().is_success() => response,
        _ => return fallback(),
    };
    // reqwest 未启用 json feature，手动解析 body。
    let body_text = match response.text() {
        Ok(body_text) => body_text,
        Err(_) => return fallback(),
    };
    let body: serde_json::Value = match serde_json::from_str(&body_text) {
        Ok(body) => body,
        Err(_) => return fallback(),
    };
    let Some(token) = body.get("token").and_then(|token| token.as_str()) else {
        return fallback();
    };
    if token.is_empty() {
        return fallback();
    }
    // 令牌为 URL-safe 随机串（secrets.token_urlsafe），可直接置于 query。
    format!("http://127.0.0.1:{port}/launcher-login?token={token}")
}

/// URL 的日志安全形式：只保留 scheme/host/port/path，剔除 query，避免
/// /launcher-login 的一次性令牌经日志落盘。
fn redacted_url_log(url: &Url) -> String {
    let Some(host) = url.host_str() else {
        return url.to_string();
    };
    let mut out = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    out.push_str(url.path());
    out
}

/// 时间炸弹配置：到期时间、网络时间来源 URL 与过期提示文案。
///
/// 由 Cargo.toml 的 package.metadata.azurpilot.time-bomb 段解析而来；
/// 未启用时间炸弹时整体缺省（None）。
#[derive(Clone, Debug)]
struct TimeBombConfig {
    /// 到期时间：RFC 3339 解析为带固定时区的时间，网络时间不早于该值即过期。
    expires_at: DateTime<FixedOffset>,
    /// 用于获取"可信当前时间"的 URL，取其 HTTP Date 响应头判断。
    network_time_url: String,
    /// 过期后弹窗展示的提示文案。
    message: String,
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

/// 加载 Windows 托盘图标（从内嵌 PNG 字节解码）。
///
/// # Panics
/// 图标字节解码失败时 panic——没有图标时托盘无法创建。
#[cfg(windows)]
fn tray_icon_for_platform() -> Image<'static> {
    Image::from_bytes(WINDOWS_TRAY_ICON).unwrap_or_else(|err| {
        error!(?err, "Failed to load tray icon from embedded icon bytes.");
        panic!("Failed to load tray icon from embedded icon bytes: {err}");
    })
}

fn time_bomb_config() -> Result<Option<TimeBombConfig>> {
    let Some(section) = cargo_toml_section("package.metadata.azurpilot.time-bomb") else {
        return Ok(None);
    };
    // enabled 字段缺省按 false 处理：解析不到就当作未启用时间炸弹。
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
    // network-time-url 与 message 均有内置默认值，缺省也能工作。
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

/// 在内嵌 Cargo.toml 文本中定位 `[section_name]` 段，返回段体文本
/// （到下一个段头或文件末尾为止）。找不到段时返回 None。
///
/// 采用手写查找而非完整 TOML 库：仅需读取一个已知段，避免额外依赖。
fn cargo_toml_section(section_name: &str) -> Option<&'static str> {
    let header = format!("[{section_name}]");
    let start = TIME_BOMB_CONFIG_SOURCE.find(&header)? + header.len();
    let rest = &TIME_BOMB_CONFIG_SOURCE[start..];
    let end = rest.find("\n[").unwrap_or(rest.len());
    Some(&rest[..end])
}

/// 在 TOML 段文本中按行查找 `key = value`，返回去掉两侧引号后的字符串值。
///
/// 只处理单行简单键值：`#` 之后视为注释；找不到 key 时返回 None。
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

/// 判断时间炸弹是否已过期。
///
/// # Returns
/// 已过期返回 Some(配置的提示文案)；未配置或未过期返回 Ok(None)。
///
/// # Errors
/// 配置解析或网络时间获取失败时返回 Err（调用方会告警并放行启动）。
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

/// 从指定 URL 的 HTTP Date 响应头获取"可信"网络时间。
///
/// 直接读取响应头而非解析响应体，可复用任意轻量端点（如 gstatic 的
/// generate_204），无需依赖专门的授时服务。
///
/// # Errors
/// 请求失败、响应缺少 Date 头或 Date 头不是合法 RFC 2822 时间时返回 Err。
fn fetch_network_time(url: &str) -> Result<DateTime<Utc>> {
    // 5 秒超时并禁用系统代理：取的是响应头时间，必须直连目标服务器，
    // 避免代理故障拖慢启动或干扰结果。
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

/// 构造更新请求的公共 HTTP 头：伪装成 Chrome 浏览器并附带语言偏好，
/// 以通过 CDN/网关的默认访问规则。
fn launcher_arg_present(flags: &[&str]) -> bool {
    std::env::args().skip(1).any(|arg| {
        let arg = arg.to_ascii_lowercase();
        flags.iter().any(|flag| arg == *flag)
    })
}

/// 是否以"跳过更新检查"预览模式启动（用于测试无更新时的启动路径）。
fn preview_no_update_arg_present() -> bool {
    launcher_arg_present(PREVIEW_NO_UPDATE_ARGS)
}

/// 是否以"启动即报错"预览模式启动（用于测试 splash 错误态展示）。
fn preview_crash_arg_present() -> bool {
    launcher_arg_present(PREVIEW_CRASH_ARGS)
}

/// 是否以"启动后最小化到托盘"模式启动。
fn start_minimized_arg_present() -> bool {
    launcher_arg_present(START_MINIMIZED_ARGS)
}

/// 生成更新载荷的临时落地路径：系统临时目录 + 进程 ID + 原可执行文件名，
/// 避免多实例或并发更新互相覆盖。
#[cfg(windows)]
fn prompt_for_missing_nodejs(
    app_handle: &tauri::AppHandle,
    splash: &WebviewWindow,
    mut status_updater: &mut impl FnMut(SplashUpdate),
    cancel_requested: &Arc<AtomicBool>,
    start_minimized: bool,
) -> bool {
    let availability = crate::nodejs::is_nodejs_available();
    if matches!(availability, crate::nodejs::NodeJsAvailability::Ready) {
        return true;
    }
    if cancel_requested.load(Ordering::SeqCst) {
        return false;
    }

    // 两种情形文案不同：完全没装，与装了但版本低于构建要求。
    let (title, message, log) = match &availability {
        crate::nodejs::NodeJsAvailability::Outdated(found) => (
            t!("dialog.nodejs_outdated_title"),
            t!(
                "dialog.nodejs_outdated_message",
                found = found,
                minimum = crate::nodejs::NODEJS_MIN_FRONTEND_VERSION_TEXT
            ),
            "Node.js version is below the version required by the frontend build",
        ),
        _ => (
            t!("dialog.nodejs_missing_title"),
            t!("dialog.nodejs_missing_message"),
            "Node.js was not found on this Windows system",
        ),
    };
    warn!("{log}");
    // 最小化启动时先把 splash 唤起，保证弹窗有父窗口可依附。
    if start_minimized {
        let _ = reveal_window(splash);
    }
    status_updater(SplashUpdate::loading(
        t!("setup.checking_nodejs"),
        t!("setup.checking_nodejs"),
        5,
    ));

    // 阻塞式弹窗：用户选择"立即安装"才进入安装流程，"暂不"则继续启动。
    let install_requested = app_handle
        .dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            t!("dialog.nodejs_install").to_string(),
            t!("dialog.nodejs_not_now").to_string(),
        ))
        .parent(splash)
        .blocking_show();
    if !install_requested {
        info!("Node.js installation was declined by the user");
        return true;
    }

    match crate::nodejs::install_nodejs(&availability, cancel_requested, &mut status_updater) {
        Ok(()) => {
            info!("Node.js installation completed successfully");
            true
        }
        Err(_) if cancel_requested.load(Ordering::SeqCst) => false,
        Err(error) => {
            error!("Node.js installation failed: {error:#}");
            // 安装失败：splash 进入错误态并弹窗提示，但流程仍继续——
            // 后续步骤失败时会给出更具体的报错。
            status_updater(SplashUpdate::error(
                t!("dialog.nodejs_install_failed"),
                t!(
                    "dialog.nodejs_install_failed_detail",
                    error = format!("{error:#}")
                ),
                7,
            ));
            app_handle
                .dialog()
                .message(t!(
                    "dialog.nodejs_install_failed_detail",
                    error = format!("{error:#}")
                ))
                .title(t!("dialog.nodejs_install_failed"))
                .kind(MessageDialogKind::Error)
                .parent(splash)
                .blocking_show();
            true
        }
    }
}

fn main() -> Result<()> {
    launcher::run()
}

fn initialize_logging() -> Result<WorkerGuard> {
    let log_dir = Path::new("log");
    let log_filename = today_launcher_log_filename();
    truncate_log_file(log_dir, &log_filename)?;
    // rolling::never 不做按时间/大小轮转，固定写当日文件（启动时已先截断）。
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

/// 确保日志目录存在并把日志文件截断为空（File::create 语义）。
/// 每次启动重新记录，避免单文件无限增长。
///
/// # Errors
/// 目录创建或文件创建失败时返回 Err。
fn truncate_log_file(log_dir: &Path, filename: &str) -> Result<()> {
    fs::create_dir_all(log_dir)?;
    let path = log_dir.join(filename);
    fs::File::create(&path)
        .with_context(|| format!("truncate launcher log file {}", path.display()))?;
    Ok(())
}

/// Tauri 命令：由注入的 window.saveAs 调用（见 page_load_injector）。
///
/// 前端传入文件名与 base64 编码的文件内容，此处解码后弹出系统"另存为"
/// 对话框并写盘。无返回值，失败仅记录日志。
#[tauri::command]
fn save_as(app_handle: tauri::AppHandle, filename: &str, data: &str) {
    match BASE64_STANDARD.decode(data) {
        // 解码成功：弹出系统"另存为"对话框；写盘在回调中完成，失败只记日志。
        Ok(decoded_data) => app_handle
            .dialog()
            .file()
            .set_file_name(filename)
            .save_file(move |path| {
                let result: Result<()> = (move || {
                    let file_path = path.as_ref().and_then(FilePath::as_path).ok_or_else(|| {
                        anyhow!(t!(
                            "errors.invalid_file_path",
                            path = format!("{:?}", &path)
                        ))
                    })?;
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

/// Tauri 命令：把今天的 GUI 日志（log/{日期}_gui.txt）另存到用户选择的位置。
/// 返回被保存的文件名；读取失败时返回错误字符串。
#[tauri::command]
fn download_today_gui_log(app_handle: tauri::AppHandle) -> std::result::Result<String, String> {
    download_log_file(app_handle, today_gui_log_filename(), "GUI")
}

/// Tauri 命令：把今天的启动器日志（log/{日期}_launcher.txt）另存到用户
/// 选择的位置。返回被保存的文件名；读取失败时返回错误字符串。
#[tauri::command]
fn download_today_launcher_log(
    app_handle: tauri::AppHandle,
) -> std::result::Result<String, String> {
    download_log_file(app_handle, today_launcher_log_filename(), "launcher")
}

/// 读取指定日志文件并弹出另存为对话框。
///
/// # Errors
/// 以 Err(String) 返回（前端直接展示）：获取工作目录或读取日志文件失败时
/// 携带本地化错误文案；保存对话框是异步的，写盘失败只记日志。
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
                let file_path = path.as_ref().and_then(FilePath::as_path).ok_or_else(|| {
                    anyhow!(t!(
                        "errors.invalid_file_path",
                        path = format!("{:?}", &path)
                    ))
                })?;
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

/// 生成今天的 GUI 日志文件名（{日期}_gui.txt，日期取本地时区）。
fn today_gui_log_filename() -> String {
    format!("{}_gui.txt", Local::now().format("%Y-%m-%d"))
}

/// log 归档进度事件载荷：done/total 为已写入与总计字节数。
#[derive(Clone, serde::Serialize)]
struct LogArchiveProgress {
    done: u64,
    total: u64,
}

/// Tauri 命令：弹出 zip 另存为对话框（带过滤器与预填文件名），
/// 返回用户选择的目标路径；取消时返回 None。
///
/// # Errors
/// 对话框通道关闭时返回错误字符串。
#[tauri::command]
async fn prompt_log_archive_path(
    app_handle: tauri::AppHandle,
) -> std::result::Result<Option<String>, String> {
    let archive_name = format!("AzurPilot-log-{}.zip", Local::now().format("%Y-%m-%d"));
    let (sender, receiver) = std::sync::mpsc::channel();
    app_handle
        .dialog()
        .file()
        .add_filter("ZIP", &["zip"])
        .set_file_name(&archive_name)
        .save_file(move |path| {
            let _ = sender.send(path.map(|p| p.to_string()));
        });
    tauri::async_runtime::spawn_blocking(move || receiver.recv())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Tauri 命令：把 log 目录顶层文件打包为 zip 写入 target 路径。
/// 打包在后台线程执行，逐文件向 launcher 窗口发进度事件
/// （launcher://log-archive-progress，done/total 字节）。返回文件数。
///
/// # Errors
/// 日志目录缺失、目录扫描或 zip 写入失败时返回本地化错误字符串。
#[tauri::command]
async fn download_log_archive(
    app_handle: tauri::AppHandle,
    target: String,
) -> std::result::Result<usize, String> {
    let log_dir = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("log");
    if !log_dir.is_dir() {
        return Err(t!(
            "errors.log_dir_missing",
            path = log_dir.to_string_lossy().to_string()
        )
        .to_string());
    }
    let mut files: Vec<(std::path::PathBuf, String, u64)> = Vec::new();
    for entry in std::fs::read_dir(&log_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let size = entry.metadata().map_err(|e| e.to_string())?.len();
        files.push((path, entry.file_name().to_string_lossy().to_string(), size));
    }
    if files.is_empty() {
        return Err(t!(
            "errors.log_dir_missing",
            path = log_dir.to_string_lossy().to_string()
        )
        .to_string());
    }
    let total: u64 = files.iter().map(|(_, _, size)| *size).sum();
    let emitter = app_handle;
    tauri::async_runtime::spawn_blocking(move || {
        let file = fs::File::create(&target).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut done: u64 = 0;
        for (path, name, size) in &files {
            zip.start_file(name.clone(), options.clone())
                .map_err(|e| e.to_string())?;
            let mut source = fs::File::open(path).map_err(|e| e.to_string())?;
            std::io::copy(&mut source, &mut zip).map_err(|e| e.to_string())?;
            done += size;
            let _ = emitter.emit_to(
                "launcher",
                "launcher://log-archive-progress",
                LogArchiveProgress { done, total },
            );
        }
        zip.finish().map_err(|e| e.to_string())?;
        Ok(files.len())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 生成今天的启动器日志文件名（{日期}_launcher.txt，日期取本地时区）。
fn today_launcher_log_filename() -> String {
    format!("{}_launcher.txt", Local::now().format("%Y-%m-%d"))
}

/// Tauri 命令：隐藏主窗口并最小化到托盘（自定义标题栏的"托盘化"按钮与
/// 关闭选择菜单的"最小化到托盘"选项均调用此命令）。
#[tauri::command]
fn window_hide(app_handle: tauri::AppHandle) -> tauri::Result<()> {
    minimize_main_window_to_tray(&app_handle);
    Ok(())
}

/// Tauri 命令：最小化窗口到任务栏（splash 与标题栏的最小化按钮调用）。
#[tauri::command]
fn window_minimize(window: WebviewWindow) -> tauri::Result<()> {
    window.minimize()
}

/// Tauri 命令：切换窗口最大化/还原状态。
/// 返回切换后是否处于最大化，供标题栏按钮同步图标。
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

/// Tauri 命令：关闭窗口（splash 的关闭按钮调用；主窗口的关闭行为由
/// RunEvent::WindowEvent 的 CloseRequested 分支按平台处理）。
#[tauri::command]
fn window_close(window: WebviewWindow) -> tauri::Result<()> {
    window.close()
}

/// Tauri 命令：用户在关闭选择菜单确认退出时调用——置位放行标志并以
/// 退出码 0 结束应用，使 ExitRequested 分支放行真正的关停（含终止后端）。
#[tauri::command]
fn window_exit_application(app_handle: tauri::AppHandle) -> Result<(), String> {
    app_handle
        .state::<Arc<launcher::Launcher>>()
        .exit()
        .map_err(|e| e.to_string())
}

/// Tauri 命令：开始拖动窗口（标题栏拖拽区与 splash 拖拽区调用）。
#[tauri::command]
fn window_start_dragging(window: WebviewWindow) -> tauri::Result<()> {
    window.start_dragging()
}

/// Tauri 命令：查询窗口当前是否处于最大化（标题栏按钮同步图标用）。
#[tauri::command]
fn window_is_maximized(window: WebviewWindow) -> tauri::Result<bool> {
    window.is_maximized()
}

/// Tauri 命令：错误页面的"重试连接"按钮（含每秒自动重试）调用。
///
/// 在阻塞线程中等待后端端口恢复可达，随后换发免密令牌并导航过去；
/// 超时未恢复返回 Ok(false)（前端提示"仍然失败"），恢复并成功导航返回
/// Ok(true)。
///
/// # Errors
/// 以 Err(String) 返回：阻塞任务失败、URL 解析或导航失败时携带错误信息。
#[tauri::command]
async fn retry_backend_connection(
    window: WebviewWindow,
    controller: State<'_, Arc<launcher::Launcher>>,
) -> std::result::Result<bool, String> {
    if window.label() != "main" || !launcher::is_local_page(window.app_handle(), &window) {
        return Err(t!("launcher.forbidden").to_string());
    }
    let port = controller.port();
    let task = controller.snapshot().task_id;
    // 等待与换发免密令牌均含阻塞调用，统一放在阻塞线程中执行。
    let target_url = tauri::async_runtime::spawn_blocking(move || {
        if wait_for_backend_connection(port, BACKEND_NAVIGATION_TIMEOUT).is_err() {
            return None;
        }
        Some(webui_navigate_url(port))
    })
    .await
    .map_err(|e| {
        error!("Backend retry task failed: {e:?}");
        e.to_string()
    })?;

    let Some(target_url) = target_url else {
        return Ok(false);
    };

    if controller.snapshot().task_id != task || !controller.snapshot().backend_running {
        return Ok(false);
    }
    let url = Url::parse(&target_url).map_err(|e| e.to_string())?;
    window.navigate(url).map_err(|e| {
        error!("Failed to navigate to reconnected backend: {e:?}");
        e.to_string()
    })?;
    Ok(true)
}

/// 页面加载完成回调：向主窗口页面注入启动器辅助 JS。
///
/// 注入内容（一个幂等的 IIFE）：阻止浏览器历史后退；把 window.saveAs
/// 覆盖为"经 FileReader 转 base64 后调用 Tauri save_as 命令"的版本；并
/// 拼入自定义标题栏脚本（macOS 除外，见 main_window_titlebar_injection_script）。
fn page_load_injector(webview: WebviewWindow, payload: PageLoadPayload<'_>) {
    // 只在加载完成（Finished）时注入：开始加载阶段文档尚未就绪，注入无效。
    if payload.event() == PageLoadEvent::Finished {
        info!(
            "Injecting saveFile function to loaded page: {}",
            redacted_url_log(payload.url())
        );
        let injected_js = r#"
if (!window.azurpilot_launcher_injected) {
    window.azurpilot_launcher_injected = true;
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
__AZURPILOT_TITLEBAR_SCRIPT__
    })();
}
"#
        .replace(
            "__AZURPILOT_TITLEBAR_SCRIPT__",
            &main_window_titlebar_injection_script(),
        );
        // 注入失败不影响页面本身功能，仅记录日志。
        if let Err(e) = webview.eval(&injected_js) {
            error!("Failed to inject JS to webview: {:?}", e);
        }
    }
}

fn backend_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}
