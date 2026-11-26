//! Cliente ligero STUN (RFC 5389) y constantes de UDP hole punching P2P.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

/// Paquete corto enviado durante el UDP hole punching simultaneo entre pares.
pub const P2P_PUNCH_MAGIC: &[u8] = b"RDPUNCH\0";

const STUN_BINDING_REQUEST: u16 = 0x0001;
const STUN_BINDING_SUCCESS: u16 = 0x0101;
const STUN_MAGIC_COOKIE: u32 = 0x2112_A442;
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// Servidores STUN publicos para descubrir IP:puerto externo del socket UDP.
pub const DEFAULT_STUN_SERVERS: &[&str] = &[
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun.cloudflare.com:3478",
];

/// Construye un mensaje STUN Binding Request de 20 bytes con magic cookie y TxID.
pub fn build_binding_request(tx_id: [u8; 12]) -> [u8; 20] {
    let mut pkt = [0u8; 20];
    pkt[0..2].copy_from_slice(&STUN_BINDING_REQUEST.to_be_bytes());
    pkt[2..4].copy_from_slice(&0u16.to_be_bytes());
    pkt[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
    pkt[8..20].copy_from_slice(&tx_id);
    pkt
}

/// Parsea un STUN Binding Success Response validando el `tx_id` esperado.
pub fn parse_binding_response(buf: &[u8], expected_tx: &[u8; 12]) -> Option<SocketAddr> {
    if buf.len() < 20 {
        return None;
    }
    let msg_type = u16::from_be_bytes([buf[0], buf[1]]);
    let msg_len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    let cookie = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if msg_type != STUN_BINDING_SUCCESS
        || cookie != STUN_MAGIC_COOKIE
        || &buf[8..20] != expected_tx
        || buf.len() < 20 + msg_len
    {
        return None;
    }

    let mut fallback: Option<SocketAddr> = None;
    let mut off = 20;
    let end = 20 + msg_len;
    while off + 4 <= end {
        let attr_type = u16::from_be_bytes([buf[off], buf[off + 1]]);
        let attr_len = u16::from_be_bytes([buf[off + 2], buf[off + 3]]) as usize;
        off += 4;
        if off + attr_len > end {
            break;
        }
        let val = &buf[off..off + attr_len];
        if attr_type == ATTR_XOR_MAPPED_ADDRESS {
            if let Some(addr) = parse_mapped_addr(val, true, &buf[4..20]) {
                return Some(addr);
            }
        } else if attr_type == ATTR_MAPPED_ADDRESS && fallback.is_none() {
            fallback = parse_mapped_addr(val, false, &buf[4..20]);
        }
        let padded = (attr_len + 3) & !3;
        off = off.saturating_add(padded);
    }
    fallback
}

fn parse_mapped_addr(val: &[u8], xor: bool, header_cookie_tx: &[u8]) -> Option<SocketAddr> {
    if val.len() < 8 {
        return None;
    }
    let family = val[1];
    let raw_port = u16::from_be_bytes([val[2], val[3]]);
    let port = if xor {
        raw_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16)
    } else {
        raw_port
    };
    match family {
        0x01 if val.len() >= 8 => {
            let mut ip_bytes = [val[4], val[5], val[6], val[7]];
            if xor {
                let cookie_bytes = STUN_MAGIC_COOKIE.to_be_bytes();
                for i in 0..4 {
                    ip_bytes[i] ^= cookie_bytes[i];
                }
            }
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip_bytes)), port))
        }
        0x02 if val.len() >= 20 => {
            let mut ip_bytes = [0u8; 16];
            ip_bytes.copy_from_slice(&val[4..20]);
            if xor {
                for (b, key) in ip_bytes.iter_mut().zip(header_cookie_tx.iter()) {
                    *b ^= *key;
                }
            }
            Some(SocketAddr::new(IpAddr::V6(ip_bytes.into()), port))
        }
        _ => None,
    }
}

/// Descubre la direccion publica `ip:port` del socket UDP dado usando servidores STUN.
/// Restaura `read_timeout` al terminar para no alterar el uso posterior en QUIC.
pub fn discover_public_addr_sync(sock: &UdpSocket, timeout: Duration) -> Option<SocketAddr> {
    let prev_timeout = sock.read_timeout().ok().flatten();
    let _ = sock.set_read_timeout(Some(timeout));
    let tx_id = rand::random::<[u8; 12]>();
    let req = build_binding_request(tx_id);

    let mut resolved = None;
    'outer: for server in DEFAULT_STUN_SERVERS {
        let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(server) else {
            continue;
        };
        for target in addrs {
            if !target.is_ipv4() {
                continue;
            }
            if sock.send_to(&req, target).is_err() {
                continue;
            }
            let mut buf = [0u8; 512];
            if let Ok((n, _)) = sock.recv_from(&mut buf) {
                if let Some(pub_addr) = parse_binding_response(&buf[..n], &tx_id) {
                    if !is_private_or_cgnat(pub_addr.ip()) {
                        resolved = Some(pub_addr);
                        break 'outer;
                    }
                }
            }
        }
    }
    let _ = sock.set_read_timeout(prev_timeout);
    resolved
}

/// Indica si una IP pertenece a rango privado, loopback o CGNAT (100.64.0.0/10).
pub fn is_private_or_cgnat(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || (o[0] == 100 && (o[1] & 0b1100_0000) == 64)
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified() || v6.is_unique_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stun_xor_mapped_roundtrip_parses_ip_and_port() {
        let tx_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let req = build_binding_request(tx_id);
        assert_eq!(&req[0..2], &0x0001u16.to_be_bytes());
        assert_eq!(&req[4..8], &STUN_MAGIC_COOKIE.to_be_bytes());

        // Simula respuesta STUN con XOR-MAPPED-ADDRESS para 203.0.113.42:54321
        let ip = [203u8, 0, 113, 42];
        let port = 54321u16;
        let cookie_bytes = STUN_MAGIC_COOKIE.to_be_bytes();
        let xor_port = port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);

        let mut resp = Vec::new();
        resp.extend_from_slice(&STUN_BINDING_SUCCESS.to_be_bytes());
        resp.extend_from_slice(&12u16.to_be_bytes());
        resp.extend_from_slice(&cookie_bytes);
        resp.extend_from_slice(&tx_id);
        resp.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
        resp.extend_from_slice(&8u16.to_be_bytes());
        resp.push(0);
        resp.push(0x01);
        resp.extend_from_slice(&xor_port.to_be_bytes());
        for i in 0..4 {
            resp.push(ip[i] ^ cookie_bytes[i]);
        }

        let parsed = parse_binding_response(&resp, &tx_id).expect("STUN parseo valido");
        assert_eq!(parsed, "203.0.113.42:54321".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn cgnat_and_private_detection() {
        assert!(is_private_or_cgnat("192.168.1.5".parse().unwrap()));
        assert!(is_private_or_cgnat("100.64.0.1".parse().unwrap()));
        assert!(!is_private_or_cgnat("8.8.8.8".parse().unwrap()));
    }
}
