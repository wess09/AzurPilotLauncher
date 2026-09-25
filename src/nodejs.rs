#[cfg(windows)]
use std::{
    env,
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Read, Write},
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    process::Command,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[cfg(windows)]
use anyhow::{bail, Context, Result};
#[cfg(windows)]
use rand::RngCore;
#[cfg(windows)]
use reqwest::blocking::Client;
#[cfg(windows)]
use sha2::{Digest, Sha256};
#[cfg(windows)]
use tracing::{info, warn};

#[cfg(windows)]
use crate::{
    setup::{run_status_command, SplashUpdate},
    window_util::CreateNoWindow as _,
};
#[cfg(windows)]
use rust_i18n::t;
#[cfg(windows)]
use winapi::{
    shared::sddl::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
    um::{
        fileapi::CreateDirectoryW, minwinbase::SECURITY_ATTRIBUTES,
        sysinfoapi::{GetSystemDirectoryW, GetWindowsDirectoryW}, winbase::LocalFree,
        winnt::PSECURITY_DESCRIPTOR,
    },
};

/// 前端构建所需的最低 Node.js 版本，与 frontend/package.json 的 engines 一致。
/// 用作可用性判据；安装目标另见 NODEJS_LTS_VERSION。
const NODEJS_MIN_FRONTEND_VERSION: (u32, u32, u32) = (22, 12, 0);
#[cfg(windows)]
/// 与 NODEJS_MIN_FRONTEND_VERSION 对应的文本形式，供 UI 文案使用。
pub const NODEJS_MIN_FRONTEND_VERSION_TEXT: &str = "22.12.0";

#[cfg(windows)]
/// 私有安装目录的上级目录名，位于仅管理员可写的 ProgramData 下。
const NODEJS_MACHINE_DIRECTORY: &str = "AzurPilotLauncher";
#[cfg(windows)]
const NODEJS_PRIVATE_SUBDIRECTORY: &str = "nodejs";

// 发行包 URL 与校验和放在一起。
// 采用官方 zip，可直接解压进私有目录。
#[cfg(windows)]
const NODEJS_LTS_VERSION: &str = "24.21.0";
#[cfg(windows)]
const NODEJS_LTS_X64_ZIP_URL: &str = "https://nodejs.org/dist/v24.21.0/node-v24.21.0-win-x64.zip";
#[cfg(windows)]
const NODEJS_LTS_X64_ZIP_SHA256: &str =
    "158f7685b44de51f6c0df1d153526cbcd3e1bc739a8dfc607721cef75de9e541";
#[cfg(windows)]
const NODEJS_LTS_ARM64_ZIP_URL: &str = "https://nodejs.org/dist/v24.21.0/node-v24.21.0-win-arm64.zip";
#[cfg(windows)]
const NODEJS_LTS_ARM64_ZIP_SHA256: &str =
    "8779b1bde1d39f8d420e3b57aa657b39891af434d3de44a919044cec06785921";
#[cfg(windows)]
const NODEJS_LTS_X86_VERSION: &str = "22.22.2";
#[cfg(windows)]
const NODEJS_LTS_X86_ZIP_URL: &str = "https://nodejs.org/dist/v22.22.2/node-v22.22.2-win-x86.zip";
#[cfg(windows)]
const NODEJS_LTS_X86_ZIP_SHA256: &str =
    "ca892f829a733109e341c43585fd2094177e9d2f2c45f97c7ed3cf329d5427c5";
#[cfg(windows)]
const NODEJS_LTS_X64_MSI_URL: &str = "https://nodejs.org/dist/v24.21.0/node-v24.21.0-x64.msi";
#[cfg(windows)]
const NODEJS_LTS_X64_MSI_SHA256: &str =
    "bb0eaee134f9357f22aea915ee793343e627aefc1e66488164bac6915bce2cac";
#[cfg(windows)]
const NODEJS_LTS_ARM64_MSI_URL: &str = "https://nodejs.org/dist/v24.21.0/node-v24.21.0-arm64.msi";
#[cfg(windows)]
const NODEJS_LTS_ARM64_MSI_SHA256: &str =
    "22ca85110f26015696a3fa9216bc372ae65203d170622eaf7d211e2dd5bb49e3";
