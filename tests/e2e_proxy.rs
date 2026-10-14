//! End-to-end proxy tests: real TCP/TLS traffic through the lens proxy.
//!
//! Behavior contract: docs/REFACTOR-AUDIT.md — R1 (S1.1, S1.2), R2 (S2.1).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lens_lib::proxy::{ProxyOptions, UpstreamTls};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const TEST_TIMEOUT: Duration = Duration::from_secs(15);

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lens-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Read one HTTP message: head until CRLFCRLF, then body by Content-Length.
async fn read_http_message<S: AsyncReadExt + Unpin>(stream: &mut S) -> Vec<u8> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            let head_end = pos + 4;
            let body_len = content_length(&data[..head_end]).unwrap_or(0);
            if data.len() >= head_end + body_len {
                break;
            }
        }
    }
    data
}

fn content_length(head: &[u8]) -> Option<usize> {
    let s = String::from_utf8_lossy(head);
    for line in s.split("\r\n") {
        if line.to_ascii_lowercase().starts_with("content-length:") {
            return line[15..].trim().parse().ok();
        }
    }
    None
}

fn body_of(message: &[u8]) -> &[u8] {
    match message.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(p) => &message[p + 4..],
        None => &[],
    }
}

fn count_turns(db: &Path) -> i64 {
    if !db.exists() {
        return 0;
    }
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.query_row("SELECT COUNT(*) FROM turns", [], |r| r.get(0))
        .unwrap_or(0)
}

async fn wait_for_turns(db: &Path, expected: i64) -> i64 {
    for _ in 0..100 {
        let n = count_turns(db);
        if n >= expected {
            return n;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    count_turns(db)
}

const MOCK_REPLY: &str = r#"{"id":"cc-1","object":"chat.completion","model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"Non-stream reply"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;

/// S1.1: plain HTTP request must reach the upstream intact and be recorded.
#[tokio::test(flavor = "multi_thread")]
async fn http_request_is_forwarded_intact_and_recorded() {
    let dir = temp_dir("http");
    let (proxy_port, upstream_port) = (free_port(), free_port());

    // Mock upstream: records what it received.
    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
    tokio::spawn(async move {
        let listener = TcpListener::bind(("127.0.0.1", upstream_port))
            .await
            .unwrap();
        let (mut conn, _) = listener.accept().await.unwrap();
        let req = read_http_message(&mut conn).await;
        let _ = tx.send(req);
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            MOCK_REPLY.len(),
            MOCK_REPLY
        );
        conn.write_all(resp.as_bytes()).await.unwrap();
        let _ = conn.shutdown().await;
    });

    let data_dir = dir.join("data");
    let cert_dir = dir.join("certs");
    tokio::spawn({
        let data_dir = data_dir.clone();
        let cert_dir = cert_dir.clone();
        async move {
            lens_lib::proxy::run(
                &format!("127.0.0.1:{proxy_port}"),
                cert_dir.to_str().unwrap(),
                &data_dir,
            )
            .await
            .unwrap();
        }
    });
    tokio::time::sleep(Duration::from_millis(400)).await;

    let body = r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hello from e2e"}],"stream":false}"#;
    let request = format!(
        "POST http://127.0.0.1:{upstream_port}/v1/chat/completions HTTP/1.1\r\n\
         Host: 127.0.0.1:{upstream_port}\r\n\
         Content-Type: application/json\r\n\
         Authorization: Bearer sk-test\r\n\
         User-Agent: claude-code/2.1.0\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let client = async {
        let mut s = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let mut resp = Vec::new();
        s.read_to_end(&mut resp).await.unwrap();
        resp
    };
    let resp = tokio::time::timeout(TEST_TIMEOUT, client)
        .await
        .expect("client timed out talking to proxy");

    let resp_str = String::from_utf8_lossy(&resp);
    assert!(
        resp_str.starts_with("HTTP/1.1 200 OK"),
        "client must receive the upstream response, got: {:?}",
        &resp_str[..resp_str.len().min(120)]
    );
    assert!(
        resp_str.contains("Non-stream reply"),
        "client must receive the upstream body"
    );

    let upstream_saw = tokio::time::timeout(TEST_TIMEOUT, rx)
        .await
        .expect("upstream accepted no connection / no request from proxy")
        .unwrap();
    let upstream_str = String::from_utf8_lossy(&upstream_saw);
    assert!(
        upstream_str.contains("POST /v1/chat/completions"),
        "upstream must see the request line, got: {:?}",
        &upstream_str[..upstream_str.len().min(120)]
    );
    assert!(
        upstream_str
            .to_ascii_lowercase()
            .contains("user-agent: claude-code"),
        "upstream must see original headers"
    );
    assert!(
        String::from_utf8_lossy(body_of(&upstream_saw)).contains("hello from e2e"),
        "upstream must see the original body"
    );

    let turns = wait_for_turns(&data_dir.join("lens.db"), 1).await;
    assert_eq!(turns, 1, "exactly one turn must be recorded");
    let conn = rusqlite::Connection::open(data_dir.join("lens.db")).unwrap();
    let agent: String = conn
        .query_row("SELECT agent_type FROM turns LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(agent, "claude-code", "agent detected from User-Agent");

    let _ = std::fs::remove_dir_all(&dir);
}

const ANTHROPIC_REPLY: &str = r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"HTTPS reply"}],"stop_reason":"end_turn","usage":{"input_tokens":7,"output_tokens":3}}"#;

