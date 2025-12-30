use super::*;
#[cfg(not(target_os = "macos"))]
// LLM engine is not available in lightweight Android version
#[cfg(not(target_os = "android"))]
use crate::llm_engine::{self, llama_engine::LlamaEngine};
use crate::util::system_info::{
    collect_device_info, collect_system_info, get_engine_models, pull_ollama_model, run_model,
};
use anyhow::Result;
use common::{
    format_bytes, format_duration, join_streams, read_command, write_command, Command, CommandV1,
    CommandV2, EngineType as ClientEngineType, Model, OsType, P2PCandidate, P2PCandidateType,
    P2PConnectionType, P2PTransport, SystemInfo, MAX_MESSAGE_SIZE,
};
use tokio::io::AsyncWriteExt;

use futures_util::StreamExt;

use bincode::{self as bincode, config as bincode_config};
use bytes::BytesMut;
use crc32fast::Hasher as Crc32;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::HashMap;
use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::BufReader;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::net::UdpSocket;
use tokio::time::interval;
use tokio::time::timeout;
#[cfg(not(target_os = "android"))]
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, ServerName},
        ClientConfig, RootCertStore,
    },
    TlsConnector,
};
use tracing::{debug, error, info, warn};
use url::Url;

// Filter internal GGUF control tokens from streaming output
fn filter_control_tokens(text: &str) -> String {
    // Skip everything that looks like internal thinking process
    if text.contains("analysis")
        || text.contains("The user is speaking")
        || text.contains("Means \"")
        || text.contains("The assistant should")
        || text.contains("We need to")
        || text.contains("Thus produce")
        || text.contains("Ok produce answer")
        || text.contains("<assistant")
        || text.contains("<|channel|>")
        || text.contains("<|start|>")
    {
        return String::new();
    }

    let mut result = String::new();
    let mut chars = text.chars().peekable();
    let mut buffer = String::new();

    while let Some(ch) = chars.next() {
        buffer.push(ch);

        // Check for any control token patterns
        if buffer.contains("<|") {
            // Skip until we find a safe point
            while let Some(c) = chars.next() {
                buffer.push(c);
                if buffer.ends_with(">") {
                    // Check if this was a control token
                    if buffer.contains("<|channel|>")
                        || buffer.contains("<|start|>")
                        || buffer.contains("<|end|>")
                        || buffer.contains("<|message|>")
                    {
                        buffer.clear();
                        break;
                    }
                    // If it's not a recognized control token, keep it
                    let safe_end = buffer.find('>').unwrap_or(buffer.len()) + 1;
                    result.push_str(&buffer[..safe_end]);
                    buffer.clear();
                    break;
                }
                if buffer.len() > 50 {
                    // Safety: prevent infinite growth
                    buffer.clear();
                    break;
                }
            }
            continue;
        }

        // Flush safe content periodically
        if buffer.len() > 20 {
            let safe_end = buffer.find('<').unwrap_or(buffer.len());
            if safe_end > 0 {
                result.push_str(&buffer[..safe_end]);
                buffer.drain(0..safe_end);
            }
        }
    }

    // Flush remaining buffer
    result.push_str(&buffer);

    // Final cleanup
    result
        .replace("<|end|>", "")
        .replace("<|start|>", "")
        .replace("<|channel|>", "")
        .replace("<|message|>", "")
}

