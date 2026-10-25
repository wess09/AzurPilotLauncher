//! 运行组件与部署引导协议。
//!
//! 部署阶段只下载两类内容：CI 打包的 `git`/`adb` 二进制组件（内容寻址
//! ZIP，经事务提交），以及后端仓库 master 分支上的 deploy 部署脚本与
//! `config/deploy.template*.yaml` 模板（清单由 CI 写入 stable.json 的
//! `runtime.deploy`，启动器直连 CNB/GitHub raw 逐文件下载，跟随 master
//! 浮动更新）。`config/` 下的 json 与其余文件一律不下载、不修改。
//!
//! Python、依赖、源码与前端均不在部署范围：它们由启动阶段经内嵌 uv 与
//! deploy 脚本（`deploy.git` / `deploy.app` / uv sync）自行安装拉取。
use crate::{
    launcher::ensure_not_cancelled,
    progress::SplashUpdate,
    sources::{DownloadSource, Preferences},
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

const IDS: [&str; 2] = ["git", "adb"];
const RECEIPT: &str = ".azurpilot/installed.json";
const TRANSACTION: &str = ".azurpilot/transaction";
pub(crate) const TOOLS_DIR: &str = ".azurpilot/tools";
const MANIFEST_URLS: [&str; 2] = [
    crate::updater::LAUNCHER_UPDATE_URL,
    crate::updater::LAUNCHER_UPDATE_FALLBACK_URL,
];
/// 部署引导脚本就绪的判定文件：核心 deploy 脚本到位即视为引导完成。
const BOOTSTRAP_MARKER: &str = "deploy/installer.py";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Component {
    pub id: String,
    pub sha256: String,
    pub size: u64,
    pub urls: Vec<String>,
}
/// deploy 引导下载信息：raw 基址（跟随 master）与文件清单，由 CI 生成。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct DeployBootstrap {
    pub bases: Vec<String>,
    pub files: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Manifest {
    pub schema: u32,
    pub revision: String,
    pub platforms: BTreeMap<String, Vec<Component>>,
    #[serde(default)]
    pub deploy: DeployBootstrap,
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
    #[serde(default)]
    bootstrap_files: BTreeSet<String>,
}
#[derive(Deserialize, Serialize)]
struct Change {
    path: String,
    existed: bool,
}

fn platform() -> String {
    // Windows ARM64 可运行完整的 x64 git/ADB，统一使用 x64 二进制。
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
    for name in &receipt.bootstrap_files {
        bootstrap_path(name)?;
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

/// 部署是否必需：引导物（git/adb 组件与 deploy 脚本）缺失或未完成时为真。
pub(crate) fn deployment_required(root: &Path) -> bool {
    if root.join(TRANSACTION).join("journal.json").exists() {
        return true;
    }
    let Ok(receipt) = read_receipt(root) else {
        return true;
    };
    receipt.bootstrap_files.is_empty()
        || receipt
            .bootstrap_files
            .iter()
            .any(|name| !bootstrap_file_ready(root, name))
        || IDS
            .iter()
            .any(|id| !receipt.components.get(*id).is_some_and(|c| intact(root, c)))
}

fn bootstrap_file_ready(root: &Path, name: &str) -> bool {
    secure_target(root, name).is_ok_and(|path| {
        fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    })
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Manifest {
    fn validate(&self, target: &str) -> Result<&Vec<Component>> {
        if self.schema != 2
            || self.revision.len() != 40
            || !self.revision.bytes().all(|b| b.is_ascii_hexdigit())
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
        }
        if self.deploy.bases.is_empty()
            || self
                .deploy
                .bases
                .iter()
                .any(|url| validate_url(url).is_err())
            || !self
                .deploy
                .files
                .iter()
                .any(|name| name == BOOTSTRAP_MARKER)
        {
            bail!(t!("components.invalid_manifest"));
        }
        for file in &self.deploy.files {
            bootstrap_path(file)?;
        }
        Ok(components)
    }
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
    let components = manifest.validate(&platform())?.iter().collect::<Vec<_>>();
    let transaction = secure_target(root, TRANSACTION)?;
    fs::create_dir_all(transaction.join("new"))?;
    let mut installed = BTreeMap::new();
    let mut changed = BTreeSet::new();
    for (index, component) in components.iter().enumerate() {
        ensure_not_cancelled(&cancel)?;
        let previous = old.as_ref().and_then(|r| r.components.get(&component.id));
        if !repair && previous.is_some_and(|p| p.sha256 == component.sha256 && intact(root, p)) {
            installed.insert(component.id.clone(), previous.unwrap().clone());
            continue;
        }
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
                    6.0 + (index as f64 + percent / 100.0) / components.len() as f64 * 12.0,
                ));
            },
        )?;
        let record = unpack(&archive, component, &transaction.join("new"), &cancel)?;
        if let Some(previous) = previous {
            changed.extend(previous.files.keys().cloned());
        }
        changed.extend(record.files.keys().cloned());
        installed.insert(component.id.clone(), record);
    }
    // 二进制组件必须来自同一份清单，不能逐个提前提交。
    let mut owners = BTreeSet::new();
    for record in installed.values() {
        for name in record.files.keys() {
            if !owners.insert(name.to_lowercase()) {
                bail!("Overlapping component path: {name}");
            }
        }
    }
    let bootstrap_files =
        download_deploy_bootstrap(root, &manifest, client, &cancel, sources, progress)?;
    let receipt = Receipt {
        manifest: manifest.clone(),
        platform: platform(),
        components: installed,
        bootstrap_files,
    };
    let target_receipt = transaction.join("new").join(RECEIPT);
    fs::create_dir_all(target_receipt.parent().unwrap())?;
    fs::write(target_receipt, serde_json::to_vec(&receipt)?)?;
    changed.insert(RECEIPT.into());
    ensure_not_cancelled(&cancel)?;
    progress(SplashUpdate::loading(
        t!("components.installing"),
        t!("components.installing_detail"),
        19,
    ));
    commit(root, &changed, &cancel)?;
    progress(SplashUpdate::loading(
        t!("setup.finishing"),
        t!("setup.ready_to_launch"),
        24,
    ));
    info!(
        "Deployed bootstrap components at revision {} ({} deploy files)",
        manifest.revision,
        manifest.deploy.files.len()
    );
    Ok(())
}

