//! 连接错误独立弹窗窗口：一个真正的 OS 顶层窗口（Tauri WebviewWindow）。
//!
//! WebUI 后端连不上时，main 窗口导航到 React 连接错误页（纯背景层），
//! 同时本模块打开独立窗口加载 `#/connection-dialog` 路由——窗口本身就是
//! WinUI 3 风格报错弹窗（错误图标 + 原因卡片 + 灰底等宽操作栏），提供
//! "退出 / 重试连接"操作。重试成功后自动导航回 WebUI 并关闭本窗口。
//!
//! 独立窗口经 capabilities/connection-dialog.json 获得最小 IPC 授权：
//! 仅允许读取上下文快照、重试连接与退出应用。
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// 独立弹窗窗口的 label（capabilities 按此授权）。
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
const CONNECTION_WINDOW_LABEL: &str = "connection";

/// 打开（或聚焦已存在的）连接错误独立窗口。
pub(crate) fn show_connection_error_dialog(app: &tauri::AppHandle) {
    // 已存在则仅前置聚焦，避免重复弹窗。
    if let Some(existing) = app.get_webview_window(CONNECTION_WINDOW_LABEL) {
        let _ = existing.set_focus();
        return;
    }
    let result = WebviewWindowBuilder::new(
        app,
        CONNECTION_WINDOW_LABEL,
        // 用 query 而非 hash 标记弹窗路由：Tauri 拼接 devUrl/dist 基址时
        // query 可靠保留（hash 实测会被丢弃导致白屏）。
        WebviewUrl::App("index.html?window=connection".into()),
    )
    .title("AzurPilot")
    .inner_size(480.0, 248.0)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    // WinUI 3 浮层形态：无系统标题栏，窗口透明 + 内容自绘 8px 圆角，
    // 标题行内自绘关闭键与拖动区（见 ConnectionWindow）。
    // 亚克力材质：Win10+ Acrylic 模糊透出背后内容（对齐系统错误弹窗观感）。
    .decorations(false)
    .transparent(true)
    .effects(tauri::utils::config::WindowEffectsConfig {
        effects: vec![tauri::utils::WindowEffect::Acrylic],
        state: None,
        radius: None,
        color: None,
    })
    .always_on_top(true)
    .skip_taskbar(true)
    .center()
    // 与 tauri.conf.json 中的窗口保持一致：禁用系统代理等参数，
    // 否则系统代理（如 Clash）会拦截 127.0.0.1 的页面请求导致白屏。
    .additional_browser_args(
        "--enable-features=msWebView2EnableDraggableRegions --disable-features=ElasticOverscroll,msWebOOUI,msPdfOOUI,msSmartScreenProtection --no-proxy-server --autoplay-policy=no-user-gesture-required",
    )
    .build();
    if let Err(error) = result {
        tracing::warn!("Failed to open connection dialog window: {error:?}");
    }
}

/// 关闭连接错误独立窗口（重试成功返回 WebUI 后调用）。
pub(crate) fn close_connection_dialog(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(CONNECTION_WINDOW_LABEL) {
        let _ = window.close();
    }
}
