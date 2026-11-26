//! 启动器行为设置：SQLite key/value 存储、Tauri 命令与开机自启副作用。
//!
//! 数据库位于系统数据目录（`dirs::data_dir()/AzurPilot/launcher.db`），
//! 打开或建表失败时自动降级为内存数据库，保证启动器始终可用。

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri_plugin_dialog::DialogExt;
use tracing::{info, warn};

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
    /// 关闭背景视频（改用静态图片作为背景）。
    #[serde(default)]
    pub disable_background_video: bool,
    /// 阻止计算机自动睡眠（持有系统电源锁）。
    pub prevent_sleep: bool,
    /// 背景视频播放音频（默认关闭）。
    pub audio_enabled: bool,
    /// 跳过启动器更新：不检查更新，也不因待更新/检查失败阻断启动。
    #[serde(default)]
    pub skip_update: bool,
    /// UI 语言偏好；None 表示跟随系统。
    pub language: Option<String>,
    /// 自定义强调色（#RRGGBB）；None 表示跟随系统默认主题色。
    #[serde(default)]
    pub accent_color: Option<String>,
    /// 自定义背景图片路径（应用数据目录内的文件）；None 表示使用内置图片。
    #[serde(default)]
    pub custom_background_image: Option<String>,
    /// 自定义背景视频路径（应用数据目录内的文件）；None 表示使用内置视频。
    #[serde(default)]
    pub custom_background_video: Option<String>,
    /// 更新通道：正式版或金丝雀版（每次 main 构建）。
    pub update_channel: crate::updater::UpdateChannel,
    /// 启动器更新下载源。
    pub update_source: crate::updater::UpdateSource,
    #[serde(flatten)]
    pub sources: crate::sources::Preferences,
    /// 启用 Mirror 酱 Git Pack 本体源码更新。
    #[serde(default)]
    pub use_mirror_chyan: bool,
    /// 启动器自更新与本体源码更新共用的 Mirror 酱 CDK。
    #[serde(default)]
    pub mirror_chyan_cdk: Option<String>,
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
        let as_string = |value: Option<rusqlite::types::Value>| -> Option<String> {
            match value {
                Some(rusqlite::types::Value::Text(t)) if !t.is_empty() => Some(t),
                _ => None,
            }
        };
        let language = as_string(raw("language"));
        Settings {
            launch_on_startup: as_bool(raw("launchOnStartup")),
            auto_start_service: as_bool(raw("autoStartService")),
            keep_webview_on_tray: as_bool(raw("keepWebviewOnTray")),
            reduced_motion: as_bool(raw("reducedMotion")),
            disable_background_video: as_bool(raw("disableBackgroundVideo")),
            prevent_sleep: as_bool(raw("preventSleep")),
            audio_enabled: as_bool(raw("audioEnabled")),
            skip_update: as_bool(raw("skipUpdate")),
            update_channel: match raw("updateChannel") {
                Some(rusqlite::types::Value::Text(ref text)) if text == "canary" => {
                    crate::updater::UpdateChannel::Canary
                }
                _ => crate::updater::UpdateChannel::Stable,
            },
            update_source: match raw("updateSource") {
                Some(rusqlite::types::Value::Text(ref text)) => match text.as_str() {
                    "official" | "edgeone" => crate::updater::UpdateSource::Official,
                    "github" => crate::updater::UpdateSource::Github,
                    "cloudflare" => crate::updater::UpdateSource::Cloudflare,
                    "cnb" => crate::updater::UpdateSource::Cnb,
                    "gitcode" => crate::updater::UpdateSource::Gitcode,
                    "mirrorchyan" => crate::updater::UpdateSource::MirrorChyan,
                    _ => crate::updater::UpdateSource::Auto,
                },
                _ => crate::updater::UpdateSource::Auto,
            },
            language,
            accent_color: as_string(raw("accentColor")),
            custom_background_image: as_string(raw("customBackgroundImage")),
            custom_background_video: as_string(raw("customBackgroundVideo")),
            sources: crate::sources::Preferences {
                download_source: read_source(raw("downloadSource")),
                python_source: read_source(raw("pythonSource")),
                nodejs_source: read_source(raw("nodejsSource")),
                pypi_source: read_source(raw("pypiSource")),
            },
            use_mirror_chyan: as_bool(raw("useMirrorChyan")),
            mirror_chyan_cdk: as_string(raw("mirrorChyanCdk")),
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
            "disableBackgroundVideo",
            if settings.disable_background_video {
                "1"
            } else {
                "0"
            },
        )?;
        upsert(
            "preventSleep",
            if settings.prevent_sleep { "1" } else { "0" },
        )?;
        upsert(
            "audioEnabled",
            if settings.audio_enabled { "1" } else { "0" },
        )?;
        upsert(
            "skipUpdate",
            if settings.skip_update {
                "1"
            } else {
                "0"
            },
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
                crate::updater::UpdateSource::Cnb => "cnb",
                crate::updater::UpdateSource::Gitcode => "gitcode",
                crate::updater::UpdateSource::MirrorChyan => "mirrorchyan",
            },
        )?;
        upsert(
            "useMirrorChyan",
            if settings.use_mirror_chyan { "1" } else { "0" },
        )?;
        for (key, value) in [
            ("language", settings.language.as_deref()),
            ("accentColor", settings.accent_color.as_deref()),
            (
                "customBackgroundImage",
                settings.custom_background_image.as_deref(),
            ),
            (
                "customBackgroundVideo",
                settings.custom_background_video.as_deref(),
            ),
            (
                "mirrorChyanCdk",
                settings
                    .mirror_chyan_cdk
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
            ),
        ] {
            match value {
                Some(value) => upsert(key, value)?,
                None => {
                    transaction
                        .execute("DELETE FROM settings WHERE key = ?1", [key])
                        .map_err(anyhow::Error::from)?;
                }
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
                "nodejsSource",
                serde_json::to_value(settings.sources.nodejs_source)?,
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
        assert!(!store.get().disable_background_video);
        assert!(!store.get().prevent_sleep);
        assert!(!store.get().audio_enabled);
        assert!(!store.get().skip_update);
        assert!(!store.get().use_mirror_chyan);
        assert!(store.get().mirror_chyan_cdk.is_none());
        let next = Settings {
            launch_on_startup: true,
            auto_start_service: false,
            keep_webview_on_tray: true,
            reduced_motion: true,
            disable_background_video: true,
            prevent_sleep: true,
            audio_enabled: true,
            skip_update: true,
            update_channel: crate::updater::UpdateChannel::Canary,
            update_source: crate::updater::UpdateSource::MirrorChyan,
            language: Some("zh-CN".to_string()),
            accent_color: Some("#ff5500".to_string()),
            custom_background_image: Some("image.png".to_string()),
            custom_background_video: Some("video.mp4".to_string()),
            sources: crate::sources::Preferences::default(),
            use_mirror_chyan: true,
            mirror_chyan_cdk: Some("cdk-test".to_string()),
        };
        store.set(&next).expect("save settings");
        let read = store.get();
        assert!(read.launch_on_startup);
        assert!(read.keep_webview_on_tray);
        assert!(read.reduced_motion);
        assert!(read.disable_background_video);
        assert!(read.prevent_sleep);
        assert!(read.audio_enabled);
        assert!(read.skip_update);
        assert_eq!(read.update_channel, crate::updater::UpdateChannel::Canary);
        assert_eq!(read.update_source, crate::updater::UpdateSource::MirrorChyan);
        assert_eq!(read.language.as_deref(), Some("zh-CN"));
        assert_eq!(read.accent_color.as_deref(), Some("#ff5500"));
        assert_eq!(read.custom_background_image.as_deref(), Some("image.png"));
        assert_eq!(read.custom_background_video.as_deref(), Some("video.mp4"));
        assert!(read.use_mirror_chyan);
        assert_eq!(read.mirror_chyan_cdk.as_deref(), Some("cdk-test"));
    }

    #[test]
    fn mirror_settings_sync_to_git_section_in_the_bootstrap_format() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config/deploy.yaml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let original = "Deploy:\n  Git:\n    Branch: master\n  Webui:\n    WebuiPort: 22267\n";
        std::fs::write(&config, original).unwrap();

        assert!(!sync_mirror_chyan_to_deploy_yaml(root.path(), true, Some("cdk-test")).unwrap());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);

        let client = root.path().join("deploy/mirrorchyan/client.py");
        std::fs::create_dir_all(client.parent().unwrap()).unwrap();
        std::fs::write(client, "").unwrap();
        assert!(sync_mirror_chyan_to_deploy_yaml(root.path(), true, Some(" cdk-test ")).unwrap());
        let updated = std::fs::read_to_string(&config).unwrap();
        assert!(updated.contains("    MirrorChyanCdk: 'cdk-test'\n"));
        let parsed: serde_json::Value = serde_yaml::from_str(&updated).unwrap();
        assert_eq!(parsed["Deploy"]["Git"]["UseMirrorChyan"], true);
        assert_eq!(parsed["Deploy"]["Git"]["MirrorChyanCdk"], "cdk-test");
        assert_eq!(parsed["Deploy"]["Webui"]["WebuiPort"], 22267);

        assert!(sync_mirror_chyan_to_deploy_yaml(root.path(), false, None).unwrap());
        let disabled = std::fs::read_to_string(&config).unwrap();
        assert_eq!(disabled.matches("MirrorChyanCdk:").count(), 1);
        assert!(disabled.contains("    UseMirrorChyan: false\n    MirrorChyanCdk: null\n"));
    }

    #[test]
    fn mirror_preferences_round_trip_and_legacy_records_default_to_auto() {
        use crate::sources::*;
        let store = Store::in_memory();
        assert_eq!(store.get().sources.download_source, DownloadSource::Auto);
        assert_eq!(store.get().sources.nodejs_source, NodejsSource::Auto);
        let mut next = store.get();
        next.sources = Preferences {
            download_source: DownloadSource::Github,
            python_source: PythonSource::Npmmirror,
            nodejs_source: NodejsSource::Nju,
            pypi_source: PypiSource::Ustc,
        };
        store.set(&next).unwrap();
        let read = store.get().sources;
        assert_eq!(read.download_source, DownloadSource::Github);
        assert_eq!(read.python_source, PythonSource::Npmmirror);
        assert_eq!(read.nodejs_source, NodejsSource::Nju);
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

    #[test]
    fn legacy_settings_default_nodejs_source_and_reject_custom_download_urls() {
        use crate::sources::NodejsSource;
        let store = Store::in_memory();
        let mut settings = store.get();
        settings.sources.nodejs_source = NodejsSource::Npmmirror;
        store.set(&settings).unwrap();
        assert_eq!(store.get().sources.nodejs_source, NodejsSource::Npmmirror);
        let mut serialized = serde_json::to_value(settings).unwrap();
        serialized.as_object_mut().unwrap().remove("nodejsSource");
        assert_eq!(
            serde_json::from_value::<Settings>(serialized.clone())
                .unwrap()
                .sources
                .nodejs_source,
            NodejsSource::Auto
        );
        serialized["nodejsSource"] = "https://example.com/node.zip".into();
        assert!(serde_json::from_value::<Settings>(serialized).is_err());
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE settings SET value = 'invalid' WHERE key = 'nodejsSource'",
                [],
            )
            .unwrap();
        assert_eq!(store.get().sources.nodejs_source, NodejsSource::Auto);
    }

    #[test]
    fn replaces_background_only_after_a_successful_copy() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("selected.mp4");
        let destination = root.path().join("backgrounds").join("background_video.mp4");
        std::fs::write(&source, b"first video").unwrap();
        copy_background_file(&source, &destination).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"first video");

        assert!(copy_background_file(&root.path().join("missing.mp4"), &destination).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"first video");

        std::fs::write(&source, b"replacement video").unwrap();
        copy_background_file(&source, &destination).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"replacement video");
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
    if previous.skip_update != settings.skip_update {
        // 立即放行/重新封锁启动门禁，再跑一次检查同步首页的更新提示。
        controller.sync_skip_update(settings.skip_update);
        controller.request_update_check();
    }
    crate::i18n::apply_override(settings.language.as_deref());
    let saved = store.get();
    if previous.use_mirror_chyan != saved.use_mirror_chyan
        || previous.mirror_chyan_cdk != saved.mirror_chyan_cdk
    {
        if let Err(error) = sync_mirror_chyan_to_deploy_yaml(
            &crate::setup::azurpilot_repo_dir(),
            saved.use_mirror_chyan,
            saved.mirror_chyan_cdk.as_deref(),
        ) {
            warn!("Failed to sync MirrorChyan settings to config/deploy.yaml: {error:#}");
        }
    }
    Ok(saved)
}

