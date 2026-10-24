//! Windows 平台 Node.js 运行时的检测与自动安装。
//!
//! 仅在 Windows 上编译（绝大多数条目带 #[cfg(windows)]）：优先探测系统级
//! Node.js，缺失或版本过低时经用户确认后自动安装。下载一律使用官方 HTTPS
//! 地址并用固定 SHA-256 校验；安装包存放于随机命名的受保护目录，MSI 由系统
//! 目录下的 msiexec.exe 静默安装，避免提权启动器被路径劫持或安装包替换。
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
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
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
        fileapi::CreateDirectoryW,
        minwinbase::SECURITY_ATTRIBUTES,
        sysinfoapi::{GetSystemDirectoryW, GetWindowsDirectoryW},
        winbase::LocalFree,
        winnt::PSECURITY_DESCRIPTOR,
    },
};

/// Python 后端 WebUI 构建所需的最低 Node.js 版本；与启动器自身的开发依赖分开。
/// 用作可用性判据；安装目标另见 NODEJS_LTS_VERSION。
const NODEJS_MIN_FRONTEND_VERSION: (u32, u32, u32) = (22, 12, 0);
#[cfg(windows)]
/// 与 NODEJS_MIN_FRONTEND_VERSION 对应的文本形式，供 UI 文案使用。
pub const NODEJS_MIN_FRONTEND_VERSION_TEXT: &str = "22.12.0";

#[cfg(windows)]
/// 私有安装目录的上级目录名，位于仅管理员可写的 ProgramData 下。
const NODEJS_MACHINE_DIRECTORY: &str = "AzurPilotLauncher";
#[cfg(windows)]
/// 私有 Node.js 的目录名，位于 NODEJS_MACHINE_DIRECTORY 之下。
const NODEJS_PRIVATE_SUBDIRECTORY: &str = "nodejs";

// 发行包 URL 与校验和放在一起。
// 采用官方 zip，可直接解压进私有目录。
#[cfg(windows)]
/// 便携 zip 与系统 MSI 共用的 Node.js LTS 版本（x86 单独使用旧版 LTS）。
const NODEJS_LTS_VERSION: &str = "24.21.0";
#[cfg(windows)]
/// x64 平台官方 zip 便携包的下载地址。
const NODEJS_LTS_X64_ZIP_URL: &str = "https://nodejs.org/dist/v24.21.0/node-v24.21.0-win-x64.zip";
#[cfg(windows)]
/// x64 zip 的预期 SHA-256，与官方公布的校验值保持一致。
const NODEJS_LTS_X64_ZIP_SHA256: &str =
    "158f7685b44de51f6c0df1d153526cbcd3e1bc739a8dfc607721cef75de9e541";
#[cfg(windows)]
/// ARM64 平台官方 zip 便携包的下载地址。
const NODEJS_LTS_ARM64_ZIP_URL: &str =
    "https://nodejs.org/dist/v24.21.0/node-v24.21.0-win-arm64.zip";
#[cfg(windows)]
/// ARM64 zip 的预期 SHA-256，与官方公布的校验值保持一致。
const NODEJS_LTS_ARM64_ZIP_SHA256: &str =
    "8779b1bde1d39f8d420e3b57aa657b39891af434d3de44a919044cec06785921";
#[cfg(windows)]
/// x86 平台使用的 Node.js LTS 版本：新版本已不再提供 x86 分发。
const NODEJS_LTS_X86_VERSION: &str = "22.22.2";
#[cfg(windows)]
/// x86 平台官方 zip 便携包的下载地址。
const NODEJS_LTS_X86_ZIP_URL: &str = "https://nodejs.org/dist/v22.22.2/node-v22.22.2-win-x86.zip";
#[cfg(windows)]
/// x86 zip 的预期 SHA-256，与官方公布的校验值保持一致。
const NODEJS_LTS_X86_ZIP_SHA256: &str =
    "ca892f829a733109e341c43585fd2094177e9d2f2c45f97c7ed3cf329d5427c5";
#[cfg(windows)]
/// x64 平台官方 MSI 安装包的下载地址。
const NODEJS_LTS_X64_MSI_URL: &str = "https://nodejs.org/dist/v24.21.0/node-v24.21.0-x64.msi";
#[cfg(windows)]
/// x64 MSI 的预期 SHA-256，与官方公布的校验值保持一致。
const NODEJS_LTS_X64_MSI_SHA256: &str =
    "bb0eaee134f9357f22aea915ee793343e627aefc1e66488164bac6915bce2cac";
