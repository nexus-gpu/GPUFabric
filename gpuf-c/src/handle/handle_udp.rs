use super::*;
use bincode::{self as bincode, config as bincode_config};
use crc32fast::Hasher as Crc32;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::VecDeque;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use url::Url;

use anyhow::{anyhow, Result};
use common::Command;

impl ClientWorker {
    pub(super) const P2P_UDP_MAGIC: [u8; 4] = *b"P2PU";
    pub(super) const P2P_UDP_VERSION: u8 = 1;
    pub(super) const P2P_UDP_FLAG_ACK: u8 = 0x01;
    pub(super) const P2P_UDP_HEADER_LEN: usize = 4 + 1 + 1 + 4 + 2 + 2; // magic + version + flags + msg_id + frag_idx + frag_cnt
    pub(super) const P2P_UDP_MTU_PAYLOAD: usize = 1200;

    pub(super) fn p2p_udp_make_header(
        flags: u8,
        msg_id: u32,
        frag_idx: u16,
        frag_cnt: u16,
    ) -> [u8; Self::P2P_UDP_HEADER_LEN] {
        let mut h = [0u8; Self::P2P_UDP_HEADER_LEN];
        h[0..4].copy_from_slice(&Self::P2P_UDP_MAGIC);
        h[4] = Self::P2P_UDP_VERSION;
        h[5] = flags;
        h[6..10].copy_from_slice(&msg_id.to_be_bytes());
        h[10..12].copy_from_slice(&frag_idx.to_be_bytes());
        h[12..14].copy_from_slice(&frag_cnt.to_be_bytes());
        h
    }

    pub(super) fn p2p_udp_parse_header(buf: &[u8]) -> Option<(u8, u32, u16, u16)> {
        if buf.len() < Self::P2P_UDP_HEADER_LEN {
            return None;
        }
        if &buf[0..4] != Self::P2P_UDP_MAGIC {
            return None;
        }
        if buf[4] != Self::P2P_UDP_VERSION {
            return None;
        }
        let flags = buf[5];
        let msg_id = u32::from_be_bytes([buf[6], buf[7], buf[8], buf[9]]);
        let frag_idx = u16::from_be_bytes([buf[10], buf[11]]);
        let frag_cnt = u16::from_be_bytes([buf[12], buf[13]]);
        Some((flags, msg_id, frag_idx, frag_cnt))
    }

    pub(super) async fn p2p_udp_send_ack(socket: &UdpSocket, to: std::net::SocketAddr, msg_id: u32) {
        let hdr = Self::p2p_udp_make_header(Self::P2P_UDP_FLAG_ACK, msg_id, 0, 0);
        let _ = socket.send_to(&hdr, to).await;
    }

    pub(super) async fn p2p_udp_send_reliable(
        socket: &UdpSocket,
        to: std::net::SocketAddr,
        msg_id: u32,
        payload: &[u8],
    ) -> Result<()> {
        let max_payload = Self::P2P_UDP_MTU_PAYLOAD.saturating_sub(Self::P2P_UDP_HEADER_LEN);
        if max_payload == 0 {
            return Err(anyhow!("p2p udp mtu too small"));
        }
        let frag_cnt = ((payload.len() + max_payload - 1) / max_payload).max(1);
        if frag_cnt > u16::MAX as usize {
            return Err(anyhow!("p2p udp too many fragments"));
        }

        for frag_idx in 0..frag_cnt {
            let start = frag_idx * max_payload;
            let end = ((frag_idx + 1) * max_payload).min(payload.len());
            let hdr = Self::p2p_udp_make_header(0, msg_id, frag_idx as u16, frag_cnt as u16);
            let mut pkt = Vec::with_capacity(Self::P2P_UDP_HEADER_LEN + (end - start));
            pkt.extend_from_slice(&hdr);
            pkt.extend_from_slice(&payload[start..end]);

            // Stop-and-wait per fragment.
            let mut tries = 0u32;
            loop {
                tries += 1;
                socket.send_to(&pkt, to).await?;

                let mut ack_buf = [0u8; 64];
                let ack_res = timeout(Duration::from_millis(400), socket.recv_from(&mut ack_buf)).await;
                if let Ok(Ok((n, from))) = ack_res {
                    if from != to {
                        continue;
                    }
                    if let Some((flags, ack_id, _fi, _fc)) = Self::p2p_udp_parse_header(&ack_buf[..n]) {
                        if (flags & Self::P2P_UDP_FLAG_ACK) != 0 && ack_id == msg_id {
                            break;
                        }
                    }
                }
                if tries >= 10 {
                    return Err(anyhow!("p2p udp send timeout msg_id={msg_id}"));
                }
            }
        }
        Ok(())
    }

