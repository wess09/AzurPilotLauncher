//! 定位便携目录、按清单部署组件、维护后端配置。
pub use crate::progress::SplashUpdate;
use anyhow::{anyhow, bail, Context, Result};
use rust_i18n::t;
use serde_json::Value as JsonValue;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tracing::info;
const CLEANUP_RETRIES: usize = 20;

/// 首次运行允许目录为空，无需 deploy/installer.py。
pub fn azurpilot_repo_dir() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("AZURPILOT_REPO_DIR") {
        return PathBuf::from(root);
    }
    let folder = std::env::current_exe()
        .expect("launcher executable path")
        .parent()
        .expect("launcher directory")
        .to_path_buf();
    #[cfg(target_os = "macos")]
    if folder.file_name() == Some(std::ffi::OsStr::new("MacOS")) {
        return folder.parent().unwrap().join("AzurLaneAutoScript");
    }
    folder
}
fn venv_dir() -> PathBuf {
    azurpilot_repo_dir().join(".venv")
}
fn venv_bin_dir() -> PathBuf {
    venv_dir().join(if cfg!(windows) { "Scripts" } else { "bin" })
}
pub fn venv_python() -> PathBuf {
    venv_dir().join("runtime").join(if cfg!(windows) {
        "python.exe"
    } else {
        "bin/python3.14"
    })
}
pub fn setup_environment() -> Result<()> {
    let root = azurpilot_repo_dir();
    fs::create_dir_all(&root)?;
    std::env::set_current_dir(&root)?;
    let mut paths = vec![venv_bin_dir()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths)?);
    Ok(())
}
pub fn setup_azurpilot_repo(
    mut status_updater: impl FnMut(SplashUpdate),
    cancel_requested: Arc<AtomicBool>,
    skip_repository_update: bool,
    sources: &crate::sources::Preferences,
) -> Result<()> {
    crate::components::deploy(
        &azurpilot_repo_dir(),
        &mut status_updater,
        cancel_requested,
        skip_repository_update,
        sources,
    )?;
    migrate_dependency_config(sources)
}
pub fn repair_environment(
    mut status_updater: impl FnMut(SplashUpdate),
    cancel_requested: Arc<AtomicBool>,
    sources: &crate::sources::Preferences,
) -> Result<()> {
    crate::components::repair(
        &azurpilot_repo_dir(),
        &mut status_updater,
        cancel_requested,
        sources,
    )?;
    migrate_dependency_config(sources)
}
pub fn get_deploy_config() -> Option<JsonValue> {
    let config_content = fs::read_to_string("./config/deploy.yaml").ok()?;
    let config: JsonValue = serde_yaml::from_str(&config_content).ok()?;
    Some(config)
}

/// 配置仅供后端读取 WebUI/ADB 选项，不调用后端安装器。
fn migrate_dependency_config(sources: &crate::sources::Preferences) -> Result<()> {
    let path = azurpilot_repo_dir().join("config/deploy.yaml");
    fs::create_dir_all(path.parent().unwrap())?;
    let mut config: serde_yaml::Value = match fs::read_to_string(&path) {
        Ok(content) => serde_yaml::from_str(&content).context("Invalid config/deploy.yaml")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            serde_yaml::from_str(default_deploy_config())?
        }
        Err(error) => return Err(error.into()),
    };
    if !config.is_mapping() {
        bail!("Invalid config/deploy.yaml: expected mapping");
    }
    let deploy = &mut config["Deploy"];
    if !deploy.is_null() && !deploy.is_mapping() {
        bail!("Invalid Deploy section");
    }
    for key in ["Python", "Adb", "Git"] {
        if !deploy[key].is_null() && !deploy[key].is_mapping() {
            bail!("Invalid Deploy.{key} section");
        }
    }
    deploy["Python"]["PythonExecutable"] =
        serde_yaml::Value::String(venv_python().to_string_lossy().into_owned());
    deploy["Python"]["InstallDependencies"] = serde_yaml::Value::Bool(false);
    deploy["Adb"]["AdbExecutable"] = serde_yaml::Value::String(
        venv_bin_dir()
            .join(if cfg!(windows) { "adb.exe" } else { "adb" })
            .to_string_lossy()
            .into_owned(),
    );
    deploy["Python"]["PypiMirror"] =
        serde_yaml::Value::String(sources.pypi_source.index(sources.download_source).into());
    deploy["Git"]["AutoUpdate"] = serde_yaml::Value::Bool(false);
    fs::write(path, serde_yaml::to_string(&config)?)?;
    info!("Configured prebuilt runtime paths");
    Ok(())
}
pub(crate) fn remove_venv_entry_with_retry(
    path: &Path,
    cancel_requested: &AtomicBool,
) -> Result<()> {
    let mut last_error = None;
    for attempt in 0..CLEANUP_RETRIES {
        if cancel_requested.load(Ordering::SeqCst) {
            bail!(t!("launcher.cancelled"));
        }

        match remove_venv_entry(path, cancel_requested) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if !path.exists() {
                    return Ok(());
                }
                wait_for_venv_recovery_retry(
                    Duration::from_millis(250 + attempt as u64 * 100),
                    cancel_requested,
                )?;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        anyhow!(t!(
            "errors.delete_failed",
            error = path.display().to_string()
        ))
    }))
}

