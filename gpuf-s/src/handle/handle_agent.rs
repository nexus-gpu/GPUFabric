

use tracing::{error, info, warn};
use tokio::net::TcpStream;
use uuid::Uuid;
use super::*;
use std::time::Duration;

use rdkafka::producer::{FutureProducer, FutureRecord};


#[cfg(target_os = "linux")]
use tokio_uring::net::TcpStream as UringTcpStream;

use crate::util::{
    protoc::{ClientId, ProxyConnId, RequestIDAndClientIDMessage},
};
use bytes::BytesMut;

use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncWrite};
use twoway;

use anyhow::{anyhow, Result};
use simd_json;
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio_rustls::{
    rustls::{crypto::aws_lc_rs, server::ServerConfig},
    TlsAcceptor,
};

use tracing::debug;
use crate::db::client::get_user_client_by_token;
use crate::util::msg::ApiResponse;


impl ServerState {

   pub async fn handle_proxy_connections(self: Arc<Self>, listener: TcpListener) -> Result<()> {
        let cert_chain = self.cert_chain.clone();
        let priv_key = self.priv_key.clone();

        aws_lc_rs::default_provider()
            .install_default()
            .expect("failed to install aws-lc-rs provider");
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain.to_vec(), priv_key.clone_key())?;
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        loop {
            let (proxy_stream, addr) = listener.accept().await?;
            info!("New proxy connection from: {}", addr);
            let _ = proxy_stream.set_nodelay(true);
            let acceptor = acceptor.clone();
            let pending_clone = self.pending_connections.clone();
            let buffer_pool = self.buffer_pool.clone();
            tokio::spawn(async move {
                let mut buf = BytesMut::with_capacity(1024 * 1024);

                let mut tls_proxy_stream = match acceptor.accept(proxy_stream).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        error!("rustls: {}", e);
                        return;
                    }
                };

                if let Ok(Command::V1(CommandV1::NewProxyConn { proxy_conn_id })) =
                    read_command(&mut tls_proxy_stream, &mut buf).await
                {
                    info!(
                        "Received proxy conn notification for id: {:?}",
                        proxy_conn_id
                    );
                    let mut pending = pending_clone.lock().await;

                    if let Some((user_stream, buf)) = pending.remove(&ProxyConnId(proxy_conn_id)) {
                        info!(
                            "Pairing user stream with proxy stream for id: {:?}",
                            proxy_conn_id
                        );
                        let request_str = String::from_utf8_lossy(&buf);
                        let parts: Vec<&str> = request_str.split("\r\n\r\n").collect();
                        if parts.len() > 1 {
                            debug!("=== HTTP Headers ===");
                            debug!("{}", parts[0]);
                            debug!("=== HTTP Body ===");
                            debug!("{} len: {}", parts[1], parts[1].len());
                        } else {
                            debug!("Full request (no body separator): {}", request_str);
                        }
                        //print buffer
                        debug!(
                            "Sending buffer to client stream: {:?} len: {} buf len: {}",
                            request_str,
                            request_str.len(),
                            buf.len()
                        );
                        let _ = tls_proxy_stream.write_all(buf.as_ref()).await;
                        let _ = tls_proxy_stream.flush().await;
                        buffer_pool.put(buf).await;

                        tokio::spawn(async move {
                            if let Err(e) = join_streams(user_stream, tls_proxy_stream).await {
                                error!("Error joining streams: {}", e);
                            }
                            info!("Streams for {:?} joined and finished.", proxy_conn_id);
                        });
                    } else {
                        warn!(
                            "No pending user connection found for proxy_conn_id: {:?}",
                            proxy_conn_id
                        );
                    }
                } else {
                    error!("Failed to read NewProxyConn command from {}", addr);
                }
            });
        }
    }

   pub async fn handle_public_connections(self: Arc<Self>, listener: TcpListener) -> Result<()> {
        loop {
            let (user_stream, addr) = listener.accept().await?;
            info!("New public connection from: {}", addr);
            let active_clients_clone = self.active_clients.clone();
            let pending_connections_clone = self.pending_connections.clone();
            let total_connections_clone = self.total_connections.clone();
            //let api_key = api_key.clone();
            // let _redis_client_clone = self.redis_client.clone();
            let db_pool_clone = self.db_pool.clone();

            let producer_clone = self.producer.clone();
            let buffer_pool_clone = self.buffer_pool.clone();
            tokio::spawn(async move {
                // Increment total connections counter
                {
                    let mut counter = total_connections_clone.lock().await;
                    *counter += 1;
                }

                if let Err(e) = route_public_connection_new(
                    user_stream,
                    buffer_pool_clone,
                    active_clients_clone,
                    pending_connections_clone,
                    db_pool_clone,
                    producer_clone,
                )
                .await
                {
                    //send_http_error_response(user_stream, 401, "Invalid API key").await;
                    error!("Failed to route public connection from {} : {}", addr, e);
                }
            });
        }
    }

    #[cfg(target_os = "linux")]
   pub async fn handle_proxy_connections_uring(
        self: Arc<Self>,
        listener: TcpListener,
        api_key: String,
    ) -> Result<()> {
        let active_clients = self.active_clients.clone();
        let pending_connections = self.pending_connections.clone();
        let db_pool = self.db_pool.clone();
        let redis_client = self.redis_client.clone();
        let producer = self.producer.clone();
        
        tokio_uring::start(async {
            let std_listener = listener.into_std()?;
            let listener = tokio_uring::net::TcpListener::from_std(std_listener);

            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let active_clients = active_clients.clone();
                        let pending_connections = pending_connections.clone();
                        let db_pool = db_pool.clone();
                        let redis_client = redis_client.clone();
                        let producer = producer.clone();
                        let api_key = api_key.clone();

                        // Spawn a new task to handle each connection
                        tokio_uring::spawn(async move {
                            if let Err(e) = route_public_connection_uring(
                                stream,
                                active_clients,
                                pending_connections,
                                api_key,
                                db_pool,
                                redis_client,
                                producer,
                            )
                            .await
                            {
                                error!("Error handling connection: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Error accepting connection: {}", e);
                        // Add a small delay to prevent tight loop on errors
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        })
    }

}

#[cfg(target_os = "linux")]
async fn route_public_connection_uring(
    _user_stream: UringTcpStream,
    _active_clients: ActiveClients,
    _pending_connections: PendingConnections,
    _api_key: String,
    _db_pool: Arc<Pool<Postgres>>,
    _redis_client: Arc<RedisClient>,
    _producer: Arc<FutureProducer>,
) -> Result<()> {
    // TODO: Implement uring version of route_public_connection
    info!("Handling connection with io_uring (not yet implemented)");
    Ok(())
}

#[cfg(target_os = "linux")]
async fn parse_request_uring(_user_stream: UringTcpStream) -> Result<()> {
    // TODO: Implement uring version of parse_request
    info!("Parsing request with io_uring (not yet implemented)");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
async fn parse_request(
    user_stream: &TcpStream,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let mut buf = [0u8; 2048]; 
    let n = user_stream.peek(&mut buf).await?;
    if n == 0 {
        return Err(anyhow::anyhow!("Connection closed by peer"));
    }

    // 2. httparse
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut req = httparse::Request::new(&mut headers);

    match req.parse(&buf[..n]) {
        Ok(httparse::Status::Complete(header_len)) => {
            let api_key = req
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case("authorization"))
                .and_then(|h| std::str::from_utf8(h.value).ok())
                .and_then(|h| h.strip_prefix("Bearer "))
                .map(str::to_string);

            info!("authorization api_key: {:?}", api_key);
            // 获取 Content-Length
            let content_length = req
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case("content-length"))
                .and_then(|h| std::str::from_utf8(h.value).ok())
                .and_then(|s| s.parse::<usize>().ok());

            let request_id = req
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case("request-id"))
                .and_then(|h| std::str::from_utf8(h.value).ok())
                .map(|s| s.to_string());

            let is_complete = if let Some(len) = content_length {
                n >= header_len + len
            } else {
                true
            };
            let mut model = None;
            info!(
                "is_complete: {:?}, content_length: {:?}",
                is_complete, content_length
            );
            if is_complete && content_length.is_some() {
                let body_start = header_len;
                let body_end = header_len + content_length.unwrap();

                if body_end <= n {
                    let mut body = buf[body_start..body_end].to_vec();

                    if let Ok(parsed_body) = simd_json::from_slice::<serde_json::Value>(&mut body) {
                        model = parsed_body
                            .get("model")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                    }
                } else {
                    warn!("Body not complete");
                }
            }
            Ok((api_key, model, request_id))
        }
        Ok(httparse::Status::Partial) => {
            if n == buf.len() {
                return Err(anyhow::anyhow!("Request header too large"));
            }
            Err(anyhow::anyhow!("Incomplete HTTP headers"))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to parse HTTP request: {}", e)),
    }
}

async fn authenticate_and_select_client(
    api_key: Option<String>,
    db_pool: &Pool<Postgres>,
) -> Result<(Vec<ClientId>, i32)> {
    let api_key = api_key.ok_or_else(|| anyhow::anyhow!("Missing API key"))?;
    if api_key.len() != 48 {
        warn!("Invalid API key length");
        return Err(anyhow::anyhow!("Invalid API key length"));
    }
    // Validate token and client using database with Redis caching
    match get_user_client_by_token(db_pool, api_key.as_str()).await {
        Ok(client_ids) => Ok(client_ids),
        Err(e) => Err(e),
    }
}

#[allow(dead_code)]
struct TeeReader<R> {
    reader: R,
    writer: Option<tokio::io::DuplexStream>,
}

impl<R: AsyncRead + Unpin> AsyncRead for TeeReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.reader).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                if let Some(writer) = &mut self.writer {
                    let filled = buf.filled();
                    let new_data = &filled[before..];
                    if !new_data.is_empty() {
                        let _ = futures::executor::block_on(writer.write_all(new_data));
                    }
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}
/// A wrapper around TcpStream that allows peeking at the data without consuming it
#[allow(dead_code)]
pub struct PeekableTcpStream {
    inner: TcpStream,
    peek_buf: Vec<u8>,
    peek_pos: usize,
    consumed_data: Vec<u8>,
}

