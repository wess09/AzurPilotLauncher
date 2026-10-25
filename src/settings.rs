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
    /// 减弱动态效果（暂停背景视频等界面动效）。
    pub reduced_motion: bool,
    /// 阻止计算机自动睡眠（持有系统电源锁）。
    pub prevent_sleep: bool,
    /// UI 语言偏好；None 表示跟随系统。
    pub language: Option<String>,
    /// 更新通道：正式版或金丝雀版（每次 main 构建）。
    pub update_channel: crate::updater::UpdateChannel,
    /// 更新下载源：自动 / 官方源 / GitHub / Cloudflare CDN。
    pub update_source: crate::updater::UpdateSource,
    #[serde(flatten)]
    pub sources: crate::sources::Preferences,
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
                let connection = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
                Self::ensure_table(&connection).expect("in-memory settings schema");
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
    ///
    /// value 列早期为 INTEGER（布尔），现也存语言码字符串；按 SQLite
    /// 动态类型分别解析，保证旧库平滑兼容。
    pub fn get(&self) -> Settings {
        let connection = self.connection.lock().unwrap();
        let raw = |key: &str| -> Option<rusqlite::types::Value> {
            connection
                .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                    row.get::<_, rusqlite::types::Value>(0)
                })
                .ok()
        };
        let as_bool = |value: Option<rusqlite::types::Value>| -> bool {
            match value {
                Some(rusqlite::types::Value::Integer(i)) => i != 0,
                Some(rusqlite::types::Value::Text(t)) => t == "1",
                _ => false,
            }
        };
        let language = raw("language").and_then(|value| match value {
            rusqlite::types::Value::Text(t) if !t.is_empty() => Some(t),
            _ => None,
        });
        Settings {
            launch_on_startup: as_bool(raw("launchOnStartup")),
            auto_start_service: as_bool(raw("autoStartService")),
            keep_webview_on_tray: as_bool(raw("keepWebviewOnTray")),
            reduced_motion: as_bool(raw("reducedMotion")),
            prevent_sleep: as_bool(raw("preventSleep")),
            update_channel: match raw("updateChannel") {
                Some(rusqlite::types::Value::Text(ref text)) if text == "canary" => {
                    crate::updater::UpdateChannel::Canary
                }
                _ => crate::updater::UpdateChannel::Stable,
            },
            update_source: match raw("updateSource") {
                Some(rusqlite::types::Value::Text(ref text)) => match text.as_str() {
                    "official" => crate::updater::UpdateSource::Official,
                    "github" => crate::updater::UpdateSource::Github,
                    "cloudflare" => crate::updater::UpdateSource::Cloudflare,
                    _ => crate::updater::UpdateSource::Auto,
                },
                _ => crate::updater::UpdateSource::Auto,
            },
            language,
            sources: crate::sources::Preferences {
                download_source: read_source(raw("downloadSource")),
                python_source: read_source(raw("pythonSource")),
                pypi_source: read_source(raw("pypiSource")),
            },
        }
    }

    /// 保存设置；开机自启等副作用由调用方执行。
    ///
    /// # Errors
    /// SQLite 写入失败时返回错误。
    pub fn set(&self, settings: &Settings) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let upsert = |key: &str, value: &str| -> Result<()> {
            transaction
                .execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    rusqlite::params![key, value],
                )
                .map_err(anyhow::Error::from)?;
            Ok(())
        };
        upsert(
            "launchOnStartup",
            if settings.launch_on_startup { "1" } else { "0" },
        )?;
        upsert(
            "autoStartService",
            if settings.auto_start_service {
                "1"
            } else {
                "0"
            },
        )?;
        upsert(
            "keepWebviewOnTray",
            if settings.keep_webview_on_tray {
                "1"
            } else {
                "0"
            },
        )?;
        upsert(
            "reducedMotion",
            if settings.reduced_motion { "1" } else { "0" },
        )?;
        upsert(
            "preventSleep",
            if settings.prevent_sleep { "1" } else { "0" },
        )?;
        upsert(
            "updateChannel",
            match settings.update_channel {
                crate::updater::UpdateChannel::Stable => "stable",
                crate::updater::UpdateChannel::Canary => "canary",
            },
        )?;
        upsert(
            "updateSource",
            match settings.update_source {
                crate::updater::UpdateSource::Auto => "auto",
                crate::updater::UpdateSource::Official => "official",
                crate::updater::UpdateSource::Github => "github",
                crate::updater::UpdateSource::Cloudflare => "cloudflare",
            },
        )?;
        match settings.language.as_deref() {
            Some(language) => upsert("language", language)?,
            None => {
                transaction
                    .execute("DELETE FROM settings WHERE key = 'language'", [])
                    .map_err(anyhow::Error::from)?;
            }
        }
        for (key, value) in [
            (
                "downloadSource",
                serde_json::to_value(settings.sources.download_source)?,
            ),
            (
                "pythonSource",
                serde_json::to_value(settings.sources.python_source)?,
            ),
            (
                "pypiSource",
                serde_json::to_value(settings.sources.pypi_source)?,
            ),
        ] {
            upsert(
                key,
                value.as_str().expect("source enum serializes as string"),
            )?;
        }
        transaction.commit()?;
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
        assert!(!store.get().reduced_motion);
        assert!(!store.get().prevent_sleep);
        let next = Settings {
            launch_on_startup: true,
            auto_start_service: false,
            keep_webview_on_tray: true,
            reduced_motion: true,
            prevent_sleep: true,
            update_channel: crate::updater::UpdateChannel::Canary,
            update_source: crate::updater::UpdateSource::Cloudflare,
            language: Some("zh-CN".to_string()),
            sources: crate::sources::Preferences::default(),
        };
        store.set(&next).expect("save settings");
        let read = store.get();
        assert!(read.launch_on_startup);
        assert!(read.keep_webview_on_tray);
        assert!(read.reduced_motion);
        assert!(read.prevent_sleep);
        assert_eq!(read.update_channel, crate::updater::UpdateChannel::Canary);
        assert_eq!(read.update_source, crate::updater::UpdateSource::Cloudflare);
        assert_eq!(read.language.as_deref(), Some("zh-CN"));
    }
    #[test]
    fn mirror_preferences_round_trip_and_legacy_records_default_to_auto() {
        use crate::sources::*;
        let store = Store::in_memory();
        assert_eq!(store.get().sources.download_source, DownloadSource::Auto);
        let mut next = store.get();
        next.sources = Preferences {
            download_source: DownloadSource::Github,
            python_source: PythonSource::Npmmirror,
            pypi_source: PypiSource::Ustc,
        };
        store.set(&next).unwrap();
        let read = store.get().sources;
        assert_eq!(read.download_source, DownloadSource::Github);
        assert_eq!(read.python_source, PythonSource::Npmmirror);
        assert_eq!(read.pypi_source, PypiSource::Ustc);
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE settings SET value = 'invalid' WHERE key = 'pythonSource'",
                [],
            )
            .unwrap();
        assert_eq!(store.get().sources.python_source, PythonSource::Auto);
        let mut serialized = serde_json::to_value(next).unwrap();
        serialized["pythonSource"] = "https://example.com/script".into();
        assert!(serde_json::from_value::<Settings>(serialized).is_err());
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
pub(crate) fn settings_get(
    window: tauri::WebviewWindow,
    store: tauri::State<'_, Arc<Store>>,
) -> Result<Settings, String> {
    crate::launcher::authorize(&window, false)?;
    Ok(store.get())
}

