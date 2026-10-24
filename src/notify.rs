//! 桌面通知模块：订阅后端的 SSE 通知流，并把事件转发为平台原生通知。
//! `start_notify_stream` 在独立线程中维持对 `http://127.0.0.1:{port}/api/notify_stream`
//! 的长连接，断线后固定间隔 3 秒重连，直到 `allow_exit` 置位；分发层按平台
//! 选择通知后端——Windows 用 tauri-winrt-notification（Toast），Linux 用
//! notify-rust，macOS 等其余平台用 tauri-plugin-notification；由 main.rs 在后端就绪后启用。

use std::{
    io::{BufRead, BufReader},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

#[cfg(windows)]
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
#[cfg(target_os = "linux")]
use notify_rust::{Hint, Notification};
use reqwest::blocking::Client;
use reqwest::header::ACCEPT;
use rust_i18n::t;
use serde::Deserialize;
#[cfg(all(not(windows), not(target_os = "linux")))]
use tauri_plugin_notification::NotificationExt;
use tracing::{debug, info, warn};

/// 通知被点击时执行的回调。
///
/// 启动器用它把通知点击统一汇聚到"唤起主窗口"等逻辑（由 main.rs 提供），
/// 以 `Arc` 包装便于克隆给各平台的通知实现与后台线程。
pub type NotificationClickHandler = Arc<dyn Fn() + Send + Sync + 'static>;

/// Windows Toast"一般"通知的 AUMID（AppUserModelId）。
///
/// Windows 按不同 AUMID 在操作中心分组并展示不同的应用显示名称，
/// 因此三类通知各用一个 ID，让用户能一眼区分通知来源。
#[cfg(windows)]
const WINDOWS_APP_ID: &str = "moe.taiho.azurpilot.notification";

/// Windows Toast"更新"通知的 AUMID，更新类事件单独分组以免被普通通知淹没。
#[cfg(windows)]
const WINDOWS_APP_ID_UPDATE: &str = "moe.taiho.azurpilot.notification.update";

/// Windows Toast"公告"通知的 AUMID，运营公告与功能更新同样分开分组。
#[cfg(windows)]
const WINDOWS_APP_ID_ANNOUNCEMENT: &str = "moe.taiho.azurpilot.notification.announcement";

/// "一般"通知在操作中心里显示的应用名（随界面语言本地化）。
fn windows_app_name() -> String {
    t!("notify.info_app_name").to_string()
}

/// "更新"通知在操作中心里显示的应用名。
fn windows_app_name_update() -> String {
    t!("notify.update_app_name").to_string()
}

/// "公告"通知在操作中心里显示的应用名。
fn windows_app_name_announcement() -> String {
    t!("notify.announcement_app_name").to_string()
}

/// 编译期嵌入的应用图标字节，供 Windows Toast 图标落盘使用。
///
/// Toast 的 `IconUri` 必须指向磁盘上的真实文件，无法直接内嵌内存数据，
/// 因此携带一份图标内容，运行时写入本地数据目录（见
/// [`ensure_windows_notification_icon`]）。
#[cfg(windows)]
const WINDOWS_NOTIFICATION_ICON: &[u8] = include_bytes!("../icons/icon.png");

/// 后端 SSE 事件反序列化后的通知载荷。
///
/// 所有字段均可缺失，缺失时展示层逐级回退到本地化默认文案。
#[derive(Debug, Deserialize)]
struct NotifyPayload {
    /// 产生通知的 ALAS 实例名，正文缺失时用于拼出兜底文案。
    instance: Option<String>,
    /// 通知标题，空白时回退到默认标题。
    title: Option<String>,
    /// 通知正文，空白时回退到实例名或默认正文。
    content: Option<String>,
    /// 事件类别标记（后端历史拼写，实为 update）；取值决定通知归类为
    /// 更新、公告或普通通知，见 [`show_notification`]。
    updata: Option<serde_json::Value>,
}

/// 通知类别，决定 Windows 上使用的 AUMID/显示名以及标题覆盖策略。
#[derive(Debug, Clone, Copy)]
enum NotificationType {
    /// 普通运行通知，标题沿用载荷内容。
    Normal,
    /// 版本更新类通知，标题固定为本地化的"更新"文案。
    Update,
    /// 运营公告类通知，标题固定为本地化的"公告"文案。
    Announcement,
}

/// 启动后台线程订阅通知流，直到 `allow_exit` 置位才停止重连。
///
/// 该线程与主线程完全解耦：后端重启、依赖同步期间通知流会反复断开，
/// 均在此线程内静默重连，不影响启动器其他功能。
pub fn start_notify_stream(
    app: tauri::AppHandle,
    port: u16,
    allow_exit: Arc<AtomicBool>,
    on_click: NotificationClickHandler,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let url = format!("http://127.0.0.1:{port}/api/notify_stream");
        // 连接超时 5 秒并绕过系统代理：目标是本机回环地址，走代理只会添乱。
        let client = match Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(None)
            .no_proxy()
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                warn!("Unable to create notify stream client: {e}");
                return;
            }
        };

        // 断线固定等 3 秒再重连：实现简单且足以应对后端重启窗口；
        // 退出标志置位后立即跳出，不再做无谓的睡眠与连接。
        while !allow_exit.load(Ordering::SeqCst) {
            info!("Connecting to notify stream: {url}");
            match read_notify_stream(&client, &url, &app, &allow_exit, &on_click) {
                Ok(()) => debug!("Notify stream ended"),
                Err(e) => warn!("Notify stream disconnected: {e}"),
            }

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

/// 建立一次 SSE 连接并阻塞读取，直到对端关闭或收到退出信号。
///
/// 按 SSE 协议逐行解析：`data:` 前缀行为事件数据，空行为事件分隔符；
/// 每凑齐一条完整事件就立即分发为桌面通知。
///
/// # Errors
///
/// 连接建立失败、服务端返回非 2xx 状态，或读取流时发生 IO 错误
/// （含退出时主动断开导致的错误）时返回 `Err`，由外层统一重连。
fn read_notify_stream(
    client: &Client,
    url: &str,
    app: &tauri::AppHandle,
    allow_exit: &Arc<AtomicBool>,
    on_click: &NotificationClickHandler,
) -> Result<()> {
    // SSE 协议要求客户端声明 Accept: text/event-stream，后端据此返回流式响应。
    let response = crate::cancel::request(
        client.get(url).header(ACCEPT, "text/event-stream"),
        allow_exit,
    )?;

    if !response.status().is_success() {
        return Err(anyhow!("server returned {}", response.status()));
    }

    // SSE 以空行作为一条事件（message）的边界，因此逐行缓冲读取。
    let mut reader = BufReader::new(crate::cancel::body(response, allow_exit.clone()));
    let mut data_lines = Vec::new();

    while !allow_exit.load(Ordering::SeqCst) {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        // read_line 返回 0 字节表示对端已关闭连接，结束本次读取交由外层重连。
        if bytes == 0 {
            break;
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            // 空行 = 事件结束：把积累的 data 行拼成分发一条通知。
            if !allow_exit.load(Ordering::SeqCst) {
                dispatch_sse_data(&mut data_lines, app, on_click);
            }
            continue;
        }

        // 同一事件可拆成多个 data: 行，先累积，遇到空行时再统一处理。
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_owned());
        }
    }

    // 退出前把未跟随空行的残余数据也分发一次，避免丢失最后一条通知。
    if !allow_exit.load(Ordering::SeqCst) {
        dispatch_sse_data(&mut data_lines, app, on_click);
    }
    Ok(())
}

