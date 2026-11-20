//! Tauri 构建脚本：在编译期固化 Windows 清单与关键构建资源。
//!
//! 内嵌 Windows manifest（requireAdministrator 提权、Common-Controls 依赖）；
//! 处理 `AZURPILOT_BOOTSTRAP_UV`（内嵌 uv 引导二进制，未设置时写空占位符、
//! 运行时回退 `UV` 环境变量或 PATH 中的 uv）、
//! `AZURPILOT_LAUNCHER_MTLS_IDENTITY_PEM_B64`（mTLS 自更新证书
//! base64，未设置回退"证书.txt"）、`LAUNCHER_UPDATE_URL`（自更新源覆盖）
//! 与 `LAUNCHER_UPDATE_CN_URL`（国内优化自更新源覆盖）。
use std::{env, fs, path::PathBuf};

use base64::{prelude::BASE64_STANDARD, Engine};

/// 内嵌 launcher 自更新 mTLS 客户端证书（PEM 的 base64 编码）的环境变量名。
const MTLS_IDENTITY_ENV: &str = "AZURPILOT_LAUNCHER_MTLS_IDENTITY_PEM_B64";
/// 强制要求提供 mTLS 证书的环境变量名；设置后证书缺失即构建失败。
const REQUIRE_MTLS_ENV: &str = "REQUIRE_LAUNCHER_MTLS_IDENTITY";
/// 编译期覆盖自更新源地址的环境变量名。
const LAUNCHER_UPDATE_URL_ENV: &str = "LAUNCHER_UPDATE_URL";
/// 编译期覆盖国内优化自更新源地址的环境变量名。
const LAUNCHER_UPDATE_CN_URL_ENV: &str = "LAUNCHER_UPDATE_CN_URL";
/// 是否显式覆盖了官方入口；覆盖时自更新保持单源语义，不再优先访问国内入口。
const LAUNCHER_UPDATE_URL_CUSTOM_ENV: &str = "AZURPILOT_LAUNCHER_UPDATE_URL_CUSTOM";
/// 未显式覆盖时使用的默认自更新源地址。
const DEFAULT_LAUNCHER_UPDATE_URL: &str = "https://launcher-api.azurpilot.com/updata/stable.json";
/// 国内优化的更新检查入口：默认优先使用，失败回退官方入口。
const DEFAULT_LAUNCHER_UPDATE_CN_URL: &str =
    "https://launcher-api.cn.azurpilot.com/updata/stable.json";
/// tauri.conf.json 中窗口 UA 的版本占位符，构建时替换为启动器版本。
const UA_VERSION_PLACEHOLDER: &str = "__AZURPILOT_VERSION__";

/// 把 tauri.conf.json 中窗口 UA 的版本占位符替换为当前启动器版本。
///
/// UA 字符串只在 tauri.conf.json 定义一次，版本取自 CARGO_PKG_VERSION：
/// - 修改后的 windows 配置作为 TAURI_CONFIG 补丁写入当前进程（供
///   `tauri_build::try_build`）与 rustc 环境（供 `generate_context!` 宏），
///   确保构建期与运行期配置里的 UA 都是注入后的版本；
/// - 完整 UA 通过 `cargo:rustc-env` 暴露给 Rust 代码的 `env!` 常量。
///
/// # Panics
///
/// 当清单目录/文件缺失、JSON 解析失败、CARGO_PKG_VERSION 缺失，或配置里
/// 找不到含占位符的窗口 UA 时直接 panic，避免发布出未注入版本的构建。
fn inject_webview_user_agent() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let config_path = manifest_dir.join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", config_path.display());
    let raw = fs::read_to_string(&config_path).expect("read tauri.conf.json");
    let mut config: serde_json::Value = serde_json::from_str(&raw).expect("parse tauri.conf.json");
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");

    let mut resolved_ua: Option<String> = None;
    if let Some(windows) = config["app"]["windows"].as_array_mut() {
        for window in windows.iter_mut() {
            if let Some(ua) = window["userAgent"].as_str() {
                let replaced = ua.replace(UA_VERSION_PLACEHOLDER, &version);
                if resolved_ua.is_none() {
                    resolved_ua = Some(replaced.clone());
                }
                window["userAgent"] = serde_json::Value::String(replaced);
            }
        }
    }
    let ua = resolved_ua.expect("tauri.conf.json must contain a window userAgent with version placeholder");

    // JSON merge patch 对数组整体替换：补丁提供完整的 windows 数组。
    let patch = serde_json::json!({ "app": { "windows": config["app"]["windows"] } });
    let patch_json = patch.to_string();
    // 当前构建脚本进程：`tauri_build::try_build` 会读取并 merge 此补丁。
    env::set_var("TAURI_CONFIG", &patch_json);
    // 编译 crate 的 rustc 进程：`generate_context!` 宏经 tauri-codegen 读取。
    println!("cargo:rustc-env=TAURI_CONFIG={patch_json}");
    // Rust 代码统一通过该常量取 UA，避免重复维护字符串。
    println!("cargo:rustc-env=AZURPILOT_WEBVIEW_UA={ua}");
}