    pub(super) fn p2p_udp_encode_command_payload(command: &Command) -> Result<Vec<u8>> {
        // payload uses same framing as udp_encode_command (len + bincode) so we can reuse decode.
        Self::udp_encode_command(command)
    }

    pub(super) fn p2p_udp_try_reassemble(
        parts: &mut std::collections::HashMap<u16, Vec<u8>>,
        frag_cnt: u16,
    ) -> Option<Vec<u8>> {
        if frag_cnt == 0 {
            return None;
        }
        for i in 0..frag_cnt {
            if !parts.contains_key(&i) {
                return None;
            }
        }
        let mut out = Vec::new();
        for i in 0..frag_cnt {
            if let Some(p) = parts.remove(&i) {
                out.extend_from_slice(&p);
            }
        }
        Some(out)
    }

    pub(super) fn stun_new_txid() -> [u8; 12] {
        uuid::Uuid::new_v4().as_bytes()[..12]
            .try_into()
            .unwrap_or([0u8; 12])
    }

    pub(super) fn stun_write_attr(buf: &mut Vec<u8>, attr_type: u16, value: &[u8]) {
        buf.extend_from_slice(&attr_type.to_be_bytes());
        buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
        buf.extend_from_slice(value);
        let pad = (4 - (value.len() % 4)) % 4;
        if pad != 0 {
            buf.extend(std::iter::repeat_n(0u8, pad));
        }
    }

    pub(super) fn stun_build_message(
        msg_type: u16,
        txid: [u8; 12],
        attrs: &[(&u16, Vec<u8>)],
        mi: Option<(&str, &str, &str)>,
        fingerprint: bool,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        for (t, v) in attrs {
            Self::stun_write_attr(&mut body, **t, v);
        }

        let mut header = Vec::with_capacity(20);
        header.extend_from_slice(&msg_type.to_be_bytes());
        header.extend_from_slice(&0u16.to_be_bytes());
        header.extend_from_slice(&0x2112A442u32.to_be_bytes());
        header.extend_from_slice(&txid);

        let mut msg = header;
        msg.extend_from_slice(&body);

        if let Some((username, realm, password)) = mi {
            // MESSAGE-INTEGRITY is computed over the message up to the MI attribute (with header length set accordingly).
            // Key for long-term credentials is MD5(username:realm:password).
            let key_src = format!("{}:{}:{}", username, realm, password);
            let key = md5::compute(key_src.as_bytes());

            let mi_attr_len = 20usize;
            let mi_total = 4 + mi_attr_len;
            let fp_total = if fingerprint { 8 } else { 0 };
            let new_len = (msg.len() - 20 + mi_total + fp_total) as u16;
            msg[2..4].copy_from_slice(&new_len.to_be_bytes());

            // Append MI header with zeroed value for HMAC calc length purposes.
            let mut tmp = msg.clone();
            tmp.extend_from_slice(&0x0008u16.to_be_bytes());
            tmp.extend_from_slice(&(mi_attr_len as u16).to_be_bytes());
            tmp.extend_from_slice(&[0u8; 20]);

            let mut mac = Hmac::<Sha1>::new_from_slice(&key.0).expect("hmac key");
            mac.update(&tmp);
            let out = mac.finalize().into_bytes();

            msg.extend_from_slice(&0x0008u16.to_be_bytes());
            msg.extend_from_slice(&(mi_attr_len as u16).to_be_bytes());
            msg.extend_from_slice(&out);
        } else {
            let new_len = (msg.len() - 20) as u16;
            msg[2..4].copy_from_slice(&new_len.to_be_bytes());
        }

        if fingerprint {
            // FINGERPRINT over entire message up to the fingerprint attribute (with header length set accordingly).
            let new_len = (msg.len() - 20 + 8) as u16;
            msg[2..4].copy_from_slice(&new_len.to_be_bytes());

            let mut crc = Crc32::new();
            crc.update(&msg);
            let fp = crc.finalize() ^ 0x5354554e;
            msg.extend_from_slice(&0x8028u16.to_be_bytes());
            msg.extend_from_slice(&4u16.to_be_bytes());
            msg.extend_from_slice(&fp.to_be_bytes());
        }

        msg
    }