/// 把一条 SSE 事件的 `data` 行集合还原为 JSON 并展示为桌面通知。
///
/// SSE 规范允许事件数据拆分为多个 `data:` 行，还原时需以换行拼接；
/// 空 `data_lines`（如心跳注释产生的空行）直接忽略。
fn dispatch_sse_data(
    data_lines: &mut Vec<String>,
    app: &tauri::AppHandle,
    on_click: &NotificationClickHandler,
) {
    if data_lines.is_empty() {
        return;
    }

    let data = data_lines.join("\n");
    data_lines.clear();

    // 载荷非法（JSON 损坏或字段类型不符）时仅告警丢弃，不让单个坏事件
    // 拖垮整条通知流。
    match serde_json::from_str::<NotifyPayload>(&data) {
        Ok(payload) => show_notification(app, payload, on_click),
        Err(e) => warn!("Ignoring invalid notify payload: {e}; payload={data}"),
    }
}

/// 把一条通知载荷渲染为标题与正文，并按平台分发给对应的通知后端。
///
/// `updata` 字段取值决定归类：真值（含历史错拼 `"ture"`）归为更新通知、
/// 假值（含历史错拼 `"fl"`）归为公告通知，其余按普通通知处理；兼容错拼
/// 是为了照顾尚未升级的旧版后端。标题或正文缺失时逐级回退本地化默认文案。
fn show_notification(
    app: &tauri::AppHandle,
    payload: NotifyPayload,
    on_click: &NotificationClickHandler,
) {
    let NotifyPayload {
        instance,
        title,
        content,
        updata,
    } = payload;

    // updata 取值同时兼容布尔与字符串两种 JSON 形态；"ture"/"fl" 是后端
    // 历史版本发出过的错拼值，为保持向后兼容而一并识别。
    let (title, notify_type) = if let Some(v) = updata {
        if v.as_bool() == Some(true) || v.as_str() == Some("true") || v.as_str() == Some("ture") {
            (
                t!("notify.update_title").to_string(),
                NotificationType::Update,
            )
        } else if v.as_bool() == Some(false)
            || v.as_str() == Some("false")
            || v.as_str() == Some("fl")
        {
            (
                t!("notify.announcement_title").to_string(),
                NotificationType::Announcement,
            )
        } else {
            // 归类不明的取值按普通通知处理，标题沿用载荷内容。
            (
                clean_text(title).unwrap_or_else(|| t!("notify.default_title").to_string()),
                NotificationType::Normal,
            )
        }
    } else {
        (
            clean_text(title).unwrap_or_else(|| t!("notify.default_title").to_string()),
            NotificationType::Normal,
        )
    };
    // 正文回退链：正文内容 -> 实例名 -> 本地化默认文案。
    let body = clean_text(content)
        .or_else(|| clean_text(instance).map(|instance| format!("Instance: {instance}")))
        .unwrap_or_else(|| t!("notify.default_body").to_string());

    // Windows 走独立路径：Toast 支持按通知类型切换 AUMID，且能挂点击回调。
    #[cfg(windows)]
    {
        if let Err(e) = show_windows_notification(&title, &body, notify_type, on_click.clone()) {
            warn!("Failed to show Windows notification: {e}");
        }
        // 该平台不使用 Tauri 通知插件，显式消费参数避免未使用告警。
        let _ = app;
    }

    // Linux 走 freedesktop 通知规范（notify-rust），通过 "default" 动作实现点击回调。
    #[cfg(target_os = "linux")]
    {
        if let Err(e) = show_linux_notification(&title, &body, on_click.clone()) {
            warn!("Failed to show Linux notification: {e}");
        }
        // 同上，此平台不消费 Tauri 通知插件。
        let _ = app;
    }

    // macOS 等其余平台使用 Tauri 官方通知插件；该插件不暴露点击回调，
    // 故忽略 on_click（点击行为由系统按应用图标处理）。
    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        if let Err(e) = app.notification().builder().title(title).body(body).show() {
            warn!("Failed to show system notification: {e}");
        }
        let _ = on_click;
    }
}

