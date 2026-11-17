use super::{rst_connection, Connection};
use anyhow::{bail, Context, Result};
use libloading::os::windows::{
    Library, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_char, c_void},
    fs, io, mem,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::Path,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
};

const DLL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rst-WinDivert.dll"));
const DRIVER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rst-WinDivert64.sys"));
const LICENSE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rst-LICENSE"));
const FILTER: &[u8] = b"inbound and tcp.Rst and !loopback\0";
const ERROR_NO_DATA: i32 = 232;
static GUARD: OnceLock<std::result::Result<Mutex<Option<Guard>>, String>> = OnceLock::new();

#[repr(C)]
#[derive(Clone, Copy)]
struct Address {
    timestamp: i64,
    flags: u32,
    reserved: u32,
    data: [u8; 64],
}
impl Default for Address {
    fn default() -> Self {
        Self {
            timestamp: 0,
            flags: 0,
            reserved: 0,
            data: [0; 64],
        }
    }
}

type Open = unsafe extern "system" fn(*const c_char, u32, i16, u64) -> isize;
type Recv = unsafe extern "system" fn(isize, *mut c_void, u32, *mut u32, *mut Address) -> i32;
type Send = unsafe extern "system" fn(isize, *const c_void, u32, *mut u32, *const Address) -> i32;
type Shutdown = unsafe extern "system" fn(isize, u32) -> i32;
type Close = unsafe extern "system" fn(isize) -> i32;

struct Api {
    _library: Library,
    open: Open,
    recv: Recv,
    send: Send,
    shutdown: Shutdown,
    close: Close,
}

impl Api {
    fn load(directory: &Path) -> Result<Arc<Self>> {
        // 绝对路径加载且只从组件目录及 System32 搜索依赖，避免 PATH 劫持。
        unsafe {
            let library = Library::load_with_flags(
                directory.join("WinDivert.dll"),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )?;
            Ok(Arc::new(Self {
                open: *library.get(b"WinDivertOpen\0")?,
                recv: *library.get(b"WinDivertRecv\0")?,
                send: *library.get(b"WinDivertSend\0")?,
                shutdown: *library.get(b"WinDivertShutdown\0")?,
                close: *library.get(b"WinDivertClose\0")?,
                _library: library,
            }))
        }
    }
}

#[cfg(not(test))]
pub(super) fn ensure_started() -> Result<()> {
    if DLL.is_empty() {
        static WARNING: std::sync::Once = std::sync::Once::new();
        WARNING.call_once(|| tracing::warn!("RST filter is not embedded in this development build; prepare WinDivert before building"));
        return Ok(());
    }
    match GUARD.get_or_init(|| {
        Guard::start()
            .map(|guard| Mutex::new(Some(guard)))
            .map_err(|error| format!("{error:#}"))
    }) {
        Ok(guard) => {
            let guard = guard.lock().unwrap_or_else(|error| error.into_inner());
            if guard
                .as_ref()
                .is_some_and(|guard| guard.active.load(Ordering::SeqCst))
            {
                Ok(())
            } else {
                bail!(rust_i18n::t!(
                    "network.rst_filter_failed",
                    error = "filter stopped"
                ))
            }
        }
        Err(error) => bail!(rust_i18n::t!("network.rst_filter_failed", error = error)),
    }
}

pub(super) fn shutdown() {
    if let Some(Ok(guard)) = GUARD.get() {
        drop(
            guard
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take(),
        );
    }
}

struct Guard {
    api: Arc<Api>,
    handle: isize,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    closed: Arc<Mutex<bool>>,
    dropped: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}

