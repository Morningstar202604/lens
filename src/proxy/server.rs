//! HTTP/HTTPS 透明代理服务器 — 入口与连接分发。
//!
//! 工作流程：
//! 1. 监听 TCP 连接
//! 2. 读取**完整请求头**（首行 + 全部头部，允许携带多余字节），据首行分发
//! 3. CONNECT → `connect`；其余 → `http_handler`

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use tokio::net::{TcpListener, TcpStream};
use tracing::info;

use super::cert::CertManagerArc;
use super::connect::UpstreamTls;
use super::io::read_http_headers;
use crate::engine::Store;

pub type Db = Store;

/// 录制开关（按 agent 类型）。默认全部开启。
#[derive(Clone, Debug)]
pub struct AgentFilter {
    pub opencode: bool,
    pub codex: bool,
    pub claude_code: bool,
    pub other: bool,
}

impl Default for AgentFilter {
    fn default() -> Self {
        Self {
            opencode: true,
            codex: true,
            claude_code: true,
            other: true,
        }
    }
}

impl AgentFilter {
    pub fn allows(&self, agent_type: &str) -> bool {
        match agent_type {
            "opencode" => self.opencode,
            "codex" => self.codex,
            "claude-code" => self.claude_code,
            _ => self.other,
        }
    }
}

/// 代理运行选项。
#[derive(Clone, Debug, Default)]
pub struct ProxyOptions {
    pub upstream_tls: UpstreamTls,
    pub agents: AgentFilter,
    /// 单写入者队列;None 时在请求路径同步落库(测试直连用)。
    pub writer: Option<crate::engine::writer::WriterHandle>,
}

pub async fn run(bind: &str, cert_dir: &str, data_dir: &Path) -> Result<()> {
    run_with(bind, cert_dir, data_dir, ProxyOptions::default()).await
}

pub async fn run_with(
    bind: &str,
    cert_dir: &str,
    data_dir: &Path,
    options: ProxyOptions,
) -> Result<()> {
    // 安装 rustls CryptoProvider（ring 后端），幂等
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        // 已安装过，忽略
    }

    let addr: std::net::SocketAddr = bind.parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!("代理监听: bind={}", bind);

    let cert_manager: CertManagerArc =
        Arc::new(super::cert::CertManager::init(Path::new(cert_dir))?);
    let store = Arc::new(Store::open(data_dir)?);
    let mut options = options;
    if options.writer.is_none() {
        options.writer = Some(crate::engine::writer::WriterHandle::spawn(store.clone()));
    }

    loop {
        let (stream, _peer_addr) = listener.accept().await?;
        let cm = cert_manager.clone();
        let st = store.clone();
        let opts = options.clone();

        tokio::spawn(async move {
            if let Err(e) = dispatch_connection(stream, cm, st, opts).await {
                tracing::warn!("连接处理失败: {}", e);
            }
        });
    }
}

/// 读取完整请求头（无缓冲丢失），按首行分发。
async fn dispatch_connection(
    mut stream: TcpStream,
    cert_manager: CertManagerArc,
    store: Arc<Store>,
    options: ProxyOptions,
) -> Result<()> {
    let head = read_http_headers(&mut stream).await?;
    if head.is_empty() {
        return Ok(());
    }

    let head_str = String::from_utf8_lossy(&head);
    let first_line = head_str.lines().next().unwrap_or_default();
    if first_line.trim().is_empty() {
        return Ok(());
    }

    if first_line.starts_with("CONNECT ") {
        super::connect::handle_connect(&head, stream, cert_manager, store, options).await
    } else {
        super::http_handler::handle_http(&head, stream, &store, &options).await
    }
}

// Re-exports for backwards compatibility.
pub use super::connect::handle_connect;
pub use super::http_handler::handle_http;