    pub(super) async fn stun_read_message<S: tokio::io::AsyncRead + Unpin>(
        stream: &mut S,
    ) -> Result<Vec<u8>> {
        let mut header = [0u8; 20];
        stream.read_exact(&mut header).await?;
        let len = u16::from_be_bytes([header[2], header[3]]) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await?;
        let mut msg = Vec::with_capacity(20 + len);
        msg.extend_from_slice(&header);
        msg.extend_from_slice(&body);
        Ok(msg)
    }

    pub(super) async fn stun_binding_srflx_on_socket(
        socket: &UdpSocket,
        stun_url: &str,
    ) -> Result<std::net::SocketAddr> {
        let Some((host, port)) = Self::parse_stun_host_port(stun_url) else {
            return Err(anyhow!("Invalid STUN url: {stun_url}"));
        };

        let server = (host.as_str(), port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| anyhow!("STUN host resolve failed"))?;

        let txid = Self::stun_new_txid();
        let req = Self::stun_build_message(0x0001, txid, &Vec::new(), None, true);
        socket.send_to(&req, server).await?;

        let mut buf = [0u8; 1500];
        let (n, _from) = timeout(Duration::from_secs(3), socket.recv_from(&mut buf)).await??;
        let resp = &buf[..n];
        let attrs = Self::stun_attr_iter(resp)?;
        let mapped = attrs
            .iter()
            .find(|(t, _)| *t == 0x0020)
            .and_then(|(_, v)| Self::stun_parse_xor_addr(v, &txid))
            .ok_or_else(|| anyhow!("STUN response missing XOR-MAPPED-ADDRESS"))?;
        Ok(mapped)
    }

    pub(super) fn udp_encode_command(command: &Command) -> Result<Vec<u8>> {
        let config = bincode_config::standard()
            .with_fixed_int_encoding()
            .with_little_endian();
        let payload = bincode::encode_to_vec(command, config)?;
        let len = payload.len() as u32;
        let mut out = Vec::with_capacity(4 + payload.len());
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }

    pub(super) fn udp_decode_command(datagram: &[u8]) -> Result<Command> {
        if datagram.len() < 4 {
            return Err(anyhow!("udp datagram too short"));
        }
        let len = u32::from_be_bytes([datagram[0], datagram[1], datagram[2], datagram[3]]) as usize;
        if datagram.len() < 4 + len {
            return Err(anyhow!("udp datagram truncated"));
        }
        let config = bincode_config::standard()
            .with_fixed_int_encoding()
            .with_little_endian();
        let (cmd, _) = bincode::decode_from_slice(&datagram[4..4 + len], config)
            .map_err(|e| anyhow!("Failed to deserialize command: {}", e))?;
        Ok(cmd)
    }

    #[cfg(not(target_os = "android"))]
    pub(super) fn turn_encode_xor_peer_address(peer: std::net::SocketAddr, txid: &[u8; 12]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(0);
        match peer {
            std::net::SocketAddr::V4(v4) => {
                out.push(0x01);
                out.extend_from_slice(&(v4.port() ^ 0x2112).to_be_bytes());
                let ip = u32::from(*v4.ip()) ^ 0x2112A442;
                out.extend_from_slice(&ip.to_be_bytes());
            }
            std::net::SocketAddr::V6(v6) => {
                out.push(0x02);
                out.extend_from_slice(&(v6.port() ^ 0x2112).to_be_bytes());
                let mut mask = [0u8; 16];
                mask[..4].copy_from_slice(&0x2112A442u32.to_be_bytes());
                mask[4..].copy_from_slice(txid);
                let ip = v6.ip().octets();
                let mut x = [0u8; 16];
                for i in 0..16 {
                    x[i] = ip[i] ^ mask[i];
                }
                out.extend_from_slice(&x);
            }
        }
        out
    }

