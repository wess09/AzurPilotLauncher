//! Windows 后端使用允许显式脱离的 Job，避免退出时连带关闭模拟器。
//!
//! 普通 Python worker 继续继承 Job；只有 ALAS 的模拟器启动命令请求
//! CREATE_BREAKAWAY_FROM_JOB。不能使用 SILENT_BREAKAWAY，否则 worker 也会泄漏。

use std::{
    io,
    mem::size_of,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command, ExitStatus},
    ptr,
    sync::atomic::Ordering,
    thread::sleep,
    time::{Duration, Instant},
};

use winapi::um::{
    handleapi::INVALID_HANDLE_VALUE,
    jobapi2::{
        AssignProcessToJobObject, CreateJobObjectW, QueryInformationJobObject,
        SetInformationJobObject, TerminateJobObject,
    },
    processthreadsapi::{OpenThread, ResumeThread},
    tlhelp32::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    },
    winbase::{CREATE_NO_WINDOW, CREATE_SUSPENDED},
    winnt::{
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, THREAD_SUSPEND_RESUME,
    },
};

use crate::window_util::HAS_CONSOLE;

/// ALAS 只在启动器明确提供此能力时请求脱离，兼容旧启动器和直接运行 Python。
const BREAKAWAY_ENV: &str = "ALAS_LAUNCHER_JOB_BREAKAWAY";

pub(crate) struct BackendChild {
    child: Child,
    job: OwnedHandle,
}

impl BackendChild {
    pub(crate) fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child.stdout.take()
    }
    pub(crate) fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.child.stderr.take()
    }
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        let raw_job = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        if raw_job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = unsafe { OwnedHandle::from_raw_handle(raw_job as _) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        if unsafe {
            SetInformationJobObject(
                raw_job,
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }

        // 先挂起、登记到 Job，再恢复，防止 Python 在登记之前派生不受管理的 worker。
        let flags = CREATE_SUSPENDED
            | if HAS_CONSOLE.load(Ordering::Relaxed) {
                0
            } else {
                CREATE_NO_WINDOW
            };
        command.creation_flags(flags).env(BREAKAWAY_ENV, "1");
        let mut child = command.spawn()?;
        let setup = (|| {
            if unsafe { AssignProcessToJobObject(raw_job, child.as_raw_handle() as _) } == 0 {
                return Err(io::Error::last_os_error());
            }
            resume_child(child.id())
        })();
        if let Err(error) = setup {
            // 登记失败时也必须回收仍挂起的根进程；不能留下无法就绪的 Python。
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(Self { child, job })
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub(crate) fn kill(&mut self) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.job.as_raw_handle() as _, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait()?;
        // TerminateJobObject 发起终止后，根进程退出不代表最后一个 worker 已退出。
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            if unsafe {
                QueryInformationJobObject(
                    self.job.as_raw_handle() as _,
                    JobObjectBasicAccountingInformation,
                    &mut accounting as *mut _ as _,
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if accounting.ActiveProcesses == 0 {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "后端 Job 仍有工作进程未退出",
                ));
            }
            sleep(Duration::from_millis(20));
        }
    }
}

