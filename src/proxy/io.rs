//! HTTP I/O utilities: reading requests/responses from streams.
//!
//! 响应读取（含 chunked 框架）见 `httpbuf`；本模块保留请求读取与
//! body 解码等纯函数。

use anyhow::Result;
use tokio::io::AsyncReadExt;

/// Maximum body size (10 MiB). Larger responses are truncated.
pub const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Maximum header block size (256 KiB).
pub(crate) const MAX_HEADER_SIZE: usize = 262_144;

/// Re-export BufReader for server.rs.
pub use tokio::io::BufReader as IoBufReader;

/// 读取完整的 HTTP 头部（到 \r\n\r\n）。
/// 返回的字节可能包含紧随头部的多余数据，调用方需按前缀语义继续处理。
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

/// Check whether the response is chunked-encoded.
pub(crate) fn is_chunked(header_str: &str) -> bool {
    header_str
        .lines()
        .any(|l| l.to_lowercase().contains("transfer-encoding: chunked"))
}

/// 从"头部 + 原始 body"的完整响应中解出解码后的 body。
/// 支持 Content-Length 与 chunked；无法判定时返回原始 body 段。
pub fn decode_http_body(resp: &[u8]) -> Vec<u8> {
    let head_end = match resp.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(p) => p + 4,
        None => return Vec::new(),
    };
    let head = String::from_utf8_lossy(&resp[..head_end]);
    let raw = &resp[head_end..];
    if is_chunked(&head) {
        dechunk(raw)
    } else if let Some(len) = find_content_length_str(&head) {
        raw[..raw.len().min(len)].to_vec()
    } else {
        raw.to_vec()
    }
}

/// 去掉 chunked 分块框架，返回 payload。
fn dechunk(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < raw.len() {
        let line_end = match raw[i..].windows(2).position(|w| w == b"\r\n") {
            Some(p) => i + p,
            None => break,
        };
        let size_str = String::from_utf8_lossy(&raw[i..line_end]);
        let size_digits = size_str.trim().split(';').next().unwrap_or("0").trim();
        let size = usize::from_str_radix(size_digits, 16).unwrap_or(0);
        let start = line_end + 2;
        if size == 0 || start > raw.len() {
            break;
        }
        let end = (start + size).min(raw.len());
        out.extend_from_slice(&raw[start..end]);
        i = end + 2;
    }
    out
}

/// 将代理请求行从 absolute-form 改写为 origin-form（转发给源站时必须）。
/// `POST http://host:port/path HTTP/1.1` → `POST /path HTTP/1.1`
pub fn to_origin_form(req: &[u8]) -> Vec<u8> {
    let line_end = match req.windows(2).position(|w| w == b"\r\n") {
        Some(p) => p,
        None => return req.to_vec(),
    };
    let Ok(line) = std::str::from_utf8(&req[..line_end]) else {
        return req.to_vec();
    };
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return req.to_vec();
    };
    let origin = match target.find("://") {
        Some(scheme_end) => {
            let after_scheme = &target[scheme_end + 3..];
            match after_scheme.find('/') {
                Some(p) => &after_scheme[p..],
                None => "/",
            }
        }
        None => target,
    };
    let mut out = Vec::with_capacity(req.len());
    out.extend_from_slice(format!("{method} {origin} {version}\r\n").as_bytes());
    out.extend_from_slice(&req[line_end + 2..]);
    out
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
