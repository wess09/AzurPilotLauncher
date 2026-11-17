use super::*;

fn ipv4(flags: u8) -> Vec<u8> {
    let mut packet = vec![0u8; 40];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&40u16.to_be_bytes());
    packet[9] = 6;
    packet[12..16].copy_from_slice(&[104, 18, 43, 174]);
    packet[16..20].copy_from_slice(&[192, 168, 1, 10]);
    packet[20..22].copy_from_slice(&443u16.to_be_bytes());
    packet[22..24].copy_from_slice(&55000u16.to_be_bytes());
    packet[32] = 0x50;
    packet[33] = flags;
    packet
}

#[test]
fn every_rst_flag_combination_is_matched_but_fin_and_data_are_not() {
    for flags in [0x04, 0x14, 0x05, 0xff] {
        let connection = rst_connection(&ipv4(flags)).unwrap();
        assert_eq!(connection.local, "192.168.1.10:55000".parse().unwrap());
        assert_eq!(connection.remote, "104.18.43.174:443".parse().unwrap());
        assert!(connection.is_external());
    }
    for flags in [0, 0x01, 0x10, 0x11, 0x18] {
        assert!(rst_connection(&ipv4(flags)).is_none());
    }
}

#[test]
fn ipv6_extension_headers_are_walked_and_truncated_packets_are_rejected() {
    let mut packet = vec![0u8; 68];
    packet[0] = 0x60;
    packet[4..6].copy_from_slice(&28u16.to_be_bytes());
    packet[6] = 60;
    packet[8..24].copy_from_slice(
        &"2606:4700::1111"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    packet[24..40].copy_from_slice(
        &"2001:db8::2"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    packet[40] = 6;
    packet[48..50].copy_from_slice(&443u16.to_be_bytes());
    packet[50..52].copy_from_slice(&55000u16.to_be_bytes());
    packet[60] = 0x50;
    packet[61] = 4;
    let connection = rst_connection(&packet).unwrap();
    assert_eq!(connection.remote, "[2606:4700::1111]:443".parse().unwrap());
    for length in 0..packet.len() {
        assert!(rst_connection(&packet[..length]).is_none());
    }
    packet[40] = 17;
    assert!(rst_connection(&packet).is_none());
}

#[test]
fn loopback_connections_are_outside_the_protected_scope() {
    let mut packet = ipv4(4);
    packet[12..16].copy_from_slice(&[127, 0, 0, 1]);
    assert!(!rst_connection(&packet).unwrap().is_external());
    packet[12..16].copy_from_slice(&[104, 18, 43, 174]);
    packet[16..20].copy_from_slice(&[127, 0, 0, 1]);
    assert!(!rst_connection(&packet).unwrap().is_external());
}
