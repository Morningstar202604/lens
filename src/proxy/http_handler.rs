//! 明文 HTTP 代理处理。
//!
//! 收到非 CONNECT 的 HTTP 请求时：
//! 1. 读取完整请求（头部 + body），请求行改写为 origin-form
//! 2. 解析 + agent 识别
//! 3. 转发到目标 host（Host 缺失时 fail-closed 返回 400，不猜测上游）
//! 4. 读取响应（流式边转发边聚合）→ 记录 + 回传

use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use super::httpbuf::HttpBuf;
use super::io::{
    decode_http_body, extract_host_header, is_chunked, read_full_request, strip_header,
    to_origin_form, IO_IDLE_TIMEOUT,
};
use super::parser::Provider;
use super::request::parse_request_data;
use super::server::ProxyOptions;
use super::sse::SseCollector;
use crate::engine::Store;

/// 在响应头部插入 `Connection: close`（若尚未声明），让客户端知道本连接不复用。
/// handle_http 每次只服务一个请求，若不声明，HTTP/1.1 客户端会尝试复用并遭 RST。
fn with_connection_close(resp: &[u8]) -> Vec<u8> {
    let Some(head_end) = resp.windows(4).position(|w| w == b"\r\n\r\n") else {
        return resp.to_vec();
    };
    let head = String::from_utf8_lossy(&resp[..head_end]);
    if head.to_lowercase().contains("connection:") {
        return resp.to_vec();
    }
    let mut out = Vec::with_capacity(resp.len() + 24);
    out.extend_from_slice(&resp[..head_end]);
    out.extend_from_slice(b"\r\nConnection: close");
    out.extend_from_slice(&resp[head_end..]);
    out
}

/// 请求是否期待 `100 Continue`（客户端会等 interim 响应后才发 body）。
fn expects_continue(head: &[u8]) -> bool {
    String::from_utf8_lossy(head)
        .to_lowercase()
        .contains("expect: 100-continue")
}

/// 处理一个明文 HTTP 请求。`head_bytes` 是分发器已读取的完整请求头。
pub async fn handle_http(
    head_bytes: &[u8],
    client_stream: TcpStream,
    store: &Store,
    options: &ProxyOptions,
) -> Result<()> {
    let mut client_stream = client_stream;

    // Expect: 100-continue —— 客户端在等 interim 响应，先放行再读 body，
    // 否则双方互等造成死锁。
    if expects_continue(head_bytes) {
        if let Err(e) = client_stream
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .await
        {
            tracing::debug!("发送 100 Continue 失败: {}", e);
            return Ok(());
        }
    }

    let req_data = match read_full_request(head_bytes, &mut client_stream).await {
        Ok(d) => d,
        Err(e) => {
            let msg = format!("读取请求失败: {e}");
            let resp = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                msg.len(),
                msg
            );
            let _ = client_stream.write_all(resp.as_bytes()).await;
            return Err(e);
        }
    };
    if req_data.is_empty() {
        return Ok(());
    }

    let req_str = String::from_utf8_lossy(&req_data);
    let parsed = parse_request_data(&req_data);

    let addr = match extract_host_header(&req_str) {
        Some(addr) => addr,
        None => {
            let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            client_stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };
    let forward_data = to_origin_form(&req_data);
    // 100-continue 已在上方本地应答，剥离后再转发，避免上游重复握手。
    let forward_data = if expects_continue(&req_data) {
        strip_header(&forward_data, "expect")
    } else {
        forward_data
    };

    let upstream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("连接目标失败 {}: {}", addr, e);
            let resp = format!(
                "HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                msg.len(),
                msg
            );
            client_stream.write_all(resp.as_bytes()).await?;
            anyhow::bail!("连接目标失败 {}", addr);
        }
    };
    let mut upstream = HttpBuf::new(upstream);

    upstream.inner_mut().write_all(&forward_data).await?;
    upstream.inner_mut().flush().await?;

    let head = upstream.read_head().await?;
    if head.is_empty() {
        return Ok(());
    }
    let head_str = String::from_utf8_lossy(&head).to_lowercase();
    let is_event_stream = head_str.contains("content-type: text/event-stream");

    if is_event_stream && is_chunked(&head_str) {
        let provider = parsed
            .as_ref()
            .map(|(r, _)| r.provider.clone())
            .unwrap_or(Provider::Unknown("stream".into()));
        let mut collector = SseCollector::new(provider);
        let head_out = with_connection_close(&head);
        upstream
            .relay_chunked_stream(&head_out, &mut client_stream, |payload| {
                collector.feed(payload)
            })
            .await?;
        if let Some((req, agent_type)) = &parsed {
            if let Some(resp) = collector.finish(&req.id) {
                if options.agents.allows(agent_type) {
                    crate::engine::writer::record_turn(
                        store,
                        options.writer.as_ref(),
                        agent_type,
                        "proxy",
                        req.clone(),
                        resp,
                    );
                }
            }
        }
    } else {
        let raw_body = upstream.read_raw_body(&head).await?;
        let mut full = head.clone();
        full.extend_from_slice(&raw_body);

        if let Some((req, agent_type)) = &parsed {
            let body = decode_http_body(&full);
            let resp = if is_event_stream {
                let mut collector = SseCollector::new(req.provider.clone());
                collector.feed(&body);
                collector.finish(&req.id)
            } else {
                super::parser::ResponseParser::parse_response(&req.provider, &body, &req.id)
            };
            if let Some(resp) = resp {
                if options.agents.allows(agent_type) {
                    crate::engine::writer::record_turn(
                        store,
                        options.writer.as_ref(),
                        agent_type,
                        "proxy",
                        req.clone(),
                        resp,
                    );
                }
            }
        }

        client_stream
            .write_all(&with_connection_close(&full))
            .await?;
    }
    let _ = tokio::time::timeout(IO_IDLE_TIMEOUT, client_stream.shutdown()).await;
    Ok(())
}