/// Tauri 命令：保存设置并应用开机自启副作用。
///
/// # Errors
/// SQLite 写入失败，或开机自启注册失败（含非 Windows 平台不支持）时
/// 返回错误字符串，前端据此回滚开关。
#[tauri::command]
pub(crate) fn settings_set(
    window: tauri::WebviewWindow,
    store: tauri::State<'_, Arc<Store>>,
    controller: tauri::State<'_, Arc<crate::launcher::Launcher>>,
    settings: Settings,
) -> Result<Settings, String> {
    crate::launcher::authorize(&window, true)?;
    let previous = store.get();
    store.set(&settings).map_err(|e| e.to_string())?;
    if previous.launch_on_startup != settings.launch_on_startup {
        if let Err(error) = crate::autostart::set_enabled(settings.launch_on_startup) {
            let _ = store.set(&previous);
            return Err(error.to_string());
        }
    }
    if previous.prevent_sleep != settings.prevent_sleep {
        if let Err(error) = crate::power::set_prevent_sleep(settings.prevent_sleep) {
            let _ = store.set(&previous);
            return Err(error.to_string());
        }
    }
    if previous.update_channel != settings.update_channel {
        // 通道切换后立即静默检查一次更新，结果反映到首页，由用户手动确认安装。
        controller.request_update_check();
    }
    crate::i18n::apply_override(settings.language.as_deref());
    Ok(store.get())
}

fn read_source<T: serde::de::DeserializeOwned + Default>(
    value: Option<rusqlite::types::Value>,
) -> T {
    match value {
        Some(rusqlite::types::Value::Text(text)) => {
            serde_json::from_value(serde_json::Value::String(text)).unwrap_or_default()
        }
        _ => T::default(),
    }
}
