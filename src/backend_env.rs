//! Python 后端环境协议：AP 为新名称，ALAS 别名兼容现有后端。

use std::{ffi::OsString, process::Command};

pub(crate) const LAUNCHER_PID_ENVS: [&str; 2] = ["AP_LAUNCHER_PID", "ALAS_LAUNCHER_PID"];
const TRUST_SECRET_ENVS: [&str; 2] = ["AP_WEBUI_TRUST_SECRET", "ALAS_WEBUI_TRUST_SECRET"];
#[cfg(windows)]
pub(crate) const BREAKAWAY_ENVS: [&str; 2] =
    ["AP_LAUNCHER_JOB_BREAKAWAY", "ALAS_LAUNCHER_JOB_BREAKAWAY"];

/// 仅给后端命令注入双名称，不污染启动器及其他安装任务的环境。
pub(crate) fn configure(command: &mut Command, launcher_pid: u32, secret: &str) {
    for key in LAUNCHER_PID_ENVS {
        command.env(key, launcher_pid.to_string());
    }
    for key in TRUST_SECRET_ENVS {
        command.env(key, secret);
    }
}

/// 后端为独立模拟器清除任一版本的归属标记后，均不再视为托管子进程。
/// 必须同时匹配两个标记，避免旧后端只清除 ALAS 时误杀仍继承 AP 的模拟器。
pub(crate) fn is_managed_child(environment: &[OsString], launcher_pid: u32) -> bool {
    LAUNCHER_PID_ENVS.iter().all(|key| {
        let marker = OsString::from(format!("{key}={launcher_pid}"));
        environment.contains(&marker)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_receives_matching_current_and_legacy_values() {
        let mut command = Command::new("python");
        configure(&mut command, 123, "test-secret");
        let environment: Vec<OsString> = command
            .get_envs()
            .map(|(key, value)| {
                let mut entry = key.to_os_string();
                entry.push("=");
                entry.push(value.unwrap());
                entry
            })
            .collect();

        assert!(is_managed_child(&environment, 123));
        assert!(!is_managed_child(&environment, 456));
        for key in TRUST_SECRET_ENVS {
            assert!(environment.contains(&OsString::from(format!("{key}=test-secret"))));
        }
    }

    #[test]
    fn detached_emulator_is_excluded_when_either_alias_is_removed() {
        let markers = LAUNCHER_PID_ENVS.map(|key| OsString::from(format!("{key}=123")));
        assert!(is_managed_child(&markers, 123));
        assert!(!is_managed_child(&markers[..1], 123));
        assert!(!is_managed_child(&markers[1..], 123));
        assert!(!is_managed_child(&[], 123));
    }
}
