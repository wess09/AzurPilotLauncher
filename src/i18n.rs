//! 国际化模块：确定界面语言并设置全局 locale。
//!
//! 语言选择按系统 locale 自动检测，未命中时回退到 `en`；用户在设置中
//! 保存的语言偏好由 [`apply_override`] 覆盖。翻译文件位于 `locales/*.yml`
//! （基准语言为 `zh-CN`），代码中通过 `t!("module.key")` 宏取词；本模块在
//! 应用启动最早阶段调用 [`init`] 完成设置，之后所有模块共享同一 locale。

/// 初始化 i18n：检测系统语言并设置全局 locale。
///
/// 结果写入 rust-i18n 的全局 locale，并记录日志便于排查。
pub fn init() {
    let locale = detect_locale();
    rust_i18n::set_locale(&locale);
    tracing::info!("i18n locale set to: {}", locale);
}

/// 应用设置里的语言覆盖（None 表示跟随系统检测）。
///
/// 供设置模块在用户切换界面语言、以及启动时恢复已保存语言时调用，
/// 保证 Rust 侧消息与前端界面语言一致。
pub fn apply_override(language: Option<&str>) {
    let locale = language.map(normalize_locale).unwrap_or_else(detect_locale);
    rust_i18n::set_locale(&locale);
    tracing::info!("i18n locale overridden to: {}", locale);
}

/// 将用户输入的语言代码标准化为支持的 locale。
///
/// 支持的取值：`zh-CN`（含所有 `zh*` 前缀，除非明确指向繁体）、
/// `zh-TW`（含 `tw` / `hk` / `hant` / `zht` 变体）、`ja`（含 `jp`）、
/// `en`（含 `us`）。无法识别的输入告警后回退 `en`，保证界面
/// 永远有可用的语言。
fn normalize_locale(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    if lower == "zh-neko" || lower.contains("neko") {
        "zh-neko".to_string()
    } else if lower.starts_with("zh") {
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
    tracing::info!("Detected system locale: {}", sys_locale);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_locale() {
        assert_eq!(normalize_locale("zh-neko"), "zh-neko");
        assert_eq!(normalize_locale("neko"), "zh-neko");
        assert_eq!(normalize_locale("ZH-NEKO"), "zh-neko");
        assert_eq!(normalize_locale("zh-CN"), "zh-CN");
        assert_eq!(normalize_locale("zh-TW"), "zh-TW");
        assert_eq!(normalize_locale("ja"), "ja");
        assert_eq!(normalize_locale("en"), "en");
        assert_eq!(normalize_locale("unknown"), "en");
    }
}