#[cfg(windows)]
const NODEJS_LTS_X86_MSI_URL: &str = "https://nodejs.org/dist/v22.22.2/node-v22.22.2-x86.msi";
#[cfg(windows)]
const NODEJS_LTS_X86_MSI_SHA256: &str =
    "e43cf42f461cbfea23a079925cfdd132a18cf66d4e30f64ec5ab4ec31dbb41f3";
#[cfg(windows)]
const NODEJS_MSI_FILE_NAME: &str = "nodejs-lts.msi";
#[cfg(windows)]
const NODEJS_INSTALLER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);
#[cfg(windows)]
const NODEJS_ARCHIVE_FILE_NAME: &str = "nodejs.zip";
#[cfg(windows)]
const NODEJS_DOWNLOAD_BUFFER_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const NODEJS_REGISTRY_PATH: &str = r"SOFTWARE\Node.js";
#[cfg(windows)]
const WINDOWS_CURRENT_VERSION_REGISTRY_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion";
#[cfg(windows)]
const NODEJS_INSTALL_PATH_VALUE: &str = "InstallPath";
// Protected DACL: only SYSTEM and elevated Administrators can alter files;
// the owner-rights denial prevents a medium-integrity owner from rewriting it.
#[cfg(windows)]
const NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL: &str =
    "D:P(D;OICI;WDWO;;;OW)(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
#[cfg(windows)]
const NODEJS_SECURE_INSTALLER_DIRECTORY_ATTEMPTS: usize = 32;
#[cfg(windows)]
const NODEJS_SECURE_INSTALLER_DIRECTORY_RANDOM_BYTES: usize = 16;

#[cfg(windows)]
#[derive(Debug)]
struct NodeJsInstallation {
    executable: PathBuf,
    version: String,
    parsed: (u32, u32, u32),
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug)]
struct NodeJsInstaller {
    version: &'static str,
    url: &'static str,
    sha256: &'static str,
}

#[cfg(windows)]
struct SecureNodeJsInstallerDir {
    path: PathBuf,
}

#[cfg(windows)]
impl SecureNodeJsInstallerDir {
    fn new() -> Result<Self> {
        let root = windows_temp_directory()?;
        for _ in 0..NODEJS_SECURE_INSTALLER_DIRECTORY_ATTEMPTS {
            let path = root.join(format!("AzurPilot-NodeJs-{}", secure_directory_suffix()));
            match create_secure_directory(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "create protected Node.js installer directory {}",
                            path.display()
                        )
                    })
                }
            }
        }

        bail!(t!("errors.nodejs_installer_dir"))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(windows)]
impl Drop for SecureNodeJsInstallerDir {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            if error.kind() != io::ErrorKind::NotFound {
                warn!(path = %self.path.display(), "Unable to remove Node.js installer directory: {error}");
            }
        }
    }
}

#[cfg(windows)]
/// 已有 Node.js 相对最低可用版本的判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeJsAvailability {
    /// 私有目录、注册表与 Program Files 中都没有可用的 node.exe。
    Missing,
    /// 找到了但版本不足；携带检测到的版本号。
    Outdated(String),
    /// 版本满足前端构建要求，可直接使用。
    Ready,
}

#[cfg(windows)]
/// 启动器自有的 Node.js 安装目录，与系统安装完全隔离。
fn private_nodejs_directory() -> Result<PathBuf> {
    let base = std::env::var_os("ProgramData")
        .ok_or_else(|| anyhow::anyhow!(t!("errors.programdata_not_found")))?;
    Ok(PathBuf::from(base)
        .join(NODEJS_MACHINE_DIRECTORY)
        .join(NODEJS_PRIVATE_SUBDIRECTORY))
}

#[cfg(windows)]
fn nodejs_candidates() -> Result<Vec<PathBuf>> {
    let mut candidates = vec![private_nodejs_directory()?.join("node.exe")];
    candidates.extend(trusted_nodejs_candidates());
    Ok(candidates)
}

