//! 明文 HTTP 代理处理。
//!
//! 收到非 CONNECT 的 HTTP 请求时：
//! 1. 读取完整请求（头部 + body）
//! 2. 解析 + agent 识别
//! 3. 转发到目标 host
//! 4. 读取响应 → 记录 + 回传

use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::warn;

use super::io::{extract_host_header, read_full_request, read_http_response};
use super::request::parse_request_data;
use crate::engine::Store;

/// 处理一个明文 HTTP 请求。
/// `first_line_bytes` 包含已读取的首行（含 \r\n），用于拼接完整请求。
pub async fn handle_http(
    first_line_bytes: &[u8],
    client_stream: TcpStream,
    store: &Store,
) -> Result<()> {
    let mut client_stream = client_stream;

    // 读取完整请求
    let req_data = read_full_request(first_line_bytes, &mut client_stream).await?;
    if req_data.is_empty() {
        return Ok(());
    }

    // 解析请求 + agent 识别
    let parsed = parse_request_data(&req_data);

    // 提取 Host（含端口）
    let req_str = String::from_utf8_lossy(&req_data);
    let addr = extract_host_header(&req_str).unwrap_or_else(|| "localhost:80".to_string());

    // 连接到目标服务器
    let mut upstream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("连接目标失败 {}: {}", addr, e);
            let resp = format!(
                "HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\n\r\n{}",
                msg.len(),
                msg
            );
            client_stream.write_all(resp.as_bytes()).await?;
            anyhow::bail!("连接目标失败 {}", addr);
        }
    };

    // 转发请求
    upstream.write_all(&req_data).await?;

    // 读取响应
    let resp_data = read_http_response(&mut upstream).await?;

    // 尝试解析 LLM 响应
    if let Some((req, agent_type)) = parsed {
        // 分离 body 交给 parser
        let body = extract_body(&resp_data);
        if let Some(resp) =
            super::parser::ResponseParser::parse_response(&req.provider, body, &req.id)
        {
            if let Err(e) = store.record_turn(&agent_type, req.clone(), resp) {
                warn!("记录失败: {}", e);
            }
        }
    }

    // 回传响应
    client_stream.write_all(&resp_data).await?;
    Ok(())
}

/// 从完整的 HTTP 响应数据中提取 body 部分。
fn extract_body(data: &[u8]) -> &[u8] {
    let pos = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(0);
    &data[pos..]
}