/// 递归删除 `.venv` 内的单个条目（恢复流程专用，逐步检查取消标志）。
///
/// 符号链接与 Reparse Point 只删链接本身、不递归目标——防止
/// `.venv` 内的链接指向目录外时误删外部文件。
///
/// # Errors
/// 用户取消或删除失败时返回 Err。
fn remove_venv_entry(path: &Path, cancel_requested: &AtomicBool) -> Result<()> {
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("launcher.cancelled"));
    }

    let metadata = fs::symlink_metadata(path)?;
    if is_symlink_or_reparse_point(&metadata) {
        return remove_venv_link_or_reparse_point(path, &metadata);
    }

    clear_readonly(path)?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            if cancel_requested.load(Ordering::SeqCst) {
                bail!(t!("launcher.cancelled"));
            }
            remove_venv_entry(&entry?.path(), cancel_requested)?;
        }
        fs::remove_dir(path).with_context(|| {
            t!(
                "errors.delete_dir_failed",
                error = path.display().to_string()
            )
        })?;
    } else {
        fs::remove_file(path).with_context(|| {
            t!(
                "errors.delete_file_failed",
                error = path.display().to_string()
            )
        })?;
    }
    Ok(())
}

fn default_deploy_config() -> &'static str {
    let cn = rust_i18n::locale().starts_with("zh");
    #[cfg(windows)]
    {
        if cn {
            include_str!("../deploy.windows-cn.yaml")
        } else {
            include_str!("../deploy.windows.yaml")
        }
    }
    #[cfg(target_os = "macos")]
    {
        if cn {
            include_str!("../deploy.mac-cn.yaml")
        } else {
            include_str!("../deploy.mac.yaml")
        }
    }
    #[cfg(target_os = "linux")]
    {
        if cn {
            include_str!("../deploy.unix-cn.yaml")
        } else {
            include_str!("../deploy.unix.yaml")
        }
    }
}

/// 判断给定元数据是否为符号链接或 Windows Reparse Point。
///
/// Reparse Point（含 junction）在标准 `is_symlink()` 下不可见，
/// Windows 需额外检查文件属性位，二者都不应被递归删除。
fn is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }

    #[cfg(not(windows))]
    false
}

/// 删除符号链接或 Reparse Point 本身（不递归目标内容）。
///
/// Windows 需区分"指向目录"与"指向文件"两种链接选用不同 API；
/// Unix 一律按文件链接删除。
///
/// # Errors
/// 删除失败时返回 Err（含 i18n 错误信息）。
fn remove_venv_link_or_reparse_point(path: &Path, _metadata: &fs::Metadata) -> Result<()> {
    #[cfg(windows)]
    let result = if fs::metadata(path)
        .map(|target_metadata| target_metadata.is_dir())
        .unwrap_or(false)
    {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };

    #[cfg(not(windows))]
    let result = fs::remove_file(path);

    result.with_context(|| {
        t!(
            "errors.delete_file_failed",
            error = path.display().to_string()
        )
    })
}

/// 在重试间隔内以 50ms 粒度小步等待，随时响应取消请求。
///
/// 不能用一次长 sleep——用户取消后应立刻停止而不是等完整间隔。
///
/// # Errors
/// 等待期间检测到取消请求时返回 Err。
fn wait_for_venv_recovery_retry(delay: Duration, cancel_requested: &AtomicBool) -> Result<()> {
    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
        if cancel_requested.load(Ordering::SeqCst) {
            bail!(t!("launcher.cancelled"));
        }
        thread::sleep(
            Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    Ok(())
}

/// 清除路径的只读属性（Windows 删除只读文件前必须先清位）。
///
/// 路径已消失时静默成功；非只读时不做任何操作。
///
/// # Errors
/// 属性修改失败时返回 Err。
fn clear_readonly(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)
            .with_context(|| t!("errors.chmod_failed", error = path.display().to_string()))?;
    }
    Ok(())
}

pub(crate) fn isolate_python_child_environment(cmd: &mut Command) {
    cmd.env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1");
    for key in [
        "PYTHONHOME",
        "pythonhome",
        "PYTHONPATH",
        "pythonpath",
        "VIRTUAL_ENV",
        "virtual_env",
        "__PYVENV_LAUNCHER__",
    ] {
        cmd.env_remove(key);
    }
}
