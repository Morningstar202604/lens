//! HTTP/HTTPS 透明代理服务器 — 入口与连接分发。
//!
//! 工作流程：
//! 1. 监听 TCP 连接
//! 2. 读取首行判断 CONNECT vs 明文 HTTP
//! 3. 分发到 `connect` 或 `http_handler` 模块
//!
//! 实际的处理逻辑已拆分为独立模块：
//! - `connect.rs` — HTTPS CONNECT 隧道（MITM TLS）
//! - `http_handler.rs` — 明文 HTTP 代理
//! - `io.rs` — I/O 辅助函数

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::info;

use super::cert::CertManagerArc;
use crate::engine::Store;

pub async fn run(bind: &str, cert_dir: &str, data_dir: &Path) -> Result<()> {
    // 安装 rustls CryptoProvider（ring 后端），幂等
    if rustls::crypto::ring::default_provider().install_default().is_err() {
        // 已安装过，忽略
    }

    let addr: std::net::SocketAddr = bind.parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!("代理监听: bind={}", bind);

    let cert_manager: CertManagerArc = Arc::new(super::cert::CertManager::init(Path::new(cert_dir))?);
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

/// 读取首行并分发到对应处理器。
async fn dispatch_connection(
    mut stream: TcpStream,
    cert_manager: CertManagerArc,
    store: Arc<Store>,
) -> Result<()> {
    // 读取请求首行
    let mut line_buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).await?;
        line_buf.push(byte[0]);
        if line_buf.ends_with(b"\r\n") {
            break;
        }
        if line_buf.len() > 8192 {
            anyhow::bail!("请求行过长");
        }
    }

    let first_line = String::from_utf8_lossy(&line_buf);
    let first_line = first_line.trim();

    if first_line.starts_with("CONNECT ") {
        super::connect::handle_connect(first_line, stream, cert_manager, store).await
    } else {
        super::http_handler::handle_http(&line_buf, stream, &store).await
    }
}

// Re-export so existing code using `super::server::run` still works.
pub use super::connect::handle_connect;
pub use super::http_handler::handle_http;