#[cfg(windows)]
pub fn is_nodejs_available() -> NodeJsAvailability {
    let mut outdated: Option<String> = None;
    let Ok(candidates) = nodejs_candidates() else {
        return NodeJsAvailability::Missing;
    };
    for executable in candidates {
        let Some(installation) = probe_node(&executable) else {
            continue;
        };
        if !meets_minimum_version(installation.parsed) {
            warn!(
                version = %installation.version,
                executable = %installation.executable.display(),
                minimum = %format_version(NODEJS_MIN_FRONTEND_VERSION),
                "Node.js runtime is below the version required by the frontend build"
            );
            outdated = Some(installation.version);
            continue;
        }

        // 仅达标目录可入 PATH；低版本目录会遮蔽私有目录中的达标版本。
        if let Some(directory) = installation
            .executable
            .parent()
            .filter(|directory| !directory.as_os_str().is_empty())
        {
            prepend_to_path(directory);
        }
        info!(
            version = installation.version,
            executable = %installation.executable.display(),
            "Node.js runtime is available"
        );
        return NodeJsAvailability::Ready;
    }

    match outdated {
        Some(version) => NodeJsAvailability::Outdated(version),
        None => NodeJsAvailability::Missing,
    }
}

#[cfg(windows)]
/// 按可用性分派：达标不动，缺失装系统路径，版本过低装私有目录。
pub fn install_nodejs(
    availability: &NodeJsAvailability,
    cancel_requested: &AtomicBool,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<()> {
    match availability {
        NodeJsAvailability::Ready => Ok(()),
        NodeJsAvailability::Missing => install_nodejs_system_wide(cancel_requested, &mut status_updater),
        NodeJsAvailability::Outdated(_) => install_nodejs_portable(cancel_requested, &mut status_updater),
    }
}

#[cfg(windows)]
fn install_nodejs_portable(
    cancel_requested: &AtomicBool,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<()> {
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("setup.cancel_cleaning"));
    }

    let installer = nodejs_installer_for_current_architecture()?;
    validate_nodejs_installer_source(installer.url, installer.sha256)?;
    let archive_directory = SecureNodeJsInstallerDir::new()?;
    let archive_path = archive_directory
        .path()
        .join(NODEJS_ARCHIVE_FILE_NAME);

    status_updater(SplashUpdate::loading(
        t!("setup.installing_nodejs"),
        t!("setup.downloading_nodejs", version = installer.version),
        5,
    ));
    download_nodejs_installer(installer, &archive_path, cancel_requested)?;
    verify_nodejs_installer(&archive_path, installer.sha256)?;

    let target = private_nodejs_directory()?;
    status_updater(SplashUpdate::loading(
        t!("setup.installing_nodejs"),
        t!("setup.installing_nodejs"),
        7,
    ));
    extract_nodejs_zip(&archive_path, &target, cancel_requested)?;

    // 仅以私有目录的安装结果作为成功判据。
    let installed = target.join("node.exe");
    if probe_node(&installed)
        .is_some_and(|installation| meets_minimum_version(installation.parsed))
    {
        return Ok(());
    }

    bail!(
        "Node.js archive was extracted, but {} is missing or below the required version",
        installed.display()
    );
}
#[cfg(windows)]
fn nodejs_msi_installer_for_current_architecture() -> Result<NodeJsInstaller> {
    nodejs_msi_installer_for_architecture(env::consts::ARCH)
}

#[cfg(windows)]
fn nodejs_msi_installer_for_architecture(architecture: &str) -> Result<NodeJsInstaller> {
    match architecture {
        "x86_64" => Ok(NodeJsInstaller {
            version: NODEJS_LTS_VERSION,
            url: NODEJS_LTS_X64_MSI_URL,
            sha256: NODEJS_LTS_X64_MSI_SHA256,
        }),
        "aarch64" => Ok(NodeJsInstaller {
            version: NODEJS_LTS_VERSION,
            url: NODEJS_LTS_ARM64_MSI_URL,
            sha256: NODEJS_LTS_ARM64_MSI_SHA256,
        }),
        "x86" => Ok(NodeJsInstaller {
            version: NODEJS_LTS_X86_VERSION,
            url: NODEJS_LTS_X86_MSI_URL,
            sha256: NODEJS_LTS_X86_MSI_SHA256,
        }),
        other => bail!(
            "Node.js automatic installation is not available for Windows architecture {other}"
        ),
    }
}

