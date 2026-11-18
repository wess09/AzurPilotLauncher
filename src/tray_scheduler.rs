//! 托盘菜单「暂停任务 / 继续任务」。
//!
//! 经启动器 WS 调用后端 `scheduler.state` / `scheduler.pause` / `scheduler.resume`：
//! 暂停会停止所有运行中的实例并由后端记住，继续时重新启动它们。后台线程每隔几秒
//! 刷新菜单文字；后端未运行或不支持这些方法时菜单项置灰。

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use rust_i18n::t;
use serde_json::Value;
use tauri::{menu::MenuItem, AppHandle, Manager, Runtime};
use tracing::{info, warn};

use crate::launcher::Launcher;

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
/// 停止实例要等子进程退出，给足时间。
const ACTION_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SchedulerState {
    Running,
    Stopped,
    Unavailable,
}

/// 有暂停记录时优先继续，避免再次暂停覆盖上一次的记录。
pub(crate) fn parse_state(result: &Value) -> SchedulerState {
    let Some(running) = result.get("running").and_then(Value::as_array) else {
        return SchedulerState::Unavailable;
    };
    let has_paused = result
        .get("paused")
        .and_then(Value::as_array)
        .is_some_and(|paused| !paused.is_empty());
    if running.is_empty() || has_paused {
        SchedulerState::Stopped
    } else {
        SchedulerState::Running
    }
}

fn call<R: Runtime>(
    app: &AppHandle<R>,
    method: &'static str,
    timeout: Duration,
) -> anyhow::Result<Value> {
    let launcher = app
        .try_state::<Arc<Launcher>>()
        .ok_or_else(|| anyhow::anyhow!("launcher not ready"))?;
    launcher.backend_call(method, timeout)
}

fn query<R: Runtime>(app: &AppHandle<R>) -> SchedulerState {
    call(app, "scheduler.state", QUERY_TIMEOUT)
        .map(|result| parse_state(&result))
        .unwrap_or(SchedulerState::Unavailable)
}

fn label(state: SchedulerState) -> String {
    match state {
        SchedulerState::Running => t!("tray.pause_scheduler").to_string(),
        SchedulerState::Stopped => t!("tray.resume_scheduler").to_string(),
        SchedulerState::Unavailable => t!("tray.scheduler_unavailable").to_string(),
    }
}