/// 在 Windows 上弹出一条 Toast 通知。
///
/// 按通知类型选择 AUMID 与显示名，使操作中心内三类通知各自分组；
/// 点击 Toast 会触发启动器统一的 `on_click` 回调。
///
/// # Errors
///
/// AUMID 注册表写入失败、图标落盘失败或 Toast 展示失败（例如系统
/// 通知功能被禁用）时返回错误。
#[cfg(windows)]
fn show_windows_notification(
    title: &str,
    body: &str,
    notify_type: NotificationType,
    on_click: NotificationClickHandler,
) -> Result<()> {
    // 三类通知使用不同 AUMID：操作中心据此分组，并显示各自的应用名。
    let (app_id, app_name) = match notify_type {
        NotificationType::Normal => (WINDOWS_APP_ID, windows_app_name()),
        NotificationType::Update => (WINDOWS_APP_ID_UPDATE, windows_app_name_update()),
        NotificationType::Announcement => {
            (WINDOWS_APP_ID_ANNOUNCEMENT, windows_app_name_announcement())
        }
    };

    // Toast 的 IconUri 要求磁盘路径且以 URI 形式书写，因此先把嵌入图标
    // 落盘，再把路径分隔符统一为正斜杠，避免反斜杠带来的转义歧义。
    let icon_path = ensure_windows_app_user_model_id(app_id, &app_name)?;
    let icon_uri_path = icon_path.to_string_lossy().replace('\\', "/");
    tauri_winrt_notification::Toast::new(app_id)
        .icon(
            Path::new(&icon_uri_path),
            tauri_winrt_notification::IconCrop::Square,
            &app_name,
        )
        .title(title)
        .text1(body)
        // Short：通知短暂显示后自动收进操作中心，减少打扰。
        .duration(tauri_winrt_notification::Duration::Short)
        // 用户点击 Toast 时触发，转发给启动器统一的点击处理。
        .on_activated(move |_| {
            on_click();
            Ok(())
        })
        .show()
        .map_err(|e| anyhow!("{e:?}"))
}

