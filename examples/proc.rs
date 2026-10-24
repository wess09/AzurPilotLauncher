//! 示例：扫描全系统进程的环境变量，打印 AP / ALAS 两套启动器归属标记。
//!
//! 启动器启动 `gui.py` 时同时注入 `AP_LAUNCHER_PID` 和 `ALAS_LAUNCHER_PID`，
//! 用于退出时识别并回收自身派生的子进程。本示例是手动排查工具：
//! 以 `cargo run --example proc` 运行，可确认标记是否存在、
//! 残留的启动器子进程有哪些，辅助验证进程回收机制是否生效。

fn main() {
    // new_all 会一次性采集进程表与环境变量快照，足够本工具的一次性排查使用
    let sys = sysinfo::System::new_all();
    for (pid, process) in sys.processes() {
        for var in process.environ() {
            let value = var.to_str().unwrap_or_default();
            if value.starts_with("AP_LAUNCHER_PID=") || value.starts_with("ALAS_LAUNCHER_PID=") {
                println!("[{pid}] {var:?}");
            }
        }
    }
}