#[cfg(windows)]
/// 系统路径下没有 Node.js 时安装官方 MSI 到系统路径。
fn install_nodejs_system_wide(
    cancel_requested: &AtomicBool,
    status_updater: &mut impl FnMut(SplashUpdate),
) -> Result<()> {
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("setup.cancel_cleaning"));
    }

    let installer = nodejs_msi_installer_for_current_architecture()?;
    validate_nodejs_installer_source(installer.url, installer.sha256)?;
    let installer_directory = SecureNodeJsInstallerDir::new()?;
    let installer_path = installer_directory.path().join(NODEJS_MSI_FILE_NAME);

    status_updater(SplashUpdate::loading(
        t!("setup.installing_nodejs"),
        t!("setup.downloading_nodejs", version = installer.version),
        5,
    ));
    download_nodejs_installer(installer, &installer_path, cancel_requested)?;
    verify_nodejs_installer(&installer_path, installer.sha256)?;

    status_updater(SplashUpdate::loading(
        t!("setup.installing_nodejs"),
        t!("setup.installing_nodejs"),
        7,
    ));
    run_nodejs_installer(&installer_path, cancel_requested, status_updater)?;

    if matches!(is_nodejs_available(), NodeJsAvailability::Ready) {
        return Ok(());
    }

    bail!(t!("errors.nodejs_exe_missing"))
}

#[cfg(windows)]
fn nodejs_msi_args(installer_path: &Path) -> Vec<std::ffi::OsString> {
    vec![
        "/i".into(),
        installer_path.as_os_str().to_owned(),
        "/qn".into(),
        "/norestart".into(),
    ]
}

#[cfg(windows)]
fn run_nodejs_installer(
    installer_path: &Path,
    cancel_requested: &AtomicBool,
    status_updater: &mut impl FnMut(SplashUpdate),
) -> Result<()> {
    let mut command = Command::new(system_msiexec_path()?);
    command.args(nodejs_msi_args(installer_path));
    let mut child = command.create_no_window().spawn()?;
    let mut wait_ticks = 0u16;

    loop {
        if cancel_requested.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            bail!(t!("setup.cancel_cleaning"));
        }

        if let Some(status) = child.try_wait()? {
            if status.success() || status.code() == Some(3010) {
                return Ok(());
            }
            bail!(t!("errors.nodejs_installer_exit", status = status.to_string()));
        }

        wait_ticks = wait_ticks.saturating_add(1);
        if wait_ticks >= 10 {
            wait_ticks = 0;
            status_updater(SplashUpdate::loading(
                t!("setup.installing_nodejs"),
                t!("setup.installing_nodejs"),
                7,
            ));
        }

        std::thread::sleep(NODEJS_INSTALLER_POLL_INTERVAL);
    }
}

#[cfg(windows)]
fn system_msiexec_path() -> Result<PathBuf> {
    // System directory resolved by the Win32 API, independent of PATH and the working directory.
    let msiexec = system_directory()?.join("msiexec.exe");
    if msiexec.is_file() {
        return Ok(msiexec);
    }
    bail!(t!("errors.msiexec_not_found", path = msiexec.display().to_string()));
}

#[cfg(windows)]
/// 取系统自带的 PowerShell；提权进程不从 PATH 解析可执行文件。
fn system_powershell_path() -> Result<PathBuf> {
    let mut buffer = vec![0u16; 260];
    let length = loop {
        let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            bail!(t!("errors.windows_dir_not_found"));
        }
        let length = length as usize;
        if length < buffer.len() {
            break length;
        }
        buffer.resize(length.saturating_add(1), 0);
    };
    let windows_directory = PathBuf::from(OsString::from_wide(&buffer[..length]));
    let powershell = windows_directory
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if powershell.is_file() {
        return Ok(powershell);
    }
    bail!(t!("errors.powershell_not_found", path = powershell.display().to_string()));
}