use tokio::io::ReadBuf;

impl PeekableTcpStream {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            inner: stream,
            peek_buf: Vec::with_capacity(4096),
            peek_pos: 0,
            consumed_data: Vec::with_capacity(4096),
        }
    }

    /// Peek at the data without consuming it
    pub async fn peek(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // If we've already peeked data, return it
        if self.peek_pos < self.peek_buf.len() {
            let available = (self.peek_buf.len() - self.peek_pos).min(buf.len());
            buf[..available]
                .copy_from_slice(&self.peek_buf[self.peek_pos..self.peek_pos + available]);
            return Ok(available);
        }

        // Otherwise, read new data into our peek buffer
        let mut temp_buf = vec![0u8; buf.len().max(4096)];
        let n = self.inner.peek(&mut temp_buf).await?;

        if n == 0 {
            return Ok(0);
        }

        self.peek_buf.extend_from_slice(&temp_buf[..n]);
        // let to_copy = (self.peek_buf.len() - self.peek_pos).min(buf.len());
        // buf[..to_copy].copy_from_slice(&self.peek_buf[self.peek_pos..self.peek_pos + to_copy]);
        let available = n.min(buf.len());
        buf[..available].copy_from_slice(&self.peek_buf[..available]);

        Ok(available)
    }

    /// Consume n bytes from the peek buffer
    pub fn consume(&mut self, n: usize) {
        self.peek_pos = (self.peek_pos + n).min(self.peek_buf.len());
        if self.peek_pos == self.peek_buf.len() {
            warn!(
                "Peek buffer overflow peek_pos {} peek_buf len {}",
                self.peek_pos,
                self.peek_buf.len()
            );
            self.peek_buf.clear();
            self.peek_pos = 0;
        }
    }

    /// Get a reference to the inner TcpStream
    pub fn get_ref(&self) -> &TcpStream {
        &self.inner
    }

    /// Get a mutable reference to the inner TcpStream
    pub fn get_mut(&mut self) -> &mut TcpStream {
        &mut self.inner
    }

    /// Convert back into the inner TcpStream
    pub fn into_inner(self) -> TcpStream {
        self.inner
    }
}