/// S2.1: chunked SSE must be aggregated into one recorded turn and relayed progressively.
#[tokio::test(flavor = "multi_thread")]
async fn sse_stream_is_aggregated_recorded_and_relayed_progressively() {
    let dir = temp_dir("sse");
    let (proxy_port, upstream_port) = (free_port(), free_port());

    let chunk = |payload: &str| format!("{:x}\r\n{}\r\n", payload.len(), payload).into_bytes();

    tokio::spawn(async move {
        let listener = TcpListener::bind(("127.0.0.1", upstream_port))
            .await
            .unwrap();
        let (mut conn, _) = listener.accept().await.unwrap();
        let _ = read_http_message(&mut conn).await;
        conn.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
        )
        .await
        .unwrap();
        for i in 0..3 {
            let data = format!(
                "data: {{\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-4o\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"tok{i}\"}},\"finish_reason\":null}}]}}\n\n"
            );
            conn.write_all(&chunk(&data)).await.unwrap();
            conn.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let final_chunk = "data: {\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n\n";
        conn.write_all(&chunk(final_chunk)).await.unwrap();
        conn.write_all(&chunk("data: [DONE]\n\n")).await.unwrap();
        conn.write_all(b"0\r\n\r\n").await.unwrap();
        conn.flush().await.unwrap();
        let _ = conn.shutdown().await;
    });

    let data_dir = dir.join("data");
    let cert_dir = dir.join("certs");
    tokio::spawn({
        let data_dir = data_dir.clone();
        let cert_dir = cert_dir.clone();
        async move {
            lens_lib::proxy::run(
                &format!("127.0.0.1:{proxy_port}"),
                cert_dir.to_str().unwrap(),
                &data_dir,
            )
            .await
            .unwrap();
        }
    });
    tokio::time::sleep(Duration::from_millis(400)).await;

    let body = r#"{"model":"gpt-4o","messages":[{"role":"user","content":"stream please"}],"stream":true}"#;
    let request = format!(
        "POST http://127.0.0.1:{upstream_port}/v1/chat/completions HTTP/1.1\r\n\
         Host: 127.0.0.1:{upstream_port}\r\n\
         Content-Type: application/json\r\n\
         User-Agent: claude-code/2.1.0\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let client = async {
        let mut s = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let started = std::time::Instant::now();
        let mut first_byte_at: Option<Duration> = None;
        let mut resp = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = s.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            if first_byte_at.is_none() {
                first_byte_at = Some(started.elapsed());
            }
            resp.extend_from_slice(&buf[..n]);
        }
        (resp, first_byte_at.unwrap_or_default(), started.elapsed())
    };
    let (resp, first_byte_at, total) = tokio::time::timeout(TEST_TIMEOUT, client)
        .await
        .expect("client timed out on SSE");

    let resp_str = String::from_utf8_lossy(&resp);
    assert!(resp_str.starts_with("HTTP/1.1 200 OK"));
    for i in 0..3 {
        assert!(
            resp_str.contains(&format!("tok{i}")),
            "stream must carry tok{i}"
        );
    }
    assert!(resp_str.contains("[DONE]"), "client must see stream end");
    assert!(
        first_byte_at < Duration::from_millis(400),
        "SSE must be relayed progressively (first byte at {first_byte_at:?})"
    );
    assert!(
        total > Duration::from_millis(500),
        "mock stream takes ~600ms total (got {total:?})"
    );

    let turns = wait_for_turns(&data_dir.join("lens.db"), 1).await;
    assert_eq!(turns, 1, "streamed turn must be recorded");
    let conn = rusqlite::Connection::open(data_dir.join("lens.db")).unwrap();
    let (text, total_tokens): (Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT json_extract(response_json, '$.content[0].text'),
                    json_extract(response_json, '$.usage.total_tokens')
             FROM turns LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        text.as_deref(),
        Some("tok0tok1tok2"),
        "recorded content must be the concatenated deltas"
    );
    assert_eq!(total_tokens, Some(15), "usage from stream must be recorded");

    let _ = std::fs::remove_dir_all(&dir);
}

