//! 电源管理：响应设置中的“阻止计算机自动睡眠”持有/释放系统电源锁。
//!
//! 三平台实现统一暴露 [`set_prevent_sleep`]：
//! - Windows：常驻工作线程调用 `SetThreadExecutionState`（仅作用于调用
//!   线程，故必须由专用线程长期持有），进程退出后自动失效；
//! - macOS：`IOPMAssertionCreateWithName` 持有 `PreventUserIdleSystemSleep`；
//! - Linux：启动 `systemd-inhibit` 常驻子进程并设置 `PR_SET_PDEATHSIG`
//!   随宿主退出，避免残留孤儿进程。
//!
//! 电源锁只阻止系统空闲进入睡眠，不阻止显示器关闭；背景视频等造成的
//! 显示器唤醒锁由前端“减弱动态效果”设置处理。

/// 启用或释放“阻止系统睡眠”电源锁；重复设置同一状态为幂等操作。
///
/// # Errors
/// Windows 工作线程已终止、macOS 断言创建失败或 Linux 无法启动
/// `systemd-inhibit` 时返回错误，调用方应回滚设置开关。
pub(crate) fn set_prevent_sleep(enabled: bool) -> anyhow::Result<()> {
    imp::set_prevent_sleep(enabled)
}

#[cfg(target_os = "windows")]
mod imp {
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Mutex, OnceLock};
    use winapi::um::winbase::SetThreadExecutionState;
    use winapi::um::winnt::{ES_CONTINUOUS, ES_SYSTEM_REQUIRED};

    static WORKER: OnceLock<Mutex<Option<Sender<bool>>>> = OnceLock::new();

    /// `SetThreadExecutionState` 作用于调用线程，由专用线程按指令切换
    /// `ES_SYSTEM_REQUIRED`；线程随进程退出终结，锁自动释放。
    ///
    /// # Errors
    /// 工作线程创建失败或已终止导致指令无法送达时返回错误。
    pub(crate) fn set_prevent_sleep(enabled: bool) -> anyhow::Result<()> {
        let worker = WORKER.get_or_init(|| Mutex::new(None));
        let mut guard = worker.lock().unwrap();
        if guard.is_none() {
            let (sender, receiver) = mpsc::channel::<bool>();
            std::thread::Builder::new()
                .name("prevent-sleep".to_string())
                .spawn(move || {
                    while let Ok(enable) = receiver.recv() {
                        let flags =
                            ES_CONTINUOUS | if enable { ES_SYSTEM_REQUIRED } else { 0 };
                        // 返回 0 表示失败；仅记录，后续切换会重新下发同一状态。
                        if unsafe { SetThreadExecutionState(flags) } == 0 {
                            tracing::warn!("SetThreadExecutionState failed");
                        }
                    }
                })?;
            *guard = Some(sender);
        }
        guard.as_ref().expect("worker sender present").send(enabled)?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_char;
    use std::ptr;
    use std::sync::{Mutex, OnceLock};

    /// 断言 ID；`None` 表示当前未持有电源锁。
    static ASSERTION: OnceLock<Mutex<Option<u32>>> = OnceLock::new();

    #[repr(C)]
    struct CFString {
        _opaque: [u8; 0],
    }
    type CFStringRef = *const CFString;
    const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    /// `kIOPMAssertionLevelOn`。
    const ASSERTION_LEVEL_ON: u32 = 255;
    /// `kIOReturnSuccess`。
    const KERN_SUCCESS: i32 = 0;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const core::ffi::c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(cf: *const core::ffi::c_void);
    }
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            level: u32,
            assertion_name: CFStringRef,
            assertion_id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: u32) -> i32;
    }

    /// 以 `PreventUserIdleSystemSleep` 断言阻止系统空闲睡眠。
    ///
    /// # Errors
    /// IOPMAssertion 创建失败（返回非 0 的 IOReturn）时返回错误。
    pub(crate) fn set_prevent_sleep(enabled: bool) -> anyhow::Result<()> {
        let assertion = ASSERTION.get_or_init(|| Mutex::new(None));
        let mut guard = assertion.lock().unwrap();
        if enabled {
            if guard.is_some() {
                return Ok(());
            }
            let create = |c_str: &'static [u8]| CFStringCreateWithCString(
                ptr::null(),
                c_str.as_ptr().cast(),
                KCF_STRING_ENCODING_UTF8,
            );
            // SAFETY：传入的 CFString 均由本函数创建且非空；调用后立即释放，
            // 断言 ID 由 IOKit 持有，不受 CFString 释放影响。
            unsafe {
                let assertion_type = create(b"PreventUserIdleSystemSleep\0");
                let label = create(b"AzurPilot Launcher\0");
                let mut id = 0u32;
                let status =
                    IOPMAssertionCreateWithName(assertion_type, ASSERTION_LEVEL_ON, label, &mut id);
                if !assertion_type.is_null() {
                    CFRelease(assertion_type.cast());
                }
                if !label.is_null() {
                    CFRelease(label.cast());
                }
                if status == KERN_SUCCESS {
                    *guard = Some(id);
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("IOPMAssertionCreateWithName failed: {status}"))
                }
            }
        } else {
            if let Some(id) = guard.take() {
                // SAFETY：id 来自此前的 IOPMAssertionCreateWithName。
                let status = IOPMAssertionRelease(id);
                if status != KERN_SUCCESS {
                    tracing::warn!("IOPMAssertionRelease failed: {status}");
                }
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::process::{Child, Command, Stdio};
    use std::sync::{Mutex, OnceLock};

    static INHIBITOR: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

    /// 通过 `systemd-inhibit` 常驻子进程持有 sleep 抑制锁。
    ///
    /// # Errors
    /// `systemd-inhibit` 不存在或无法启动时返回错误。
    pub(crate) fn set_prevent_sleep(enabled: bool) -> anyhow::Result<()> {
        let slot = INHIBITOR.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        if enabled {
            if let Some(child) = guard.as_mut() {
                // 子进程仍在运行时幂等返回；已退出则重新拉起。
                if child.try_wait()?.is_none() {
                    return Ok(());
                }
            }
            let mut command = Command::new("systemd-inhibit");
            command
                .arg("--what").arg("sleep")
                .arg("--who").arg("AzurPilot Launcher")
                .arg("--why").arg("User requested to prevent system sleep")
                .arg("--")
                .arg("sleep").arg("infinity")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // SAFETY：pre_exec 内只调用 prctl，不触碰分配器，fork 后单线程安全。
            unsafe {
                command.pre_exec(|| {
                    // 宿主退出后让子进程随内核清理，避免残留 sleep 进程。
                    nix::sys::prctl::set_pdeathsig(nix::sys::signal::Signal::SIGKILL)
                        .map_err(std::io::Error::from)?;
                    Ok(())
                });
            }
            *guard = Some(command.spawn()?);
        } else if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// 往返切换不报错；结束时回到关闭态，不长时间占用构建机电源锁。
    #[test]
    fn prevent_sleep_toggles_round_trip() {
        super::set_prevent_sleep(true).expect("enable prevent sleep");
        super::set_prevent_sleep(true).expect("idempotent enable");
        super::set_prevent_sleep(false).expect("disable prevent sleep");
        super::set_prevent_sleep(false).expect("idempotent disable");
    }
}
