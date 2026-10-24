//! 启动器状态机及任务调度。所有环境操作必须先在同一把锁下取得任务槽位。
#[cfg(windows)]
use crate::prompt_for_missing_nodejs;
#[cfg(any(windows, target_os = "macos"))]
use crate::tray_icon_for_platform;
use crate::{
    backend::{ManagedBackend, WebuiLaunchConfig},
    initialize_logging,
    launcher_control::start_launcher_control_stream,
    notify::{start_notify_stream, NotificationClickHandler},
    preview_crash_arg_present, preview_no_update_arg_present,
    progress::{self, SplashUpdate},
    setup::{self, get_deploy_config, setup_azurpilot_repo, setup_environment},
    start_minimized_arg_present, time_bomb_expiration_message, updater, window_util,
    windows::*,
};
use anyhow::{bail, Context, Result};
use rust_i18n::t;
use serde::Serialize;
use std::sync::Condvar;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::Emitter;
#[cfg(any(windows, target_os = "macos"))]
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
};
use tauri::{Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tracing::{debug, info, warn};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Checking,
    UpdateAvailable,
    Updating,
    Idle,
    Starting,
    Running,
    Stopping,
    Repairing,
    Cancelling,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Operation {
    Check,
    Update,
    Start,
    Stop,
    Repair,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Snapshot {
    pub sequence: u64,
    pub task_id: u64,
    pub phase: Phase,
    pub operation: Option<Operation>,
    pub update_version: Option<String>,
    pub update_checked: bool,
    pub backend_running: bool,
    pub progress: Option<SplashUpdate>,
    pub error: Option<String>,
    pub connection_error: Option<String>,
    pub message: Option<String>,
    pub can_start: bool,
    pub can_repair: bool,
    pub can_stop: bool,
    pub can_check: bool,
    pub can_update: bool,
    pub can_cancel: bool,
}

struct Model {
    view: Snapshot,
    active: bool,
    cancel: Arc<AtomicBool>,
    committing: bool,
    exiting: bool,
    dirty: bool,
    progress: progress::ProgressState,
    update: Option<updater::AvailableUpdate>,
}

impl Model {
    fn new() -> Self {
        Self {
            view: Snapshot {
                sequence: 1,
                task_id: 0,
                phase: Phase::Idle,
                operation: None,
                update_version: None,
                update_checked: false,
                backend_running: false,
                progress: None,
                error: None,
                connection_error: None,
                message: None,
                can_start: false,
                can_repair: false,
                can_stop: false,
                can_check: true,
                can_update: false,
                can_cancel: false,
            },
            active: false,
            cancel: Arc::new(AtomicBool::new(false)),
            committing: false,
            exiting: false,
            dirty: true,
            progress: Default::default(),
            update: None,
        }
    }

    fn snapshot(&self) -> Snapshot {
        let mut view = self.view.clone();
        let free = !self.active && !self.exiting;
        let ready = free && view.update_checked && self.update.is_none() && !view.backend_running;
        view.can_start = ready;
        view.can_repair = ready;
        view.can_stop = free && view.backend_running;
        view.can_check = free && !view.backend_running;
        view.can_update = free && self.update.is_some();
        view.can_cancel = self.active && !self.committing && !self.exiting;
        view
    }

    fn touch(&mut self) {
        self.view.sequence += 1;
        self.dirty = true;
    }

    fn begin(&mut self, operation: Operation) -> Result<(u64, Arc<AtomicBool>)> {
        let view = self.snapshot();
        let allowed = match operation {
            Operation::Check => view.can_check,
            Operation::Update => view.can_update,
            Operation::Start => view.can_start,
            Operation::Repair => view.can_repair,
            Operation::Stop => view.can_stop,
        };
        if !allowed {
            bail!(t!("launcher.busy"));
        }
        self.active = true;
        self.committing = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.progress = Default::default();
        self.view.task_id += 1;
        self.view.phase = match operation {
            Operation::Check => Phase::Checking,
            Operation::Update => Phase::Updating,
            Operation::Start => Phase::Starting,
            Operation::Stop => Phase::Stopping,
            Operation::Repair => Phase::Repairing,
        };
        self.view.operation = Some(operation);
        self.view.progress = None;
        self.view.error = None;
        self.view.message = None;
        if operation == Operation::Check {
            self.view.update_checked = false;
        }
        self.touch();
        Ok((self.view.task_id, self.cancel.clone()))
    }

    fn accepts(&self, task: u64) -> bool {
        self.active
            && self.view.task_id == task
            && !self.cancel.load(Ordering::SeqCst)
            && !self.exiting
    }

    fn publish(&mut self, task: u64, update: SplashUpdate) {
        if self.accepts(task) && self.progress.accept(update) {
            self.view.progress = self.progress.latest.clone();
            self.touch();
        }
    }

    fn finish(&mut self, task: u64, result: Result<()>) {
        if self.view.task_id != task {
            return;
        }
        self.active = false;
        self.committing = false;
        if !self.exiting {
            match result {
                Ok(()) => {
                    self.view.phase = if self.view.backend_running {
                        Phase::Running
                    } else if self.update.is_some() {
                        Phase::UpdateAvailable
                    } else {
                        Phase::Idle
                    };
                    self.view.error = None;
                    self.view.progress = None;
                }
                Err(error) => {
                    self.view.phase = Phase::Error;
                    self.view.error = Some(format!("{error:#}"));
                }
            }
        }
        self.touch();
    }
}

struct Session {
    task: u64,
    stop: Arc<AtomicBool>,
    streams: Vec<thread::JoinHandle<()>>,
}

pub(crate) struct Launcher {
    app: tauri::AppHandle,
    model: Mutex<Model>,
    changed: Condvar,
    backend: Mutex<Option<(u64, ManagedBackend)>>,
    session: Mutex<Option<Session>>,
    allow_exit: Arc<AtomicBool>,
    recreate: Arc<AtomicBool>,
    port: u16,
    skip_update: bool,
    minimized: bool,
}

impl Launcher {
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.model.lock().unwrap().snapshot()
    }

    fn change(&self, change: impl FnOnce(&mut Model)) {
        let mut model = self.model.lock().unwrap();
        change(&mut model);
        self.changed.notify_all();
    }

    fn publish(&self, task: u64, update: SplashUpdate) {
        self.change(|m| m.publish(task, update));
    }

    fn dispatch(self: &Arc<Self>) {
        let controller = self.clone();
        thread::spawn(move || {
            let mut sent = Instant::now() - progress::UPDATE_INTERVAL;
            loop {
                let mut model = controller.model.lock().unwrap();
                while !model.dirty && !controller.allow_exit.load(Ordering::SeqCst) {
                    model = controller.changed.wait(model).unwrap();
                }
                if controller.allow_exit.load(Ordering::SeqCst) {
                    return;
                }
                let urgent = !model.active
                    || matches!(model.view.phase, Phase::Cancelling | Phase::Error)
                    || model
                        .view
                        .progress
                        .as_ref()
                        .is_some_and(|p| p.is_error || p.progress >= 100.0);
                let delay = progress::UPDATE_INTERVAL.saturating_sub(sent.elapsed());
                if !urgent && !delay.is_zero() {
                    let _ = controller.changed.wait_timeout(model, delay).unwrap();
                    continue;
                }
                model.dirty = false;
                let snapshot = model.snapshot();
                drop(model);
                let _ = controller
                    .app
                    .emit_to("launcher", "launcher://state", &snapshot);
                if let Some(window) = controller.app.get_webview_window("main") {
                    if is_local_page(&controller.app, &window) {
                        let _ = window.emit("launcher://state", &snapshot);
                    }
                }
                sent = Instant::now();
            }
        });
    }

    fn begin(&self, operation: Operation) -> Result<(u64, Arc<AtomicBool>)> {
        let result = self.model.lock().unwrap().begin(operation);
        self.changed.notify_all();
        result
    }

    fn finish(&self, task: u64, result: Result<()>) {
        let failed = result.is_err();
        self.change(|m| m.finish(task, result));
        if failed && !self.model.lock().unwrap().exiting {
            let _ = show_launcher_home(&self.app);
        }
    }

    fn check(self: &Arc<Self>, auto_start: bool) -> Result<()> {
        let (task, cancel) = self.begin(Operation::Check)?;
        let controller = self.clone();
        thread::spawn(move || {
            let result = (|| {
                let update = if controller.skip_update {
                    None
                } else {
                    updater::check_update(&cancel)?
                };
                let mut model = controller.model.lock().unwrap();
                if !model.accepts(task) {
                    bail!(t!("launcher.cancelled"));
                }
                model.view.update_version = update.as_ref().map(|u| u.version.clone());
                model.update = update;
                model.view.update_checked = true;
                Ok(())
            })();
            let success = result.is_ok();
            controller.finish(task, result);
            let view = controller.snapshot();
            if auto_start && success && view.can_start {
                let _ = controller.start();
            } else if view.update_version.is_some() {
                let _ = show_launcher_home(&controller.app);
            }
        });
        Ok(())
    }

    fn install_update(self: &Arc<Self>) -> Result<()> {
        let (task, cancel) = self.begin(Operation::Update)?;
        let controller = self.clone();
        thread::spawn(move || {
            let result = (|| {
                let update = controller
                    .model
                    .lock()
                    .unwrap()
                    .update
                    .clone()
                    .context("missing update")?;
                updater::install_update(
                    &update,
                    cancel,
                    |u| controller.publish(task, u),
                    || {
                        // Cancellation and the atomic replacement handoff share the state lock.
                        let mut model = controller.model.lock().unwrap();
                        if !model.accepts(task) {
                            bail!(t!("launcher.cancelled"));
                        }
                        model.committing = true;
                        model.touch();
                        controller.changed.notify_all();
                        Ok(())
                    },
                )?;
                controller.allow_exit.store(true, Ordering::SeqCst);
                controller.changed.notify_all();
                controller.app.exit(0);
                Ok(())
            })();
            controller.finish(task, result);
        });
        Ok(())
    }

    fn start(self: &Arc<Self>) -> Result<()> {
        let (task, cancel) = self.begin(Operation::Start)?;
        let controller = self.clone();
        thread::spawn(move || {
            let result = (|| {
                #[cfg(windows)]
                {
                    let window = controller
                        .app
                        .get_webview_window("launcher")
                        .context("launcher window missing")?;
                    if !prompt_for_missing_nodejs(
                        &controller.app,
                        &window,
                        &mut |u| controller.publish(task, u),
                        &cancel,
                        controller.minimized,
                    ) {
                        bail!(t!("launcher.cancelled"));
                    }
                }
                setup_azurpilot_repo(
                    |u| controller.publish(task, u),
                    cancel.clone(),
                    controller.skip_update,
                )?;
                ensure_not_cancelled(&cancel)?;
                let mut config =
                    WebuiLaunchConfig::from_deploy_config(get_deploy_config().as_ref());
                config.port = controller.port;
                controller.publish(
                    task,
                    SplashUpdate::loading(t!("splash.starting"), t!("splash.starting_backend"), 97),
                );
                let backend = ManagedBackend::new_cancellable(&config, &cancel)?;
                // Prevent cancellation racing with adoption of a newly spawned backend.
                {
                    let mut model = controller.model.lock().unwrap();
                    if !model.accepts(task) {
                        bail!(t!("launcher.cancelled"));
                    }
                    *controller.backend.lock().unwrap() = Some((task, backend));
                    model.view.backend_running = true;
                    model.view.connection_error = None;
                }
                controller.start_session(task);
                Ok(())
            })();
            let success = result.is_ok();
            controller.finish(task, result);
            if success && !controller.model.lock().unwrap().exiting {
                if controller.minimized {
                    #[cfg(target_os = "linux")]
                    hide_launcher_home(&controller.app);
                } else {
                    controller.open_webui();
                }
            }
        });
        Ok(())
    }

    fn repair(self: &Arc<Self>) -> Result<()> {
        let (task, cancel) = self.begin(Operation::Repair)?;
        let controller = self.clone();
        thread::spawn(move || {
            let result = (|| {
                #[cfg(windows)]
                {
                    let window = controller
                        .app
                        .get_webview_window("launcher")
                        .context("launcher window missing")?;
                    if !prompt_for_missing_nodejs(
                        &controller.app,
                        &window,
                        &mut |u| controller.publish(task, u),
                        &cancel,
                        false,
                    ) {
                        bail!(t!("launcher.cancelled"));
                    }
                }
                setup::repair_environment(|u| controller.publish(task, u), cancel)?;
                controller.change(|m| {
                    if m.accepts(task) {
                        m.view.message = Some(t!("launcher.repair_done").to_string());
                    }
                });
                Ok(())
            })();
            controller.finish(task, result);
        });
        Ok(())
    }

    fn start_session(self: &Arc<Self>, task: u64) {
        let stop = Arc::new(AtomicBool::new(false));
        let on_click: NotificationClickHandler = {
            let controller = self.clone();
            let stop = stop.clone();
            Arc::new(move || {
                if !stop.load(Ordering::SeqCst) {
                    controller.open_webui();
                }
            })
        };
        let streams = vec![
            start_notify_stream(self.app.clone(), self.port, stop.clone(), on_click),
            start_launcher_control_stream(self.port, stop.clone()),
        ];
        *self.session.lock().unwrap() = Some(Session {
            task,
            stop: stop.clone(),
            streams,
        });
        let controller = self.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(250));
                let exited = {
                    let mut backend = controller.backend.lock().unwrap();
                    backend
                        .as_mut()
                        .filter(|(id, _)| *id == task)
                        .and_then(|(_, b)| b.try_wait().ok().flatten())
                        .is_some()
                };
                if exited && !stop.load(Ordering::SeqCst) {
                    controller.unexpected_exit(task);
                    break;
                }
            }
        });
    }

    fn unexpected_exit(&self, task: u64) {
        // Claim this session before joining its streams. A simultaneous stop wins safely.
        {
            let mut model = self.model.lock().unwrap();
            if model.active
                || model.exiting
                || !model.view.backend_running
                || model.view.task_id != task
            {
                return;
            }
            model.active = true;
            model.view.phase = Phase::Stopping;
            model.touch();
        }
        let _ = self.stop_backend();
        self.change(|m| {
            m.active = false;
            m.view.backend_running = false;
            m.view.phase = Phase::Error;
            m.view.operation = Some(Operation::Start);
            m.view.error = Some(t!("launcher.backend_exited").to_string());
            m.view.connection_error = m.view.error.clone();
            m.touch();
        });
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = navigate_to_backend_error(&window, self.port, &t!("launcher.backend_exited"));
        }
        let _ = show_launcher_home(&self.app);
    }

    fn stop_backend(&self) -> Result<()> {
        let session = self.session.lock().unwrap().take();
        if let Some(session) = &session {
            session.stop.store(true, Ordering::SeqCst);
        }
        let result = if let Some((_, mut backend)) = self.backend.lock().unwrap().take() {
            backend.terminate().map(|_| ())
        } else {
            Ok(())
        };
        if let Some(session) = session {
            for stream in session.streams {
                let _ = stream.join();
            }
            debug!("Stopped backend session {}", session.task);
        }
        result
    }

    fn stop(self: &Arc<Self>) -> Result<()> {
        let (task, _) = self.begin(Operation::Stop)?;
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.destroy();
        }
        let controller = self.clone();
        thread::spawn(move || {
            let result = controller.stop_backend();
            controller.change(|m| {
                m.view.backend_running = false;
                m.view.connection_error = None;
                m.view.message = Some(t!("launcher.stopped").to_string());
            });
            controller.finish(task, result);
        });
        Ok(())
    }

    pub(crate) fn connection_error(&self, error: String) {
        self.change(|m| {
            m.view.connection_error = Some(error);
            m.touch();
        });
    }

    pub(crate) fn open_webui(&self) {
        let view = self.snapshot();
        if !view.backend_running || view.phase != Phase::Running {
            let _ = show_launcher_home(&self.app);
            return;
        }
        restore_main_window_from_any_thread(self.app.clone(), self.port, self.recreate.clone());
    }

    pub(crate) fn request_close(self: &Arc<Self>) {
        let view = self.snapshot();
        if view.can_cancel {
            let _ = show_launcher_home(&self.app);
            let _ = self
                .app
                .emit_to("launcher", "launcher://close-requested", ());
        } else if view.backend_running {
            hide_launcher_home(&self.app);
        } else if !self.model.lock().unwrap().active {
            let _ = self.exit();
        }
    }

    /// 取消当前任务但保持启动器运行：置位任务取消标志并切换到 Cancelling
    /// 阶段，任务流程在下一个取消检查点中止，随后以 `launcher.cancelled`
    /// 错误进入错误态；启动器窗口保留，可直接重试或退出。
    ///
    /// # Errors
    /// 任务处于提交（committing）阶段时返回 `launcher.busy`；其余情况返回
    /// `Ok`，包括当前没有活动任务（幂等空操作）。
    pub(crate) fn cancel_task(self: &Arc<Self>) -> Result<()> {
        let mut model = self.model.lock().unwrap();
        if model.committing {
            bail!(t!("launcher.busy"));
        }
        if !model.active || model.exiting {
            return Ok(());
        }
        model.cancel.store(true, Ordering::SeqCst);
        model.view.phase = Phase::Cancelling;
        model.touch();
        self.changed.notify_all();
        Ok(())
    }

    pub(crate) fn exit(self: &Arc<Self>) -> Result<()> {
        {
            let mut model = self.model.lock().unwrap();
            if model.committing {
                bail!(t!("launcher.busy"));
            }
            if model.exiting {
                return Ok(());
            }
            model.exiting = true;
            model.cancel.store(true, Ordering::SeqCst);
            model.view.phase = Phase::Cancelling;
            model.touch();
            self.changed.notify_all();
        }
        let controller = self.clone();
        thread::spawn(move || {
            let mut model = controller.model.lock().unwrap();
            while model.active {
                model = controller.changed.wait(model).unwrap();
            }
            drop(model);
            if let Err(error) = controller.stop_backend() {
                warn!("Backend shutdown: {error:#}");
            }
            controller.allow_exit.store(true, Ordering::SeqCst);
            controller.changed.notify_all();
            controller.app.exit(0);
        });
        Ok(())
    }
}

