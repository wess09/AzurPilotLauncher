//! 启动器行为设置：SQLite key/value 存储、Tauri 命令与开机自启副作用。
//!
//! 数据库位于系统数据目录（`dirs::data_dir()/AzurPilot/launcher.db`），
//! 打开或建表失败时自动降级为内存数据库，保证启动器始终可用。

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::warn;

/// 启动器行为设置；序列化为 camelCase 与前端对齐。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// 开机自启。
    pub launch_on_startup: bool,
    /// 启动器启动时直接启动服务。
    pub auto_start_service: bool,
    /// 托盘化时保留 WebView（不销毁主窗口）。
    pub keep_webview_on_tray: bool,
}

/// SQLite 设置存储：内部互斥锁保护连接。
pub struct Store {
    connection: Mutex<rusqlite::Connection>,
}

impl Store {
    /// 打开（必要时创建）设置数据库并确保表结构；失败时降级内存库。
    pub fn new() -> Self {
        match Self::open() {
            Ok(store) => store,
            Err(e) => {
                warn!("Failed to open settings database, falling back to memory: {e:#}");
                let connection =
                    rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
                Self {
                    connection: Mutex::new(connection),
                }
            }
        }
    }

    /// 打开磁盘数据库并建表；父目录不存在时自动创建。
    ///
    /// # Errors
    /// 数据目录创建或 SQLite 打开/建表失败时返回错误。
    fn open() -> Result<Self> {
        let path = database_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = rusqlite::Connection::open(path)?;
        Self::ensure_table(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// 建表（幂等）。
    ///
    /// # Errors
    /// SQLite 建表失败时返回错误。
    fn ensure_table(connection: &rusqlite::Connection) -> Result<()> {
        connection.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value INTEGER NOT NULL
            )",
            [],
        )?;
        Ok(())
    }

    /// 读取全部设置；缺失的键按默认值（关闭）返回。
    pub fn get(&self) -> Settings {
        let connection = self.connection.lock().unwrap();
        let read = |key: &str| -> bool {
            connection
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    [key],
                    |row| row.get::<_, i64>(0),
                )
                .map(|value| value != 0)
                .unwrap_or(false)
        };
        Settings {
            launch_on_startup: read("launchOnStartup"),
            auto_start_service: read("autoStartService"),
            keep_webview_on_tray: read("keepWebviewOnTray"),
        }
    }

    /// 保存设置；开机自启等副作用由调用方执行。
    ///
    /// # Errors
    /// SQLite 写入失败时返回错误。
    pub fn set(&self, settings: &Settings) -> Result<()> {
        let connection = self.connection.lock().unwrap();
        for (key, value) in [
            ("launchOnStartup", settings.launch_on_startup),
            ("autoStartService", settings.auto_start_service),
            ("keepWebviewOnTray", settings.keep_webview_on_tray),
        ] {
            connection.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, i64::from(value)],
            )?;
        }
        Ok(())
    }

    /// 托盘化时是否保留 WebView。
    pub fn keep_webview_on_tray(&self) -> bool {
        self.get().keep_webview_on_tray
    }

    /// 启动器启动时是否直接启动服务。
    pub fn auto_start_service(&self) -> bool {
        self.get().auto_start_service
    }

    /// 测试专用：内存数据库实例，不触碰用户数据目录。
    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        let connection = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        Self::ensure_table(&connection).expect("create settings table");
        Self {
            connection: Mutex::new(connection),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_disabled_and_round_trip() {
        let store = Store::in_memory();
        assert!(!store.get().launch_on_startup);
        assert!(!store.get().auto_start_service);
        assert!(!store.get().keep_webview_on_tray);
        let next = Settings {
            launch_on_startup: true,
            auto_start_service: false,
            keep_webview_on_tray: true,
        };
        store.set(&next).expect("save settings");
        let read = store.get();
        assert!(read.launch_on_startup);
        assert!(!read.auto_start_service);
        assert!(read.keep_webview_on_tray);
    }
}

/// 设置数据库路径：系统数据目录/AzurPilot/launcher.db。
fn database_path() -> Result<PathBuf> {
    let base = dirs::data_dir()
        .ok_or_else(|| anyhow!("Unable to resolve the application data directory"))?;
    Ok(base.join("AzurPilot").join("launcher.db"))
}

/// Tauri 命令：读取全部设置。
#[tauri::command]
pub(crate) fn settings_get(store: tauri::State<'_, Arc<Store>>) -> Settings {
    store.get()
}

/// Tauri 命令：保存设置并应用开机自启副作用。
///
/// # Errors
/// SQLite 写入失败，或开机自启注册失败（含非 Windows 平台不支持）时
/// 返回错误字符串，前端据此回滚开关。
#[tauri::command]
pub(crate) fn settings_set(
    store: tauri::State<'_, Arc<Store>>,
    settings: Settings,
) -> Result<Settings, String> {
    let previous = store.get();
    store.set(&settings).map_err(|e| e.to_string())?;
    if previous.launch_on_startup != settings.launch_on_startup {
        crate::autostart::set_enabled(settings.launch_on_startup)
            .map_err(|e| e.to_string())?;
    }
    Ok(store.get())
}
