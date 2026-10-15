//! 带残余缓冲的 HTTP 读取器。
//!
//! 一次 `read()` 常把"头部 + 若干 body 字节"一起带回；本模块保证这些
//! 多余字节留在缓冲里被后续按框架消费，不再出现字节丢失或框架错位。

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::io::{
    find_content_length_str, is_chunked, MAX_BODY_SIZE, MAX_HEADER_SIZE, UPSTREAM_READ_TIMEOUT,
};

pub struct HttpBuf<S> {
    stream: S,
    buf: Vec<u8>,
    pos: usize,
    /// 上游读取空闲超时（可调，默认 [`UPSTREAM_READ_TIMEOUT`]）。
    read_timeout: std::time::Duration,
}

impl<S: AsyncReadExt + Unpin> HttpBuf<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            buf: Vec::new(),
            pos: 0,
            read_timeout: UPSTREAM_READ_TIMEOUT,
        }
    }

    /// 覆盖上游读取空闲超时。
    pub fn with_read_timeout(mut self, t: std::time::Duration) -> Self {
        self.read_timeout = t;
        self
    }

    /// 取出底层流用于写（请求转发等）。
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    fn compact(&mut self) {
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
    }

    fn available(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    async fn fill(&mut self) -> Result<usize> {
        self.compact();
        let mut tmp = [0u8; 8192];
        let n = tokio::time::timeout(self.read_timeout, self.stream.read(&mut tmp))
            .await
            .map_err(|_| anyhow::anyhow!("读取上游超时（{:?} 无数据）", self.read_timeout))??;
        if n > 0 {
            self.buf.extend_from_slice(&tmp[..n]);
        }
        Ok(n)
    }

    /// 读取完整头部（到 `\r\n\r\n`），消费到终止符为止；多余字节留在缓冲。
    /// EOF 时返回剩余残片（调用方据此判断断连）。
    pub async fn read_head(&mut self) -> Result<Vec<u8>> {
        loop {
            if let Some(p) = find_crlfcrlf(self.available()) {
                let head = self.available()[..p + 4].to_vec();
                self.pos += p + 4;
                return Ok(head);
            }
            if self.available().len() > MAX_HEADER_SIZE {
                anyhow::bail!("HTTP 头部超过 {MAX_HEADER_SIZE} bytes 限制");
            }
            if self.fill().await? == 0 {
                let rest = self.available().to_vec();
                self.pos += rest.len();
                return Ok(rest);
            }
        }
    }

    /// 按响应头部框架读取 body 的**原始字节**（chunked 框架保留，便于原样转发）。
    /// 超过 `MAX_BODY_SIZE` 时 fail-closed（调用方应中断连接，避免框架错位）。
    pub async fn read_raw_body(&mut self, head: &[u8]) -> Result<Vec<u8>> {
        let head_str = String::from_utf8_lossy(head).to_string();
        if is_chunked(&head_str) {
            let mut out = Vec::new();
            loop {
                let line = self.read_line(64).await?;
                if line.is_empty() {
                    break;
                }
                out.extend_from_slice(&line);
                let size = parse_chunk_size(&line)?;
                if size == 0 {
                    loop {
                        let trailer = self.read_line(MAX_HEADER_SIZE).await?;
                        out.extend_from_slice(&trailer);
                        if trailer == b"\r\n" || trailer.is_empty() {
                            break;
                        }
                    }
                    break;
                }
                let data = self.read_exact_n(size).await?;
                out.extend_from_slice(&data);
                out.extend_from_slice(&self.read_exact_n(2).await?);
                if out.len() > MAX_BODY_SIZE {
                    anyhow::bail!("响应体超过 {MAX_BODY_SIZE} bytes 上限");
                }
            }
            Ok(out)
        } else if let Some(len) = find_content_length_str(&head_str) {
            if len > MAX_BODY_SIZE {
                anyhow::bail!("响应体超过 {MAX_BODY_SIZE} bytes 上限");
            }
            self.read_exact_n(len).await
        } else {
            let mut out = self.available().to_vec();
            self.pos += out.len();
            loop {
                // read to EOF
                self.compact();
                let mut tmp = [0u8; 8192];
                let n = self.stream.read(&mut tmp).await?;
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&tmp[..n]);
                if out.len() > MAX_BODY_SIZE {
                    break;
                }
            }
            Ok(out)
        }
    }

    /// 流式转发 chunked 响应：先原样送出 head，随后逐 chunk 转发；
    /// 每个 chunk 的 payload 交给 `on_payload`（聚合录制用）。
    /// 返回前保证已送达终止 0-chunk（否则为上游断连）。
    pub async fn relay_chunked_stream<C, F>(
        &mut self,
        head: &[u8],
        client: &mut C,
        mut on_payload: F,
    ) -> Result<()>
    where
        C: AsyncWriteExt + Unpin,
        F: FnMut(&[u8]),
    {
        client.write_all(head).await?;
        client.flush().await?;
        let mut total: usize = 0;
        loop {
            let line = self.read_line(64).await?;
            if line.is_empty() {
                anyhow::bail!("流式响应在上游断连前未终止");
            }
            client.write_all(&line).await?;
            let size = parse_chunk_size(&line)?;
            if size == 0 {
                loop {
                    let trailer = self.read_line(MAX_HEADER_SIZE).await?;
                    client.write_all(&trailer).await?;
                    if trailer == b"\r\n" || trailer.is_empty() {
                        break;
                    }
                }
                client.flush().await?;
                return Ok(());
            }
            let data = self.read_exact_n(size).await?;
            let crlf = self.read_exact_n(2).await?;
            client.write_all(&data).await?;
            client.write_all(&crlf).await?;
            client.flush().await?;
            if total <= MAX_BODY_SIZE {
                total += data.len();
                on_payload(&data);
            }
        }
    }

    async fn read_line(&mut self, max: usize) -> Result<Vec<u8>> {
        loop {
            if let Some(p) = self.available().windows(2).position(|w| w == b"\r\n") {
                let line = self.available()[..p + 2].to_vec();
                self.pos += p + 2;
                return Ok(line);
            }
            if self.available().len() > max {
                anyhow::bail!("HTTP 行超过 {max} bytes 限制");
            }
            if self.fill().await? == 0 {
                let rest = self.available().to_vec();
                self.pos += rest.len();
                return Ok(rest);
            }
        }
    }

    async fn read_exact_n(&mut self, n: usize) -> Result<Vec<u8>> {
        while self.available().len() < n {
            if self.fill().await? == 0 {
                break;
            }
        }
        let take = n.min(self.available().len());
        let out = self.available()[..take].to_vec();
        self.pos += take;
        Ok(out)
    }
}

pub fn find_crlfcrlf(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_chunk_size(line: &[u8]) -> Result<usize> {
    let s = String::from_utf8_lossy(line);
    let digits = s.trim().split(';').next().unwrap_or("0").trim();
    usize::from_str_radix(digits, 16).map_err(|_| anyhow::anyhow!("无效 chunk size: {digits:?}"))
}