#[cfg(windows)]
fn probe_node(executable: &Path) -> Option<NodeJsInstallation> {
    if !executable.is_absolute() || !executable.is_file() {
        return None;
    }
    let output = Command::new(executable)
        .arg("--version")
        .create_no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let (major, minor, patch) = parse_nodejs_version(&output.stdout)?;
    Some(NodeJsInstallation {
        executable: executable.to_path_buf(),
        version: format!("{major}.{minor}.{patch}"),
        parsed: (major, minor, patch),
    })
}

#[cfg(windows)]
fn trusted_nodejs_candidates() -> Vec<PathBuf> {
    // This launcher is elevated, so never resolve node.exe from PATH or CWD.
    // These machine-level registry entries are protected from standard users.
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(key) = windows_registry::LOCAL_MACHINE.open(NODEJS_REGISTRY_PATH) {
        if let Ok(install_path) = key.get_string(NODEJS_INSTALL_PATH_VALUE) {
            push_unique_absolute_path(
                &mut candidates,
                PathBuf::from(install_path).join("node.exe"),
            );
        }
    }

    if let Ok(key) = windows_registry::LOCAL_MACHINE.open(WINDOWS_CURRENT_VERSION_REGISTRY_PATH) {
        for value_name in [
            "ProgramFilesDir",
            "ProgramW6432Dir",
            "ProgramFilesDir (x86)",
        ] {
            if let Ok(root) = key.get_string(value_name) {
                push_unique_absolute_path(
                    &mut candidates,
                    PathBuf::from(root).join("nodejs").join("node.exe"),
                );
            }
        }
    }
    candidates
}

#[cfg(windows)]
fn push_unique_absolute_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if candidate.is_absolute()
        && !paths
            .iter()
            .any(|existing| same_windows_path(existing, &candidate))
    {
        paths.push(candidate);
    }
}

#[cfg(windows)]
fn same_windows_path(left: &Path, right: &Path) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

#[cfg(windows)]
fn prepend_to_path(directory: &Path) {
    let existing_path = env::var_os("PATH").unwrap_or_default();
    if env::split_paths(&existing_path).any(|existing| same_windows_path(&existing, directory)) {
        return;
    }

    let mut paths = vec![directory.to_path_buf()];
    paths.extend(env::split_paths(&existing_path));
    if let Ok(path) = env::join_paths(paths) {
        env::set_var("PATH", path);
    }
}

fn parse_nodejs_version(output: &[u8]) -> Option<(u32, u32, u32)> {
    let value = std::str::from_utf8(output).ok()?.trim();
    let version = value.strip_prefix('v')?;
    let mut components = version.split('.');
    let major = components.next()?.parse::<u32>().ok()?;
    let minor = components.next()?.parse::<u32>().ok()?;
    let patch = components.next()?.parse::<u32>().ok()?;
    if major == 0 && minor == 0 && patch == 0 {
        return None;
    }
    Some((major, minor, patch))
}

/// 元组按位比较即字典序，与语义化版本一致。
fn meets_minimum_version(parsed: (u32, u32, u32)) -> bool {
    parsed >= NODEJS_MIN_FRONTEND_VERSION
}

fn format_version((major, minor, patch): (u32, u32, u32)) -> String {
    format!("{major}.{minor}.{patch}")
}

#[cfg(windows)]
fn nodejs_installer_for_current_architecture() -> Result<NodeJsInstaller> {
    nodejs_installer_for_architecture(env::consts::ARCH)
}

#[cfg(windows)]
fn nodejs_installer_for_architecture(architecture: &str) -> Result<NodeJsInstaller> {
    match architecture {
        "x86_64" => Ok(NodeJsInstaller {
            version: NODEJS_LTS_VERSION,
            url: NODEJS_LTS_X64_ZIP_URL,
            sha256: NODEJS_LTS_X64_ZIP_SHA256,
        }),
        "aarch64" => Ok(NodeJsInstaller {
            version: NODEJS_LTS_VERSION,
            url: NODEJS_LTS_ARM64_ZIP_URL,
            sha256: NODEJS_LTS_ARM64_ZIP_SHA256,
        }),
        "x86" => Ok(NodeJsInstaller {
            version: NODEJS_LTS_X86_VERSION,
            url: NODEJS_LTS_X86_ZIP_URL,
            sha256: NODEJS_LTS_X86_ZIP_SHA256,
        }),
        other => bail!(
            "Node.js automatic installation is not available for Windows architecture {other}"
        ),
    }
}

