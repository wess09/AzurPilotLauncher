//! 环境搭建模块：负责 AzurPilot 运行所需的完整环境生命周期。
//!
//! 主要职责（由 main.rs 在启动画面阶段的后台线程调用）：
//! - 定位 AzurLaneAutoScript 仓库目录并切换工作目录；
//! - 通过内嵌/系统 uv 安装 Python 3.14.6、创建可重定位 `.venv`，
//!   并将 uv / git / adb 一并部署进 `.venv`（`ensure_runtime_tools`）；
//! - 经 git 拉取最新仓库代码（`git_update`，最多重试 20 次）；
//! - 按 `uv.lock` 同步 Python 依赖（`uv_sync_project`）；
//! - 迁移与校验 `config/deploy.yaml` 部署配置（`migrate_dependency_config`）；
//! - 用户主动修复时，仅重建 `.venv`，保留代码、配置与缓存。
//!
//! 阶段进度通过 [`SplashUpdate`] 回调上报启动画面；所有长时间操作均支持
//! 经 `cancel_requested` 原子标志请求取消。

use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use reqwest::redirect::Policy;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::env::set_current_dir;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, RecvTimeoutError, Sender},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::window_util::CreateNoWindow as _;
use rust_i18n::t;

pub use crate::progress::SplashUpdate;

/// 启动画面小贴士总数，需与 locales 中 `tips.*` 键的数量保持一致。
const TIPS_COUNT: usize = 19;

/// 随机取一条启动画面小贴士（按当前时间戳取模轮换）。
pub fn get_tip() -> String {
    let now = Local::now().timestamp() as usize;
    let idx = now % TIPS_COUNT;
    let key = format!("tips.{idx}");
    t!(&key).to_string()
}

/// 脚本执行的阶段细分，用于在总进度中区分 git 更新与依赖同步两段。
#[derive(Clone, Copy, Debug)]
enum ScriptPhase {
    /// git 拉取仓库代码阶段。
    Git,
    /// uv 依赖同步阶段。
    Dependencies,
}

/// git 更新阶段的累计进度状态。
#[derive(Default)]
struct GitProgressState {
    /// 当前累计进度百分比。
    progress: f64,
}

/// uv sync 输出解析状态机：跟踪解析/下载/安装各阶段并估算整体进度。
struct UvProgressState {
    /// uv 进程启动时刻，用于计算整体耗时与速率。
    started_at: Instant,
    /// 首个包开始下载的时刻（用于估算下载速度）。
    download_started_at: Option<Instant>,
    /// 各包体积（字节），用于按量加权估算总进度。
    package_sizes: HashMap<String, u64>,
    /// 已完成下载的包名集合（去重，避免重复计数）。
    downloaded_packages: HashSet<String>,
    /// 累计已下载字节数。
    downloaded_bytes: u64,
    /// 依赖解析是否已完成。
    resolved: bool,
    /// 包准备（下载）是否已完成。
    prepared: bool,
    /// 安装是否已完成。
    installed: bool,
    /// 上一次上报的进度，用于保证进度单调不回退。
    last_progress: f64,
}

impl UvProgressState {
    /// 创建初始状态；进度从 2% 起步，0-2 预留给 uv 启动前的准备工作。
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            download_started_at: None,
            package_sizes: HashMap::new(),
            downloaded_packages: HashSet::new(),
            downloaded_bytes: 0,
            resolved: false,
            prepared: false,
            installed: false,
            last_progress: 2.0,
        }
    }
}

/// git 更新的最大重试次数：网络抖动时自动重试，20 次（约 20 秒）已能覆盖常见短暂断网。
const MAX_UPDATE_RETRIES: usize = 20;
/// 每次重试之间的固定间隔。
const RETRY_DELAY: Duration = Duration::from_secs(1);
/// 删除 `.venv` 等目录时的最大重试次数（文件被占用时按退避间隔重试）。
const CLEANUP_RETRIES: usize = 20;
/// 内置 Python 解释器版本，需与 CI 打包及 uv 安装的版本保持一致。
const PYTHON_VERSION: &str = "3.14.6";
/// 可复用 `.venv` 的最低 Python 版本：低于此版本说明环境过旧，直接删除重建以保证一致性。
const MIN_REUSABLE_VENV_PYTHON_VERSION: (u16, u16, u16) = (3, 14, 5);
/// uv 安装 Python standalone 的默认镜像列表：前两条为国内镜像用于加速，
/// 后两条为官方源兜底；按顺序尝试直到成功。
const DEFAULT_UV_PYTHON_INSTALL_MIRRORS: &[&str] = &[
    "https://registry.npmmirror.com/-/binary/python-build-standalone/",
    "https://mirror.nju.edu.cn/github-release/astral-sh/python-build-standalone/",
    "https://python-standalone.org/mirror/astral-sh/python-build-standalone/",
    "https://downloads.astral.sh/python/",
    "https://github.com/astral-sh/python-build-standalone/releases/download/",
];
/// 默认 PyPI 索引（国际源）。
const DEFAULT_PYPI_INDEX: &str = "https://pypi.org/simple/";
/// 内置的国内 PyPI 镜像列表：国际源不可用时按顺序回退。
const BUILTIN_PYPI_INDEXES: &[&str] = &[
    "https://mirrors.aliyun.com/pypi/simple/",
    "https://mirrors.cloud.tencent.com/pypi/simple/",
    "https://repo.huaweicloud.com/repository/pypi/simple/",
    "https://mirrors.cernet.edu.cn/pypi/web/simple/",
];
/// 编译期内嵌的 uv 引导二进制：由 build.rs 依据 `AZURPILOT_BOOTSTRAP_UV` 写入；
/// 空文件表示本地开发构建未内嵌，运行时回退到 `UV` 环境变量或 PATH 中的 uv。
const BOOTSTRAP_UV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bootstrap_uv.bin"));

/// 读取当前平台对应的内置 deploy.yaml 基线内容（编译期嵌入）。
fn default_deploy_config() -> &'static str {
    #[cfg(windows)]
    {
        include_str!("../deploy.windows.yaml")
    }
    #[cfg(target_os = "macos")]
    {
        include_str!("../deploy.mac.yaml")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        include_str!("../deploy.unix.yaml")
    }
}

/// 返回 deploy.yaml 中 Python 可执行文件的平台约定路径（.venv 内相对路径）。
fn platform_python_config_path() -> &'static str {
    if cfg!(windows) {
        "./.venv/Scripts/python.exe"
    } else {
        "./.venv/bin/python"
    }
}

/// 返回 deploy.yaml 中 adb 可执行文件的平台约定路径。
fn platform_adb_config_path() -> &'static str {
    if cfg!(windows) {
        "./.venv/Scripts/adb.exe"
    } else {
        "./.venv/bin/adb"
    }
}

/// 返回 deploy.yaml 中 git 可执行文件的平台约定路径。
fn platform_git_config_path() -> &'static str {
    if cfg!(windows) {
        "./.venv/Scripts/git/cmd/git.exe"
    } else {
        "./.venv/bin/git"
    }
}

/// 定位 AzurLaneAutoScript 仓库根目录。
///
/// 优先按"便携式同目录发行"查找：启动器可执行文件所在目录若存在
/// `deploy/installer.py` 即认为是仓库根（Windows/Linux 的常规布局）。
/// macOS 额外支持 `.app` 包布局：从 `AzurPilot.app/Contents/MacOS`
/// 回退到 `AzurPilot.app/Contents/AzurLaneAutoScript`。
///
/// # Panics
/// 两种布局都未命中时 panic——启动器必须与仓库同发行，无法继续运行。
pub fn azurpilot_repo_dir() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("AZURPILOT_REPO_DIR") {
        let root = PathBuf::from(root)
            .canonicalize()
            .expect("AZURPILOT_REPO_DIR must exist");
        assert!(
            root.join("deploy/installer.py").is_file(),
            "AZURPILOT_REPO_DIR must contain deploy/installer.py"
        );
        return root;
    }

    // 优先检查常规的"同目录便携发行"布局
    let exe_folder = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut installer_py = exe_folder.clone();
    installer_py.extend(["deploy", "installer.py"]);
    if fs::exists(installer_py).unwrap() {
        return exe_folder;
    }
    // macOS 需额外考虑 .app 包布局：AzurPilot.app/Contents/AzurLaneAutoScript
    #[cfg(target_os = "macos")]
    {
        use std::ffi::OsStr;
        if exe_folder.file_name() == Some(OsStr::new("MacOS")) {
            let mut repo_folder = exe_folder;
            repo_folder.pop();
            repo_folder.push("AzurLaneAutoScript");
            if fs::exists(&repo_folder).unwrap() {
                return repo_folder;
            }
        }
    }
    panic!("Cannot find AzurPilot repo folder");
}

/// 把给定路径前插到指定环境变量（PATH 类）的首位，使其优先级最高。
///
/// 用于让 `.venv` 内的工具链（uv/git/adb）压过系统同名工具，
/// 保证启动流程使用的是受控版本。
fn prepend_path_to_env(key: &str, path: PathBuf) {
    let mut paths = Vec::new();
    paths.push(path);
    if let Some(ref old_path) = &std::env::var_os(key) {
        paths.extend(std::env::split_paths(old_path));
    }
    std::env::set_var(key, std::env::join_paths(paths).unwrap());
}

/// 仓库内虚拟环境目录：`.venv`（所有平台一致）。
fn venv_dir() -> PathBuf {
    azurpilot_repo_dir().join(".venv")
}

/// `.venv` 的可执行文件目录：Windows 为 `Scripts`，Unix 为 `bin`。
fn venv_bin_dir() -> PathBuf {
    let venv = venv_dir();
    if cfg!(windows) {
        venv.join("Scripts")
    } else {
        venv.join("bin")
    }
}

/// `.venv` 内 Python 解释器的完整路径。
pub fn venv_python() -> PathBuf {
    venv_bin_dir().join(if cfg!(windows) {
        "python.exe"
    } else {
        "python"
    })
}

/// uv managed Python 的实际安装目录（`.venv/python`），与标准 venv 布局并存。
fn venv_python_install_dir() -> PathBuf {
    venv_dir().join("python")
}

/// `.venv` 内 uv 的完整路径。
fn venv_uv() -> PathBuf {
    venv_bin_dir().join(if cfg!(windows) { "uv.exe" } else { "uv" })
}

/// `.venv` 内 adb 的完整路径。
fn venv_adb() -> PathBuf {
    venv_bin_dir().join(if cfg!(windows) { "adb.exe" } else { "adb" })
}

/// `.venv` 内 git 的完整路径；Windows 发行版为 MinGit 完整布局
/// （`Scripts/git/cmd/git.exe`），Unix 为单文件 `bin/git`。
fn venv_git() -> PathBuf {
    if cfg!(windows) {
        venv_bin_dir().join("git").join("cmd").join("git.exe")
    } else {
        venv_bin_dir().join("git")
    }
}

/// git 的 `GIT_EXEC_PATH` 目录（Windows/Unix 发行包布局均含 `libexec/git-core`）。
fn venv_git_exec_path() -> PathBuf {
    venv_dir().join("libexec").join("git-core")
}

/// git 模板目录（`share/git-core/templates`），供 `GIT_TEMPLATE_DIR` 使用。
fn venv_git_template_dir() -> PathBuf {
    venv_dir().join("share").join("git-core").join("templates")
}

