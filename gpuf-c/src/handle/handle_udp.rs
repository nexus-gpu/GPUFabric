use super::*;
use bincode::{self as bincode, config as bincode_config};
use crc32fast::Hasher as Crc32;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::Sha256;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use url::Url;

use anyhow::{anyhow, Result};
use common::{Command, CommandV2, MAX_MESSAGE_SIZE};
use tracing::warn;

#[derive(Debug)]
pub(super) struct P2PReplayWindow {
    seen: HashSet<u64>,
    order: VecDeque<(u64, u64)>,
}

impl P2PReplayWindow {
    pub(super) fn new() -> Self {
        Self {
            seen: HashSet::new(),
            order: VecDeque::new(),
        }
    }

    fn prune(&mut self, now: u64) {
        while let Some((seq, ts)) = self.order.front().copied() {
            if now.saturating_sub(ts) <= ClientWorker::P2P_REPLAY_WINDOW_SECS {
                break;
            }
            self.order.pop_front();
            self.seen.remove(&seq);
        }
        while self.order.len() > ClientWorker::P2P_REPLAY_CACHE_LIMIT {
            if let Some((seq, _)) = self.order.pop_front() {
                self.seen.remove(&seq);
            }
        }
    }

    pub(super) fn accept_tcp_seq(&mut self, seq: u64, timestamp: u64, now: u64) -> bool {
        self.prune(now);
        if self.seen.contains(&seq) {
            return false;
        }
        self.seen.insert(seq);
        self.order.push_back((seq, timestamp));
        self.prune(now);
        true
    }
}

#[derive(Debug)]
struct P2PUdpPartialMessage {
    created_at: Instant,
    last_seen: Instant,
    frag_cnt: u16,
    bytes: usize,
    parts: HashMap<u16, Vec<u8>>,
}

#[derive(Debug)]
struct P2PUdpSourceState {
    window_started: Instant,
    bad_count: u32,
    banned_until: Option<Instant>,
}

#[derive(Debug)]
pub(super) struct P2PUdpReassemblyState {
    inflight: HashMap<(SocketAddr, u32), P2PUdpPartialMessage>,
    completed: HashMap<(SocketAddr, u32), Instant>,
    source_state: HashMap<IpAddr, P2PUdpSourceState>,
    total_bytes: usize,
}

impl P2PUdpReassemblyState {
    pub(super) fn new() -> Self {
        Self {
            inflight: HashMap::new(),
            completed: HashMap::new(),
            source_state: HashMap::new(),
            total_bytes: 0,
        }
    }

    fn prune(&mut self) {
        let now = Instant::now();
        let mut removed_bytes = 0usize;
        self.inflight.retain(|_, entry| {
            let keep = now.duration_since(entry.created_at) <= ClientWorker::P2P_REASSEMBLY_TTL;
            if !keep {
                removed_bytes = removed_bytes.saturating_add(entry.bytes);
            }
            keep
        });
        self.total_bytes = self.total_bytes.saturating_sub(removed_bytes);
        self.completed.retain(|_, seen_at| {
            now.duration_since(*seen_at)
                <= Duration::from_secs(ClientWorker::P2P_REPLAY_WINDOW_SECS)
        });
        self.source_state.retain(|_, state| {
            if let Some(until) = state.banned_until {
                until > now
            } else {
                now.duration_since(state.window_started) <= ClientWorker::P2P_SOURCE_BAN_TTL
                    || state.bad_count > 0
            }
        });
    }

    pub(super) fn is_source_banned(&mut self, from: SocketAddr) -> bool {
        self.prune();
        self.source_state
            .get(&from.ip())
            .and_then(|state| state.banned_until)
            .is_some_and(|until| until > Instant::now())
    }

    pub(super) fn record_invalid_source(&mut self, from: SocketAddr) {
        self.prune();
        let now = Instant::now();
        let state = self
            .source_state
            .entry(from.ip())
            .or_insert(P2PUdpSourceState {
                window_started: now,
                bad_count: 0,
                banned_until: None,
            });
        if state.banned_until.is_some_and(|until| until > now) {
            return;
        }
        if now.duration_since(state.window_started) > Duration::from_secs(10) {
            state.window_started = now;
            state.bad_count = 0;
        }
        state.bad_count = state.bad_count.saturating_add(1);
        if state.bad_count >= ClientWorker::P2P_SOURCE_BAN_THRESHOLD {
            state.banned_until = Some(now + ClientWorker::P2P_SOURCE_BAN_TTL);
            state.bad_count = 0;
            warn!(
                "P2P UDP source {} temporarily banned after invalid traffic",
                from.ip()
            );
        }
    }

