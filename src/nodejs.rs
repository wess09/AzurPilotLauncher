//! 后端前端构建所需的 Node.js：复用达标环境，否则自动下载经过摘要校验的
//! 便携发行包。只为子进程设置 PATH，不修改系统安装或全局环境变量。

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use rust_i18n::t;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{atomic::AtomicBool, mpsc, Arc},
    thread,
    time::{Duration, Instant},
};
use tracing::{info, warn};

use crate::{
    launcher::ensure_not_cancelled, progress::SplashUpdate, sources::Preferences,
    task_process::TaskChild,
};

// 更新版本时同时核对官方 release 的 SHASUMS256.txt，并同步六个平台的摘要。
const VERSION: &str = "24.22.2";
const MIN_VERSION: (u32, u32, u32) = (22, 12, 0);
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;
const EXTRACT_SCRIPT: &str = include_str!("../tools/extract_nodejs.py");

struct Distribution {
    platform: &'static str,
    sha256: &'static str,
}

fn distribution(os: &str, arch: &str) -> Result<Distribution> {
    let (platform, sha256) = match (os, arch) {
        ("windows", "x86_64") => (
            "win-x64",
            "ca1bff5d8ead13c9f2ea404a176db46bd430704d5b5113734049fa365e7799e2",
        ),
        ("windows", "aarch64") => (
            "win-arm64",
            "f47968c213105077f64c6510f007632c0ca2574802a0b5067c327c49abe1c468",
        ),
        ("macos", "x86_64") => (
            "darwin-x64",
            "d5e336d588c45be0c37dbd52cc29895593009ca4997a77a1fd02815131ed0e52",
        ),
        ("macos", "aarch64") => (
            "darwin-arm64",
            "aa9549349e47dffaa42dcf95d6e06ade8c4bdafc980979883b88767670df385a",
        ),
        ("linux", "x86_64") => (
            "linux-x64",
            "361718c444e49af2ca5a110c83ba6a8901143f23ab285ebbef0341f391f482a9",
        ),
        ("linux", "aarch64") => (
            "linux-arm64",
            "95c264150aceb7f3437a29fd6984924f5f8d1511ea3cbd6c508b83f6c555dae2",
        ),
        _ => bail!(t!(
            "errors.nodejs_unsupported",
            platform = format!("{os}-{arch}")
        )),
    };
    Ok(Distribution { platform, sha256 })
}

impl Distribution {
    fn directory(&self) -> String {
        format!("node-v{VERSION}-{}", self.platform)
    }

    fn filename(&self) -> String {
        let extension = if self.platform.starts_with("win-") {
            "zip"
        } else {
            "tar.gz"
        };
        format!("{}.{extension}", self.directory())
    }

    fn target(&self, root: &Path) -> Result<PathBuf> {
        crate::components::secure_target(
            root,
            &format!(".azurpilot/tools/nodejs/{}", self.directory()),
        )
    }
}

fn bin_directory(runtime: &Path) -> PathBuf {
    if cfg!(windows) {
        runtime.to_owned()
    } else {
        runtime.join("bin")
    }
}

fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().strip_prefix('v')?.split('.');
    let result = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(result)
}