/// 取得本次进程用于引导的 uv 可执行文件路径。
///
/// 优先级：内嵌二进制（释放到带进程 ID 的临时目录，避免多实例冲突）>
/// `UV` 环境变量 > PATH 搜索。本地开发构建未内嵌 uv 时走后两者。
///
/// # Errors
/// 未内嵌且系统上找不到 uv 时返回 Err（提示用户 uv 未找到）。
fn bootstrap_uv_path() -> Result<PathBuf> {
    // 临时目录带进程 ID：防止同一台机器并行运行多个启动器实例时互相覆盖
    let dir = std::env::temp_dir().join(format!("azurpilot-bootstrap-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    let path = dir.join(if cfg!(windows) { "uv.exe" } else { "uv" });
    if !path.exists() {
        if BOOTSTRAP_UV.is_empty() {
            if let Some(path_uv) = std::env::var_os("UV").map(PathBuf::from) {
                return Ok(path_uv);
            }
            if let Some(path_uv) = find_on_path("uv") {
                return Ok(path_uv);
            }
            bail!(t!("errors.uv_not_found"));
        }
        fs::write(&path, BOOTSTRAP_UV)
            .with_context(|| t!("errors.write_uv_failed", path = path.display().to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions)?;
        }
    }
    Ok(path)
}

/// 在 PATH 各目录中查找可执行文件（Windows 自动尝试 `.exe` 后缀）。
fn find_on_path(executable: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(executable);
        if candidate.exists() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{executable}.exe"));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 建立后续所有子进程的基础环境：切换工作目录到仓库根，
/// 并把 `.venv` 工具链目录提升到 PATH 最前。
///
/// # Errors
/// 仓库目录不存在或切换工作目录失败时返回 Err。
pub fn setup_environment() -> Result<()> {
    let dir = azurpilot_repo_dir();
    info!("AzurPilot dir is {:?}", &dir);
    set_current_dir(&dir)?;
    prepend_path_to_env("PATH", venv_bin_dir());
    if cfg!(windows) {
        // Windows MinGit 布局的 git.exe 藏在 git/cmd 下，需单独加入 PATH
        prepend_path_to_env("PATH", venv_bin_dir().join("git").join("cmd"));
    } else {
        refresh_git_environment();
    }
    Ok(())
}

/// 设置 Unix 发行包 git 所需的环境变量（执行路径与模板目录）。
///
/// 仅在对应目录真实存在时设置，避免把不存在的路径传给 git。
fn refresh_git_environment() {
    let git_exec_path = venv_git_exec_path();
    if git_exec_path.exists() {
        std::env::set_var("GIT_EXEC_PATH", git_exec_path);
    }

    let git_template_dir = venv_git_template_dir();
    if git_template_dir.exists() {
        std::env::set_var("GIT_TEMPLATE_DIR", git_template_dir);
    }
}

/// Linux 专用：探测系统 CA 证书并配置给 git。
///
/// 内嵌 git 无法自动找到发行版的 CA 证书，git 拉取 HTTPS 仓库会失败；
/// 此处经 openssl-probe 定位证书文件，同时写入进程环境与仓库本地配置。
#[cfg(target_os = "linux")]
fn setup_git_ca_bundle() {
    let cert_file = openssl_probe::probe().cert_file;
    if let Some(file) = cert_file.as_ref().and_then(|f| f.to_str()) {
        std::env::set_var("GIT_SSL_CAINFO", file);
        let _ = Command::new("git")
            .args(["config", "--local", "http.sslCAInfo", file])
            .status();
    }
}

/// 环境搭建总入口：按序完成工具安装 → 配置迁移 → 仓库更新 → 依赖同步。
///
/// 流程与 `deploy/installer.py` 保持一致，各阶段经 `status_updater`
/// 上报启动画面进度；`cancel_requested` 允许取消窗口随时中止全流程。
///
/// # Errors
/// 任一阶段失败（下载、更新、同步、配置迁移）或用户取消时返回 Err，
/// 由 main.rs 决定展示错误页面。
pub fn setup_azurpilot_repo(
    mut status_updater: impl FnMut(SplashUpdate),
    cancel_requested: Arc<AtomicBool>,
    skip_repository_update: bool,
) -> Result<()> {
    info!("Starting setup for AzurPilot repository...");
    // Linux 的内嵌 git 需要显式指定 CA 证书，必须最先配置
    #[cfg(target_os = "linux")]
    setup_git_ca_bundle();
    // 流程与 deploy/installer.py 保持一致
    status_updater(
        SplashUpdate::loading(
            t!("setup.preparing_workspace"),
            t!("setup.cleaning_cache"),
            8,
        )
        .with_subtitle(t!("setup.checking_env", tip = get_tip())),
    );
    let bootstrap_uv = bootstrap_uv_path()?;
    ensure_runtime_tools(&bootstrap_uv, &cancel_requested, &mut status_updater)?;
    atomic_failure_cleanup("./config", &cancel_requested)?;
    migrate_dependency_config()?;
    if skip_repository_update {
        info!("Skipping AzurPilot repository update because preview no-update mode is enabled");
        status_updater(
            SplashUpdate::loading(
                t!("setup.skipping_update"),
                t!("setup.skipping_update_detail"),
                18,
            )
            .with_subtitle(t!("setup.preview_mode", tip = get_tip())),
        );
    } else {
        status_updater(
            SplashUpdate::loading(t!("setup.updating"), t!("setup.fetching_patches"), 18)
                .with_subtitle(t!("setup.syncing", tip = get_tip())),
        );
        git_update(&mut status_updater, &bootstrap_uv, &cancel_requested)?;
    }
    status_updater(
        SplashUpdate::loading(t!("setup.installing_deps"), t!("setup.verifying_deps"), 64)
            .with_subtitle(t!("setup.syncing_deps", tip = get_tip())),
    );
    uv_sync_project(&mut status_updater, &bootstrap_uv, &cancel_requested)?;
    status_updater(
        SplashUpdate::loading(t!("setup.finishing"), t!("setup.ready_to_launch"), 94)
            .with_subtitle(t!("setup.launching", tip = get_tip())),
    );
    Ok(())
}

/// 用户主动修复：仅删除 `.venv` 并重建工具与依赖。
///
/// 与完整 setup 不同，此流程跳过 git 更新，迁移工具路径配置并同步
/// Python 依赖；只在用户确认后执行。
///
/// # Errors
/// 用户取消、`.venv` 删除失败或依赖同步失败时返回 Err。
pub fn repair_environment(
    mut status_updater: impl FnMut(SplashUpdate),
    cancel_requested: Arc<AtomicBool>,
) -> Result<()> {
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("launcher.cancelled"));
    }

    status_updater(
        SplashUpdate::loading(t!("setup.preparing_env"), t!("launcher.repair_body"), 4)
            .with_subtitle(t!("setup.rebuilding_env", tip = get_tip())),
    );
    remove_venv_for_repair(&cancel_requested)?;

    let bootstrap_uv = bootstrap_uv_path()?;
    ensure_runtime_tools(&bootstrap_uv, &cancel_requested, &mut status_updater)?;
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("launcher.cancelled"));
    }

    status_updater(
        SplashUpdate::loading(t!("setup.installing_deps"), t!("setup.verifying_deps"), 64)
            .with_subtitle(t!("setup.syncing_deps", tip = get_tip())),
    );
    migrate_dependency_config()?;
    uv_sync_project(&mut status_updater, &bootstrap_uv, &cancel_requested)
}

/// 读取解析后的 `config/deploy.yaml` 内容；文件缺失或格式错误时返回 `None`。
pub fn get_deploy_config() -> Option<JsonValue> {
    let config_content = fs::read_to_string("./config/deploy.yaml").ok()?;
    let config: JsonValue = serde_yaml::from_str(&config_content).ok()?;
    Some(config)
}