    pub(super) fn accept_fragment(
        &mut self,
        from: SocketAddr,
        msg_id: u32,
        frag_idx: u16,
        frag_cnt: u16,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        self.prune();
        if frag_cnt == 0 || frag_idx >= frag_cnt {
            return Err(anyhow!("invalid p2p udp fragment metadata"));
        }
        if frag_cnt > ClientWorker::P2P_MAX_FRAGMENTS_PER_MESSAGE {
            return Err(anyhow!("p2p udp fragment count exceeds limit"));
        }
        if payload.len() > ClientWorker::P2P_UDP_MTU_PAYLOAD {
            return Err(anyhow!("p2p udp fragment payload too large"));
        }

        let key = (from, msg_id);
        if self.completed.contains_key(&key) {
            warn!("P2P UDP replayed completed msg_id={} from {}", msg_id, from);
            return Ok(None);
        }

        let is_new_message = !self.inflight.contains_key(&key);
        if is_new_message {
            if self.inflight.len() >= ClientWorker::P2P_MAX_INFLIGHT_MESSAGES {
                return Err(anyhow!("p2p udp inflight message limit exceeded"));
            }
            let source_messages = self
                .inflight
                .keys()
                .filter(|(addr, _)| addr.ip() == from.ip())
                .count();
            if source_messages >= ClientWorker::P2P_MAX_MESSAGES_PER_SOURCE {
                return Err(anyhow!("p2p udp per-source message limit exceeded"));
            }
        }

        let duplicate = self
            .inflight
            .get(&key)
            .is_some_and(|entry| entry.parts.contains_key(&frag_idx));
        if duplicate {
            return Ok(None);
        }

        if self.total_bytes.saturating_add(payload.len()) > ClientWorker::P2P_MAX_INFLIGHT_BYTES {
            return Err(anyhow!("p2p udp inflight byte limit exceeded"));
        }

        let now = Instant::now();
        let entry = self
            .inflight
            .entry(key)
            .or_insert_with(|| P2PUdpPartialMessage {
                created_at: now,
                last_seen: now,
                frag_cnt,
                bytes: 0,
                parts: HashMap::new(),
            });
        if entry.frag_cnt != frag_cnt {
            return Err(anyhow!("p2p udp fragment count changed within message"));
        }
        if entry.bytes.saturating_add(payload.len()) > MAX_MESSAGE_SIZE {
            return Err(anyhow!("p2p udp reassembled message exceeds max size"));
        }
        entry.parts.insert(frag_idx, payload.to_vec());
        entry.bytes = entry.bytes.saturating_add(payload.len());
        entry.last_seen = now;
        self.total_bytes = self.total_bytes.saturating_add(payload.len());

        if entry.parts.len() != frag_cnt as usize {
            return Ok(None);
        }

        let mut entry = self
            .inflight
            .remove(&key)
            .ok_or_else(|| anyhow!("p2p udp reassembly state disappeared"))?;
        self.total_bytes = self.total_bytes.saturating_sub(entry.bytes);
        let mut out = Vec::with_capacity(entry.bytes);
        for idx in 0..entry.frag_cnt {
            let part = entry
                .parts
                .remove(&idx)
                .ok_or_else(|| anyhow!("p2p udp missing fragment during reassembly"))?;
            out.extend_from_slice(&part);
        }
        self.completed.insert(key, now);
        Ok(Some(out))
    }
}

