//! 通用跨平台文件系统工具库
//!
//! 提供带重试与取消感知的目录递归删除、只读属性清除、Reparse Point 识别及父目录自动创建等基础能力，
//! 供 setup、components 等上层模块共同复用，避免层级反向依赖。

use anyhow::{anyhow, bail, Context, Result};
use rust_i18n::t;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

const CLEANUP_RETRIES: usize = 5;

/// 确保路径的父级目录存在；若不存在则自动递归创建。
pub fn ensure_parent_dir<'a>(path: &'a Path) -> Result<&'a Path> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("missing parent directory for {}", path.display()))?;
    fs::create_dir_all(parent)?;
    Ok(parent)
}

/// 带取消检查与退避重试的目录递归删除。
///
/// 每次删除尝试前后都检查取消标志，保证用户取消时能尽快中止。
pub fn remove_dir_all_with_retry(path: &Path, cancel_requested: &AtomicBool) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let mut last_error = None;
    for attempt in 0..CLEANUP_RETRIES {
        if cancel_requested.load(Ordering::SeqCst) {
            bail!(t!("launcher.cancelled"));
        }

        match remove_entry(path, cancel_requested) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if !path.exists() {
                    return Ok(());
                }
                wait_for_retry(
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

/// 递归删除路径条目（逐步检查取消标志，跳过符号链接目标递归）。
fn remove_entry(path: &Path, cancel_requested: &AtomicBool) -> Result<()> {
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("launcher.cancelled"));
    }

    let metadata = fs::symlink_metadata(path)?;
    if is_symlink_or_reparse_point(&metadata) {
        return remove_link_or_reparse_point(path, &metadata);
    }

    clear_readonly(path)?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            if cancel_requested.load(Ordering::SeqCst) {
                bail!(t!("launcher.cancelled"));
            }
            remove_entry(&entry?.path(), cancel_requested)?;
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

/// 判断给定元数据是否为符号链接或 Windows Reparse Point。
pub fn is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
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
fn remove_link_or_reparse_point(path: &Path, _metadata: &fs::Metadata) -> Result<()> {
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

/// 清除路径的只读属性（Windows 删除只读文件前必须先清位）。
pub fn clear_readonly(path: &Path) -> Result<()> {
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

/// 在重试间隔内以 50ms 粒度小步等待，随时响应取消请求。
fn wait_for_retry(delay: Duration, cancel_requested: &AtomicBool) -> Result<()> {
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