/// 引导文件在清单中允许的路径：deploy 部署脚本与分系统的 deploy 模板。
///
/// `config/` 下的 json 等其余文件一律拒绝，防止清单被用于写用户数据。
fn bootstrap_path(name: &str) -> Result<PathBuf> {
    let path = relative(name)?;
    let allowed = name.starts_with("deploy/")
        || matches!(
            name,
            "config/deploy.template.yaml"
                | "config/deploy.template-cn.yaml"
                | "config/deploy.template-linux.yaml"
                | "config/deploy.template-linux-cn.yaml"
        );
    if !allowed {
        bail!("Deploy bootstrap cannot own {name}");
    }
    Ok(path)
}

/// 当前系统应使用的 deploy 模板文件名；国内网络环境优先 `-cn` 变体。
pub(crate) fn platform_template_file(sources: &Preferences) -> &'static str {
    let cn = sources.download_source.prefer_cnb();
    if cfg!(windows) {
        if cn {
            "config/deploy.template-cn.yaml"
        } else {
            "config/deploy.template.yaml"
        }
    } else if cn {
        "config/deploy.template-linux-cn.yaml"
    } else {
        "config/deploy.template-linux.yaml"
    }
}

/// 逐文件从仓库 raw 端点下载 deploy 引导脚本与当前系统的模板。
///
/// 清单跟随 master：文件内容不锁定摘要，双源（国内优先 CNB）按序回退，
/// 每个文件原子写入；任一必需文件全源失败时部署失败。
fn download_deploy_bootstrap(
    root: &Path,
    manifest: &Manifest,
    client: &Client,
    cancel: &Arc<AtomicBool>,
    sources: &Preferences,
    progress: &mut impl FnMut(SplashUpdate),
) -> Result<BTreeSet<String>> {
    let template = platform_template_file(sources);
    let mut files: Vec<&str> = manifest
        .deploy
        .files
        .iter()
        .map(String::as_str)
        .filter(|name| name.starts_with("deploy/"))
        .collect();
    if !manifest.deploy.files.iter().any(|name| name == template) {
        bail!("Deploy manifest has no platform template");
    }
    files.push(template);
    if manifest.deploy.bases.is_empty() || files.is_empty() {
        bail!(t!("components.invalid_manifest"));
    }
    let ordered_bases = sources.download_source.order_urls(&manifest.deploy.bases);
    let total = files.len();
    for (index, name) in files.iter().enumerate() {
        ensure_not_cancelled(cancel)?;
        // Git 接管后允许脚本正常更新，只补缺失/空文件，不覆盖已存在的脚本。
        if bootstrap_file_ready(root, name) {
            continue;
        }
        progress(SplashUpdate::loading(
            t!("components.downloading"),
            t!(
                "components.bootstrap_detail",
                file = *name,
                current = (index + 1).to_string(),
                total = total.to_string()
            ),
            18.0 + (index as f64 + 1.0) / total as f64,
        ));
        download_raw_file(root, name, &ordered_bases, client, cancel)?;
    }
    Ok(files.into_iter().map(str::to_owned).collect())
}