/// 确保指定 AUMID 已注册到当前用户注册表，并返回可用的图标路径。
///
/// Toast 通知要求 AUMID 预先注册（`HKCU\SOFTWARE\Classes\AppUserModelId`），
/// 否则系统无法在通知中显示应用名称与图标。写入幂等：重复调用只做覆盖。
///
/// # Errors
///
/// 注册表创建/写入失败，或通知图标落盘失败时返回错误。
#[cfg(windows)]
fn ensure_windows_app_user_model_id(id: &str, name: &str) -> Result<PathBuf> {
    // 图标路径同时用于注册表 IconUri 与 Toast 的 icon 参数，先准备妥当。
    let icon_path = ensure_windows_notification_icon()?;
    let key = windows_registry::CURRENT_USER
        .create(format!(r"SOFTWARE\Classes\AppUserModelId\{id}"))
        .map_err(|e| anyhow!("{e:?}"))?;

    // DisplayName、IconBackgroundColor 与 IconUri 共同决定操作中心里的
    // 显示名称与圆形图标外观。
    key.set_string("DisplayName", name)
        .map_err(|e| anyhow!("{e:?}"))?;
    key.set_string("IconBackgroundColor", "0")
        .map_err(|e| anyhow!("{e:?}"))?;
    key.set_hstring("IconUri", &icon_path.as_path().into())
        .map_err(|e| anyhow!("{e:?}"))?;
    Ok(icon_path)
}

/// 把编译期嵌入的通知图标落盘到本地数据目录，返回其路径。
///
/// Toast 的 IconUri 只认磁盘文件，因此需要持久化一份图标；当嵌入图标
/// 内容发生变化（随升级更换）时覆盖重写，保证注册表里的路径始终有效。
///
/// # Errors
///
/// 本地数据目录不存在且无法创建，或图标读写失败时返回错误。
#[cfg(windows)]
fn ensure_windows_notification_icon() -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| anyhow!(t!("errors.appdata_not_found")))?
        .join("AzurPilotLauncher");
    fs::create_dir_all(&data_dir)?;

    // 已存在的图标内容与嵌入版本一致时跳过写入，减少磁盘 IO。
    let icon_path = data_dir.join("notification-icon.png");
    let should_write = fs::read(&icon_path)
        .map(|current| current != WINDOWS_NOTIFICATION_ICON)
        .unwrap_or(true);
    if should_write {
        fs::write(&icon_path, WINDOWS_NOTIFICATION_ICON)?;
    }

    Ok(icon_path)
}

/// 在 Linux 上通过 freedesktop 通知规范弹出通知。
///
/// 使用 notify-rust 直接对接通知守护进程；"default" 动作即用户点击
/// 通知本体，触发后转发给启动器统一的点击回调。
///
/// # Errors
///
/// 通知守护进程不可用或展示失败时返回错误。
#[cfg(target_os = "linux")]
fn show_linux_notification(
    title: &str,
    body: &str,
    on_click: NotificationClickHandler,
) -> Result<()> {
    let mut notification = Notification::new();
    notification
        .summary(title)
        .body(body)
        // auto_icon 让通知守护进程按应用自动匹配图标，无需自带图标文件。
        .auto_icon()
        .action("default", &t!("notify.open").to_string())
        // Resident 提示通知常驻、不因超时自动消失，确保重要事件被看到。
        .hint(Hint::Resident(true));
    let handle = notification.show()?;

    // wait_for_action 会阻塞等待用户交互，必须放入独立线程，否则会卡住
    // SSE 读取线程进而阻断后续通知。
    thread::spawn(move || {
        handle.wait_for_action(move |action| {
            if action == "default" {
                on_click();
            }
        });
    });

    Ok(())
}

/// 规整可选文本：去除首尾空白并把空串归一为 `None`，统一"缺失"语义。
fn clean_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::NotifyPayload;

    /// 后端通知载荷应能正常反序列化，且各字段保持原样。
    #[test]
    fn parses_notify_payload() {
        let payload: NotifyPayload = serde_json::from_str(
            r#"{"instance":"alas","title":"AzurPilot <alas> 警告","content":"<alas> 游戏卡住"}"#,
        )
        .unwrap();

        assert_eq!(payload.instance.as_deref(), Some("alas"));
        assert_eq!(payload.title.as_deref(), Some("AzurPilot <alas> 警告"));
        assert_eq!(payload.content.as_deref(), Some("<alas> 游戏卡住"));
    }
}
