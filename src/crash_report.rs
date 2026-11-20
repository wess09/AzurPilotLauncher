//! 崩溃报告：后端启动失败或运行期意外退出时，从日志中提取崩溃原因，
//! 把技术性的退出码/日志文本翻译成人话摘要，并保留原始日志作为可展开详情。
//!
//! 数据由 Snapshot.crash 传给前端的独立崩溃报告窗口（?window=crash）。

use rust_i18n::t;
use serde::Serialize;

/// 传给前端的崩溃报告。
#[derive(Clone, Debug, Serialize)]
pub(crate) struct CrashReport {
    /// 退出码（deploy ErrorCodes / gui EXIT_*），无码时为 None。
    pub code: Option<i32>,
    /// 人话一句话摘要，作为弹窗主标题下的原因。
    pub summary: String,
    /// 原始日志尾部（含四段式错误块与 Traceback），作为可展开详情。
    pub detail: String,
}

/// 把 gui.py 的退出码翻译成人话摘要。
///
/// 70-78 为 gui.py 的 EXIT_*，10-13 为 deploy ErrorCodes；其余（如裸 1）
/// 走通用兜底。reason 是日志里的单行原因，拼在摘要后补充具体场景。
fn exit_code_summary(code: Option<i32>, reason: Option<&str>) -> String {
    let base = match code {
        Some(10) => t!("crash.code_command").to_string(),
        Some(11) => t!("crash.code_git_fetch").to_string(),
        Some(12) => t!("crash.code_git_merge").to_string(),
        Some(13) => t!("crash.code_deps").to_string(),
        Some(70) => t!("crash.code_startup").to_string(),
        Some(71) => t!("crash.code_worker_cleanup").to_string(),
        Some(72) => t!("crash.code_deps").to_string(),
        Some(73) => t!("crash.code_frontend").to_string(),
        Some(74) => t!("crash.code_subprocess").to_string(),
        Some(75) => t!("crash.code_port_timeout").to_string(),
        Some(76) => t!("crash.code_webui_crash").to_string(),
        Some(77) => t!("crash.code_terminate").to_string(),
        Some(78) => t!("crash.code_ipc").to_string(),
        Some(1) => t!("crash.code_unhandled").to_string(),
        _ => t!("crash.code_unknown").to_string(),
    };
    match reason.filter(|r| !r.is_empty()) {
        // 原因已是中文人话短句时，作为括号补充说明具体场景。
        Some(reason) if !base.is_empty() => format!("{base}：{reason}"),
        _ => base,
    }
}

/// 采集崩溃报告。code 来自子进程退出码；fallback 为调用方已有的错误文本
/// （如 backend_exit_error 生成的消息），在日志不可用时使用。
pub(crate) fn collect(code: Option<i32>, fallback: &str) -> CrashReport {
    let (reason, detail) = crate::backend::read_backend_failure_detail();
    let summary = exit_code_summary(code, reason.as_deref());
    // 日志读不到时，摘要退化为通用文案 + 调用方文本的第一行，避免暴露长堆栈。
    let summary = if reason.is_none() && summary.is_empty() {
        let first_line = fallback.lines().next().unwrap_or("").trim();
        if first_line.is_empty() {
            t!("crash.generic").to_string()
        } else {
            format!("{}：{}", t!("crash.generic"), first_line)
        }
    } else {
        summary
    };
    let detail = if detail.is_empty() {
        fallback.to_owned()
    } else {
        detail
    };
    CrashReport {
        code,
        summary,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_summary_maps_webui_codes() {
        // 各退出码都应得到非空人话摘要，且不泄漏原始码值。
        for code in [10, 11, 12, 13, 70, 71, 72, 73, 74, 75, 76, 77, 78, 1, 99, 0] {
            let text = exit_code_summary(Some(code), None);
            assert!(!text.is_empty(), "code {code} produced empty summary");
            assert!(!text.contains(&code.to_string()), "code leaked in summary: {text}");
        }
    }

    #[test]
    fn exit_code_summary_appends_reason() {
        let text = exit_code_summary(Some(73), Some("React 前端构建失败"));
        assert!(text.contains("React 前端构建失败"));
    }

    #[test]
    fn exit_code_summary_handles_no_code() {
        assert!(!exit_code_summary(None, None).is_empty());
    }
}