/// 在系统浏览器打开 Mirror 酱官网。
#[tauri::command]
pub(crate) fn settings_open_mirror_chyan(window: tauri::WebviewWindow) -> Result<(), String> {
    const MIRROR_CHYAN_WEBSITE: &str = "https://mirrorchyan.com/?source=azurpilot_launcher_settings";

    crate::launcher::authorize(&window, true)?;
    #[cfg(target_os = "windows")]
    {
        use std::ptr::{null, null_mut};
        use winapi::um::{shellapi::ShellExecuteW, winuser::SW_SHOWNORMAL};

        let url: Vec<u16> = MIRROR_CHYAN_WEBSITE.encode_utf16().chain([0]).collect();
        // 将网址交给 Windows 注册的 HTTPS 协议处理程序。
        let status = unsafe {
            ShellExecuteW(
                null_mut(),
                null(),
                url.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if status <= 32 {
            return Err(rust_i18n::t!("errors.open_mirror_chyan_failed", error = status.to_string())
                .into_owned());
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        use crate::window_util::CreateNoWindow;
        use std::process::Command;

        #[cfg(target_os = "macos")]
        let program = "open";
        #[cfg(not(target_os = "macos"))]
        let program = "xdg-open";
        Command::new(program)
            .arg(MIRROR_CHYAN_WEBSITE)
            .create_no_window()
            .spawn()
            .map(|_| ())
            .map_err(|error| {
                rust_i18n::t!("errors.open_mirror_chyan_failed", error = error.to_string())
                    .into_owned()
            })
    }
}

/// 将 Mirror 酱配置同步到支持它的本体部署脚本。首次部署前仅保存在启动器数据库中。
pub(crate) fn sync_mirror_chyan_to_deploy_yaml(
    root: &Path,
    use_mirror_chyan: bool,
    mirror_chyan_cdk: Option<&str>,
) -> Result<bool> {
    // 旧版本的部署脚本会把未知配置项写入日志。
    if !root.join("deploy/mirrorchyan/client.py").is_file() {
        return Ok(false);
    }
    let directory = root.join("config");
    let path = directory.join("deploy.yaml");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let updated = apply_mirror_chyan_lines(&content, use_mirror_chyan, mirror_chyan_cdk)?;
    if updated != content {
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
        temporary.write_all(updated.as_bytes())?;
        temporary.persist(&path).map_err(|error| error.error)?;
    }
    Ok(true)
}

/// 按行更新 YAML 文本中的 `UseMirrorChyan` 与 `MirrorChyanCdk`，不输出明文 CDK。
fn apply_mirror_chyan_lines(
    content: &str,
    use_mirror_chyan: bool,
    mirror_chyan_cdk: Option<&str>,
) -> Result<String> {
    let bool_str = if use_mirror_chyan { "true" } else { "false" };
    let cdk_val = match mirror_chyan_cdk.map(str::trim).filter(|cdk| !cdk.is_empty()) {
        Some(cdk) => {
            if cdk.chars().any(char::is_control) {
                return Err(anyhow!("MirrorChyan CDK contains control characters"));
            }
            // 本体引导阶段的简易 YAML 读取器只剥除外层单引号。
            format!("'{}'", cdk.replace('\'', "''"))
        }
        None => "null".to_owned(),
    };

    let mut found_use = false;
    let mut found_cdk = false;
    let mut git_insertion = None;
    let mut output = String::with_capacity(content.len() + 64);

    for line in content.lines() {
        let indent_len = line.len() - line.trim_start().len();
        let indent = &line[..indent_len];
        let trimmed = line.trim_start();
        if trimmed.starts_with("UseMirrorChyan:") {
            found_use = true;
            output.push_str(indent);
            output.push_str("UseMirrorChyan: ");
            output.push_str(bool_str);
        } else if trimmed.starts_with("MirrorChyanCdk:") {
            found_cdk = true;
            output.push_str(indent);
            output.push_str("MirrorChyanCdk: ");
            output.push_str(&cdk_val);
        } else {
            output.push_str(line);
        }
        output.push('\n');
        if trimmed == "Git:" {
            git_insertion = Some((output.len(), indent_len + 2));
        }
    }

    let (position, indent) = git_insertion.unwrap_or((output.len(), 0));
    let mut missing = String::new();
    if !found_use {
        missing.push_str(&format!("{:indent$}UseMirrorChyan: {bool_str}\n", ""));
    }
    if !found_cdk {
        missing.push_str(&format!("{:indent$}MirrorChyanCdk: {cdk_val}\n", ""));
    }
    output.insert_str(position, &missing);
    Ok(output)
}

/// 弹出文件对话框选择背景图片/视频，复制到应用数据目录并返回其绝对路径。
/// 取消选择时返回 None。
#[tauri::command]
pub(crate) async fn pick_background_file(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    kind: String,
) -> std::result::Result<Option<String>, String> {
    crate::launcher::authorize(&window, true)?;
    let (filter_name, extensions, stem) = match kind.as_str() {
        "image" => (
            "Images",
            vec!["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"],
            "background_image",
        ),
        "video" => (
            "Videos",
            vec!["mp4", "webm", "mkv", "avi", "mov"],
            "background_video",
        ),
        _ => return Err(format!("Unsupported background kind: {kind}")),
    };
    let builder = app
        .dialog()
        .file()
        .set_parent(&window)
        .add_filter(filter_name, &extensions);
    let (sender, receiver) = std::sync::mpsc::channel();
    builder.pick_file(move |path| {
        let _ = sender.send(path.map(|p| p.to_string()));
    });
    let picked = tauri::async_runtime::spawn_blocking(move || receiver.recv())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let Some(path) = picked else {
        return Ok(None);
    };
    let source = PathBuf::from(path);
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("bin");
    let directory = database_path()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or_else(|| "Unable to resolve background directory".to_string())?
        .join("backgrounds");
    let destination = directory.join(format!("{stem}.{extension}"));
    tauri::async_runtime::spawn_blocking(move || {
        copy_background_file(&source, &destination).map_err(|e| e.to_string())?;
        info!("Copied custom {kind} background to {}", destination.display());
        Ok(Some(destination.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn copy_background_file(source: &Path, destination: &Path) -> Result<()> {
    let directory = destination
        .parent()
        .ok_or_else(|| anyhow!("Unable to resolve background directory"))?;
    std::fs::create_dir_all(directory)?;
    // 同格式替换时先完整复制，避免播放中的请求读到半写入的文件。
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    std::io::copy(&mut std::fs::File::open(source)?, &mut temporary)?;
    temporary.persist(destination)?;
    Ok(())
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
