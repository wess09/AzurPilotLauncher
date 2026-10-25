//! CI 组件协议。运行时只使用 HTTPS/ZIP，不执行 Git、uv、npm 或 deploy 安装脚本。
//! 全部包校验成功后才提交；持久化日志保证进程中断后也能回滚。
use crate::{
    launcher::ensure_not_cancelled,
    progress::SplashUpdate,
    sources::{DownloadSource, Preferences, PypiSource, PythonSource},
};
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tracing::{info, warn};

const IDS: [&str; 6] = [
    "source",
    "resources",
    "frontend",
    "python",
    "dependencies",
    "adb",
];
const RECEIPT: &str = ".azurpilot/installed.json";
const TRANSACTION: &str = ".azurpilot/transaction";
const MANIFEST_URLS: [&str; 2] = [
    crate::updater::LAUNCHER_UPDATE_URL,
    crate::updater::LAUNCHER_UPDATE_FALLBACK_URL,
];

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Component {
    pub id: String,
    pub sha256: String,
    pub size: u64,
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Manifest {
    pub schema: u32,
    pub revision: String,
    pub python: String,
    pub platforms: BTreeMap<String, Vec<Component>>,
    #[serde(default)]
    pub installers: BTreeMap<String, Component>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct InstalledComponent {
    sha256: String,
    files: BTreeMap<String, u64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Receipt {
    manifest: Manifest,
    platform: String,
    components: BTreeMap<String, InstalledComponent>,
}
#[derive(Deserialize, Serialize)]
struct Change {
    path: String,
    existed: bool,
}

fn platform() -> String {
    // Windows ARM64 可运行完整的 x64 Python/ADB，不能混用 ARM64 解释器和 x64 wheels。
    let arch = if cfg!(windows) {
        "x86_64"
    } else {
        std::env::consts::ARCH
    };
    format!("{}-{arch}", std::env::consts::OS)
}

fn read_receipt(root: &Path) -> Result<Receipt> {
    let receipt: Receipt = serde_json::from_slice(&fs::read(root.join(RECEIPT))?)?;
    let expected = receipt.manifest.validate(&receipt.platform)?;
    if receipt.platform != platform() {
        bail!("Installed runtime belongs to another platform");
    }
    for (id, installed) in &receipt.components {
        if !IDS.contains(&id.as_str())
            || !valid_hash(&installed.sha256)
            || installed.files.is_empty()
        {
            bail!("Invalid installed component: {id}");
        }
        for name in installed.files.keys() {
            component_path(id, name)?;
        }
        validate_files(id, &installed.files)?;
        if !expected
            .iter()
            .any(|c| c.id == *id && c.sha256 == installed.sha256)
        {
            bail!("Installed component differs from manifest: {id}");
        }
    }
    Ok(receipt)
}

fn intact(root: &Path, installed: &InstalledComponent) -> bool {
    !installed.files.is_empty()
        && installed.files.iter().all(|(name, size)| {
            secure_target(root, name)
                .is_ok_and(|path| fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == *size))
        })
}

pub(crate) fn deployment_required(root: &Path) -> bool {
    if root.join(TRANSACTION).join("journal.json").exists() {
        return true;
    }
    let Ok(receipt) = read_receipt(root) else {
        return true;
    };
    IDS.iter()
        .any(|id| !receipt.components.get(*id).is_some_and(|c| intact(root, c)))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Manifest {
    fn validate(&self, target: &str) -> Result<&Vec<Component>> {
        let parts = self.python.split('.').collect::<Vec<_>>();
        if self.schema != 1
            || self.revision.len() != 40
            || !self.revision.bytes().all(|b| b.is_ascii_hexdigit())
            || parts.len() != 3
            || parts[0] != "3"
            || parts[1] != "14"
            || parts[2].parse::<u32>().is_err()
        {
            bail!(t!("components.invalid_manifest"));
        }
        let components = self
            .platforms
            .get(target)
            .with_context(|| t!("components.unsupported", platform = target))?;
        let ids = components
            .iter()
            .map(|c| c.id.as_str())
            .collect::<BTreeSet<_>>();
        if ids != IDS.into_iter().collect() || components.len() != IDS.len() {
            bail!(t!("components.invalid_manifest"));
        }
        for component in components {
            if !valid_hash(&component.sha256)
                || component.size == 0
                || component.size > 16 * 1024 * 1024 * 1024
                || component.urls.is_empty()
                || component.urls.iter().any(|url| validate_url(url).is_err())
            {
                bail!(t!("components.invalid_manifest"));
            }
            if let Some(requirements) = &component.requirements {
                validate_requirements(requirements)?;
            }
        }
        if let Some(uv) = self.installers.get(target) {
            if uv.id != "uv" || !valid_hash(&uv.sha256) || uv.size == 0 || uv.urls.is_empty() {
                bail!("Invalid uv installer");
            }
            for url in &uv.urls {
                validate_url(url)?;
            }
        }
        Ok(components)
    }
}

fn validate_requirements(requirements: &str) -> Result<()> {
    if requirements.trim().is_empty() || requirements.len() > 256 * 1024 {
        bail!("Invalid pinned requirements");
    }
    if requirements.lines().any(|line| {
        let Some((name, version)) = line.split_once("==") else {
            return true;
        };
        name.is_empty()
            || version.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || !version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+!-_".contains(&b))
    }) {
        bail!("PyPI requirements must contain only pinned package versions");
    }
    Ok(())
}

fn run_install_command(command: &mut std::process::Command, cancel: &AtomicBool) -> Result<()> {
    use std::io::{BufRead, BufReader};
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    crate::setup::isolate_python_child_environment(command);
    for key in [
        "UV_INDEX",
        "UV_INDEX_URL",
        "UV_DEFAULT_INDEX",
        "UV_EXTRA_INDEX_URL",
        "PIP_INDEX_URL",
        "PIP_EXTRA_INDEX_URL",
        "UV_PYTHON",
        "VIRTUAL_ENV",
    ] {
        command.env_remove(key);
    }
    let mut child = crate::task_process::TaskChild::spawn(command)?;
    let readers: Vec<Box<dyn Read + Send>> = child
        .take_stdout()
        .map(|s| Box::new(s) as Box<dyn Read + Send>)
        .into_iter()
        .chain(
            child
                .take_stderr()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        )
        .collect();
    let handles = readers
        .into_iter()
        .map(|reader| {
            std::thread::spawn(move || {
                for line in BufReader::new(reader)
                    .lines()
                    .map_while(std::result::Result::ok)
                {
                    info!("runtime installer: {line}");
                }
            })
        })
        .collect::<Vec<_>>();
    loop {
        ensure_not_cancelled(cancel)?;
        if let Some(status) = child.try_wait()? {
            for handle in handles {
                let _ = handle.join();
            }
            if !status.success() {
                bail!("Runtime installer failed: {status}");
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn copy_python_tree(
    source: &Path,
    target: &Path,
    canonical_root: &Path,
    cancel: &AtomicBool,
    depth: u32,
) -> Result<()> {
    ensure_not_cancelled(cancel)?;
    if depth > 64 || !source.canonicalize()?.starts_with(canonical_root) {
        bail!("Python runtime link escapes installation directory");
    }
    if source.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            if matches!(
                entry.file_name().to_str(),
                Some("site-packages" | "__pycache__")
            ) {
                continue;
            }
            copy_python_tree(
                &entry.path(),
                &target.join(entry.file_name()),
                canonical_root,
                cancel,
                depth + 1,
            )?;
        }
    } else if source.is_file() && source.extension().is_none_or(|e| e != "pyc") {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    }
    Ok(())
}

fn record_tree(
    root: &Path,
    directory: &Path,
    id: &str,
    cancel: &AtomicBool,
    files: &mut BTreeMap<String, u64>,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        ensure_not_cancelled(cancel)?;
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == "__pycache__" || path.extension().is_some_and(|e| e == "pyc") {
            continue;
        }
        if entry.file_type()?.is_symlink() {
            bail!("Installer created an unexpected symlink");
        }
        if path.is_dir() {
            record_tree(root, &path, id, cancel, files)?;
        } else {
            let name = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            component_path(id, &name)?;
            files.insert(name, entry.metadata()?.len());
        }
    }
    Ok(())
}

fn install_from_mirror(
    root: &Path,
    manifest: &Manifest,
    component: &Component,
    client: &Client,
    cancel: Arc<AtomicBool>,
    sources: &Preferences,
    progress: &mut impl FnMut(SplashUpdate),
) -> Result<InstalledComponent> {
    let transaction = secure_target(root, TRANSACTION)?;
    let stage = transaction.join("new");
    let runtime = stage.join(".venv/runtime");
    let uv_component = manifest
        .installers
        .get(&platform())
        .context("This manifest has no mirror installer; use automatic component downloads")?;
    let uv_dir = transaction.join("tools");
    let uv = uv_dir.join(if cfg!(windows) { "uv.exe" } else { "uv" });
    if !uv.is_file() {
        let archive = download(
            root,
            uv_component,
            client,
            cancel.clone(),
            sources.download_source,
            |_| {},
        )?;
        unpack(&archive, uv_component, &uv_dir, &cancel)?;
    }
    progress(SplashUpdate::loading(
        t!("components.installing"),
        t!(
            "components.mirror_install",
            component = component.id.as_str()
        ),
        45,
    ));
    let directory;
    if component.id == "python" {
        if runtime.exists() {
            crate::setup::remove_venv_entry_with_retry(&runtime, &cancel)?;
        }
        let managed = transaction.join("managed-python");
        let mut command = std::process::Command::new(&uv);
        command
            .args([
                "--no-config",
                "python",
                "install",
                "--no-bin",
                "--managed-python",
                "--install-dir",
            ])
            .arg(&managed)
            .arg(if cfg!(windows) {
                format!("cpython-{}-windows-x86_64-none", manifest.python)
            } else {
                manifest.python.clone()
            });
        if let Some(mirror) = sources.python_source.mirror().or_else(|| {
            if sources.download_source.prefer_cnb() {
                PythonSource::Cnb.mirror()
            } else {
                None
            }
        }) {
            command.env("UV_PYTHON_INSTALL_MIRROR", mirror);
        } else {
            command.env_remove("UV_PYTHON_INSTALL_MIRROR");
        }
        command.env("UV_CACHE_DIR", secure_target(root, ".azurpilot/uv-cache")?);
        run_install_command(&mut command, &cancel)?;
        let home = fs::read_dir(&managed)?
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .find(|p| {
                p.join(if cfg!(windows) {
                    "python.exe"
                } else {
                    "bin/python3.14"
                })
                .is_file()
            })
            .context("Python mirror did not provide the requested interpreter")?;
        copy_python_tree(&home, &runtime, &home.canonicalize()?, &cancel, 0)?;
        directory = runtime;
    } else {
        let python_relative = if cfg!(windows) {
            "python.exe"
        } else {
            "bin/python3.14"
        };
        if !runtime.join(python_relative).is_file() {
            let installed = secure_target(root, ".venv/runtime")?;
            copy_python_tree(&installed, &runtime, &installed.canonicalize()?, &cancel, 0)?;
        }
        let requirements = component
            .requirements
            .as_deref()
            .context("This manifest has no locked PyPI requirements")?;
        validate_requirements(requirements)?;
        let requirement_path = transaction.join("requirements.txt");
        fs::write(&requirement_path, requirements)?;
        let packages = runtime.join(if cfg!(windows) {
            "Lib/site-packages"
        } else {
            "lib/python3.14/site-packages"
        });
        if packages.exists() {
            crate::setup::remove_venv_entry_with_retry(&packages, &cancel)?;
        }
        let mut command = std::process::Command::new(&uv);
        command
            .args([
                "--no-config",
                "pip",
                "sync",
                "--system",
                "--break-system-packages",
                "--python",
            ])
            .arg(runtime.join(python_relative))
            .arg("--default-index")
            .arg(sources.pypi_source.index(sources.download_source))
            .arg(&requirement_path)
            .env("UV_CACHE_DIR", secure_target(root, ".azurpilot/uv-cache")?);
        run_install_command(&mut command, &cancel)?;
        directory = packages;
    }
    let mut files = BTreeMap::new();
    record_tree(&stage, &directory, &component.id, &cancel, &mut files)?;
    validate_files(&component.id, &files)?;
    Ok(InstalledComponent {
        sha256: component.sha256.clone(),
        files,
    })
}

fn validate_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value)?;
    let local = cfg!(any(test, debug_assertions))
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if (url.scheme() != "https" && !local) || !url.username().is_empty() || url.password().is_some()
    {
        bail!("Component URL must use HTTPS without credentials");
    }
    Ok(())
}

fn client() -> Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent(concat!("AzurPilotLauncher/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 10 || validate_url(attempt.url().as_str()).is_err() {
                attempt.error("Unsafe component redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()?)
}

fn fetch_manifest(client: &Client, cancel: Arc<AtomicBool>) -> Result<Manifest> {
    #[cfg(debug_assertions)]
    let urls = std::env::var("AZURPILOT_STABLE_MANIFEST")
        .ok()
        .map(|u| vec![u])
        .unwrap_or_else(|| MANIFEST_URLS.iter().map(|u| (*u).into()).collect());
    #[cfg(not(debug_assertions))]
    let urls = MANIFEST_URLS
        .iter()
        .map(|u| (*u).to_owned())
        .collect::<Vec<_>>();
    let mut errors = Vec::new();
    for url in urls {
        ensure_not_cancelled(&cancel)?;
        let result = (|| {
            validate_url(&url)?;
            let response =
                crate::cancel::request(client.get(&url).timeout(Duration::from_secs(30)), &cancel)?
                    .error_for_status()?;
            let mut bytes = Vec::new();
            crate::cancel::body(response, cancel.clone())
                .take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 2 * 1024 * 1024 {
                bail!("Component manifest too large");
            }
            let stable: serde_json::Value = serde_json::from_slice(&bytes)?;
            let manifest: Manifest = serde_json::from_value(
                stable
                    .get("runtime")
                    .context("stable.json is missing runtime components")?
                    .clone(),
            )?;
            manifest.validate(&platform())?;
            Ok(manifest)
        })();
        match result {
            Ok(manifest) => return Ok(manifest),
            Err(error) => {
                warn!("Component manifest {url}: {error:#}");
                errors.push(format!("{error:#}"));
            }
        }
    }
    bail!(t!("components.manifest_failed", error = errors.join("; ")))
}

pub(crate) fn deploy(
    root: &Path,
    progress: &mut impl FnMut(SplashUpdate),
    cancel: Arc<AtomicBool>,
    skip_update: bool,
    sources: &Preferences,
) -> Result<()> {
    install(root, progress, cancel, skip_update, false, sources)
}
pub(crate) fn repair(
    root: &Path,
    progress: &mut impl FnMut(SplashUpdate),
    cancel: Arc<AtomicBool>,
    sources: &Preferences,
) -> Result<()> {
    install(root, progress, cancel, true, true, sources)
}

fn install(
    root: &Path,
    progress: &mut impl FnMut(SplashUpdate),
    cancel: Arc<AtomicBool>,
    skip_update: bool,
    repair: bool,
    sources: &Preferences,
) -> Result<()> {
    progress(SplashUpdate::loading(
        t!("components.checking"),
        t!("components.checking_detail"),
        5,
    ));
    fs::create_dir_all(root)?;
    secure_target(root, RECEIPT)?;
    recover(root)?;
    let old = read_receipt(root).ok();
    let client = client()?;
    let manifest = if skip_update && old.is_some() {
        old.as_ref().unwrap().manifest.clone()
    } else {
        match fetch_manifest(&client, cancel.clone()) {
            Ok(manifest) => manifest,
            Err(error) => {
                ensure_not_cancelled(&cancel)?;
                if !repair && !deployment_required(root) {
                    warn!("Using complete installed runtime offline: {error:#}");
                    return Ok(());
                }
                old.as_ref().map(|r| r.manifest.clone()).ok_or(error)?
            }
        }
    };
    apply_manifest(
        root, progress, cancel, manifest, old, &client, repair, sources,
    )
}

fn apply_manifest(
    root: &Path,
    progress: &mut impl FnMut(SplashUpdate),
    cancel: Arc<AtomicBool>,
    manifest: Manifest,
    old: Option<Receipt>,
    client: &Client,
    repair: bool,
    sources: &Preferences,
) -> Result<()> {
    let mut components = manifest.validate(&platform())?.iter().collect::<Vec<_>>();
    components.sort_by_key(|component| match component.id.as_str() {
        "python" => 0,
        "dependencies" => 1,
        _ => 2,
    });
    let transaction = secure_target(root, TRANSACTION)?;
    fs::create_dir_all(transaction.join("new"))?;
    let mut installed = BTreeMap::new();
    let mut changed = BTreeSet::new();
    for (index, component) in components.iter().enumerate() {
        ensure_not_cancelled(&cancel)?;
        let previous = old.as_ref().and_then(|r| r.components.get(&component.id));
        let force = repair && matches!(component.id.as_str(), "python" | "dependencies" | "adb");
        if !force && previous.is_some_and(|p| p.sha256 == component.sha256 && intact(root, p)) {
            installed.insert(component.id.clone(), previous.unwrap().clone());
            continue;
        }
        let custom = (component.id == "python" && sources.python_source != PythonSource::Auto)
            || (component.id == "dependencies" && sources.pypi_source != PypiSource::Auto);
        let normal = if custom {
            None
        } else {
            Some((|| {
                let archive = download(
                    root,
                    component,
                    client,
                    cancel.clone(),
                    sources.download_source,
                    |percent| {
                        progress(SplashUpdate::loading(
                            t!("components.downloading"),
                            t!(
                                "components.download_detail",
                                component = component.id.as_str(),
                                percent = format!("{percent:.0}")
                            ),
                            10.0 + (index as f64 + percent / 100.0) / components.len() as f64
                                * 70.0,
                        ));
                    },
                )?;
                unpack(&archive, component, &transaction.join("new"), &cancel)
            })())
        };
        let record = match normal {
            Some(Ok(record)) => record,
            Some(Err(error)) => {
                ensure_not_cancelled(&cancel)?;
                if !matches!(component.id.as_str(), "python" | "dependencies")
                    || !manifest.installers.contains_key(&platform())
                {
                    return Err(error);
                }
                warn!("Prebuilt component unavailable, trying configured mirror: {error:#}");
                install_from_mirror(
                    root,
                    &manifest,
                    component,
                    client,
                    cancel.clone(),
                    sources,
                    progress,
                )?
            }
            None => install_from_mirror(
                root,
                &manifest,
                component,
                client,
                cancel.clone(),
                sources,
                progress,
            )?,
        };
        if let Some(previous) = previous {
            changed.extend(previous.files.keys().cloned());
        }
        changed.extend(record.files.keys().cloned());
        installed.insert(component.id.clone(), record);
    }
    // 源码、前端与依赖必须来自同一份清单，不能逐个组件提前提交。
    let mut owners = BTreeSet::new();
    for record in installed.values() {
        for name in record.files.keys() {
            if !owners.insert(name.to_lowercase()) {
                bail!("Overlapping component path: {name}");
            }
        }
    }
    let receipt = Receipt {
        manifest: manifest.clone(),
        platform: platform(),
        components: installed,
    };
    let target_receipt = transaction.join("new").join(RECEIPT);
    fs::create_dir_all(target_receipt.parent().unwrap())?;
    fs::write(target_receipt, serde_json::to_vec(&receipt)?)?;
    changed.insert(RECEIPT.into());
    ensure_not_cancelled(&cancel)?;
    progress(SplashUpdate::loading(
        t!("components.installing"),
        t!("components.installing_detail"),
        85,
    ));
    commit(root, &changed, &cancel)?;
    progress(SplashUpdate::loading(
        t!("setup.finishing"),
        t!("setup.ready_to_launch"),
        94,
    ));
    info!(
        "Deployed revision {} with Python {}",
        manifest.revision, manifest.python
    );
    Ok(())
}

fn file_hash(path: &Path, cancel: &AtomicBool) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut length = 0;
    let mut buffer = vec![0; 128 * 1024];
    loop {
        ensure_not_cancelled(cancel)?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length += count as u64;
        hash.update(&buffer[..count]);
    }
    Ok((format!("{:x}", hash.finalize()), length))
}

fn download(
    root: &Path,
    component: &Component,
    client: &Client,
    cancel: Arc<AtomicBool>,
    source: DownloadSource,
    mut progress: impl FnMut(f64),
) -> Result<PathBuf> {
    let cache = secure_target(root, &format!(".azurpilot/cache/{}.zip", component.sha256))?;
    fs::create_dir_all(cache.parent().unwrap())?;
    if cache.is_file() && file_hash(&cache, &cancel)? == (component.sha256.clone(), component.size)
    {
        return Ok(cache);
    }
    let mut errors = Vec::new();
    for url in source.order_urls(&component.urls) {
        ensure_not_cancelled(&cancel)?;
        let result = (|| -> Result<()> {
            let response = crate::cancel::request(client.get(url), &cancel)?.error_for_status()?;
            if response
                .content_length()
                .is_some_and(|size| size != component.size)
            {
                bail!("Component size differs from manifest");
            }
            let mut temporary = tempfile::NamedTempFile::new_in(cache.parent().unwrap())?;
            let mut body = crate::cancel::body(response, cancel.clone());
            let mut buffer = vec![0; 128 * 1024];
            let mut hash = Sha256::new();
            let mut length = 0;
            loop {
                let count = body.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                length += count as u64;
                if length > component.size {
                    bail!("Component exceeds declared size");
                }
                hash.update(&buffer[..count]);
                temporary.write_all(&buffer[..count])?;
                progress(length as f64 / component.size as f64 * 100.0);
            }
            if length != component.size || format!("{:x}", hash.finalize()) != component.sha256 {
                bail!(t!(
                    "components.checksum_failed",
                    component = component.id.as_str()
                ));
            }
            ensure_not_cancelled(&cancel)?;
            temporary.as_file().sync_all()?;
            if cache.exists() {
                fs::remove_file(&cache)?;
            }
            temporary.persist(&cache)?;
            Ok(())
        })();
        match result {
            Ok(()) => return Ok(cache),
            Err(error) => errors.push(format!("{error:#}")),
        }
    }
    bail!(t!(
        "components.download_failed",
        component = component.id.as_str(),
        error = errors.join("; ")
    ))
}

fn relative(name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.contains(['\\', ':', '\0']) {
        bail!("Unsafe component path: {name}");
    }
    for segment in name.split('/') {
        let stem = segment.split('.').next().unwrap_or("").to_ascii_uppercase();
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.ends_with(['.', ' '])
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit())
        {
            bail!("Unsafe component path: {name}");
        }
    }
    Ok(PathBuf::from(name))
}

fn component_path(id: &str, name: &str) -> Result<PathBuf> {
    let path = relative(name)?;
    let allowed = match id {
        "python" => name.starts_with(".venv/runtime/") && !name.contains("/site-packages/"),
        "dependencies" => {
            name.starts_with(".venv/runtime/Lib/site-packages/")
                || name.starts_with(".venv/runtime/lib/python3.14/site-packages/")
        }
        "adb" => name.starts_with(".venv/Scripts/") || name.starts_with(".venv/bin/"),
        "frontend" => name.starts_with("frontend/dist/"),
        "resources" => name.starts_with("assets/") || name.starts_with("bin/"),
        "uv" => name == "uv" || name == "uv.exe",
        "source" => {
            let lower = name.to_ascii_lowercase();
            let root = lower.split('/').next().unwrap();
            !root.starts_with('.')
                && !matches!(
                    root,
                    "assets"
                        | "bin"
                        | "bootstrap"
                        | "log"
                        | "cache"
                        | "screenshots"
                        | "downloads"
                        | "azurpilot"
                        | "azurpilot.exe"
                )
                && !lower.starts_with("frontend/dist/")
                && (root != "config"
                    || lower.starts_with("config/template")
                    || lower.starts_with("config/deploy.template"))
        }
        _ => false,
    };
    if !allowed {
        bail!("Component {id} cannot own {name}");
    }
    Ok(path)
}

fn validate_files(id: &str, files: &BTreeMap<String, u64>) -> Result<()> {
    let required = match id {
        "source" => vec![
            "gui.py",
            "pyproject.toml",
            "BUILD_MANIFEST",
            "module/runtime/prebuilt.py",
        ],
        "frontend" => vec![
            "frontend/dist/index.html",
            "frontend/dist/.source-fingerprint",
        ],
        "python" => vec![if cfg!(windows) {
            ".venv/runtime/python.exe"
        } else {
            ".venv/runtime/bin/python3.14"
        }],
        "adb" => vec![if cfg!(windows) {
            ".venv/Scripts/adb.exe"
        } else {
            ".venv/bin/adb"
        }],
        "dependencies" | "resources" => vec![],
        "uv" => vec![if cfg!(windows) { "uv.exe" } else { "uv" }],
        _ => bail!("Unknown component: {id}"),
    };
    if files.is_empty() || required.iter().any(|name| !files.contains_key(*name)) {
        bail!("Component {id} is missing required files");
    }
    Ok(())
}

/// 不跟随已有符号链接、junction 或挂载点，避免管理员进程写出安装目录。
fn secure_target(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = relative(name)?;
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                let mut link = metadata.file_type().is_symlink();
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    link |= metadata.file_attributes() & 0x400 != 0;
                }
                if link {
                    bail!("Refusing component path through a link: {}", path.display());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

fn unpack(
    archive: &Path,
    component: &Component,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<InstalledComponent> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    let mut files = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for index in 0..zip.len() {
        ensure_not_cancelled(cancel)?;
        let mut entry = zip.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let relative = component_path(&component.id, &name)?;
        if !names.insert(name.to_lowercase())
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            bail!("Duplicate or symlink ZIP entry: {name}");
        }
        total = total
            .checked_add(entry.size())
            .context("Component size overflow")?;
        if total > 32 * 1024 * 1024 * 1024 {
            bail!("Uncompressed component too large");
        }
        let path = destination.join(relative);
        if path.exists() {
            bail!("Overlapping component file: {name}");
        }
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = File::create(&path)?;
        let expected = entry.size();
        let mut written = 0u64;
        let mut buffer = vec![0; 128 * 1024];
        loop {
            ensure_not_cancelled(cancel)?;
            let count = entry.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            written += count as u64;
            if written > expected {
                bail!("ZIP entry too large");
            }
            file.write_all(&buffer[..count])?;
        }
        if written != expected {
            bail!("Truncated ZIP entry");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &path,
                fs::Permissions::from_mode(entry.unix_mode().unwrap_or(0o644) & 0o777),
            )?;
        }
        files.insert(name, expected);
    }
    validate_files(&component.id, &files)?;
    Ok(InstalledComponent {
        sha256: component.sha256.clone(),
        files,
    })
}

fn commit(root: &Path, changed: &BTreeSet<String>, cancel: &AtomicBool) -> Result<()> {
    let transaction = secure_target(root, TRANSACTION)?;
    let mut changes = Vec::new();
    for name in changed {
        let target = secure_target(root, name)?;
        if target.exists() && !target.is_file() {
            bail!("Component target is not a file: {name}");
        }
        changes.push(Change {
            path: name.clone(),
            existed: target.is_file(),
        });
    }
    let journal = transaction.join("journal.json");
    let mut file = File::create(&journal)?;
    file.write_all(&serde_json::to_vec(&changes)?)?;
    file.sync_all()?;
    let result = (|| -> Result<()> {
        for change in &changes {
            ensure_not_cancelled(cancel)?;
            let target = secure_target(root, &change.path)?;
            let backup = transaction.join("backup").join(&change.path);
            let staged = transaction.join("new").join(&change.path);
            if change.existed {
                fs::create_dir_all(backup.parent().unwrap())?;
                fs::rename(&target, backup)?;
            }
            if staged.is_file() {
                fs::create_dir_all(target.parent().unwrap())?;
                fs::rename(staged, target)?;
            }
        }
        // 删除日志即提交点；其后中断只会留下可清理的临时目录。
        fs::remove_file(&journal)?;
        Ok(())
    })();
    if let Err(error) = result {
        recover(root).context("Component rollback failed; restart launcher to recover")?;
        return Err(error);
    }
    clean_transaction(root)?;
    Ok(())
}

fn clean_transaction(root: &Path) -> Result<()> {
    let transaction = secure_target(root, TRANSACTION)?;
    if transaction.exists() {
        crate::setup::remove_venv_entry_with_retry(&transaction, &AtomicBool::new(false))?;
    }
    Ok(())
}

fn recover(root: &Path) -> Result<()> {
    let transaction = secure_target(root, TRANSACTION)?;
    let journal = transaction.join("journal.json");
    if journal.is_file() {
        let changes: Vec<Change> = serde_json::from_slice(&fs::read(&journal)?)?;
        for change in changes.iter().rev() {
            let target = secure_target(root, &change.path)?;
            let backup = secure_target(root, &format!("{TRANSACTION}/backup/{}", change.path))?;
            if backup.is_file() {
                if target.exists() {
                    fs::remove_file(&target)?;
                }
                fs::create_dir_all(target.parent().unwrap())?;
                fs::rename(backup, target)?;
            } else if !change.existed && target.is_file() {
                fs::remove_file(target)?;
            }
        }
        fs::remove_file(journal)?;
        warn!("Recovered interrupted component deployment");
    }
    clean_transaction(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, sync::atomic::Ordering};
    use zip::write::SimpleFileOptions;

    #[test]
    fn mirror_requirements_allow_only_locked_binary_package_versions() {
        assert!(validate_requirements("numpy==2.4.6\nuv==0.13.5\n").is_ok());
        for value in [
            "",
            "--index-url https://example.com",
            "demo @ git+https://example.com/repo",
            "demo>=1",
            "demo==1 --extra-index-url https://example.com",
        ] {
            assert!(validate_requirements(value).is_err());
        }
    }

    fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            writer
                .start_file(*name, SimpleFileOptions::default().unix_permissions(0o755))
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn fixture(root: &Path, version: u8) -> Manifest {
        let python = if cfg!(windows) {
            ".venv/runtime/python.exe"
        } else {
            ".venv/runtime/bin/python3.14"
        };
        let dependency = if cfg!(windows) {
            ".venv/runtime/Lib/site-packages/demo.py"
        } else {
            ".venv/runtime/lib/python3.14/site-packages/demo.py"
        };
        let adb = if cfg!(windows) {
            ".venv/Scripts/adb.exe"
        } else {
            ".venv/bin/adb"
        };
        let body = vec![version; version as usize + 10];
        let mut source = vec![
            ("gui.py", body.as_slice()),
            ("pyproject.toml", b"test".as_slice()),
            ("BUILD_MANIFEST", b"{}".as_slice()),
            ("module/runtime/prebuilt.py", b"pass".as_slice()),
        ];
        if version == 1 {
            source.push(("obsolete.py", b"old"));
        }
        let groups = vec![
            source,
            vec![("assets/demo.png", b"image".as_slice())],
            vec![
                ("frontend/dist/index.html", b"index".as_slice()),
                (
                    "frontend/dist/.source-fingerprint",
                    b"fingerprint".as_slice(),
                ),
            ],
            vec![(python, b"python".as_slice())],
            vec![(dependency, b"dependency".as_slice())],
            vec![(adb, b"adb".as_slice())],
        ];
        fs::create_dir_all(root.join(".azurpilot/cache")).unwrap();
        let components = IDS
            .iter()
            .zip(groups)
            .map(|(id, files)| {
                let bytes = archive(&files);
                let hash = format!("{:x}", Sha256::digest(&bytes));
                fs::write(root.join(format!(".azurpilot/cache/{hash}.zip")), &bytes).unwrap();
                Component {
                    id: (*id).into(),
                    sha256: hash,
                    size: bytes.len() as u64,
                    urls: vec!["http://127.0.0.1:9/unavailable.zip".into()],
                    ..Default::default()
                }
            })
            .collect();
        Manifest {
            schema: 1,
            revision: format!("{version:040x}"),
            python: "3.14.6".into(),
            platforms: BTreeMap::from([(platform(), components)]),
            installers: BTreeMap::new(),
        }
    }
    fn apply(root: &Path, manifest: Manifest) -> Result<()> {
        recover(root)?;
        apply_manifest(
            root,
            &mut |_| {},
            Arc::new(AtomicBool::new(false)),
            manifest,
            read_receipt(root).ok(),
            &client()?,
            false,
            &Preferences::default(),
        )
    }

    #[test]
    fn empty_install_then_only_missing_component_is_redeployed() {
        let root = tempfile::tempdir().unwrap();
        assert!(deployment_required(root.path()));
        let manifest = fixture(root.path(), 1);
        apply(root.path(), manifest.clone()).unwrap();
        assert!(!deployment_required(root.path()));
        let receipt = read_receipt(root.path()).unwrap();
        let missing = receipt.components["dependencies"]
            .files
            .keys()
            .next()
            .unwrap();
        fs::remove_file(root.path().join(missing)).unwrap();
        assert!(deployment_required(root.path()));
        // 只有依赖 ZIP 留在缓存，其他源不可访问；成功说明没有重复请求完整组件。
        for component in &manifest.platforms[&platform()] {
            if component.id != "dependencies" {
                fs::remove_file(
                    root.path()
                        .join(format!(".azurpilot/cache/{}.zip", component.sha256)),
                )
                .unwrap();
            }
        }
        fs::create_dir_all(root.path().join("config")).unwrap();
        fs::write(root.path().join("config/user.json"), "keep").unwrap();
        apply(root.path(), manifest).unwrap();
        assert!(!deployment_required(root.path()));
        assert_eq!(
            fs::read_to_string(root.path().join("config/user.json")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn updates_remove_only_old_managed_files_and_keep_user_data() {
        let root = tempfile::tempdir().unwrap();
        let first = fixture(root.path(), 1);
        apply(root.path(), first).unwrap();
        fs::write(root.path().join("custom.py"), "user").unwrap();
        let next = fixture(root.path(), 2);
        apply(root.path(), next).unwrap();
        assert!(!root.path().join("obsolete.py").exists());
        assert_eq!(fs::read(root.path().join("gui.py")).unwrap(), vec![2; 12]);
        assert_eq!(
            fs::read_to_string(root.path().join("custom.py")).unwrap(),
            "user"
        );
        assert!(!deployment_required(root.path()));
    }

    #[test]
    fn failed_download_does_not_commit_already_staged_source() {
        let root = tempfile::tempdir().unwrap();
        let first = fixture(root.path(), 1);
        apply(root.path(), first).unwrap();
        let mut next = fixture(root.path(), 2);
        let dependencies = next
            .platforms
            .get_mut(&platform())
            .unwrap()
            .iter_mut()
            .find(|c| c.id == "dependencies")
            .unwrap();
        dependencies.sha256 = "0".repeat(64);
        assert!(apply(root.path(), next).is_err());
        assert_eq!(fs::read(root.path().join("gui.py")).unwrap(), vec![1; 11]);
        assert!(!deployment_required(root.path()));
    }

    #[test]
    fn interrupted_commit_recovers_old_and_new_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        fs::create_dir_all(path.join(format!("{TRANSACTION}/backup"))).unwrap();
        fs::write(path.join("gui.py"), "new").unwrap();
        fs::write(path.join(format!("{TRANSACTION}/backup/gui.py")), "old").unwrap();
        fs::write(path.join("new.py"), "new").unwrap();
        fs::write(
            path.join(format!("{TRANSACTION}/journal.json")),
            serde_json::to_vec(&vec![
                Change {
                    path: "gui.py".into(),
                    existed: true,
                },
                Change {
                    path: "new.py".into(),
                    existed: false,
                },
            ])
            .unwrap(),
        )
        .unwrap();
        recover(path).unwrap();
        recover(path).unwrap();
        assert_eq!(fs::read_to_string(path.join("gui.py")).unwrap(), "old");
        assert!(!path.join("new.py").exists());
    }

    #[test]
    fn cancellation_before_commit_preserves_existing_environment() {
        let root = tempfile::tempdir().unwrap();
        let first = fixture(root.path(), 1);
        apply(root.path(), first).unwrap();
        let cancel = Arc::new(AtomicBool::new(true));
        let next = fixture(root.path(), 2);
        assert!(apply_manifest(
            root.path(),
            &mut |_| {},
            cancel,
            next,
            read_receipt(root.path()).ok(),
            &client().unwrap(),
            true,
            &Preferences::default()
        )
        .is_err());
        assert_eq!(fs::read(root.path().join("gui.py")).unwrap(), vec![1; 11]);
        assert!(!deployment_required(root.path()));
    }

    #[test]
    fn checksum_failure_uses_next_mirror() {
        use std::{net::TcpListener, thread};
        let root = tempfile::tempdir().unwrap();
        let bytes = archive(&[("demo", b"payload")]);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let size = bytes.len();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let server = thread::spawn(move || {
            for body in [vec![0; size], bytes] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                stream.read(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        let component = Component {
            id: "source".into(),
            sha256: hash.clone(),
            size: size as u64,
            urls: vec![
                format!("http://{address}/bad"),
                format!("http://{address}/good"),
            ],
            ..Default::default()
        };
        let path = download(
            root.path(),
            &component,
            &client().unwrap(),
            Arc::new(AtomicBool::new(false)),
            DownloadSource::Cnb,
            |_| {},
        )
        .unwrap();
        assert_eq!(
            file_hash(&path, &AtomicBool::new(false)).unwrap(),
            (hash, size as u64)
        );
        server.join().unwrap();
    }

    #[test]
    fn unsafe_archives_and_manifests_are_rejected() {
        for name in [
            "../outside",
            "/absolute",
            "C:/outside",
            "a\\b",
            "a/../b",
            "a/CON.txt",
            "a/file.",
        ] {
            assert!(relative(name).is_err(), "{name}");
        }
        for (id, path) in [
            ("source", "config/user.json"),
            ("source", ".azurpilot/installed.json"),
            ("source", "frontend/dist/index.html"),
            ("python", ".venv/runtime/Lib/site-packages/a.py"),
            ("dependencies", "gui.py"),
        ] {
            assert!(component_path(id, path).is_err());
        }
        let root = tempfile::tempdir().unwrap();
        let mut manifest = fixture(root.path(), 1);
        manifest.python = "3.15.0".into();
        assert!(manifest.validate(&platform()).is_err());
        manifest.python = "3.14.7rc1".into();
        assert!(manifest.validate(&platform()).is_err());
        manifest.python = "3.14.123".into();
        assert!(manifest.validate(&platform()).is_ok());
        manifest.platforms.get_mut(&platform()).unwrap().pop();
        assert!(manifest.validate(&platform()).is_err());
    }

    #[test]
    fn cancelled_body_and_path_links_cannot_modify_files() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let link = std::os::windows::fs::symlink_dir(outside.path(), root.path().join(".venv"));
        #[cfg(unix)]
        let link = std::os::unix::fs::symlink(outside.path(), root.path().join(".venv"));
        if link.is_ok() {
            assert!(secure_target(root.path(), ".venv/runtime/python.exe").is_err());
        }
        let cancel = AtomicBool::new(false);
        cancel.store(true, Ordering::SeqCst);
        assert!(ensure_not_cancelled(&cancel).is_err());
    }
}
