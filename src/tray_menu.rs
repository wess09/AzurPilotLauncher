//! 托盘右键自绘弹窗菜单：贴着托盘图标弹出的无边框透明小窗口。
//!
//! Windows 的托盘右键菜单是 Win32 原生 HMENU，由系统绘制、无法自定义
//! 样式（Chrome/QQ NT 均以自绘窗口绕开）。因此 Windows 不再给托盘挂
//! 原生菜单：右键直接显示本窗口，加载 React 的 `?view=tray` 路由渲染
//! Fluent 风格菜单；macOS 原生 NSMenu 外观即系统样式，继续用原生菜单。
//!
//! 窗口随启动器预建并隐藏，点击托盘时按 DPI 换算、贴着指针上方定位；
//! 失焦即收起（见 launcher.rs 的 RunEvent::WindowEvent），短时间内再次
//! 右键视为“再点一次收起”。IPC 经 capabilities/tray-menu.json 最小授权，
//! 命令内还保留调用方标签的防御性校验。
use crate::windows::{hide_window, show_launcher_home, toggle_main_window_visibility};
use rust_i18n::t;
use std::sync::Arc;
use tauri::{Emitter, Manager, WebviewWindow};
use tracing::warn;
#[cfg(windows)]
use {
    anyhow::Context as _,
    std::{
        sync::Mutex,
        time::{Duration, Instant},
    },
    tauri::{WebviewUrl, WebviewWindowBuilder},
};

/// 托盘弹窗菜单窗口的 label（capabilities 与前端路由均按此识别）。
pub(crate) const TRAY_MENU_LABEL: &str = "tray_menu";

/// 弹窗逻辑尺寸（CSS 像素），需与 frontend 的 trayMenu.module.css 一致：
/// 外圈 8px 留给 CSS 阴影，菜单本体 208×125（3 项 + 分隔线 + 内边距）。
#[cfg(windows)]
const TRAY_MENU_LOGICAL_SIZE: (f64, f64) = (224.0, 141.0);

/// 两次右键间隔小于该时长且弹窗可见时，视为“再点一次收起”。
#[cfg(windows)]
const RECLICK_CLOSE: Duration = Duration::from_millis(450);

/// 托盘菜单动作：原生菜单（macOS）与 Windows 自绘弹窗共用同一套逻辑。
pub(crate) fn handle_tray_menu_action(app: &tauri::AppHandle, id: &str) {
    let controller = app.state::<Arc<crate::launcher::Launcher>>();
    match id {
        "home" => {
            let _ = show_launcher_home(app);
        }
        "toggle" => {
            if controller.snapshot().backend_running {
                toggle_main_window_visibility(app, controller.port(), controller.recreate());
            } else {
                let _ = show_launcher_home(app);
            }
        }
        "quit" => {
            if controller.snapshot().can_cancel {
                let _ = show_launcher_home(app);
                let _ = app.emit_to("launcher", "launcher://close-requested", ());
            } else {
                let _ = controller.exit();
            }
        }
        _ => {}
    }
}

/// 隐藏托盘弹窗菜单窗口（失焦、Esc、菜单动作后调用）。
pub(crate) fn hide_tray_menu_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(TRAY_MENU_LABEL) {
        let _ = hide_window(&window);
    }
}

/// 托盘弹窗菜单动作（?view=tray 前端点击菜单项时调用）。
#[tauri::command]
pub(crate) fn tray_menu_action(window: WebviewWindow, id: String) -> Result<(), String> {
    authorize_tray_menu(&window)?;
    handle_tray_menu_action(window.app_handle(), &id);
    // 动作可能前置主窗口或退出应用；无论哪种都先收起弹窗。
    hide_tray_menu_window(window.app_handle());
    Ok(())
}

/// 收起托盘弹窗菜单（前端按 Esc 时调用）。
#[tauri::command]
pub(crate) fn tray_menu_hide(window: WebviewWindow) -> Result<(), String> {
    authorize_tray_menu(&window)?;
    hide_tray_menu_window(window.app_handle());
    Ok(())
}