/// 执行 Tauri 构建并把编译期资源写入 `OUT_DIR`。
///
/// 资源包括 launcher 的 mTLS 客户端证书，供运行时内嵌；
/// 自更新源地址通过 `cargo:rustc-env` 注入为编译期环境变量，重编译条件
/// 也在此声明。
///
/// # Panics
///
/// 当 Tauri 构建失败、`OUT_DIR` 或
/// `CARGO_MANIFEST_DIR` 缺失、base64 解码失败、本地证书读取失败，或
/// REQUIRE_LAUNCHER_MTLS_IDENTITY 已设置但证书缺失时直接 panic。
fn main() {
    // 先注入动态 UA（TAURI_CONFIG 供 tauri-build 与 generate_context! 读取），
    // 必须在 try_build 之前完成，否则构建期配置拿不到替换后的版本。
    inject_webview_user_agent();

    // 内嵌 manifest：requireAdministrator 保证启动器始终以管理员运行，
    // 以便执行需要提权的安装与修复操作；Common-Controls 6 启用现代视觉
    // 样式，确保原生控件外观一致。
    let windows = tauri_build::WindowsAttributes::new().app_manifest(
        r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#,
    );
    let attrs = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attrs).expect("failed to run tauri build script");

    // 图标目录与相关环境变量发生变化时，必须重新执行构建脚本。
    println!("cargo:rerun-if-changed=icons/");
    println!("cargo:rerun-if-env-changed={MTLS_IDENTITY_ENV}");
    println!("cargo:rerun-if-env-changed={REQUIRE_MTLS_ENV}");
    println!("cargo:rerun-if-env-changed={LAUNCHER_UPDATE_URL_ENV}");
    println!("cargo:rerun-if-env-changed={LAUNCHER_UPDATE_CN_URL_ENV}");

    // 自更新源允许在构建时覆盖；空白值视同未设置，回落到默认地址。
    let custom_update_url = env::var(LAUNCHER_UPDATE_URL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let launcher_update_url = custom_update_url
        .clone()
        .unwrap_or_else(|| DEFAULT_LAUNCHER_UPDATE_URL.to_string());
    let launcher_update_cn_url = env::var(LAUNCHER_UPDATE_CN_URL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_LAUNCHER_UPDATE_CN_URL.to_string());
    // cargo:rustc-env 在编译期注入这些值，运行时用 env! 宏读取。
    println!("cargo:rustc-env={LAUNCHER_UPDATE_URL_ENV}={launcher_update_url}");
    println!("cargo:rustc-env={LAUNCHER_UPDATE_CN_URL_ENV}={launcher_update_cn_url}");
    println!(
        "cargo:rustc-env={LAUNCHER_UPDATE_URL_CUSTOM_ENV}={}",
        if custom_update_url.is_some() { "1" } else { "0" }
    );

    // uv 引导二进制与 mTLS 证书统一写入 OUT_DIR，供运行时按路径内嵌。
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by Cargo");
    let out_dir = PathBuf::from(out_dir);

    // 内嵌 uv 引导二进制：CI 构建经 AZURPILOT_BOOTSTRAP_UV 指定 uv 路径。
    // 本地构建通常没有现成 uv：写空占位符，运行时回退到 UV 环境变量或
    // PATH 中的 uv，保证开发流程不被阻断。
    let bootstrap_uv_target = out_dir.join("bootstrap_uv.bin");
    if let Ok(source) = env::var("AZURPILOT_BOOTSTRAP_UV") {
        if !source.trim().is_empty() {
            fs::copy(&source, &bootstrap_uv_target).expect("copy AZURPILOT_BOOTSTRAP_UV");
            println!("cargo:rerun-if-env-changed=AZURPILOT_BOOTSTRAP_UV");
            println!("cargo:rerun-if-changed={source}");
        } else {
            fs::write(&bootstrap_uv_target, []).expect("write empty bootstrap uv placeholder");
        }
    } else {
        fs::write(&bootstrap_uv_target, []).expect("write empty bootstrap uv placeholder");
        println!("cargo:warning=AZURPILOT_BOOTSTRAP_UV is not set; launcher will use PATH uv for local builds");
        println!("cargo:rerun-if-env-changed=AZURPILOT_BOOTSTRAP_UV");
    }

    let mtls_identity_target = out_dir.join("launcher_mtls_identity.pem");
    // mTLS 证书解析顺序：环境变量优先，其次仓库根目录的本地证书文件，
    // 都没有时按是否强制（REQUIRE_LAUNCHER_MTLS_IDENTITY）决定成败。
    let mtls_identity = match env::var(MTLS_IDENTITY_ENV) {
        Ok(encoded) if !encoded.trim().is_empty() => Some(
            BASE64_STANDARD
                .decode(encoded.trim().as_bytes())
                // 解码失败说明环境变量内容损坏，立即失败而非静默降级。
                .expect("decode AZURPILOT_LAUNCHER_MTLS_IDENTITY_PEM_B64"),
        ),
        _ => {
            // 未设置环境变量时回退到仓库根目录的"证书.txt"，仅供开发使用。
            let manifest_dir =
                PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
            let local_cert_path = manifest_dir.join("证书.txt");
            if local_cert_path.exists() {
                println!(
                    "cargo:warning=AZURPILOT_LAUNCHER_MTLS_IDENTITY_PEM_B64 is not set; using local {}",
                    local_cert_path.display()
                );
                println!("cargo:rerun-if-changed={}", local_cert_path.display());
                Some(fs::read(&local_cert_path).expect("read local certificate file"))
            } else {
                // 发布构建通过 REQUIRE_LAUNCHER_MTLS_IDENTITY 强制提供证书；
                // 缺失必须失败，绝不允许无证书的构建混入发布渠道。
                if env::var_os(REQUIRE_MTLS_ENV).is_some() {
                    panic!("AZURPILOT_LAUNCHER_MTLS_IDENTITY_PEM_B64 is required but not set");
                }
                println!(
                    "cargo:warning=AZURPILOT_LAUNCHER_MTLS_IDENTITY_PEM_B64 is not set; launcher update client will build without mTLS identity"
                );
                None
            }
        }
    };
    // 无论有无证书都写出文件：空文件即占位符，运行时据此禁用 mTLS 自更新。
    match mtls_identity {
        Some(bytes) => {
            fs::write(&mtls_identity_target, bytes).expect("write launcher mTLS identity")
        }
        None => {
            fs::write(&mtls_identity_target, []).expect("write empty launcher mTLS placeholder")
        }
    }
}