impl Guard {
    fn start() -> Result<Self> {
        let executable = std::env::current_exe()?;
        let parent = executable.parent().context("launcher directory")?;
        let directory =
            crate::components::secure_target(parent, ".azurpilot/tools/windivert-2.2.2-A")?;
        prepare_files(&directory)?;
        let api = Api::load(&directory)?;
        let handle = unsafe { (api.open)(FILTER.as_ptr().cast(), 0, 0, 0) };
        if handle == -1 {
            return Err(io::Error::last_os_error()).context("open process-scoped RST interception");
        }
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(true));
        let closed = Arc::new(Mutex::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let (worker_api, worker_stop, worker_active, worker_dropped) =
            (api.clone(), stop.clone(), active.clone(), dropped.clone());
        let worker_closed = closed.clone();
        let pid = std::process::id();
        let worker = thread::Builder::new()
            .name("launcher-rst-filter".into())
            .spawn(move || {
                let mut buffer = vec![0u8; 65575];
                let mut logged_lookup_error = false;
                loop {
                    let mut address = Address::default();
                    let mut received = 0u32;
                    let ok = unsafe {
                        (worker_api.recv)(
                            handle,
                            buffer.as_mut_ptr().cast(),
                            buffer.len() as u32,
                            &mut received,
                            &mut address,
                        )
                    };
                    if ok == 0 {
                        let error = io::Error::last_os_error();
                        if error.raw_os_error() != Some(ERROR_NO_DATA) {
                            tracing::error!("RST receive failed: {error}");
                        }
                        break;
                    }
                    let packet = &buffer[..received as usize];
                    let owned = if worker_stop.load(Ordering::SeqCst) {
                        false
                    } else {
                        match rst_connection(packet)
                            .filter(Connection::is_external)
                            .map(|connection| owns_connection(connection, pid))
                        {
                            Some(Ok(owned)) => owned,
                            Some(Err(error)) => {
                                // 不能确认归属时必须放行，避免影响其他程序。
                                if !logged_lookup_error {
                                    tracing::error!(
                                        "Cannot determine RST socket ownership: {error:#}"
                                    );
                                    logged_lookup_error = true;
                                }
                                false
                            }
                            None => false,
                        }
                    };
                    if owned {
                        let total = worker_dropped.fetch_add(1, Ordering::Relaxed) + 1;
                        if total == 1 || total.is_power_of_two() {
                            tracing::info!(
                                total,
                                "Dropped inbound TCP RST for launcher connection"
                            );
                        }
                        continue;
                    }
                    // NETWORK 层无 PID 字段：实时查所有者后，仅丢弃本 PID 的包。
                    // 其他进程报文包括原始元数据一起注回，不能直接使用全局 DROP 标志。
                    if unsafe {
                        (worker_api.send)(
                            handle,
                            packet.as_ptr().cast(),
                            received,
                            ptr::null_mut(),
                            &address,
                        )
                    } == 0
                    {
                        tracing::error!("RST pass-through failed: {}", io::Error::last_os_error());
                        break;
                    }
                }
                let mut closed = worker_closed
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if !*closed {
                    unsafe {
                        (worker_api.close)(handle);
                    }
                    *closed = true;
                }
                worker_active.store(false, Ordering::SeqCst);
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                unsafe {
                    (api.close)(handle);
                }
                return Err(error.into());
            }
        };
        tracing::info!("Blocking all inbound TCP RST for this launcher's Internet connections");
        Ok(Self {
            api,
            handle,
            stop,
            active,
            closed,
            dropped,
            worker: Some(worker),
        })
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // 停止捕获新包，排空队列时全部放行，再关闭本进程过滤句柄。
        let mut closed = self
            .closed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !*closed && unsafe { (self.api.shutdown)(self.handle, 1) == 0 } {
            unsafe {
                (self.api.close)(self.handle);
            }
            *closed = true;
        }
        drop(closed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let mut closed = self
            .closed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !*closed {
            unsafe {
                (self.api.close)(self.handle);
            }
            *closed = true;
        }
        tracing::info!(
            dropped = self.dropped.load(Ordering::Relaxed),
            "Stopped launcher RST filter"
        );
    }
}

fn prepare_files(directory: &Path) -> Result<()> {
    crate::components::secure_target(directory, "SOURCE.txt")?;
    if DLL.is_empty() || DRIVER.is_empty() {
        bail!("WinDivert is not embedded");
    }
    fs::create_dir_all(directory)?;
    for (name, bytes) in [
        ("WinDivert.dll", DLL),
        ("WinDivert64.sys", DRIVER),
        ("LICENSE", LICENSE),
    ] {
        let path = crate::components::secure_target(directory, name)?;
        let expected = Sha256::digest(bytes);
        if fs::read(&path).is_ok_and(|existing| Sha256::digest(existing) == expected) {
            continue;
        }
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        use std::io::Write;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path).map_err(|error| error.error)?;
    }
    fs::write(directory.join("SOURCE.txt"), "WinDivert 2.2.2-A, unmodified official distribution\nhttps://github.com/basil00/WinDivert/tree/v2.2.2\nLicense: LGPLv3 (see LICENSE)\n")?;
    Ok(())
}