/// 限时且可取消地执行探测命令，避免损坏的本地 node 挂住启动任务。
fn node_version(node: &Path, cancel: &AtomicBool) -> Result<Option<(u32, u32, u32)>> {
    ensure_not_cancelled(cancel)?;
    let mut command = Command::new(node);
    command
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    let Ok(mut child) = TaskChild::spawn(&mut command) else {
        return Ok(None);
    };
    let stdout = child.take_stdout().expect("piped stdout");
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = String::new();
        let result = stdout.take(128).read_to_string(&mut output).map(|_| output);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        ensure_not_cancelled(cancel)?;
        if let Some(status) = child.try_wait()? {
            return Ok(if status.success() {
                receiver
                    .recv_timeout(Duration::from_millis(100))
                    .ok()
                    .and_then(|result| result.ok())
                    .and_then(|output| parse_version(&output))
            } else {
                None
            });
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn usable(bin: &Path, cancel: &AtomicBool) -> Result<bool> {
    let node = bin.join(if cfg!(windows) { "node.exe" } else { "node" });
    let npm = bin.join(if cfg!(windows) { "npm.cmd" } else { "npm" });
    Ok(node.is_file()
        && npm.is_file()
        && node_version(&node, cancel)?.is_some_and(|version| version >= MIN_VERSION))
}

fn system_candidates() -> Vec<PathBuf> {
    let mut paths = env::var_os("PATH")
        .map(|path| {
            env::split_paths(&path)
                .filter(|p| p.is_absolute())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    #[cfg(windows)]
    {
        // 兼容由旧启动器下载的私有 Node.js，以及安装后尚未刷新 PATH 的系统运行时。
        if let Some(base) = env::var_os("ProgramData") {
            paths.push(PathBuf::from(base).join("AzurPilotLauncher/nodejs"));
        }
        for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(base) = env::var_os(key) {
                paths.push(PathBuf::from(base).join("nodejs"));
            }
        }
    }
    #[cfg(unix)]
    paths.extend(["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"].map(PathBuf::from));
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}

pub(crate) fn configure_command(command: &mut Command, bin: &Path) -> Result<()> {
    let existing = command
        .get_envs()
        .find(|(key, _)| {
            key.to_str()
                .is_some_and(|key| key.eq_ignore_ascii_case("PATH"))
        })
        .map(|(_, value)| value.map(ToOwned::to_owned))
        .unwrap_or_else(|| env::var_os("PATH"));
    let mut paths = vec![bin.to_owned()];
    if let Some(existing) = existing {
        paths.extend(env::split_paths(&existing).filter(|path| path != bin));
    }
    command.env("PATH", env::join_paths(paths)?);
    Ok(())
}

fn update(title: impl Into<String>, detail: impl Into<String>, progress: f64) -> SplashUpdate {
    SplashUpdate::loading(title, detail, progress)
        .with_subtitle(t!("setup.syncing", tip = crate::setup::get_tip()))
}

fn download(
    client: &Client,
    urls: &[String],
    sha256: &str,
    archive: &Path,
    cancel: Arc<AtomicBool>,
    status: &mut impl FnMut(SplashUpdate),
) -> Result<()> {
    let mut last_error = None;
    for url in urls {
        ensure_not_cancelled(&cancel)?;
        let result = (|| -> Result<()> {
            let response = crate::cancel::request(client.get(url), &cancel)?.error_for_status()?;
            let length = response.content_length();
            if length.is_some_and(|size| size > MAX_DOWNLOAD) {
                bail!(t!("errors.nodejs_download_limit"));
            }
            let mut body = crate::cancel::body(response, cancel.clone());
            let mut file = fs::File::create(archive)?;
            let mut digest = Sha256::new();
            let mut received = 0;
            let mut buffer = [0; 128 * 1024];
            let mut last_update = Instant::now();
            loop {
                ensure_not_cancelled(&cancel)?;
                let size = body.read(&mut buffer)?;
                if size == 0 {
                    break;
                }
                received += size as u64;
                if received > MAX_DOWNLOAD {
                    bail!(t!("errors.nodejs_download_limit"));
                }
                digest.update(&buffer[..size]);
                file.write_all(&buffer[..size])?;
                if last_update.elapsed() >= Duration::from_millis(100) {
                    let fraction = length
                        .filter(|size| *size > 0)
                        .map_or(0.0, |size| received as f64 / size as f64)
                        .min(1.0);
                    status(update(
                        t!("setup.downloading_nodejs", version = VERSION),
                        format!("{:.1} MiB", received as f64 / 1048576.0),
                        86.0 + fraction * 2.0,
                    ));
                    last_update = Instant::now();
                }
            }
            if format!("{:x}", digest.finalize()) != sha256 {
                bail!(t!("errors.nodejs_digest_mismatch"));
            }
            file.sync_all()?;
            Ok(())
        })();
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                ensure_not_cancelled(&cancel)?;
                warn!("Node.js download failed: {error:#}");
                last_error = Some(error);
            }
        }
    }
    Err(last_error.expect("at least one download source"))
}

fn extract(
    python: &Path,
    archive: &Path,
    destination: &Path,
    directory: &str,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", EXTRACT_SCRIPT])
        .arg(archive)
        .arg(destination)
        .arg(directory);
    crate::setup::isolate_python_child_environment(&mut command);
    let status = crate::setup::run_status_command(&mut command, cancel)?;
    if !status.success() {
        bail!(t!(
            "errors.nodejs_extract_exit",
            status = status.to_string()
        ));
    }
    Ok(())
}

/// 完整下载、校验和探测后才替换目标；失败时恢复旧目录。
fn activate(staged: &Path, target: &Path, backup: &Path) -> Result<()> {
    let existed = target.exists();
    if existed {
        fs::rename(target, backup)?;
    }
    if let Err(error) = fs::rename(staged, target) {
        if existed {
            fs::rename(backup, target)
                .with_context(|| t!("errors.nodejs_restore_failed").to_string())?;
        }
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn ensure(
    root: &Path,
    python: &Path,
    cancel: Arc<AtomicBool>,
    sources: &Preferences,
    mut status: impl FnMut(SplashUpdate),
) -> Result<PathBuf> {
    ensure_with_candidates(
        root,
        python,
        cancel,
        sources,
        &mut status,
        system_candidates(),
    )
}

fn ensure_with_candidates(
    root: &Path,
    python: &Path,
    cancel: Arc<AtomicBool>,
    sources: &Preferences,
    mut status: impl FnMut(SplashUpdate),
    candidates: Vec<PathBuf>,
) -> Result<PathBuf> {
    ensure_not_cancelled(&cancel)?;
    status(update(
        t!("setup.checking_nodejs"),
        t!("setup.checking_nodejs"),
        86.0,
    ));
    let distribution = distribution(env::consts::OS, env::consts::ARCH)?;
    let target = distribution.target(root)?;
    for bin in std::iter::once(bin_directory(&target)).chain(candidates) {
        if usable(&bin, &cancel)? {
            info!("Using Node.js from {}", bin.display());
            return Ok(bin);
        }
    }
    let parent = target.parent().expect("Node.js tools directory");
    fs::create_dir_all(parent)?;
    let temporary = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(parent)?;
    let archive = temporary.path().join(distribution.filename());
    let official = format!(
        "https://nodejs.org/dist/v{VERSION}/{}",
        distribution.filename()
    );
    let mirror = format!(
        "https://registry.npmmirror.com/-/binary/node/v{VERSION}/{}",
        distribution.filename()
    );
    let urls = if sources.download_source.prefer_cnb() {
        vec![mirror, official]
    } else {
        vec![official, mirror]
    };
    let client = Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()?;
    status(update(
        t!("setup.downloading_nodejs", version = VERSION),
        t!("setup.downloading_nodejs", version = VERSION),
        86.0,
    ));
    download(
        &client,
        &urls,
        distribution.sha256,
        &archive,
        cancel.clone(),
        &mut status,
    )?;
    status(update(
        t!("setup.installing_nodejs"),
        t!("setup.installing_nodejs"),
        88.0,
    ));
    let extracted = temporary.path().join("extracted");
    fs::create_dir(&extracted)?;
    extract(
        python,
        &archive,
        &extracted,
        &distribution.directory(),
        &cancel,
    )?;
    let staged = extracted.join(distribution.directory());
    if !usable(&bin_directory(&staged), &cancel)? {
        bail!(t!("errors.nodejs_runtime_invalid"));
    }
    ensure_not_cancelled(&cancel)?;
    // 再次拒绝被替换成链接的目标路径；所有目录操作都位于独立工具目录内。
    let target = distribution.target(root)?;
    let backup = temporary.path().join("previous");
    if let Err(error) = activate(&staged, &target, &backup) {
        // 恢复旧目录也失败时保留备份，不能让 TempDir 清理掉原环境。
        if backup.exists() {
            let preserved = temporary.keep();
            warn!(
                "Preserved previous Node.js installation in {}",
                preserved.display()
            );
        }
        return Err(error);
    }
    info!(
        "Installed portable Node.js {VERSION} at {}",
        target.display()
    );
    Ok(bin_directory(&target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn supported_distributions_have_pinned_digests_and_native_archives() {
        for os in ["windows", "macos", "linux"] {
            for arch in ["x86_64", "aarch64"] {
                let package = distribution(os, arch).unwrap();
                assert_eq!(package.sha256.len(), 64);
                assert!(package.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
                assert!(package.filename().ends_with(if os == "windows" {
                    ".zip"
                } else {
                    ".tar.gz"
                }));
            }
        }
        assert!(distribution("linux", "unknown").is_err());
        // Vite's minimum: reject old 22.x and 20.x, allow 22.12+ and 24.x.
        for (text, supported) in [
            ("v22.11.0", false),
            ("v22.12.0", true),
            ("v20.18.0", false),
            ("v24.22.2\r\n", true),
        ] {
            assert_eq!(
                parse_version(text).is_some_and(|version| version >= MIN_VERSION),
                supported
            );
        }
        for text in ["node v24.22.2", "v24.22.2-extra", "v24.22", "v24.22.2.0"] {
            assert!(parse_version(text).is_none());
        }
    }

    #[test]
    fn runtime_path_is_only_added_to_the_child_and_preserves_other_entries() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("node bin");
        let other = root.path().join("other");
        let original = env::var_os("PATH");
        let mut command = Command::new("python");
        command.env("PATH", env::join_paths([&other, &bin]).unwrap());
        configure_command(&mut command, &bin).unwrap();
        let path = command
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .unwrap()
            .1
            .unwrap();
        assert_eq!(env::split_paths(path).collect::<Vec<_>>(), vec![bin, other]);
        assert_eq!(env::var_os("PATH"), original);
    }

    #[test]
    fn download_retries_corrupt_payload_and_only_accepts_the_pinned_digest() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let payload = b"verified archive";
        let server = thread::spawn(move || {
            for bytes in [b"corrupt archive".as_slice(), payload.as_slice()] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0; 4096];
                stream.read(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                )
                .unwrap();
                stream.write_all(bytes).unwrap();
            }
        });
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("node.zip");
        let urls = vec![
            format!("http://{address}/first"),
            format!("http://{address}/fallback"),
        ];
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        download(
            &client,
            &urls,
            &format!("{:x}", Sha256::digest(payload)),
            &archive,
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(archive).unwrap(), payload);
        server.join().unwrap();
    }

    #[test]
    fn failed_activation_restores_the_previous_runtime() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("runtime");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), "old runtime").unwrap();
        assert!(activate(
            &root.path().join("missing"),
            &target,
            &root.path().join("backup")
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(target.join("keep")).unwrap(),
            "old runtime"
        );
        assert!(!root.path().join("backup").exists());
    }

    #[test]
    fn cancelled_setup_does_not_create_or_remove_runtime_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("keep"), "environment").unwrap();
        assert!(ensure_with_candidates(
            root.path(),
            Path::new("unused"),
            Arc::new(AtomicBool::new(true)),
            &Preferences::default(),
            |_| {},
            vec![]
        )
        .is_err());
        assert!(!root.path().join(".azurpilot").exists());
        assert_eq!(
            fs::read_to_string(root.path().join("keep")).unwrap(),
            "environment"
        );
    }

    #[test]
    #[ignore = "downloads official Node.js; set AZURPILOT_NODEJS_TEST_PYTHON to Python 3.14"]
    fn real_nodejs_portable_install_and_reuse() {
        use crate::window_util::CreateNoWindow as _;
        let python =
            PathBuf::from(env::var_os("AZURPILOT_NODEJS_TEST_PYTHON").expect("Python path"));
        let root = tempfile::tempdir().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let sources = Preferences {
            download_source: crate::sources::DownloadSource::Github,
            ..Default::default()
        };
        let bin = ensure_with_candidates(
            root.path(),
            &python,
            cancel.clone(),
            &sources,
            |_| {},
            vec![],
        )
        .unwrap();
        assert!(bin.starts_with(root.path()));
        let node = bin.join(if cfg!(windows) { "node.exe" } else { "node" });
        let cli = if cfg!(windows) {
            bin.join("node_modules/npm/bin/npm-cli.js")
        } else {
            bin.join("../lib/node_modules/npm/bin/npm-cli.js")
        };
        // The same child PATH used by deploy.app must expose both node and npm.
        let mut command = Command::new(&node);
        command.arg(&cli).arg("--version").stdout(Stdio::null());
        configure_command(&mut command, &bin).unwrap();
        assert!(crate::setup::run_status_command(&mut command, &cancel)
            .unwrap()
            .success());
        let mut command = Command::new(&python);
        command.args(["-I", "-c", "import subprocess; subprocess.run(['node', '--version'], check=True); import shutil; assert shutil.which('npm')"]);
        configure_command(&mut command, &bin).unwrap();
        assert!(command.create_no_window().status().unwrap().success());
        let mut downloaded_again = false;
        let reused = ensure_with_candidates(
            root.path(),
            &python,
            cancel,
            &sources,
            |status| {
                downloaded_again |= status.title.contains(VERSION);
            },
            vec![],
        )
        .unwrap();
        assert_eq!(bin, reused);
        assert!(!downloaded_again);
    }
}