/// 从 raw 基址下载单个文件并原子写入目标路径。
///
/// 返回 Err 前会尝试全部基址；空响应视为失败（raw 端点不会返回空文件）。
fn download_raw_file(
    root: &Path,
    name: &str,
    bases: &[&str],
    client: &Client,
    cancel: &Arc<AtomicBool>,
) -> Result<()> {
    let target = secure_target(root, name)?;
    let mut errors = Vec::new();
    for base in bases {
        ensure_not_cancelled(cancel)?;
        let url = format!("{base}/{name}");
        let result = (|| -> Result<()> {
            let response = crate::cancel::request(client.get(&url), cancel)?.error_for_status()?;
            let mut bytes = Vec::new();
            crate::cancel::body(response, cancel.clone())
                .take(16 * 1024 * 1024)
                .read_to_end(&mut bytes)?;
            if bytes.is_empty() {
                bail!("Empty response");
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut temporary = tempfile::NamedTempFile::new_in(target.parent().unwrap_or(root))?;
            temporary.write_all(&bytes)?;
            temporary.as_file().sync_all()?;
            temporary.persist(&target).map_err(|error| error.error)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                info!("Deploy bootstrap file {name} downloaded");
                return Ok(());
            }
            Err(error) => errors.push(format!("{error:#}")),
        }
    }
    bail!(t!(
        "components.bootstrap_failed",
        file = name,
        error = errors.join("; ")
    ))
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
    let allowed = IDS.contains(&id) && name.starts_with(&format!("{TOOLS_DIR}/{id}/"));
    if !allowed {
        bail!("Component {id} cannot own {name}");
    }
    Ok(path)
}

fn validate_files(id: &str, files: &BTreeMap<String, u64>) -> Result<()> {
    let required = match id {
        "git" => vec![if cfg!(windows) {
            ".azurpilot/tools/git/cmd/git.exe"
        } else {
            ".azurpilot/tools/git/bin/git"
        }],
        "adb" => vec![if cfg!(windows) {
            ".azurpilot/tools/adb/adb.exe"
        } else {
            ".azurpilot/tools/adb/adb"
        }],
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
    use std::{io::Cursor, net::TcpListener, sync::atomic::Ordering, thread};
    use zip::write::SimpleFileOptions;

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
        let git = if cfg!(windows) {
            ".azurpilot/tools/git/cmd/git.exe"
        } else {
            ".azurpilot/tools/git/bin/git"
        };
        let adb = if cfg!(windows) {
            ".azurpilot/tools/adb/adb.exe"
        } else {
            ".azurpilot/tools/adb/adb"
        };
        let mut git_files = vec![(git, b"git".as_slice())];
        if !cfg!(windows) {
            git_files.push((
                ".azurpilot/tools/git/libexec/git-core/git-remote-https",
                b"helper".as_slice(),
            ));
        }
        if version == 1 {
            git_files.push((
                if cfg!(windows) {
                    ".azurpilot/tools/git/obsolete.exe"
                } else {
                    ".azurpilot/tools/git/libexec/git-core/obsolete"
                },
                b"old".as_slice(),
            ));
        }
        let groups = vec![git_files, vec![(adb, b"adb".as_slice())]];
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
                }
            })
            .collect();
        Manifest {
            schema: 2,
            revision: format!("{version:040x}"),
            platforms: BTreeMap::from([(platform(), components)]),
            deploy: DeployBootstrap {
                bases: vec!["http://127.0.0.1:9/base".into()],
                files: vec![
                    "deploy/installer.py".into(),
                    "deploy/git.py".into(),
                    "config/deploy.template.yaml".into(),
                    "config/deploy.template-cn.yaml".into(),
                    "config/deploy.template-linux.yaml".into(),
                    "config/deploy.template-linux-cn.yaml".into(),
                ],
            },
        }
    }
    fn apply(root: &Path, manifest: Manifest) -> Result<()> {
        recover(root)?;
        // 组件事务测试预置全部引导文件；中断补齐由 HTTP 回归测试覆盖。
        for name in &manifest.deploy.files {
            let path = root.join(name);
            if !path.exists() {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, "# bootstrap").unwrap();
            }
        }
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
        let missing = receipt.components["adb"].files.keys().next().unwrap();
        fs::remove_file(root.path().join(missing)).unwrap();
        assert!(deployment_required(root.path()));
        // 只有 adb ZIP 留在缓存，其他源不可访问；成功说明没有重复请求完整组件。
        for component in &manifest.platforms[&platform()] {
            if component.id != "adb" {
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
        let obsolete = if cfg!(windows) {
            root.path().join(".azurpilot/tools/git/obsolete.exe")
        } else {
            root.path()
                .join(".azurpilot/tools/git/libexec/git-core/obsolete")
        };
        assert!(!obsolete.exists());
        assert!(!deployment_required(root.path()));
        assert_eq!(
            fs::read_to_string(root.path().join("custom.py")).unwrap(),
            "user"
        );
    }

    #[test]
    fn failed_download_does_not_commit_already_staged_files() {
        let root = tempfile::tempdir().unwrap();
        let first = fixture(root.path(), 1);
        apply(root.path(), first).unwrap();
        let mut next = fixture(root.path(), 2);
        let adb = next
            .platforms
            .get_mut(&platform())
            .unwrap()
            .iter_mut()
            .find(|c| c.id == "adb")
            .unwrap();
        adb.sha256 = "0".repeat(64);
        assert!(apply(root.path(), next).is_err());
        assert!(!deployment_required(root.path()));
    }

    #[test]
    fn interrupted_commit_recovers_old_and_new_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        fs::create_dir_all(path.join(".azurpilot/tools/adb")).unwrap();
        fs::create_dir_all(path.join(format!("{TRANSACTION}/backup/.azurpilot/tools/adb")))
            .unwrap();
        fs::write(path.join(".azurpilot/tools/adb/adb"), "new").unwrap();
        fs::write(
            path.join(format!("{TRANSACTION}/backup/.azurpilot/tools/adb/adb")),
            "old",
        )
        .unwrap();
        fs::write(
            path.join(format!("{TRANSACTION}/journal.json")),
            serde_json::to_vec(&vec![Change {
                path: ".azurpilot/tools/adb/adb".into(),
                existed: true,
            }])
            .unwrap(),
        )
        .unwrap();
        recover(path).unwrap();
        recover(path).unwrap();
        assert_eq!(
            fs::read_to_string(path.join(".azurpilot/tools/adb/adb")).unwrap(),
            "old"
        );
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
            false,
            &Preferences::default()
        )
        .is_err());
        assert!(!deployment_required(root.path()));
    }

    #[test]
    fn checksum_failure_uses_next_mirror() {
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
            id: "git".into(),
            sha256: hash.clone(),
            size: size as u64,
            urls: vec![
                format!("http://{address}/bad"),
                format!("http://{address}/good"),
            ],
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
    fn raw_bootstrap_download_uses_fallback_base_and_writes_file() {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            let body = b"print('installer')";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        });
        let bases = vec![
            format!("http://127.0.0.1:9/unreachable"),
            format!("http://{address}"),
        ];
        download_raw_file(
            root.path(),
            "deploy/installer.py",
            &bases.iter().map(String::as_str).collect::<Vec<_>>(),
            &client().unwrap(),
            &Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("deploy/installer.py")).unwrap(),
            "print('installer')"
        );
    }

    #[test]
    fn interrupted_bootstrap_retries_only_missing_files_even_when_installer_exists() {
        fn server(
            responses: Vec<(u16, &'static str)>,
        ) -> (String, thread::JoinHandle<Vec<String>>) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let handle = thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut requests = Vec::new();
                for (status, body) in responses {
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(
                                    std::time::Instant::now() < deadline,
                                    "bootstrap request missing"
                                );
                                thread::sleep(Duration::from_millis(10));
                            }
                            Err(error) => panic!("{error}"),
                        }
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut bytes = [0; 4096];
                    let count = stream.read(&mut bytes).unwrap();
                    requests.push(
                        String::from_utf8_lossy(&bytes[..count])
                            .lines()
                            .next()
                            .unwrap()
                            .to_owned(),
                    );
                    write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                }
                requests
            });
            (format!("http://{address}"), handle)
        }

        let root = tempfile::tempdir().unwrap();
        let mut manifest = fixture(root.path(), 1);
        fs::create_dir_all(root.path().join("deploy")).unwrap();
        fs::write(root.path().join(BOOTSTRAP_MARKER), "# already downloaded").unwrap();
        fs::create_dir_all(root.path().join("config")).unwrap();
        fs::write(root.path().join("config/user.json"), "keep").unwrap();
        let sources = Preferences {
            download_source: DownloadSource::Github,
            ..Default::default()
        };
        let (base, first) = server(vec![(200, "# git bootstrap"), (503, "unavailable")]);
        manifest.deploy.bases = vec![base];
        let attempt = apply_manifest(
            root.path(),
            &mut |_| {},
            Arc::new(AtomicBool::new(false)),
            manifest.clone(),
            None,
            &client().unwrap(),
            false,
            &sources,
        );
        assert!(attempt.is_err());
        assert_eq!(first.join().unwrap().len(), 2);
        assert!(deployment_required(root.path()));
        assert!(!root.path().join(RECEIPT).exists());

        let (base, retry) = server(vec![(200, "Deploy: {}")]);
        manifest.deploy.bases = vec![base];
        recover(root.path()).unwrap();
        apply_manifest(
            root.path(),
            &mut |_| {},
            Arc::new(AtomicBool::new(false)),
            manifest,
            None,
            &client().unwrap(),
            false,
            &sources,
        )
        .unwrap();
        let requests = retry.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains(platform_template_file(&sources)));
        assert!(!deployment_required(root.path()));
        assert_eq!(
            fs::read_to_string(root.path().join("config/user.json")).unwrap(),
            "keep"
        );
        fs::remove_file(root.path().join("deploy/git.py")).unwrap();
        assert!(deployment_required(root.path()));
    }

    #[test]
    fn platform_template_matches_system_and_cn_preference() {
        let mut overseas = Preferences::default();
        overseas.download_source = DownloadSource::Github;
        assert_eq!(
            platform_template_file(&overseas),
            if cfg!(windows) {
                "config/deploy.template.yaml"
            } else {
                "config/deploy.template-linux.yaml"
            }
        );
        let mut cn = Preferences::default();
        cn.download_source = DownloadSource::Cnb;
        assert!(platform_template_file(&cn).ends_with("-cn.yaml"));
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
            ("git", ".azurpilot/tools/adb/adb"),
            ("git", "deploy/git.py"),
            ("git", "gui.py"),
            ("adb", "gui.py"),
            ("adb", "deploy/installer.py"),
        ] {
            assert!(component_path(id, path).is_err(), "{id} {path}");
        }
        if !cfg!(windows) {
            assert!(component_path("git", ".azurpilot/tools/git/bin/git").is_ok());
            assert!(component_path(
                "git",
                ".azurpilot/tools/git/libexec/git-core/git-remote-https"
            )
            .is_ok());
        } else {
            assert!(component_path("git", ".azurpilot/tools/git/cmd/git.exe").is_ok());
            assert!(component_path("git", ".venv/Scripts/python.exe").is_err());
        }
        // 引导清单只允许 deploy 脚本与分系统模板，json 等一律拒绝。
        for name in [
            "config/alas.json",
            "config/template.json",
            "config/deploy.yaml",
            "module/webui/server.py",
            "gui.py",
        ] {
            assert!(bootstrap_path(name).is_err(), "{name}");
        }
        for name in [
            "deploy/installer.py",
            "config/deploy.template.yaml",
            "config/deploy.template-linux-cn.yaml",
        ] {
            assert!(bootstrap_path(name).is_ok(), "{name}");
        }
        let root = tempfile::tempdir().unwrap();
        let mut manifest = fixture(root.path(), 1);
        assert!(manifest.validate(&platform()).is_ok());
        manifest.schema = 1;
        assert!(manifest.validate(&platform()).is_err());
        manifest.schema = 2;
        manifest.deploy.files.push("config/alas.json".into());
        assert!(manifest.validate(&platform()).is_err());
        manifest.deploy.files.pop();
        manifest.platforms.get_mut(&platform()).unwrap().pop();
        assert!(manifest.validate(&platform()).is_err());
    }

    #[test]
    fn cancelled_body_and_path_links_cannot_modify_files() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let link =
            std::os::windows::fs::symlink_dir(outside.path(), root.path().join(".azurpilot"));
        #[cfg(unix)]
        let link = std::os::unix::fs::symlink(outside.path(), root.path().join(".azurpilot"));
        if link.is_ok() {
            assert!(secure_target(root.path(), ".azurpilot/tools/git/bin/git").is_err());
        }
        let cancel = AtomicBool::new(false);
        cancel.store(true, Ordering::SeqCst);
        assert!(ensure_not_cancelled(&cancel).is_err());
    }
}