#[cfg(windows)]
fn validate_nodejs_installer_source(url: &str, digest: &str) -> Result<()> {
    if !url.starts_with("https://") {
        bail!(t!("errors.nodejs_url_not_https"));
    }
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!(t!("errors.nodejs_digest_invalid"));
    }
    Ok(())
}

#[cfg(windows)]
fn download_nodejs_installer(
    installer: NodeJsInstaller,
    part_path: &Path,
    cancel_requested: &AtomicBool,
) -> Result<()> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(15 * 60))
        .build()
        .context("build Node.js download client")?;
    let mut response = client
        .get(installer.url)
        .send()
        .context("download Node.js installer")?
        .error_for_status()
        .context("Node.js installer download returned an error status")?;
    let mut destination = File::create(part_path).context("create Node.js installer part file")?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; NODEJS_DOWNLOAD_BUFFER_BYTES];

    loop {
        if cancel_requested.load(Ordering::SeqCst) {
            bail!(t!("setup.cancel_cleaning"));
        }
        let read = response
            .read(&mut buffer)
            .context("read Node.js installer download")?;
        if read == 0 {
            break;
        }
        destination
            .write_all(&buffer[..read])
            .context("write Node.js installer part file")?;
        digest.update(&buffer[..read]);
    }
    destination
        .flush()
        .context("flush Node.js installer part file")?;

    let actual = format!("{:x}", digest.finalize());
    if !actual.eq_ignore_ascii_case(installer.sha256) {
        bail!(
            "Node.js installer checksum mismatch: expected {}, got {}",
            installer.sha256,
            actual
        );
    }
    Ok(())
}

#[cfg(windows)]
fn verify_nodejs_installer(installer_path: &Path, expected_sha256: &str) -> Result<()> {
    let mut source =
        File::open(installer_path).context("open Node.js installer for verification")?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; NODEJS_DOWNLOAD_BUFFER_BYTES];
    loop {
        let read = source
            .read(&mut buffer)
            .context("read Node.js installer for verification")?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }

    let actual = format!("{:x}", digest.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        bail!(
            "Node.js installer checksum mismatch after download: expected {}, got {}",
            expected_sha256,
            actual
        );
    }
    Ok(())
}

#[cfg(windows)]
/// 清空私有目录后重建，使解压结果不与旧安装残留混合。
fn reset_private_directory(target: &Path) -> Result<()> {
    match fs::remove_dir_all(target) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("clear private Node.js directory {}", target.display()));
        }
    }
    fs::create_dir_all(target)
        .with_context(|| format!("create private Node.js directory {}", target.display()))
}

#[cfg(windows)]
/// 用系统 PowerShell 解开官方 zip，再摊平归档内的顶层目录。
fn extract_nodejs_zip(
    archive_path: &Path,
    target: &Path,
    cancel_requested: &AtomicBool,
) -> Result<()> {
    reset_private_directory(target)?;

    let powershell = system_powershell_path()?;
    // 经环境变量传路径；短名可避免含空格或引号的路径被拆成多个参数。
    let mut command = Command::new(powershell);
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        "Expand-Archive -LiteralPath $env:a -DestinationPath $env:t -Force",
    ]);
    command.env("a", archive_path);
    command.env("t", target);

    let status = run_status_command(&mut command, cancel_requested)?;
    if !status.success() {
        bail!(t!("errors.nodejs_extract_exit", status = status.to_string()));
    }

    flatten_single_child_directory(target)?;

    let installed = target.join("node.exe");
    if !installed.is_file() {
        bail!(t!("errors.nodejs_archive_incomplete", path = installed.display().to_string()));
    }
    Ok(())
}