#[cfg(windows)]
/// ARM64 平台官方 MSI 安装包的下载地址。
const NODEJS_LTS_ARM64_MSI_URL: &str = "https://nodejs.org/dist/v24.21.0/node-v24.21.0-arm64.msi";
#[cfg(windows)]
/// ARM64 MSI 的预期 SHA-256，与官方公布的校验值保持一致。
const NODEJS_LTS_ARM64_MSI_SHA256: &str =
    "22ca85110f26015696a3fa9216bc372ae65203d170622eaf7d211e2dd5bb49e3";
#[cfg(windows)]
/// x86 平台官方 MSI 安装包的下载地址。
const NODEJS_LTS_X86_MSI_URL: &str = "https://nodejs.org/dist/v22.22.2/node-v22.22.2-x86.msi";
#[cfg(windows)]
/// x86 MSI 的预期 SHA-256，与官方公布的校验值保持一致。
const NODEJS_LTS_X86_MSI_SHA256: &str =
    "e43cf42f461cbfea23a079925cfdd132a18cf66d4e30f64ec5ab4ec31dbb41f3";
#[cfg(windows)]
/// MSI 下载到暂存目录时使用的固定文件名。
const NODEJS_MSI_FILE_NAME: &str = "nodejs-lts.msi";
#[cfg(windows)]
/// 轮询 msiexec 退出状态的间隔；足够短以快速响应取消，又不至于空转。
const NODEJS_INSTALLER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);
#[cfg(windows)]
/// zip 下载到暂存目录时使用的固定文件名。
const NODEJS_ARCHIVE_FILE_NAME: &str = "nodejs.zip";
#[cfg(windows)]
/// 下载与校验共用的流式缓冲区大小，在内存占用与系统调用次数之间取平衡。
const NODEJS_DOWNLOAD_BUFFER_BYTES: usize = 64 * 1024;
#[cfg(windows)]
/// Node.js 官方安装器写入 HKLM 的注册表键，用于定位系统级安装。
const NODEJS_REGISTRY_PATH: &str = r"SOFTWARE\Node.js";
#[cfg(windows)]
/// HKLM 下记录 Program Files 各根目录值的注册表键。
const WINDOWS_CURRENT_VERSION_REGISTRY_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion";
#[cfg(windows)]
/// NODEJS_REGISTRY_PATH 键中记录安装目录的值名。
const NODEJS_INSTALL_PATH_VALUE: &str = "InstallPath";
/// 安装器暂存目录的受保护 DACL（SDDL 格式）。
///
/// `D:P` 表示保护性 DACL，不继承父目录的任何 ACE；`(D;OICI;WDWO;;;OW)`
/// 拒绝对象所有者改写权限与所有者变更，防止中等完整性进程借所有者身份
/// 解除保护；随后仅向 SYSTEM 与 Administrators 授予完全控制，标准用户
/// 无法替换或篡改目录中的安装包。
#[cfg(windows)]
const NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL: &str =
    "D:P(D;OICI;WDWO;;;OW)(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
#[cfg(windows)]
/// 随机目录名碰撞时的重试上限；碰撞概率极低，仅作兜底。
const NODEJS_SECURE_INSTALLER_DIRECTORY_ATTEMPTS: usize = 32;
#[cfg(windows)]
/// 目录名随机后缀的熵源字节数，展开后为 32 个十六进制字符。
const NODEJS_SECURE_INSTALLER_DIRECTORY_RANDOM_BYTES: usize = 16;

#[cfg(windows)]
/// 一次成功探测得到的 Node.js 安装信息。
#[derive(Debug)]
struct NodeJsInstallation {
    /// node.exe 的绝对路径。
    executable: PathBuf,
    /// `node --version` 解析后的版本文本（如 "24.21.0"）。
    version: String,
    /// 供版本比较使用的数值三元组。
    parsed: (u32, u32, u32),
}

#[cfg(windows)]
/// 描述某个具体架构的 Node.js 安装包：版本号、下载地址与预期摘要。
#[derive(Clone, Copy, Debug)]
struct NodeJsInstaller {
    /// Node.js 版本号（如 "24.21.0"），用于进度文案。
    version: &'static str,
    /// 官方 HTTPS 下载地址。
    url: &'static str,
    /// 下载内容的预期 SHA-256 十六进制摘要。
    sha256: &'static str,
}

#[cfg(windows)]
/// 位于 Windows\Temp 下的随机命名受保护目录，Drop 时自动清理。
struct SecureNodeJsInstallerDir {
    /// 暂存目录的完整路径。
    path: PathBuf,
}