fn derive_model_id_from_path(model_path: &str) -> String {
    let lower = model_path.to_ascii_lowercase();
    if lower.contains("llama-3") || lower.contains("llama3") {
        return "llama3".to_string();
    }

    let file_name = std::path::Path::new(model_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(model_path);

    file_name
        .trim_end_matches(".gguf")
        .trim_end_matches(".bin")
        .to_string()
}

const CURRENT_VERSION: u32 = 1;

impl ClientWorker {
    const P2P_UDP_MAGIC: [u8; 4] = *b"P2PU";
    const P2P_UDP_VERSION: u8 = 1;
    const P2P_UDP_FLAG_ACK: u8 = 0x01;
    const P2P_UDP_HEADER_LEN: usize = 4 + 1 + 1 + 4 + 2 + 2; // magic + version + flags + msg_id + frag_idx + frag_cnt
    const P2P_UDP_MTU_PAYLOAD: usize = 1200;

    fn p2p_udp_make_header(
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

    fn p2p_udp_parse_header(buf: &[u8]) -> Option<(u8, u32, u16, u16)> {
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

    async fn p2p_udp_send_ack(socket: &UdpSocket, to: std::net::SocketAddr, msg_id: u32) {
        let hdr = Self::p2p_udp_make_header(Self::P2P_UDP_FLAG_ACK, msg_id, 0, 0);
        let _ = socket.send_to(&hdr, to).await;
    }

    async fn p2p_udp_send_reliable(
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
                let ack_res =
                    timeout(Duration::from_millis(400), socket.recv_from(&mut ack_buf)).await;
                if let Ok(Ok((n, from))) = ack_res {
                    if from != to {
                        continue;
                    }
                    if let Some((flags, ack_id, _fi, _fc)) =
                        Self::p2p_udp_parse_header(&ack_buf[..n])
                    {
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

    fn p2p_udp_encode_command_payload(command: &Command) -> Result<Vec<u8>> {
        // payload uses same framing as udp_encode_command (len + bincode) so we can reuse decode.
        Self::udp_encode_command(command)
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
    /// Execute inference task using local LLM engine (Android specific)

    async fn execute_inference_task(
        &self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    ) -> Result<String> {
        #[cfg(not(target_os = "android"))]
        {
            let engine_guard = self.engine.lock().await;
            let engine = engine_guard
                .as_ref()
                .ok_or_else(|| anyhow!("Engine not initialized"))?;

            match engine {
                AnyEngine::Llama(llama) => {
                    let sampling = crate::llm_engine::llama_engine::SamplingParams {
                        temperature: temperature,
                        top_k: top_k as i32,
                        top_p: top_p,
                        repeat_penalty: repeat_penalty,
                        repeat_last_n: repeat_last_n,
                        seed: 0,
                        min_keep: min_keep as usize,
                    };

                    let (text, _prompt_tokens, _completion_tokens) = llama
                        .generate_with_cached_model_sampling(prompt, max_tokens as usize, &sampling)
                        .await?;
                    Ok(text)
                }

                _ => Err(anyhow!(
                    "execute_inference_task is only supported for LLAMA engine"
                )),
            }
        }

        #[cfg(target_os = "android")]
        {
            use crate::{
                gpuf_generate_final_solution_text, GLOBAL_CONTEXT_PTR, GLOBAL_INFERENCE_MUTEX,
                GLOBAL_MODEL_PTR,
            };
            use std::ffi::CString;
            use std::sync::atomic::Ordering;

            // Acquire global inference lock to prevent concurrent execution
            let _lock = GLOBAL_INFERENCE_MUTEX.lock().unwrap();

            // Get global model and context pointers
            let model_ptr = GLOBAL_MODEL_PTR.load(Ordering::SeqCst);
            let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);

            if model_ptr.is_null() || context_ptr.is_null() {
                return Err(anyhow!("Model not loaded - please load a model first"));
            }

            // Convert prompt to CString
            let prompt_cstr = CString::new(prompt).map_err(|e| anyhow!("Invalid prompt: {}", e))?;

            // Create output buffer
            let mut output = vec![0u8; 4096];

            // Execute inference using existing JNI function
            // SAFETY: We're calling an FFI function with valid pointers:
            // - model_ptr and context_ptr are checked for null above
            // - prompt_cstr.as_ptr() is a valid C string pointer
            // - output buffer is properly sized and mutable
            let result = gpuf_generate_final_solution_text(
                model_ptr,
                context_ptr,
                prompt_cstr.as_ptr(),
                max_tokens as i32,
                output.as_mut_ptr() as *mut std::os::raw::c_char,
                output.len() as i32,
            );

            if result > 0 {
                let output_str = unsafe {
                    std::ffi::CStr::from_ptr(output.as_ptr() as *const std::os::raw::c_char)
                        .to_str()
                        .map_err(|e| anyhow!("Invalid UTF-8 in output: {}", e))?
                };
                Ok(output_str.to_string())
            } else {
                Err(anyhow!("Inference failed with code: {}", result))
            }
        }
    }

    async fn stream_inference_task_to_server(
        &self,
        task_id: String,
        prompt: String,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    ) -> Result<()> {
        #[cfg(not(target_os = "android"))]
        {
            let engine_guard = self.engine.lock().await;
            let engine = engine_guard
                .as_ref()
                .ok_or_else(|| anyhow!("Engine not initialized"))?;

            let AnyEngine::Llama(llama) = engine else {
                return Err(anyhow!(
                    "stream_inference_task_to_server is only supported for LLAMA engine"
                ));
            };

            let sampling = crate::llm_engine::llama_engine::SamplingParams {
                temperature,
                top_k: top_k as i32,
                top_p,
                repeat_penalty,
                repeat_last_n,
                seed: 0,
                min_keep: min_keep as usize,
            };

            let prompt_tokens: u32 = {
                let prompt = prompt.clone();
                let cached_model = llama
                    .cached_model
                    .as_ref()
                    .ok_or_else(|| anyhow!("Model not loaded - call load_model() first"))?
                    .clone();

                tokio::task::spawn_blocking(move || {
                    use llama_cpp_2::model::AddBos;

                    let model_guard = cached_model
                        .lock()
                        .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;

                    let tokens = model_guard
                        .str_to_token(&prompt, AddBos::Always)
                        .map_err(|e| anyhow!("Failed to tokenize prompt: {:?}", e))?;
                    Ok::<u32, anyhow::Error>(tokens.len().min(u32::MAX as usize) as u32)
                })
                .await??
            };

            let mut stream = llama
                .stream_with_cached_model_sampling(&prompt, max_tokens as usize, &sampling)
                .await?;

            let mut stream = Box::pin(stream);

            let max_bytes: usize = self.args.stream_chunk_bytes.max(1);
            let mut seq: u32 = 0;
            let mut buf = String::new();
            let mut completion_tokens: u32 = 0;

            let mut cancelled_early = false;
            loop {
                {
                    let cancelled = self.cancel_state.cancelled.lock().await;
                    if cancelled.contains(&task_id) {
                        cancelled_early = true;
                        debug!(task_id = %task_id, "Cancellation observed in stream loop");
                        break;
                    }
                }

                tokio::select! {
                    _ = self.cancel_state.notify.notified() => {
                        let cancelled = self.cancel_state.cancelled.lock().await;
                        if cancelled.contains(&task_id) {
                            cancelled_early = true;
                            debug!(task_id = %task_id, "Cancellation notified during streaming");
                            break;
                        }
                    }
                    piece_res = stream.next() => {
                        let Some(piece_res) = piece_res else {
                            break;
                        };
                        let piece = piece_res?;
                        let filtered = filter_control_tokens(&piece);
                        // Each streamed `piece` corresponds to (at most) one generated token.
                        // Never count bytes/chars here, otherwise completion_tokens can greatly exceed max_tokens.
                        completion_tokens = completion_tokens.saturating_add(1);

                        if !filtered.is_empty() {
                            buf.push_str(&filtered);

                            if buf.len() >= max_bytes {
                                let delta = std::mem::take(&mut buf);
                                let chunk = CommandV1::InferenceResultChunk {
                                    task_id: task_id.clone(),
                                    seq,
                                    delta,
                                    done: false,
                                    error: None,
                                    prompt_tokens,
                                    completion_tokens,
                                };
                                self.send_command(chunk).await?;
                                seq = seq.wrapping_add(1);
                            }
                        }
                    }
                }
            }

            if !buf.is_empty() {
                let chunk = CommandV1::InferenceResultChunk {
                    task_id: task_id.clone(),
                    seq,
                    delta: buf,
                    done: false,
                    error: None,
                    prompt_tokens,
                    completion_tokens,
                };
                self.send_command(chunk).await?;
                seq = seq.wrapping_add(1);
            }

            let done_chunk = CommandV1::InferenceResultChunk {
                task_id: task_id.clone(),
                seq,
                delta: String::new(),
                done: true,
                error: None,
                prompt_tokens,
                completion_tokens,
            };
            self.send_command(done_chunk).await?;

            if cancelled_early {
                debug!(task_id = %task_id, "Sent done chunk after cancellation");
            }

            {
                let mut cancelled = self.cancel_state.cancelled.lock().await;
                cancelled.remove(&task_id);
            }
            return Ok(());
        }

        #[cfg(target_os = "android")]
        {
            let _ = (
                task_id,
                prompt,
                max_tokens,
                temperature,
                top_k,
                top_p,
                repeat_penalty,
                repeat_last_n,
                min_keep,
            );
            Err(anyhow!("Android streaming is not implemented"))
        }
    }

    fn build_chat_prompt_fallback(&self, messages: &[common::ChatMessage]) -> String {
        let template = std::env::var("CHAT_TEMPLATE").unwrap_or_else(|_| "simple".to_string());
        match template.to_ascii_lowercase().as_str() {
            "chatml" => {
                let mut prompt = String::new();
                for msg in messages {
                    prompt.push_str(&format!("{}\n{}\n", msg.role, msg.content));
                }
                prompt.push_str("\nassistant\n");
                prompt
            }
            "llama3" => {
                let mut prompt = String::from("<|begin_of_text|>");
                for msg in messages {
                    prompt.push_str(&format!(
                        "<|start_header_id|>{}\n\n{}\n<|eot_id|>",
                        msg.role, msg.content
                    ));
                }
                prompt.push_str("<|start_header_id|>assistant\n\n");
                prompt
            }
            _ => {
                let mut prompt = String::new();
                for msg in messages {
                    let role = match msg.role.as_str() {
                        "user" => "Human",
                        "assistant" => "Assistant",
                        _ => "System",
                    };
                    prompt.push_str(&format!("{}: {}\n\n", role, msg.content));
                }
                prompt.push_str("Assistant: ");
                prompt
            }
        }
    }

    /// Send command to server
    async fn send_command(&self, command: CommandV1) -> Result<()> {
        use common::{write_command, Command};

        let command = Command::V1(command);
        let mut writer = self.writer.lock().await;
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;
        Ok(())
    }

    async fn send_command_v2_on_writer(
        writer: Arc<Mutex<WriteHalf<TcpStream>>>,
        command: CommandV2,
    ) -> Result<()> {
        use common::{write_command, Command};

        let command = Command::V2(command);
        let mut w = writer.lock().await;
        write_command(&mut *w, &command).await?;
        w.flush().await?;
        Ok(())
    }

    fn parse_turns_url(url: &str) -> Result<(String, u16)> {
        let parsed = Url::parse(url).map_err(|e| anyhow!("Invalid TURN url: {e}"))?;
        if parsed.scheme() != "turns" {
            return Err(anyhow!("TURN url must be turns://"));
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow!("TURN url missing host"))?
            .to_string();
        let port = parsed.port().unwrap_or(5349);
        Ok((host, port))
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
        let connector = TlsConnector::from(Arc::new(config));
        let server_name =
            ServerName::try_from(host.to_string()).map_err(|_| anyhow!("Invalid SNI name"))?;
        Ok(connector.connect(server_name, stream).await?)
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
            buf.extend(std::iter::repeat_n(0u8, pad));
        }
    }

    fn stun_build_message(
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

    async fn stun_binding_srflx_on_socket(
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

    fn udp_encode_command(command: &Command) -> Result<Vec<u8>> {
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

    fn udp_decode_command(datagram: &[u8]) -> Result<Command> {
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

    #[cfg(not(target_os = "android"))]
    async fn turn_allocate_udp(
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
    async fn turn_send_indication(
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
    fn turn_parse_data_indication(msg: &[u8]) -> Option<(std::net::SocketAddr, Vec<u8>)> {
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
    async fn turn_send_reliable_over_indication(
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
            out.push((t, msg[pos..pos + l].to_vec()));
            pos += l;
            let pad = (4 - (l % 4)) % 4;
            pos += pad;
        }
        Ok(out)
    }

    fn stun_get_text_attr(attrs: &[(u16, Vec<u8>)], t: u16) -> Option<String> {
        attrs
            .iter()
            .find(|(k, _)| *k == t)
            .and_then(|(_, v)| String::from_utf8(v.clone()).ok())
    }

    fn stun_parse_xor_addr(v: &[u8], txid: &[u8; 12]) -> Option<std::net::SocketAddr> {
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
        let (host, port) = Self::parse_turns_url(turn_url)?;
        let mut tls = Self::turn_tls_connect(&host, port, cert_chain_path).await?;

        let txid = Self::stun_new_txid();
        let requested_transport_t: u16 = 0x0019;
        let lifetime_t: u16 = 0x000d;

        let mut attrs = Vec::new();
        attrs.push((&requested_transport_t, vec![6u8, 0, 0, 0]));
        attrs.push((&lifetime_t, 600u32.to_be_bytes().to_vec()));
        let req = Self::stun_build_message(0x0003, txid, &attrs, None, true);
        tls.write_all(&req).await?;
        tls.flush().await?;

        let resp = Self::stun_read_message(&mut tls).await?;
        let rx_txid: [u8; 12] = resp[8..20].try_into().unwrap_or([0u8; 12]);
        let attrs = Self::stun_attr_iter(&resp)?;

        // If 401, retry with long-term credentials.
        let msg_type = u16::from_be_bytes([resp[0], resp[1]]);
        if msg_type == 0x0113 {
            let realm = Self::stun_get_text_attr(&attrs, 0x0014)
                .ok_or_else(|| anyhow!("TURN missing REALM"))?;
            let nonce = Self::stun_get_text_attr(&attrs, 0x0015)
                .ok_or_else(|| anyhow!("TURN missing NONCE"))?;

            let username_t: u16 = 0x0006;
            let realm_t: u16 = 0x0014;
            let nonce_t: u16 = 0x0015;

            let txid2 = Self::stun_new_txid();
            let mut attrs2 = Vec::new();
            attrs2.push((&username_t, username.as_bytes().to_vec()));
            attrs2.push((&realm_t, realm.as_bytes().to_vec()));
            attrs2.push((&nonce_t, nonce.as_bytes().to_vec()));
            attrs2.push((&requested_transport_t, vec![6u8, 0, 0, 0]));
            attrs2.push((&lifetime_t, 600u32.to_be_bytes().to_vec()));

            let req2 = Self::stun_build_message(
                0x0003,
                txid2,
                &attrs2,
                Some((username, &realm, password)),
                true,
            );
            tls.write_all(&req2).await?;
            tls.flush().await?;

            let resp2 = Self::stun_read_message(&mut tls).await?;
            let txid_resp: [u8; 12] = resp2[8..20].try_into().unwrap_or([0u8; 12]);
            let attrs2 = Self::stun_attr_iter(&resp2)?;

            let msg_type2 = u16::from_be_bytes([resp2[0], resp2[1]]);
            if msg_type2 != 0x0103 {
                return Err(anyhow!("TURN Allocate failed (type=0x{:04x})", msg_type2));
            }

            let relayed = attrs2
                .iter()
                .find(|(t, _)| *t == 0x0016)
                .and_then(|(_, v)| Self::stun_parse_xor_addr(v, &txid_resp))
                .ok_or_else(|| anyhow!("TURN Allocate missing XOR-RELAYED-ADDRESS"))?;

            return Ok((tls, relayed, realm, nonce));
        }

        // Unexpected response.
        Err(anyhow!(
            "TURN Allocate unexpected response type=0x{:04x} txid={:?}",
            msg_type,
            rx_txid
        ))
    }

    #[cfg(not(target_os = "android"))]
    async fn turn_connect_peer(
        tls: &mut tokio_rustls::client::TlsStream<TcpStream>,
        peer: std::net::SocketAddr,
        username: &str,
        password: &str,
        realm: &str,
        nonce: &str,
    ) -> Result<Vec<u8>> {
        let username_t: u16 = 0x0006;
        let realm_t: u16 = 0x0014;
        let nonce_t: u16 = 0x0015;
        let xor_peer_t: u16 = 0x0012;

        let txid = Self::stun_new_txid();

        // XOR-PEER-ADDRESS value is encoded like XOR-MAPPED-ADDRESS, but for peer.
        let mut addr_val = Vec::new();
        addr_val.push(0);
        match peer {
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

        let req = Self::stun_build_message(
            0x000a,
            txid,
            &attrs,
            Some((username, realm, password)),
            true,
        );
        tls.write_all(&req).await?;
        tls.flush().await?;

        let resp = Self::stun_read_message(&mut *tls).await?;
        let msg_type = u16::from_be_bytes([resp[0], resp[1]]);
        if msg_type != 0x010a {
            return Err(anyhow!("TURN Connect failed (type=0x{:04x})", msg_type));
        }
        let attrs = Self::stun_attr_iter(&resp)?;
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
        let (host, port) = Self::parse_turns_url(turn_url)?;
        let mut tls = Self::turn_tls_connect(&host, port, cert_chain_path).await?;

        let username_t: u16 = 0x0006;
        let realm_t: u16 = 0x0014;
        let nonce_t: u16 = 0x0015;
        let conn_id_t: u16 = 0x002a;

        let txid = Self::stun_new_txid();
        let mut attrs = Vec::new();
        attrs.push((&username_t, username.as_bytes().to_vec()));
        attrs.push((&realm_t, realm.as_bytes().to_vec()));
        attrs.push((&nonce_t, nonce.as_bytes().to_vec()));
        attrs.push((&conn_id_t, conn_id.to_vec()));
        let req = Self::stun_build_message(
            0x000b,
            txid,
            &attrs,
            Some((username, realm, password)),
            true,
        );

        tls.write_all(&req).await?;
        tls.flush().await?;
        let resp = Self::stun_read_message(&mut tls).await?;
        let msg_type = u16::from_be_bytes([resp[0], resp[1]]);
        if msg_type != 0x010b {
            return Err(anyhow!(
                "TURN ConnectionBind failed (type=0x{:04x})",
                msg_type
            ));
        }
        Ok(tls)
    }

    async fn send_command_v2(&self, command: CommandV2) -> Result<()> {
        use common::{write_command, Command};

        let command = Command::V2(command);
        let mut writer = self.writer.lock().await;
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;
        Ok(())
    }

    fn parse_stun_host_port(url: &str) -> Option<(String, u16)> {
        let url = url.trim();
        let rest = url.strip_prefix("stun:").unwrap_or(url);
        let rest = rest.strip_prefix("//").unwrap_or(rest);
        let (host, port) = rest.rsplit_once(':')?;
        let port: u16 = port.parse().ok()?;
        Some((host.to_string(), port))
    }

    fn build_stun_binding_request() -> ([u8; 12], Vec<u8>) {
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

    fn parse_xor_mapped_address(resp: &[u8], txid: &[u8; 12]) -> Option<std::net::SocketAddr> {
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

    async fn stun_binding_srflx(stun_url: &str) -> Result<std::net::SocketAddr> {
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

    async fn detect_outbound_ip() -> Result<std::net::IpAddr> {
        // UDP "connect" doesn't send packets, but lets OS pick the outbound interface.
        // Then we can read the chosen local address.
        let sock = UdpSocket::bind("0.0.0.0:0").await?;
        sock.connect("1.1.1.1:80").await?;
        Ok(sock.local_addr()?.ip())
    }

    async fn get_advertise_ip(&self) -> Result<String> {
        if let Some(ip) = self.args.p2p_advertise_ip.as_deref() {
            return Ok(ip.to_string());
        }
        Ok(Self::detect_outbound_ip().await?.to_string())
    }

    #[cfg(not(target_os = "android"))]
    async fn execute_inference_task_with_engine(
        engine: Arc<Mutex<Option<AnyEngine>>>,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    ) -> Result<String> {
        let engine_guard = engine.lock().await;
        let engine = engine_guard
            .as_ref()
            .ok_or_else(|| anyhow!("Engine not initialized"))?;

        match engine {
            AnyEngine::Llama(llama) => {
                let sampling = crate::llm_engine::llama_engine::SamplingParams {
                    temperature,
                    top_k: top_k as i32,
                    top_p,
                    repeat_penalty,
                    repeat_last_n,
                    seed: 0,
                    min_keep: min_keep as usize,
                };

                let (text, _prompt_tokens, _completion_tokens) = llama
                    .generate_with_cached_model_sampling(prompt, max_tokens as usize, &sampling)
                    .await?;
                Ok(text)
            }
            _ => Err(anyhow!(
                "execute_inference_task is only supported for LLAMA engine"
            )),
        }
    }

    #[cfg(not(target_os = "android"))]
    async fn serve_p2p_io_with_engine<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
        engine: Arc<Mutex<Option<AnyEngine>>>,
        mut stream: S,
        connection_id: [u8; 16],
    ) -> Result<()> {
        let mut buf = BytesMut::with_capacity(MAX_MESSAGE_SIZE);
        loop {
            let cmd = read_command(&mut stream, &mut buf).await?;
            match cmd {
                Command::V2(CommandV2::P2PInferenceRequest {
                    connection_id: req_conn_id,
                    task_id,
                    model: _model,
                    prompt,
                    max_tokens,
                    temperature,
                    top_k,
                    top_p,
                    repeat_penalty,
                    repeat_last_n,
                    min_keep,
                }) => {
                    if req_conn_id != connection_id {
                        continue;
                    }

                    // Stream implementation: send multiple chunks (done=false) and finally a done message.
                    let engine_guard = engine.lock().await;
                    let engine_ref = engine_guard
                        .as_ref()
                        .ok_or_else(|| anyhow!("Engine not initialized"))?;

                    let AnyEngine::Llama(llama) = engine_ref else {
                        let chunk = Command::V2(CommandV2::P2PInferenceChunk {
                            connection_id,
                            task_id: task_id.clone(),
                            seq: 0,
                            delta: String::new(),
                            done: true,
                            error: Some(
                                "P2P streaming is only supported for LLAMA engine".to_string(),
                            ),
                        });
                        write_command(&mut stream, &chunk).await?;
                        stream.flush().await?;
                        continue;
                    };

                    let sampling = crate::llm_engine::llama_engine::SamplingParams {
                        temperature,
                        top_k: top_k as i32,
                        top_p,
                        repeat_penalty,
                        repeat_last_n,
                        seed: 0,
                        min_keep: min_keep as usize,
                    };

                    let token_stream = llama
                        .stream_with_cached_model_sampling(&prompt, max_tokens as usize, &sampling)
                        .await?;
                    let mut token_stream = Box::pin(token_stream);

                    let mut seq: u32 = 0;

                    while let Some(piece_res) = token_stream.next().await {
                        let piece = piece_res?;
                        let filtered = filter_control_tokens(&piece);
                        if filtered.is_empty() {
                            continue;
                        }

                        let mut start: usize = 0;
                        let max_bytes: usize = 64;
                        while start < filtered.len() {
                            let mut end = (start + max_bytes).min(filtered.len());
                            while end < filtered.len() && !filtered.is_char_boundary(end) {
                                end -= 1;
                            }
                            if end == start {
                                end = filtered
                                    .char_indices()
                                    .nth(1)
                                    .map(|(i, _)| i)
                                    .unwrap_or(filtered.len());
                            }
                            let delta = filtered[start..end].to_string();
                            start = end;

                            let chunk = Command::V2(CommandV2::P2PInferenceChunk {
                                connection_id,
                                task_id: task_id.clone(),
                                seq,
                                delta,
                                done: false,
                                error: None,
                            });
                            write_command(&mut stream, &chunk).await?;
                            stream.flush().await?;
                            seq = seq.wrapping_add(1);
                        }
                    }

                    let done = Command::V2(CommandV2::P2PInferenceDone {
                        connection_id,
                        task_id,
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    });
                    write_command(&mut stream, &done).await?;
                    stream.flush().await?;
                }

                Command::V2(CommandV2::P2PCancelInference {
                    connection_id: req_conn_id,
                    task_id: _,
                }) => {
                    if req_conn_id != connection_id {
                        continue;
                    }
                    // TODO: wire into cancel_state for actual cancellation.
                }

                _ => {}
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    async fn serve_p2p_stream_with_engine(
        engine: Arc<Mutex<Option<AnyEngine>>>,
        stream: TcpStream,
        connection_id: [u8; 16],
    ) -> Result<()> {
        Self::serve_p2p_io_with_engine(engine, stream, connection_id).await
    }

    #[cfg(not(target_os = "android"))]
    async fn turn_control_loop(
        mut tls: tokio_rustls::client::TlsStream<TcpStream>,
        turn_url: String,
        username: String,
        password: String,
        realm: String,
        nonce: String,
        cert_chain_path: String,
        engine: Arc<Mutex<Option<AnyEngine>>>,
        connection_id: [u8; 16],
    ) {
        loop {
            let msg = match Self::stun_read_message(&mut tls).await {
                Ok(m) => m,
                Err(e) => {
                    warn!("TURN control read failed: {}", e);
                    return;
                }
            };

            let msg_type = u16::from_be_bytes([msg[0], msg[1]]);
            // CONNECTION-ATTEMPT indication.
            if msg_type != 0x001c {
                continue;
            }

            let attrs = match Self::stun_attr_iter(&msg) {
                Ok(a) => a,
                Err(e) => {
                    warn!("TURN control parse failed: {}", e);
                    continue;
                }
            };

            let conn_id = match attrs
                .iter()
                .find(|(t, _)| *t == 0x002a)
                .map(|(_, v)| v.clone())
            {
                Some(v) => v,
                None => {
                    warn!("TURN CONNECTION-ATTEMPT missing CONNECTION-ID");
                    continue;
                }
            };

            match Self::turn_connection_bind(
                &turn_url,
                &conn_id,
                &username,
                &password,
                &realm,
                &nonce,
                &cert_chain_path,
            )
            .await
            {
                Ok(data_stream) => {
                    let engine = Arc::clone(&engine);
                    tokio::spawn(async move {
                        if let Err(e) =
                            TCPWorker::serve_p2p_io_with_engine(engine, data_stream, connection_id)
                                .await
                        {
                            error!("TURN data-plane stream error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    warn!("TURN ConnectionBind (incoming) failed: {}", e);
                }
            }
        }
    }
    pub async fn new(args: Args) -> Result<ClientWorker> {
        let (device_info, device_memtotal_mb) = match collect_device_info().await {
            Ok(info) => info,
            Err(e) => {
                error!("Failed to collect device info: {}", e);
                return Err(anyhow!("Failed to collect device info"));
            }
        };

        if device_info.num == 0 {
            error!(" device is empty");
            return Err(anyhow!(" device is empty"));
        }

        info!("Debug: Engine type from args: {:?}", args.engine_type);

        let os_type = if cfg!(target_os = "macos") {
            OsType::MACOS
        } else if cfg!(target_os = "windows") {
            OsType::WINDOWS
        } else if cfg!(target_os = "linux") {
            OsType::LINUX
        } else if cfg!(target_os = "android") {
            OsType::ANDROID
        } else if cfg!(target_os = "ios") {
            OsType::IOS
        } else {
            OsType::NONE
        };
        let engine_type = match args.engine_type {
            EngineType::VLLM => ClientEngineType::Vllm,
            EngineType::OLLAMA => ClientEngineType::Ollama,
            EngineType::LLAMA => ClientEngineType::Llama,
        };
        #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
        let mut engine: Option<AnyEngine> = None;
        #[cfg(any(target_os = "macos", target_os = "android"))]
        let mut engine: Option<()> = None;
        #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
        {
            if args.engine_type == EngineType::VLLM {
                let mut llvm_worker = llm_engine::create_engine(
                    args.engine_type.clone(),
                    args.hugging_face_hub_token.clone(),
                    args.chat_template_path.clone(),
                );
                match llvm_worker.init().await {
                    Ok(_) => info!("VLLM init success"),
                    Err(e) => error!("VLLM init failed: {}", e),
                }
                engine = Some(llvm_worker);
            } else if args.engine_type == EngineType::LLAMA {
                // Initialize LLAMA engine (single shared instance for both worker and HTTP server)
                let mut llama_worker = if let Some(model_path) = &args.llama_model_path {
                    // Use provided model path
                    info!("Creating LLAMA engine with model: {}", model_path);
                    llm_engine::AnyEngine::Llama(LlamaEngine::with_config(
                        model_path.clone(),
                        4096,              // context size
                        args.n_gpu_layers, // GPU layers
                    ))
                } else {
                    // Create engine without model (will be set later)
                    info!("Creating LLAMA engine without model (will be set later)");
                    llm_engine::create_engine(
                        args.engine_type.clone(),
                        args.hugging_face_hub_token.clone(),
                        args.chat_template_path.clone(),
                    )
                };

                // Initialize the engine (only once)
                match llama_worker.init().await {
                    Ok(_) => {
                        info!("LLAMA engine init success");
                        // Start worker
                        match llama_worker.start_worker().await {
                            Ok(_) => info!("LLAMA worker started"),
                            Err(e) => error!("LLAMA worker start failed: {}", e),
                        }
                    }
                    Err(e) => error!("LLAMA init failed: {}", e),
                }

                // Clone the engine for HTTP server (same instance, shared data via Arc)
                let server_engine = match llama_worker {
                    AnyEngine::Llama(ref e) => e.clone(),
                    _ => unreachable!(),
                };

                // Store engine for GPUFabric worker
                engine = Some(llama_worker);

                // Start local HTTP API server for LLAMA (for proxy forwarding)
                // Use the SAME engine instance for both worker and HTTP server
                let local_addr = args.local_addr.clone();
                let local_port = args.local_port;
                let local_addr_clone = local_addr.clone();
                info!(
                    "Starting LLAMA HTTP API server on {}:{}",
                    local_addr, local_port
                );

                use crate::llm_engine::llama_server::start_server;
                use std::sync::Arc;
                use tokio::sync::RwLock;

                // Wrap the shared engine in Arc<RwLock> for HTTP server
                let engine_arc = Arc::new(RwLock::new(server_engine));

                // Spawn server in background
                tokio::spawn(async move {
                    if let Err(e) = start_server(engine_arc, &local_addr_clone, local_port).await {
                        error!("LLAMA HTTP server error: {}", e);
                    }
                });

                info!(
                    "LLAMA HTTP API server started successfully on {}:{}",
                    local_addr, local_port
                );
            }
        }
        #[cfg(target_os = "macos")]
        {
            // if args.engine_type == EngineType::OLLAMA {
            //     let mut llvm_worker = llm_engine::create_engine(
            //         args.engine_type.clone(),
            //         args.hugging_face_hub_token.clone(),
            //         args.chat_template_path.clone(),
            //     );
            //     match llvm_worker.init().await {
            //         Ok(_) => info!("VLLM init success"),
            //         Err(e) => error!("VLLM init failed: {}", e),
            //     }
            //     engine = Some(llvm_worker);
            // }

            if args.engine_type == EngineType::OLLAMA {
                if let Err(e) = check_and_restart_ollama().await {
                    error!("Failed to manage Ollama process: {}", e);
                    // Decide whether to return error or continue without Ollama
                }
            }
        }
        let device_memtotal_gb = device_memtotal_mb as u32;
        let device_total_tflops = device_info.total_tflops as u32;

        let addr_str = format!("{}:{}", args.server_addr, args.control_port);
        let addr = addr_str.to_socket_addrs()?.next().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid server address or port",
            )
        })?;
        let ip_addr = addr.ip();
        let control_stream = TcpStream::connect(addr).await?;

        info!("Connected to control port.");

        let (reader, writer) = tokio::io::split(control_stream);
        //network monitor
        let network_monitor = Arc::new(Mutex::new(
            SessionNetworkMonitor::new(None).expect("Failed to create network monitor"),
        ));
        //system info
        let (cpu_useage, mem_useage, disk_useage, _computer_name) = collect_system_info().await?;

        let stats = network_monitor.lock().await.refresh().unwrap_or((0, 0));
        let worker = ClientWorker {
            addr: ip_addr,
            #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
            engine: Arc::new(Mutex::new(engine)),
            #[cfg(any(target_os = "macos", target_os = "android"))]
            _engine: PhantomData,
            //TODO: only one device
            devices_info: Arc::new(vec![device_info]),
            reader: Arc::new(Mutex::new(reader)),
            writer: Arc::new(Mutex::new(writer)),
            system_info: Arc::new(SystemInfo {
                cpu_usage: cpu_useage,
                memory_usage: mem_useage,
                disk_usage: disk_useage,
                network_rx: stats.0,
                network_tx: stats.1,
            }),
            client_id: args.client_id.expect("client_id is required"),
            device_memtotal_gb,
            device_total_tflops,
            os_type,
            engine_type,
            args,
            network_monitor,
            cancel_state: Arc::new(CancelState {
                cancelled: Mutex::new(HashSet::new()),
                notify: tokio::sync::Notify::new(),
            }),
        };
        Ok(worker)
    }

    #[allow(dead_code)]
    pub async fn system_info(&self) -> Result<SystemInfo> {
        let (cpu_useage, mem_useage, disk_useage, _computer_name) = collect_system_info().await?; //network info
        let mut network_info = self.network_monitor.lock().await;
        let stats = network_info.refresh().unwrap_or((0, 0));
        Ok(SystemInfo {
            cpu_usage: cpu_useage,
            memory_usage: mem_useage,
            disk_usage: disk_useage,
            network_rx: stats.0,
            network_tx: stats.1,
        })
    }
}

type TCPWorker = ClientWorker;

#[cfg(target_os = "macos")]
#[allow(dead_code)]
async fn check_and_restart_ollama() -> Result<()> {
    use std::process::Stdio;
    use tokio::process::Command;

    // Check if Ollama is running
    let check_status = Command::new("pgrep")
        .arg("Ollama")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match check_status {
        Ok(status) if status.success() => {
            // Ollama is running, kill it first
            info!("Ollama is running, restarting...");
            let _ = Command::new("pkill")
                .arg("Ollama")
                .status()
                .await
                .map_err(|e| {
                    error!("Failed to kill Ollama process: {}", e);
                    anyhow::anyhow!("Failed to kill Ollama process: {}", e)
                })?;

            // Give it a moment to shut down
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        _ => {
            info!("Ollama is not running, will start it");
        }
    }

    // Start Ollama with environment variables
    let mut cmd = Command::new("ollama");
    cmd.arg("serve")
        .env("OLLAMA_HOST", "0.0.0.0")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Start the process in the background
    cmd.spawn().map_err(|e| {
        error!("Failed to start Ollama: {}", e);
        anyhow::anyhow!("Failed to start Ollama: {}", e)
    })?;

    // Wait for Ollama to be ready
    let max_retries = 10;
    let mut retry_count = 0;
    let client = reqwest::Client::new();

    while retry_count < max_retries {
        match client
            .get("http://localhost:11434/api/tags")
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                info!("Ollama is ready");
                return Ok(());
            }
            Err(e) => {
                debug!(
                    "Ollama not ready yet (attempt {}/{}): {}",
                    retry_count + 1,
                    max_retries,
                    e
                );
            }
            _ => {}
        }

        retry_count += 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    Err(anyhow::anyhow!(
        "Failed to start Ollama: timeout after {} retries",
        max_retries
    ))
}

impl WorkerHandle for ClientWorker {
    fn login(&self) -> impl Future<Output = Result<()>> + Send {
        async move {
            info!("🔧 Starting login process...");
            let login_cmd = CommandV1::Login {
                version: CURRENT_VERSION,
                auto_models: self.args.auto_models,
                os_type: self.os_type.clone(),
                client_id: self.client_id.clone(),
                system_info: (*self.system_info).clone(),
                device_memtotal_gb: self.device_memtotal_gb,
                device_total_tflops: self.device_total_tflops,
                devices_info: self.devices_info.as_ref().clone(),
            };
            info!("📤 About to write login command to server...");
            match write_command(&mut *self.writer.lock().await, &Command::V1(login_cmd)).await {
                Ok(_) => {
                    info!("✅ Login command written successfully");
                    Ok(())
                }
                Err(e) => {
                    error!("❌ Failed to write login command: {}", e);
                    Err(e)
                }
            }
        }
    }

    fn model_task(&self, get_last_models: &str) -> impl Future<Output = Result<()>> + Send {
        async move {
            let writer_clone = Arc::clone(&self.writer);
            let client_id = Arc::new(self.client_id.clone());
            // let device_memtotal_gb = self.device_memtotal_gb;
            // let auto_models = self.args.auto_models;
            let engine_type = self.engine_type.clone();

            if self.args.auto_models {
                match engine_type {
                    common::EngineType::Ollama => {
                        pull_ollama_model(get_last_models, self.args.local_port).await?
                    }
                    common::EngineType::Vllm => {
                        #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
                        if let Some(_engine) = self.engine.lock().await.as_mut() {
                            // Engine functionality disabled in lightweight version
                        }
                    }
                    _ => {}
                }
            }
            let local_port = self.args.local_port;
            let devices_info = self.devices_info.clone();
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_secs(300)); // Send heartbeat every 10 seconds
                loop {
                    interval.tick().await;

                    let models: Vec<common::Model> = match engine_type {
                        common::EngineType::Ollama => match get_engine_models(local_port).await {
                            Ok(models) => {
                                info!("Successfully fetched {} models from Ollama.", models.len());
                                models
                            }
                            Err(e) => {
                                warn!(
                                    "Could not fetch models from Ollama: {}. This is okay if Ollama is not running.",
                                    e
                                );
                                Vec::new()
                            }
                        },
                        common::EngineType::Llama => {
                            let current_model_path = crate::MODEL_STATUS
                                .lock()
                                .ok()
                                .and_then(|s| s.current_model.clone());

                            match current_model_path {
                                Some(model_path) => {
                                    let model_id = derive_model_id_from_path(&model_path);
                                    vec![Model {
                                        id: model_id,
                                        object: "model".to_string(),
                                        created: 0,
                                        owned_by: "gpuf-c".to_string(),
                                    }]
                                }
                                None => Vec::new(),
                            }
                        }
                        _ => Vec::new(),
                    };
                    let model_cmd = CommandV1::ModelStatus {
                        client_id: *client_id,
                        models,
                        auto_models_device: devices_info.clone().to_vec(),
                    };
                    if let Err(e) =
                        write_command(&mut *writer_clone.lock().await, &Command::V1(model_cmd))
                            .await
                    {
                        error!(
                            "Failed to send model status (connection may be closed): {}",
                            e
                        );
                        break;
                    }
                }
            });
            Ok(())
        }
    }

    fn heartbeat_task(&self) -> impl Future<Output = Result<()>> + Send {
        async move {
            let writer_clone = Arc::clone(&self.writer);
            let client_id = Arc::new(self.client_id.clone());
            let network_monitor = Arc::clone(&self.network_monitor);
            // network_monitor.lock().await.update();
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_secs(120)); // Send heartbeat every 120 seconds

                loop {
                    interval.tick().await;

                    let (cpu_usage, memory_usage, disk_usage, _computer_name) =
                        match collect_system_info().await {
                            Ok(info) => info,
                            Err(e) => {
                                error!("Failed to collect system info: {}", e);
                                continue;
                            }
                        };

                    // device_info should be real-time for monitoring
                    let (device_info, device_memtotal_mb) = match collect_device_info().await {
                        Ok(info) => info,
                        Err(e) => {
                            error!("Failed to collect device info: {}", e);
                            (DevicesInfo::default(), 0)
                        }
                    };

                    // TODO: device_info is remote device info
                    info!("heartbeat: cpu_usage {}% memory_usage {}% disk_usage {}% device_memtotal {}mb", cpu_usage, memory_usage, disk_usage, device_memtotal_mb);

                    let (stats, session_stats) = {
                        let mut monitor = network_monitor.lock().await;
                        let stats = monitor.refresh().unwrap_or((0, 0));
                        let session_stats = monitor.get_session_stats();
                        (stats, session_stats)
                    };
                    info!(
                        "network_stats: up {} down {} | session_total: up {} down {} | duration: {} ", 
                        format_bytes!(stats.1),
                        format_bytes!(stats.0),
                        format_bytes!(session_stats.1),
                        format_bytes!(session_stats.0),
                        format_duration!(session_stats.2.as_secs())
                    );

                    let mut writer = writer_clone.lock().await;
                    if let Err(e) = write_command(
                        &mut *writer,
                        &Command::V1(CommandV1::Heartbeat {
                            client_id: *client_id,
                            system_info: SystemInfo {
                                cpu_usage: cpu_usage,
                                memory_usage: memory_usage,
                                disk_usage: disk_usage,
                                network_rx: stats.0,
                                network_tx: stats.1,
                            },
                            // TODO: devices_info device_count device_total_tflops and device_memtotal_gb is single device
                            device_memtotal_gb: device_info.memtotal_gb as u32,
                            device_total_tflops: device_info.total_tflops as u32,
                            device_count: device_info.num as u16,
                            devices_info: vec![device_info],
                        }),
                    )
                    .await
                    {
                        error!("Failed to send heartbeat: {}", e);
                        break;
                    }
                }
            });
            Ok(())
        }
    }

    fn handler(&self) -> impl Future<Output = Result<()>> + Send {
        async move {
            let mut buf = BytesMut::with_capacity(MAX_MESSAGE_SIZE);
            let mut p2p_turn_config: HashMap<[u8; 16], (Vec<String>, String, String, String)> =
                HashMap::new();
            // (turn_urls, username, password, peer_id as hex) - peer_id used only for debugging/selection
            loop {
                match read_command(&mut *self.reader.lock().await, &mut buf).await? {
                    Command::V1(cmd_v1) => {
                        match cmd_v1 {
                            CommandV1::CancelInference { task_id } => {
                                debug!(task_id = %task_id, "Received CancelInference");
                                {
                                    let mut cancelled = self.cancel_state.cancelled.lock().await;
                                    cancelled.insert(task_id);
                                }
                                self.cancel_state.notify.notify_waiters();
                            }
                            CommandV1::LoginResult {
                                success,
                                pods_model,
                                error,
                            } => {
                                if success {
                                    if pods_model.is_empty() {
                                        error!("Received empty models from server");
                                        return Err(anyhow!(
                                            "device is not compatible with the model"
                                        ));
                                    }
                                    //TODO models is local models
                                    for pod_model in pods_model {
                                        if let Some(model_name) = pod_model.model_name {
                                            self.model_task(&model_name).await?;
                                        }
                                    }
                                    self.heartbeat_task().await?;
                                    debug!("Successfully logged in.");
                                    continue;
                                } else {
                                    error!("Login failed: {}", error.unwrap_or_default());
                                    return Err(anyhow!("Login failed"));
                                }
                            }
                            CommandV1::PullModelResult { pods_model, error } => {
                                if error.is_some() {
                                    error!("Pull model failed: {}", error.unwrap_or_default());
                                    return Err(anyhow!("Pull model failed"));
                                }
                                if pods_model.is_empty() {
                                    error!("device is not compatible with the model");
                                    return Err(anyhow!("device is not compatible with the model"));
                                }
                                // TODO: pull model
                                for pod_model in pods_model {
                                    if let Some(model_name) = pod_model.model_name {
                                        match self.engine_type {
                                            common::EngineType::Ollama => {
                                                pull_ollama_model(&model_name, self.args.local_port)
                                                    .await?
                                            }
                                            common::EngineType::Vllm => {
                                                #[cfg(all(
                                                    not(target_os = "macos"),
                                                    not(target_os = "android")
                                                ))]
                                                if let Some(_engine) =
                                                    self.engine.lock().await.as_mut()
                                                {
                                                    // Engine functionality disabled in lightweight version
                                                }
                                            }
                                            _ => {}
                                        }
                                        match run_model(
                                            self.args.local_port,
                                            &model_name,
                                            "hello world",
                                        )
                                        .await
                                        {
                                            Ok(output) => {
                                                info!("Model {} output: {}", model_name, output)
                                            }
                                            Err(e) => error!("run_model Error: {}", e),
                                        }
                                    }
                                }
                            }
                            CommandV1::RequestNewProxyConn { proxy_conn_id } => {
                                info!(
                                    "Received request for new proxy connection: {:?}",
                                    proxy_conn_id
                                );
                                let args_clone = self.args.clone();
                                let cert_chain_path_clone = self.args.cert_chain_path.clone();
                                let addr_clone = self.addr;
                                tokio::spawn(async move {
                                    if let Err(e) = create_proxy_connection(
                                        args_clone,
                                        addr_clone,
                                        proxy_conn_id,
                                        cert_chain_path_clone,
                                    )
                                    .await
                                    {
                                        error!("Failed to create proxy connection: {}", e);
                                    }
                                });
                            }
                            CommandV1::ChatInferenceTask {
                                task_id,
                                model: _model,
                                messages,
                                max_tokens,
                                temperature,
                                top_k,
                                top_p,
                                repeat_penalty,
                                repeat_last_n,
                                min_keep,
                            } => {
                                let prompt = {
                                    #[cfg(target_os = "android")]
                                    {
                                        self.build_chat_prompt_fallback(&messages)
                                    }

                                    #[cfg(not(target_os = "android"))]
                                    {
                                        let cached_model = {
                                            let engine_guard = self.engine.lock().await;
                                            let engine = engine_guard
                                                .as_ref()
                                                .ok_or_else(|| anyhow!("Engine not initialized"))?;

                                            let AnyEngine::Llama(llama) = engine else {
                                                return Err(anyhow!(
                                                    "ChatInferenceTask is only supported for LLAMA engine"
                                                ));
                                            };

                                            llama.cached_model
                                                .as_ref()
                                                .ok_or_else(|| {
                                                    anyhow!("Model not loaded - call load_model() first")
                                                })?
                                                .clone()
                                        };

                                        let messages_for_fallback = messages.clone();
                                        match tokio::task::spawn_blocking(
                                            move || -> anyhow::Result<String> {
                                                use llama_cpp_2::model::LlamaChatMessage;

                                                let model_guard =
                                                    cached_model.lock().map_err(|e| {
                                                        anyhow!("Failed to lock model: {:?}", e)
                                                    })?;

                                                let tmpl = model_guard
                                                    .chat_template(None)
                                                    .map_err(|e| {
                                                        anyhow!(
                                                            "Failed to get chat template: {:?}",
                                                            e
                                                        )
                                                    })?;

                                                let mut chat =
                                                    Vec::with_capacity(messages_for_fallback.len());
                                                for m in messages_for_fallback {
                                                    let msg =
                                                        LlamaChatMessage::new(m.role, m.content)
                                                            .map_err(|e| {
                                                                anyhow!(
                                                            "Failed to build chat message: {:?}",
                                                            e
                                                        )
                                                            })?;
                                                    chat.push(msg);
                                                }

                                                model_guard
                                                    .apply_chat_template(&tmpl, &chat, true)
                                                    .map_err(|e| {
                                                        anyhow!(
                                                            "Failed to apply chat template: {:?}",
                                                            e
                                                        )
                                                    })
                                            },
                                        )
                                        .await
                                        {
                                            Ok(Ok(p)) => p,
                                            _ => self.build_chat_prompt_fallback(&messages),
                                        }
                                    }
                                };
                                let result = self
                                    .stream_inference_task_to_server(
                                        task_id.clone(),
                                        prompt,
                                        max_tokens,
                                        temperature,
                                        top_k,
                                        top_p,
                                        repeat_penalty,
                                        repeat_last_n,
                                        min_keep,
                                    )
                                    .await;

                                if let Err(e) = result {
                                    let chunk = CommandV1::InferenceResultChunk {
                                        task_id,
                                        seq: 0,
                                        delta: String::new(),
                                        done: true,
                                        completion_tokens: 0,
                                        prompt_tokens: 0,
                                        error: Some(e.to_string()),
                                    };
                                    self.send_command(chunk).await?;
                                }
                            }
                            CommandV1::InferenceTask {
                                task_id,
                                prompt,
                                max_tokens,
                                temperature,
                                top_k,
                                top_p,
                                repeat_penalty,
                                repeat_last_n,
                                min_keep,
                            } => {
                                info!(
                                    "Received inference task: {} max_tokens: {}",
                                    task_id, max_tokens
                                );

                                let start_time = std::time::Instant::now();

                                #[cfg(not(target_os = "android"))]
                                {
                                    let result = self
                                        .stream_inference_task_to_server(
                                            task_id.clone(),
                                            prompt.clone(),
                                            max_tokens,
                                            temperature,
                                            top_k,
                                            top_p,
                                            repeat_penalty,
                                            repeat_last_n,
                                            min_keep,
                                        )
                                        .await;

                                    let _execution_time = start_time.elapsed().as_millis() as u64;
                                    if let Err(e) = result {
                                        let chunk = CommandV1::InferenceResultChunk {
                                            task_id,
                                            seq: 0,
                                            delta: String::new(),
                                            done: true,
                                            completion_tokens: 0,
                                            prompt_tokens: 0,
                                            error: Some(e.to_string()),
                                        };
                                        self.send_command(chunk).await?;
                                    }
                                }

                                #[cfg(target_os = "android")]
                                {
                                    let result = self
                                        .execute_inference_task(
                                            &prompt,
                                            max_tokens,
                                            temperature,
                                            top_k,
                                            top_p,
                                            repeat_penalty,
                                            repeat_last_n,
                                            min_keep,
                                        )
                                        .await;

                                    let _execution_time = start_time.elapsed().as_millis() as u64;

                                    match result {
                                        Ok(output) => {
                                            let mut seq: u32 = 0;
                                            let max_bytes: usize =
                                                self.args.stream_chunk_bytes.max(1);
                                            let mut start: usize = 0;
                                            while start < output.len() {
                                                let mut end = (start + max_bytes).min(output.len());
                                                while end < output.len()
                                                    && !output.is_char_boundary(end)
                                                {
                                                    end -= 1;
                                                }
                                                if end == start {
                                                    end = output
                                                        .char_indices()
                                                        .nth(1)
                                                        .map(|(i, _)| i)
                                                        .unwrap_or(output.len());
                                                }

                                                let delta = output[start..end].to_string();
                                                let chunk = CommandV1::InferenceResultChunk {
                                                    task_id: task_id.clone(),
                                                    seq,
                                                    delta,
                                                    done: false,
                                                    error: None,
                                                    prompt_tokens: 0,
                                                    completion_tokens: 0,
                                                };
                                                self.send_command(chunk).await?;
                                                seq = seq.wrapping_add(1);
                                                start = end;
                                            }

                                            let done_chunk = CommandV1::InferenceResultChunk {
                                                task_id,
                                                seq,
                                                delta: String::new(),
                                                done: true,
                                                error: None,
                                                prompt_tokens: 0,
                                                completion_tokens: 0,
                                            };
                                            self.send_command(done_chunk).await?;
                                        }
                                        Err(e) => {
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id,
                                                seq: 0,
                                                delta: String::new(),
                                                done: true,
                                                error: Some(e.to_string()),
                                                prompt_tokens: 0,
                                                completion_tokens: 0,
                                            };
                                            self.send_command(chunk).await?;
                                        }
                                    }
                                }
                            }
                            _ => {
                                warn!("Received unexpected CommandV1: {:?}", cmd_v1);
                            }
                        }
                    }
                    Command::V2(cmd_v2) => {
                        match cmd_v2 {
                            CommandV2::P2PConnectionConfig {
                                peer_id,
                                connection_id,
                                stun_urls,
                                turn_urls,
                                turn_username,
                                turn_password,
                                expires_at: _,
                                force_tls: _,
                            } => {
                                p2p_turn_config.insert(
                                    connection_id,
                                    (
                                        turn_urls.clone(),
                                        turn_username.clone(),
                                        turn_password.clone(),
                                        hex::encode(peer_id),
                                    ),
                                );

                                // Mode 1: gpuf-c acts as server for P2P data-plane.
                                // Start a UDP socket for P2P data-plane.
                                let bind_addr = format!("0.0.0.0:{}", self.args.p2p_udp_port);
                                let socket = match UdpSocket::bind(&bind_addr).await {
                                    Ok(s) => Arc::new(s),
                                    Err(e) => {
                                        warn!("P2P UDP bind failed on {}: {}. Falling back to random port.", bind_addr, e);
                                        Arc::new(UdpSocket::bind("0.0.0.0:0").await?)
                                    }
                                };
                                let local_port = socket.local_addr()?.port();
                                let advertise_ip = self.get_advertise_ip().await?;

                                #[cfg(not(target_os = "android"))]
                                {
                                    let engine = Arc::clone(&self.engine);
                                    let socket = Arc::clone(&socket);
                                    tokio::spawn(async move {
                                        let mut next_msg_id: u32 = 1;
                                        let mut inflight: std::collections::HashMap<
                                            u32,
                                            std::collections::HashMap<u16, Vec<u8>>,
                                        > = std::collections::HashMap::new();
                                        let mut buf = vec![0u8; 64 * 1024];
                                        loop {
                                            let (n, from) = match socket.recv_from(&mut buf).await {
                                                Ok(v) => v,
                                                Err(e) => {
                                                    error!("P2P UDP recv error: {}", e);
                                                    return;
                                                }
                                            };

                                            let Some((flags, msg_id, frag_idx, frag_cnt)) =
                                                TCPWorker::p2p_udp_parse_header(&buf[..n])
                                            else {
                                                continue;
                                            };

                                            if (flags & TCPWorker::P2P_UDP_FLAG_ACK) != 0 {
                                                // acks are consumed by sender path
                                                continue;
                                            }

                                            // For now, ACK every fragment with msg_id.
                                            TCPWorker::p2p_udp_send_ack(&socket, from, msg_id)
                                                .await;

                                            let payload = &buf[TCPWorker::P2P_UDP_HEADER_LEN..n];
                                            let entry = inflight.entry(msg_id).or_default();
                                            entry.insert(frag_idx, payload.to_vec());
                                            let Some(full) =
                                                TCPWorker::p2p_udp_try_reassemble(entry, frag_cnt)
                                            else {
                                                continue;
                                            };
                                            inflight.remove(&msg_id);

                                            let cmd = match TCPWorker::udp_decode_command(&full) {
                                                Ok(c) => c,
                                                Err(e) => {
                                                    warn!("P2P UDP decode failed: {}", e);
                                                    continue;
                                                }
                                            };

                                            let Command::V2(CommandV2::P2PInferenceRequest {
                                                connection_id: req_conn_id,
                                                task_id,
                                                model: _model,
                                                prompt,
                                                max_tokens,
                                                temperature,
                                                top_k,
                                                top_p,
                                                repeat_penalty,
                                                repeat_last_n,
                                                min_keep,
                                            }) = cmd
                                            else {
                                                continue;
                                            };

                                            if req_conn_id != connection_id {
                                                continue;
                                            }

                                            // Stream inference over UDP data-plane.
                                            let sampling =
                                                crate::llm_engine::llama_engine::SamplingParams {
                                                    temperature,
                                                    top_k: top_k as i32,
                                                    top_p,
                                                    repeat_penalty,
                                                    repeat_last_n,
                                                    seed: 0,
                                                    min_keep: min_keep as usize,
                                                };

                                            let token_stream_res = {
                                                let engine_guard = engine.lock().await;
                                                let engine_ref = match engine_guard.as_ref() {
                                                    Some(v) => v,
                                                    None => {
                                                        let chunk = Command::V2(
                                                            CommandV2::P2PInferenceChunk {
                                                                connection_id,
                                                                task_id: task_id.clone(),
                                                                seq: 0,
                                                                delta: String::new(),
                                                                done: true,
                                                                error: Some(
                                                                    "Engine not initialized"
                                                                        .to_string(),
                                                                ),
                                                            },
                                                        );
                                                        if let Ok(pkt) = TCPWorker::p2p_udp_encode_command_payload(&chunk) {
                                                            let msg_id = next_msg_id;
                                                            next_msg_id = next_msg_id.wrapping_add(1);
                                                            let _ = TCPWorker::p2p_udp_send_reliable(&socket, from, msg_id, &pkt).await;
                                                        }
                                                        continue;
                                                    }
                                                };

                                                let AnyEngine::Llama(llama) = engine_ref else {
                                                    let chunk = Command::V2(CommandV2::P2PInferenceChunk {
                                                        connection_id,
                                                        task_id: task_id.clone(),
                                                        seq: 0,
                                                        delta: String::new(),
                                                        done: true,
                                                        error: Some("P2P UDP streaming is only supported for LLAMA engine".to_string()),
                                                    });
                                                    if let Ok(pkt) =
                                                        TCPWorker::p2p_udp_encode_command_payload(
                                                            &chunk,
                                                        )
                                                    {
                                                        let msg_id = next_msg_id;
                                                        next_msg_id = next_msg_id.wrapping_add(1);
                                                        let _ = TCPWorker::p2p_udp_send_reliable(
                                                            &socket, from, msg_id, &pkt,
                                                        )
                                                        .await;
                                                    }
                                                    continue;
                                                };

                                                llama
                                                    .stream_with_cached_model_sampling(
                                                        &prompt,
                                                        max_tokens as usize,
                                                        &sampling,
                                                    )
                                                    .await
                                            };

                                            let token_stream = match token_stream_res {
                                                Ok(s) => s,
                                                Err(e) => {
                                                    let chunk =
                                                        Command::V2(CommandV2::P2PInferenceChunk {
                                                            connection_id,
                                                            task_id: task_id.clone(),
                                                            seq: 0,
                                                            delta: String::new(),
                                                            done: true,
                                                            error: Some(e.to_string()),
                                                        });
                                                    if let Ok(pkt) =
                                                        TCPWorker::p2p_udp_encode_command_payload(
                                                            &chunk,
                                                        )
                                                    {
                                                        let msg_id = next_msg_id;
                                                        next_msg_id = next_msg_id.wrapping_add(1);
                                                        let _ = TCPWorker::p2p_udp_send_reliable(
                                                            &socket, from, msg_id, &pkt,
                                                        )
                                                        .await;
                                                    }
                                                    continue;
                                                }
                                            };

                                            let mut token_stream = Box::pin(token_stream);
                                            let mut seq: u32 = 0;

                                            while let Some(piece_res) = token_stream.next().await {
                                                let piece = match piece_res {
                                                    Ok(p) => p,
                                                    Err(e) => {
                                                        let chunk = Command::V2(
                                                            CommandV2::P2PInferenceChunk {
                                                                connection_id,
                                                                task_id: task_id.clone(),
                                                                seq,
                                                                delta: String::new(),
                                                                done: true,
                                                                error: Some(e.to_string()),
                                                            },
                                                        );
                                                        if let Ok(pkt) = TCPWorker::p2p_udp_encode_command_payload(&chunk) {
                                                            let msg_id = next_msg_id;
                                                            next_msg_id = next_msg_id.wrapping_add(1);
                                                            let _ = TCPWorker::p2p_udp_send_reliable(&socket, from, msg_id, &pkt).await;
                                                        }
                                                        break;
                                                    }
                                                };
                                                let filtered = filter_control_tokens(&piece);
                                                if filtered.is_empty() {
                                                    continue;
                                                }

                                                let mut start: usize = 0;
                                                let max_bytes: usize = 64;
                                                while start < filtered.len() {
                                                    let mut end =
                                                        (start + max_bytes).min(filtered.len());
                                                    while end < filtered.len()
                                                        && !filtered.is_char_boundary(end)
                                                    {
                                                        end -= 1;
                                                    }
                                                    if end == start {
                                                        end = filtered
                                                            .char_indices()
                                                            .nth(1)
                                                            .map(|(i, _)| i)
                                                            .unwrap_or(filtered.len());
                                                    }
                                                    let delta = filtered[start..end].to_string();
                                                    start = end;

                                                    let chunk =
                                                        Command::V2(CommandV2::P2PInferenceChunk {
                                                            connection_id,
                                                            task_id: task_id.clone(),
                                                            seq,
                                                            delta,
                                                            done: false,
                                                            error: None,
                                                        });
                                                    if let Ok(pkt) =
                                                        TCPWorker::p2p_udp_encode_command_payload(
                                                            &chunk,
                                                        )
                                                    {
                                                        let msg_id = next_msg_id;
                                                        next_msg_id = next_msg_id.wrapping_add(1);
                                                        let _ = TCPWorker::p2p_udp_send_reliable(
                                                            &socket, from, msg_id, &pkt,
                                                        )
                                                        .await;
                                                    }
                                                    seq = seq.wrapping_add(1);
                                                }
                                            }

                                            let done = Command::V2(CommandV2::P2PInferenceDone {
                                                connection_id,
                                                task_id,
                                                prompt_tokens: 0,
                                                completion_tokens: 0,
                                                total_tokens: 0,
                                            });
                                            if let Ok(pkt) =
                                                TCPWorker::p2p_udp_encode_command_payload(&done)
                                            {
                                                let msg_id = next_msg_id;
                                                next_msg_id = next_msg_id.wrapping_add(1);
                                                let _ = TCPWorker::p2p_udp_send_reliable(
                                                    &socket, from, msg_id, &pkt,
                                                )
                                                .await;
                                            }
                                        }
                                    });
                                }

                                let mut candidates = Vec::<P2PCandidate>::new();
                                candidates.push(P2PCandidate {
                                    candidate_type: P2PCandidateType::Host,
                                    transport: P2PTransport::Udp,
                                    addr: format!("{}:{}", advertise_ip, local_port),
                                    priority: 200,
                                });
                                if let Some(stun_url) = stun_urls.first() {
                                    match Self::stun_binding_srflx_on_socket(&socket, stun_url)
                                        .await
                                    {
                                        Ok(addr) => {
                                            candidates.push(P2PCandidate {
                                                candidate_type: P2PCandidateType::Srflx,
                                                transport: P2PTransport::Udp,
                                                addr: addr.to_string(),
                                                priority: 100,
                                            });
                                        }
                                        Err(e) => {
                                            warn!("STUN binding failed: {}", e);
                                        }
                                    }
                                }

                                #[cfg(not(target_os = "android"))]
                                if let Some(turn_url) = turn_urls.first() {
                                    let writer = Arc::clone(&self.writer);
                                    let source_client_id_copy = self.client_id;
                                    let peer_id_copy = peer_id;
                                    let connection_id_copy = connection_id;
                                    let turn_url = turn_url.clone();
                                    let username = turn_username.clone();
                                    let password = turn_password.clone();
                                    let engine = Arc::clone(&self.engine);
                                    tokio::spawn(async move {
                                        match TCPWorker::turn_allocate_udp(
                                            &turn_url, &username, &password,
                                        )
                                        .await
                                        {
                                            Ok((turn_sock, relayed, realm, nonce)) => {
                                                let relay_candidate = P2PCandidate {
                                                    candidate_type: P2PCandidateType::Relay,
                                                    transport: P2PTransport::Udp,
                                                    addr: relayed.to_string(),
                                                    priority: 50,
                                                };
                                                let cmd = CommandV2::P2PCandidates {
                                                    source_client_id: source_client_id_copy,
                                                    target_client_id: peer_id_copy,
                                                    connection_id: connection_id_copy,
                                                    candidates: vec![relay_candidate],
                                                };
                                                if let Err(e) =
                                                    TCPWorker::send_command_v2_on_writer(
                                                        writer, cmd,
                                                    )
                                                    .await
                                                {
                                                    error!(
                                                        "Failed to send TURN relay candidate: {}",
                                                        e
                                                    );
                                                }

                                                let mut permitted: HashSet<std::net::SocketAddr> =
                                                    HashSet::new();
                                                let mut inflight: HashMap<
                                                    u32,
                                                    HashMap<u16, Vec<u8>>,
                                                > = HashMap::new();
                                                let mut inbox: VecDeque<(
                                                    std::net::SocketAddr,
                                                    Vec<u8>,
                                                )> = VecDeque::new();
                                                let mut next_msg_id: u32 = 1;
                                                let mut buf = vec![0u8; 4096];

                                                loop {
                                                    let (peer, data) = if let Some((p, d)) =
                                                        inbox.pop_front()
                                                    {
                                                        (p, d)
                                                    } else {
                                                        let n = match turn_sock.recv(&mut buf).await
                                                        {
                                                            Ok(n) => n,
                                                            Err(e) => {
                                                                warn!("TURN/UDP recv error: {}", e);
                                                                return;
                                                            }
                                                        };
                                                        let Some((peer, data)) =
                                                            TCPWorker::turn_parse_data_indication(
                                                                &buf[..n],
                                                            )
                                                        else {
                                                            continue;
                                                        };
                                                        (peer, data)
                                                    };

                                                    if !permitted.contains(&peer) {
                                                        if let Err(e) =
                                                            TCPWorker::turn_create_permission(
                                                                &turn_sock, peer, &username,
                                                                &password, &realm, &nonce,
                                                            )
                                                            .await
                                                        {
                                                            warn!(
                                                                "TURN CreatePermission failed: {}",
                                                                e
                                                            );
                                                        } else {
                                                            permitted.insert(peer);
                                                        }
                                                    }

                                                    if let Some((
                                                        flags,
                                                        msg_id,
                                                        frag_idx,
                                                        frag_cnt,
                                                    )) = TCPWorker::p2p_udp_parse_header(&data)
                                                    {
                                                        if (flags & TCPWorker::P2P_UDP_FLAG_ACK)
                                                            != 0
                                                        {
                                                            continue;
                                                        }
                                                        // ACK over TURN by sending an indication carrying only header.
                                                        let hdr = TCPWorker::p2p_udp_make_header(
                                                            TCPWorker::P2P_UDP_FLAG_ACK,
                                                            msg_id,
                                                            0,
                                                            0,
                                                        );
                                                        let _ = TCPWorker::turn_send_indication(
                                                            &turn_sock, peer, &hdr,
                                                        )
                                                        .await;

                                                        if data.len()
                                                            < TCPWorker::P2P_UDP_HEADER_LEN
                                                        {
                                                            continue;
                                                        }
                                                        let payload =
                                                            &data[TCPWorker::P2P_UDP_HEADER_LEN..];
                                                        let entry =
                                                            inflight.entry(msg_id).or_default();
                                                        entry.insert(frag_idx, payload.to_vec());
                                                        let Some(full) =
                                                            TCPWorker::p2p_udp_try_reassemble(
                                                                entry, frag_cnt,
                                                            )
                                                        else {
                                                            continue;
                                                        };
                                                        inflight.remove(&msg_id);

                                                        let cmd =
                                                            match TCPWorker::udp_decode_command(
                                                                &full,
                                                            ) {
                                                                Ok(c) => c,
                                                                Err(e) => {
                                                                    warn!("TURN/UDP decode failed: {}", e);
                                                                    continue;
                                                                }
                                                            };

                                                        let Command::V2(
                                                            CommandV2::P2PInferenceRequest {
                                                                connection_id: req_conn_id,
                                                                task_id,
                                                                model: _model,
                                                                prompt,
                                                                max_tokens,
                                                                temperature,
                                                                top_k,
                                                                top_p,
                                                                repeat_penalty,
                                                                repeat_last_n,
                                                                min_keep,
                                                            },
                                                        ) = cmd
                                                        else {
                                                            continue;
                                                        };
                                                        if req_conn_id != connection_id_copy {
                                                            continue;
                                                        }

                                                        // Stream inference over TURN/UDP data-plane.
                                                        let sampling = crate::llm_engine::llama_engine::SamplingParams {
                                                            temperature,
                                                            top_k: top_k as i32,
                                                            top_p,
                                                            repeat_penalty,
                                                            repeat_last_n,
                                                            seed: 0,
                                                            min_keep: min_keep as usize,
                                                        };

                                                        let token_stream_res = {
                                                            let engine_guard = engine.lock().await;
                                                            let engine_ref = match engine_guard
                                                                .as_ref()
                                                            {
                                                                Some(v) => v,
                                                                None => {
                                                                    let chunk = Command::V2(CommandV2::P2PInferenceChunk {
                                                                        connection_id: connection_id_copy,
                                                                        task_id: task_id.clone(),
                                                                        seq: 0,
                                                                        delta: String::new(),
                                                                        done: true,
                                                                        error: Some("Engine not initialized".to_string()),
                                                                    });
                                                                    if let Ok(pkt) = TCPWorker::p2p_udp_encode_command_payload(&chunk) {
                                                                        let msg_id = next_msg_id;
                                                                        next_msg_id = next_msg_id.wrapping_add(1);
                                                                        let _ = TCPWorker::turn_send_reliable_over_indication(
                                                                            &turn_sock,
                                                                            peer,
                                                                            msg_id,
                                                                            &pkt,
                                                                            &mut inbox,
                                                                        )
                                                                        .await;
                                                                    }
                                                                    continue;
                                                                }
                                                            };

                                                            let AnyEngine::Llama(llama) =
                                                                engine_ref
                                                            else {
                                                                let chunk = Command::V2(CommandV2::P2PInferenceChunk {
                                                                    connection_id: connection_id_copy,
                                                                    task_id: task_id.clone(),
                                                                    seq: 0,
                                                                    delta: String::new(),
                                                                    done: true,
                                                                    error: Some("P2P TURN/UDP streaming is only supported for LLAMA engine".to_string()),
                                                                });
                                                                if let Ok(pkt) = TCPWorker::p2p_udp_encode_command_payload(&chunk) {
                                                                    let msg_id = next_msg_id;
                                                                    next_msg_id = next_msg_id.wrapping_add(1);
                                                                    let _ = TCPWorker::turn_send_reliable_over_indication(
                                                                        &turn_sock,
                                                                        peer,
                                                                        msg_id,
                                                                        &pkt,
                                                                        &mut inbox,
                                                                    )
                                                                    .await;
                                                                }
                                                                continue;
                                                            };

                                                            llama
                                                                .stream_with_cached_model_sampling(
                                                                    &prompt,
                                                                    max_tokens as usize,
                                                                    &sampling,
                                                                )
                                                                .await
                                                        };

                                                        let token_stream = match token_stream_res {
                                                            Ok(s) => s,
                                                            Err(e) => {
                                                                let chunk = Command::V2(
                                                                    CommandV2::P2PInferenceChunk {
                                                                        connection_id:
                                                                            connection_id_copy,
                                                                        task_id: task_id.clone(),
                                                                        seq: 0,
                                                                        delta: String::new(),
                                                                        done: true,
                                                                        error: Some(e.to_string()),
                                                                    },
                                                                );
                                                                if let Ok(pkt) = TCPWorker::p2p_udp_encode_command_payload(&chunk) {
                                                                    let msg_id = next_msg_id;
                                                                    next_msg_id = next_msg_id.wrapping_add(1);
                                                                    let _ = TCPWorker::turn_send_reliable_over_indication(
                                                                        &turn_sock,
                                                                        peer,
                                                                        msg_id,
                                                                        &pkt,
                                                                        &mut inbox,
                                                                    )
                                                                    .await;
                                                                }
                                                                continue;
                                                            }
                                                        };

                                                        let mut token_stream =
                                                            Box::pin(token_stream);
                                                        let mut seq: u32 = 0;

                                                        while let Some(piece_res) =
                                                            token_stream.next().await
                                                        {
                                                            let piece = match piece_res {
                                                                Ok(p) => p,
                                                                Err(e) => {
                                                                    let chunk = Command::V2(CommandV2::P2PInferenceChunk {
                                                                        connection_id: connection_id_copy,
                                                                        task_id: task_id.clone(),
                                                                        seq,
                                                                        delta: String::new(),
                                                                        done: true,
                                                                        error: Some(e.to_string()),
                                                                    });
                                                                    if let Ok(pkt) = TCPWorker::p2p_udp_encode_command_payload(&chunk) {
                                                                        let msg_id = next_msg_id;
                                                                        next_msg_id = next_msg_id.wrapping_add(1);
                                                                        let _ = TCPWorker::turn_send_reliable_over_indication(
                                                                            &turn_sock,
                                                                            peer,
                                                                            msg_id,
                                                                            &pkt,
                                                                            &mut inbox,
                                                                        )
                                                                        .await;
                                                                    }
                                                                    break;
                                                                }
                                                            };
                                                            let filtered =
                                                                filter_control_tokens(&piece);
                                                            if filtered.is_empty() {
                                                                continue;
                                                            }

                                                            let mut start: usize = 0;
                                                            let max_bytes: usize = 64;
                                                            while start < filtered.len() {
                                                                let mut end = (start + max_bytes)
                                                                    .min(filtered.len());
                                                                while end < filtered.len()
                                                                    && !filtered
                                                                        .is_char_boundary(end)
                                                                {
                                                                    end -= 1;
                                                                }
                                                                if end == start {
                                                                    end = filtered
                                                                        .char_indices()
                                                                        .nth(1)
                                                                        .map(|(i, _)| i)
                                                                        .unwrap_or(filtered.len());
                                                                }
                                                                let delta = filtered[start..end]
                                                                    .to_string();
                                                                start = end;

                                                                let chunk = Command::V2(
                                                                    CommandV2::P2PInferenceChunk {
                                                                        connection_id:
                                                                            connection_id_copy,
                                                                        task_id: task_id.clone(),
                                                                        seq,
                                                                        delta,
                                                                        done: false,
                                                                        error: None,
                                                                    },
                                                                );
                                                                if let Ok(pkt) = TCPWorker::p2p_udp_encode_command_payload(&chunk) {
                                                                    let msg_id = next_msg_id;
                                                                    next_msg_id = next_msg_id.wrapping_add(1);
                                                                    let _ = TCPWorker::turn_send_reliable_over_indication(
                                                                        &turn_sock,
                                                                        peer,
                                                                        msg_id,
                                                                        &pkt,
                                                                        &mut inbox,
                                                                    )
                                                                    .await;
                                                                }
                                                                seq = seq.wrapping_add(1);
                                                            }
                                                        }

                                                        let done = Command::V2(
                                                            CommandV2::P2PInferenceDone {
                                                                connection_id: connection_id_copy,
                                                                task_id,
                                                                prompt_tokens: 0,
                                                                completion_tokens: 0,
                                                                total_tokens: 0,
                                                            },
                                                        );
                                                        if let Ok(pkt) = TCPWorker::p2p_udp_encode_command_payload(&done) {
                                                            let msg_id = next_msg_id;
                                                            next_msg_id = next_msg_id.wrapping_add(1);
                                                            let _ = TCPWorker::turn_send_reliable_over_indication(
                                                                &turn_sock,
                                                                peer,
                                                                msg_id,
                                                                &pkt,
                                                                &mut inbox,
                                                            )
                                                            .await;
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                warn!("TURN/UDP allocate failed: {}", e);
                                            }
                                        }
                                    });
                                }

                                let cmd = CommandV2::P2PCandidates {
                                    source_client_id: self.client_id,
                                    target_client_id: peer_id,
                                    connection_id,
                                    candidates,
                                };
                                self.send_command_v2(cmd).await?;
                            }

                            CommandV2::P2PCandidates {
                                source_client_id,
                                target_client_id,
                                connection_id,
                                candidates,
                            } => {
                                // Only handle if we are the intended target.
                                if target_client_id != self.client_id {
                                    continue;
                                }

                                // Try direct TCP connect to host/srflx candidates.
                                let mut last_err: Option<anyhow::Error> = None;
                                for c in &candidates {
                                    if !matches!(
                                        c.candidate_type,
                                        P2PCandidateType::Host | P2PCandidateType::Srflx
                                    ) {
                                        continue;
                                    }

                                    let addr = c.addr.clone();
                                    match timeout(Duration::from_secs(3), TcpStream::connect(&addr))
                                        .await
                                    {
                                        Ok(Ok(stream)) => {
                                            let established = CommandV2::P2PConnectionEstablished {
                                                peer_id: source_client_id,
                                                connection_id,
                                                connection_type: P2PConnectionType::Direct,
                                            };
                                            self.send_command_v2(established).await?;

                                            #[cfg(not(target_os = "android"))]
                                            {
                                                let engine = Arc::clone(&self.engine);
                                                tokio::spawn(async move {
                                                    if let Err(e) =
                                                        TCPWorker::serve_p2p_stream_with_engine(
                                                            engine,
                                                            stream,
                                                            connection_id,
                                                        )
                                                        .await
                                                    {
                                                        error!("P2P direct data-plane stream error: {}", e);
                                                    }
                                                });
                                            }

                                            last_err = None;
                                            break;
                                        }
                                        Ok(Err(e)) => last_err = Some(e.into()),
                                        Err(e) => last_err = Some(anyhow!("connect timeout: {e}")),
                                    }
                                }

                                if let Some(e) = last_err {
                                    #[cfg(not(target_os = "android"))]
                                    {
                                        if let Some((turn_urls, username, password, _peer_hex)) =
                                            p2p_turn_config.get(&connection_id).cloned()
                                        {
                                            if let Some(turn_url) = turn_urls.first() {
                                                match TCPWorker::turn_allocate_tcp(
                                                    turn_url,
                                                    &username,
                                                    &password,
                                                    &self.args.cert_chain_path,
                                                )
                                                .await
                                                {
                                                    Ok((mut tls, _relayed, realm, nonce)) => {
                                                        // Find peer relay candidate from incoming list.
                                                        let peer_relay = candidates
                                                            .iter()
                                                            .find(|c| {
                                                                matches!(
                                                                    c.candidate_type,
                                                                    P2PCandidateType::Relay
                                                                )
                                                            })
                                                            .and_then(|c| c.addr.parse().ok());
                                                        if let Some(peer_relay) = peer_relay {
                                                            match TCPWorker::turn_connect_peer(
                                                                &mut tls,
                                                                peer_relay,
                                                                &username,
                                                                &password,
                                                                &realm,
                                                                &nonce,
                                                            )
                                                            .await
                                                            {
                                                                Ok(conn_id) => {
                                                                    match TCPWorker::turn_connection_bind(
                                                                        turn_url,
                                                                        &conn_id,
                                                                        &username,
                                                                        &password,
                                                                        &realm,
                                                                        &nonce,
                                                                        &self.args.cert_chain_path,
                                                                    )
                                                                    .await
                                                                    {
                                                                        Ok(data_stream) => {
                                                                            let established = CommandV2::P2PConnectionEstablished {
                                                                                peer_id: source_client_id,
                                                                                connection_id,
                                                                                connection_type: P2PConnectionType::TURN,
                                                                            };
                                                                            self.send_command_v2(established).await?;

                                                                            let engine = Arc::clone(&self.engine);
                                                                            tokio::spawn(async move {
                                                                                if let Err(e) = TCPWorker::serve_p2p_io_with_engine(engine, data_stream, connection_id).await {
                                                                                    error!("TURN data-plane stream error: {}", e);
                                                                                }
                                                                            });
                                                                            continue;
                                                                        }
                                                                        Err(e2) => {
                                                                            last_err = Some(e2);
                                                                        }
                                                                    }
                                                                }
                                                                Err(e2) => {
                                                                    last_err = Some(e2);
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Err(e2) => {
                                                        last_err = Some(e2);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    let failed = CommandV2::P2PConnectionFailed {
                                        peer_id: source_client_id,
                                        connection_id,
                                        error: format!("connect failed: {}", e),
                                    };
                                    self.send_command_v2(failed).await?;
                                }
                            }

                            _ => {
                                // Ignore other V2 commands for now.
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
fn load_root_cert(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?; // Manually collect and handle errors

    if certs.is_empty() {
        anyhow::bail!("no certificates found in {}", path);
    }
    Ok(certs)
}

#[cfg(target_os = "android")]
fn load_root_cert(path: &str) -> anyhow::Result<Vec<u8>> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let certs: Vec<Vec<u8>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|cert| cert.to_vec())
        .collect();

    if certs.is_empty() {
        anyhow::bail!("no certificates found in {}", path);
    }
    Ok(certs.into_iter().flatten().collect())
}

#[cfg(not(target_os = "android"))]
pub async fn create_proxy_connection(
    args: Args,
    addr: std::net::IpAddr,
    proxy_conn_id: [u8; 16],
    cert_chain_path: String,
) -> Result<()> {
    // DONE: addr is sent to server addr
    let addr_str = format!("{}:{}", addr.to_string(), args.proxy_port);
    let addr = addr_str.to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid server address or port",
        )
    })?;

    let tcp_stream = match TcpStream::connect(addr).await {
        Ok(stream) => stream,
        Err(e) => {
            error!(" create proxy connection failed {}: {}", addr, e);
            return Err(e.into());
        }
    };

    match tcp_stream.set_nodelay(true) {
        Ok(_) => info!("Set nodelay for proxy connection {:?}", proxy_conn_id),
        Err(e) => error!(
            "Failed to set nodelay for proxy connection {:?}: {}",
            proxy_conn_id, e
        ),
    };

    let cert = match load_root_cert(cert_chain_path.as_str()) {
        Ok(cert) => cert,
        Err(e) => {
            error!("Failed to load root cert: {}", e);
            return Err(e.into());
        }
    };

    let mut root_store = RootCertStore::empty();
    match root_store.add(cert[0].clone()) {
        Ok(_) => info!("Add root cert for proxy connection {:?}", proxy_conn_id),
        Err(e) => error!(
            "Failed to add root cert for proxy connection {:?}: {}",
            proxy_conn_id, e
        ),
    };

    info!(
        " proxy_conn_id {:?} Connected to proxy port.",
        proxy_conn_id
    );

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(config));

    let server_addr_clone = args.server_addr.clone();
    let server_addr_clone2 = args.server_addr.clone();
    let server_name = if let Ok(ip) = server_addr_clone.parse::<std::net::IpAddr>() {
        // For IP address
        ServerName::try_from(ip.to_string())
            .map_err(|_| anyhow::anyhow!("Invalid server name: {}", server_addr_clone))?
    } else {
        // For domain name
        ServerName::try_from(args.server_addr)
            .map_err(|_| anyhow::anyhow!("Invalid server name: {}", server_addr_clone2))?
    };

    let mut tls_proxy_stream = match connector.connect(server_name, tcp_stream).await {
        Ok(stream) => stream,
        Err(e) => {
            error!("rustls: {}", e);
            return Err(anyhow!("Failed to connect to proxy port: {}", e));
        }
    };

    let notify_cmd = Command::V1(CommandV1::NewProxyConn {
        proxy_conn_id: proxy_conn_id.clone(),
    });

    match write_command(&mut tls_proxy_stream, &notify_cmd).await {
        Ok(_) => info!(
            "proxy_conn_id {:?} Sent new proxy connection notification.",
            proxy_conn_id
        ),
        Err(e) => error!("Failed to send new proxy connection notification: {}", e),
    };

    let local_stream =
        match TcpStream::connect(format!("{}:{}", args.local_addr, args.local_port)).await {
            Ok(stream) => stream,
            Err(e) => {
                error!("Failed to connect to local service: {}", e);
                return Err(anyhow!("Failed to connect to local service: {}", e));
            }
        };
    info!(
        "proxy_conn_id {:?} Connected to local service at {}:{}",
        proxy_conn_id, args.local_addr, args.local_port
    );

    info!("proxy_conn_id {:?} Joining streams...", proxy_conn_id);

    match join_streams(tls_proxy_stream, local_stream).await {
        Ok(_) => {
            info!(
                "proxy_conn_id {:?} Streams joined and finished.",
                proxy_conn_id
            );
            return Ok(());
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                info!(
                    "proxy_conn_id {:?} Connection closed by peer",
                    proxy_conn_id
                );
                return Ok(());
            } else {
                error!(
                    "proxy_conn_id {:?} Error joining streams: {}",
                    proxy_conn_id, e
                );
                return Err(e.into());
            }
        }
    }
}

#[cfg(target_os = "android")]
pub async fn create_proxy_connection(
    args: Args,
    addr: std::net::IpAddr,
    proxy_conn_id: [u8; 16],
    cert_chain_path: String,
) -> Result<()> {
    // Android implementation using native TLS - simplified version
    warn!("Android TLS proxy connections are simplified - full TLS support requires additional configuration");

    // For now, just establish TCP connection without TLS
    let addr_str = format!("{}:{}", addr.to_string(), args.proxy_port);
    let addr = addr_str.to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid server address or port",
        )
    })?;

    let mut tcp_stream = match TcpStream::connect(addr).await {
        Ok(stream) => stream,
        Err(e) => {
            error!("create proxy connection failed {}: {}", addr, e);
            return Err(e.into());
        }
    };

    match tcp_stream.set_nodelay(true) {
        Ok(_) => info!("Set nodelay for proxy connection {:?}", proxy_conn_id),
        Err(e) => error!(
            "Failed to set nodelay for proxy connection {:?}: {}",
            proxy_conn_id, e
        ),
    };

    info!(
        "proxy_conn_id {:?} Connected to proxy port (Android - TCP only).",
        proxy_conn_id
    );

    let notify_cmd = Command::V1(CommandV1::NewProxyConn {
        proxy_conn_id: proxy_conn_id.clone(),
    });

    match write_command(&mut tcp_stream, &notify_cmd).await {
        Ok(_) => info!(
            "proxy_conn_id {:?} Sent new proxy connection notification.",
            proxy_conn_id
        ),
        Err(e) => error!("Failed to send new proxy connection notification: {}", e),
    };

    let local_stream =
        match TcpStream::connect(format!("{}:{}", args.local_addr, args.local_port)).await {
            Ok(stream) => stream,
            Err(e) => {
                error!("create local connection failed {}: {}", args.local_addr, e);
                return Err(e.into());
            }
        };

    match local_stream.set_nodelay(true) {
        Ok(_) => info!("Set nodelay for local connection {:?}", proxy_conn_id),
        Err(e) => error!(
            "Failed to set nodelay for local connection {:?}: {}",
            proxy_conn_id, e
        ),
    };

    info!("proxy_conn_id {:?} Connected to local port.", proxy_conn_id);

    info!("proxy_conn_id {:?} Joining streams...", proxy_conn_id);

    match join_streams(tcp_stream, local_stream).await {
        Ok(_) => {
            info!(
                "proxy_conn_id {:?} Streams joined and finished.",
                proxy_conn_id
            );
            return Ok(());
        }
        Err(e) => {
            error!(
                "proxy_conn_id {:?} Failed to join streams: {}",
                proxy_conn_id, e
            );
            return Err(e.into());
        }
    }
}
