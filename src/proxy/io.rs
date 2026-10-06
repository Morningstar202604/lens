//! HTTP I/O utilities: reading requests/responses from TLS streams.

use anyhow::Result;
use tokio::io::AsyncReadExt;

/// 读取完整的 HTTP 头部（到 \r\n\r\n）。
/// 返回读取到的所有字节（包含 body 片段，如果有）。
pub async fn read_http_headers<S: AsyncReadExt + Unpin>(stream: &mut S) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        if data.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if data.len() > 1_000_000 {
            anyhow::bail!("HTTP 头部超过 1MB 限制");
        }
    }
    Ok(data)
}

/// 读取完整的 HTTP 响应（含 body，按 Content-Length）。
/// 如果无法解析 Content-Length，则返回已读到的所有数据。
pub async fn read_http_response<S: AsyncReadExt + Unpin>(stream: &mut S) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];

    // 读头部
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(data);
        }
        data.extend_from_slice(&buf[..n]);
        if data.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if data.len() > 262144 {
            anyhow::bail!("响应头超过 256KB");
        }
    }

    // 解析 Content-Length
    let header_len = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(data.len());

    let body_len = find_content_length(&data).unwrap_or(0);
    let total_needed = header_len + body_len;

    // 继续读 body
    while data.len() < total_needed {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }

    Ok(data)
}

/// 读取 HTTP body（根据 Content-Length 从 stream 中补充剩余部分）。
/// `data` 应已包含完整的 HTTP 头部。
pub async fn read_http_body<S: AsyncReadExt + Unpin>(
    data: &mut Vec<u8>,
    stream: &mut S,
) -> Result<()> {
    let header_end = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(data.len());

    let declared_len = find_content_length(data).unwrap_or(0);
    let total_needed = header_end + declared_len;

    if data.len() >= total_needed {
        return Ok(());
    }

    let mut buf = [0u8; 8192];
    while data.len() < total_needed {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    Ok(())
}

/// 从 HTTP 头部块中提取 Content-Length 值。
pub fn find_content_length(data: &[u8]) -> Option<usize> {
    let s = String::from_utf8_lossy(data);
    for line in s.split("\r\n") {
        if line.to_lowercase().starts_with("content-length:") {
            let val = line[15..].trim();
            if let Ok(n) = val.parse::<usize>() {
                return Some(n);
            }
        }
    }
    None
}

/// 从 HTTP 请求字符串中提取 Host 头的值（含端口）。
pub fn extract_host_header(req: &str) -> Option<String> {
    for line in req.split("\r\n") {
        if line.to_lowercase().starts_with("host:") {
            let v = line[5..].trim();
            return Some(v.to_string());
        }
    }
    None
}

/// 读取一个完整请求（头部 + body）从给定的 stream。
/// `prefix` 是已经读取到的字节（通常是首行）。
pub async fn read_full_request<S: AsyncReadExt + Unpin>(
    prefix: &[u8],
    stream: &mut S,
) -> Result<Vec<u8>> {
    let mut data = prefix.to_vec();
    let mut buf = [0u8; 8192];

    // 先读到头部完整
    loop {
        if data.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        if data.len() > 65536 {
            anyhow::bail!("请求头过大");
        }
    }

    // 根据 Content-Length 补充 body
    read_http_body(&mut data, stream).await?;
    Ok(data)
}
