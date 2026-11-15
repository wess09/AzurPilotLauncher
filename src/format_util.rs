//! 通用数据体积与速率格式化工具

/// 把字节数格式化为人类可读体积（自动选择 B/KiB/MiB/GiB 单位，一位小数）。
pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.1} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.1} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// 计算平均下载速度（字节/秒）；耗时不足 0.1 秒时按 0.1 秒折算，避免除零。
pub fn download_speed_bytes_per_second(downloaded: u64, started_at: std::time::Instant) -> f64 {
    let elapsed = started_at.elapsed().as_secs_f64().max(0.1);
    downloaded as f64 / elapsed
}

/// 把字节每秒的速率格式化为人类可读体积速率（负数按 0 处理）。
pub fn format_speed(bytes_per_second: f64) -> String {
    format_bytes(bytes_per_second.max(0.0).round() as u64)
}