pub(crate) fn ensure_not_cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        bail!(t!("launcher.cancelled"));
    }
    Ok(())
}

pub(crate) fn is_local_page(app: &tauri::AppHandle, window: &WebviewWindow) -> bool {
    let Ok(url) = window.url() else {
        return false;
    };
    let Ok(local) = local_page_url(app, "") else {
        return false;
    };
    same_local_origin(&url, &local) && (url.path() == "/" || url.path() == "/index.html")
}

// url::Origin treats custom schemes as opaque; compare explicit components so
// tauri://localhost also authorizes correctly on macOS and Linux.
fn same_local_origin(url: &tauri::Url, local: &tauri::Url) -> bool {
    url.scheme() == local.scheme()
        && url.host_str() == local.host_str()
        && url.port() == local.port()
        && url.username().is_empty()
        && url.password().is_none()
}

fn authorize(window: &WebviewWindow, home_only: bool) -> Result<(), String> {
    if (window.label() != "launcher" && (home_only || window.label() != "main"))
        || !is_local_page(window.app_handle(), window)
    {
        return Err(t!("launcher.forbidden").to_string());
    }
    Ok(())
}

#[tauri::command]
fn launcher_context(window: WebviewWindow) -> Result<serde_json::Value, String> {
    authorize(&window, false)?;
    info!("React page context ready: {}", window.label());
    Ok(
        serde_json::json!({ "locale": rust_i18n::locale().to_string(), "version": env!("CARGO_PKG_VERSION"), "platform": std::env::consts::OS }),
    )
}
#[tauri::command]
fn launcher_snapshot(
    window: WebviewWindow,
    controller: State<'_, Arc<Launcher>>,
) -> Result<Snapshot, String> {
    authorize(&window, false)?;
    debug!("React page snapshot ready: {}", window.label());
    Ok(controller.snapshot())
}

