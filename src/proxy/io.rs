//! HTTP I/O utilities: reading requests/responses from streams.
//!
//! 修复：
//! - `read_http_response` 支持 chunked transfer encoding
//! - 头部大小限制从 1MB 降至 256KB（防 DoS）
//! - 通过 `MAX_BODY_SIZE` 常量限制 body 大小（防内存溢出）

use anyhow::Result;
use tokio::io::AsyncReadExt;

/// Maximum body size (10 MiB). Larger responses are truncated.
pub const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Maximum header block size (256 KiB).
const MAX_HEADER_SIZE: usize = 262_144;

/// Re-export BufReader for server.rs.
pub use tokio::io::BufReader as IoBufReader;

/// 读取完整的 HTTP 头部（到 \r\n\r\n）。
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
        if data.len() > MAX_HEADER_SIZE {
            anyhow::bail!("HTTP 头部超过 {MAX_HEADER_SIZE} bytes 限制");
        }
    }
    Ok(data)
}

/// 读取完整的 HTTP 响应（含 body）。
///
/// 支持三种 body 读取方式：
/// 1. `Content-Length` — 精确读取 N 字节（受 `MAX_BODY_SIZE` 限制）
/// 2. `Transfer-Encoding: chunked` — 分块读取到 0-length chunk
/// 3. 无前两者 — 读到 EOF（fallback，通常不会发生在 LLM API）
pub async fn read_http_response<S: AsyncReadExt + Unpin>(stream: &mut S) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];

    // ── 读头部 ────────────────────────────────────────────────
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(data);
        }
        data.extend_from_slice(&buf[..n]);
        if data.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if data.len() > MAX_HEADER_SIZE {
            anyhow::bail!("响应头超过 {MAX_HEADER_SIZE} bytes");
        }
    }

    let header_len = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(data.len());

    let header_str = String::from_utf8_lossy(&data[..header_len]);

    // ── 判断传输方式 ──────────────────────────────────────────
    if is_chunked(&header_str) {
        // 分块编码：读取直到 0-length chunk
        let mut remaining = MAX_BODY_SIZE;
        while remaining > 0 {
            let mut size_line = String::new();
            // 需要用 BufReader 来按行读 chunk size
            let mut one_byte = [0u8; 1];
            loop {
                stream.read_exact(&mut one_byte).await?;
                size_line.push(one_byte[0] as char);
                if size_line.ends_with("\r\n") {
                    break;
                }
                if size_line.len() > 32 {
                    anyhow::bail!("chunk size 行过长");
                }
            }
            let chunk_size = usize::from_str_radix(size_line.trim(), 16)
                .map_err(|_| anyhow::anyhow!("无效 chunk size: {:?}", size_line.trim()))?;
            if chunk_size == 0 {
                // 读最后的 CRLF
                let mut trailer = [0u8; 2];
                let _ = stream.read_exact(&mut trailer).await;
                break;
            }
            let to_read = chunk_size.min(remaining);
            let start = data.len();
            data.resize(start + to_read, 0);
            stream.read_exact(&mut data[start..start + to_read]).await?;
            if to_read < chunk_size {
                let mut skip = vec![0u8; chunk_size - to_read];
                stream.read_exact(&mut skip).await?;
            }
            // chunk 后的 CRLF
            let mut crlf = [0u8; 2];
            let _ = stream.read_exact(&mut crlf).await;
            remaining -= to_read;
        }
        Ok(data)
    } else if let Some(body_len) = find_content_length_str(&header_str) {
        // Content-Length: 读取指定字节数
        let clamped = body_len.min(MAX_BODY_SIZE);
        let total_needed = header_len + clamped;
        while data.len() < total_needed {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        Ok(data)
    } else {
        // fallback: read to EOF
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
            if data.len() > MAX_HEADER_SIZE + MAX_BODY_SIZE {
                break;
            }
        }
        Ok(data)
    }
}

/// Check whether the response is chunked-encoded.
fn is_chunked(header_str: &str) -> bool {
    header_str
        .lines()
        .any(|l| l.to_lowercase().contains("transfer-encoding: chunked"))
}

/// Extract `Content-Length` value from an HTTP header block.
pub fn find_content_length(data: &[u8]) -> Option<usize> {
    let s = String::from_utf8_lossy(data);
    find_content_length_str(&s)
}

/// Extract `Content-Length` value from a string.
pub fn find_content_length_str(s: &str) -> Option<usize> {
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

/// Extract `Host` value (with port) from a request string.
pub fn extract_host_header(req: &str) -> Option<String> {
    for line in req.split("\r\n") {
        if line.to_lowercase().starts_with("host:") {
            let v = line[5..].trim();
            return Some(v.to_string());
        }
    }
    None
}

/// Extract hostname (without port) from a request string.
pub fn find_host(req_str: &str) -> Option<String> {
    for line in req_str.lines() {
        if line.to_lowercase().starts_with("host:") {
            let value = line[5..].trim();
            return value.split(':').next().map(str::to_owned);
        }
    }
    None
}

/// 读取一个完整请求（头部 + body）。
/// `prefix` 是已经读取到的字节（通常是首行）。
pub async fn read_full_request<S: AsyncReadExt + Unpin>(
    prefix: &[u8],
    stream: &mut S,
) -> Result<Vec<u8>> {
    let mut data = prefix.to_vec();
    let mut buf = [0u8; 8192];

    loop {
        if data.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        if data.len() > 65_536 {
            anyhow::bail!("请求头过大");
        }
    }

    read_http_body(&mut data, stream).await?;
    Ok(data)
}

/// 根据 Content-Length 补充 body 到 `data`。
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
    let clamped_len = declared_len.min(MAX_BODY_SIZE);
    let total_needed = header_end + clamped_len;

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