#[cfg(windows)]
impl SecureNodeJsInstallerDir {
    /// 在 Windows\Temp 下创建随机命名的受保护暂存目录。
    ///
    /// 目录名带有不可预测的随机后缀，攻击者无法提前抢占同名路径，
    /// 从根源上排除符号链接与路径劫持；受保护 DACL 则保证目录创建后
    /// 不能被低权限进程改写。
    ///
    /// # Errors
    ///
    /// 当基础临时目录不可用、目录创建失败（"已存在"除外），或重试
    /// 次数用尽仍未取得独占路径时返回错误。
    fn new() -> Result<Self> {
        let root = windows_temp_directory()?;
        // 随机后缀几乎不会撞名；"已存在"按碰撞处理，换一个名字重试。
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

        // 连续碰撞视为异常环境，交由上层以本地化文案提示用户。
        bail!(t!("errors.nodejs_installer_dir"))
    }

    /// 返回暂存目录的路径。
    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(windows)]
/// 暂存目录的生命周期守卫：无论安装成功与否，析构时删除整个目录。
impl Drop for SecureNodeJsInstallerDir {
    fn drop(&mut self) {
        // 清理失败只记录告警：NotFound 属正常情况，其余不应中断安装结果判定。
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
///
/// 目录位于 ProgramData 之下：普通用户可读但不可写，只有管理员能放置
/// 文件，保证提权进程执行其中的 node.exe 不会被篡改。
///
/// # Errors
///
/// 当环境变量 `ProgramData` 缺失时返回错误。
fn private_nodejs_directory() -> Result<PathBuf> {
    let base = std::env::var_os("ProgramData")
        .ok_or_else(|| anyhow::anyhow!(t!("errors.programdata_not_found")))?;
    Ok(PathBuf::from(base)
        .join(NODEJS_MACHINE_DIRECTORY)
        .join(NODEJS_PRIVATE_SUBDIRECTORY))
}

#[cfg(windows)]
/// 汇总所有可信的 node.exe 候选路径，私有目录优先于系统位置。
///
/// # Errors
///
/// 当私有目录路径无法确定（缺少 `ProgramData` 环境变量）时返回错误。
fn nodejs_candidates() -> Result<Vec<PathBuf>> {
    let mut candidates = vec![private_nodejs_directory()?.join("node.exe")];
    candidates.extend(trusted_nodejs_candidates());
    Ok(candidates)
}

#[cfg(windows)]
/// 依次探测候选路径，给出当前 Node.js 的可用性结论。
///
/// 命中达标版本时会把其所在目录前置到本进程的 PATH，让后续子进程
/// （前端构建等）优先使用该运行时。
pub fn is_nodejs_available() -> NodeJsAvailability {
    let mut outdated: Option<String> = None;
    let Ok(candidates) = nodejs_candidates() else {
        return NodeJsAvailability::Missing;
    };
    // 探测失败的候选（文件缺失或无法执行）静默跳过，继续尝试下一个。
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

    // 全部候选均不达标时，报告"版本过低"而非"缺失"，文案更准确。
    match outdated {
        Some(version) => NodeJsAvailability::Outdated(version),
        None => NodeJsAvailability::Missing,
    }
}

#[cfg(windows)]
/// 按可用性分派：达标不动，缺失装系统路径，版本过低装私有目录。
///
/// # Errors
///
/// 当对应安装流程失败或用户请求取消时返回错误。
pub fn install_nodejs(
    availability: &NodeJsAvailability,
    cancel_requested: &Arc<AtomicBool>,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<()> {
    match availability {
        NodeJsAvailability::Ready => Ok(()),
        NodeJsAvailability::Missing => {
            install_nodejs_system_wide(cancel_requested, &mut status_updater)
        }
        NodeJsAvailability::Outdated(_) => {
            install_nodejs_portable(cancel_requested, &mut status_updater)
        }
    }
}

#[cfg(windows)]
/// 下载官方 zip 并解压到私有目录，安装仅供本启动器使用的便携 Node.js。
///
/// 用于系统已有 Node.js 但版本过低的场景：不动系统安装，避免破坏
/// 其他程序依赖的运行时。
///
/// # Errors
///
/// 当架构不受支持、下载或校验失败、解压失败，或解压后探测不到达标
/// 的 node.exe 时返回错误；用户请求取消时同样返回错误。
fn install_nodejs_portable(
    cancel_requested: &Arc<AtomicBool>,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<()> {
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("launcher.cancelled"));
    }

    let installer = nodejs_installer_for_current_architecture()?;
    // 只信任官方 HTTPS 地址与固定摘要；任何常量被篡改都会在此失败。
    validate_nodejs_installer_source(installer.url, installer.sha256)?;
    // 安装包落在随机受保护目录，防止被预先放置的同名文件顶替。
    let archive_directory = SecureNodeJsInstallerDir::new()?;
    let archive_path = archive_directory.path().join(NODEJS_ARCHIVE_FILE_NAME);

    status_updater(SplashUpdate::loading(
        t!("setup.installing_nodejs"),
        t!("setup.downloading_nodejs", version = installer.version),
        5,
    ));
    download_nodejs_installer(
        installer,
        &archive_path,
        cancel_requested,
        &mut status_updater,
    )?;
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
    if probe_node(&installed).is_some_and(|installation| meets_minimum_version(installation.parsed))
    {
        return Ok(());
    }

    bail!(
        "Node.js archive was extracted, but {} is missing or below the required version",
        installed.display()
    );
}
#[cfg(windows)]
/// 返回当前 CPU 架构对应的官方 MSI 安装包描述。
///
/// # Errors
///
/// 当架构不受支持时返回错误。
fn nodejs_msi_installer_for_current_architecture() -> Result<NodeJsInstaller> {
    nodejs_msi_installer_for_architecture(env::consts::ARCH)
}

#[cfg(windows)]
/// 按架构字符串选择官方 MSI 安装包。
///
/// x86 沿用 22.x LTS：新版本已不再提供 x86 的 MSI 分发。
///
/// # Errors
///
/// 当架构不受支持时返回错误。
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
///
/// # Errors
///
/// 当架构不受支持、下载或校验失败、msiexec 安装失败，或安装后仍探测
/// 不到达标运行时时返回错误；用户请求取消时同样返回错误。
fn install_nodejs_system_wide(
    cancel_requested: &Arc<AtomicBool>,
    status_updater: &mut impl FnMut(SplashUpdate),
) -> Result<()> {
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("launcher.cancelled"));
    }