/// std::process::Child 只暴露进程句柄，恢复初始线程需通过 Toolhelp 快照定位。
fn resume_child(pid: u32) -> io::Result<()> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot as _) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut found = unsafe { Thread32First(snapshot.as_raw_handle() as _, &mut entry) };
    let mut resumed = false;
    while found != 0 {
        if entry.th32OwnerProcessID == pid {
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            let thread = unsafe { OwnedHandle::from_raw_handle(thread as _) };
            let previous_count = unsafe { ResumeThread(thread.as_raw_handle() as _) };
            if previous_count == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            // 快照可能包含注入线程；继续遍历，确保挂起的初始线程确实恢复。
            resumed |= previous_count > 0;
        }
        found = unsafe { Thread32Next(snapshot.as_raw_handle() as _, &mut entry) };
    }
    if resumed {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "找不到后端进程的挂起线程",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        process::Stdio,
        thread::sleep,
        time::{Duration, Instant},
    };
    use winapi::um::{
        processthreadsapi::{OpenProcess, TerminateProcess},
        synchapi::WaitForSingleObject,
        winbase::CREATE_BREAKAWAY_FROM_JOB,
        winnt::{PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, SYNCHRONIZE},
    };

    const FIXTURE: &str = "backend_process::tests::backend_lifetime_fixture";

    fn fixture_command(role: &str, dir: &Path) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", FIXTURE, "--nocapture"])
            .env("ALAS_BACKEND_TEST_ROLE", role)
            .env("ALAS_BACKEND_TEST_DIR", dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        command
    }

    #[test]
    fn backend_lifetime_fixture() {
        let Ok(role) = std::env::var("ALAS_BACKEND_TEST_ROLE") else {
            return;
        };
        let dir = std::path::PathBuf::from(std::env::var_os("ALAS_BACKEND_TEST_DIR").unwrap());
        if role == "backend" {
            assert_eq!(std::env::var(BREAKAWAY_ENV).unwrap(), "1");
            let _worker = fixture_command("worker", &dir)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .unwrap();
            let _emulator = fixture_command("emulator", &dir)
                .creation_flags(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB)
                .spawn()
                .unwrap();
        }
        fs::write(dir.join(&role), std::process::id().to_string()).unwrap();
        sleep(Duration::from_secs(30));
    }

    /// 所有测试进程都持有本次创建的句柄，断言失败时也不会留下伪模拟器。
    struct TestProcess(OwnedHandle);
    impl TestProcess {
        fn open(dir: &Path, role: &str) -> Self {
            let start = Instant::now();
            loop {
                if let Some(pid) = fs::read_to_string(dir.join(role))
                    .ok()
                    .and_then(|s| s.parse().ok())
                {
                    let handle = unsafe {
                        OpenProcess(
                            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE,
                            0,
                            pid,
                        )
                    };
                    assert!(!handle.is_null(), "无法打开测试进程 {role}");
                    return Self(unsafe { OwnedHandle::from_raw_handle(handle as _) });
                }
                assert!(
                    start.elapsed() < Duration::from_secs(10),
                    "测试进程 {role} 未就绪"
                );
                sleep(Duration::from_millis(20));
            }
        }
        fn exited(&self, timeout: u32) -> bool {
            unsafe { WaitForSingleObject(self.0.as_raw_handle() as _, timeout) == 0 }
        }
    }
    impl Drop for TestProcess {
        fn drop(&mut self) {
            unsafe {
                TerminateProcess(self.0.as_raw_handle() as _, 0);
                WaitForSingleObject(self.0.as_raw_handle() as _, 3000);
            }
        }
    }

    fn check_cleanup(mode: &str) {
        let dir = tempfile::tempdir().unwrap();
        let mut child = BackendChild::spawn(&mut fixture_command("backend", dir.path())).unwrap();
        let root = TestProcess::open(dir.path(), "backend");
        let worker = TestProcess::open(dir.path(), "worker");
        let emulator = TestProcess::open(dir.path(), "emulator");
        if mode == "kill" {
            child.kill().unwrap();
            child.wait().unwrap();
            // Job 回收 worker 是异步的，wait() 返回时可能还没完成最终退出。
            assert!(
                worker.exited(3000),
                "wait 返回前应完成所有普通 worker 的回收"
            );
        } else if mode == "root_exit" {
            assert_ne!(
                unsafe { TerminateProcess(root.0.as_raw_handle() as _, 0) },
                0
            );
            assert!(root.exited(3000));
            assert!(child.try_wait().unwrap().is_some());
            assert!(!worker.exited(100), "根进程退出时应保留 worker 供 Job 回收");
        }
        drop(child);
        assert!(root.exited(3000), "后端根进程未回收");
        assert!(worker.exited(3000), "普通 worker 未回收");
        assert!(!emulator.exited(100), "显式脱离的模拟器被连带关闭");
    }

    #[test]
    fn terminate_keeps_emulator_and_reaps_workers() {
        check_cleanup("kill");
    }

    #[test]
    fn drop_keeps_emulator_and_reaps_workers() {
        check_cleanup("drop");
    }

    #[test]
    fn root_exit_still_reaps_workers_without_closing_emulator() {
        check_cleanup("root_exit");
    }
}