impl ClientWorker {
    pub(super) const P2P_UDP_MAGIC: [u8; 4] = *b"P2PU";
    pub(super) const P2P_UDP_VERSION: u8 = 2;
    pub(super) const P2P_UDP_FLAG_ACK: u8 = 0x01;
    pub(super) const P2P_UDP_HEADER_LEN: usize = 4 + 1 + 1 + 4 + 2 + 2 + 8 + 32;
    pub(super) const P2P_UDP_MTU_PAYLOAD: usize = 1200;
    pub(super) const P2P_REPLAY_WINDOW_SECS: u64 = 300;
    pub(super) const P2P_REPLAY_CACHE_LIMIT: usize = 4096;
    pub(super) const P2P_MAX_FRAGMENTS_PER_MESSAGE: u16 = 128;
    pub(super) const P2P_REASSEMBLY_TTL: Duration = Duration::from_secs(30);
    pub(super) const P2P_MAX_INFLIGHT_MESSAGES: usize = 128;
    pub(super) const P2P_MAX_INFLIGHT_BYTES: usize = 8 * 1024 * 1024;
    pub(super) const P2P_MAX_MESSAGES_PER_SOURCE: usize = 32;
    pub(super) const P2P_SOURCE_BAN_THRESHOLD: u32 = 64;
    pub(super) const P2P_SOURCE_BAN_TTL: Duration = Duration::from_secs(60);

    pub(super) fn p2p_now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub(super) fn p2p_udp_make_header(
        flags: u8,
        msg_id: u32,
        frag_idx: u16,
        frag_cnt: u16,
        timestamp: u64,
        tag: &[u8; 32],
    ) -> [u8; Self::P2P_UDP_HEADER_LEN] {
        let mut h = [0u8; Self::P2P_UDP_HEADER_LEN];
        h[0..4].copy_from_slice(&Self::P2P_UDP_MAGIC);
        h[4] = Self::P2P_UDP_VERSION;
        h[5] = flags;
        h[6..10].copy_from_slice(&msg_id.to_be_bytes());
        h[10..12].copy_from_slice(&frag_idx.to_be_bytes());
        h[12..14].copy_from_slice(&frag_cnt.to_be_bytes());
        h[14..22].copy_from_slice(&timestamp.to_be_bytes());
        h[22..54].copy_from_slice(tag);
        h
    }

