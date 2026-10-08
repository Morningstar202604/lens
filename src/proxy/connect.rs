//! HTTPS CONNECT 隧道处理。
//!
//! 收到 `CONNECT host:443` 后：
//! 1. 与上游建立 TCP + TLS 连接
//! 2. 回复客户端 200 Connection Established
//! 3. 用动态证书与客户端完成 TLS 握手（MITM）
//! 4. 循环读取明文 HTTP 请求 → 转发 → 读取响应 → 记录 → 回传
//!
//! 修复：
//! - HTTP/1.1 keep-alive：同一隧道内循环处理多个请求（不再一条就关闭）
//! - SSE / chunked：流式转发，不等待完整响应
//! - 记录失败不会中断隧道

use anyhow::Result;
use rustls::ClientConfig;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::{info, warn};

use super::cert::CertManagerArc;
use super::io::{read_http_headers, read_http_response};
use super::request::parse_request_data;
use crate::engine::Store;

const CRLF: &[u8] = b"\r\n\r\n";

/// 处理一个 CONNECT 隧道请求。
/// `request_line` 是完整的 CONNECT 请求行，例如 `CONNECT api.example.com:443 HTTP/1.1`。
pub async fn handle_connect(
    request_line: &str,
    mut client_stream: TcpStream,
    cert_manager: CertManagerArc,
    store: Arc<Store>,
) -> Result<()> {
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        anyhow::bail!("CONNECT 格式错误");
    }
    let target = parts[1];
    let host = target.split(':').next().unwrap_or(target);

    info!("CONNECT → {}", target);

    drain_request_headers(&mut client_stream).await?;

    // ── 上游 TCP + TLS ─────────────────────────────────────────
    let upstream_tcp = match TcpStream::connect(target).await {
        Ok(s) => s,
        Err(e) => {
            send_gateway_error(
                &mut client_stream,
                format!("连接上游失败 {}: {}", target, e),
            )
            .await?;
            anyhow::bail!("连接上游 TCP 失败: {}", e);
        }
    };

    let mut upstream = match tls_connect_upstream(upstream_tcp, host).await {
        Ok(s) => s,
        Err(e) => {
            send_gateway_error(
                &mut client_stream,
                format!("TLS 上游握手失败 {}: {}", target, e),
            )
            .await?;
            anyhow::bail!("TLS 上游握手失败 {}: {}", target, e);
        }
    };

    // ── 告知客户端隧道已建立 ───────────────────────────────────
    client_stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;

    // ── 与客户端完成 TLS 握手（MITM）───────────────────────────
    let tls_client = match tls_accept_client(client_stream, &*cert_manager, host).await {
        Ok(s) => s,
        Err(e) => {
            warn!("TLS 握手失败 ({}): {}", host, e);
            return Ok(());
        }
    };

    // ── 复用隧道：循环读取请求直到连接关闭 ─────────────────────
    let mut tls_client = tls_client;
    loop {
        let req_data = match read_http_headers(&mut tls_client).await {
            Ok(d) => {
                if d.is_empty() {
                    break;
                }
                d
            }
            Err(e) => {
                tracing::debug!("读取请求头失败 (断连): {}", e);
                break;
            }
        };

        // 预判：SSE / chunked → 流式传输
        let is_streaming = is_streaming_request(&req_data);

        // 解析 + 识别
        let parsed = parse_request_data(&req_data);

        // 转发给上游
        if let Err(e) = upstream.write_all(&req_data).await {
            tracing::warn!("转发请求到上游失败: {}", e);
            break;
        }
        if let Err(e) = upstream.flush().await {
            tracing::warn!("刷新上游失败: {}", e);
            break;
        }

        if is_streaming {
            // ── SSE / chunked: 逐 chunk 转发 ──
            match stream_relay(&mut upstream, &mut tls_client).await {
                Ok(n) => tracing::debug!("流式转发 {} bytes ({})", n, host),
                Err(e) => tracing::debug!("流式转发结束: {}", e),
            }
            // SSE 是大响应，转发完后关闭此隧道（多数客户端会重建）
            break;
        } else {
            // ── 普通请求：读取完整响应 ──
            let resp_data = match read_http_response(&mut upstream).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("读取响应失败: {}", e);
                    break;
                }
            };

            // 解析并记录 LLM 调用（记录失败不影响代理）
            if let Some((req, agent_type)) = &parsed {
                if let Some(resp) = super::parser::ResponseParser::parse_response(
                    &req.provider,
                    &resp_data,
                    &req.id,
                ) {
                    if let Err(e) = store.record_turn(agent_type, req.clone(), resp) {
                        warn!("记录失败: {}", e);
                    }
                }
            }

            // 转发给客户端
            if let Err(e) = tls_client.write_all(&resp_data).await {
                tracing::debug!("转发响应给客户端失败: {}", e);
                break;
            }
            if let Err(e) = tls_client.flush().await {
                tracing::debug!("刷新客户端失败: {}", e);
                break;
            }

            // 检查 Connection: close
            if connection_close(&resp_data) {
                break;
            }
        }
    }

    // 关连接（ignore errors — they are normal on teardown）
    let _ = tls_client.shutdown().await;
    let _ = upstream.shutdown().await;
    Ok(())
}

// ─── helpers ─────────────────────────────────────────────────

/// 判断请求是否是 SSE 或 chunked（需要流式转发）
fn is_streaming_request(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(data);
    let lower = text.to_lowercase();
    lower.contains("content-type: text/event-stream")
        || lower.contains("accept: text/event-stream")
        || lower.contains("transfer-encoding: chunked")
}

/// 判断响应是否带 Connection: close
fn connection_close(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(data);
    text.to_lowercase()
        .lines()
        .any(|l| l.trim() == "connection: close" || l.trim() == "proxy-connection: close")
}

/// 从一个 TLS 流 relay 逐 chunk 到另一个，用于 SSE / chunked。
/// 返回已传输的总字节数。
async fn stream_relay<R, W>(upstream: &mut R, client: &mut W) -> Result<u64>
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut total: u64 = 0;
    let mut buf = vec![0u8; 8192];
    loop {
        match upstream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                client.write_all(&buf[..n]).await?;
                client.flush().await?;
                total += n as u64;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(total)
}

async fn drain_request_headers(stream: &mut TcpStream) -> Result<()> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).await?;
        buf.push(byte[0]);
        if buf.ends_with(CRLF) {
            break;
        }
        if buf.len() > 65_536 {
            anyhow::bail!("CONNECT 请求头过大");
        }
    }
    Ok(())
}

async fn send_gateway_error(stream: &mut TcpStream, msg: String) -> Result<()> {
    let resp = format!(
        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\n\r\n{}",
        msg.len(),
        msg
    );
    stream.write_all(resp.as_bytes()).await?;
    Ok(())
}

async fn tls_connect_upstream(
    upstream_tcp: TcpStream,
    host: &str,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let config = ClientConfig::builder()
        .with_root_certificates({
            let mut store = rustls::RootCertStore::empty();
            store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            store
        })
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let server_name = host
        .to_string()
        .try_into()
        .map_err(|_| anyhow::anyhow!("无效域名: {}", host))?;
    let stream = connector.connect(server_name, upstream_tcp).await?;
    Ok(stream)
}

async fn tls_accept_client(
    client_stream: TcpStream,
    cert_manager: &super::cert::CertManager,
    host: &str,
) -> Result<tokio_rustls::server::TlsStream<TcpStream>> {
    let tls_config = cert_manager.build_server_config(host)?;
    let acceptor = TlsAcceptor::from(tls_config);
    let stream = acceptor.accept(client_stream).await?;
    Ok(stream)
}