#[link(name = "iphlpapi")]
extern "system" {
    fn GetExtendedTcpTable(
        table: *mut c_void,
        size: *mut u32,
        sorted: i32,
        family: u32,
        class: u32,
        reserved: u32,
    ) -> u32;
}

fn owns_connection(connection: Connection, pid: u32) -> Result<bool> {
    if owns_connection_in_table(connection, pid, connection.local.is_ipv6())? {
        return Ok(true);
    }
    if connection.local.is_ipv4() {
        return owns_connection_in_table(connection, pid, true);
    }
    Ok(false)
}

fn owns_connection_in_table(connection: Connection, pid: u32, ipv6: bool) -> Result<bool> {
    let family = if ipv6 { 23 } else { 2 };
    let row_size = if ipv6 { 56 } else { 24 };
    let mut size = 0u32;
    let status = unsafe { GetExtendedTcpTable(ptr::null_mut(), &mut size, 0, family, 5, 0) };
    if status != 122 && status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32).into());
    }
    for _ in 0..3 {
        if !(4..=16_777_216).contains(&size) {
            bail!("Invalid TCP ownership table size");
        }
        // u64 保证原生表地址对齐，读取字段仍使用显式字节偏移避免未对齐引用。
        let mut words = vec![0u64; (size as usize).div_ceil(8)];
        let capacity = words.len() * mem::size_of::<u64>();
        let status =
            unsafe { GetExtendedTcpTable(words.as_mut_ptr().cast(), &mut size, 0, family, 5, 0) };
        if status == 122 {
            continue;
        }
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32).into());
        }
        if size as usize > capacity {
            bail!("TCP ownership table overflow");
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), size as usize) };
        let count = u32_at(bytes, 0)? as usize;
        if count > (bytes.len().saturating_sub(4)) / row_size {
            bail!("Truncated TCP ownership table");
        }
        for row in bytes[4..].chunks_exact(row_size).take(count) {
            if let Some(candidate) = owned_row(row, ipv6, pid)? {
                if candidate == connection {
                    return Ok(true);
                }
            }
        }
        return Ok(false);
    }
    bail!("TCP ownership table changed repeatedly")
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_ne_bytes(
        bytes
            .get(offset..offset + 4)
            .context("TCP ownership field")?
            .try_into()?,
    ))
}

fn owned_row(row: &[u8], ipv6: bool, pid: u32) -> Result<Option<Connection>> {
    if row.len() != if ipv6 { 56 } else { 24 } {
        bail!("Truncated TCP owner row");
    }
    let (local, local_port, remote, remote_port, owner) = if ipv6 {
        (
            IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&row[..16])?)),
            20,
            IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&row[24..40])?)),
            44,
            52,
        )
    } else {
        (
            IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(&row[4..8])?)),
            8,
            IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(&row[12..16])?)),
            16,
            20,
        )
    };
    if u32_at(row, owner)? != pid {
        return Ok(None);
    }
    let normalize = |address: IpAddr| match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map_or(IpAddr::V6(address), IpAddr::V4),
        address => address,
    };
    Ok(Some(Connection {
        local: SocketAddr::new(
            normalize(local),
            u16::from_be(u32_at(row, local_port)? as u16),
        ),
        remote: SocketAddr::new(
            normalize(remote),
            u16::from_be(u32_at(row, remote_port)? as u16),
        ),
    }))
}

#[cfg(test)]
mod tests;