    #[cfg(not(target_os = "android"))]
    pub(super) async fn turn_allocate_udp(
        turn_url: &str,
        username: &str,
        password: &str,
    ) -> Result<(Arc<UdpSocket>, std::net::SocketAddr, String, String)> {
        let url = Url::parse(turn_url)?;
        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("turn url missing host"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| anyhow!("turn url missing port"))?;
        let server = (host, port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| anyhow!("turn host resolve failed"))?;

        let sock = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
        sock.connect(server).await?;

        let requested_transport_t: u16 = 0x0019;
        let lifetime_t: u16 = 0x000d;

        let txid = Self::stun_new_txid();
        let mut attrs = Vec::new();
        // UDP = 17
        attrs.push((&requested_transport_t, vec![17u8, 0, 0, 0]));
        attrs.push((&lifetime_t, 600u32.to_be_bytes().to_vec()));
        let req = Self::stun_build_message(0x0003, txid, &attrs, None, true);
        sock.send(&req).await?;

        let mut buf = vec![0u8; 2048];
        let n = timeout(Duration::from_secs(3), sock.recv(&mut buf)).await??;
        let resp = &buf[..n];
        let msg_type = u16::from_be_bytes([resp[0], resp[1]]);
        if msg_type != 0x0113 {
            return Err(anyhow!(
                "TURN Allocate expected 401, got type=0x{:04x}",
                msg_type
            ));
        }

        let attrs_resp = Self::stun_attr_iter(resp)?;
        let realm = Self::stun_get_text_attr(&attrs_resp, 0x0014)
            .ok_or_else(|| anyhow!("TURN missing REALM"))?;
        let nonce = Self::stun_get_text_attr(&attrs_resp, 0x0015)
            .ok_or_else(|| anyhow!("TURN missing NONCE"))?;

        let username_t: u16 = 0x0006;
        let realm_t: u16 = 0x0014;
        let nonce_t: u16 = 0x0015;

        let txid2 = Self::stun_new_txid();
        let mut attrs2 = Vec::new();
        attrs2.push((&username_t, username.as_bytes().to_vec()));
        attrs2.push((&realm_t, realm.as_bytes().to_vec()));
        attrs2.push((&nonce_t, nonce.as_bytes().to_vec()));
        attrs2.push((&requested_transport_t, vec![17u8, 0, 0, 0]));
        attrs2.push((&lifetime_t, 600u32.to_be_bytes().to_vec()));
        let req2 = Self::stun_build_message(
            0x0003,
            txid2,
            &attrs2,
            Some((username, &realm, password)),
            true,
        );
        sock.send(&req2).await?;

        let n2 = timeout(Duration::from_secs(3), sock.recv(&mut buf)).await??;
        let resp2 = &buf[..n2];
        let msg_type2 = u16::from_be_bytes([resp2[0], resp2[1]]);
        if msg_type2 != 0x0103 {
            return Err(anyhow!("TURN Allocate failed type=0x{:04x}", msg_type2));
        }
        let attrs2_resp = Self::stun_attr_iter(resp2)?;
        let relayed = attrs2_resp
            .iter()
            .find(|(t, _)| *t == 0x0016)
            .and_then(|(_, v)| Self::stun_parse_xor_addr(v, &txid2))
            .ok_or_else(|| anyhow!("TURN Allocate missing XOR-RELAYED-ADDRESS"))?;

        Ok((sock, relayed, realm, nonce))
    }

