//! 安装任务拥有独立进程组/Job；取消与错误返回同时回收其后代进程。
use crate::window_util::CreateNoWindow;
#[cfg(unix)]
use command_group::CommandGroup;
use std::{
    io,
    process::{ChildStderr, ChildStdout, Command, ExitStatus},
};

pub(crate) struct TaskChild {
    #[cfg(windows)]
    child: crate::backend_process::BackendChild,
    #[cfg(unix)]
    child: command_group::GroupChild,
}

impl TaskChild {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        command.create_no_window();
        #[cfg(windows)]
        let child = crate::backend_process::BackendChild::spawn(command)?;
        #[cfg(unix)]
        let child = command.group().spawn()?;
        Ok(Self { child })
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        #[cfg(windows)]
        {
            self.child.take_stdout()
        }
        #[cfg(unix)]
        {
            self.child.inner().stdout.take()
        }
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        #[cfg(windows)]
        {
            self.child.take_stderr()
        }
        #[cfg(unix)]
        {
            self.child.inner().stderr.take()
        }
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }
}
impl Drop for TaskChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };
    const FIXTURE: &str = "task_process::tests::task_fixture";
    fn fixture(dir: &Path, role: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", FIXTURE, "--nocapture"])
            .env("AZURPILOT_TASK_TEST_DIR", dir)
            .env("AZURPILOT_TASK_TEST_ROLE", role)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        command
    }
    #[test]
    fn task_fixture() {
        let Some(dir) = std::env::var_os("AZURPILOT_TASK_TEST_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let role = std::env::var("AZURPILOT_TASK_TEST_ROLE").unwrap();
        if role == "root" {
            let _worker = fixture(&dir, "worker").create_no_window().spawn().unwrap();
        }
        fs::write(dir.join(role), std::process::id().to_string()).unwrap();
        thread::sleep(Duration::from_secs(30));
    }
    #[test]
    fn cancelling_an_install_task_reaps_its_worker_and_preserves_environment_files() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join(".venv")).unwrap();
        fs::write(temp.path().join(".venv/keep"), "environment").unwrap();
        let mut child = TaskChild::spawn(&mut fixture(temp.path(), "root")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !temp.path().join("worker").is_file() {
            assert!(Instant::now() < deadline, "fixture did not start");
            thread::sleep(Duration::from_millis(20));
        }
        let pid = fs::read_to_string(temp.path().join("worker"))
            .unwrap()
            .parse::<u32>()
            .unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        let system = sysinfo::System::new_all();
        if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
            assert_eq!(process.status(), sysinfo::ProcessStatus::Zombie);
        }
        assert_eq!(
            fs::read_to_string(temp.path().join(".venv/keep")).unwrap(),
            "environment"
        );
    }
}
