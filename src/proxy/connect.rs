//! HTTPS CONNECT 隧道处理。
//!
//! 收到 `CONNECT host:443` 后：
//! 1. 与上游建立 TCP + TLS 连接（上游信任策略由 `UpstreamTls` 决定）
//! 2. 回复客户端 200 Connection Established
//! 3. 用动态证书与客户端完成 TLS 握手（MITM）
//! 4. 循环读取完整请求（头 + body）→ 转发 → 读取响应 → 记录 → 回传
//!
//! 分发器已消费 CONNECT 的完整请求头（`head`），此处不再重复读取。

use anyhow::Result;
use rustls::ClientConfig;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::{info, warn};

use super::cert::CertManagerArc;
use super::httpbuf::HttpBuf;
use super::io::{read_full_request_with_timeout, TUNNEL_IDLE_TIMEOUT, UPSTREAM_CONNECT_TIMEOUT};
use super::parser::Provider;
use super::request::parse_request_data;
use super::server::ProxyOptions;
use super::sse::SseCollector;
use crate::engine::Store;

/// 上游 TLS 信任策略。
#[derive(Clone, Debug, Default)]
pub enum UpstreamTls {
    /// 公共 CA（webpki-roots）。默认。
    #[default]
    SystemRoots,
    /// 在公共 CA 之外额外信任指定 PEM 文件中的 CA 证书（企业自签网关）。
    ExtraCa(PathBuf),
    /// 跳过上游证书校验（仅调试用途，启动时会打印警告）。
    Insecure,
}

/// 处理一个 CONNECT 隧道请求。`head` 为分发器读取的完整 CONNECT 请求头。
pub async fn handle_connect(
    head: &[u8],
    mut client_stream: TcpStream,
    cert_manager: CertManagerArc,
    store: Arc<Store>,
    options: ProxyOptions,
) -> Result<()> {
    let head_str = String::from_utf8_lossy(head);
    let request_line = head_str.lines().next().unwrap_or_default();
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 || !parts[0].eq_ignore_ascii_case("CONNECT") {
        anyhow::bail!("CONNECT 格式错误");
    }
    let target = parts[1];
    let host = target.split(':').next().unwrap_or(target);

    info!("CONNECT → {}", target);

    // ── 上游 TCP + TLS ─────────────────────────────────────────
    let upstream_tcp =
        match tokio::time::timeout(UPSTREAM_CONNECT_TIMEOUT, TcpStream::connect(target)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                send_gateway_error(
                    &mut client_stream,
                    format!("连接上游失败 {}: {}", target, e),
                )
                .await?;
                anyhow::bail!("连接上游 TCP 失败: {}", e);
            }
            Err(_) => {
                send_gateway_error(
                    &mut client_stream,
                    format!("连接上游超时 {}（{UPSTREAM_CONNECT_TIMEOUT:?}）", target),
                )
                .await?;
                anyhow::bail!("连接上游 TCP 超时: {}", target);
            }
        };

    let upstream_tls_stream = match tokio::time::timeout(
        UPSTREAM_CONNECT_TIMEOUT,
        tls_connect_upstream(upstream_tcp, host, &options.upstream_tls),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            send_gateway_error(
                &mut client_stream,
                format!("TLS 上游握手失败 {}: {}", target, e),
            )
            .await?;
            anyhow::bail!("TLS 上游握手失败 {}: {}", target, e);
        }
        Err(_) => {
            send_gateway_error(
                &mut client_stream,
                format!(
                    "TLS 上游握手超时 {}（{UPSTREAM_CONNECT_TIMEOUT:?}）",
                    target
                ),
            )
            .await?;
            anyhow::bail!("TLS 上游握手超时: {}", target);
        }
    };
    let mut upstream = HttpBuf::new(upstream_tls_stream);

    // ── 告知客户端隧道已建立 ───────────────────────────────────
    client_stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;

    // ── 与客户端完成 TLS 握手（MITM）───────────────────────────
    let tls_client = match tls_accept_client(client_stream, &cert_manager, host).await {
        Ok(s) => s,
        Err(e) => {
            warn!("TLS 握手失败 ({}): {}", host, e);
            return Ok(());
        }
    };

    // ── 复用隧道：循环读取完整请求直到连接关闭 ─────────────────
    let mut tls_client = tls_client;
    loop {
        let req_data =
            match read_full_request_with_timeout(&[], &mut tls_client, TUNNEL_IDLE_TIMEOUT).await {
                Ok(d) => {
                    if d.is_empty() {
                        break;
                    }
                    d
                }
                Err(e) => {
                    tracing::debug!("读取请求失败 (断连): {}", e);
                    break;
                }
            };

        let parsed = parse_request_data(&req_data);

        if let Err(e) = upstream.inner_mut().write_all(&req_data).await {
            warn!("转发请求到上游失败: {}", e);
            break;
        }
        if let Err(e) = upstream.inner_mut().flush().await {
            warn!("刷新上游失败: {}", e);
            break;
        }

        let head = match upstream.read_head().await {
            Ok(h) => h,
            Err(e) => {
                warn!("读取响应头失败: {}", e);
                break;
            }
        };
        if head.is_empty() {
            break;
        }
        let head_str = String::from_utf8_lossy(&head).to_lowercase();
        let is_event_stream = head_str.contains("content-type: text/event-stream");

        if is_event_stream && super::io::is_chunked(&head_str) {
            // ── 流式：边转发边聚合 ──
            let provider = parsed
                .as_ref()
                .map(|(r, _)| r.provider.clone())
                .unwrap_or(Provider::Unknown("stream".into()));
            let mut collector = SseCollector::new(provider);
            let relay = upstream
                .relay_chunked_stream(&head, &mut tls_client, |payload| collector.feed(payload))
                .await;
            if let Err(e) = relay {
                tracing::debug!("流式转发结束: {}", e);
                break;
            }
            if let Some((req, agent_type)) = &parsed {
                if options.agents.allows(agent_type) {
                    if let Some(resp) = collector.finish(&req.id) {
                        super::super::engine::writer::record_turn(
                            &store,
                            options.writer.as_ref(),
                            agent_type,
                            "proxy",
                            req.clone(),
                            resp,
                        );
                    }
                }
            }
            if connection_close(&head) {
                break;
            }
        } else {
            // ── 普通请求：按框架读取完整响应 ──
            let raw_body = match upstream.read_raw_body(&head).await {
                Ok(b) => b,
                Err(e) => {
                    warn!("读取响应失败: {}", e);
                    break;
                }
            };
            let mut full = head.clone();
            full.extend_from_slice(&raw_body);

            // 解析并记录 LLM 调用（记录失败不影响代理）
            if let Some((req, agent_type)) = &parsed {
                let body = super::io::decode_http_body(&full);
                let resp = if is_event_stream {
                    let mut collector = SseCollector::new(req.provider.clone());
                    collector.feed(&body);
                    collector.finish(&req.id)
                } else {
                    super::parser::ResponseParser::parse_response(&req.provider, &body, &req.id)
                };
                if let Some(resp) = resp {
                    if options.agents.allows(agent_type) {
                        super::super::engine::writer::record_turn(
                            &store,
                            options.writer.as_ref(),
                            agent_type,
                            "proxy",
                            req.clone(),
                            resp,
                        );
                    }
                }
            }

            if let Err(e) = tls_client.write_all(&full).await {
                tracing::debug!("转发响应给客户端失败: {}", e);
                break;
            }
            if let Err(e) = tls_client.flush().await {
                tracing::debug!("刷新客户端失败: {}", e);
                break;
            }

            if connection_close(&full) {
                break;
            }
        }
    }

    // 关连接（ignore errors — they are normal on teardown）
    let _ = tls_client.shutdown().await;
    let _ = upstream.inner_mut().shutdown().await;
    Ok(())
}