    let installer = nodejs_msi_installer_for_current_architecture()?;
    // 只信任官方 HTTPS 地址与固定摘要，防止安装包来源被篡改。
    validate_nodejs_installer_source(installer.url, installer.sha256)?;
    // MSI 落在随机受保护目录，交给 msiexec 前不会被低权限进程替换。
    let installer_directory = SecureNodeJsInstallerDir::new()?;
    let installer_path = installer_directory.path().join(NODEJS_MSI_FILE_NAME);

    status_updater(SplashUpdate::loading(
        t!("setup.installing_nodejs"),
        t!("setup.downloading_nodejs", version = installer.version),
        5,
    ));
    download_nodejs_installer(installer, &installer_path, cancel_requested, status_updater)?;
    verify_nodejs_installer(&installer_path, installer.sha256)?;

    status_updater(SplashUpdate::loading(
        t!("setup.installing_nodejs"),
        t!("setup.installing_nodejs"),
        7,
    ));
    run_nodejs_installer(&installer_path, cancel_requested, status_updater)?;

    // 以全局探测结果为准，确认 MSI 确实安装出了可用的系统级运行时。
    if matches!(is_nodejs_available(), NodeJsAvailability::Ready) {
        return Ok(());
    }

    bail!(t!("errors.nodejs_exe_missing"))
}

#[cfg(windows)]
/// 组装 msiexec 的静默安装参数。
///
/// `/qn` 全程无界面，`/norestart` 阻止安装器在无人值守场景触发重启。
fn nodejs_msi_args(installer_path: &Path) -> Vec<std::ffi::OsString> {
    vec![
        "/i".into(),
        installer_path.as_os_str().to_owned(),
        "/qn".into(),
        "/norestart".into(),
    ]
}

