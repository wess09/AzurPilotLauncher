//! 启动进度的唯一状态源：合并跨阶段/重试进度，以最新快照定频推送。

use std::time::Duration;

use rust_i18n::t;
use serde::Serialize;

pub const UPDATE_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SplashUpdate {
    pub subtitle: String,
    pub title: String,
    pub detail: String,
    /// 保留小数精度；百分比文本由页面按实际呈现值取整。
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uv_progress: Option<UvProgress>,
    pub is_error: bool,
    /// 清理/中止后的状态只能由后续清理状态替换，避免后台迟到更新覆盖。
    pub terminal: bool,
    /// 发布器分配的快照序号，页面据此拒绝乱序消息。
    pub sequence: u64,
    /// 开始新的依赖任务（正常同步/超时重建）；同任务的镜像重试不能设置。
    #[serde(skip)]
    new_uv_task: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UvProgress {
    pub progress: f64,
    pub detail: String,
    pub task_id: u64,
}

fn percentage(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

impl SplashUpdate {
    pub fn loading(
        title: impl Into<String>,
        detail: impl Into<String>,
        progress: impl Into<f64>,
    ) -> Self {
        Self {
            subtitle: t!("setup.connecting").to_string(),
            title: title.into(),
            detail: detail.into(),
            progress: percentage(progress.into()),
            uv_progress: None,
            is_error: false,
            terminal: false,
            sequence: 0,
            new_uv_task: false,
        }
    }

    pub fn error(
        title: impl Into<String>,
        detail: impl Into<String>,
        progress: impl Into<f64>,
    ) -> Self {
        let mut update = Self::loading(title, detail, progress);
        update.subtitle = t!("setup.connection_failed").to_string();
        update.is_error = true;
        update
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    pub fn with_uv_progress(mut self, progress: impl Into<f64>, detail: impl Into<String>) -> Self {
        self.uv_progress = Some(UvProgress {
            progress: percentage(progress.into()),
            detail: detail.into(),
            task_id: 0,
        });
        self
    }

    #[cfg(test)]
    pub fn with_terminal(mut self) -> Self {
        self.terminal = true;
        self
    }

    pub fn with_new_uv_task(mut self) -> Self {
        self.new_uv_task = true;
        self
    }
}

#[derive(Default)]
pub(crate) struct ProgressState {
    pub(crate) latest: Option<SplashUpdate>,
    highest_uv: f64,
    uv_task: u64,
}

impl ProgressState {
    pub(crate) fn accept(&mut self, mut update: SplashUpdate) -> bool {
        if let Some(previous) = &self.latest {
            if previous.terminal && !update.terminal {
                return false;
            }
            update.progress = percentage(update.progress).max(previous.progress);
            update.sequence = previous.sequence;
        } else {
            update.progress = percentage(update.progress);
            update.sequence = 0;
        }
        if update.new_uv_task {
            self.uv_task += 1;
            self.highest_uv = 0.0;
            update.new_uv_task = false;
        }
        if let Some(uv) = &mut update.uv_progress {
            uv.progress = percentage(uv.progress).max(self.highest_uv);
            uv.task_id = self.uv_task;
            self.highest_uv = uv.progress;
        }
        if self.latest.as_ref() == Some(&update) {
            return false;
        }
        update.sequence += 1;
        self.latest = Some(update);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_and_hidden_uv_preserve_progress() {
        let mut state = ProgressState::default();
        state.accept(
            SplashUpdate::loading("sync", "download", 85.5).with_uv_progress(80.5, "download"),
        );
        state.accept(SplashUpdate::loading("retry", "mirror", 64).with_uv_progress(2, "resolve"));
        assert_eq!(state.latest.as_ref().unwrap().progress, 85.5);
        assert_eq!(
            state
                .latest
                .as_ref()
                .unwrap()
                .uv_progress
                .as_ref()
                .unwrap()
                .progress,
            80.5
        );
        state.accept(SplashUpdate::loading("backend", "recover", 97));
        state.accept(SplashUpdate::loading("sync", "rebuild", 64).with_uv_progress(2, "resolve"));
        assert_eq!(
            state
                .latest
                .as_ref()
                .unwrap()
                .uv_progress
                .as_ref()
                .unwrap()
                .progress,
            80.5
        );
    }

    #[test]
    fn cleanup_rejects_late_work_but_allows_failure_and_retry() {
        let mut state = ProgressState::default();
        state.accept(SplashUpdate::loading("setup", "download", 30));
        state.accept(SplashUpdate::loading("cleanup", "wait", 99).with_terminal());
        assert!(!state.accept(SplashUpdate::loading("setup", "late", 100)));
        assert_eq!(state.latest.as_ref().unwrap().title, "cleanup");
        assert!(state.accept(SplashUpdate::error("cleanup failed", "retry", 99).with_terminal()));
        assert!(state.accept(SplashUpdate::loading("cleanup", "retry", 99).with_terminal()));
    }

    #[test]
    fn backend_recovery_starts_a_new_uv_task_while_main_progress_stays_monotonic() {
        let mut state = ProgressState::default();
        state.accept(
            SplashUpdate::loading("sync", "done", 90)
                .with_uv_progress(100, "done")
                .with_new_uv_task(),
        );
        state.accept(SplashUpdate::loading("backend", "timeout", 97));
        state.accept(
            SplashUpdate::loading("sync", "rebuild", 64)
                .with_uv_progress(2, "resolve")
                .with_new_uv_task(),
        );
        let snapshot = state.latest.as_ref().unwrap();
        assert_eq!(snapshot.progress, 97.0);
        let uv = snapshot.uv_progress.as_ref().unwrap();
        assert_eq!(uv.progress, 2.0);
        assert_eq!(uv.task_id, 2);
        state.accept(SplashUpdate::loading("retry", "mirror", 64).with_uv_progress(1, "resolve"));
        assert_eq!(
            state
                .latest
                .as_ref()
                .unwrap()
                .uv_progress
                .as_ref()
                .unwrap()
                .progress,
            2.0
        );
    }

    #[test]
    fn ordinary_errors_can_resume_and_duplicate_snapshots_are_skipped() {
        let mut state = ProgressState::default();
        state.accept(SplashUpdate::error("node", "failed", 7));
        let update = SplashUpdate::loading("setup", "start", 8);
        assert!(state.accept(update.clone()));
        assert!(!state.accept(update));
        assert_eq!(state.latest.as_ref().unwrap().sequence, 2);
    }

    #[test]
    fn invalid_values_are_finite_clamped_and_monotonic() {
        let mut state = ProgressState::default();
        state.accept(SplashUpdate::loading("setup", "first", 12.25));
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            state.accept(
                SplashUpdate::loading("setup", "invalid", invalid)
                    .with_uv_progress(invalid, "invalid"),
            );
            assert_eq!(state.latest.as_ref().unwrap().progress, 12.25);
            assert_eq!(
                state
                    .latest
                    .as_ref()
                    .unwrap()
                    .uv_progress
                    .as_ref()
                    .unwrap()
                    .progress,
                0.0
            );
            assert!(serde_json::to_string(state.latest.as_ref().unwrap()).is_ok());
        }
    }
}