    pub(super) fn p2p_udp_parse_header(buf: &[u8]) -> Option<(u8, u32, u16, u16, u64, [u8; 32])> {
        if buf.len() < Self::P2P_UDP_HEADER_LEN {
            return None;
        }
        if buf.get(0..4)? != Self::P2P_UDP_MAGIC {
            return None;
        }
        if buf[4] != Self::P2P_UDP_VERSION {
            return None;
        }
        let flags = buf[5];
        let msg_id = u32::from_be_bytes([buf[6], buf[7], buf[8], buf[9]]);
        let frag_idx = u16::from_be_bytes([buf[10], buf[11]]);
        let frag_cnt = u16::from_be_bytes([buf[12], buf[13]]);
        let timestamp = u64::from_be_bytes([
            buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20], buf[21],
        ]);
        let tag = buf[22..54].try_into().ok()?;
        Some((flags, msg_id, frag_idx, frag_cnt, timestamp, tag))
    }

    fn p2p_udp_mac_input(
        connection_id: &[u8; 16],
        flags: u8,
        msg_id: u32,
        frag_idx: u16,
        frag_cnt: u16,
        timestamp: u64,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(32 + 16 + 1 + 4 + 2 + 2 + 8 + payload.len());
        data.extend_from_slice(b"GPUF-P2P-UDP-V2");
        data.extend_from_slice(connection_id);
        data.push(flags);
        data.extend_from_slice(&msg_id.to_be_bytes());
        data.extend_from_slice(&frag_idx.to_be_bytes());
        data.extend_from_slice(&frag_cnt.to_be_bytes());
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(payload);
        data
    }

    pub(super) fn p2p_hmac_sha256(secret: &[u8; 32], data: &[u8]) -> [u8; 32] {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac sha256 key");
        mac.update(data);
        mac.finalize().into_bytes().into()
    }

    pub(super) fn p2p_udp_tag(
        secret: &[u8; 32],
        connection_id: &[u8; 16],
        flags: u8,
        msg_id: u32,
        frag_idx: u16,
        frag_cnt: u16,
        timestamp: u64,
        payload: &[u8],
    ) -> [u8; 32] {
        let data = Self::p2p_udp_mac_input(
            connection_id,
            flags,
            msg_id,
            frag_idx,
            frag_cnt,
            timestamp,
            payload,
        );
        Self::p2p_hmac_sha256(secret, &data)
    }

    pub(super) fn p2p_timestamp_is_fresh(timestamp: u64, now: u64) -> bool {
        timestamp <= now.saturating_add(30)
            && now.saturating_sub(timestamp) <= Self::P2P_REPLAY_WINDOW_SECS
    }

    pub(super) fn p2p_udp_validate_fragment(
        secret: &[u8; 32],
        connection_id: &[u8; 16],
        flags: u8,
        msg_id: u32,
        frag_idx: u16,
        frag_cnt: u16,
        timestamp: u64,
        payload: &[u8],
        tag: &[u8; 32],
        now: u64,
    ) -> Result<()> {
        let is_ack = (flags & Self::P2P_UDP_FLAG_ACK) != 0;
        if is_ack {
            if frag_idx != 0 || frag_cnt != 0 || !payload.is_empty() {
                return Err(anyhow!("invalid p2p udp ack metadata"));
            }
        } else {
            if frag_cnt == 0 || frag_cnt > Self::P2P_MAX_FRAGMENTS_PER_MESSAGE {
                return Err(anyhow!("invalid p2p udp fragment count"));
            }
            if frag_idx >= frag_cnt {
                return Err(anyhow!("invalid p2p udp fragment index"));
            }
        }
        if !Self::p2p_timestamp_is_fresh(timestamp, now) {
            return Err(anyhow!("stale p2p udp fragment"));
        }
        let expected = Self::p2p_udp_tag(
            secret,
            connection_id,
            flags,
            msg_id,
            frag_idx,
            frag_cnt,
            timestamp,
            payload,
        );
        if expected.as_slice() != tag.as_slice() {
            return Err(anyhow!("p2p udp fragment authentication failed"));
        }
        Ok(())
    }

    pub(super) async fn p2p_udp_send_ack(
        socket: &UdpSocket,
        to: SocketAddr,
        connection_id: [u8; 16],
        secret: [u8; 32],
        msg_id: u32,
    ) {
        let timestamp = Self::p2p_now_secs();
        let tag = Self::p2p_udp_tag(
            &secret,
            &connection_id,
            Self::P2P_UDP_FLAG_ACK,
            msg_id,
            0,
            0,
            timestamp,
            &[],
        );
        let hdr = Self::p2p_udp_make_header(Self::P2P_UDP_FLAG_ACK, msg_id, 0, 0, timestamp, &tag);
        let _ = socket.send_to(&hdr, to).await;
    }

    pub(super) fn p2p_udp_ack_packet(
        connection_id: [u8; 16],
        secret: [u8; 32],
        msg_id: u32,
    ) -> [u8; Self::P2P_UDP_HEADER_LEN] {
        let timestamp = Self::p2p_now_secs();
        let tag = Self::p2p_udp_tag(
            &secret,
            &connection_id,
            Self::P2P_UDP_FLAG_ACK,
            msg_id,
            0,
            0,
            timestamp,
            &[],
        );
        Self::p2p_udp_make_header(Self::P2P_UDP_FLAG_ACK, msg_id, 0, 0, timestamp, &tag)
    }

    pub(super) async fn p2p_udp_send_reliable(
        socket: &UdpSocket,
        to: SocketAddr,
        connection_id: [u8; 16],
        secret: [u8; 32],
        msg_id: u32,
        payload: &[u8],
    ) -> Result<()> {
        let max_payload = Self::P2P_UDP_MTU_PAYLOAD.saturating_sub(Self::P2P_UDP_HEADER_LEN);
        if max_payload == 0 {
            return Err(anyhow!("p2p udp mtu too small"));
        }
        let frag_cnt = ((payload.len() + max_payload - 1) / max_payload).max(1);
        if frag_cnt > Self::P2P_MAX_FRAGMENTS_PER_MESSAGE as usize {
            return Err(anyhow!("p2p udp too many fragments"));
        }

        for frag_idx in 0..frag_cnt {
            let start = frag_idx * max_payload;
            let end = ((frag_idx + 1) * max_payload).min(payload.len());
            let frag_payload = &payload[start..end];
            let timestamp = Self::p2p_now_secs();
            let tag = Self::p2p_udp_tag(
                &secret,
                &connection_id,
                0,
                msg_id,
                frag_idx as u16,
                frag_cnt as u16,
                timestamp,
                frag_payload,
            );
            let hdr = Self::p2p_udp_make_header(
                0,
                msg_id,
                frag_idx as u16,
                frag_cnt as u16,
                timestamp,
                &tag,
            );
            let mut pkt = Vec::with_capacity(Self::P2P_UDP_HEADER_LEN + frag_payload.len());
            pkt.extend_from_slice(&hdr);
            pkt.extend_from_slice(frag_payload);

            let mut tries = 0u32;
            loop {
                tries += 1;
                socket.send_to(&pkt, to).await?;

                let mut ack_buf = [0u8; Self::P2P_UDP_HEADER_LEN];
                let ack_res =
                    timeout(Duration::from_millis(400), socket.recv_from(&mut ack_buf)).await;
                if let Ok(Ok((n, from))) = ack_res {
                    if from != to {
                        continue;
                    }
                    if let Some((flags, ack_id, ack_frag_idx, ack_frag_cnt, ts, tag)) =
                        Self::p2p_udp_parse_header(&ack_buf[..n])
                    {
                        let valid_ack = (flags & Self::P2P_UDP_FLAG_ACK) != 0
                            && ack_id == msg_id
                            && ack_frag_idx == 0
                            && ack_frag_cnt == 0
                            && Self::p2p_udp_validate_fragment(
                                &secret,
                                &connection_id,
                                flags,
                                ack_id,
                                ack_frag_idx,
                                ack_frag_cnt,
                                ts,
                                &[],
                                &tag,
                                Self::p2p_now_secs(),
                            )
                            .is_ok();
                        if valid_ack {
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
        Self::udp_encode_command(command)
    }

    pub(super) fn p2p_udp_try_reassemble(
        parts: &mut HashMap<u16, Vec<u8>>,
        frag_cnt: u16,
    ) -> Option<Vec<u8>> {
        if frag_cnt == 0 || frag_cnt > Self::P2P_MAX_FRAGMENTS_PER_MESSAGE {
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

    pub(super) fn p2p_encode_data_plane_envelope(
        command: &Command,
        connection_id: [u8; 16],
        secret: [u8; 32],
        seq: u64,
        timestamp: u64,
    ) -> Result<Command> {
        let payload = bincode::encode_to_vec(command, bincode_config::standard())?;
        if payload.len() > MAX_MESSAGE_SIZE {
            return Err(anyhow!("p2p data-plane payload too large"));
        }
        let mut mac_input = Vec::with_capacity(32 + 16 + 8 + 8 + payload.len());
        mac_input.extend_from_slice(b"GPUF-P2P-TCP-V1");
        mac_input.extend_from_slice(&connection_id);
        mac_input.extend_from_slice(&seq.to_be_bytes());
        mac_input.extend_from_slice(&timestamp.to_be_bytes());
        mac_input.extend_from_slice(&payload);
        let tag = Self::p2p_hmac_sha256(&secret, &mac_input);
        Ok(Command::V2(CommandV2::P2PDataPlaneEnvelope {
            connection_id,
            seq,
            timestamp,
            payload,
            tag,
        }))
    }

    pub(super) fn p2p_decode_data_plane_envelope(
        command: Command,
        connection_id: [u8; 16],
        secret: [u8; 32],
        replay: &mut P2PReplayWindow,
    ) -> Result<Command> {
        let Command::V2(CommandV2::P2PDataPlaneEnvelope {
            connection_id: req_conn_id,
            seq,
            timestamp,
            payload,
            tag,
        }) = command
        else {
            return Err(anyhow!("expected signed p2p data-plane envelope"));
        };
        if req_conn_id != connection_id {
            return Err(anyhow!("p2p envelope connection mismatch"));
        }
        if payload.len() > MAX_MESSAGE_SIZE {
            return Err(anyhow!("p2p envelope payload too large"));
        }
        let now = Self::p2p_now_secs();
        if !Self::p2p_timestamp_is_fresh(timestamp, now) {
            return Err(anyhow!("stale p2p envelope"));
        }
        let mut mac_input = Vec::with_capacity(32 + 16 + 8 + 8 + payload.len());
        mac_input.extend_from_slice(b"GPUF-P2P-TCP-V1");
        mac_input.extend_from_slice(&connection_id);
        mac_input.extend_from_slice(&seq.to_be_bytes());
        mac_input.extend_from_slice(&timestamp.to_be_bytes());
        mac_input.extend_from_slice(&payload);
        let expected = Self::p2p_hmac_sha256(&secret, &mac_input);
        if expected != tag {
            return Err(anyhow!("p2p envelope authentication failed"));
        }
        if !replay.accept_tcp_seq(seq, timestamp, now) {
            return Err(anyhow!("replayed p2p envelope"));
        }
        let (inner, _) = bincode::decode_from_slice(&payload, bincode_config::standard())?;
        Ok(inner)
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
    pub(super) fn turn_encode_xor_peer_address(
        peer: std::net::SocketAddr,
        txid: &[u8; 12],
    ) -> Vec<u8> {
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
    pub(super) fn turn_parse_data_indication(
        msg: &[u8],
    ) -> Option<(std::net::SocketAddr, Vec<u8>)> {
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
        peer: SocketAddr,
        connection_id: [u8; 16],
        secret: [u8; 32],
        msg_id: u32,
        payload: &[u8],
        inbox: &mut VecDeque<(SocketAddr, Vec<u8>)>,
    ) -> Result<()> {
        let max_payload = Self::P2P_UDP_MTU_PAYLOAD.saturating_sub(Self::P2P_UDP_HEADER_LEN);
        if max_payload == 0 {
            return Err(anyhow!("p2p udp mtu too small"));
        }
        let frag_cnt = ((payload.len() + max_payload - 1) / max_payload).max(1);
        if frag_cnt > Self::P2P_MAX_FRAGMENTS_PER_MESSAGE as usize {
            return Err(anyhow!("p2p udp too many fragments"));
        }

        for frag_idx in 0..frag_cnt {
            let start = frag_idx * max_payload;
            let end = ((frag_idx + 1) * max_payload).min(payload.len());
            let frag_payload = &payload[start..end];
            let timestamp = Self::p2p_now_secs();
            let tag = Self::p2p_udp_tag(
                &secret,
                &connection_id,
                0,
                msg_id,
                frag_idx as u16,
                frag_cnt as u16,
                timestamp,
                frag_payload,
            );
            let hdr = Self::p2p_udp_make_header(
                0,
                msg_id,
                frag_idx as u16,
                frag_cnt as u16,
                timestamp,
                &tag,
            );
            let mut pkt = Vec::with_capacity(Self::P2P_UDP_HEADER_LEN + frag_payload.len());
            pkt.extend_from_slice(&hdr);
            pkt.extend_from_slice(frag_payload);

            let mut tries = 0u32;
            loop {
                tries += 1;
                Self::turn_send_indication(sock, peer, &pkt).await?;

                let mut buf = vec![0u8; 4096];
                let recv_res = timeout(Duration::from_millis(400), sock.recv(&mut buf)).await;
                if let Ok(Ok(n)) = recv_res {
                    if let Some((src, data)) = Self::turn_parse_data_indication(&buf[..n]) {
                        if src == peer {
                            if let Some((flags, ack_id, ack_frag_idx, ack_frag_cnt, ts, ack_tag)) =
                                Self::p2p_udp_parse_header(&data)
                            {
                                let valid_ack = (flags & Self::P2P_UDP_FLAG_ACK) != 0
                                    && ack_id == msg_id
                                    && ack_frag_idx == 0
                                    && ack_frag_cnt == 0
                                    && Self::p2p_udp_validate_fragment(
                                        &secret,
                                        &connection_id,
                                        flags,
                                        ack_id,
                                        ack_frag_idx,
                                        ack_frag_cnt,
                                        ts,
                                        &[],
                                        &ack_tag,
                                        Self::p2p_now_secs(),
                                    )
                                    .is_ok();
                                if valid_ack {
                                    break;
                                }
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

#[cfg(test)]
mod p2p_security_tests {
    use super::*;
    use common::OutputPhase;

    fn sample_command(connection_id: [u8; 16]) -> Command {
        Command::V2(CommandV2::P2PInferenceDone {
            connection_id,
            task_id: "task-1".to_string(),
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
            analysis_tokens: 0,
            final_tokens: 2,
        })
    }

    #[test]
    fn udp_fragment_auth_rejects_tamper_and_stale_timestamp() {
        let secret = [7u8; 32];
        let connection_id = [3u8; 16];
        let payload = b"payload";
        let now = ClientWorker::p2p_now_secs();
        let tag = ClientWorker::p2p_udp_tag(&secret, &connection_id, 0, 42, 0, 1, now, payload);

        assert!(ClientWorker::p2p_udp_validate_fragment(
            &secret,
            &connection_id,
            0,
            42,
            0,
            1,
            now,
            payload,
            &tag,
            now,
        )
        .is_ok());

        let mut tampered = *payload;
        tampered[0] ^= 1;
        assert!(ClientWorker::p2p_udp_validate_fragment(
            &secret,
            &connection_id,
            0,
            42,
            0,
            1,
            now,
            &tampered,
            &tag,
            now,
        )
        .is_err());

        assert!(ClientWorker::p2p_udp_validate_fragment(
            &secret,
            &connection_id,
            0,
            42,
            0,
            1,
            now.saturating_sub(ClientWorker::P2P_REPLAY_WINDOW_SECS + 1),
            payload,
            &tag,
            now,
        )
        .is_err());
    }

    #[test]
    fn tcp_envelope_rejects_replay_and_cross_connection() {
        let secret = [9u8; 32];
        let connection_id = [4u8; 16];
        let command = sample_command(connection_id);
        let signed = ClientWorker::p2p_encode_data_plane_envelope(
            &command,
            connection_id,
            secret,
            1,
            ClientWorker::p2p_now_secs(),
        )
        .unwrap();

        let mut replay = P2PReplayWindow::new();
        assert!(ClientWorker::p2p_decode_data_plane_envelope(
            signed.clone(),
            connection_id,
            secret,
            &mut replay,
        )
        .is_ok());
        assert!(ClientWorker::p2p_decode_data_plane_envelope(
            signed.clone(),
            connection_id,
            secret,
            &mut replay,
        )
        .is_err());

        let mut other_replay = P2PReplayWindow::new();
        assert!(ClientWorker::p2p_decode_data_plane_envelope(
            signed,
            [5u8; 16],
            secret,
            &mut other_replay,
        )
        .is_err());
    }

    #[test]
    fn udp_reassembly_enforces_fragment_limits_and_dedupes_completed_messages() {
        let mut state = P2PUdpReassemblyState::new();
        let from: SocketAddr = "127.0.0.1:9999".parse().unwrap();

        assert!(state
            .accept_fragment(
                from,
                1,
                0,
                ClientWorker::P2P_MAX_FRAGMENTS_PER_MESSAGE + 1,
                b"x",
            )
            .is_err());

        assert!(state
            .accept_fragment(from, 2, 1, 2, b"b")
            .unwrap()
            .is_none());
        let full = state.accept_fragment(from, 2, 0, 2, b"a").unwrap().unwrap();
        assert_eq!(full, b"ab");
        assert!(state
            .accept_fragment(from, 2, 0, 2, b"a")
            .unwrap()
            .is_none());
    }

    #[test]
    fn signed_payload_round_trip_preserves_command_shape() {
        let secret = [1u8; 32];
        let connection_id = [2u8; 16];
        let command = Command::V2(CommandV2::P2PInferenceChunk {
            connection_id,
            task_id: "task-1".to_string(),
            seq: 7,
            delta: "hello".to_string(),
            phase: OutputPhase::Final,
            done: false,
            error: None,
            analysis_tokens: 0,
            final_tokens: 1,
        });
        let signed = ClientWorker::p2p_encode_data_plane_envelope(
            &command,
            connection_id,
            secret,
            11,
            ClientWorker::p2p_now_secs(),
        )
        .unwrap();
        let decoded = ClientWorker::p2p_decode_data_plane_envelope(
            signed,
            connection_id,
            secret,
            &mut P2PReplayWindow::new(),
        )
        .unwrap();
        match decoded {
            Command::V2(CommandV2::P2PInferenceChunk { seq, delta, .. }) => {
                assert_eq!(seq, 7);
                assert_eq!(delta, "hello");
            }
            other => panic!("unexpected decoded command: {:?}", other),
        }
    }
}