/// 弹窗命令的防御性校验：ACL 已限定 tray_menu 窗口，这里再查一次标签。
fn authorize_tray_menu(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != TRAY_MENU_LABEL {
        warn!(
            "Rejected IPC from window '{}': tray_menu only",
            window.label()
        );
        return Err(t!("launcher.forbidden").to_string());
    }
    Ok(())
}

/// 记录上次弹出时间，实现“再次右键收起”。
#[cfg(windows)]
#[derive(Default)]
pub(crate) struct TrayMenuState {
    last_shown: Mutex<Option<Instant>>,
}

/// 取回（或创建）隐藏的托盘弹窗窗口。
#[cfg(windows)]
pub(crate) fn ensure_tray_menu_window(app: &tauri::AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(TRAY_MENU_LABEL) {
        return Ok(window);
    }
    let (width, height) = TRAY_MENU_LOGICAL_SIZE;
    let builder = WebviewWindowBuilder::new(
        app,
        TRAY_MENU_LABEL,
        // 用 query 而非 hash 标记弹窗路由：Tauri 拼接 devUrl/dist 基址时
        // query 可靠保留（hash 实测会被丢弃导致白屏），与连接错误弹窗一致。
        WebviewUrl::App("index.html?view=tray".into()),
    )
    .title("AzurPilot")
    .inner_size(width, height)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .focused(false)
    // 与 tauri.conf.json / native_dialog.rs 保持一致：禁用系统代理等
    // 参数，避免系统代理拦截本地页面导致白屏。
    .additional_browser_args(
        "--enable-features=msWebView2EnableDraggableRegions --disable-features=ElasticOverscroll,msWebOOUI,msPdfOOUI,msSmartScreenProtection --no-proxy-server --autoplay-policy=no-user-gesture-required",
    )
    // 透明背景关掉系统阴影，圆角与投影交给 CSS 自绘。
    .transparent(true)
    .shadow(false);
    builder.build()
}

/// 在托盘图标旁显示（或收起）自绘菜单弹窗。
#[cfg(windows)]
pub(crate) fn show_tray_menu_popup(
    app: &tauri::AppHandle,
    cursor: tauri::PhysicalPosition<f64>,
) -> anyhow::Result<()> {
    let window = ensure_tray_menu_window(app)?;
    // 短时间内再次右键：收起已显示的弹窗。
    let state = app.state::<TrayMenuState>();
    {
        let mut last = state.last_shown.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(shown) = *last {
            if shown.elapsed() < RECLICK_CLOSE && window.is_visible().unwrap_or(false) {
                hide_window(&window)?;
                *last = None;
                return Ok(());
            }
        }
        *last = Some(Instant::now());
    }
    // 按指针所在显示器换算物理尺寸，并夹在显示器范围内；任务栏通常在
    // 底部，弹窗默认出现在指针上方，顶部任务栏则改为下方。
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)?
        .or_else(|| app.primary_monitor().ok().flatten());
    let scale = monitor
        .as_ref()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0);
    let (logical_width, logical_height) = TRAY_MENU_LOGICAL_SIZE;
    let width = (logical_width * scale) as i32;
    let height = (logical_height * scale) as i32;
    let mut x = cursor.x as i32 - width / 2;
    let mut y = cursor.y as i32 - height - 8;
    if let Some(monitor) = &monitor {
        let (left, top) = (monitor.position().x, monitor.position().y);
        let right = left + monitor.size().width as i32;
        let bottom = top + monitor.size().height as i32;
        x = x.clamp(left + 8, (right - width - 8).max(left + 8));
        if y < top + 8 {
            y = cursor.y as i32 + 8;
        }
        y = y.clamp(top + 8, (bottom - height - 8).max(top + 8));
    }
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .context("set tray menu position")?;
    window.show().context("show tray menu")?;
    window.set_focus().context("focus tray menu")?;
    Ok(())
}