#[cfg(windows)]
/// 官方 zip 的内容位于带版本号的顶层目录下；摊平后 node.exe 落在 target 根。
fn flatten_single_child_directory(target: &Path) -> Result<()> {
    let mut entries = fs::read_dir(target)
        .with_context(|| format!("read {}", target.display()))?
        .collect::<io::Result<Vec<_>>>()
        .with_context(|| format!("read {}", target.display()))?;
    if entries.len() != 1 || !entries[0].file_type()?.is_dir() {
        return Ok(());
    }

    let wrapper = entries.remove(0).path();
    for entry in fs::read_dir(&wrapper)
        .with_context(|| format!("read {}", wrapper.display()))?
    {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if destination.exists() {
            continue;
        }
        fs::rename(entry.path(), &destination).with_context(|| {
            format!(
                "move {} to {}",
                entry.path().display(),
                destination.display()
            )
        })?;
    }
    let _ = fs::remove_dir(&wrapper);
    Ok(())
}


#[cfg(windows)]
fn windows_temp_directory() -> Result<PathBuf> {
    let system_directory = system_directory()?;
    let windows_directory = system_directory
        .parent()
        .ok_or_else(|| anyhow::anyhow!(t!("errors.windows_dir_not_found")))?;
    let temp_directory = windows_directory.join("Temp");
    if temp_directory.is_dir() {
        return Ok(temp_directory);
    }
    bail!(
        "Windows temporary directory was not found at {}",
        temp_directory.display()
    );
}

#[cfg(windows)]
fn system_directory() -> Result<PathBuf> {
    let mut buffer = vec![0u16; 260];
    loop {
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            bail!(t!("errors.windows_system_dir_not_found"));
        }
        let length = length as usize;
        if length < buffer.len() {
            return Ok(PathBuf::from(OsString::from_wide(&buffer[..length])));
        }
        buffer.resize(length.saturating_add(1), 0);
    }
}

#[cfg(windows)]
fn create_secure_directory(path: &Path) -> io::Result<()> {
    let path = wide_null(path.as_os_str());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let mut sddl = NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL
        .encode_utf16()
        .collect::<Vec<_>>();
    sddl.push(0);

    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            u32::from(SDDL_REVISION_1),
            &mut descriptor,
            ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let created = unsafe { CreateDirectoryW(path.as_ptr(), &mut attributes) };
    unsafe {
        LocalFree(descriptor);
    }
    if created == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn secure_directory_suffix() -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0u8; NODEJS_SECURE_INSTALLER_DIRECTORY_RANDOM_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut suffix = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        suffix.push(HEX[(byte >> 4) as usize] as char);
        suffix.push(HEX[(byte & 0x0f) as usize] as char);
    }
    suffix
}