impl AsyncRead for PeekableTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // First, serve from the peek buffer if we have data
        if self.peek_pos < self.peek_buf.len() {
            let available = self.peek_buf.len() - self.peek_pos;
            let to_copy = available.min(buf.remaining());
            buf.put_slice(&self.peek_buf[self.peek_pos..self.peek_pos + to_copy]);
            self.peek_pos += to_copy;
            return Poll::Ready(Ok(()));
        }

        // Otherwise, read from the underlying stream
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for PeekableTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

use simd_json::{prelude::*, OwnedValue};
#[derive(Debug)]
//pub struct ChatRequestInfo<R: AsyncRead + Unpin> {
pub struct ChatRequestInfo {
    pub model: Option<String>,
    pub request_id: Option<String>,
    pub api_key: Option<String>,
    pub content_type: Option<String>,
    // pub reader: R,
}
use http::header::{HeaderMap, HeaderName, HeaderValue};
use std::str::FromStr;

fn parse_headers(data: &[u8]) -> io::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    let data_str = std::str::from_utf8(data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8 in headers"))?;

    let mut lines = data_str.lines();
    let _request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty request"))?;

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_lowercase();
            let value = value.trim();

            if let (Ok(name), Ok(value)) =
                (HeaderName::from_str(&name), HeaderValue::from_str(value))
            {
                headers.insert(name, value);
            }
        }
    }

    Ok(headers)
}
async fn extract_chat_info<R: AsyncRead + Unpin>(
    reader: &mut R,
    buffer: &mut BytesMut,
) -> Result<ChatRequestInfo> {
    let mut headers = None;
    let mut body_start = 0;
    let mut temp_buf = [0u8; 1024];
    'header_loop: while buffer.len() < 64 * 1024 {
        let n = match reader.read(&mut temp_buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e.into()),
        };

        buffer.extend_from_slice(&temp_buf[..n]);

        if let Some(pos) = twoway::find_bytes(&buffer, b"\r\n\r\n") {
            let headers_data = &buffer[..pos + 4]; // 包含 \r\n\r\n
            headers = Some(parse_headers(headers_data)?);
            body_start = pos + 4;
            break 'header_loop;
        }
    }

    let headers = match headers {
        Some(h) => h,
        None => {
            return Err(anyhow::anyhow!("Invalid HTTP headers"));
        }
    };

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or("").trim().to_lowercase())
        .filter(|s| !s.is_empty());

    let api_key = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    let request_id = headers
        .get("request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    debug!(
        "api_key: {:?}, request_id: {:?}, content_type: {:?}",
        api_key, request_id, content_type
    );

    debug!("buffer.len: {} body_start: {} ", buffer.len(), body_start);

    let chat_info = if let Some(ct) = &content_type {
        if ct == "application/json" {
            let model = if body_start < buffer.len() {
                // let body = buffer[body_start..].to_vec();
                // buffer = Vec::with_capacity(8 * 1024);
                if let Some(model) = try_parse_model_from_slice(&buffer[body_start..]) {
                    Some(model)
                } else {

                    let mut found = false;
                    let mut model = None;
                    let mut buf = [0u8; 1024];

                    while !found {
                        match reader.read(&mut buf).await {
                            Ok(0) => {
                                warn!("Connection closed");
                                break;
                            } 
                            Ok(n) => {
                                buffer.truncate(body_start + n);
                                buffer.extend_from_slice(&buf[..n]);

                                if let Some(m) = try_parse_model_from_slice(&buffer[body_start..]) {
                                    model = Some(m);
                                    found = true;
                                }
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }

                    model
                }
            } else {
                debug!("No body data found");
                //let mut combined = Vec::new();
                let mut found = false;
                let mut model = None;
                let mut buf = [0u8; 1024];

                while !found {
                    match reader.read(&mut buf).await {
                        Ok(0) => {
                            warn!("Connection closed");
                            break;
                        } 
                        Ok(n) => {
                            buffer.truncate(body_start + n);
                            buffer.extend_from_slice(&buf[..n]);
                            if let Some(m) = try_parse_model_from_slice(&buffer[body_start..]) {
                                model = Some(m);
                                found = true;
                            }
                            debug!(" buffer.len: {}", buffer.len());
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                model
            };

            ChatRequestInfo {
                model,
                request_id,
                api_key,
                content_type,
                // reader,
            }
        } else {
            warn!("Unsupported content type: {:?}", ct);
            ChatRequestInfo {
                model: None,
                request_id: None,
                api_key,
                content_type,
                // reader,
            }
        }
    } else {
        ChatRequestInfo {
            model: None,
            request_id: None,
            api_key,
            content_type: None,
            //reader,
        }
    };

    Ok(chat_info)
}

#[allow(dead_code)]
async fn parse_json_body<R: AsyncRead + Unpin>(
    reader: &mut R,
    mut buffer: Vec<u8>,
) -> io::Result<Option<String>> {
    let mut temp_buf = [0u8; 1024];

    if let Ok(model) = try_parse_chat_info(&buffer) {
        if model.is_some() {
            return Ok(model);
        }
    }

    while buffer.len() < 10 * 1024 * 1024 {
        let n = match reader.read(&mut temp_buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e),
        };

        buffer.extend_from_slice(&temp_buf[..n]);
        if let Ok(model) = try_parse_chat_info(&buffer) {
            return Ok(model);
        }
    }

    try_parse_chat_info(&buffer)
}

fn try_parse_model_from_slice(data: &[u8]) -> Option<String> {

    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(data) {
        debug!("json: {:?}", json);
        if let Some(model) = json.get("model").and_then(|m| m.as_str()) {
            return Some(model.to_string());
        }
    }

    let s = String::from_utf8_lossy(data);
    if let Some(start) = s.find(r#""model":"#) {
        let remaining = &s[start + 8..]; // "model":"
        if let Some(end) = remaining.find('"') {
            return Some(remaining[..end].to_string());
        }
    }

    None
}

#[allow(dead_code)]
fn try_parse_chat_info(data: &[u8]) -> io::Result<Option<String>> {
    let mut data = data.to_vec();
    let json: Result<OwnedValue, _> = simd_json::from_slice(&mut data);

    let v = match json {
        Ok(v) => v,
        Err(_) => return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid JSON")),
    };

    let model = v
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(model)
}

pub async fn send_http_error_response(
    mut stream: TcpStream,
    status_code: u16,
    error_message: &str,
) -> Result<()> {
    let error_response = ApiResponse::<()>::error(error_message.to_string());
    
    let json_body = serde_json::to_string(&error_response)?;

    let status_text = match status_code {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        500 => "Internal Server Error",
        _ => "Error",
    };

    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status_code, status_text, json_body.len(), json_body
    );

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn route_public_connection_new(
    mut user_stream: TcpStream,
    buffer_pool: Arc<BufferPool>,
    active_clients: ActiveClients,
    pending_connections: PendingConnections,
    db_pool: Arc<Pool<Postgres>>,
    producer: Arc<FutureProducer>,
) -> Result<()> {
    // Request Parsing Module - Handle HTTP request parsing and validation

    let mut buffer = buffer_pool.get().await;
    let chat_info = match extract_chat_info(&mut user_stream, &mut buffer).await {
        Ok(result) => result,
        Err(e) => {
            error!("Request parsing timed out: {}", e);
            buffer_pool.put(buffer).await;
            return Err(anyhow::anyhow!("Request parsing timed out"));
        }
    };

    // debug!("Request Parsing Module - Handle HTTP request parsing and validation chat_info {:?}", chat_info);
    // Validate model and request_id
    if chat_info.model.is_none() || chat_info.api_key.is_none() {
        buffer_pool.put(buffer).await;
        send_http_error_response(user_stream, 401, "Invalid model or api_key").await?;
        return Err(anyhow::anyhow!(
            "Missing model {} or api_key {}",
            chat_info.model.unwrap_or_default(),
            chat_info.api_key.unwrap_or_default()
        ));
    }

    // TODO: use map to cacheclient info
    // Authentication Module - Handle API key validation
    debug!("Authentication Module - Handle API key validationt");
    let (client_ids, access_level) =
        match authenticate_and_select_client(chat_info.api_key, &db_pool).await {
            Ok(client) => client,
            Err(e) => {
                buffer_pool.put(buffer).await;
                error!("Failed to authenticate and select client: {}", e);
                send_http_error_response(
                    user_stream,
                    401,
                    "Failed to authenticate and select client",
                )
                .await?;
                return Err(anyhow::anyhow!("Failed to authenticate and select client"));
            }
        };

    if client_ids.is_empty() {
        buffer_pool.put(buffer).await;
        send_http_error_response(user_stream, 401, "No available clients").await?;
        return Err(anyhow::anyhow!("No available clients"));
    }

    // Route public connection to chosen client
    debug!("Route public connection to chosen client");
    let mut active_clients = active_clients.lock().await;

    let chosen_client_id = match connect_client_filter_model_and_client(
        chat_info.model.as_ref().unwrap(),
        client_ids,
        &mut active_clients,
    )
    .await
    {
        Ok((chosen_client_id, chosen_client_proxy_conn_id)) => {
            pending_connections
                .lock()
                .await
                .insert(chosen_client_proxy_conn_id, (user_stream, buffer));
            chosen_client_id
        }
        Err(e) => {
            buffer_pool.put(buffer).await;
            send_http_error_response(user_stream, 400, "No available clients").await?;
            return Err(anyhow::anyhow!("No available clients {}", e));
        }
    };

    if access_level != -1 {
        debug!("Send kafka key-value (request_id, client_id) pair");
        return Ok(());
    }

    // share api Send kafka key-value (request_id, client_id) pair
    match request_to_kafka(chat_info.request_id, chosen_client_id, producer).await {
        Ok(_) => Ok(()),
        Err(e) => {
            error!("Failed to send request to Kafka: {:?}", e);
            Err(anyhow::anyhow!("Failed to send request to Kafka: {:?}", e))
        }
    }
}


pub async fn connect_client_filter_model_and_client(
    model_name: &str,
    client_ids: Vec<ClientId>,
    clients: &mut HashMap<ClientId, ClientInfo>,
) -> Result<(ClientId, ProxyConnId)> {
    let chosen_client: Option<(&ClientInfo, ClientId)> =
        client_ids.into_iter().find_map(|client_id| {
            if let Some(client_info) = clients.get(&client_id) {
                if let Some(models) = &client_info.models {
                    if models.iter().any(|m| m.id == model_name) {
                        return Some((client_info, client_id));
                    }
                }
            }
            None
        });
    match chosen_client {
        Some((client_info, client_id)) => {
            if !client_info.authed {
                return Err(anyhow!("Chosen client not authenticated"));
            }
            let proxy_conn_id = Uuid::new_v4().as_bytes().clone();
            let command = Command::V1(CommandV1::RequestNewProxyConn { proxy_conn_id });

            info!(
                "Requesting new proxy connection with id: {:?}",
                proxy_conn_id
            );
            let mut writer = client_info.writer.lock().await;

            if let Err(e) = write_command(&mut *writer, &command).await {
                error!(
                "Failed to send RequestNewProxyConn to client {}: {}. Removing from active list.",
                client_id, e
                );
                drop(writer);
                clients.remove(&client_id);
                return Err(e);
            }
            info!(
                "Successfully sent RequestNewProxyConn to client {}",
                client_id
            );
            Ok((client_id, ProxyConnId(proxy_conn_id)))
        }
        None => {
            error!("Chosen client disappeared");
            return Err(anyhow!("Chosen client disappeared"));
        }
    }
}



async fn request_to_kafka(
    request_id: Option<String>,
    chosen_client_id: ClientId,
    producer: Arc<FutureProducer>,
) -> Result<()> {
    if let Some(request_id_str) = request_id {
        let message = RequestIDAndClientIDMessage {
            request_id: hex::decode(request_id_str)?
                .try_into()
                .map_err(|_| anyhow!("Invalid client ID length"))?,
            client_id: chosen_client_id.0,
        };

        let request_message_bytes = serde_json::to_vec(&message).unwrap();

        if let Err(e) = producer
            .send(
                FutureRecord::to("request-message")
                    .payload(&request_message_bytes)
                    .key(&chosen_client_id.to_string()),
                Duration::from_secs(0),
            )
            .await
        {
            error!("Failed to send heartbeat to Kafka: {:?}", e);
        };
    }
    Ok(())
}
