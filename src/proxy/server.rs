//! HTTP/HTTPS 透明代理服务器 — 入口与连接分发。
//!
//! 工作流程：
//! 1. 监听 TCP 连接
//! 2. 读取首行判断 CONNECT vs 明文 HTTP
//! 3. 分发到 `connect` 或 `http_handler` 模块

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use tokio::io::AsyncBufReadExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::info;

use super::cert::CertManagerArc;
use crate::engine::Store;

pub type Db = Store;

pub async fn run(bind: &str, cert_dir: &str, data_dir: &Path) -> Result<()> {
    // 安装 rustls CryptoProvider（ring 后端），幂等
    if rustls::crypto::ring::default_provider().install_default().is_err() {
        // 已安装过，忽略
    }

    let addr: std::net::SocketAddr = bind.parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!("代理监听: bind={}", bind);

    let cert_manager: CertManagerArc =
        Arc::new(super::cert::CertManager::init(Path::new(cert_dir))?);
    let store = Arc::new(Store::open(data_dir)?);

    loop {
        let (stream, _peer_addr) = listener.accept().await?;
        let cm = cert_manager.clone();
        let st = store.clone();

        tokio::spawn(async move {
            if let Err(e) = dispatch_connection(stream, cm, st).await {
                tracing::warn!("连接处理失败: {}", e);
            }
        });
    }
}

/// Read the first request line, then dispatch to the right handler.
async fn dispatch_connection(
    stream: TcpStream,
    cert_manager: CertManagerArc,
    store: Arc<Store>,
) -> Result<()> {
    let mut buf_reader = tokio::io::BufReader::new(stream);

    // Read the first line efficiently (no more byte-by-byte).
    let mut first_line = String::new();
    buf_reader.read_line(&mut first_line).await?;

    if first_line.trim_end().is_empty() {
        return Ok(());
    }

    // Take the underlying stream back for handlers that need it.
    let stream = buf_reader.into_inner();

    if first_line.starts_with("CONNECT ") {
        super::connect::handle_connect(&first_line, stream, cert_manager, store).await
    } else {
        let first_line_bytes = format!("{first_line}\r\n");
        super::http_handler::handle_http(first_line_bytes.as_bytes(), stream, &store).await
    }
}

// Re-exports for backwards compatibility.
pub use super::connect::handle_connect;
pub use super::http_handler::handle_http;