    #[cfg(not(target_os = "android"))]
    pub(super) async fn turn_create_permission(
        sock: &UdpSocket,
        peer: std::net::SocketAddr,
        username: &str,
        password: &str,
        realm: &str,
        nonce: &str,
    ) -> Result<()> {
        let username_t: u16 = 0x0006;
        let realm_t: u16 = 0x0014;
        let nonce_t: u16 = 0x0015;
        let xor_peer_t: u16 = 0x0012;

        let txid = Self::stun_new_txid();
        let xor_peer = Self::turn_encode_xor_peer_address(peer, &txid);
        let mut attrs = Vec::new();
        attrs.push((&username_t, username.as_bytes().to_vec()));
        attrs.push((&realm_t, realm.as_bytes().to_vec()));
        attrs.push((&nonce_t, nonce.as_bytes().to_vec()));
        attrs.push((&xor_peer_t, xor_peer));
        let req = Self::stun_build_message(
            0x0008,
            txid,
            &attrs,
            Some((username, realm, password)),
            true,
        );
        sock.send(&req).await?;

        let mut buf = vec![0u8; 2048];
        let n = timeout(Duration::from_secs(3), sock.recv(&mut buf)).await??;
        let resp = &buf[..n];
        let msg_type = u16::from_be_bytes([resp[0], resp[1]]);
        if msg_type != 0x0108 {
            return Err(anyhow!(
                "TURN CreatePermission failed type=0x{:04x}",
                msg_type
            ));
        }
        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    pub(super) async fn turn_send_indication(
        sock: &UdpSocket,
        peer: std::net::SocketAddr,
        data: &[u8],
    ) -> Result<()> {
        let xor_peer_t: u16 = 0x0012;
        let data_t: u16 = 0x0013;
        let txid = Self::stun_new_txid();
        let xor_peer = Self::turn_encode_xor_peer_address(peer, &txid);
        let mut attrs = Vec::new();
        attrs.push((&xor_peer_t, xor_peer));
        attrs.push((&data_t, data.to_vec()));
        // Send Indication: method 0x0016
        let msg = Self::stun_build_message(0x0016, txid, &attrs, None, true);
        sock.send(&msg).await?;
        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    pub(super) fn turn_parse_data_indication(msg: &[u8]) -> Option<(std::net::SocketAddr, Vec<u8>)> {
        if msg.len() < 20 {
            return None;
        }
        let msg_type = u16::from_be_bytes([msg[0], msg[1]]);
        // Data Indication: 0x0017
        if msg_type != 0x0017 {
            return None;
        }
        let txid: [u8; 12] = msg[8..20].try_into().unwrap_or([0u8; 12]);
        let attrs = Self::stun_attr_iter(msg).ok()?;
        let peer = attrs
            .iter()
            .find(|(t, _)| *t == 0x0012)
            .and_then(|(_, v)| Self::stun_parse_xor_addr(v, &txid))?;
        let data = attrs
            .iter()
            .find(|(t, _)| *t == 0x0013)
            .map(|(_, v)| v.clone())?;
        Some((peer, data))
    }

    #[cfg(not(target_os = "android"))]
    pub(super) async fn turn_send_reliable_over_indication(
        sock: &UdpSocket,
        peer: std::net::SocketAddr,
        msg_id: u32,
        payload: &[u8],
        inbox: &mut VecDeque<(std::net::SocketAddr, Vec<u8>)>,
    ) -> Result<()> {
        let max_payload = Self::P2P_UDP_MTU_PAYLOAD.saturating_sub(Self::P2P_UDP_HEADER_LEN);
        if max_payload == 0 {
            return Err(anyhow!("p2p udp mtu too small"));
        }
        let frag_cnt = ((payload.len() + max_payload - 1) / max_payload).max(1);
        if frag_cnt > u16::MAX as usize {
            return Err(anyhow!("p2p udp too many fragments"));
        }

        for frag_idx in 0..frag_cnt {
            let start = frag_idx * max_payload;
            let end = ((frag_idx + 1) * max_payload).min(payload.len());
            let hdr = Self::p2p_udp_make_header(0, msg_id, frag_idx as u16, frag_cnt as u16);
            let mut pkt = Vec::with_capacity(Self::P2P_UDP_HEADER_LEN + (end - start));
            pkt.extend_from_slice(&hdr);
            pkt.extend_from_slice(&payload[start..end]);

            let mut tries = 0u32;
            loop {
                tries += 1;
                Self::turn_send_indication(sock, peer, &pkt).await?;

                let mut buf = vec![0u8; 4096];
                let recv_res = timeout(Duration::from_millis(400), sock.recv(&mut buf)).await;
                if let Ok(Ok(n)) = recv_res {
                    if let Some((src, data)) = Self::turn_parse_data_indication(&buf[..n]) {
                        if let Some((flags, ack_id, _fi, _fc)) = Self::p2p_udp_parse_header(&data) {
                            if (flags & Self::P2P_UDP_FLAG_ACK) != 0 && ack_id == msg_id {
                                break;
                            }
                        }
                        inbox.push_back((src, data));
                    }
                }

                if tries >= 10 {
                    return Err(anyhow!("p2p udp send timeout msg_id={msg_id}"));
                }
            }
        }
        Ok(())
    }

    pub(super) fn stun_attr_iter(msg: &[u8]) -> Result<Vec<(u16, Vec<u8>)>> {
        if msg.len() < 20 {
            return Err(anyhow!("stun msg too short"));
        }
        let len = u16::from_be_bytes([msg[2], msg[3]]) as usize;
        if msg.len() < 20 + len {
            return Err(anyhow!("stun msg len mismatch"));
        }
        let mut out = Vec::new();
        let mut pos = 20;
        let end = 20 + len;
        while pos + 4 <= end {
            let t = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
            let l = u16::from_be_bytes([msg[pos + 2], msg[pos + 3]]) as usize;
            pos += 4;
            if pos + l > end {
                break;
            }
            out.push((t, msg[pos..pos + l].to_vec()));
            pos += l;
            let pad = (4 - (l % 4)) % 4;
            pos += pad;
        }
        Ok(out)
    }

    pub(super) fn stun_get_text_attr(attrs: &[(u16, Vec<u8>)], t: u16) -> Option<String> {
        attrs
            .iter()
            .find(|(k, _)| *k == t)
            .and_then(|(_, v)| String::from_utf8(v.clone()).ok())
    }

    pub(super) fn stun_parse_xor_addr(v: &[u8], txid: &[u8; 12]) -> Option<std::net::SocketAddr> {
        if v.len() < 8 {
            return None;
        }
        let family = v[1];
        let xport = u16::from_be_bytes([v[2], v[3]]);
        let port = xport ^ 0x2112;
        if family == 0x01 {
            let xaddr = u32::from_be_bytes([v[4], v[5], v[6], v[7]]);
            let addr = xaddr ^ 0x2112A442;
            let ip = std::net::Ipv4Addr::from(addr);
            return Some(std::net::SocketAddr::new(ip.into(), port));
        }
        if family == 0x02 && v.len() >= 20 {
            let mut x = [0u8; 16];
            x.copy_from_slice(&v[4..20]);
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&0x2112A442u32.to_be_bytes());
            mask[4..].copy_from_slice(txid);
            let mut out = [0u8; 16];
            for i in 0..16 {
                out[i] = x[i] ^ mask[i];
            }
            let ip = std::net::Ipv6Addr::from(out);
            return Some(std::net::SocketAddr::new(ip.into(), port));
        }
        None
    }

    pub(super) fn parse_stun_host_port(url: &str) -> Option<(String, u16)> {
        let url = url.trim();
        let rest = url.strip_prefix("stun:").unwrap_or(url);
        let rest = rest.strip_prefix("//").unwrap_or(rest);
        let (host, port) = rest.rsplit_once(':')?;
        let port: u16 = port.parse().ok()?;
        Some((host.to_string(), port))
    }

    pub(super) fn build_stun_binding_request() -> ([u8; 12], Vec<u8>) {
        // RFC5389: STUN Binding Request
        // Header: type(2)=0x0001, length(2), cookie(4)=0x2112A442, transaction_id(12)
        let txid = uuid::Uuid::new_v4().as_bytes()[..12]
            .try_into()
            .unwrap_or([0u8; 12]);

        let mut msg = Vec::with_capacity(20);
        msg.extend_from_slice(&0x0001u16.to_be_bytes());
        msg.extend_from_slice(&0u16.to_be_bytes());
        msg.extend_from_slice(&0x2112A442u32.to_be_bytes());
        msg.extend_from_slice(&txid);
        (txid, msg)
    }

    pub(super) fn parse_xor_mapped_address(
        resp: &[u8],
        txid: &[u8; 12],
    ) -> Option<std::net::SocketAddr> {
        // Minimal STUN response parser. Only supports XOR-MAPPED-ADDRESS.
        if resp.len() < 20 {
            return None;
        }
        let msg_type = u16::from_be_bytes([resp[0], resp[1]]);
        if msg_type != 0x0101 {
            return None;
        }
        let msg_len = u16::from_be_bytes([resp[2], resp[3]]) as usize;
        if resp.len() < 20 + msg_len {
            return None;
        }
        let cookie = &resp[4..8];
        if cookie != 0x2112A442u32.to_be_bytes() {
            return None;
        }
        if &resp[8..20] != txid {
            return None;
        }

        let mut pos = 20;
        let end = 20 + msg_len;
        while pos + 4 <= end {
            let attr_type = u16::from_be_bytes([resp[pos], resp[pos + 1]]);
            let attr_len = u16::from_be_bytes([resp[pos + 2], resp[pos + 3]]) as usize;
            pos += 4;
            if pos + attr_len > end {
                return None;
            }

            if attr_type == 0x0020 {
                // XOR-MAPPED-ADDRESS
                if attr_len < 8 {
                    return None;
                }
                let family = resp[pos + 1];
                let xport = u16::from_be_bytes([resp[pos + 2], resp[pos + 3]]);
                let port = xport ^ 0x2112;

                if family == 0x01 {
                    // IPv4
                    if attr_len < 8 {
                        return None;
                    }
                    let xaddr = u32::from_be_bytes([
                        resp[pos + 4],
                        resp[pos + 5],
                        resp[pos + 6],
                        resp[pos + 7],
                    ]);
                    let addr = xaddr ^ 0x2112A442;
                    let ip = std::net::Ipv4Addr::from(addr);
                    return Some(std::net::SocketAddr::new(ip.into(), port));
                }

                if family == 0x02 {
                    // IPv6
                    if attr_len < 20 {
                        return None;
                    }
                    let mut x = [0u8; 16];
                    x.copy_from_slice(&resp[pos + 4..pos + 20]);
                    let mut out = [0u8; 16];
                    // XOR with cookie + txid
                    let mut mask = [0u8; 16];
                    mask[..4].copy_from_slice(&0x2112A442u32.to_be_bytes());
                    mask[4..].copy_from_slice(txid);
                    for i in 0..16 {
                        out[i] = x[i] ^ mask[i];
                    }
                    let ip = std::net::Ipv6Addr::from(out);
                    return Some(std::net::SocketAddr::new(ip.into(), port));
                }
            }

            // attrs are padded to 4-byte boundary
            pos += attr_len;
            let pad = (4 - (attr_len % 4)) % 4;
            pos += pad;
        }

        None
    }

    pub(super) async fn stun_binding_srflx(stun_url: &str) -> Result<std::net::SocketAddr> {
        let Some((host, port)) = Self::parse_stun_host_port(stun_url) else {
            return Err(anyhow!("Invalid STUN url: {stun_url}"));
        };
        let server = format!("{}:{}", host, port);

        let sock = UdpSocket::bind("0.0.0.0:0").await?;
        let (txid, req) = Self::build_stun_binding_request();

        sock.send_to(&req, &server).await?;

        let mut buf = [0u8; 1500];
        let (n, _addr) = timeout(Duration::from_secs(3), sock.recv_from(&mut buf)).await??;
        let resp = &buf[..n];
        Self::parse_xor_mapped_address(resp, &txid)
            .ok_or_else(|| anyhow!("Failed to parse STUN XOR-MAPPED-ADDRESS"))
    }

    pub(super) async fn detect_outbound_ip() -> Result<std::net::IpAddr> {
        // UDP "connect" doesn't send packets, but lets OS pick the outbound interface.
        // Then we can read the chosen local address.
        let sock = UdpSocket::bind("0.0.0.0:0").await?;
        sock.connect("1.1.1.1:80").await?;
        Ok(sock.local_addr()?.ip())
    }
}
