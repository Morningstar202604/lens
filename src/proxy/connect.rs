//! HTTPS CONNECT 隧道处理。
//!
//! 收到 `CONNECT host:443` 后：
//! 1. 与上游建立 TCP + TLS 连接
//! 2. 回复客户端 200 Connection Established
//! 3. 用动态证书与客户端完成 TLS 握手（MITM）
//! 4. 读取明文 HTTP 请求 → 转发 → 读取响应 → 记录 → 回传

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

/// 处理一个 CONNECT 隧道请求。
/// `request_line` 是完整的 CONNECT 请求行，例如 `CONNECT api.example.com:443 HTTP/1.1`。
pub async fn handle_connect(
    request_line: &str,
    mut client_stream: TcpStream,
    cert_manager: CertManagerArc,
    store: Arc<Store>,
) -> Result<()> {
    // 解析目标 host:port
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        anyhow::bail!("CONNECT 格式错误");
    }
    let target = parts[1];
    let host = target.split(':').next().unwrap_or(target);

    info!("CONNECT → {}", target);

    // 消耗剩余的请求头（到 \r\n\r\n）
    // 注意：首行已经读走，剩余头需要读完才能开始 TLS 握手
    drain_request_headers(&mut client_stream).await?;

    // 与上游建立 TCP 连接
    let upstream_tcp = match TcpStream::connect(target).await {
        Ok(s) => s,
        Err(e) => {
            send_gateway_error(&mut client_stream, &format!("连接上游失败 {}: {}", target, e))
                .await?;
            anyhow::bail!("连接上游 TCP 失败: {}", e);
        }
    };

    // TLS 连接到上游
    let mut upstream = match tls_connect_upstream(upstream_tcp, host).await {
        Ok(s) => s,
        Err(e) => {
            send_gateway_error(
                &mut client_stream,
                &format!("TLS 上游握手失败 {}: {}", target, e),
            )
            .await?;
            anyhow::bail!("TLS 上游握手失败 {}: {}", target, e);
        }
    };

    // 回复客户端隧道已建立
    client_stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;

    // 用动态证书与客户端完成 TLS 握手（MITM）
    let tls_client = match tls_accept_client(client_stream, &*cert_manager, host).await {
        Ok(s) => s,
        Err(e) => {
            warn!("TLS 握手失败 ({}): {}", host, e);
            return Ok(());
        }
    };

    // 现在 tls_client 是解密的 HTTP 明文通道
    let mut tls_client = tls_client;

    // 读取完整 HTTP 请求
    let req_data = read_http_headers(&mut tls_client).await?;
    if req_data.is_empty() {
        return Ok(());
    }

    // 解析请求 + agent 识别
    let parsed = parse_request_data(&req_data);

    // 转发请求到上游
    upstream.write_all(&req_data).await?;

    // 读取完整响应
    let resp_data = read_http_response(&mut upstream).await?;

    // 尝试解析 LLM 响应并记录
    if let Some((req, agent_type)) = parsed {
        if let Some(resp) =
            super::parser::ResponseParser::parse_response(&req.provider, &resp_data, &req.id)
        {
            if let Err(e) = store.record_turn(&agent_type, req.clone(), resp) {
                warn!("记录失败: {}", e);
            }
        }
    }

    // 转发响应给客户端
    tls_client.write_all(&resp_data).await?;
    if let Err(e) = tls_client.shutdown().await {
        tracing::debug!("客户端 TLS 关闭: {}", e);
    }
    if let Err(e) = upstream.shutdown().await {
        tracing::debug!("上游 TLS 关闭: {}", e);
    }

    Ok(())
}

// ─── internals ───────────────────────────────────────────────

async fn drain_request_headers(stream: &mut TcpStream) -> Result<()> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).await?;
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if buf.len() > 65536 {
            anyhow::bail!("CONNECT 请求头过大");
        }
    }
    Ok(())
}

async fn send_gateway_error(stream: &mut TcpStream, msg: &str) -> Result<()> {
    let body = msg.as_bytes();
    let resp = format!(
        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        msg
    );
    stream.write_all(resp.as_bytes()).await?;
    Ok(())
}

async fn tls_connect_upstream(upstream_tcp: TcpStream, host: &str) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
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