fn ring_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// S1.2: HTTPS through a CONNECT tunnel must be forwarded and recorded.
#[tokio::test(flavor = "multi_thread")]
async fn https_connect_tunnel_is_forwarded_and_recorded() {
    let dir = temp_dir("https");
    let (proxy_port, upstream_port) = (free_port(), free_port());

    // Upstream CA + leaf for localhost.
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let mut ca_params = rcgen::CertificateParams::new(vec!["lens-e2e-ca".to_string()]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();

    let leaf_key = rcgen::KeyPair::generate().unwrap();
    let leaf_params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let leaf_cert = leaf_params.signed_by(&leaf_key, &ca_cert, &ca_key).unwrap();

    let ca_pem_path = dir.join("upstream-ca.pem");
    std::fs::write(&ca_pem_path, ca_cert.pem()).unwrap();

    // TLS mock upstream.
    let server_config = rustls::ServerConfig::builder_with_provider(ring_provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf_cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::try_from(leaf_key.serialize_der()).unwrap(),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));

    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
    tokio::spawn(async move {
        let listener = TcpListener::bind(("127.0.0.1", upstream_port))
            .await
            .unwrap();
        let (conn, _) = listener.accept().await.unwrap();
        let mut conn = acceptor.accept(conn).await.unwrap();
        let req = read_http_message(&mut conn).await;
        let _ = tx.send(req);
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            ANTHROPIC_REPLY.len(),
            ANTHROPIC_REPLY
        );
        conn.write_all(resp.as_bytes()).await.unwrap();
        let _ = conn.shutdown().await;
    });

    let data_dir = dir.join("data");
    let cert_dir = dir.join("certs");
    tokio::spawn({
        let data_dir = data_dir.clone();
        let cert_dir = cert_dir.clone();
        let options = ProxyOptions {
            upstream_tls: UpstreamTls::ExtraCa(ca_pem_path.clone()),
            ..Default::default()
        };
        async move {
            lens_lib::proxy::run_with(
                &format!("127.0.0.1:{proxy_port}"),
                cert_dir.to_str().unwrap(),
                &data_dir,
                options,
            )
            .await
            .unwrap();
        }
    });

    // Wait for the proxy to generate its MITM CA.
    let ca_crt = cert_dir.join("ca.crt");
    for _ in 0..100 {
        if ca_crt.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ca_crt.exists(), "proxy must generate its MITM CA");

    let body = r#"{"model":"claude-sonnet-4-20250514","max_tokens":64,"system":[{"type":"text","text":"You are Claude Code."}],"messages":[{"role":"user","content":"e2e https"}],"stream":false}"#;
    let request = format!(
        "POST /v1/messages HTTP/1.1\r\n\
         Host: localhost:{upstream_port}\r\n\
         Content-Type: application/json\r\n\
         X-Api-Key: sk-ant-test\r\n\
         User-Agent: claude-code/2.1.0\r\n\
         Content-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );

    let client = async {
        let mut tcp = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        let connect = format!(
            "CONNECT localhost:{upstream_port} HTTP/1.1\r\nHost: localhost:{upstream_port}\r\n\r\n"
        );
        tcp.write_all(connect.as_bytes()).await.unwrap();

        let mut status = Vec::new();
        loop {
            let mut b = [0u8; 1];
            tcp.read_exact(&mut b).await.unwrap();
            status.push(b[0]);
            if status.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        assert!(
            status.starts_with(b"HTTP/1.1 200"),
            "CONNECT must be accepted, got {:?}",
            String::from_utf8_lossy(&status)
        );

        let mut roots = rustls::RootCertStore::empty();
        let pem = std::fs::read(&ca_crt).unwrap();
        let mut reader = std::io::BufReader::new(pem.as_slice());
        for cert in rustls_pemfile::certs(&mut reader) {
            roots.add(cert.unwrap()).unwrap();
        }
        let config = rustls::ClientConfig::builder_with_provider(ring_provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let mut tls = connector
            .connect("localhost".try_into().unwrap(), tcp)
            .await
            .expect("MITM TLS handshake with proxy");

        tls.write_all(request.as_bytes()).await.unwrap();
        tls.flush().await.unwrap();
        let resp = read_http_message(&mut tls).await;
        String::from_utf8_lossy(&resp).to_string()
    };

    let resp = tokio::time::timeout(TEST_TIMEOUT, client)
        .await
        .expect("client timed out on CONNECT tunnel");
    assert!(
        resp.starts_with("HTTP/1.1 200 OK"),
        "client must receive upstream response over tunnel, got: {:?}",
        &resp[..resp.len().min(120)]
    );
    assert!(resp.contains("HTTPS reply"), "client must receive body");

    let upstream_saw = tokio::time::timeout(TEST_TIMEOUT, rx)
        .await
        .expect("upstream accepted no connection")
        .unwrap();
    let upstream_str = String::from_utf8_lossy(&upstream_saw);
    assert!(
        upstream_str.contains("POST /v1/messages"),
        "upstream must see request line, got: {:?}",
        &upstream_str[..upstream_str.len().min(120)]
    );
    assert!(
        upstream_str
            .to_ascii_lowercase()
            .contains("user-agent: claude-code"),
        "upstream must see original headers"
    );
    assert!(
        String::from_utf8_lossy(body_of(&upstream_saw)).contains("e2e https"),
        "upstream must see the original body"
    );

    let turns = wait_for_turns(&data_dir.join("lens.db"), 1).await;
    assert_eq!(turns, 1, "HTTPS turn must be recorded");
    let conn = rusqlite::Connection::open(data_dir.join("lens.db")).unwrap();
    let agent: String = conn
        .query_row("SELECT agent_type FROM turns LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(agent, "claude-code");

    let _ = std::fs::remove_dir_all(&dir);
}
