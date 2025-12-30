use anyhow::{anyhow, Result};
use bytes::BytesMut;
use clap::Parser;
use common::{
    read_command, write_command, Command, CommandV1, CommandV2, DevicesInfo, OsType, P2PTransport,
    SystemInfo, MAX_MESSAGE_SIZE,
};
use crc32fast::Hasher as Crc32;
use hmac::{Hmac, Mac};
use md5;
use std::net::ToSocketAddrs;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;
use url::Url;

use sha1::Sha1;

#[cfg(not(target_os = "android"))]
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, ServerName},
        ClientConfig, RootCertStore,
    },
    TlsConnector,
};

fn udp_encode_command(command: &Command) -> Result<Vec<u8>> {
    let config = bincode::config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();
    let payload = bincode::encode_to_vec(command, config)?;
    let len = payload.len() as u32;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

const P2P_UDP_MAGIC: [u8; 4] = *b"P2PU";
const P2P_UDP_VERSION: u8 = 1;
const P2P_UDP_FLAG_ACK: u8 = 0x01;
const P2P_UDP_HEADER_LEN: usize = 4 + 1 + 1 + 4 + 2 + 2;
const P2P_UDP_MTU_PAYLOAD: usize = 1200;

fn p2p_udp_make_header(
    flags: u8,
    msg_id: u32,
    frag_idx: u16,
    frag_cnt: u16,
) -> [u8; P2P_UDP_HEADER_LEN] {
    let mut h = [0u8; P2P_UDP_HEADER_LEN];
    h[0..4].copy_from_slice(&P2P_UDP_MAGIC);
    h[4] = P2P_UDP_VERSION;
    h[5] = flags;
    h[6..10].copy_from_slice(&msg_id.to_be_bytes());
    h[10..12].copy_from_slice(&frag_idx.to_be_bytes());
    h[12..14].copy_from_slice(&frag_cnt.to_be_bytes());
    h
}

fn p2p_udp_parse_header(buf: &[u8]) -> Option<(u8, u32, u16, u16)> {
    if buf.len() < P2P_UDP_HEADER_LEN {
        return None;
    }
    if &buf[0..4] != P2P_UDP_MAGIC {
        return None;
    }
    if buf[4] != P2P_UDP_VERSION {
        return None;
    }
    let flags = buf[5];
    let msg_id = u32::from_be_bytes([buf[6], buf[7], buf[8], buf[9]]);
    let frag_idx = u16::from_be_bytes([buf[10], buf[11]]);
    let frag_cnt = u16::from_be_bytes([buf[12], buf[13]]);
    Some((flags, msg_id, frag_idx, frag_cnt))
}

async fn p2p_udp_send_ack(socket: &UdpSocket, to: std::net::SocketAddr, msg_id: u32) {
    let hdr = p2p_udp_make_header(P2P_UDP_FLAG_ACK, msg_id, 0, 0);
    let _ = socket.send_to(&hdr, to).await;
}

async fn p2p_udp_send_reliable(
    socket: &UdpSocket,
    to: std::net::SocketAddr,
    msg_id: u32,
    payload: &[u8],
) -> Result<()> {
    let max_payload = P2P_UDP_MTU_PAYLOAD.saturating_sub(P2P_UDP_HEADER_LEN);
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
        let hdr = p2p_udp_make_header(0, msg_id, frag_idx as u16, frag_cnt as u16);
        let mut pkt = Vec::with_capacity(P2P_UDP_HEADER_LEN + (end - start));
        pkt.extend_from_slice(&hdr);
        pkt.extend_from_slice(&payload[start..end]);

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
                if let Some((flags, ack_id, _fi, _fc)) = p2p_udp_parse_header(&ack_buf[..n]) {
                    if (flags & P2P_UDP_FLAG_ACK) != 0 && ack_id == msg_id {
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

fn p2p_udp_try_reassemble(
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

fn turn_encode_xor_peer_address(peer: std::net::SocketAddr, txid: &[u8; 12]) -> Vec<u8> {
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

async fn turn_allocate_udp(
    turn_url: &str,
    username: &str,
    password: &str,
) -> Result<(UdpSocket, std::net::SocketAddr, String, String)> {
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

    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(server).await?;

    let requested_transport_t: u16 = 0x0019;
    let lifetime_t: u16 = 0x000d;

    let txid = stun_new_txid();
    let mut attrs = Vec::new();
    attrs.push((&requested_transport_t, vec![17u8, 0, 0, 0]));
    attrs.push((&lifetime_t, 600u32.to_be_bytes().to_vec()));
    let req = stun_build_message(0x0003, txid, &attrs, None, true);
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
    let attrs_resp = stun_attr_iter(resp)?;
    let realm =
        stun_get_text_attr(&attrs_resp, 0x0014).ok_or_else(|| anyhow!("TURN missing REALM"))?;
    let nonce =
        stun_get_text_attr(&attrs_resp, 0x0015).ok_or_else(|| anyhow!("TURN missing NONCE"))?;

    let username_t: u16 = 0x0006;
    let realm_t: u16 = 0x0014;
    let nonce_t: u16 = 0x0015;

    let txid2 = stun_new_txid();
    let mut attrs2 = Vec::new();
    attrs2.push((&username_t, username.as_bytes().to_vec()));
    attrs2.push((&realm_t, realm.as_bytes().to_vec()));
    attrs2.push((&nonce_t, nonce.as_bytes().to_vec()));
    attrs2.push((&requested_transport_t, vec![17u8, 0, 0, 0]));
    attrs2.push((&lifetime_t, 600u32.to_be_bytes().to_vec()));
    let req2 = stun_build_message(
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
    let attrs2_resp = stun_attr_iter(resp2)?;
    let relayed = attrs2_resp
        .iter()
        .find(|(t, _)| *t == 0x0016)
        .and_then(|(_, v)| stun_parse_xor_addr(v, &txid2))
        .ok_or_else(|| anyhow!("TURN Allocate missing XOR-RELAYED-ADDRESS"))?;

    Ok((sock, relayed, realm, nonce))
}

async fn turn_create_permission(
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

    let txid = stun_new_txid();
    let xor_peer = turn_encode_xor_peer_address(peer, &txid);
    let mut attrs = Vec::new();
    attrs.push((&username_t, username.as_bytes().to_vec()));
    attrs.push((&realm_t, realm.as_bytes().to_vec()));
    attrs.push((&nonce_t, nonce.as_bytes().to_vec()));
    attrs.push((&xor_peer_t, xor_peer));
    let req = stun_build_message(
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

async fn turn_send_indication(
    sock: &UdpSocket,
    peer: std::net::SocketAddr,
    data: &[u8],
) -> Result<()> {
    let xor_peer_t: u16 = 0x0012;
    let data_t: u16 = 0x0013;
    let txid = stun_new_txid();
    let xor_peer = turn_encode_xor_peer_address(peer, &txid);
    let mut attrs = Vec::new();
    attrs.push((&xor_peer_t, xor_peer));
    attrs.push((&data_t, data.to_vec()));
    // Send Indication: 0x0016
    let msg = stun_build_message(0x0016, txid, &attrs, None, true);
    sock.send(&msg).await?;
    Ok(())
}

fn turn_parse_data_indication(msg: &[u8]) -> Option<(std::net::SocketAddr, Vec<u8>)> {
    if msg.len() < 20 {
        return None;
    }
    let msg_type = u16::from_be_bytes([msg[0], msg[1]]);
    if msg_type != 0x0017 {
        return None;
    }
    let txid: [u8; 12] = msg[8..20].try_into().ok()?;
    let attrs = stun_attr_iter(msg).ok()?;
    let peer = attrs
        .iter()
        .find(|(t, _)| *t == 0x0012)
        .and_then(|(_, v)| stun_parse_xor_addr(v, &txid))?;
    let data = attrs
        .iter()
        .find(|(t, _)| *t == 0x0013)
        .map(|(_, v)| v.clone())?;
    Some((peer, data))
}

async fn turn_send_reliable_over_indication(
    sock: &UdpSocket,
    peer: std::net::SocketAddr,
    msg_id: u32,
    payload: &[u8],
    inbox: &mut std::collections::VecDeque<(std::net::SocketAddr, Vec<u8>)>,
) -> Result<()> {
    let max_payload = P2P_UDP_MTU_PAYLOAD.saturating_sub(P2P_UDP_HEADER_LEN);
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
        let hdr = p2p_udp_make_header(0, msg_id, frag_idx as u16, frag_cnt as u16);
        let mut pkt = Vec::with_capacity(P2P_UDP_HEADER_LEN + (end - start));
        pkt.extend_from_slice(&hdr);
        pkt.extend_from_slice(&payload[start..end]);

        let mut tries = 0u32;
        loop {
            tries += 1;
            turn_send_indication(sock, peer, &pkt).await?;

            let mut buf = vec![0u8; 4096];
            let recv_res = timeout(Duration::from_millis(400), sock.recv(&mut buf)).await;
            if let Ok(Ok(n)) = recv_res {
                if let Some((src, data)) = turn_parse_data_indication(&buf[..n]) {
                    if let Some((flags, ack_id, _fi, _fc)) = p2p_udp_parse_header(&data) {
                        if (flags & P2P_UDP_FLAG_ACK) != 0 && ack_id == msg_id {
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

fn udp_decode_command(datagram: &[u8]) -> Result<Command> {
    if datagram.len() < 4 {
        return Err(anyhow!("udp datagram too short"));
    }
    let len = u32::from_be_bytes([datagram[0], datagram[1], datagram[2], datagram[3]]) as usize;
    if datagram.len() < 4 + len {
        return Err(anyhow!("udp datagram truncated"));
    }
    let config = bincode::config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();
    let (cmd, _) = bincode::decode_from_slice(&datagram[4..4 + len], config)
        .map_err(|e| anyhow!("Failed to deserialize command: {}", e))?;
    Ok(cmd)
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    server_addr: String,

    #[arg(long, default_value_t = 17000)]
    control_port: u16,

    /// Target gpuf-c client_id in hex (32 chars, optionally prefixed with 0x)
    #[arg(long)]
    target_client_id: String,

    /// Optional source client_id in hex. If not set, a random UUID will be used.
    #[arg(long)]
    source_client_id: Option<String>,

    #[arg(long, default_value = "Hello from P2P SDK")]
    prompt: String,

    #[arg(long, default_value_t = 128)]
    max_tokens: u32,

    /// Path to a PEM root certificate chain used to verify the TURN server (turns:5349).
    #[arg(long, default_value = "certs/root.pem")]
    cert_chain_path: String,
}

fn parse_client_id_hex(s: &str) -> Result<[u8; 16]> {
    let s = s.trim().trim_start_matches("0x");
    let bytes = hex::decode(s).map_err(|e| anyhow!("invalid hex client_id: {e}"))?;
    let arr: [u8; 16] = bytes
        .try_into()
        .map_err(|_| anyhow!("client_id must be 16 bytes (32 hex chars)"))?;
    Ok(arr)
}

fn stun_parse_xor_addr(v: &[u8], txid: &[u8; 12]) -> Option<std::net::SocketAddr> {
    if v.len() < 4 {
        return None;
    }
    let family = v[1];
    let xport = u16::from_be_bytes([v[2], v[3]]);
    let port = xport ^ 0x2112;
    if family == 0x01 {
        if v.len() < 8 {
            return None;
        }
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

fn stun_new_txid() -> [u8; 12] {
    uuid::Uuid::new_v4().as_bytes()[..12]
        .try_into()
        .unwrap_or([0u8; 12])
}

fn stun_write_attr(buf: &mut Vec<u8>, attr_type: u16, value: &[u8]) {
    buf.extend_from_slice(&attr_type.to_be_bytes());
    buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
    buf.extend_from_slice(value);
    let pad = (4 - (value.len() % 4)) % 4;
    if pad != 0 {
        buf.extend_from_slice(&vec![0u8; pad]);
    }
}

fn stun_build_message(
    msg_type: u16,
    txid: [u8; 12],
    attrs: &Vec<(&u16, Vec<u8>)>,
    mi: Option<(&str, &str, &str)>,
    fingerprint: bool,
) -> Vec<u8> {
    let mut body = Vec::new();
    for (t, v) in attrs {
        stun_write_attr(&mut body, **t, v);
    }

    let mut msg = Vec::with_capacity(20 + body.len() + 64);
    msg.extend_from_slice(&msg_type.to_be_bytes());
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&0x2112A442u32.to_be_bytes());
    msg.extend_from_slice(&txid);
    msg.extend_from_slice(&body);

    if let Some((username, realm, password)) = mi {
        let key_src = format!("{}:{}:{}", username, realm, password);
        let key = md5::compute(key_src.as_bytes());

        let mi_attr_len = 20usize;
        let mi_total = 4 + mi_attr_len;
        let fp_total = if fingerprint { 8 } else { 0 };
        let new_len = (msg.len() - 20 + mi_total + fp_total) as u16;
        msg[2..4].copy_from_slice(&new_len.to_be_bytes());

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

async fn stun_read_message<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> Result<Vec<u8>> {
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

fn stun_attr_iter(msg: &[u8]) -> Result<Vec<(u16, Vec<u8>)>> {
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
        let v = msg[pos..pos + l].to_vec();
        out.push((t, v));
        pos += l;
        let pad = (4 - (l % 4)) % 4;
        pos += pad;
    }
    Ok(out)
}

fn stun_get_text_attr(attrs: &[(u16, Vec<u8>)], t: u16) -> Option<String> {
    attrs
        .iter()
        .find(|(tt, _)| *tt == t)
        .and_then(|(_, v)| std::str::from_utf8(v).ok())
        .map(|s| s.to_string())
}

fn parse_turns_url(turn_url: &str) -> Result<(String, u16)> {
    let url = Url::parse(turn_url)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("turn url missing host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("turn url missing port"))?;
    Ok((host.to_string(), port))
}

#[cfg(not(target_os = "android"))]
fn load_root_cert(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let f = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(f);
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        anyhow::bail!("no certificates found in {}", path);
    }
    Ok(certs)
}

#[cfg(not(target_os = "android"))]
async fn turn_tls_connect(
    host: &str,
    port: u16,
    cert_chain_path: &str,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let addr = format!("{}:{}", host, port);
    let stream = TcpStream::connect(&addr).await?;

    let certs = load_root_cert(cert_chain_path)?;
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(certs);
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(std::sync::Arc::new(config));
    let server_name =
        ServerName::try_from(host.to_string()).map_err(|_| anyhow!("Invalid SNI name"))?;
    Ok(connector.connect(server_name, stream).await?)
}

#[cfg(not(target_os = "android"))]
async fn turn_allocate_tcp(
    turn_url: &str,
    username: &str,
    password: &str,
    cert_chain_path: &str,
) -> Result<(
    tokio_rustls::client::TlsStream<TcpStream>,
    std::net::SocketAddr,
    String,
    String,
)> {
    let (host, port) = parse_turns_url(turn_url)?;
    let mut tls = turn_tls_connect(&host, port, cert_chain_path).await?;

    let requested_transport_t: u16 = 0x0019;
    let lifetime_t: u16 = 0x000d;
    let txid = stun_new_txid();

    let mut attrs = Vec::new();
    attrs.push((&requested_transport_t, vec![6u8, 0, 0, 0]));
    attrs.push((&lifetime_t, 600u32.to_be_bytes().to_vec()));
    let req = stun_build_message(0x0003, txid, &attrs, None, true);
    tls.write_all(&req).await?;
    tls.flush().await?;

    let resp = stun_read_message(&mut tls).await?;
    let msg_type = u16::from_be_bytes([resp[0], resp[1]]);
    if msg_type != 0x0113 {
        return Err(anyhow!(
            "TURN Allocate expected 401, got type=0x{:04x}",
            msg_type
        ));
    }

    let attrs = stun_attr_iter(&resp)?;
    let realm = stun_get_text_attr(&attrs, 0x0014).ok_or_else(|| anyhow!("TURN missing REALM"))?;
    let nonce = stun_get_text_attr(&attrs, 0x0015).ok_or_else(|| anyhow!("TURN missing NONCE"))?;

    let username_t: u16 = 0x0006;
    let realm_t: u16 = 0x0014;
    let nonce_t: u16 = 0x0015;

    let mut attrs2 = Vec::new();
    attrs2.push((&username_t, username.as_bytes().to_vec()));
    attrs2.push((&realm_t, realm.as_bytes().to_vec()));
    attrs2.push((&nonce_t, nonce.as_bytes().to_vec()));
    attrs2.push((&requested_transport_t, vec![6u8, 0, 0, 0]));
    attrs2.push((&lifetime_t, 600u32.to_be_bytes().to_vec()));

    let txid2 = stun_new_txid();
    let req2 = stun_build_message(
        0x0003,
        txid2,
        &attrs2,
        Some((username, &realm, password)),
        true,
    );
    tls.write_all(&req2).await?;
    tls.flush().await?;

    let resp2 = stun_read_message(&mut tls).await?;
    let msg_type2 = u16::from_be_bytes([resp2[0], resp2[1]]);
    if msg_type2 != 0x0103 {
        return Err(anyhow!("TURN Allocate failed type=0x{:04x}", msg_type2));
    }
    let attrs2 = stun_attr_iter(&resp2)?;
    let txid_resp: [u8; 12] = resp2[8..20].try_into().unwrap_or([0u8; 12]);
    let relayed = attrs2
        .iter()
        .find(|(t, _)| *t == 0x0016)
        .and_then(|(_, v)| stun_parse_xor_addr(v, &txid_resp))
        .ok_or_else(|| anyhow!("TURN Allocate missing XOR-RELAYED-ADDRESS"))?;

    Ok((tls, relayed, realm, nonce))
}

#[cfg(not(target_os = "android"))]
async fn turn_connect_peer(
    tls: &mut tokio_rustls::client::TlsStream<TcpStream>,
    peer_relay: std::net::SocketAddr,
    username: &str,
    password: &str,
    realm: &str,
    nonce: &str,
) -> Result<Vec<u8>> {
    let username_t: u16 = 0x0006;
    let realm_t: u16 = 0x0014;
    let nonce_t: u16 = 0x0015;
    let xor_peer_t: u16 = 0x0012;

    let txid = stun_new_txid();
    let mut addr_val = Vec::new();
    addr_val.push(0);
    match peer_relay {
        std::net::SocketAddr::V4(v4) => {
            addr_val.push(0x01);
            addr_val.extend_from_slice(&(v4.port() ^ 0x2112).to_be_bytes());
            let ip = u32::from(*v4.ip()) ^ 0x2112A442;
            addr_val.extend_from_slice(&ip.to_be_bytes());
        }
        std::net::SocketAddr::V6(v6) => {
            addr_val.push(0x02);
            addr_val.extend_from_slice(&(v6.port() ^ 0x2112).to_be_bytes());
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&0x2112A442u32.to_be_bytes());
            mask[4..].copy_from_slice(&txid);
            let ip = v6.ip().octets();
            let mut out = [0u8; 16];
            for i in 0..16 {
                out[i] = ip[i] ^ mask[i];
            }
            addr_val.extend_from_slice(&out);
        }
    }

    let mut attrs = Vec::new();
    attrs.push((&username_t, username.as_bytes().to_vec()));
    attrs.push((&realm_t, realm.as_bytes().to_vec()));
    attrs.push((&nonce_t, nonce.as_bytes().to_vec()));
    attrs.push((&xor_peer_t, addr_val));
    let req = stun_build_message(
        0x000a,
        txid,
        &attrs,
        Some((username, realm, password)),
        true,
    );
    tls.write_all(&req).await?;
    tls.flush().await?;

    let resp = stun_read_message(tls).await?;
    let msg_type = u16::from_be_bytes([resp[0], resp[1]]);
    if msg_type != 0x010a {
        return Err(anyhow!("TURN Connect failed type=0x{:04x}", msg_type));
    }
    let attrs = stun_attr_iter(&resp)?;
    let conn_id = attrs
        .iter()
        .find(|(t, _)| *t == 0x002a)
        .map(|(_, v)| v.clone())
        .ok_or_else(|| anyhow!("TURN Connect missing CONNECTION-ID"))?;
    Ok(conn_id)
}

#[cfg(not(target_os = "android"))]
async fn turn_connection_bind(
    turn_url: &str,
    conn_id: &[u8],
    username: &str,
    password: &str,
    realm: &str,
    nonce: &str,
    cert_chain_path: &str,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let (host, port) = parse_turns_url(turn_url)?;
    let mut tls = turn_tls_connect(&host, port, cert_chain_path).await?;

    let username_t: u16 = 0x0006;
    let realm_t: u16 = 0x0014;
    let nonce_t: u16 = 0x0015;
    let conn_id_t: u16 = 0x002a;

    let txid = stun_new_txid();
    let mut attrs = Vec::new();
    attrs.push((&username_t, username.as_bytes().to_vec()));
    attrs.push((&realm_t, realm.as_bytes().to_vec()));
    attrs.push((&nonce_t, nonce.as_bytes().to_vec()));
    attrs.push((&conn_id_t, conn_id.to_vec()));
    let req = stun_build_message(
        0x000b,
        txid,
        &attrs,
        Some((username, realm, password)),
        true,
    );
    tls.write_all(&req).await?;
    tls.flush().await?;

    let resp = stun_read_message(&mut tls).await?;
    let msg_type = u16::from_be_bytes([resp[0], resp[1]]);
    if msg_type != 0x010b {
        return Err(anyhow!(
            "TURN ConnectionBind failed type=0x{:04x}",
            msg_type
        ));
    }
    Ok(tls)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let target_client_id = parse_client_id_hex(&args.target_client_id)?;
    let source_client_id = match args.source_client_id {
        Some(v) => parse_client_id_hex(&v)?,
        None => *uuid::Uuid::new_v4().as_bytes(),
    };
    let connection_id = *uuid::Uuid::new_v4().as_bytes();

    let addr = format!("{}:{}", args.server_addr, args.control_port);
    let mut stream = TcpStream::connect(&addr).await?;

    // Minimal login so gpuf-s will accept V2 signaling.
    let login = Command::V1(CommandV1::Login {
        client_id: source_client_id,
        version: 1,
        os_type: OsType::LINUX,
        auto_models: false,
        system_info: SystemInfo::default(),
        device_memtotal_gb: 0,
        device_total_tflops: 0,
        devices_info: vec![DevicesInfo::default()],
    });
    write_command(&mut stream, &login).await?;
    stream.flush().await?;

    let req = Command::V2(CommandV2::P2PConnectionRequest {
        source_client_id,
        target_client_id,
        connection_id,
    });
    write_command(&mut stream, &req).await?;
    stream.flush().await?;

    let mut buf = BytesMut::with_capacity(MAX_MESSAGE_SIZE);
    let mut turn_cfg: Option<(Vec<String>, String, String)> = None;
    let peer_candidates = loop {
        let cmd = read_command(&mut stream, &mut buf).await?;
        match cmd {
            Command::V2(CommandV2::P2PConnectionConfig {
                connection_id: cid,
                turn_urls,
                turn_username,
                turn_password,
                ..
            }) if cid == connection_id => {
                turn_cfg = Some((turn_urls, turn_username, turn_password));
            }
            Command::V2(CommandV2::P2PCandidates {
                connection_id: cid,
                candidates,
                ..
            }) if cid == connection_id => {
                break candidates;
            }
            _ => {
                // ignore
            }
        }
    };

    let direct_udp = peer_candidates
        .iter()
        .filter(|c| {
            c.transport == P2PTransport::Udp
                && matches!(
                    c.candidate_type,
                    common::P2PCandidateType::Host | common::P2PCandidateType::Srflx
                )
        })
        .max_by_key(|c| c.priority)
        .ok_or_else(|| anyhow!("no UDP host/srflx candidate received"))?
        .addr
        .parse::<std::net::SocketAddr>()?;

    let relay_udp = peer_candidates
        .iter()
        .filter(|c| {
            c.transport == P2PTransport::Udp
                && matches!(c.candidate_type, common::P2PCandidateType::Relay)
        })
        .max_by_key(|c| c.priority)
        .map(|c| c.addr.parse::<std::net::SocketAddr>())
        .transpose()?;

    let socket = UdpSocket::bind("0.0.0.0:0").await?;

    let task_id = uuid::Uuid::new_v4().to_string();
    let inf = Command::V2(CommandV2::P2PInferenceRequest {
        connection_id,
        task_id: task_id.clone(),
        model: None,
        prompt: args.prompt,
        max_tokens: args.max_tokens,
        temperature: 0.7,
        top_k: 40,
        top_p: 0.9,
        repeat_penalty: 1.1,
        repeat_last_n: 64,
        min_keep: 0,
    });
    let pkt = udp_encode_command(&inf)?;
    let mut next_msg_id: u32 = 1;

    // Option C: handshake first (empty reliable packet), then inference. If anything times out -> TURN/UDP.
    let direct_result: Result<String> = async {
        p2p_udp_send_reliable(&socket, direct_udp, next_msg_id, &[]).await?;
        next_msg_id = next_msg_id.wrapping_add(1);

        p2p_udp_send_reliable(&socket, direct_udp, next_msg_id, &pkt).await?;
        next_msg_id = next_msg_id.wrapping_add(1);

        let mut out = String::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut inflight: std::collections::HashMap<u32, std::collections::HashMap<u16, Vec<u8>>> =
            std::collections::HashMap::new();
        loop {
            let (n, from) = timeout(Duration::from_secs(15), socket.recv_from(&mut buf)).await??;
            if from != direct_udp {
                continue;
            }

            let Some((flags, msg_id, frag_idx, frag_cnt)) = p2p_udp_parse_header(&buf[..n]) else {
                continue;
            };
            if (flags & P2P_UDP_FLAG_ACK) != 0 {
                continue;
            }

            p2p_udp_send_ack(&socket, from, msg_id).await;

            let payload = &buf[P2P_UDP_HEADER_LEN..n];
            let entry = inflight.entry(msg_id).or_default();
            entry.insert(frag_idx, payload.to_vec());
            let Some(full) = p2p_udp_try_reassemble(entry, frag_cnt) else {
                continue;
            };
            inflight.remove(&msg_id);

            let cmd = udp_decode_command(&full)?;
            match cmd {
                Command::V2(CommandV2::P2PInferenceChunk {
                    connection_id: cid,
                    task_id: tid,
                    delta,
                    done: _,
                    error,
                    ..
                }) if cid == connection_id && tid == task_id => {
                    if let Some(e) = error {
                        return Err(anyhow!("p2p inference error: {e}"));
                    }
                    out.push_str(&delta);
                    println!("direct_result: {}", delta);
                }
                Command::V2(CommandV2::P2PInferenceDone {
                    connection_id: cid,
                    task_id: tid,
                    ..
                }) if cid == connection_id && tid == task_id => {
                    break;
                }
                _ => {}
            }
        }
        Ok(out)
    }
    .await;

    let out = match direct_result {
        Ok(s) => s,
        Err(_e) => {
            let (turn_urls, username, password) =
                turn_cfg.ok_or_else(|| anyhow!("direct UDP failed and no TURN config received"))?;
            let turn_url = turn_urls
                .first()
                .ok_or_else(|| anyhow!("empty TURN url list"))?
                .clone();
            let peer_relay = relay_udp.ok_or_else(|| anyhow!("no UDP relay candidate received"))?;

            let (turn_sock, _relayed, realm, nonce) =
                turn_allocate_udp(&turn_url, &username, &password).await?;
            turn_create_permission(&turn_sock, peer_relay, &username, &password, &realm, &nonce)
                .await?;

            let mut inbox: std::collections::VecDeque<(std::net::SocketAddr, Vec<u8>)> =
                std::collections::VecDeque::new();
            turn_send_reliable_over_indication(
                &turn_sock,
                peer_relay,
                next_msg_id,
                &[],
                &mut inbox,
            )
            .await?;
            next_msg_id = next_msg_id.wrapping_add(1);
            turn_send_reliable_over_indication(
                &turn_sock,
                peer_relay,
                next_msg_id,
                &pkt,
                &mut inbox,
            )
            .await?;
            next_msg_id = next_msg_id.wrapping_add(1);

            let mut out = String::new();
            let mut inflight: std::collections::HashMap<
                u32,
                std::collections::HashMap<u16, Vec<u8>>,
            > = std::collections::HashMap::new();
            let mut buf = vec![0u8; 4096];
            loop {
                let (peer, data) = if let Some((p, d)) = inbox.pop_front() {
                    (p, d)
                } else {
                    let n = timeout(Duration::from_secs(15), turn_sock.recv(&mut buf)).await??;
                    turn_parse_data_indication(&buf[..n])
                        .ok_or_else(|| anyhow!("not a TURN Data Indication"))?
                };
                if peer != peer_relay {
                    continue;
                }
                let Some((flags, msg_id, frag_idx, frag_cnt)) = p2p_udp_parse_header(&data) else {
                    continue;
                };
                if (flags & P2P_UDP_FLAG_ACK) != 0 {
                    continue;
                }

                // ACK over TURN.
                let ack = p2p_udp_make_header(P2P_UDP_FLAG_ACK, msg_id, 0, 0);
                turn_send_indication(&turn_sock, peer_relay, &ack).await?;

                if data.len() < P2P_UDP_HEADER_LEN {
                    continue;
                }
                let payload = &data[P2P_UDP_HEADER_LEN..];
                let entry = inflight.entry(msg_id).or_default();
                entry.insert(frag_idx, payload.to_vec());
                let Some(full) = p2p_udp_try_reassemble(entry, frag_cnt) else {
                    continue;
                };
                inflight.remove(&msg_id);
                let cmd = udp_decode_command(&full)?;
                match cmd {
                    Command::V2(CommandV2::P2PInferenceChunk {
                        connection_id: cid,
                        task_id: tid,
                        delta,
                        done: _,
                        error,
                        ..
                    }) if cid == connection_id && tid == task_id => {
                        if let Some(e) = error {
                            return Err(anyhow!("p2p inference error: {e}"));
                        }
                        out.push_str(&delta);
                        println!("Received chunk: {}", delta);
                    }
                    Command::V2(CommandV2::P2PInferenceDone {
                        connection_id: cid,
                        task_id: tid,
                        ..
                    }) if cid == connection_id && tid == task_id => {
                        break;
                    }
                    _ => {}
                }
            }
            out
        }
    };

    println!("P2P output:\n{}", out);
    Ok(())
}
