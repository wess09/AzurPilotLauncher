//! 国际化模块：确定界面语言并设置全局 locale。
//!
//! 语言选择的优先级为：命令行 `--lang` / `--locale` 参数最高，
//! 其次按系统 locale 自动检测，均未命中时回退到 `en`。
//! 翻译文件位于 `locales/*.yml`（基准语言为 `zh-CN`），代码中
//! 通过 `t!("module.key")` 宏取词；本模块在应用启动最早阶段调用
//! [`init`] 完成设置，之后所有模块共享同一 locale。

const LOCALE_OVERRIDE_ARGS: &[&str] = &["--lang", "--locale", "/lang", "/locale"];

/// 初始化 i18n：检测系统语言（或启动参数覆盖），设置全局 locale。
///
/// 优先解析启动参数中的语言覆盖值；未指定时回退到系统 locale
/// 自动检测。结果写入 rust-i18n 的全局 locale，并记录日志便于排查。
pub fn init() {
    let locale = locale_from_args().unwrap_or_else(detect_locale);
    rust_i18n::set_locale(&locale);
    tracing::info!("i18n locale set to: {}", locale);
}

/// 应用设置里的语言覆盖（None 表示跟随系统检测）。
///
/// 供设置模块在用户切换界面语言、以及启动时恢复已保存语言时调用，
/// 保证 Rust 侧消息与前端界面语言一致。
pub fn apply_override(language: Option<&str>) {
    let locale = language
        .map(normalize_locale)
        .unwrap_or_else(detect_locale);
    rust_i18n::set_locale(&locale);
    tracing::info!("i18n locale overridden to: {}", locale);
}

/// 检查启动参数中是否有 --lang / --locale 覆盖。
///
/// 同时支持 `--lang <值>`（空格分隔）与 `--lang=<值>`（等号连接）
/// 两种写法；Windows 下 `/lang` 前缀同样有效。返回标准化后的
/// locale 字符串，未找到覆盖参数时返回 `None`。
fn locale_from_args() -> Option<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for (i, arg) in args.iter().enumerate() {
        let lower = arg.to_ascii_lowercase();
        if LOCALE_OVERRIDE_ARGS.iter().any(|flag| lower == *flag) {
            if let Some(value) = args.get(i + 1) {
                return Some(normalize_locale(value));
            }
        }
        // 支持 --lang=zh-CN 形式
        if let Some(value) = lower.strip_prefix("--lang=") {
            return Some(normalize_locale(value));
        }
        if let Some(value) = lower.strip_prefix("--locale=") {
            return Some(normalize_locale(value));
        }
    }
    None
}

/// 将用户输入的语言代码标准化为支持的 locale。
///
/// 支持的取值：`zh-CN`（含所有 `zh*` 前缀，除非明确指向繁体）、
/// `zh-TW`（含 `tw` / `hk` / `hant` / `zht` 变体）、`ja`（含 `jp`）、
/// `en`（含 `us`）。无法识别的输入告警后回退 `en`，保证界面
/// 永远有可用的语言。
fn normalize_locale(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("zh") {
        if lower.contains("tw")
            || lower.contains("hk")
            || lower.contains("hant")
            || lower == "zh-tw"
            || lower == "zht"
        {
            "zh-TW".to_string()
        } else {
            "zh-CN".to_string()
        }
    } else if lower.starts_with("ja") || lower == "jp" {
        "ja".to_string()
    } else if lower.starts_with("en") || lower == "us" {
        "en".to_string()
    } else {
        // 无法识别的语言回退到英语
        tracing::warn!("Unknown locale '{}', falling back to 'en'", input);
        "en".to_string()
    }
}

/// 检测系统语言，返回对应的 locale 字符串。
///
/// 通过 sys-locale crate 读取系统 locale（如 `zh_CN.UTF-8`、
/// `ja-JP`），映射规则与 [`normalize_locale`] 一致；读取失败时
/// 视为 `en`。
fn detect_locale() -> String {
    let sys_locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());

    let lang = sys_locale.to_lowercase();
    if lang.starts_with("zh") {
        if lang.contains("tw") || lang.contains("hk") || lang.contains("hant") {
            "zh-TW".to_string()
        } else {
            "zh-CN".to_string()
        }
    } else if lang.starts_with("ja") {
        "ja".to_string()
    } else {
        "en".to_string()
    }
}
