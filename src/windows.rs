//! 窗口、后端导航与保留的 WebUI 标题栏注入。
use crate::{
    launcher, page_load_injector, redacted_url_log, webui_navigate_url, BACKEND_CONNECT_TIMEOUT,
    BACKEND_NAVIGATION_TIMEOUT,
};
use anyhow::{anyhow, Result};
use rust_i18n::t;
use std::{
    net::{SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{Manager, Url, WebviewWindow};
use tracing::{debug, error, warn};

pub(crate) fn local_page_url(app: &tauri::AppHandle, hash: &str) -> Result<Url> {
    #[cfg(not(feature = "custom-protocol"))]
    if let Some(url) = &app.config().build.dev_url {
        let mut url = url.clone();
        url.set_path("/index.html");
        url.set_fragment(Some(hash.trim_start_matches('#')));
        return Ok(url);
    }
    #[cfg(windows)]
    let base = "http://tauri.localhost/index.html";
    #[cfg(not(windows))]
    let base = "tauri://localhost/index.html";
    let _ = app;
    let mut url = Url::parse(base)?;
    url.set_fragment(Some(hash.trim_start_matches('#')));
    Ok(url)
}

pub(crate) fn show_launcher_home(app: &tauri::AppHandle) -> Result<()> {
    #[cfg(target_os = "macos")]
    set_macos_activation_policy(app, true);
    if let Some(window) = app.get_webview_window("launcher") {
        reveal_window(&window)?;
    }
    Ok(())
}

pub(crate) fn hide_launcher_home(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("launcher") {
        #[cfg(target_os = "linux")]
        let _ = window.minimize();
        #[cfg(not(target_os = "linux"))]
        let _ = window.hide();
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn set_macos_activation_policy(app: &tauri::AppHandle, regular: bool) {
    let policy = if regular {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    if let Err(error) = app.set_activation_policy(policy) {
        warn!("Failed to set activation policy: {error:?}");
    }
}

pub(crate) fn check_backend_connection(port: u16) -> Result<()> {
    let address: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    TcpStream::connect_timeout(&address, BACKEND_CONNECT_TIMEOUT)
        .map(|_| ())
        .map_err(|e| {
            anyhow!(t!(
                "errors.backend_unreachable",
                address = address.to_string(),
                error = e.to_string()
            ))
        })
}

/// 未就绪提示与日志中的失败原因合成最终错误：日志里写了原因时一并带出。
pub(crate) fn backend_unready_error(
    timeout_error: anyhow::Error,
    reason: Option<String>,
) -> anyhow::Error {
    match reason {
        Some(reason) => anyhow!(t!(
            "errors.backend_unready_with_reason",
            error = format!("{timeout_error:#}"),
            reason = reason
        )),
        None => timeout_error,
    }
}

/// 在给定时限内轮询后端端口直到可达。
///
/// 每 200ms 探测一次；超时后把最后一次连接错误与后端日志中发现的失败
/// 原因合成最终错误（见 backend_unready_error）。
///
/// # Errors
/// 超时仍不可达时返回 Err（附带最后一次连接错误与可能的后端失败原因）。
pub(crate) fn wait_for_backend_connection(port: u16, timeout: Duration) -> Result<()> {
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

    let timeout_error = last_error.unwrap_or_else(|| anyhow!(t!("errors.backend_timeout")));
    Err(backend_unready_error(
        timeout_error,
        crate::backend::read_backend_failure_reason(),
    ))
}

/// 等待后端就绪后把窗口导航到 WebUI（必要时带免密令牌）；失败则导航到
/// 本地 React 连接错误页面。
///
/// # Returns
/// Ok(true) 表示已导航到后端；Ok(false) 表示后端不可达、已转错误页面。
///
/// # Errors
/// 仅当错误页导航本身失败（URL 解析或 WebView 导航出错）时返回 Err。
fn session_matches(window: &WebviewWindow, task: u64) -> bool {
    let view = window.state::<Arc<launcher::Launcher>>().snapshot();
    view.task_id == task && view.backend_running && view.phase == launcher::Phase::Running
}

pub(crate) fn navigate_backend_or_error(
    window: &WebviewWindow,
    port: u16,
    task: u64,
) -> Result<bool> {
    let result = wait_for_backend_connection(port, BACKEND_NAVIGATION_TIMEOUT);
    if !session_matches(window, task) {
        return Ok(false);
    }
    match result {
        Ok(()) => {
            let url = webui_navigate_url(port);
            if !session_matches(window, task) {
                return Ok(false);
            }
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

/// 错误详情保存在状态中，窗口只导航到固定的 React 路由。
///
/// # Errors
/// 错误 URL 构造或导航失败时返回 Err。
pub(crate) fn navigate_to_backend_error(
    window: &WebviewWindow,
    _port: u16,
    error_detail: &str,
) -> Result<()> {
    let controller = window.state::<Arc<launcher::Launcher>>();
    controller.connection_error(error_detail.to_owned());
    window.navigate(local_page_url(window.app_handle(), "#/connection-error")?)?;
    Ok(())
}

/// 主窗口导航拦截：目标不是后端地址时放行；是后端地址但连接不上时阻止
/// 本次导航，并异步把窗口切到错误页面。
///
/// # Returns
/// true 表示放行导航，false 表示已拦截。
pub(crate) fn handle_backend_navigation(app: tauri::AppHandle, port: u16, url: &Url) -> bool {
    let expected_task = app.state::<Arc<launcher::Launcher>>().snapshot().task_id;
    if !is_backend_url(url, port) {
        return true;
    }

    match check_backend_connection(port) {
        Ok(()) => true,
        Err(e) => {
            let blocked_url = redacted_url_log(url);
            warn!(
                "Blocked navigation to unreachable backend {}: {:?}",
                blocked_url, e
            );
            let error_detail = e.to_string();
            // 在独立线程里切换到错误页，避免在导航回调内同步改写导航目标。
            thread::spawn(move || {
                let view = app.state::<Arc<launcher::Launcher>>().snapshot();
                if view.task_id != expected_task
                    || !view.backend_running
                    || view.phase != launcher::Phase::Running
                {
                    return;
                }
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

/// 判断 URL 是否指向本地后端：http/https + 127.0.0.1/localhost + 目标端口。
pub(crate) fn is_backend_url(url: &Url, port: u16) -> bool {
    matches!(url.scheme(), "http" | "https")
        && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
        && url.port_or_known_default() == Some(port)
}

pub(crate) fn create_main_window(app: &tauri::AppHandle, port: u16) -> Result<WebviewWindow> {
    let main_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .ok_or_else(|| anyhow!(t!("errors.main_window_missing")))?;

    let app_for_navigation = app.clone();
    let main_window = tauri::WebviewWindowBuilder::from_config(app, main_config)?
        .on_navigation(move |url| handle_backend_navigation(app_for_navigation.clone(), port, url))
        .on_page_load(page_load_injector)
        .build()?;
    main_window.set_resizable(true)?;

    // Windows/Linux：主窗口同样去掉原生装饰（无边框，配合注入的标题栏）；
    // splash 则已在 tauri.conf.json 中配置为无边框。
    #[cfg(not(target_os = "macos"))]
    {
        main_window.set_decorations(false)?;
    }

    Ok(main_window)
}

/// 显示窗口并聚焦；若窗口处于最小化状态先还原。
///
/// # Errors
/// 任一窗口操作失败时返回 Err。
pub(crate) fn reveal_window(window: &WebviewWindow) -> tauri::Result<()> {
    if window.is_minimized()? {
        window.unminimize()?;
    }
    window.show()?;
    window.set_focus()?;
    Ok(())
}

/// 把主窗口最小化到托盘（按平台采取不同策略）。
///
/// 平台差异：Windows 上直接销毁窗口以释放 WebView 资源（恢复时重建）；
/// macOS/Linux 仅隐藏窗口；macOS 额外切换到 Accessory 激活策略以隐藏
/// Dock 图标。
pub(crate) fn minimize_main_window_to_tray(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        if let Some(window) = app.get_webview_window("main") {
            tracing::info!("Destroying main window to release WebView resources while trayed");
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

/// 任意线程安全地调度主窗口恢复：包装 run_on_main_thread，调度失败仅告警。
pub(crate) fn restore_main_window_from_any_thread(
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

/// 从托盘恢复主窗口：窗口仍存在则直接显示并聚焦；已被销毁（Windows 托盘
/// 化会销毁窗口）则以防重入标志保护，在新线程中重建并导航。
pub(crate) fn restore_main_window_from_tray(
    app: &tauri::AppHandle,
    port: u16,
    recreating_main_window: Arc<AtomicBool>,
) {
    let view = app.state::<Arc<launcher::Launcher>>().snapshot();
    if !view.backend_running || view.phase != launcher::Phase::Running {
        let _ = show_launcher_home(app);
        return;
    }
    let task = view.task_id;
    if recreating_main_window.load(Ordering::SeqCst) {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        set_macos_activation_policy(app, true);
        if reveal_window(&window).is_ok() {
            hide_launcher_home(app);
        }
        return;
    }

    // 防重入：托盘、单实例、通知点击可能并发触发，只允许一次重建。
    if recreating_main_window
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        debug!("Main window recreation already in progress");
        return;
    }

    // 重建在新线程执行：窗口构建与导航可能耗时，不能阻塞调用线程。
    let app_handle = app.clone();
    thread::spawn(move || {
        // 重建期间同样先把 macOS 切回 Regular，保证 Dock 图标即时出现。
        #[cfg(target_os = "macos")]
        set_macos_activation_policy(&app_handle, true);

        let result = (|| -> Result<()> {
            let window = create_main_window(&app_handle, port)?;
            if !session_matches(&window, task) {
                let _ = window.destroy();
                return Ok(());
            }
            navigate_backend_or_error(&window, port, task)?;
            if !session_matches(&window, task) {
                let _ = window.destroy();
                return Ok(());
            }
            reveal_window(&window)?;
            hide_launcher_home(&app_handle);
            Ok(())
        })();

        // 无论成败都复位重建标志，允许后续再次触发。
        recreating_main_window.store(false, Ordering::SeqCst);

        if let Err(e) = result {
            error!("Failed to recreate main window from tray: {:?}", e);
            let _ = show_launcher_home(&app_handle);
        }
        let view = app_handle.state::<Arc<launcher::Launcher>>().snapshot();
        if view.phase == launcher::Phase::Running && view.task_id != task {
            restore_main_window_from_tray(&app_handle, port, recreating_main_window);
        }
    });
}

/// 托盘"显示/隐藏"菜单与左键点击的入口：按当前窗口状态在托盘化与恢复
/// 之间切换；窗口不存在时走恢复（重建）路径。
pub(crate) fn toggle_main_window_visibility(
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

/// 生成注入主窗口的自定义标题栏 JS 脚本。
///
/// 脚本职责：在页面顶部叠加拖拽区，右上角放置"最小化/最大化/关闭"按钮
/// 胶囊（样式随页面深色主题自动切换）；关闭按钮在 Windows 上打开窗口内
/// "退出 / 最小化到托盘"选择菜单，其它平台直接调用 window_close；双击
/// 标题栏切换最大化。按钮动作全部经 Tauri 命令完成。脚本还会定位页面
/// 已有的 topbar/header 元素同步高度与偏移，并监听主题与窗口尺寸变化。
///
/// 平台差异：macOS 返回空脚本——系统自带红绿灯按钮，无需自绘标题栏。
pub(crate) fn main_window_titlebar_injection_script() -> String {
    // macOS：系统自带红绿灯按钮，无需注入自绘标题栏。
    #[cfg(target_os = "macos")]
    {
        String::new()
    }
    #[cfg(not(target_os = "macos"))]
    {
        // 标题栏文案以 JSON 注入，避免字符串拼接时的转义问题。
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
        // 关闭确认菜单仅 Windows 启用；其余平台保持单击直接关闭的行为。
        s.push_str(if cfg!(windows) {
            ";const closePromptEnabled = true;"
        } else {
            ";const closePromptEnabled = false;"
        });
        // 以下为原样注入的 JS 主体：创建标题栏 DOM 与样式、绑定拖拽和按钮
        // 事件、同步页面主题与布局。
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
                style.textContent = ':root{--alas-titlebar-height:56px}#alas-launcher-titlebar{position:fixed;top:0;left:0;right:0;height:var(--topbar-height,var(--alas-titlebar-height,56px));z-index:2147483647;user-select:none;pointer-events:none;background:transparent}#alas-launcher-titlebar *{box-sizing:border-box}.alas-titlebar-drag-zone{position:absolute;inset:0 148px 0 0;height:100%;pointer-events:none;background:transparent;touch-action:none}.header-icon,.header-icon *{app-region:no-drag;-webkit-app-region:no-drag}.header-icon{display:flex;align-items:center;gap:3px;padding:3px 6px;position:absolute;top:50%;transform:translateY(-50%);right:10px;height:36px;pointer-events:auto;border:1px solid rgba(255,255,255,.92);border-radius:18px;background:rgba(250,250,247,.78);box-shadow:0 4px 14px rgba(61,79,97,.1),inset 0 1px 0 rgba(255,255,255,.36);backdrop-filter:blur(16px) saturate(1.2);-webkit-backdrop-filter:blur(16px) saturate(1.2);transition:background .2s ease,border-color .2s ease,box-shadow .2s ease}.icon{width:28px;height:28px;min-width:28px;min-height:28px;margin:0;padding:0;line-height:1;border-radius:12px;border:none;background:transparent;color:#727b86;cursor:pointer;flex:0 0 auto;position:relative;transition:transform 140ms cubic-bezier(.23,1,.32,1),background-color 140ms ease,color 140ms ease;display:inline-flex;align-items:center;justify-content:center}.icon:hover{color:#202832;background:rgba(255,255,255,.72)}.icon:active{transform:scale(.96)}.icon-close{color:#e64f58}.icon-close:hover{color:#b5202e;background:rgba(244,91,91,.15)}.icon svg{width:11px;height:11px;stroke:currentColor;fill:none;stroke-width:1.2;stroke-linecap:round;stroke-linejoin:round;opacity:1;display:block;margin:auto}';
                style.textContent += '#alas-close-menu{position:fixed;top:8px;right:10px;z-index:2147483647;width:272px;padding:14px;border:1px solid rgba(255,255,255,.9);border-radius:20px;background:rgba(250,250,247,.94);box-shadow:0 18px 46px rgba(46,58,72,.2),inset 0 1px 0 rgba(255,255,255,.7);backdrop-filter:blur(20px) saturate(1.18);-webkit-backdrop-filter:blur(20px) saturate(1.18);color:#202832;opacity:0;pointer-events:none;transform:translateY(-6px) scale(.96);transform-origin:calc(100% - 64px) 0;transition:opacity 140ms ease,transform 180ms cubic-bezier(.23,1,.32,1),background .2s ease,border-color .2s ease;app-region:no-drag;-webkit-app-region:no-drag}#alas-close-menu.is-open{opacity:1;pointer-events:auto;transform:translateY(0) scale(1)}#alas-close-menu *{box-sizing:border-box;app-region:no-drag;-webkit-app-region:no-drag}#alas-close-menu-title{margin:0 0 12px;font:500 13px/1.55 "MiSans",sans-serif;color:rgba(32,40,50,.82)}#alas-close-menu-actions{display:grid;grid-template-columns:1fr 1fr;gap:8px}#alas-close-menu button{display:flex;align-items:center;justify-content:center;min-width:0;min-height:36px;margin:0;padding:0 12px;border:1px solid rgba(92,105,120,.16);border-radius:11px;background:rgba(105,118,133,.08);color:#394451;font:600 12px/1 "MiSans",sans-serif;cursor:pointer;transition:background-color 140ms ease,color 140ms ease,transform 140ms cubic-bezier(.23,1,.32,1)}#alas-close-menu button:hover{background:rgba(105,118,133,.14)}#alas-close-menu button:active{transform:scale(.98)}#alas-close-menu button:disabled{opacity:.55;cursor:default;transform:none}#alas-close-menu .alas-close-confirm{border-color:rgba(205,62,69,.28);background:#d94b4b;color:#fff}#alas-close-menu .alas-close-confirm:hover{background:#c93e3e;color:#fff}';
                style.textContent += ':root[data-theme*="dark"] .header-icon,:root[data-color-mode="dark"] .header-icon,:root.dark .header-icon,body[data-theme*="dark"] .header-icon,body[data-color-mode="dark"] .header-icon,body.dark .header-icon,[data-theme*="dark"] .header-icon,[data-color-mode="dark"] .header-icon,#alas-launcher-titlebar.is-dark .header-icon{background:rgba(30,34,42,.78);border:1px solid rgba(255,255,255,.14);box-shadow:0 4px 16px rgba(0,0,0,.4),inset 0 1px 0 rgba(255,255,255,.14)}';
                style.textContent += ':root[data-theme*="dark"] .icon,:root[data-color-mode="dark"] .icon,:root.dark .icon,body[data-theme*="dark"] .icon,body[data-color-mode="dark"] .icon,body.dark .icon,[data-theme*="dark"] .icon,[data-color-mode="dark"] .icon,#alas-launcher-titlebar.is-dark .icon{color:rgba(220,228,238,.82)}';
                style.textContent += ':root[data-theme*="dark"] .icon:hover,:root[data-color-mode="dark"] .icon:hover,:root.dark .icon:hover,body[data-theme*="dark"] .icon:hover,body[data-color-mode="dark"] .icon:hover,body.dark .icon:hover,[data-theme*="dark"] .icon:hover,[data-color-mode="dark"] .icon:hover,#alas-launcher-titlebar.is-dark .icon:hover{color:#fff;background:rgba(255,255,255,.14)}';
                style.textContent += ':root[data-theme*="dark"] .icon:active,:root[data-color-mode="dark"] .icon:active,:root.dark .icon:active,body[data-theme*="dark"] .icon:active,body[data-color-mode="dark"] .icon:active,body.dark .icon:active,[data-theme*="dark"] .icon:active,[data-color-mode="dark"] .icon:active,#alas-launcher-titlebar.is-dark .icon:active{background:rgba(255,255,255,.22)}';
                style.textContent += ':root[data-theme*="dark"] .icon-close,:root[data-color-mode="dark"] .icon-close,:root.dark .icon-close,body[data-theme*="dark"] .icon-close,body[data-color-mode="dark"] .icon-close,body.dark .icon-close,[data-theme*="dark"] .icon-close,[data-color-mode="dark"] .icon-close,#alas-launcher-titlebar.is-dark .icon-close{color:#ff6470}';
                style.textContent += ':root[data-theme*="dark"] .icon-close:hover,:root[data-color-mode="dark"] .icon-close:hover,:root.dark .icon-close:hover,body[data-theme*="dark"] .icon-close:hover,body[data-color-mode="dark"] .icon-close:hover,body.dark .icon-close:hover,[data-theme*="dark"] .icon-close:hover,[data-color-mode="dark"] .icon-close:hover,#alas-launcher-titlebar.is-dark .icon-close:hover{color:#fff;background:#e0444d}';
                style.textContent += ':root[data-theme*="dark"] .icon-close:active,:root[data-color-mode="dark"] .icon-close:active,:root.dark .icon-close:active,body[data-theme*="dark"] .icon-close:active,body[data-color-mode="dark"] .icon-close:active,body.dark .icon-close:active,[data-theme*="dark"] .icon-close:active,[data-color-mode="dark"] .icon-close:active,#alas-launcher-titlebar.is-dark .icon-close:active{background:#c7363f}';
                style.textContent += ':root[data-theme*="dark"] #alas-close-menu,:root[data-color-mode="dark"] #alas-close-menu,:root.dark #alas-close-menu,body[data-theme*="dark"] #alas-close-menu,body[data-color-mode="dark"] #alas-close-menu,body.dark #alas-close-menu,[data-theme*="dark"] #alas-close-menu,[data-color-mode="dark"] #alas-close-menu,#alas-close-menu.is-dark{background:rgba(28,32,38,.94);border:1px solid rgba(255,255,255,.14);box-shadow:0 18px 46px rgba(0,0,0,.55),inset 0 1px 0 rgba(255,255,255,.12);color:#f0f3f6}';
                style.textContent += ':root[data-theme*="dark"] #alas-close-menu-title,:root[data-color-mode="dark"] #alas-close-menu-title,:root.dark #alas-close-menu-title,body[data-theme*="dark"] #alas-close-menu-title,body[data-color-mode="dark"] #alas-close-menu-title,body.dark #alas-close-menu-title,[data-theme*="dark"] #alas-close-menu-title,[data-color-mode="dark"] #alas-close-menu-title,#alas-close-menu.is-dark #alas-close-menu-title{color:rgba(240,244,250,.9)}';
                style.textContent += ':root[data-theme*="dark"] #alas-close-menu button,:root[data-color-mode="dark"] #alas-close-menu button,:root.dark #alas-close-menu button,body[data-theme*="dark"] #alas-close-menu button,body[data-color-mode="dark"] #alas-close-menu button,body.dark #alas-close-menu button,[data-theme*="dark"] #alas-close-menu button,[data-color-mode="dark"] #alas-close-menu button,#alas-close-menu.is-dark button{border:1px solid rgba(255,255,255,.12);background:rgba(255,255,255,.08);color:rgba(230,235,245,.88)}';
                style.textContent += ':root[data-theme*="dark"] #alas-close-menu button:hover,:root[data-color-mode="dark"] #alas-close-menu button:hover,:root.dark #alas-close-menu button:hover,body[data-theme*="dark"] #alas-close-menu button:hover,body[data-color-mode="dark"] #alas-close-menu button:hover,body.dark #alas-close-menu button:hover,[data-theme*="dark"] #alas-close-menu button:hover,[data-color-mode="dark"] #alas-close-menu button:hover,#alas-close-menu.is-dark button:hover{background:rgba(255,255,255,.14);color:#fff}';
                style.textContent += ':root[data-theme*="dark"] #alas-close-menu .alas-close-confirm,:root[data-color-mode="dark"] #alas-close-menu .alas-close-confirm,:root.dark #alas-close-menu .alas-close-confirm,body[data-theme*="dark"] #alas-close-menu .alas-close-confirm,body[data-color-mode="dark"] #alas-close-menu .alas-close-confirm,body.dark #alas-close-menu .alas-close-confirm,[data-theme*="dark"] #alas-close-menu .alas-close-confirm,[data-color-mode="dark"] #alas-close-menu .alas-close-confirm,#alas-close-menu.is-dark .alas-close-confirm{border-color:rgba(244,91,91,.35);background:#d94b4b;color:#fff}';
                style.textContent += ':root[data-theme*="dark"] #alas-close-menu .alas-close-confirm:hover,:root[data-color-mode="dark"] #alas-close-menu .alas-close-confirm:hover,:root.dark #alas-close-menu .alas-close-confirm:hover,body[data-theme*="dark"] #alas-close-menu .alas-close-confirm:hover,body[data-color-mode="dark"] #alas-close-menu .alas-close-confirm:hover,body.dark #alas-close-menu .alas-close-confirm:hover,[data-theme*="dark"] #alas-close-menu .alas-close-confirm:hover,[data-color-mode="dark"] #alas-close-menu .alas-close-confirm:hover,#alas-close-menu.is-dark .alas-close-confirm:hover{background:#c93e3e;color:#fff}';
                style.textContent += '@media (prefers-color-scheme:dark){:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) .header-icon{background:rgba(30,34,42,.78);border:1px solid rgba(255,255,255,.14);box-shadow:0 4px 16px rgba(0,0,0,.4),inset 0 1px 0 rgba(255,255,255,.14)}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) .icon{color:rgba(220,228,238,.82)}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) .icon:hover{color:#fff;background:rgba(255,255,255,.14)}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) .icon:active{background:rgba(255,255,255,.22)}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) .icon-close{color:#ff6470}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) .icon-close:hover{color:#fff;background:#e0444d}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) .icon-close:active{background:#c7363f}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) #alas-close-menu{background:rgba(28,32,38,.94);border:1px solid rgba(255,255,255,.14);box-shadow:0 18px 46px rgba(0,0,0,.55),inset 0 1px 0 rgba(255,255,255,.12);color:#f0f3f6}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) #alas-close-menu-title{color:rgba(240,244,250,.9)}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) #alas-close-menu button{border:1px solid rgba(255,255,255,.12);background:rgba(255,255,255,.08);color:rgba(230,235,245,.88)}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) #alas-close-menu button:hover{background:rgba(255,255,255,.14);color:#fff}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) #alas-close-menu .alas-close-confirm{border-color:rgba(244,91,91,.35);background:#d94b4b;color:#fff}:root:not([data-theme*="light"]):not([data-color-mode="light"]):not(.light) #alas-close-menu .alas-close-confirm:hover{background:#c93e3e;color:#fff}}';
                document.head.appendChild(style);
            }
            const titlebar = document.createElement('div');
            titlebar.id = 'alas-launcher-titlebar';
            titlebar.innerHTML = '<div class="alas-titlebar-drag-zone" aria-hidden="true"></div><div class="header-icon"><button type="button" class="icon icon-hide" data-action="hide" aria-label="'+i18n.hideLabel+'" title="'+i18n.hideLabel+'"><svg viewBox="0 0 6 6"><rect x="1" y="1" width="4" height="4" rx="1"/><path d="M2 3h2"/></svg></button><button type="button" class="icon icon-minimize" data-action="minimize" aria-label="'+i18n.minimizeLabel+'" title="'+i18n.minimizeTitle+'"><svg viewBox="0 0 6 6"><line x1="1" y1="3" x2="5" y2="3"/></svg></button><button type="button" class="icon icon-maximize" data-action="maximize" aria-label="'+i18n.maximizeLabel+'" title="'+i18n.maximizeTitle+'"><svg viewBox="0 0 6 6" class="svg-restore" style="display:none"><polyline points="1,3 1,1 3,1"/><polyline points="3,5 5,5 5,3"/></svg><svg viewBox="0 0 6 6" class="svg-maximize"><polyline points="1,2.5 1,1 2.5,1"/><polyline points="3.5,5 5,5 5,3.5"/></svg></button><button type="button" class="icon icon-close" data-action="close" aria-label="'+i18n.closeLabel+'" title="'+i18n.closeTitle+'"><svg viewBox="0 0 6 6"><line x1="1" y1="1" x2="5" y2="5"/><line x1="5" y1="1" x2="1" y2="5"/></svg></button></div>';
            document.body.dataset.alasCustomTitlebar = 'true';
            document.body.prepend(titlebar);
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
            const syncTitlebarLayout = () => {
                try {
                    const topbar = document.querySelector('.topbar, header, #topbar, [class*="topbar"], [class*="Header"]');
                    let h = 0;
                    let topOffset = 0;
                    if (topbar) {
                        const rect = topbar.getBoundingClientRect();
                        if (rect.height > 20) {
                            h = rect.height;
                            topOffset = rect.top;
                        }
                    }
                    if (!h) {
                        const computed = getComputedStyle(document.documentElement).getPropertyValue('--topbar-height').trim();
                        if (computed) {
                            const parsed = parseFloat(computed);
                            if (!isNaN(parsed) && parsed > 20) {
                                h = parsed;
                            }
                        }
                    }
                    if (h > 0) {
                        titlebar.style.height = h + 'px';
                        if (topOffset >= 0) {
                            titlebar.style.top = topOffset + 'px';
                        }
                    }
                } catch (_) {}
            };
            const syncTheme = () => {
                try {
                    const doc = document.documentElement;
                    const body = document.body;
                    let isDark = false;
                    const dt = (doc.getAttribute('data-theme') || (body && body.getAttribute('data-theme')) || '').toLowerCase();
                    const dcm = (doc.getAttribute('data-color-mode') || (body && body.getAttribute('data-color-mode')) || '').toLowerCase();
                    if (dt.includes('dark') || dcm === 'dark') {
                        isDark = true;
                    } else if (dt.includes('light') || dcm === 'light') {
                        isDark = false;
                    } else if (doc.classList.contains('dark') || (body && body.classList.contains('dark'))) {
                        isDark = true;
                    } else if (doc.classList.contains('light') || (body && body.classList.contains('light'))) {
                        isDark = false;
                    } else {
                        try {
                            const st = (localStorage.getItem('azurpilot.theme') || localStorage.getItem('theme') || '').toLowerCase();
                            const sm = (localStorage.getItem('azurpilot.color-mode') || '').toLowerCase();
                            if (st.includes('dark') || sm === 'dark') isDark = true;
                            else if (st.includes('light') || sm === 'light') isDark = false;
                        } catch (_) {}
                        if (!isDark && window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) {
                            isDark = true;
                        }
                    }
                    titlebar.classList.toggle('is-dark', isDark);
                    if (closeMenu) closeMenu.classList.toggle('is-dark', isDark);
                } catch (e) {
                    console.error('Failed to sync titlebar theme', e);
                }
            };
            const setCloseMenuOpen = open => {
                if (open) {
                    syncTheme();
                    syncTitlebarLayout();
                    try {
                        const iconRect = titlebar.querySelector('.header-icon').getBoundingClientRect();
                        closeMenu.style.top = Math.round(iconRect.bottom + 8) + 'px';
                        closeMenu.style.right = Math.max(10, Math.round(window.innerWidth - iconRect.right)) + 'px';
                    } catch (_) {}
                }
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
            const interactiveSelector = 'a[href],button,input,select,textarea,summary,label[for],[role="button"],[role="link"],[contenteditable="true"],[tabindex]:not([tabindex="-1"]),[onclick]';
            const isInteractiveElement = element => {
                if (!element || !(element instanceof Element)) return false;
                return Boolean(
                    element.closest(interactiveSelector)
                    || element.closest('[data-alas-no-drag]')
                    || (getComputedStyle(element).cursor === 'pointer' && !element.closest('#alas-launcher-titlebar'))
                );
            };

            document.addEventListener('pointerdown', event => {
                syncTheme();
                syncTitlebarLayout();
                if (!event.isPrimary || event.button !== 0) return;
                const tbRect = titlebar.getBoundingClientRect();
                const titlebarHeight = tbRect.height || 56;
                const titlebarTop = tbRect.top || 0;
                const reservedRight = 148;
                if (event.clientY < titlebarTop || event.clientY > titlebarTop + titlebarHeight || event.clientX > window.innerWidth - reservedRight) return;
                const target = event.target;
                if (!target || !(target instanceof Element)) return;
                if (titlebar.contains(target) || target.closest('#alas-close-menu')) return;
                if (isInteractiveElement(target)) return;
                event.preventDefault();
                invoke('window_start_dragging').catch(error => {
                    console.error('Failed to start dragging from titlebar', error);
                });
            }, { capture: true });

            document.addEventListener('dblclick', async event => {
                syncTheme();
                syncTitlebarLayout();
                const tbRect = titlebar.getBoundingClientRect();
                const titlebarHeight = tbRect.height || 56;
                const titlebarTop = tbRect.top || 0;
                const reservedRight = 148;
                if (event.clientY < titlebarTop || event.clientY > titlebarTop + titlebarHeight || event.clientX > window.innerWidth - reservedRight) return;
                const target = event.target;
                if (!target || !(target instanceof Element)) return;
                if (titlebar.contains(target) || target.closest('#alas-close-menu')) return;
                if (isInteractiveElement(target)) return;
                try {
                    await invoke('window_toggle_maximize');
                    await syncMaximizeState();
                } catch (error) {
                    console.error('Failed to toggle maximize from titlebar', error);
                }
            }, { capture: true });

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
                    syncTheme();
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
            window.addEventListener('resize', () => { syncTitlebarLayout(); void syncMaximizeState(); syncTheme(); });
            if (window.matchMedia) {
                try {
                    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', syncTheme);
                } catch (_) {}
            }
            void syncMaximizeState();
            syncTheme();
            syncTitlebarLayout();
            setTimeout(syncTitlebarLayout, 100);
            setTimeout(syncTitlebarLayout, 500);
        };
        ensureTitlebar();
        if (!document.body) {
            window.addEventListener('DOMContentLoaded', ensureTitlebar, { once: true });
        }
        "#);
        s
    }
}
