//! 在 TCP 栈处理之前丢弃本进程互联网连接的所有入站 RST。
//! Windows x64 使用内嵌的官方签名 WinDivert；不根据 TTL/IP ID 猜测真假。
//! 其他进程的 RST 原样放行，HTTP 超时、取消和 FIN 不受影响。

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

#[cfg(all(windows, target_arch = "x86_64", not(test)))]
pub(crate) fn ensure_started() -> anyhow::Result<()> {
    windows::ensure_started()
}

#[cfg(not(all(windows, target_arch = "x86_64", not(test))))]
pub(crate) fn ensure_started() -> anyhow::Result<()> {
    // 普通测试不加载系统驱动；真实驱动验证有独立的管理员集成测试。
    // WinDivert 官方 2.2.2 不提供 ARM64/macOS/Linux 驱动。
    #[cfg(not(test))]
    {
        static WARNING: std::sync::Once = std::sync::Once::new();
        WARNING.call_once(|| tracing::warn!("Packet-level RST interception is only available on Windows x64; HTTP/3 and TLS record fragmentation remain available"));
    }
    Ok(())
}

pub(crate) fn shutdown() {
    #[cfg(all(windows, target_arch = "x86_64"))]
    windows::shutdown();
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Connection {
    local: std::net::SocketAddr,
    remote: std::net::SocketAddr,
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
impl Connection {
    fn is_external(&self) -> bool {
        !self.local.ip().is_loopback()
            && !self.remote.ip().is_loopback()
            && !self.remote.ip().is_unspecified()
            && self.remote.port() != 0
    }
}

/// WinDivert NETWORK 层从 IP 头开始；返回入站 RST 的连接四元组。
#[cfg(any(test, all(windows, target_arch = "x86_64")))]
fn rst_connection(packet: &[u8]) -> Option<Connection> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    let (source, destination, offset) = match packet.first()? >> 4 {
        4 => {
            if packet.len() < 20 || packet[9] != 6 {
                return None;
            }
            let offset = usize::from(packet[0] & 15) * 4;
            let total = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
            if offset < 20
                || total > packet.len()
                || total < offset + 20
                || u16::from_be_bytes([packet[6], packet[7]]) & 0x1fff != 0
            {
                return None;
            }
            (
                IpAddr::V4(Ipv4Addr::new(
                    packet[12], packet[13], packet[14], packet[15],
                )),
                IpAddr::V4(Ipv4Addr::new(
                    packet[16], packet[17], packet[18], packet[19],
                )),
                offset,
            )
        }
        6 => {
            if packet.len() < 40 {
                return None;
            }
            let total = 40 + usize::from(u16::from_be_bytes([packet[4], packet[5]]));
            if total > packet.len() {
                return None;
            }
            let mut next = packet[6];
            let mut offset = 40usize;
            while next != 6 {
                if next == 6 {
                    break;
                }
                let header = packet.get(offset..offset.checked_add(8)?)?;
                let length = match next {
                    0 | 43 | 60 => (usize::from(header[1]) + 1) * 8,
                    51 => (usize::from(header[1]) + 2) * 4,
                    44 if u16::from_be_bytes([header[2], header[3]]) & 0xfff8 == 0 => 8,
                    _ => return None,
                };
                next = header[0];
                offset = offset.checked_add(length)?;
                if offset > total {
                    return None;
                }
            }
            if next != 6 || offset + 20 > total {
                return None;
            }
            let source = Ipv6Addr::from(<[u8; 16]>::try_from(&packet[8..24]).ok()?);
            let destination = Ipv6Addr::from(<[u8; 16]>::try_from(&packet[24..40]).ok()?);
            (
                source
                    .to_ipv4_mapped()
                    .map_or(IpAddr::V6(source), IpAddr::V4),
                destination
                    .to_ipv4_mapped()
                    .map_or(IpAddr::V6(destination), IpAddr::V4),
                offset,
            )
        }
        _ => return None,
    };
    let tcp = packet.get(offset..offset + 20)?;
    if tcp[13] & 0x04 == 0 || tcp[12] >> 4 < 5 {
        return None;
    }
    Some(Connection {
        local: SocketAddr::new(destination, u16::from_be_bytes([tcp[2], tcp[3]])),
        remote: SocketAddr::new(source, u16::from_be_bytes([tcp[0], tcp[1]])),
    })
}

#[cfg(test)]
mod tests;