/// 递归删除目录/文件，删除前先清除只读属性。
///
/// Windows 上处于只读属性的文件无法直接删除，需先清位；
/// 目录按先清空后删除的顺序递归处理。
///
/// # Errors
/// 任一文件或目录删除失败时返回 Err（含 i18n 错误信息）。
fn remove_runtime_entry(path: &Path) -> Result<()> {
    clear_readonly(path)?;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            remove_runtime_entry(&entry?.path())?;
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

/// 带重试的删除：被杀进程尚未完全释放文件时退避重试。
///
/// 每次失败后检查目标是否已消失（可能被并发进程删掉），消失即成功。
///
/// # Errors
/// 重试次数用尽仍失败时返回最后一次的错误。
fn remove_runtime_entry_with_retry(path: &Path) -> Result<()> {
    let mut last_error = None;
    for attempt in 0..CLEANUP_RETRIES {
        match remove_runtime_entry(path) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_error = Some(err);
                if !path.exists() {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(250 + attempt as u64 * 100));
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

/// 后端恢复流程中删除 `.venv`，删除前做严格的安全校验。
///
/// 校验目的：`.venv` 必须是真实目录（不能是符号链接/Reparse Point）
/// 且规范化后必须位于仓库目录之内——防止环境变量被篡改后
/// 误删仓库外的任意目录。
///
/// # Errors
/// 用户取消、路径校验不通过或删除失败时返回 Err。
fn remove_venv_for_repair(cancel_requested: &AtomicBool) -> Result<()> {
    let repo_dir = azurpilot_repo_dir().canonicalize()?;
    let venv = venv_dir();
    let venv_metadata = match fs::symlink_metadata(&venv) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !venv_metadata.is_dir() || is_symlink_or_reparse_point(&venv_metadata) {
        bail!(t!(
            "errors.refuse_cleanup",
            actual = venv.display().to_string(),
            expected = repo_dir.display().to_string()
        ));
    }
    let venv_target = venv.canonicalize()?;
    if venv_target == repo_dir || !venv_target.starts_with(&repo_dir) {
        bail!(t!(
            "errors.refuse_cleanup",
            actual = venv_target.display().to_string(),
            expected = repo_dir.display().to_string()
        ));
    }

    remove_venv_path_for_repair(&venv, cancel_requested).with_context(|| {
        t!(
            "errors.reset_venv_failed",
            error = venv.display().to_string()
        )
    })
}

/// 实际执行 `.venv` 删除（带取消检查与重试），仅记录日志后委托重试逻辑。
///
/// # Errors
/// 用户取消或删除重试用尽仍失败时返回 Err。
fn remove_venv_path_for_repair(venv: &Path, cancel_requested: &AtomicBool) -> Result<()> {
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("launcher.cancelled"));
    }
    if !venv.exists() {
        return Ok(());
    }

    info!(
        "Removing {} for explicit environment repair",
        venv.display()
    );
    remove_venv_entry_with_retry(venv, cancel_requested)?;
    if cancel_requested.load(Ordering::SeqCst) {
        bail!(t!("launcher.cancelled"));
    }
    Ok(())
}

/// 带取消检查与退避重试的删除循环（恢复流程专用）。
///
/// 与 [`remove_runtime_entry_with_retry`] 的区别在于每次尝试前后
/// 都检查取消标志，保证用户取消时能尽快中止。
///
/// # Errors
/// 用户取消或重试用尽仍失败时返回 Err。
fn remove_venv_entry_with_retry(path: &Path, cancel_requested: &AtomicBool) -> Result<()> {
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

/// 在独立线程中逐块读取子进程输出流，按自定义协议切分事件并转发。
///
/// git/uv 的进度输出使用"前缀:内容"格式（如 uv 的 `Resolved: ...`），
/// 本函数按 ASCII 范围识别可打印字符：遇到非可打印字符（进度条
/// 回车重绘、二进制块等）立即截断发送，避免进度条刷屏；遇到冒号
/// 时若发现重复前缀模式则切分出完整事件提前发送。最终每个事件以
/// `(is_err, 文本)` 形式送入通道，由主循环统一消费。
fn pipe_lines(read: impl Read + Send + 'static, tx: Sender<(bool, String)>, is_err: bool) {
    thread::spawn(move || {
        let mut reader = BufReader::new(read);
        let mut buffer = "".to_owned();
        loop {
            let mut line = [0u8; 64];
            match reader.read(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    for c in &line[0..size] {
                        // 非 ASCII 可打印字符：视为进度重绘等噪声，截断当前缓冲
                        if *c < 32 || *c > 127 {
                            if !buffer.is_empty() {
                                let _ = tx.send((is_err, buffer));
                                buffer = "".to_owned();
                            }
                        } else if *c as char == ':' {
                            // "A:A..." 形式的重复前缀说明上一事件已完整，切分发送
                            let mut cut = 0usize;
                            if let Some((l, r)) = buffer.split_once(':') {
                                if r.ends_with(l) {
                                    cut = r.len() + 1;
                                }
                            }
                            if cut > 0 {
                                let (l, r) = buffer.split_at(cut);
                                let _ = tx.send((is_err, l.to_owned()));
                                buffer = r.to_owned();
                            }
                            buffer.push(*c as char);
                        } else {
                            buffer.push(*c as char);
                        }
                    }
                }
            }
        }
        if !buffer.is_empty() {
            let _ = tx.send((is_err, buffer));
        }
    });
}

/// 运行一个长时间子命令（git 更新或 uv 同步），实时解析输出并上报进度。
///
/// 子进程的 stdout/stderr 各由一个读取线程消费（见 [`pipe_lines`]），
/// 主循环经通道收取事件：可识别为进度的行驱动启动画面更新，普通
/// 输出写日志，stderr 行同时记为"最后错误"用于失败时展示。
/// 依赖阶段每 100ms 按实际耗时刷新等待进度，持续有日志时同样刷新。
///
/// # Errors
/// 用户取消（会先杀掉子进程）、子进程启动失败或非零退出时返回 Err。
fn run_command(
    cmd: &mut Command,
    mut status_updater: impl FnMut(SplashUpdate),
    phase: ScriptPhase,
    cancel_requested: &AtomicBool,
) -> Result<()> {
    let is_deps = matches!(phase, ScriptPhase::Dependencies);

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = crate::task_process::TaskChild::spawn(cmd)?;

    // 接收读取线程输出的通道：(是否 stderr, 行文本)
    let (tx, rx) = mpsc::channel::<(bool, String)>();

    // 为 stdout 启动一个读取线程
    if let Some(stdout) = child.take_stdout() {
        pipe_lines(stdout, tx.clone(), false);
    }

    // 为 stderr 启动一个读取线程
    if let Some(stderr) = child.take_stderr() {
        pipe_lines(stderr, tx.clone(), true);
    }

    // 主动丢弃原始 sender：两个读取线程结束后通道即关闭，主循环可退出
    drop(tx);

    let mut last_err = "".to_owned();
    let mut git_progress = GitProgressState::default();
    let mut uv_progress = UvProgressState::new();
    // 依赖阶段起点 64%：前半段留给 git 更新，保证总进度单调
    let mut dependency_progress = 64.0;
    let phase_subtitle = match phase {
        ScriptPhase::Git => t!("setup.syncing", tip = get_tip()).to_string(),
        ScriptPhase::Dependencies => t!("setup.syncing_deps", tip = get_tip()).to_string(),
    };
    let mut dependency_detail = None;
    let started_at = Instant::now();
    let mut last_wait_update = started_at;
    const UPDATE_INTERVAL: Duration = Duration::from_millis(100);

    // 主循环：收取输出行，转发到日志与启动画面进度回调
    loop {
        if cancel_requested.load(Ordering::SeqCst) {
            // 取消时必须先杀子进程再退出，防止 gui 安装/下载残留
            let _ = child.kill();
            let _ = child.wait();
            bail!(t!("launcher.cancelled"));
        }
        // 短超时保证无输出时的取消与进度刷新也及时响应。
        match rx.recv_timeout(UPDATE_INTERVAL) {
            Ok((is_err, line)) => {
                if let Some(mut update) =
                    splash_update_for_output(&line, phase, &mut git_progress, &mut uv_progress)
                {
                    // 进度只前进不回退（max 保护），避免输出乱序导致画面跳动
                    if is_deps {
                        update.progress = update.progress.max(dependency_progress);
                        dependency_progress = update.progress;
                        dependency_detail = Some(update.detail.clone());
                    }
                    update.subtitle = phase_subtitle.clone();
                    status_updater(update);
                }

                if is_err {
                    // uv 的进度日志走 stderr，属正常输出降级为 info；
                    // 其余 stderr 视为错误信息并记住最后一行用于失败提示
                    if is_deps && is_uv_progress_line(&line) {
                        info!("{line}");
                    } else {
                        warn!("{line}");
                        last_err = line;
                    }
                } else {
                    info!("{line}");
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // 读取线程结束后仍需轮询子进程退出，避免它主动关闭输出
                // 后继续运行时阻塞在 wait()，让取消与等待进度保持响应。
                if child.try_wait()?.is_some() {
                    break;
                }
                thread::sleep(UPDATE_INTERVAL);
            }
        }

        // 进度时钟独立于日志频率，避免持续有普通日志时等待进度停住，
        // 也避免把“超时次数”误当成真实耗时。
        if is_deps && last_wait_update.elapsed() >= UPDATE_INTERVAL {
            let mut update = dependency_wait_update(
                started_at.elapsed().as_secs(),
                dependency_progress,
                &mut uv_progress,
            );
            update.subtitle = phase_subtitle.clone();
            if let Some(detail) = &dependency_detail {
                update.detail = detail.clone();
            }
            dependency_progress = update.progress;
            status_updater(update);
            last_wait_update = Instant::now();
        }
    }

    // 等待子进程退出并检查退出码
    let status = child.wait()?;
    if !status.success() {
        // stderr 为空时用阶段默认文案兜底，保证错误提示永远有内容
        if last_err.is_empty() {
            last_err = match phase {
                ScriptPhase::Git => t!("setup.update_failed").to_string(),
                ScriptPhase::Dependencies => t!("setup.deps_failed").to_string(),
            };
        }
        return Err(anyhow!(last_err));
    }
    Ok(())
}

/// 带重试的命令执行封装：失败后按固定间隔重试至 [`MAX_UPDATE_RETRIES`] 次。
///
/// 每次失败都会向启动画面上报"第 N/20 次重试"状态；取消请求在
/// 每轮循环开头检查。
///
/// # Errors
/// 用户取消或重试用尽仍失败时返回最后一次的错误。
fn run_command_with_retry(
    build_cmd: impl Fn() -> Command,
    mut status_updater: impl FnMut(SplashUpdate),
    phase: ScriptPhase,
    cancel_requested: &AtomicBool,
) -> Result<()> {
    for retry in 0..=MAX_UPDATE_RETRIES {
        if cancel_requested.load(Ordering::SeqCst) {
            bail!(t!("launcher.cancelled"));
        }

        match run_command(
            &mut build_cmd(),
            &mut status_updater,
            phase,
            cancel_requested,
        ) {
            Ok(()) => return Ok(()),
            Err(err) => {
                if retry == MAX_UPDATE_RETRIES {
                    return Err(err);
                }

                let retry_count = retry + 1;
                let error_text = err.to_string();
                warn!(
                    "{} failed (retry {retry_count}/{MAX_UPDATE_RETRIES}): {error_text}",
                    phase_display_name(phase)
                );
                status_updater(splash_retry_update(phase, retry_count, &error_text));
                thread::sleep(RETRY_DELAY);
            }
        }
    }

    unreachable!()
}

/// 运行子命令并等待退出状态，支持取消，无进度回调。
///
/// 供环境搭建流程中的短命令使用（文件复制、目录创建等），
/// 不解析输出、不上报进度。
///
/// # Errors
/// 用户取消或子进程启动失败时返回 Err（退出码非零由调用方判断）。
pub fn run_status_command(
    cmd: &mut Command,
    cancel_requested: &AtomicBool,
) -> Result<std::process::ExitStatus> {
    run_status_command_with_tick(cmd, cancel_requested, || {})
}

/// 带周期回调的 [`run_status_command`] 内部实现。
///
/// `on_tick` 在每次轮询（约 100ms）时触发，供调用方刷新
/// "仍在等待"类的状态展示。
///
/// # Errors
/// 用户取消（先杀子进程）或启动失败时返回 Err。
fn run_status_command_with_tick(
    cmd: &mut Command,
    cancel_requested: &AtomicBool,
    mut on_tick: impl FnMut(),
) -> Result<std::process::ExitStatus> {
    let mut child = crate::task_process::TaskChild::spawn(cmd)?;
    loop {
        if cancel_requested.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            bail!(t!("launcher.cancelled"));
        }

        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        on_tick();
        thread::sleep(Duration::from_millis(100));
    }
}

fn phase_display_name(phase: ScriptPhase) -> String {
    match phase {
        ScriptPhase::Git => t!("setup.code_update").to_string(),
        ScriptPhase::Dependencies => t!("setup.deps_update").to_string(),
    }
}

fn splash_retry_update(phase: ScriptPhase, retry_count: usize, error_text: &str) -> SplashUpdate {
    let detail = t!(
        "setup.retry_detail",
        count = retry_count.to_string(),
        max = MAX_UPDATE_RETRIES.to_string(),
        error = error_text
    );
    match phase {
        ScriptPhase::Git => SplashUpdate::loading(t!("setup.retrying_update"), detail, 18)
            .with_subtitle(t!("setup.syncing", tip = get_tip())),
        ScriptPhase::Dependencies => SplashUpdate::loading(t!("setup.retrying_deps"), detail, 64)
            .with_subtitle(t!("setup.syncing_deps", tip = get_tip()))
            .with_uv_progress(2, t!("setup.uv_resolving", secs = "0")),
    }
}

/// 构造依赖同步阶段的起始进度（64%，uv 子进度从 2% 开始解析）。
fn dependency_start_update() -> SplashUpdate {
    SplashUpdate::loading(t!("setup.installing_deps"), t!("setup.uv_parsing"), 64)
        .with_subtitle(t!("setup.syncing_deps", tip = get_tip()))
        .with_uv_progress(2, t!("setup.uv_resolving", secs = "0"))
}

/// 子命令成功退出后确认依赖同步完成，推进到本阶段的终点。
fn dependency_complete_update() -> SplashUpdate {
    SplashUpdate::loading(t!("setup.installing_deps"), t!("setup.uv_completed"), 90)
        .with_subtitle(t!("setup.syncing_deps", tip = get_tip()))
        .with_uv_progress(100, t!("setup.uv_completed"))
}

/// 构造依赖同步阶段"等待中"的进度（子命令暂时无输出时保持画面活跃）。
///
/// 按已等待秒数切换文案（10 秒内显示"解析中"，之后显示已耗时），
/// 进度取当前值与全局 uv 进度的较大者，保证单调。
fn dependency_wait_update(
    elapsed_secs: u64,
    current_progress: f64,
    uv_progress: &mut UvProgressState,
) -> SplashUpdate {
    let progress = current_progress.max(dependency_global_progress(uv_progress.progress()));
    let detail = if elapsed_secs < 10 {
        t!("setup.uv_parsing").to_string()
    } else {
        t!("setup.uv_syncing", secs = elapsed_secs.to_string()).to_string()
    };

    SplashUpdate::loading(t!("setup.installing_deps"), detail, progress)
        .with_subtitle(t!("setup.syncing_deps", tip = get_tip()))
        .with_uv_progress(uv_progress.progress(), uv_progress.detail())
}

/// 经内嵌 Python 脚本调用 `deploy.git.GitManager` 拉取最新仓库代码。
///
/// 不直接调用 git，而是复用上游 AzurPilot 的 Python 更新逻辑，保证更新
/// 行为（远程配置、冲突处理）与官方安装器一致；脚本会给 `execute`
/// 打补丁，为 fetch 命令强制加上 `--progress` 以便解析进度条。
/// 失败时由 [`run_command_with_retry`] 自动重试（最多 20 次）。
///
/// # Errors
/// 用户取消或重试用尽仍失败时返回 Err。
fn git_update(
    status_updater: impl FnMut(SplashUpdate),
    bootstrap_uv: &Path,
    cancel_requested: &AtomicBool,
) -> Result<()> {
    // Decorate execute() to get fetch progress
    let script = r#"
import deploy.git
def decorate_execute(fn):
    def new_fn(*args, **kwargs):
        if len(args) >= 1 and ' fetch ' in args[0] and '--progress' not in args[0]:
            args = (args[0].replace(' fetch ', ' fetch --progress '),) + args[1:]
        return fn(*args, **kwargs)
    return new_fn
gm = deploy.git.GitManager()
gm.execute = decorate_execute(gm.execute)
gm.git_install()
"#;
    let python = venv_python();
    let bootstrap_uv = bootstrap_uv.to_path_buf();
    run_command_with_retry(
        || {
            let mut cmd = Command::new(&python);
            cmd.args(["-u", "-c", script])
                .env("AZURPILOT_BOOTSTRAP_UV", &bootstrap_uv);
            isolate_python_child_environment(&mut cmd);
            bypass_proxy_for_child(&mut cmd);
            cmd
        },
        status_updater,
        ScriptPhase::Git,
        cancel_requested,
    )
}

/// 按 `uv.lock` 同步 Python 依赖，带 PyPI 镜像源的多级回退。
///
/// 依次尝试 [`ranked_pypi_indexes`] 排序后的镜像源：每个源先重锁
/// `uv.lock` 再同步；同步失败会重锁重试一次，仍失败则换下一个源。
/// 全部源用尽后返回最后一次错误。
///
/// # Errors
/// 用户取消或所有候选源均失败时返回 Err。
fn uv_sync_project(
    mut status_updater: impl FnMut(SplashUpdate),
    bootstrap_uv: &Path,
    cancel_requested: &AtomicBool,
) -> Result<()> {
    let bootstrap_uv = bootstrap_uv.to_path_buf();
    // 独立同步任务从新子进度开始；同一任务内部的换源与重锁沿用任务。
    status_updater(dependency_start_update().with_new_uv_task());
    let indexes = ranked_pypi_indexes();
    let next_indexes = indexes.clone();
    let mut last_error = None;

    for (attempt, index) in indexes.iter().enumerate() {
        if cancel_requested.load(Ordering::SeqCst) {
            bail!(t!("launcher.cancelled"));
        }

        info!("Syncing dependencies with PyPI index: {index}");
        status_updater(dependency_start_update());

        // uv.lock 锁定了下载 URL，且 uv 只有在锁定的 URL 失效时才会重写，
        // 所以必须先把锁解析到当前镜像源上——这决定了实际从哪个源下载；
        // 无法完成解析的源等于不可用，直接跳过。
        if !relock_onto_index(
            &bootstrap_uv,
            index,
            &mut status_updater,
            cancel_requested,
            &mut last_error,
        ) {
            if !disable_index(index, &next_indexes, attempt, &mut status_updater) {
                break;
            }
            continue;
        }

        if sync_succeeds(
            &bootstrap_uv,
            index,
            &mut status_updater,
            cancel_requested,
            &mut last_error,
        ) {
            return Ok(());
        }

        // 探测可达的镜像仍可能在下载环节失败（403、限流、CDN 损坏、
        // 包格式不被 uv 识别等），先重锁一次再试，然后才判定该源不可用。
        info!("Retrying dependency sync with relocked URLs for {index}");
        if relock_onto_index(
            &bootstrap_uv,
            index,
            &mut status_updater,
            cancel_requested,
            &mut last_error,
        ) && sync_succeeds(
            &bootstrap_uv,
            index,
            &mut status_updater,
            cancel_requested,
            &mut last_error,
        ) {
            return Ok(());
        }

        if !disable_index(index, &next_indexes, attempt, &mut status_updater) {
            break;
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!(t!("setup.deps_failed").to_string())))
}

/// 重锁 `uv.lock`，使其下载 URL 全部指向指定镜像源；失败记录到 last_error。
///
/// 返回 true 表示重锁成功、该源可用；false 表示源无法解析依赖，
/// 应跳过换源。
fn relock_onto_index(
    bootstrap_uv: &Path,
    index: &str,
    status_updater: &mut impl FnMut(SplashUpdate),
    cancel_requested: &AtomicBool,
    last_error: &mut Option<anyhow::Error>,
) -> bool {
    match uv_relock_project(bootstrap_uv, index, status_updater, cancel_requested) {
        Ok(()) => {
            info!("Re-resolved uv.lock onto PyPI index: {index}");
            true
        }
        Err(err) => {
            warn!("Failed to relock dependencies against {index}: {err}");
            *last_error = Some(err);
            false
        }
    }
}

/// 执行一次依赖同步尝试；失败时记录错误并返回 false，由调用方决定回退。
fn sync_succeeds(
    bootstrap_uv: &Path,
    index: &str,
    status_updater: &mut impl FnMut(SplashUpdate),
    cancel_requested: &AtomicBool,
    last_error: &mut Option<anyhow::Error>,
) -> bool {
    let mut cmd = uv_sync_command(bootstrap_uv, index);
    match run_command(
        &mut cmd,
        &mut *status_updater,
        ScriptPhase::Dependencies,
        cancel_requested,
    ) {
        Ok(()) => {
            // 只有 sync 退出成功才能报完成；安装日志仍可能先于失败退出码。
            status_updater(dependency_complete_update());
            true
        }
        Err(err) => {
            warn!("Dependency sync failed with PyPI index {index}: {err}");
            *last_error = Some(err);
            false
        }
    }
}

/// 切换到下一个候选镜像源；返回 false 表示已无源可试，应终止流程。
fn disable_index(
    index: &str,
    next_indexes: &[String],
    attempt: usize,
    status_updater: &mut impl FnMut(SplashUpdate),
) -> bool {
    warn!("Disabling PyPI index {index} for this run");
    let Some(next_index) = next_indexes.get(attempt + 1) else {
        return false;
    };
    status_updater(pypi_index_fallback_update(next_index));
    thread::sleep(RETRY_DELAY);
    true
}

/// 重写 `uv.lock` 中的下载 URL，使其单独解析到指定镜像源。
///
/// 镜像源经临时配置文件传入——只有这种方式能盖过项目自带的
/// `[tool.uv]` 索引列表，`--default-index` 做不到。
///
/// # Errors
/// 用户取消或 `uv lock` 执行失败时返回 Err。
fn uv_relock_project(
    bootstrap_uv: &Path,
    index: &str,
    status_updater: &mut impl FnMut(SplashUpdate),
    cancel_requested: &AtomicBool,
) -> Result<()> {
    let override_file = UvIndexOverride::create(index)?;
    let mut cmd = uv_lock_command(bootstrap_uv, override_file.path(), index);
    run_command(
        &mut cmd,
        status_updater,
        ScriptPhase::Dependencies,
        cancel_requested,
    )
}

/// 临时 `uv.toml`，仅声明目标镜像源；Drop 时自动清理其所在目录。
struct UvIndexOverride {
    /// 临时目录（含进程 ID，避免多实例冲突），Drop 时整体删除。
    dir: PathBuf,
    /// `uv.toml` 文件路径，传给 `--config-file`。
    path: PathBuf,
}

impl UvIndexOverride {
    /// 创建临时配置目录并写入 `index-url = "<index>"`。
    ///
    /// # Errors
    /// 目录创建或文件写入失败时返回 Err（已写入部分会被清理）。
    fn create(index: &str) -> Result<Self> {
        let dir = std::env::temp_dir().join(format!("azurpilot-relock-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let path = dir.join("uv.toml");
        if let Err(err) = fs::write(&path, format!("index-url = \"{index}\"\n")) {
            let _ = fs::remove_dir_all(&dir);
            return Err(err.into());
        }
        Ok(Self { dir, path })
    }

    /// 返回临时 `uv.toml` 的路径。
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for UvIndexOverride {
    /// Drop 时删除整个临时目录；清理失败只记警告，不影响主流程。
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.dir) {
            warn!(
                "Unable to remove temporary uv index override {}: {err}",
                self.dir.display()
            );
        }
    }
}

/// 构造 `uv sync` 命令：按当前平台的 `.venv` 路径同步依赖。
fn uv_sync_command(bootstrap_uv: &Path, index: &str) -> Command {
    uv_sync_command_with_paths(
        bootstrap_uv,
        &venv_python(),
        &venv_python_install_dir(),
        index,
    )
}

/// 组装 `uv sync --no-dev --no-install-project` 命令行与环境变量。
///
/// 显式指定 Python 与安装目录，保证不依赖系统 Python；
/// `--default-index` 指定当前镜像源，并屏蔽外部 UV_INDEX 环境变量
/// 的干扰（见 [`ignore_uv_index_env`]）。
fn uv_sync_command_with_paths(
    bootstrap_uv: &Path,
    python: &Path,
    python_install_dir: &Path,
    index: &str,
) -> Command {
    let mut cmd = Command::new(bootstrap_uv);
    cmd.args(["sync", "--no-dev", "--no-install-project", "--python"])
        .arg(python)
        .args(["--default-index", index])
        .env("UV_NO_PROGRESS", "1")
        .env("UV_PYTHON_INSTALL_DIR", python_install_dir);
    uv_python_env_with_install_dir(&mut cmd, python_install_dir);
    ignore_uv_index_env(&mut cmd);
    cmd
}

/// 构造 `uv lock` 命令，额外挂载临时配置文件以覆盖项目索引设置。
fn uv_lock_command(bootstrap_uv: &Path, config_file: &Path, index: &str) -> Command {
    let mut cmd = uv_lock_command_with_paths(
        bootstrap_uv,
        &venv_python(),
        &venv_python_install_dir(),
        index,
    );
    cmd.arg("--config-file").arg(config_file);
    cmd
}

/// 组装 `uv lock` 命令行与环境变量（指定 Python、安装目录与镜像源）。
fn uv_lock_command_with_paths(
    bootstrap_uv: &Path,
    python: &Path,
    python_install_dir: &Path,
    index: &str,
) -> Command {
    let mut cmd = Command::new(bootstrap_uv);
    cmd.args(["lock", "--python"])
        .arg(python)
        .args(["--default-index", index])
        .env("UV_NO_PROGRESS", "1")
        .env("UV_PYTHON_INSTALL_DIR", python_install_dir);
    uv_python_env_with_install_dir(&mut cmd, python_install_dir);
    ignore_uv_index_env(&mut cmd);
    cmd
}

/// 迁移 `config/deploy.yaml` 到"自包含 .venv"的配置形态。
///
/// 逐行重写：可执行文件路径（Python/Adb/Git）强制指向 `.venv` 内的
/// 平台路径；移除已废弃的 `RequirementsFile`（依赖改由 uv.lock 管理）；
/// 强制 `InstallDependencies: true`；缺失的键按内置平台模板补齐。
/// 文件为空（首次启动）时直接写入内置 deploy.yaml 基线。
///
/// # Errors
/// 目录创建或文件写入失败时返回 Err。
fn migrate_dependency_config() -> Result<()> {
    let path = "./config/deploy.yaml";
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }

    let mut changed = false;
    let content = fs::read_to_string(path).unwrap_or_default();
    // 首次启动或配置为空时，以内置的平台模板作为迁移起点
    let content = if content.trim().is_empty() {
        changed = true;
        default_deploy_config().to_owned()
    } else {
        content
    };

    let mut found_python_executable = false;
    let mut found_adb_executable = false;
    let mut found_git_executable = false;
    let mut found_install_dependencies = false;
    let mut output = String::with_capacity(content.len());

    for line in content.lines() {
        let indent_len = line.len() - line.trim_start().len();
        let indent = &line[..indent_len];
        let trimmed = line.trim_start();
        if trimmed.starts_with("RequirementsFile:") {
            changed = true;
            continue;
        } else if trimmed.starts_with("PythonExecutable:") {
            found_python_executable = true;
            output.push_str(indent);
            output.push_str("PythonExecutable: ");
            output.push_str(platform_python_config_path());
            changed = true;
        } else if trimmed.starts_with("AdbExecutable:") {
            found_adb_executable = true;
            output.push_str(indent);
            output.push_str("AdbExecutable: ");
            output.push_str(platform_adb_config_path());
            changed = true;
        } else if trimmed.starts_with("GitExecutable:") {
            found_git_executable = true;
            output.push_str(indent);
            output.push_str("GitExecutable: ");
            output.push_str(platform_git_config_path());
            changed = true;
        } else if trimmed.starts_with("InstallDependencies:") {
            found_install_dependencies = true;
            if line.trim() != "InstallDependencies: true" {
                output.push_str(indent);
                output.push_str("InstallDependencies: true");
                changed = true;
            } else {
                output.push_str(line);
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }

    if !found_git_executable {
        output.push_str("\nGitExecutable: ");
        output.push_str(platform_git_config_path());
        output.push('\n');
        changed = true;
    }
    if !found_python_executable {
        output.push_str("PythonExecutable: ");
        output.push_str(platform_python_config_path());
        output.push('\n');
        changed = true;
    }
    if !found_adb_executable {
        output.push_str("AdbExecutable: ");
        output.push_str(platform_adb_config_path());
        output.push('\n');
        changed = true;
    }
    if !found_install_dependencies {
        output.push_str("InstallDependencies: true\n");
        changed = true;
    }

    if changed {
        fs::write(path, output)?;
        info!("Updated self-contained .venv settings in {path}");
    }

    Ok(())
}

/// 调用上游 `deploy.atomic.atomic_failure_cleanup` 清理上次失败的残留。
///
/// 与 git 更新同理复用上游 Python 逻辑，保证清理规则与官方安装器一致。
///
/// # Errors
/// 子命令执行失败时返回 Err（清理失败会阻断后续流程）。
fn atomic_failure_cleanup(path: &str, cancel_requested: &AtomicBool) -> Result<()> {
    let mut cmd = Command::new(venv_python());
    cmd.args([
        "-c",
        "import sys; from deploy.atomic import atomic_failure_cleanup; atomic_failure_cleanup(sys.argv[1])",
        path,
    ]);
    isolate_python_child_environment(&mut cmd);
    let _ = run_status_command(&mut cmd, cancel_requested)?;
    Ok(())
}

/// 构造运行时工具安装阶段（Python/工具链下载部署）的进度更新。
fn runtime_tools_update(
    title: impl Into<String>,
    detail: impl Into<String>,
    progress: impl Into<f64>,
) -> SplashUpdate {
    SplashUpdate::loading(title, detail, progress)
        .with_subtitle(t!("setup.rebuilding_env", tip = get_tip()).to_string())
}

/// 构造运行时工具阶段的等待进度：按耗时在 start/end 区间内线性推进，
/// 并在 8 秒后切换为显示已等待秒数的文案。
fn runtime_wait_update(
    title: &str,
    action: &str,
    elapsed: Duration,
    start_progress: f64,
    end_progress: f64,
) -> SplashUpdate {
    let elapsed_secs = elapsed.as_secs();
    // 120 秒对应整个区间，保留亚秒精度，避免一两个百分点的阶段
    // 因整数取整而长时间静止或在 100 秒时提前抵达终点。
    let percentage = elapsed.as_secs_f64() / 120.0 * 100.0;
    let progress = scale_progress(percentage, start_progress, end_progress);
    let detail = if elapsed_secs < 8 {
        t!("setup.action_wait", action = action).to_string()
    } else {
        t!(
            "setup.action_elapsed",
            action = action,
            secs = elapsed_secs.to_string()
        )
        .to_string()
    };
    runtime_tools_update(title, detail, progress)
}

/// 确保运行时工具链齐备：Python 环境 → uv 引导依赖 → 复制 uv/adb/git 进 `.venv`。
///
/// 是环境搭建的第一阶段：先保证 `.venv` 内有可用的 Python 与 requests，
/// 再把 uv、adb、git 一并部署进 `.venv`，后续所有流程都只依赖 `.venv`
/// 内的工具（用户机器无需预装任何东西）。
///
/// # Errors
/// 用户取消、Python 安装失败或工具部署失败时返回 Err。
fn ensure_runtime_tools(
    bootstrap_uv: &Path,
    cancel_requested: &AtomicBool,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<()> {
    status_updater(runtime_tools_update(
        t!("setup.preparing_env"),
        t!("setup.checking_python"),
        9,
    ));
    ensure_self_contained_python(bootstrap_uv, cancel_requested, &mut status_updater)?;
    ensure_deploy_python_dependencies(bootstrap_uv, cancel_requested, &mut status_updater)?;

    status_updater(runtime_tools_update(
        t!("setup.preparing_env"),
        t!("setup.copying_tools"),
        16,
    ));
    copy_file_if_exists(bootstrap_uv, &venv_uv())?;
    ensure_adb_in_venv()?;
    ensure_git_in_venv()?;
    Ok(())
}

/// 读取 deploy.yaml 中用户配置的 PyPI 镜像（Deploy.Python.PypiMirror）。
///
/// 空值与字面量 "null" 均视为未配置，返回 `None`。
fn deploy_pypi_mirror() -> Option<String> {
    get_deploy_config()
        .as_ref()
        .and_then(|c| c.get("Deploy"))
        .and_then(|d| d.get("Python"))
        .and_then(|p| p.get("PypiMirror"))
        .and_then(|v| v.as_str())
        .filter(|m| !m.is_empty() && *m != "null")
        .map(|m| m.to_owned())
}

/// 规范化 PyPI 索引 URL：去空白、忽略 "null"、统一补齐尾部斜杠，
/// 便于后续做等价比较；无效输入返回 `None`。
fn normalize_pypi_index(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return None;
    }
    if trimmed.ends_with('/') {
        Some(trimmed.to_owned())
    } else {
        Some(format!("{trimmed}/"))
    }
}

/// 判断两个索引 URL 是否等价（忽略尾部斜杠与大小写差异）。
fn pypi_indexes_match(left: &str, right: &str) -> bool {
    left.trim_end_matches('/')
        .eq_ignore_ascii_case(right.trim_end_matches('/'))
}

/// 向候选列表追加索引 URL，等价项去重（保持首次出现的顺序）。
fn push_unique_pypi_index(indexes: &mut Vec<String>, url: &str) {
    let Some(index) = normalize_pypi_index(url) else {
        return;
    };
    if !indexes
        .iter()
        .any(|existing| pypi_indexes_match(existing, &index))
    {
        indexes.push(index);
    }
}

/// 按偏好顺序返回候选 PyPI 索引。
///
/// deploy.yaml 中用户配置的镜像最优先——用户的选择决定 uv 首先对
/// 哪个源解析；内置国内镜像作为回退链，最后是官方 PyPI。
fn pypi_index_candidates() -> Vec<String> {
    let mut indexes = Vec::new();
    if let Some(index) = deploy_pypi_mirror() {
        push_unique_pypi_index(&mut indexes, &index);
    }
    for index in BUILTIN_PYPI_INDEXES {
        push_unique_pypi_index(&mut indexes, index);
    }
    push_unique_pypi_index(&mut indexes, DEFAULT_PYPI_INDEX);
    indexes
}

/// 构造"切换到下一个 PyPI 镜像重试"的启动画面进度。
fn pypi_index_fallback_update(next_index: &str) -> SplashUpdate {
    SplashUpdate::loading(
        t!("setup.retrying_deps"),
        format!("PyPI index: {next_index}"),
        64,
    )
    .with_subtitle(t!("setup.syncing_deps", tip = get_tip()))
    .with_uv_progress(2, t!("setup.uv_resolving", secs = "0"))
}

/// 对候选 PyPI 索引并发探测延迟，按"延迟最快优先"排序返回。
///
/// 每个候选源在独立线程中测延迟（HEAD 请求），全部探测完成后按
/// 延迟升序排序；用户在 deploy.yaml 配置的镜像无论测速结果如何
/// 都会被提到最前（尊重用户选择）。所有源都探测失败时退回配置顺序。
fn ranked_pypi_indexes() -> Vec<String> {
    let indexes = pypi_index_candidates();
    let client = pypi_probe_http_client();
    let handles = indexes
        .iter()
        .cloned()
        .enumerate()
        .map(|(order, index)| {
            let client = client.clone();
            thread::spawn(move || {
                let latency = client
                    .as_ref()
                    .and_then(|client| measure_pypi_index_latency(client, &index));
                (order, index, latency)
            })
        })
        .collect::<Vec<_>>();

    let mut probes = Vec::with_capacity(indexes.len());
    for handle in handles {
        if let Ok(probe) = handle.join() {
            probes.push(probe);
        }
    }

    for (_, index, latency) in &probes {
        if let Some(latency) = latency {
            info!("PyPI index probe {index}: {} ms", latency.as_millis());
        } else {
            warn!("PyPI index probe {index}: unavailable");
        }
    }

    let mut ranked_probe_orders = probes
        .iter()
        .filter_map(|(order, _, latency)| latency.map(|latency| (*order, latency)))
        .collect::<Vec<_>>();
    ranked_probe_orders.sort_by_key(|(order, latency)| (*latency, *order));

    let Some((fastest, _)) = ranked_probe_orders.first().copied() else {
        warn!("No PyPI index responded to probing; using configured order");
        return indexes;
    };

    let fastest_index = indexes[fastest].clone();
    let configured = deploy_pypi_mirror().and_then(|mirror| normalize_pypi_index(&mirror));
    let mut ranked: Vec<String> = Vec::with_capacity(indexes.len());
    for (order, _) in ranked_probe_orders {
        let index = &indexes[order];
        if !ranked
            .iter()
            .any(|existing| pypi_indexes_match(existing, index))
        {
            ranked.push(index.clone());
        }
    }
    for index in indexes {
        if !ranked
            .iter()
            .any(|existing| pypi_indexes_match(existing, &index))
        {
            ranked.push(index);
        }
    }
    if let Some(configured) = configured {
        if let Some(position) = ranked
            .iter()
            .position(|existing| pypi_indexes_match(existing, &configured))
        {
            if position > 0 {
                let configured = ranked.remove(position);
                info!("Using the PyPI index configured in deploy.yaml: {configured}");
                ranked.insert(0, configured);
            }
        }
    }
    info!("Fastest PyPI index selected first: {fastest_index}");
    ranked
}

/// 构造索引探测专用的 HTTP 客户端：短超时（3 秒连接 / 5 秒总超时）、
/// 禁用系统代理（探测的是直连可达性）、带 PyPI simple API 的 Accept 头。
/// 构建失败时返回 `None`（调用方回退为不排序）。
fn pypi_probe_http_client() -> Option<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("AzurPilot Launcher PyPI probe"),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/vnd.pypi.simple.v1+html,*/*;q=0.8"),
    );

    Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .redirect(Policy::limited(5))
        .no_proxy()
        .default_headers(headers)
        .build()
        .ok()
}

/// 测量索引的响应延迟（HEAD 请求往返时间）；不可达或非 2xx 返回 `None`。
fn measure_pypi_index_latency(client: &Client, index: &str) -> Option<Duration> {
    let start = Instant::now();
    let response = client.head(index).send().ok()?;
    response.status().is_success().then(|| start.elapsed())
}

/// 移除子进程环境中所有可能干扰镜像源选择的变量。
///
/// 用户机器上的 `UV_INDEX` / `PIP_INDEX_URL` 等会覆盖命令行
/// `--default-index`，导致镜像回退链失效，故显式移除。
fn ignore_uv_index_env(cmd: &mut Command) {
    for key in [
        "UV_INDEX",
        "UV_DEFAULT_INDEX",
        "UV_INDEX_URL",
        "UV_EXTRA_INDEX_URL",
        "PIP_INDEX_URL",
        "PIP_EXTRA_INDEX_URL",
    ] {
        cmd.env_remove(key);
    }
}

/// 让子进程绕过系统代理直连（下载/更新全部走镜像，避免代理故障干扰）。
///
/// 仅移除代理变量还不够——uv 在变量缺失时会回退到平台级代理设置，
/// 所以还要把 `NO_PROXY` 设为 `*` 明确声明"全部直连"。
fn bypass_proxy_for_child(cmd: &mut Command) {
    for key in [
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "PIP_PROXY",
        "pip_proxy",
    ] {
        cmd.env_remove(key);
    }
    // uv falls back to the platform proxy when these variables are absent.
    cmd.env("NO_PROXY", "*").env("no_proxy", "*");
}

/// 隔离子进程的 Python 相关环境变量。
///
/// 用户机器上残留的 `PYTHONHOME` / `PYTHONPATH` / `VIRTUAL_ENV`
/// 会让 `.venv` 内的解释器加载错误的库路径，是"本机能跑、
/// 启动器里跑不起来"的常见根因，因此所有 Python 子进程统一清洗。
pub(crate) fn isolate_python_child_environment(cmd: &mut Command) {
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

/// 确保 `.venv` 内已安装 `requests`（上游更新脚本的最小依赖）。
///
/// 先用 `import requests` 探测：已可用直接返回；缺失时按镜像源
/// 优先级逐源尝试 `uv pip install requests`，等待期间刷新进度。
///
/// # Errors
/// 用户取消或所有镜像源安装均失败时返回 Err。
fn ensure_deploy_python_dependencies(
    bootstrap_uv: &Path,
    cancel_requested: &AtomicBool,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<()> {
    status_updater(runtime_tools_update(
        t!("setup.preparing_env"),
        t!("setup.checking_requests"),
        14,
    ));
    let mut import_check = Command::new(venv_python());
    import_check.args(["-c", "import requests"]);
    isolate_python_child_environment(&mut import_check);
    let status = run_status_command(&mut import_check, cancel_requested)?;
    if status.success() {
        return Ok(());
    }

    let indexes = ranked_pypi_indexes();
    for (attempt, index) in indexes.iter().enumerate() {
        status_updater(runtime_tools_update(
            t!("setup.preparing_env"),
            t!("setup.installing_requests"),
            15,
        ));
        info!("Installing requests with PyPI index: {index}");
        let mut cmd = Command::new(bootstrap_uv);
        cmd.args(["pip", "install", "--python"])
            .arg(venv_python())
            .arg("requests")
            .args(["--default-index", index])
            .arg("--no-config")
            .env("UV_NO_PROGRESS", "1")
            .env("UV_PYTHON_INSTALL_DIR", venv_python_install_dir());
        ignore_uv_index_env(&mut cmd);
        isolate_python_child_environment(&mut cmd);
        bypass_proxy_for_child(&mut cmd);

        let started_at = Instant::now();
        let status = run_status_command_with_tick(&mut cmd, cancel_requested, || {
            status_updater(runtime_wait_update(
                &t!("setup.preparing_env"),
                &t!("setup.installing_requests"),
                started_at.elapsed(),
                15.0,
                16.0,
            ));
        })?;
        if status.success() {
            return Ok(());
        }

        warn!("Failed to install requests with PyPI index: {index}");
        if attempt + 1 < indexes.len() {
            thread::sleep(RETRY_DELAY);
        }
    }

    bail!(t!("errors.requests_install_failed"));
}

/// 为 uv 子命令设置默认的 Python 安装目录环境（使用 `.venv` 内路径）。
fn uv_python_env(cmd: &mut Command) {
    uv_python_env_with_install_dir(cmd, &venv_python_install_dir());
}

/// 统一配置 uv 子命令的环境变量：关闭交互进度、清除 UV_PYTHON 干扰、
/// 指定 Python 安装目录与下载镜像、隔离 Python 环境、绕过代理。
///
/// 镜像仅在用户未通过 `UV_PYTHON_INSTALL_MIRROR` 显式指定时才注入
/// 默认值（第一个为国内 npmmirror 加速源）。
fn uv_python_env_with_install_dir(cmd: &mut Command, python_install_dir: &Path) {
    cmd.env("UV_NO_PROGRESS", "1")
        .env_remove("UV_PYTHON")
        .env("UV_PYTHON_INSTALL_DIR", python_install_dir);
    isolate_python_child_environment(cmd);
    bypass_proxy_for_child(cmd);
    if std::env::var_os("UV_PYTHON_INSTALL_MIRROR").is_none() {
        cmd.env(
            "UV_PYTHON_INSTALL_MIRROR",
            DEFAULT_UV_PYTHON_INSTALL_MIRRORS[0],
        );
    }
}

/// 取 Python standalone 下载镜像列表：用户显式指定的镜像优先（唯一），否则用内置列表。
fn uv_python_install_mirrors() -> Vec<String> {
    if let Some(mirror) = std::env::var_os("UV_PYTHON_INSTALL_MIRROR") {
        return vec![mirror.to_string_lossy().into_owned()];
    }

    DEFAULT_UV_PYTHON_INSTALL_MIRRORS
        .iter()
        .map(|mirror| (*mirror).to_owned())
        .collect()
}

/// 确保 `.venv` 内有指定版本的自包含 Python（3.14.6）。
///
/// 缺失环境执行首次安装；现有 Python 损坏或不兼容时提示用户手动修复。
/// 不因启动失败或取消而自动删除已有环境。
///
/// # Errors
/// 用户取消、环境删除失败或 uv 安装 Python 失败时返回 Err。
fn ensure_self_contained_python(
    bootstrap_uv: &Path,
    cancel_requested: &AtomicBool,
    mut status_updater: impl FnMut(SplashUpdate),
) -> Result<()> {
    status_updater(runtime_tools_update(
        t!("setup.preparing_env"),
        t!("setup.checking_python_version", version = PYTHON_VERSION),
        10,
    ));
    let existing_venv_python_version = venv_python_version();
    let venv = venv_dir();
    if venv.exists()
        && (existing_venv_python_version.is_none()
            || existing_venv_python_version.is_some_and(venv_python_version_requires_rebuild)
            || managed_python_executable().is_none())
    {
        bail!(t!("launcher.env_repair_required"));
    }

    if existing_venv_python_version.is_some_and(is_reusable_venv_python_version)
        && managed_python_executable().is_some()
    {
        return Ok(());
    }

    if managed_python_executable().is_none() {
        fs::create_dir_all(venv_python_install_dir()).with_context(|| {
            t!(
                "errors.python_dir_failed",
                error = venv_python_install_dir().display().to_string()
            )
        })?;
        let mirrors = uv_python_install_mirrors();
        let mut downloaded = false;
        for (index, mirror) in mirrors.iter().enumerate() {
            let mirror_label = if index == 0 {
                t!("setup.primary_mirror").to_string()
            } else {
                t!("setup.fallback_mirror").to_string()
            };
            status_updater(runtime_tools_update(
                t!("setup.download_python_title"),
                t!(
                    "setup.downloading_python",
                    version = PYTHON_VERSION,
                    mirror = mirror_label,
                    current = (index + 1).to_string(),
                    total = mirrors.len().to_string()
                ),
                11,
            ));
            let mut cmd = Command::new(bootstrap_uv);
            cmd.args(["python", "install", "--install-dir"])
                .arg(venv_python_install_dir())
                .args([
                    "--no-bin",
                    "--managed-python",
                    "--mirror",
                    mirror,
                    PYTHON_VERSION,
                ]);
            uv_python_env(&mut cmd);
            let started_at = Instant::now();
            let status = run_status_command_with_tick(&mut cmd, cancel_requested, || {
                status_updater(runtime_wait_update(
                    &t!("setup.download_python_title"),
                    &t!("setup.downloading_python_action", version = PYTHON_VERSION),
                    started_at.elapsed(),
                    11.0,
                    13.0,
                ));
            })?;
            if status.success() {
                downloaded = true;
                break;
            }
            warn!(
                "{}",
                t!(
                    "errors.download_python_failed_mirror",
                    version = PYTHON_VERSION,
                    mirror = mirror
                )
            );
        }
        if !downloaded {
            bail!(t!(
                "errors.python_download_failed",
                version = PYTHON_VERSION
            ));
        }
    }

    let managed_python = managed_python_executable()
        .ok_or_else(|| anyhow!(t!("errors.python_not_found", version = PYTHON_VERSION)))?;
    status_updater(runtime_tools_update(
        t!("setup.creating_venv_title"),
        t!("setup.creating_venv"),
        13,
    ));
    reset_virtualenv_layout()?;
    let mut cmd = Command::new(bootstrap_uv);
    cmd.args(["venv", "--allow-existing", "--relocatable", "--python"])
        .arg(managed_python)
        .arg(venv_dir());
    uv_python_env(&mut cmd);
    let status = run_status_command(&mut cmd, cancel_requested)?;
    if !status.success() {
        bail!(t!("errors.venv_create_failed"));
    }
    Ok(())
}

/// 清空虚拟环境的标准布局目录（Scripts/Lib 等），为 `uv venv --allow-existing` 铺路。
///
/// uv 的 venv 命令不会覆盖已有布局文件，重建环境前必须先清掉旧布局；
/// 只删标准条目而非整个 `.venv`，尽量保留无关数据。
///
/// # Errors
/// 任一标准目录删除失败时返回 Err。
fn reset_virtualenv_layout() -> Result<()> {
    let venv = venv_dir();
    let entries = if cfg!(windows) {
        vec!["Scripts", "Lib", "Include", "pyvenv.cfg"]
    } else {
        vec!["bin", "lib", "include", "pyvenv.cfg"]
    };

    for entry in entries {
        let path = venv.join(entry);
        if path.exists() {
            remove_runtime_entry_with_retry(&path).with_context(|| {
                t!(
                    "errors.reset_venv_failed",
                    error = path.display().to_string()
                )
            })?;
        }
    }

    Ok(())
}

/// 在安装目录中查找 uv managed Python 的解释器路径。
///
/// uv 的安装目录名形如 `cpython-3.14.6-<平台标签>`，按前缀匹配
/// 后尝试各平台约定的解释器位置；未安装时返回 `None`。
fn managed_python_executable() -> Option<PathBuf> {
    let install_dir = venv_python_install_dir();
    let entries = fs::read_dir(install_dir).ok()?;
    let prefix = format!("cpython-{PYTHON_VERSION}-");
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(&prefix) {
            continue;
        }
        let candidates = if cfg!(windows) {
            vec![path.join("python.exe")]
        } else {
            vec![
                path.join("bin").join("python3.14"),
                path.join("bin").join("python"),
            ]
        };
        for candidate in candidates {
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 读取 `.venv` 内 Python 的实际版本号（major.minor.patch）。
///
/// 解释器不存在、执行失败或输出无法解析时返回 `None`
/// （均视为"环境不可用"，由调用方走重建路径）。
fn venv_python_version() -> Option<(u16, u16, u16)> {
    let python = venv_python();
    if !python.exists() {
        return None;
    }
    let mut cmd = Command::new(python);
    cmd.args([
        "-c",
        "import sys; print('.'.join(map(str, sys.version_info[:3])))",
    ]);
    isolate_python_child_environment(&mut cmd);
    let output = cmd.create_no_window().output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_python_version(std::str::from_utf8(&output.stdout).ok()?)
}

/// 解析 `sys.version_info` 输出的 `major.minor.patch` 三段版本号；
/// 多于三段（含附加后缀）视为异常返回 `None`。
fn parse_python_version(version: &str) -> Option<(u16, u16, u16)> {
    let mut components = version.trim().split('.');
    let major = components.next()?.parse().ok()?;
    let minor = components.next()?.parse().ok()?;
    let patch = components.next()?.parse().ok()?;
    if components.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// 判断该 Python 版本是否低于最低可复用版本（需要删除重建整个 `.venv`）。
fn venv_python_version_requires_rebuild(version: (u16, u16, u16)) -> bool {
    version < MIN_REUSABLE_VENV_PYTHON_VERSION
}

/// 判断该版本是否可直接复用：必须是 Python 3.14 系且不低于最低可复用版本
/// （大版本不一致说明环境来自旧发行版，不能沿用）。
fn is_reusable_venv_python_version(version: (u16, u16, u16)) -> bool {
    version.0 == 3 && version.1 == 14 && !venv_python_version_requires_rebuild(version)
}

/// 复制单个文件（源不存在时静默成功；内容相同则跳过写入）。
///
/// 跳过相同文件的检查可避免重复启动时无谓的写入；Unix 上复制后
/// 补 0755 权限，保证工具可执行。
///
/// # Errors
/// 目录创建或复制失败时返回 Err（含 i18n 错误信息）。
fn copy_file_if_exists(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    if to.exists() && files_match(from, to).unwrap_or(false) {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).with_context(|| {
            t!(
                "errors.create_dir_failed",
                path = parent.display().to_string()
            )
        })?;
    }
    fs::copy(from, to).with_context(|| {
        t!(
            "errors.copy_file_failed",
            src = from.display().to_string(),
            dest = to.display().to_string()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(to)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(to, permissions)?;
    }
    Ok(())
}

/// 比较两个文件内容是否完全一致（先比大小再逐字节比较，尽早短路）。
///
/// # Errors
/// 任一文件元数据或内容读取失败时返回 Err。
fn files_match(left: &Path, right: &Path) -> Result<bool> {
    let left_metadata = fs::metadata(left).with_context(|| {
        t!(
            "errors.read_info_failed",
            error = left.display().to_string()
        )
    })?;
    let right_metadata = fs::metadata(right).with_context(|| {
        t!(
            "errors.read_info_failed",
            error = right.display().to_string()
        )
    })?;
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }

    let left_bytes = fs::read(left).with_context(|| {
        t!(
            "errors.read_file_failed",
            error = left.display().to_string()
        )
    })?;
    let right_bytes = fs::read(right).with_context(|| {
        t!(
            "errors.read_file_failed",
            error = right.display().to_string()
        )
    })?;
    Ok(left_bytes == right_bytes)
}

/// 确保 adb 已部署进 `.venv`（已存在则跳过）。
///
/// Windows 发行包中 adb 与其依赖 DLL 分开放置，需一并复制；
/// Unix 为单文件。
///
/// # Errors
/// 工具复制失败时返回 Err。
fn ensure_adb_in_venv() -> Result<()> {
    if venv_adb().exists() {
        return Ok(());
    }
    if cfg!(windows) {
        copy_first_packaged_tool(&["adb.exe"], &venv_bin_dir())?;
        copy_matching_packaged_tools("Adb", "dll", &venv_bin_dir())?;
    } else {
        copy_first_packaged_tool(&["adb"], &venv_bin_dir())?;
    }
    Ok(())
}

/// 确保 git 已部署进 `.venv`（已存在则跳过）。
///
/// Windows 为 MinGit 完整目录布局（`Scripts/git/`）；Unix 为单文件
/// `bin/git` 加 `libexec/git-core`（HTTPS 远程支持）与模板目录，
/// 复制完统一刷新 git 环境变量。
///
/// # Errors
/// 工具复制失败时返回 Err。
fn ensure_git_in_venv() -> Result<()> {
    if cfg!(windows) {
        if !venv_git().exists() {
            let src = PathBuf::from("bootstrap").join("git");
            let dst = venv_bin_dir().join("git");
            if src.exists() {
                copy_dir_all(&src, &dst)?;
            }
        }
    } else {
        if !venv_git().exists() {
            copy_first_packaged_tool(&["git"], &venv_bin_dir())?;
        }

        let git_core_src = PathBuf::from("bootstrap").join("git-core");
        let git_core_dst = venv_git_exec_path();
        let git_remote_https = git_core_dst.join("git-remote-https");
        if (!git_core_dst.exists() || !git_remote_https.exists()) && git_core_src.exists() {
            copy_dir_all(&git_core_src, &git_core_dst)?;
        }

        let templates_src = PathBuf::from("bootstrap").join("git-templates");
        let templates_dst = venv_git_template_dir();
        if !templates_dst.exists() && templates_src.exists() {
            copy_dir_all(&templates_src, &templates_dst)?;
        }

        refresh_git_environment();
    }
    Ok(())
}

/// 从发行包的 `bootstrap/` 目录复制第一个存在的同名工具到目标目录。
///
/// 同一工具可能以多个候选名出现（如 `adb` / `adb.exe`），
/// 取先命中者复制即止。
///
/// # Errors
/// 复制失败时返回 Err。
fn copy_first_packaged_tool(names: &[&str], target_dir: &Path) -> Result<()> {
    for name in names {
        let source = PathBuf::from("bootstrap").join(name);
        if source.exists() {
            copy_file_if_exists(&source, &target_dir.join(name))?;
            return Ok(());
        }
    }
    Ok(())
}

/// 复制 `bootstrap/` 目录下所有"前缀+扩展名"匹配的文件（如 adb 的 DLL 依赖）。
///
/// 目录不存在时静默成功——发行包可能不包含该平台的附加文件。
///
/// # Errors
/// 复制失败时返回 Err。
fn copy_matching_packaged_tools(prefix: &str, extension: &str, target_dir: &Path) -> Result<()> {
    let dir = PathBuf::from("bootstrap");
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && name.ends_with(extension) {
            copy_file_if_exists(&entry.path(), &target_dir.join(name.as_ref()))?;
        }
    }
    Ok(())
}

/// 递归复制整个目录树（子目录递归，文件走 [`copy_file_if_exists`] 的去重逻辑）。
///
/// # Errors
/// 目录创建或任一文件复制失败时返回 Err。
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            copy_file_if_exists(&entry.path(), &target)?;
        }
    }
    Ok(())
}

/// 把子进程的一行输出转换为启动画面进度更新（无法识别时返回 `None`）。
///
/// 按 [`ScriptPhase`] 分派给 git 或依赖各自的输出解析器。
fn splash_update_for_output(
    line: &str,
    phase: ScriptPhase,
    git_progress: &mut GitProgressState,
    uv_progress: &mut UvProgressState,
) -> Option<SplashUpdate> {
    let sanitized = line.trim();
    if sanitized.is_empty() {
        return None;
    }

    match phase {
        ScriptPhase::Git => splash_update_for_git_output(sanitized, git_progress),
        ScriptPhase::Dependencies => splash_update_for_dependency_output(sanitized, uv_progress),
    }
}

/// 解析 git 更新输出行并映射为进度。
///
/// 上游脚本以 `===== 标题 =====` 分隔各阶段（SHOW DEPLOY CONFIG、
/// GIT INIT 等），按标题映射到预设进度点；其余行尝试解析
/// `git fetch --progress` 的百分比。
fn splash_update_for_git_output(line: &str, state: &mut GitProgressState) -> Option<SplashUpdate> {
    let subtitle = t!("setup.syncing", tip = get_tip()).to_string();

    if line.contains("=====") {
        let detail = line.replace('=', " ");
        let progress = git_section_progress(detail.trim()).unwrap_or(24.0);
        return Some(git_update_splash(
            line,
            detail.trim(),
            progress,
            state,
            subtitle,
        ));
    }

    if let Some(progress) = git_line_progress(line) {
        return Some(git_update_splash(line, line, progress, state, subtitle));
    }

    None
}

/// 组装 git 阶段的进度更新，进度经状态机钳制为单调递增。
fn git_update_splash(
    raw_line: &str,
    detail: &str,
    progress: f64,
    state: &mut GitProgressState,
    subtitle: String,
) -> SplashUpdate {
    state.progress = state.progress.max(progress);
    let display_detail = if detail.trim().is_empty() {
        raw_line
    } else {
        detail
    };
    SplashUpdate::loading(t!("setup.retrying_update"), display_detail, state.progress)
        .with_subtitle(subtitle)
}

/// 按上游脚本的阶段标题映射预设进度点（18% 起，随阶段推进）。
///
/// 标题与 `deploy/installer.py` 的输出保持一致，上游改动时需同步。
fn git_section_progress(section: &str) -> Option<f64> {
    if section.contains("SHOW DEPLOY CONFIG") {
        Some(18.0)
    } else if section.contains("UPDATE AZURPILOT") {
        Some(20.0)
    } else if section.contains("GIT INIT") {
        Some(22.0)
    } else if section.contains("SET GIT PROXY") {
        Some(23.0)
    } else if section.contains("SET GIT REPOSITORY") {
        Some(24.0)
    } else if section.contains("FETCH REPOSITORY BRANCH") {
        Some(25.0)
    } else if section.contains("PULL REPOSITORY BRANCH") {
        Some(58.0)
    } else if section.contains("SHOW VERSION") {
        Some(63.0)
    } else {
        None
    }
}

/// 解析 `git fetch` 进度行的百分比，并按子阶段映射到总进度区间。
///
/// Counting/Compressing（25-29）→ Receiving（29-52）→ Resolving
/// deltas（52-58）→ Updating files（58-62），与阶段标题的进度点衔接。
fn git_line_progress(line: &str) -> Option<f64> {
    let percentage = find_percentage(line)?;
    if line.contains("Counting objects:") || line.contains("Compressing objects:") {
        Some(scale_progress(percentage, 25, 29))
    } else if line.contains("Receiving objects:") {
        Some(scale_progress(percentage, 29, 52))
    } else if line.contains("Resolving deltas:") {
        Some(scale_progress(percentage, 52, 58))
    } else if line.contains("Updating files:") {
        Some(scale_progress(percentage, 58, 62))
    } else {
        None
    }
}

/// 解析 uv sync 的状态行并映射为依赖同步进度。
///
/// 仅识别 uv 的已知状态前缀（Resolved/Downloading/...），其余输出
/// 忽略；识别到的行先喂给状态机再取总进度。
fn splash_update_for_dependency_output(
    line: &str,
    uv_progress: &mut UvProgressState,
) -> Option<SplashUpdate> {
    let is_status_line = line.starts_with("Resolved ")
        || line.starts_with("Downloading ")
        || line.starts_with("Downloaded ")
        || line.starts_with("Prepared ")
        || line.starts_with("Installed ")
        || line.starts_with("Audited ")
        || line.starts_with("+ ");
    if !is_status_line {
        return None;
    }

    uv_progress.observe(line);
    let progress = dependency_global_progress(uv_progress.progress());
    Some(dependency_splash_update(line, progress, uv_progress))
}

/// 组装依赖同步阶段的进度更新（总进度 + uv 子进度两段显示）。
fn dependency_splash_update(
    detail: impl Into<String>,
    progress: f64,
    uv_progress: &mut UvProgressState,
) -> SplashUpdate {
    SplashUpdate::loading(t!("setup.installing_deps"), detail, progress)
        .with_subtitle(t!("setup.syncing_deps", tip = get_tip()))
        .with_uv_progress(uv_progress.progress(), uv_progress.detail())
}

/// 把 uv 子进度（0-100）线性映射到总进度条的 64-90 区间。
fn dependency_global_progress(uv_progress: f64) -> f64 {
    scale_progress(uv_progress, 64, 90)
}

/// 判断一行 stderr 是否为 uv 的正常进度/提示输出。
///
/// uv 把进度写在 stderr 上，这类行不应按错误处理（不记入
/// "最后错误"、日志级别降为 info）。
fn is_uv_progress_line(line: &str) -> bool {
    line.starts_with("Resolved ")
        || line.starts_with("Downloading ")
        || line.starts_with("Downloaded ")
        || line.starts_with("Prepared ")
        || line.starts_with("Installed ")
        || line.starts_with("Audited ")
        || line.starts_with("warning: ")
        || line.starts_with("hint: ")
        || line.starts_with("note: ")
}

impl UvProgressState {
    /// 消费一行 uv 状态输出，更新解析/下载/安装各阶段标记与下载统计。
    fn observe(&mut self, line: &str) {
        if line.starts_with("Resolved ") {
            self.resolved = true;
            return;
        }

        if line.starts_with("Downloading ") {
            self.download_started_at.get_or_insert_with(Instant::now);
            if let (Some(package), Some(size)) = (
                extract_uv_package_name(line),
                extract_uv_download_size_bytes(line),
            ) {
                self.package_sizes.entry(package).or_insert(size);
            }
            return;
        }

        if line.starts_with("Downloaded ") {
            if let Some(package) = extract_uv_downloaded_package_name(line) {
                if self.downloaded_packages.insert(package.clone()) {
                    self.downloaded_bytes = self
                        .downloaded_bytes
                        .saturating_add(self.package_sizes.get(&package).copied().unwrap_or(0));
                }
            }
            return;
        }

        if line.starts_with("Prepared ") {
            self.prepared = true;
        } else if line.starts_with("Installed ") || line.starts_with("Audited ") {
            self.installed = true;
        }
    }

    /// 估算 uv 阶段的整体进度（0-100，单调递增）。
    ///
    /// 各阶段锚点：启动后按时间缓慢爬升（2-20）、解析完成 22、
    /// 下载期取"时间估算"与"包数估算"的较大者（28-88）、准备完成
    /// 92、安装完成 98。进度只会前进，避免输出乱序导致回退。
    fn progress(&mut self) -> f64 {
        self.progress_at(Instant::now())
    }

    fn progress_at(&mut self, now: Instant) -> f64 {
        let progress = if self.installed {
            98.0
        } else if self.prepared {
            92.0
        } else if let Some(download_started_at) = self.download_started_at {
            // 双估算取大者：时间估算保证进度不因包信息缺失而停滞，
            // 包数估算让大包下载时进度更贴近真实
            let elapsed_progress = 28.0
                + now
                    .saturating_duration_since(download_started_at)
                    .as_secs_f64()
                    / 4.0;
            let package_progress = 28.0 + self.downloaded_packages.len().min(30) as f64 * 2.0;
            elapsed_progress.max(package_progress).min(88.0)
        } else if self.resolved {
            22.0
        } else {
            (2.0 + now.saturating_duration_since(self.started_at).as_secs_f64() / 10.0).min(20.0)
        };

        self.last_progress = self.last_progress.max(progress);
        self.last_progress
    }

    /// 生成 uv 子进度的描述文案：安装中 / 解析中（含耗时）/
    /// 下载中（含已下载量与实时速度估算）。
    fn detail(&self) -> String {
        if self.installed || self.prepared {
            return t!("setup.uv_installing").to_string();
        }

        let Some(download_started_at) = self.download_started_at else {
            return t!(
                "setup.uv_resolving",
                secs = self.started_at.elapsed().as_secs().to_string()
            )
            .to_string();
        };
        if self.downloaded_bytes == 0 {
            return t!("setup.uv_waiting_speed").to_string();
        }

        let elapsed = download_started_at.elapsed().as_secs_f64().max(0.1);
        let speed = self.downloaded_bytes as f64 / elapsed;
        t!(
            "setup.uv_downloading_detail",
            downloaded = format_transfer_size(self.downloaded_bytes),
            speed = format_transfer_speed(speed)
        )
        .to_string()
    }
}

/// 从 `Downloading <包名> (体积)` 行提取包名。
///
/// 兼容 `numpy==2.4.3`、`numpy @ https://...` 与裸包名三种格式；
/// 包名统一小写以便与 Downloaded 行配对。
fn extract_uv_package_name(line: &str) -> Option<String> {
    // "Downloading numpy==2.4.3 (8.2 MiB)" or "Downloading numpy (8.2 MiB)" or "Downloading numpy @ https://..."
    let rest = line.strip_prefix("Downloading ")?;
    let name = rest
        .split_once("==")
        .map(|(n, _)| n)
        .or_else(|| rest.split_once(" @ ").map(|(n, _)| n))
        .or_else(|| rest.split_once(" (").map(|(n, _)| n))
        .unwrap_or(rest);
    normalize_uv_package_name(name)
}

/// 从 `Downloaded <包名>` 行提取包名（取首个空白分隔段并去掉版本号）。
fn extract_uv_downloaded_package_name(line: &str) -> Option<String> {
    let name = line
        .strip_prefix("Downloaded ")?
        .split_whitespace()
        .next()?;
    let name = name.split_once("==").map(|(name, _)| name).unwrap_or(name);
    normalize_uv_package_name(name)
}

/// 包名归一化：去空白并转小写；空名返回 `None`。
fn normalize_uv_package_name(name: &str) -> Option<String> {
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// 从 `Downloading ... (8.2 MiB)` 行提取括号内的人类可读体积并转为字节数。
fn extract_uv_download_size_bytes(line: &str) -> Option<u64> {
    let (_, size) = line.rsplit_once('(')?;
    let size = size.strip_suffix(')')?;
    parse_transfer_size_bytes(size)
}

/// 解析人类可读体积（`1.5MiB`、`42 KiB` 等）为字节数。
///
/// 支持二进制单位（KiB/MiB/GiB）与十进制写法（KB/MB/GB）；
/// 非法输入、非有限数或超出 u64 范围时返回 `None`。
fn parse_transfer_size_bytes(size: &str) -> Option<u64> {
    let size = size.trim();
    let unit_start =
        size.find(|character: char| !character.is_ascii_digit() && character != '.')?;
    let (number, unit) = size.split_at(unit_start);
    let number: f64 = number.parse().ok()?;
    let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
        "b" => 1.0,
        "kb" | "kib" => 1024.0,
        "mb" | "mib" => 1024.0 * 1024.0,
        "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let bytes = number * multiplier;
    (bytes.is_finite() && bytes >= 0.0 && bytes <= u64::MAX as f64).then_some(bytes.round() as u64)
}

/// 把字节数格式化为人类可读体积（自动选择 B/KiB/MiB/GiB 单位，一位小数）。
fn format_transfer_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

/// 把字节每秒的速率格式化为人类可读体积速率（负数按 0 处理）。
fn format_transfer_speed(bytes_per_second: f64) -> String {
    format_transfer_size(bytes_per_second.max(0.0).round() as u64)
}

/// 把 0-100 的百分比线性缩放到 `[start, end]` 区间（进度条分段衔接用）。
fn scale_progress(percentage: impl Into<f64>, start: impl Into<f64>, end: impl Into<f64>) -> f64 {
    let percentage = percentage.into().clamp(0.0, 100.0);
    let start = start.into();
    let end = end.into();
    start + percentage * (end - start) / 100.0
}

/// 从一行文本中提取首个 `N%` 形式的百分比数字；保留小数，无百分比返回 `None`。
fn find_percentage(s: &str) -> Option<f64> {
    s.split('%')
        .next()
        .and_then(|before| {
            before
                .rsplit(|c: char| !c.is_ascii_digit() && c != '.')
                .next()
        })
        .and_then(|num| {
            num.parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(|value| value.clamp(0.0, 100.0))
        })
}

/// 单元测试：覆盖删除恢复、进度解析、uv 命令环境约束与体积格式化。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_repair_removes_only_venv() {
        let temp_dir = tempfile::tempdir().expect("create temporary repository");
        let venv = temp_dir.path().join(".venv");
        let keep = temp_dir.path().join("keep.txt");
        fs::create_dir_all(venv.join("Lib")).expect("create temporary venv");
        fs::write(venv.join("Lib").join("package.txt"), "dependency")
            .expect("write temporary dependency");
        fs::write(&keep, "keep").expect("write sibling file");
        let cancel_requested = AtomicBool::new(false);

        remove_venv_path_for_repair(&venv, &cancel_requested).expect("remove temporary venv");

        assert!(!venv.exists());
        assert_eq!(fs::read_to_string(keep).expect("read sibling file"), "keep");
    }

    #[test]
    fn cancelled_repair_preserves_venv() {
        let temp_dir = tempfile::tempdir().expect("create temporary repository");
        let venv = temp_dir.path().join(".venv");
        fs::create_dir_all(&venv).expect("create temporary venv");
        let cancel_requested = AtomicBool::new(true);

        assert!(remove_venv_path_for_repair(&venv, &cancel_requested).is_err());
        assert!(venv.exists());
    }

    #[test]
    fn test_find_percentage() {
        assert_eq!(Some(8.0), find_percentage("8%"));
        assert_eq!(Some(25.0), find_percentage("loading 25%..."));
        assert_eq!(Some(25.125), find_percentage("loading 25.125%..."));
        assert_eq!(Some(100.0), find_percentage("100%..."));
        assert_eq!(Some(100.0), find_percentage("loading 150%..."));
        assert_eq!(None, find_percentage("%1"));
    }

    #[test]
    fn test_git_line_progress_ranges() {
        assert_eq!(
            Some(25.04),
            git_line_progress("remote: Counting objects:   1% (1/66)")
        );
        assert_eq!(
            Some(40.5),
            git_line_progress("Receiving objects:  50% (43546/87092), 179.10 MiB | 5.03 MiB/s")
        );
        assert_eq!(
            Some(58.0),
            git_line_progress("Resolving deltas: 100% (66157/66157), done.")
        );
        assert_eq!(
            Some(62.0),
            git_line_progress("Updating files: 100% (9881/9881), done.")
        );
    }

    #[test]
    fn test_git_section_progress_ranges() {
        assert_eq!(Some(25.0), git_section_progress("FETCH REPOSITORY BRANCH"));
        assert_eq!(Some(58.0), git_section_progress("PULL REPOSITORY BRANCH"));
        assert_eq!(Some(63.0), git_section_progress("SHOW VERSION"));
    }

    #[test]
    fn test_progress_scaling_preserves_fraction_and_stage_bounds() {
        assert_eq!(scale_progress(50.0, 64, 90), 77.0);
        assert_eq!(scale_progress(2.5, 64, 90), 64.65);
        assert_eq!(scale_progress(150.0, 64, 90), 90.0);
        assert_eq!(scale_progress(-5.0, 64, 90), 64.0);
    }

    #[test]
    fn test_runtime_wait_progress_uses_elapsed_time_over_full_interval() {
        let update = |elapsed| runtime_wait_update("title", "action", elapsed, 11.0, 13.0);
        assert_eq!(update(Duration::ZERO).progress, 11.0);
        assert_eq!(update(Duration::from_secs(60)).progress, 12.0);
        assert!(update(Duration::from_millis(100)).progress > 11.0);
        assert!(update(Duration::from_secs(100)).progress < 13.0);
        assert_eq!(update(Duration::from_secs(120)).progress, 13.0);
        assert_eq!(update(Duration::from_secs(300)).progress, 13.0);
    }

    #[test]
    fn test_uv_sync_uses_managed_python() {
        let python = Path::new(".venv/Scripts/python.exe");
        let command = uv_sync_command_with_paths(
            Path::new("uv"),
            python,
            Path::new(".venv/python"),
            "https://pypi.org/simple",
        );
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            args[0..4],
            ["sync", "--no-dev", "--no-install-project", "--python"]
        );
        assert_eq!(args[4], python.to_string_lossy());
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--default-index", "https://pypi.org/simple"]));
        assert_proxy_bypass_env(&command);
        assert_python_environment_isolated(&command);
    }

    #[test]
    fn test_uv_index_override_declares_only_target_index() {
        let index = "https://mirrors.cloud.tencent.com/pypi/simple/";
        let override_file = UvIndexOverride::create(index).expect("create uv index override");
        let path = override_file.path().to_path_buf();

        assert!(path.exists(), "override file should exist while held");
        let content = fs::read_to_string(&path).expect("read override file");
        assert_eq!(content, format!("index-url = \"{index}\"\n"));
        assert!(
            !content.contains("index = ["),
            "override must not re-declare the project index list, which would outrank the flag"
        );

        drop(override_file);
        assert!(
            !path.exists(),
            "override file should be removed with its guard"
        );
    }

    #[test]
    fn test_uv_commands_ignore_global_python_selection() {
        let mut command = Command::new("uv");
        uv_python_env_with_install_dir(&mut command, Path::new(".venv/python"));

        assert!(command
            .get_envs()
            .any(|(key, value)| key == "UV_PYTHON" && value.is_none()));
        assert_proxy_bypass_env(&command);
        assert_python_environment_isolated(&command);
    }

    #[test]
    fn test_parse_uv_transfer_size() {
        assert_eq!(parse_transfer_size_bytes("1.5MiB"), Some(1_572_864));
        assert_eq!(parse_transfer_size_bytes("42 KiB"), Some(43_008));
        assert_eq!(parse_transfer_size_bytes("1.0 GiB"), Some(1_073_741_824));
        assert_eq!(parse_transfer_size_bytes("unknown"), None);
    }

    #[test]
    fn test_uv_progress_tracks_completed_downloads_once() {
        let mut progress = UvProgressState::new();
        let downloading =
            splash_update_for_dependency_output("Downloading demo-package (1.5MiB)", &mut progress)
                .expect("downloading output updates splash");
        let downloading_progress = downloading.uv_progress.expect("uv progress").progress;
        assert!((28.0..=88.0).contains(&downloading_progress));

        progress.download_started_at = Some(Instant::now() - Duration::from_secs(1));
        let downloaded =
            splash_update_for_dependency_output("Downloaded demo-package", &mut progress)
                .expect("downloaded output updates splash");
        assert_eq!(progress.downloaded_bytes, 1_572_864);
        assert!(downloaded.uv_progress.expect("uv progress").progress < 100.0);

        progress.observe("Downloaded demo-package");
        assert_eq!(progress.downloaded_bytes, 1_572_864);
    }

    #[test]
    fn test_uv_estimated_progress_keeps_subsecond_precision_and_stage_caps() {
        let mut progress = UvProgressState::new();
        let now = Instant::now();
        progress.started_at = now - Duration::from_millis(500);
        assert_eq!(progress.progress_at(now), 2.05);

        progress.started_at = now - Duration::from_secs(600);
        assert_eq!(progress.progress_at(now), 20.0);

        progress.download_started_at = Some(now - Duration::from_secs(2));
        assert_eq!(progress.progress_at(now), 28.5);
        progress.download_started_at = Some(now - Duration::from_secs(600));
        assert_eq!(progress.progress_at(now), 88.0);

        progress.observe("Prepared 2 packages in 1s");
        assert_eq!(progress.progress_at(now), 92.0);
        progress.observe("Installed 2 packages in 1s");
        assert_eq!(progress.progress_at(now), 98.0);

        let complete = dependency_complete_update();
        assert_eq!(complete.progress, 90.0);
        assert_eq!(complete.uv_progress.expect("uv progress").progress, 100.0);
    }

    #[test]
    fn test_dependency_wait_progress_never_goes_back_or_claims_completion() {
        let mut progress = UvProgressState::new();
        let waiting = dependency_wait_update(60, 85.0, &mut progress);
        assert_eq!(waiting.progress, 85.0);
        assert!(waiting.uv_progress.expect("uv progress").progress < 100.0);
    }

    fn assert_proxy_bypass_env(command: &Command) {
        let no_proxy = command
            .get_envs()
            .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case("NO_PROXY"))
            .and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()));
        assert_eq!(no_proxy.as_deref(), Some("*"));

        for key in ["ALL_PROXY", "HTTP_PROXY", "HTTPS_PROXY", "PIP_PROXY"] {
            let value = command
                .get_envs()
                .find(|(configured_key, _)| {
                    configured_key.to_string_lossy().eq_ignore_ascii_case(key)
                })
                .map(|(_, value)| value);
            assert!(matches!(value, Some(None)), "{key} must be removed");
        }
    }

    fn assert_python_environment_isolated(command: &Command) {
        for key in [
            "PYTHONHOME",
            "PYTHONPATH",
            "VIRTUAL_ENV",
            "__PYVENV_LAUNCHER__",
        ] {
            let value = command
                .get_envs()
                .find(|(configured_key, _)| {
                    configured_key.to_string_lossy().eq_ignore_ascii_case(key)
                })
                .map(|(_, value)| value);
            assert!(matches!(value, Some(None)), "{key} must be removed");
        }
    }

    #[test]
    fn test_isolate_python_child_environment() {
        let mut command = Command::new("python");
        isolate_python_child_environment(&mut command);

        assert_python_environment_isolated(&command);
    }

    #[test]
    fn test_parse_python_version() {
        assert_eq!(parse_python_version("3.14.5"), Some((3, 14, 5)));
        assert_eq!(parse_python_version(" 3.14.6\n"), Some((3, 14, 6)));
        assert_eq!(parse_python_version("3.14"), None);
        assert_eq!(parse_python_version("3.14.5.1"), None);
        assert_eq!(parse_python_version("not a version"), None);
    }

    #[test]
    fn test_venv_python_version_compatibility() {
        assert!(venv_python_version_requires_rebuild((3, 14, 4)));
        assert!(!venv_python_version_requires_rebuild((3, 14, 5)));
        assert!(!venv_python_version_requires_rebuild((3, 14, 6)));
        assert!(is_reusable_venv_python_version((3, 14, 5)));
        assert!(is_reusable_venv_python_version((3, 14, 6)));
        assert!(!is_reusable_venv_python_version((3, 15, 0)));
    }
}