macro_rules! action_command {
    ($name:ident, $method:ident) => {
        #[tauri::command]
        fn $name(
            window: WebviewWindow,
            controller: State<'_, Arc<Launcher>>,
        ) -> Result<(), String> {
            authorize(&window, true)?;
            controller.$method().map_err(|e| format!("{e:#}"))
        }
    };
}
action_command!(launcher_start, start);
action_command!(launcher_stop, stop);
action_command!(launcher_repair, repair);
action_command!(launcher_install_update, install_update);
action_command!(launcher_cancel_and_exit, exit);
action_command!(launcher_cancel, cancel_task);
#[tauri::command]
fn launcher_check_update(
    window: WebviewWindow,
    controller: State<'_, Arc<Launcher>>,
) -> Result<(), String> {
    authorize(&window, true)?;
    controller.check(false).map_err(|e| e.to_string())
}
#[tauri::command]
fn launcher_open_webui(
    window: WebviewWindow,
    controller: State<'_, Arc<Launcher>>,
) -> Result<(), String> {
    authorize(&window, true)?;
    controller.open_webui();
    Ok(())
}
#[tauri::command]
fn launcher_show_home(window: WebviewWindow) -> Result<(), String> {
    authorize(&window, false)?;
    show_launcher_home(window.app_handle()).map_err(|e| e.to_string())?;
    if window.label() == "main" {
        let _ = window.destroy();
    }
    Ok(())
}
#[tauri::command]
fn launcher_back(
    window: WebviewWindow,
    controller: State<'_, Arc<Launcher>>,
) -> Result<(), String> {
    authorize(&window, true)?;
    controller.change(|m| {
        if !m.active && m.view.update_checked && m.update.is_none() {
            m.view.phase = if m.view.backend_running {
                Phase::Running
            } else {
                Phase::Idle
            };
            m.view.error = None;
            m.view.progress = None;
            m.touch();
        }
    });
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    #[cfg(windows)]
    {
        if updater::try_apply_launcher_update_from_args()? {
            return Ok(());
        }
        unsafe {
            use winapi::um::wincon::{AttachConsole, ATTACH_PARENT_PROCESS};
            if std::env::var_os(updater::LAUNCHER_UPDATE_NO_CONSOLE_ENV).is_some() {
                std::env::remove_var(updater::LAUNCHER_UPDATE_NO_CONSOLE_ENV);
            } else {
                window_util::HAS_CONSOLE
                    .store(AttachConsole(ATTACH_PARENT_PROCESS) != 0, Ordering::Relaxed);
            }
        }
    }
    setup_environment()?;
    let _log_guard = initialize_logging()?;
    crate::i18n::init();
    let allow_exit = Arc::new(AtomicBool::new(false));
    let allow_for_setup = allow_exit.clone();
    let allow_for_run = allow_exit.clone();
    let port = WebuiLaunchConfig::from_deploy_config(get_deploy_config().as_ref()).port;
    let preview_crash = preview_crash_arg_present();
    let minimized = start_minimized_arg_present();
    let auto_start = minimized || std::env::var_os("AZURPILOT_RESUME_START").is_some();
    std::env::remove_var("AZURPILOT_RESUME_START");
    info!("Starting React launcher {}", env!("CARGO_PKG_VERSION"));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(move |app, _, _| {
            let Some(controller) = app.try_state::<Arc<Launcher>>() else { return; };
            if controller.snapshot().backend_running { controller.open_webui(); }
            else { let _ = show_launcher_home(app); }
        }))
        .invoke_handler(tauri::generate_handler![
            launcher_context, launcher_snapshot, launcher_start, launcher_stop, launcher_repair,
            launcher_check_update, launcher_install_update, launcher_open_webui, launcher_show_home,
            launcher_cancel_and_exit, launcher_cancel, launcher_back,
            crate::save_as, crate::download_today_gui_log, crate::download_today_launcher_log, crate::retry_backend_connection,
            crate::window_hide, crate::window_minimize, crate::window_toggle_maximize, crate::window_close, crate::window_exit_application,
            crate::window_start_dragging, crate::window_is_maximized
        ])
        .setup(move |app| {
            let controller = Arc::new(Launcher { app: app.handle().clone(), model: Mutex::new(Model::new()), changed: Condvar::new(),
                backend: Mutex::new(None), session: Mutex::new(None), allow_exit: allow_for_setup.clone(),
                recreate: Arc::new(AtomicBool::new(false)), port, skip_update: preview_crash || preview_no_update_arg_present(), minimized });
            app.manage(controller.clone());
            #[cfg(target_os = "macos")]
            app.get_webview_window("launcher").unwrap().set_decorations(true)?;
            match time_bomb_expiration_message() {
                Ok(Some(message)) => {
                    allow_for_setup.store(true, Ordering::SeqCst);
                    let handle = app.handle().clone();
                    app.dialog().message(message).title(t!("dialog.test_ended")).show(move |_| handle.exit(0));
                    return Ok(());
                }
                Err(error) => warn!("Unable to verify test expiration: {error:#}"),
                _ => {}
            }
            create_tray(app)?;
            controller.dispatch();
            if !minimized { show_launcher_home(app.handle())?; }
            if preview_crash {
                controller.change(|m| {
                    m.view.update_checked = true; m.view.phase = Phase::Error;
                    m.view.operation = Some(Operation::Start);
                    m.view.error = Some(t!("splash.preview_crash_detail").to_string()); m.touch();
                });
                show_launcher_home(app.handle())?;
            } else { controller.check(auto_start)?; }
            let ctrl_controller = controller.clone();
            ctrlc::set_handler(move || { let _ = ctrl_controller.exit(); })?;
            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(move |app, event| match event {
            tauri::RunEvent::ExitRequested { api, .. } => {
                if !allow_for_run.load(Ordering::SeqCst) {
                    api.prevent_exit();
                    let controller = app.state::<Arc<Launcher>>();
                    controller.request_close();
                }
            }
            tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { api, .. }, .. } => {
                if allow_for_run.load(Ordering::SeqCst) { return; }
                api.prevent_close();
                if label == "launcher" { app.state::<Arc<Launcher>>().request_close(); }
                else if label == "main" {
                    #[cfg(windows)]
                    if let Some(window) = app.get_webview_window("main") {
                        if window.eval("if (window.__AZURPILOT_OPEN_CLOSE_PROMPT) window.__AZURPILOT_OPEN_CLOSE_PROMPT();").is_err() { minimize_main_window_to_tray(app); }
                    }
                    #[cfg(not(windows))]
                    minimize_main_window_to_tray(app);
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(controller) = app.try_state::<Arc<Launcher>>() { let _ = controller.stop_backend(); }
            }
            _ => {}
        });
    Ok(())
}

#[cfg(any(windows, target_os = "macos"))]
fn create_tray(app: &mut tauri::App) -> Result<()> {
    let home = MenuItemBuilder::new(t!("launcher.home"))
        .id("home")
        .build(app)?;
    let toggle = MenuItemBuilder::new(t!("tray.toggle_visibility"))
        .id("toggle")
        .build(app)?;
    let quit = MenuItemBuilder::new(t!("tray.quit"))
        .id("quit")
        .build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&home)
        .item(&toggle)
        .separator()
        .item(&quit)
        .build()?;
    TrayIconBuilder::with_id("main-tray")
        .icon(tray_icon_for_platform())
        .tooltip("AzurPilot")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| {
            let controller = app.state::<Arc<Launcher>>();
            match event.id().as_ref() {
                "home" => {
                    let _ = show_launcher_home(app);
                }
                "toggle" => {
                    if controller.snapshot().backend_running {
                        toggle_main_window_visibility(
                            app,
                            controller.port,
                            controller.recreate.clone(),
                        );
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
        })
        .on_tray_icon_event(|tray, event| {
            #[cfg(windows)]
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let controller = tray.app_handle().state::<Arc<Launcher>>();
                if controller.snapshot().backend_running {
                    controller.open_webui();
                } else {
                    let _ = show_launcher_home(tray.app_handle());
                }
            }
            #[cfg(not(windows))]
            let _ = (tray, event);
        })
        .build(app)?;
    Ok(())
}
#[cfg(not(any(windows, target_os = "macos")))]
fn create_tray(_app: &mut tauri::App) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn local_origin_check_supports_custom_schemes_and_rejects_other_origins() {
        for base in [
            "tauri://localhost/index.html",
            "http://tauri.localhost/index.html",
            "http://127.0.0.1:1420/index.html",
        ] {
            let url = tauri::Url::parse(base).unwrap();
            assert!(same_local_origin(&url, &tauri::Url::parse(base).unwrap()));
            assert!(!same_local_origin(
                &tauri::Url::parse("https://example.com/index.html").unwrap(),
                &url
            ));
        }
    }
    fn ready() -> Model {
        let mut m = Model::new();
        m.view.update_checked = true;
        m
    }
    #[test]
    fn tasks_are_mutually_exclusive_and_running_repair_is_forbidden() {
        let mut m = ready();
        let (task, _) = m.begin(Operation::Start).unwrap();
        assert!(m.begin(Operation::Repair).is_err());
        m.view.backend_running = true;
        m.finish(task, Ok(()));
        assert!(m.begin(Operation::Repair).is_err());
        assert!(m.begin(Operation::Start).is_err());
        let (stop, _) = m.begin(Operation::Stop).unwrap();
        m.view.backend_running = false;
        m.finish(stop, Ok(()));
        assert!(m.begin(Operation::Repair).is_ok());
    }
    #[test]
    fn update_gate_and_failed_checks_cannot_be_bypassed() {
        let mut m = Model::new();
        assert!(m.begin(Operation::Start).is_err());
        let (task, _) = m.begin(Operation::Check).unwrap();
        m.finish(task, Err(anyhow!("offline")));
        assert!(!m.snapshot().can_start);
        assert!(!m.snapshot().can_repair);
        assert!(m.snapshot().can_check);
    }
    #[test]
    fn cancellation_and_new_tasks_reject_late_progress() {
        let mut m = ready();
        let (old, cancel) = m.begin(Operation::Start).unwrap();
        m.publish(old, SplashUpdate::loading("first", "", 85));
        cancel.store(true, Ordering::SeqCst);
        m.publish(old, SplashUpdate::loading("late", "", 100));
        assert_eq!(m.view.progress.as_ref().unwrap().title, "first");
        m.finish(old, Err(anyhow!("cancelled")));
        let (new, _) = m.begin(Operation::Repair).unwrap();
        m.publish(old, SplashUpdate::loading("old", "", 100));
        m.publish(new, SplashUpdate::loading("repair", "", 5));
        assert_eq!(m.view.progress.as_ref().unwrap().progress, 5.0);
    }
    #[test]
    fn stale_completion_cannot_finish_the_next_task() {
        let mut m = ready();
        let (old, _) = m.begin(Operation::Start).unwrap();
        m.finish(old, Err(anyhow!("failed")));
        let (new, _) = m.begin(Operation::Start).unwrap();
        m.finish(old, Ok(()));
        assert!(m.active);
        assert_eq!(m.view.task_id, new);
    }

    #[test]
    fn stopped_service_can_start_a_fresh_session() {
        let mut m = ready();
        let (first, _) = m.begin(Operation::Start).unwrap();
        m.view.backend_running = true;
        m.finish(first, Ok(()));
        let (stop, _) = m.begin(Operation::Stop).unwrap();
        m.view.backend_running = false;
        m.finish(stop, Ok(()));
        let (next, _) = m.begin(Operation::Start).unwrap();
        assert!(next > first);
        assert!(m.view.progress.is_none());
        assert!(!m.accepts(first));
    }

    #[test]
    fn update_replacement_failure_releases_the_exit_barrier() {
        let mut m = ready();
        m.active = true;
        m.committing = true;
        m.view.task_id = 1;
        m.finish(1, Err(anyhow!("replacement denied")));
        assert!(!m.committing);
        assert!(!m.active);
        assert_eq!(m.view.phase, Phase::Error);
    }
}