#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn secure_installer_dir_removes_downloaded_msi_on_drop() {
        let directory = SecureNodeJsInstallerDir::new().expect("create secure installer directory");
        let path = directory.path().to_path_buf();
        fs::write(path.join(NODEJS_MSI_FILE_NAME), b"installer payload")
            .expect("write installer payload");

        drop(directory);

        assert!(!path.exists(), "installer directory should be removed on drop");
    }

    #[test]
    fn test_nodejs_installer_source_requires_https_and_sha256() {
        let digest = "a".repeat(64);

        assert!(
            validate_nodejs_installer_source("https://nodejs.org/dist/node.msi", &digest).is_ok()
        );
        assert!(
            validate_nodejs_installer_source("http://nodejs.org/dist/node.msi", &digest).is_err()
        );
        assert!(
            validate_nodejs_installer_source("https://nodejs.org/dist/node.msi", "bad").is_err()
        );
    }

    #[test]
    fn test_nodejs_version_parser_rejects_non_node_output() {
        assert_eq!(parse_nodejs_version(b"v24.21.0\r\n"), Some((24, 21, 0)));
        assert_eq!(parse_nodejs_version(b"24.21.0\n"), None);
        assert_eq!(parse_nodejs_version(b"node v24.21.0\n"), None);
        assert_eq!(parse_nodejs_version(b"v0.0.0\n"), None);
    }

    #[test]
    fn test_flatten_unwraps_the_single_versioned_directory_of_the_official_zip() {
        let root = tempfile::tempdir().expect("temp dir");
        let wrapper = root.path().join("node-v24.21.0-win-x64");
        std::fs::create_dir_all(wrapper.join("node_modules")).expect("create wrapper");
        std::fs::write(wrapper.join("node.exe"), b"binary").expect("write node.exe");
        std::fs::write(wrapper.join("npm.cmd"), b"shim").expect("write npm.cmd");

        flatten_single_child_directory(root.path()).expect("flatten");

        assert!(root.path().join("node.exe").is_file());
        assert!(root.path().join("npm.cmd").is_file());
        assert!(root.path().join("node_modules").is_dir());
        assert!(!wrapper.exists(), "wrapper directory should be gone");
    }

    #[test]
    fn test_flatten_leaves_an_already_flat_layout_untouched() {
        let root = tempfile::tempdir().expect("temp dir");
        std::fs::write(root.path().join("node.exe"), b"binary").expect("write node.exe");
        std::fs::write(root.path().join("npm.cmd"), b"shim").expect("write npm.cmd");

        flatten_single_child_directory(root.path()).expect("flatten");

        assert!(root.path().join("node.exe").is_file());
        assert!(root.path().join("npm.cmd").is_file());
    }

    #[test]
    fn test_minimum_version_accepts_only_versions_the_frontend_can_build_with() {
        assert!(meets_minimum_version((22, 12, 0)));
        assert!(meets_minimum_version((22, 22, 2)));
        assert!(meets_minimum_version((24, 21, 0)));

        // 低于 22.12.0 的旧版本必须被拒，否则前端构建会在用户机上失败。
        assert!(!meets_minimum_version((22, 11, 0)));
        assert!(!meets_minimum_version((20, 19, 0)));
        assert!(!meets_minimum_version((18, 20, 4)));
        assert!(!meets_minimum_version((16, 20, 2)));
    }

    #[test]
    fn test_nodejs_installer_matches_windows_architecture() {
        let x64 = nodejs_installer_for_architecture("x86_64").expect("x64 installer");
        assert_eq!(x64.version, NODEJS_LTS_VERSION);
        assert_eq!(x64.url, NODEJS_LTS_X64_ZIP_URL);
        assert_eq!(x64.sha256, NODEJS_LTS_X64_ZIP_SHA256);

        let arm64 = nodejs_installer_for_architecture("aarch64").expect("arm64 installer");
        assert_eq!(arm64.version, NODEJS_LTS_VERSION);
        assert_eq!(arm64.url, NODEJS_LTS_ARM64_ZIP_URL);
        assert_eq!(arm64.sha256, NODEJS_LTS_ARM64_ZIP_SHA256);

        let x86 = nodejs_installer_for_architecture("x86").expect("x86 installer");
        assert_eq!(x86.version, NODEJS_LTS_X86_VERSION);
        assert_eq!(x86.url, NODEJS_LTS_X86_ZIP_URL);
        assert_eq!(x86.sha256, NODEJS_LTS_X86_ZIP_SHA256);

        assert!(nodejs_installer_for_architecture("mips").is_err());
    }

    #[test]
    fn test_nodejs_probe_never_resolves_a_bare_executable_name() {
        assert!(probe_node(Path::new("node")).is_none());
    }

    #[test]
    fn test_secure_installer_directory_name_uses_full_random_hex() {
        let suffix = secure_directory_suffix();

        assert_eq!(
            suffix.len(),
            NODEJS_SECURE_INSTALLER_DIRECTORY_RANDOM_BYTES * 2
        );
        assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn test_secure_installer_directory_dacl_is_protected() {
        assert!(NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL.starts_with("D:P"));
        assert!(NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL.contains("WDWO;;;OW"));
        assert!(NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL.contains("FA;;;SY"));
        assert!(NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL.contains("FA;;;BA"));
    }

    #[test]
    fn test_secure_installer_directory_dacl_is_valid_sddl() {
        let mut sddl = NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL
            .encode_utf16()
            .collect::<Vec<_>>();
        sddl.push(0);
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();

        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                u32::from(SDDL_REVISION_1),
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        assert_ne!(converted, 0, "{}", io::Error::last_os_error());
        unsafe {
            LocalFree(descriptor);
        }
    }

}