// ─── helpers ─────────────────────────────────────────────────

/// 判断响应是否带 Connection: close
fn connection_close(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(data);
    text.to_lowercase()
        .lines()
        .any(|l| l.trim() == "connection: close" || l.trim() == "proxy-connection: close")
}

async fn send_gateway_error(stream: &mut TcpStream, msg: String) -> Result<()> {
    let resp = format!(
        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\n\r\n{}",
        msg.len(),
        msg
    );
    stream.write_all(resp.as_bytes()).await?;
    Ok(())
}

fn upstream_client_config(tls: &UpstreamTls) -> Result<ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    match tls {
        UpstreamTls::Insecure => {
            let config = ClientConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerification(provider)))
                .with_no_client_auth();
            Ok(config)
        }
        other => {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if let UpstreamTls::ExtraCa(path) = other {
                let pem = std::fs::read(path)
                    .map_err(|e| anyhow::anyhow!("读取上游 CA 失败 {}: {}", path.display(), e))?;
                let mut reader = std::io::BufReader::new(pem.as_slice());
                for cert in rustls_pemfile::certs(&mut reader) {
                    roots
                        .add(cert?)
                        .map_err(|e| anyhow::anyhow!("上游 CA 证书无效: {}", e))?;
                }
            }
            let config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()?
                .with_root_certificates(roots)
                .with_no_client_auth();
            Ok(config)
        }
    }
}

async fn tls_connect_upstream(
    upstream_tcp: TcpStream,
    host: &str,
    tls: &UpstreamTls,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let config = upstream_client_config(tls)?;
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

/// `UpstreamTls::Insecure` 专用：接受任何上游证书（签名仍做结构校验）。
#[derive(Debug)]
struct NoVerification(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for NoVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