#[cfg(windows)]
/// 以系统 msiexec.exe 静默安装 MSI，并轮询等待安装结束。
///
/// # Errors
///
/// 当 msiexec 启动失败、安装以非成功码退出，或用户请求取消时返回错误。
fn run_nodejs_installer(
    installer_path: &Path,
    cancel_requested: &AtomicBool,
    status_updater: &mut impl FnMut(SplashUpdate),
) -> Result<()> {
    let mut command = Command::new(system_msiexec_path()?);
    command.args(nodejs_msi_args(installer_path));
    let mut child = crate::task_process::TaskChild::spawn(&mut command)?;
    let mut wait_ticks = 0u16;

    // 轮询而非阻塞等待，保证取消请求能及时生效。
    loop {
        if cancel_requested.load(Ordering::SeqCst) {
            // 先杀死再回收子进程，避免遗留悬挂的 msiexec 实例。
            let _ = child.kill();
            let _ = child.wait();
            bail!(t!("launcher.cancelled"));
        }

        if let Some(status) = child.try_wait()? {
            // 3010 表示安装成功但需要重启，无人值守场景视同成功。
            if status.success() || status.code() == Some(3010) {
                return Ok(());
            }
            bail!(t!(
                "errors.nodejs_installer_exit",
                status = status.to_string()
            ));
        }

        // 周期性刷新进度文案，让启动画面在长时间安装期间保持活跃。
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
/// 返回系统目录下 msiexec.exe 的绝对路径。
///
/// # Errors
///
/// 当系统目录无法确定或 msiexec.exe 不存在时返回错误。
fn system_msiexec_path() -> Result<PathBuf> {
    // 通过 Win32 API 解析系统目录，不依赖 PATH 与当前工作目录，
    // 防止提权进程被诱导执行攻击者放置的同名程序。
    let msiexec = system_directory()?.join("msiexec.exe");
    if msiexec.is_file() {
        return Ok(msiexec);
    }
    bail!(t!(
        "errors.msiexec_not_found",
        path = msiexec.display().to_string()
    ));
}

#[cfg(windows)]
/// 取系统自带的 PowerShell；提权进程不从 PATH 解析可执行文件。
///
/// # Errors
///
/// 当 Windows 目录无法确定或 powershell.exe 不存在时返回错误。
fn system_powershell_path() -> Result<PathBuf> {
    // Win32 接口使用 UTF-16 宽字符缓冲区；返回 0 表示调用失败，
    // 返回长度达到缓冲区容量说明目录名放不下，扩容后重试。
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
    // 返回值不含结尾空字符，按该长度截取即为目录文本。
    let windows_directory = PathBuf::from(OsString::from_wide(&buffer[..length]));
    let powershell = windows_directory
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if powershell.is_file() {
        return Ok(powershell);
    }
    bail!(t!(
        "errors.powershell_not_found",
        path = powershell.display().to_string()
    ));
}

#[cfg(windows)]
/// 运行 `node --version` 并解析结果，任何一步失败都返回 None。
///
/// 只接受绝对路径且真实存在的可执行文件，杜绝裸名称经 PATH 命中
/// 不可信副本的可能。
fn probe_node(executable: &Path) -> Option<NodeJsInstallation> {
    if !executable.is_absolute() || !executable.is_file() {
        return None;
    }
    // 以子进程实际执行一次 --version，避免路径存在但内容并非可用的 node.exe。
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
/// 从机器级注册表收集可信的 node.exe 候选路径。
///
/// 本启动器以管理员权限运行，绝不能从 PATH 或当前工作目录解析
/// node.exe；这里依赖的机器级注册表项需要管理员权限才能写入，
/// 标准用户无法借其伪造路径。
fn trusted_nodejs_candidates() -> Vec<PathBuf> {
    // 仅信任 HKLM 下的注册表来源，均为官方安装器写入的位置。
    let mut candidates: Vec<PathBuf> = Vec::new();
    // Node.js 官方安装器在 HKLM\SOFTWARE\Node.js 记录的 InstallPath。
    if let Ok(key) = windows_registry::LOCAL_MACHINE.open(NODEJS_REGISTRY_PATH) {
        if let Ok(install_path) = key.get_string(NODEJS_INSTALL_PATH_VALUE) {
            push_unique_absolute_path(
                &mut candidates,
                PathBuf::from(install_path).join("node.exe"),
            );
        }
    }

    // CurrentVersion 键下的 Program Files 根目录值，覆盖各架构的默认安装位置。
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
/// 追加候选路径，跳过相对路径与重复项，保持探测顺序稳定。
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
/// 以大小写不敏感的方式比较两个 Windows 路径是否指向同一位置。
///
/// Windows 路径不区分大小写，注册表与用户输入中的大小写也常不一致。
fn same_windows_path(left: &Path, right: &Path) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

#[cfg(windows)]
/// 将达标目录前置到本进程的 PATH，已存在时保持原顺序不变。
fn prepend_to_path(directory: &Path) {
    let existing_path = env::var_os("PATH").unwrap_or_default();
    if env::split_paths(&existing_path).any(|existing| same_windows_path(&existing, directory)) {
        return;
    }

    // 只影响当前进程及其子进程，不会写回用户或系统的 PATH 设置。
    let mut paths = vec![directory.to_path_buf()];
    paths.extend(env::split_paths(&existing_path));
    if let Ok(path) = env::join_paths(paths) {
        env::set_var("PATH", path);
    }
}

/// 解析 `node --version` 的标准输出（如 "v24.21.0"）为版本三元组。
///
/// 缺少 `v` 前缀、分量无法解析或三元组全为零时返回 None；全零检查
/// 可排除输出异常内容的伪装程序。
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

/// 判断版本三元组是否满足前端构建的最低要求。
///
/// 元组按位比较即字典序，与语义化版本一致。
fn meets_minimum_version(parsed: (u32, u32, u32)) -> bool {
    parsed >= NODEJS_MIN_FRONTEND_VERSION
}

/// 将版本三元组格式化为 "major.minor.patch" 文本。
fn format_version((major, minor, patch): (u32, u32, u32)) -> String {
    format!("{major}.{minor}.{patch}")
}

#[cfg(windows)]
/// 返回当前 CPU 架构对应的官方 zip 便携包描述。
///
/// # Errors
///
/// 当架构不受支持时返回错误。
fn nodejs_installer_for_current_architecture() -> Result<NodeJsInstaller> {
    nodejs_installer_for_architecture(env::consts::ARCH)
}

#[cfg(windows)]
/// 按架构字符串选择官方 zip 便携包。
///
/// x86 沿用 22.x LTS：新版本已不再提供 x86 的 zip 分发。
///
/// # Errors
///
/// 当架构不受支持时返回错误。
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
/// 校验安装包来源：地址必须为 HTTPS，摘要必须为 64 位十六进制。
///
/// # Errors
///
/// 当地址非 HTTPS 或摘要格式不合法时返回错误。
fn validate_nodejs_installer_source(url: &str, digest: &str) -> Result<()> {
    // HTTPS 保证下载链路加密；摘要长度与字符集检查能在比对之前
    // 发现常量被截断或写错的情况。
    if !url.starts_with("https://") {
        bail!(t!("errors.nodejs_url_not_https"));
    }
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!(t!("errors.nodejs_digest_invalid"));
    }
    Ok(())
}

#[cfg(windows)]
/// 下载安装包到指定路径，边下载边计算 SHA-256，完成后与预期值比对。
/// 已知总大小时把下载量映射到 5–6.8%，预留剩余进度给独立复验；
/// 下载状态每 100ms 最多上报一次，避免逐块更新挤占界面线程。
///
/// # Errors
///
/// 当 HTTP 客户端构建、请求或写盘失败，或最终摘要与预期不符时返回
/// 错误；用户请求取消时同样返回错误。
fn download_nodejs_installer(
    installer: NodeJsInstaller,
    part_path: &Path,
    cancel_requested: &Arc<AtomicBool>,
    status_updater: &mut impl FnMut(SplashUpdate),
) -> Result<()> {
    // 慢速网络也能完整下载：连接超时 20 秒，总超时放宽到 15 分钟。
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(15 * 60))
        .build()
        .context("build Node.js download client")?;
    let response = crate::cancel::request(client.get(installer.url), cancel_requested)
        .context("download Node.js installer")?
        .error_for_status()
        .context("Node.js installer download returned an error status")?;
    let total_bytes = response.content_length().filter(|total| *total > 0);
    let mut response = crate::cancel::body(response, cancel_requested.clone());
    let mut destination = File::create(part_path).context("create Node.js installer part file")?;
    let mut digest = Sha256::new();
    // 流式读取并同步计算摘要，无需把整个安装包载入内存。
    let mut buffer = [0u8; NODEJS_DOWNLOAD_BUFFER_BYTES];
    let started_at = Instant::now();
    let mut last_reported_at = started_at - Duration::from_millis(100);
    let mut downloaded = 0u64;

    // 每读一块数据都检查取消标志，保证长时间下载可随时中断。
    loop {
        if cancel_requested.load(Ordering::SeqCst) {
            bail!(t!("launcher.cancelled"));
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
        downloaded = downloaded.saturating_add(read as u64);
        if last_reported_at.elapsed() >= Duration::from_millis(100) {
            last_reported_at = Instant::now();
            let speed = crate::updater::format_speed(
                crate::updater::download_speed_bytes_per_second(downloaded, started_at),
            );
            let (progress, detail) = match total_bytes {
                Some(total) => {
                    let fraction = downloaded.min(total) as f64 / total as f64;
                    (
                        5.0 + 1.8 * fraction,
                        t!(
                            "launcher_update.downloading_detail",
                            downloaded = crate::updater::format_bytes(downloaded),
                            total = crate::updater::format_bytes(total),
                            percent = format!("{:.0}", (fraction * 100.0).floor()),
                            speed = speed
                        )
                        .to_string(),
                    )
                }
                None => (
                    5.0,
                    t!(
                        "launcher_update.downloading_detail_unknown",
                        downloaded = crate::updater::format_bytes(downloaded),
                        speed = speed
                    )
                    .to_string(),
                ),
            };
            status_updater(SplashUpdate::loading(
                t!("setup.installing_nodejs"),
                detail,
                progress,
            ));
        }
    }
    destination
        .flush()
        .context("flush Node.js installer part file")?;

    status_updater(SplashUpdate::loading(
        t!("setup.installing_nodejs"),
        t!("launcher_update.verifying_detail"),
        6.8,
    ));

    // 官方公布的摘要可能混用大小写，比较时忽略大小写差异。
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
/// 复读磁盘上的安装包，独立复验 SHA-256。
///
/// 与下载期间的流式校验互为补充：即使文件在下载结束到使用之间被
/// 替换，这里也能发现。
///
/// # Errors
///
/// 当文件无法打开或读取失败，或摘要与预期不符时返回错误。
fn verify_nodejs_installer(installer_path: &Path, expected_sha256: &str) -> Result<()> {
    let mut source =
        File::open(installer_path).context("open Node.js installer for verification")?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; NODEJS_DOWNLOAD_BUFFER_BYTES];
    // 分块读取复算摘要，避免整包载入内存。
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
///
/// # Errors
///
/// 当旧目录无法删除（目录不存在除外）或新目录无法创建时返回错误。
fn reset_private_directory(target: &Path) -> Result<()> {
    // 目录不存在视同已清空；其余失败向上传递，避免旧文件混入新安装。
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
///
/// # Errors
///
/// 当 PowerShell 不存在、解压命令失败或解压结果缺少 node.exe 时返回
/// 错误；用户请求取消时同样返回错误。
fn extract_nodejs_zip(
    archive_path: &Path,
    target: &Path,
    cancel_requested: &AtomicBool,
) -> Result<()> {
    reset_private_directory(target)?;

    let powershell = system_powershell_path()?;
    // 经环境变量传路径；短名可避免含空格或引号的路径被拆成多个参数。
    // -NoProfile 与 -NonInteractive 防止用户配置和交互提示干扰自动化。
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
        bail!(t!(
            "errors.nodejs_extract_exit",
            status = status.to_string()
        ));
    }

    // 解压后立即摊平并确认 node.exe 存在，尽早暴露不完整的归档。
    flatten_single_child_directory(target)?;

    let installed = target.join("node.exe");
    if !installed.is_file() {
        bail!(t!(
            "errors.nodejs_archive_incomplete",
            path = installed.display().to_string()
        ));
    }
    Ok(())
}

#[cfg(windows)]
/// 官方 zip 的内容位于带版本号的顶层目录下；摊平后 node.exe 落在 target 根。
///
/// # Errors
///
/// 当目录遍历或条目移动失败时返回错误。
fn flatten_single_child_directory(target: &Path) -> Result<()> {
    // 仅当目录恰好只含一个子目录（官方 zip 的固定布局）时才摊平，
    // 其余布局保持原样，保证重复调用安全。
    let mut entries = fs::read_dir(target)
        .with_context(|| format!("read {}", target.display()))?
        .collect::<io::Result<Vec<_>>>()
        .with_context(|| format!("read {}", target.display()))?;
    if entries.len() != 1 || !entries[0].file_type()?.is_dir() {
        return Ok(());
    }

    let wrapper = entries.remove(0).path();
    // 目标已存在时跳过而非覆盖，重复执行不会破坏已有文件。
    for entry in fs::read_dir(&wrapper).with_context(|| format!("read {}", wrapper.display()))? {
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
    // 空壳目录删除失败无害，静默忽略。
    let _ = fs::remove_dir(&wrapper);
    Ok(())
}

#[cfg(windows)]
/// 返回 Windows 目录下的 Temp 作为暂存根目录。
///
/// 刻意不使用用户级 TEMP：提权进程若落入普通用户可写的目录，攻击者
/// 可预先创建同名路径实施劫持；Windows\Temp 的写入需要管理员权限。
///
/// # Errors
///
/// 当系统目录或其父目录无法确定，或 Temp 目录不存在时返回错误。
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
/// 通过 GetSystemDirectoryW 取系统目录（通常为 System32）的绝对路径。
///
/// # Errors
///
/// 当 Win32 调用失败时返回错误。
fn system_directory() -> Result<PathBuf> {
    // 宽字符缓冲区从 260 个 UTF-16 单元起步；返回 0 表示调用失败，
    // 返回长度达到缓冲区容量说明放不下，扩容后重试。
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
/// 以受保护 DACL 创建目录，仅 SYSTEM 与管理员组可完全控制。
///
/// # Errors
///
/// 当 SDDL 转换或目录创建失败时返回错误；调用方将"已存在"视为
/// 碰撞并重试。
fn create_secure_directory(path: &Path) -> io::Result<()> {
    // Win32 宽字符接口要求路径以空字符结尾的 UTF-16 编码传入。
    let path = wide_null(path.as_os_str());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SDDL 文本同样需要以空字符结尾的 UTF-16 形式。
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
    // 返回 0 表示 SDDL 文本无法解析，具体原因取自线程的最后错误码。
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }

    // 将二进制安全描述符挂到 SECURITY_ATTRIBUTES，随目录创建一并生效。
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let created = unsafe { CreateDirectoryW(path.as_ptr(), &mut attributes) };
    // 描述符内存由系统分配，无论创建成败都必须释放，避免泄漏。
    unsafe {
        LocalFree(descriptor);
    }
    // 返回 0 表示创建失败；"已存在"同样走此分支，由调用方按错误类型重试。
    if created == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
/// 将 OsStr 编码为以空字符结尾的 UTF-16 序列，供 Win32 宽字符接口使用。
fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
/// 生成 32 个十六进制字符的随机后缀，用作暂存目录的不可预测名称。
fn secure_directory_suffix() -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    // 使用操作系统提供的密码学安全随机源（OsRng），而非普通伪随机数。
    let mut bytes = [0u8; NODEJS_SECURE_INSTALLER_DIRECTORY_RANDOM_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut suffix = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        suffix.push(HEX[(byte >> 4) as usize] as char);
        suffix.push(HEX[(byte & 0x0f) as usize] as char);
    }
    suffix
}

/// 覆盖安装器目录生命周期、来源校验、版本解析与解压摊平等关键路径。
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[cfg(windows)]
    /// 确认暂存目录在 Drop 时连同其中的 MSI 一起删除。
    #[test]
    fn secure_installer_dir_removes_downloaded_msi_on_drop() {
        let directory = SecureNodeJsInstallerDir::new().expect("create secure installer directory");
        let path = directory.path().to_path_buf();
        fs::write(path.join(NODEJS_MSI_FILE_NAME), b"installer payload")
            .expect("write installer payload");

        drop(directory);

        assert!(
            !path.exists(),
            "installer directory should be removed on drop"
        );
    }

    /// 确认来源校验同时强制 HTTPS 与 64 位十六进制摘要。
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

    /// 确认版本解析只接受 node.exe 的标准输出格式。
    #[test]
    fn test_nodejs_version_parser_rejects_non_node_output() {
        assert_eq!(parse_nodejs_version(b"v24.21.0\r\n"), Some((24, 21, 0)));
        assert_eq!(parse_nodejs_version(b"24.21.0\n"), None);
        assert_eq!(parse_nodejs_version(b"node v24.21.0\n"), None);
        assert_eq!(parse_nodejs_version(b"v0.0.0\n"), None);
    }

    /// 确认摊平逻辑能解开官方 zip 的单一顶层版本目录。
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

    /// 确认已是平铺结构的目录不会被误改。
    #[test]
    fn test_flatten_leaves_an_already_flat_layout_untouched() {
        let root = tempfile::tempdir().expect("temp dir");
        std::fs::write(root.path().join("node.exe"), b"binary").expect("write node.exe");
        std::fs::write(root.path().join("npm.cmd"), b"shim").expect("write npm.cmd");

        flatten_single_child_directory(root.path()).expect("flatten");

        assert!(root.path().join("node.exe").is_file());
        assert!(root.path().join("npm.cmd").is_file());
    }

    /// 确认版本判据与前端构建要求严格一致。
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

    /// 确认三种 Windows 架构都能取到对应的官方 zip 安装包。
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

    /// 确认探测拒绝裸名称，绝不经 PATH 解析 node。
    #[test]
    fn test_nodejs_probe_never_resolves_a_bare_executable_name() {
        assert!(probe_node(Path::new("node")).is_none());
    }

    /// 确认随机后缀的长度与字符集符合设计。
    #[test]
    fn test_secure_installer_directory_name_uses_full_random_hex() {
        let suffix = secure_directory_suffix();

        assert_eq!(
            suffix.len(),
            NODEJS_SECURE_INSTALLER_DIRECTORY_RANDOM_BYTES * 2
        );
        assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    /// 确认 DACL 的关键要素在 SDDL 常量中未被削弱。
    #[test]
    fn test_secure_installer_directory_dacl_is_protected() {
        assert!(NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL.starts_with("D:P"));
        assert!(NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL.contains("WDWO;;;OW"));
        assert!(NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL.contains("FA;;;SY"));
        assert!(NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL.contains("FA;;;BA"));
    }

    /// 确认 SDDL 字符串能被系统成功解析为安全描述符。
    #[test]
    fn test_secure_installer_directory_dacl_is_valid_sddl() {
        let mut sddl = NODEJS_SECURE_INSTALLER_DIRECTORY_SDDL
            .encode_utf16()
            .collect::<Vec<_>>();
        sddl.push(0);
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();

        // 与生产代码相同的转换路径，此处仅验证 SDDL 字符串可被系统接受。
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