fn apply<R: Runtime>(item: &MenuItem<R>, state: SchedulerState) {
    let _ = item.set_text(label(state));
    let _ = item.set_enabled(state != SchedulerState::Unavailable);
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// 菜单项控制器：轮询状态并处理点击；执行期间暂停轮询，避免文字来回跳。
pub(crate) struct TraySchedulerToggle<R: Runtime> {
    app: AppHandle<R>,
    item: MenuItem<R>,
    busy: Arc<AtomicBool>,
    poll_cache_dirty: Arc<AtomicBool>,
}

// 手写 Clone：derive 会额外要求 `R: Clone`，而 Runtime 并不实现 Clone。
impl<R: Runtime> Clone for TraySchedulerToggle<R> {
    fn clone(&self) -> Self {
        Self {
            app: self.app.clone(),
            item: self.item.clone(),
            busy: self.busy.clone(),
            poll_cache_dirty: self.poll_cache_dirty.clone(),
        }
    }
}

impl<R: Runtime> TraySchedulerToggle<R> {
    pub(crate) fn new(app: AppHandle<R>, item: MenuItem<R>) -> Self {
        Self {
            app,
            item,
            busy: Arc::new(AtomicBool::new(false)),
            poll_cache_dirty: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn spawn_poller(&self) {
        let this = self.clone();
        thread::spawn(move || {
            let mut last = None;
            loop {
                if !this.busy.load(Ordering::SeqCst) {
                    if this.poll_cache_dirty.swap(false, Ordering::SeqCst) {
                        last = None;
                    }
                    let state = query(&this.app);
                    if !this.busy.load(Ordering::SeqCst)
                        && !this.poll_cache_dirty.load(Ordering::SeqCst)
                        && last != Some(state)
                    {
                        apply(&this.item, state);
                        last = Some(state);
                    }
                }
                thread::sleep(POLL_INTERVAL);
            }
        });
    }

    pub(crate) fn toggle(&self) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.poll_cache_dirty.store(true, Ordering::SeqCst);
        let this = self.clone();
        thread::spawn(move || {
            let (method, pending) = match query(&this.app) {
                SchedulerState::Running => ("scheduler.pause", t!("tray.pausing_scheduler")),
                SchedulerState::Stopped => ("scheduler.resume", t!("tray.resuming_scheduler")),
                SchedulerState::Unavailable => {
                    apply(&this.item, SchedulerState::Unavailable);
                    this.busy.store(false, Ordering::SeqCst);
                    return;
                }
            };
            let _ = this.item.set_text(pending.to_string());
            let _ = this.item.set_enabled(false);
            match call(&this.app, method, ACTION_TIMEOUT) {
                Ok(result) => {
                    let errors = result.get("errors").and_then(Value::as_array);
                    if errors.is_some_and(|errors| !errors.is_empty()) {
                        warn!(method, ?errors, "Tray scheduler action partly failed");
                    } else {
                        info!(method, "Tray scheduler action succeeded");
                    }
                    apply(&this.item, parse_state(&result));
                }
                Err(err) => {
                    warn!(method, "Tray scheduler action failed: {err:#}");
                    apply(&this.item, query(&this.app));
                }
            }
            this.busy.store(false, Ordering::SeqCst);
        });
    }
}

/// Windows 自绘托盘菜单用：无状态的查询与切换（没有原生 MenuItem 可更新）。
static DETACHED_BUSY: AtomicBool = AtomicBool::new(false);

/// 返回 `running` / `stopped` / `unavailable` / `busy`，供前端菜单决定文字与可用性。
pub(crate) fn detached_state<R: Runtime>(app: &AppHandle<R>) -> &'static str {
    if DETACHED_BUSY.load(Ordering::SeqCst) {
        return "busy";
    }
    match query(app) {
        SchedulerState::Running => "running",
        SchedulerState::Stopped => "stopped",
        SchedulerState::Unavailable => "unavailable",
    }
}

pub(crate) fn detached_toggle<R: Runtime>(app: &AppHandle<R>) {
    if DETACHED_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    thread::spawn(move || {
        let method = match query(&app) {
            SchedulerState::Running => Some("scheduler.pause"),
            SchedulerState::Stopped => Some("scheduler.resume"),
            SchedulerState::Unavailable => None,
        };
        if let Some(method) = method {
            match call(&app, method, ACTION_TIMEOUT) {
                Ok(result) => {
                    let errors = result.get("errors").and_then(Value::as_array);
                    if errors.is_some_and(|errors| !errors.is_empty()) {
                        warn!(method, ?errors, "Tray scheduler action partly failed");
                    } else {
                        info!(method, "Tray scheduler action succeeded");
                    }
                }
                Err(err) => warn!(method, "Tray scheduler action failed: {err:#}"),
            }
        }
        DETACHED_BUSY.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_state_reads_running_list() {
        assert_eq!(
            parse_state(&json!({"running":["alas"],"paused":[],"errors":[]})),
            SchedulerState::Running
        );
        assert_eq!(
            parse_state(&json!({"running":[],"paused":["alas"],"errors":[]})),
            SchedulerState::Stopped
        );
        assert_eq!(parse_state(&json!({})), SchedulerState::Unavailable);
    }

    #[test]
    fn partially_paused_scheduler_offers_resume() {
        assert_eq!(
            parse_state(&json!({"running":["a"],"paused":["b"],"errors":["a failed to stop"]})),
            SchedulerState::Stopped
        );
    }
}
