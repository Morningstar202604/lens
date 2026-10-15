//! Scenario test: simulates the full Lens pipeline
//! (parse → agent detect → extract_context → record → verify_db).

use std::collections::HashMap;

#[test]
fn test_opencode_request_parses_and_stores() {
    use lens_lib::proxy::parser::{Provider, RequestParser};
    use lens_lib::proxy::request::parse_request_data;

    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [
            {"role": "system", "content": "You are a coding agent."},
            {"role": "user", "content": "Fix the auth bug in src/auth/guard.ts"}
        ],
        "temperature": 0.2,
        "workspace": "/home/user/jwt-refactor",
        "session_id": "sess-abc-123",
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();

    let mut headers = HashMap::new();
    headers.insert("content-type".into(), "application/json".into());
    headers.insert("authorization".into(), "Bearer sk-test-key".into());
    headers.insert("user-agent".into(), "opencode/1.0.0".into());
    headers.insert("host".into(), "localhost:8080".into());

    let req = RequestParser::parse_request(
        "POST",
        "https://api.openai.com/v1/chat/completions",
        &headers,
        &body_bytes,
    )
    .expect("should parse as LLM request");

    assert!(matches!(req.provider, Provider::OpenAI));
    assert_eq!(req.model.as_deref(), Some("claude-sonnet-4-20250514"));
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[1].role, "user");

    let raw = format!(
        "POST /v1/chat/completions HTTP/1.1\r\n\
         Host: api.openai.com\r\n\
         Content-Type: application/json\r\n\
         Authorization: Bearer sk-test\r\n\
         User-Agent: opencode/1.0.0\r\n\
         Content-Length: {}\r\n\r\n{}",
        body_bytes.len(),
        String::from_utf8_lossy(&body_bytes),
    );
    let parsed = parse_request_data(raw.as_bytes()).expect("parse_request_data should succeed");
    let (req2, agent_type) = parsed;
    assert_eq!(agent_type, "opencode");
    assert_eq!(req2.model.as_deref(), Some("claude-sonnet-4-20250514"));
    assert!(
        req2.metadata.contains_key("opencode.workspace"),
        "adapter should extract workspace into metadata"
    );
}

#[test]
fn test_codex_request_extracts_previous_response_id() {
    use lens_lib::proxy::request::parse_request_data;

    // Codex Responses API uses `input`, which the generic parser skips because
    // it requires `messages`. Instead we test the Chat-Completions-shaped
    // Codex request which carries `previous_response_id` + `model.reasoning.effort`.
    let body = serde_json::json!({
        "model": "o1",
        "messages": [
            {"role": "user", "content": "Refactor the middleware layer"}
        ],
        "previous_response_id": "resp_01ABC123",
        "store": true,
        "reasoning": {"effort": "high"}
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();

    let raw = format!(
        "POST /v1/chat/completions HTTP/1.1\r\n\
         Host: api.openai.com\r\n\
         Content-Type: application/json\r\n\
         Authorization: Bearer sk-codex-key\r\n\
         User-Agent: codex-cli/0.82.0\r\n\
         Content-Length: {}\r\n\r\n{}",
        body_bytes.len(),
        String::from_utf8_lossy(&body_bytes),
    );
    let parsed = parse_request_data(raw.as_bytes()).expect("should parse");
    let (req, agent_type) = parsed;
    assert_eq!(agent_type, "codex");
    assert_eq!(
        req.metadata
            .get("codex.previous_response_id")
            .map(|s| s.as_str()),
        Some("resp_01ABC123"),
        "Codex adapter must extract previous_response_id",
    );
    assert_eq!(
        req.metadata
            .get("codex.reasoning_effort")
            .map(|s| s.as_str()),
        Some("high"),
        "Codex adapter must extract reasoning_effort",
    );
}

#[test]
fn test_claude_code_extracts_system_blocks() {
    use lens_lib::proxy::request::parse_request_data;

    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 4096,
        "system": [
            {"type": "text", "text": "You are Claude Code."},
            {"type": "text", "text": "Be concise."}
        ],
        "messages": [{"role": "user", "content": "Debug the token refresh flow"}],
        "betas": ["interleaved-thinking-2025-05-14"],
        "container_id": "container-xyz"
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();

    let raw = format!(
        "POST /v1/messages HTTP/1.1\r\n\
         Host: api.anthropic.com\r\n\
         Content-Type: application/json\r\n\
         X-Api-Key: sk-ant-claude\r\n\
         User-Agent: claude-code/2.1.0\r\n\
         X-Anthropic-Beta: interleaved-thinking-2025-05-14\r\n\
         X-Anthropic-Container-Id: container-xyz\r\n\
         Content-Length: {}\r\n\r\n{}",
        body_bytes.len(),
        String::from_utf8_lossy(&body_bytes),
    );
    let parsed = parse_request_data(raw.as_bytes()).expect("should parse");
    let (req, agent_type) = parsed;
    assert_eq!(agent_type, "claude-code");
    assert_eq!(
        req.metadata.get("claude.system_blocks").map(|s| s.as_str()),
        Some("2"),
        "Claude Code adapter must extract system block count"
    );
    assert_eq!(
        req.metadata.get("claude.container_id").map(|s| s.as_str()),
        Some("container-xyz"),
        "Claude adapter must extract container_id from X-Anthropic-Container-Id header"
    );
    assert!(
        req.metadata.contains_key("claude.betas"),
        "Claude adapter must extract betas from X-Anthropic-Beta header"
    );
}

#[test]
fn test_store_record_and_retrieve() {
    use lens_lib::engine::Store;
    use lens_lib::proxy::parser::{
        ContentBlock, LlmRequest, LlmResponse, Message, MessageContent, Provider, TokenUsage,
    };

    let tmp = std::env::temp_dir().join(format!("lens-test-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&tmp).expect("open store");

    let req = LlmRequest {
        id: "turn-001".into(),
        timestamp: "2026-10-08T12:00:00Z".into(),
        provider: Provider::OpenAI,
        endpoint: "/v1/chat/completions".into(),
        messages: vec![
            Message {
                role: "system".into(),
                content: MessageContent::Text("You are an AI.".into()),
                name: None,
            },
            Message {
                role: "user".into(),
                content: MessageContent::Text("Hello".into()),
                name: None,
            },
        ],
        model: Some("gpt-4o".into()),
        temperature: Some(0.3),
        tools: None,
        metadata: {
            let mut m = HashMap::new();
            m.insert("cwd".into(), "/home/user/myproject".into());
            m.insert("user-agent".into(), "codex/1.0.0".into());
            m
        },
    };

    let resp = LlmResponse {
        request_id: "turn-001".into(),
        timestamp: "2026-10-08T12:00:01Z".into(),
        provider: Provider::OpenAI,
        content: vec![ContentBlock {
            block_type: "text".into(),
            text: Some("Hi there!".into()),
            image_url: None,
        }],
        tool_calls: None,
        usage: Some(TokenUsage {
            prompt_tokens: Some(42),
            completion_tokens: Some(10),
            total_tokens: Some(52),
        }),
        finish_reason: Some("stop".into()),
        raw: serde_json::json!({}),
    };

    store
        .record_turn("codex", req.clone(), resp)
        .expect("record_turn should succeed");

    let sessions = store
        .list_sessions(100, 0)
        .expect("list_sessions should succeed");
    assert!(!sessions.is_empty(), "should have at least one session");
    assert_eq!(sessions[0].message_count, 1, "session should have 1 turn");
    assert!(
        sessions[0].total_tokens >= 52,
        "total_tokens should be >= 52"
    );

    let session_id = &sessions[0].id;
    let detail = store
        .get_session(session_id)
        .expect("get_session should succeed");
    let detail = detail.expect("session should exist");
    assert_eq!(detail.turns.len(), 1);
    assert_eq!(detail.turns[0].agent_type, "codex");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_session_id_stable_within_window_and_isolated_by_key() {
    use lens_lib::engine::Store;
    use lens_lib::proxy::parser::{LlmRequest, Provider};

    let mk_req = |key: &str, value: &str| {
        let mut m = HashMap::new();
        m.insert(key.to_string(), value.to_string());
        LlmRequest {
            id: "x".into(),
            timestamp: "t".into(),
            provider: Provider::OpenAI,
            endpoint: "/v1/chat/completions".into(),
            messages: vec![],
            model: None,
            temperature: None,
            tools: None,
            metadata: m,
        }
    };

    // 分桶边界内稳定:同一锚点在同一窗口内产生相同会话 id
    let base = Store::SESSION_WINDOW_SECS * 1000 + 10;
    let id1 = Store::session_id_at("codex", &mk_req("cwd", "/home/user/project"), base);
    let id2 = Store::session_id_at("codex", &mk_req("cwd", "/home/user/project"), base + 60);
    assert_eq!(
        id1, id2,
        "same cwd within one window must share the session id"
    );

    // 跨窗口滚动:闲置超过一个窗口后开启新会话
    let id3 = Store::session_id_at(
        "codex",
        &mk_req("cwd", "/home/user/project"),
        base + Store::SESSION_WINDOW_SECS * 2,
    );
    assert_ne!(
        id1, id3,
        "same cwd in a later window must start a new session"
    );

    // 不同锚点隔离(含 /a/b-c vs /a/b/c 不碰撞)
    let id4 = Store::session_id_at("codex", &mk_req("cwd", "/a/b-c"), base);
    let id5 = Store::session_id_at("codex", &mk_req("cwd", "/a/b/c"), base);
    assert_ne!(id4, id5, "/a/b-c and /a/b/c must not collide");

    // 不同 agent 相同 cwd 不混
    let id6 = Store::session_id_at("opencode", &mk_req("cwd", "/home/user/project"), base);
    assert_ne!(id1, id6, "different agents must not share a session");

    // 原生会话 id 不随窗口滚动
    let n1 = Store::session_id_at("opencode", &mk_req("opencode.session_id", "ses-1"), base);
    let n2 = Store::session_id_at(
        "opencode",
        &mk_req("opencode.session_id", "ses-1"),
        base + Store::SESSION_WINDOW_SECS * 5,
    );
    assert_eq!(n1, n2, "native session ids must be window-independent");
}

#[test]
fn test_session_grouping_groups_same_cwd_into_one_session() {
    use lens_lib::engine::Store;
    use lens_lib::proxy::parser::{
        ContentBlock, LlmRequest, LlmResponse, Message, MessageContent, Provider, TokenUsage,
    };

    let tmp = std::env::temp_dir().join(format!("lens-grouping-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&tmp).expect("open store");

    let mk_turn = |n: usize| {
        let mut meta = HashMap::new();
        meta.insert("cwd".into(), "/home/dev/project".into());
        meta.insert("user-agent".into(), "codex/1.0.0".into());
        let req = LlmRequest {
            id: format!("turn-{n}"),
            timestamp: format!("2026-10-14T00:0{n}:00Z"),
            provider: Provider::OpenAI,
            endpoint: "/v1/responses".into(),
            messages: vec![Message {
                role: "user".into(),
                content: MessageContent::Text(format!("question {n}")),
                name: None,
            }],
            model: Some("gpt-5".into()),
            temperature: None,
            tools: None,
            metadata: meta,
        };
        let resp = LlmResponse {
            request_id: format!("turn-{n}"),
            timestamp: format!("2026-10-14T00:0{n}:01Z"),
            provider: Provider::OpenAI,
            content: vec![ContentBlock {
                block_type: "text".into(),
                text: Some("answer".into()),
                image_url: None,
            }],
            tool_calls: None,
            usage: Some(TokenUsage {
                prompt_tokens: Some(5),
                completion_tokens: Some(3),
                total_tokens: Some(8),
            }),
            finish_reason: Some("stop".into()),
            raw: serde_json::json!({}),
        };
        (req, resp)
    };

    for n in 0..5 {
        let (req, resp) = mk_turn(n);
        store.record_turn("codex", req, resp).expect("record");
    }

    let sessions = store.list_sessions(100, 0).expect("list");
    assert_eq!(
        sessions.len(),
        1,
        "five turns in one window/cwd = one session"
    );
    assert_eq!(sessions[0].message_count, 5);
    assert_eq!(sessions[0].agent_type, "codex");
    assert_eq!(sessions[0].model.as_deref(), Some("gpt-5"));
    assert_eq!(sessions[0].total_tokens, 40);
    assert_eq!(
        sessions[0].title, "question 0",
        "title = first user message"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_migration_upgrades_legacy_db_without_data_loss() {
    use lens_lib::engine::Store;

    let tmp = std::env::temp_dir().join(format!("lens-migrate-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let db = tmp.join("lens.db");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE turns (
                id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                agent_type TEXT NOT NULL DEFAULT 'unknown', timestamp TEXT NOT NULL,
                request_json TEXT NOT NULL, response_json TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, project_path TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE nodes (id TEXT PRIMARY KEY, label TEXT NOT NULL, kind TEXT NOT NULL,
                weight REAL DEFAULT 1.0, first_seen DATETIME DEFAULT CURRENT_TIMESTAMP);
            CREATE TABLE edges (id INTEGER PRIMARY KEY AUTOINCREMENT, source TEXT NOT NULL,
                target TEXT NOT NULL, relation TEXT NOT NULL, weight REAL DEFAULT 1.0,
                UNIQUE(source, target, relation));
            INSERT INTO sessions (id, title) VALUES ('legacy-1', 'legacy session');
            INSERT INTO turns (id, session_id, timestamp, request_json, response_json) VALUES
                ('t1', 'legacy-1', '2026-01-01T00:00:00Z',
                 '{"id":"t1","timestamp":"2026-01-01T00:00:00Z","provider":"OpenAI","endpoint":"/v1/chat/completions","messages":[],"model":"gpt-4o","temperature":null,"tools":null,"metadata":{}}',
                 '{"request_id":"t1","timestamp":"2026-01-01T00:00:01Z","provider":"OpenAI","content":[],"tool_calls":null,"usage":null,"finish_reason":"stop","raw":{}}');
            "#,
        )
        .unwrap();
    }

    let store = Store::open(&tmp).expect("open legacy db must migrate");
    let sessions = store.list_sessions(10, 0).unwrap();
    assert_eq!(sessions.len(), 1, "legacy data must survive migration");
    assert_eq!(sessions[0].message_count, 1);

    {
        let conn = store.conn.lock().unwrap();
        let source: String = conn
            .query_row(
                "SELECT source FROM sessions WHERE id = 'legacy-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "proxy", "new column gets default");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1, "schema version bumped");
    }
    assert!(
        tmp.join("lens.db.bak-v0").exists(),
        "migration must leave a backup"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_graph_build_is_session_scoped_and_idempotent() {
    use lens_lib::engine::Store;
    use lens_lib::proxy::parser::{
        ContentBlock, LlmRequest, LlmResponse, Message, MessageContent, Provider,
    };

    let tmp = std::env::temp_dir().join(format!("lens-graph-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&tmp).expect("open store");

    let mk_turn = |id: &str, cwd: &str, text: &str| {
        let mut meta = HashMap::new();
        meta.insert("cwd".into(), cwd.to_string());
        let req = LlmRequest {
            id: id.into(),
            timestamp: format!("2026-10-14T01:00:0{}Z", &id[id.len() - 1..]),
            provider: Provider::OpenAI,
            endpoint: "/v1/chat/completions".into(),
            messages: vec![Message {
                role: "user".into(),
                content: MessageContent::Text(text.into()),
                name: None,
            }],
            model: Some("gpt-4o".into()),
            temperature: None,
            tools: None,
            metadata: meta,
        };
        let resp = LlmResponse {
            request_id: id.into(),
            timestamp: format!("2026-10-14T01:00:0{}Z", &id[id.len() - 1..]),
            provider: Provider::OpenAI,
            content: vec![ContentBlock {
                block_type: "text".into(),
                text: Some("ok".into()),
                image_url: None,
            }],
            tool_calls: None,
            usage: None,
            finish_reason: Some("stop".into()),
            raw: serde_json::json!({}),
        };
        (req, resp)
    };

    // 两个会话：A 提到 a.ts + b.ts，B 提到 c.ts
    let (ra, pa) = mk_turn("turn-a", "/proj/a", "edit src/a.ts and src/b.ts refactor");
    store.record_turn("codex", ra, pa).unwrap();
    let (rb, pb) = mk_turn("turn-b", "/proj/b", "fix src/c.ts bug");
    store.record_turn("codex", rb, pb).unwrap();

    let sessions = store.list_sessions(10, 0).unwrap();
    assert_eq!(sessions.len(), 2, "two cwds → two sessions");
    let sid_a = sessions
        .iter()
        .find(|s| s.title.contains("a.ts"))
        .expect("session A")
        .id
        .clone();
    let sid_b = sessions
        .iter()
        .find(|s| s.title.contains("c.ts"))
        .expect("session B")
        .id
        .clone();

    let (nodes_a, edges_a) =
        lens_lib::engine::graph::build_graph(&tmp, Some(&sid_a)).expect("build A");
    assert!(
        nodes_a.iter().any(|n| n.label.contains("a.ts")),
        "A graph contains its own entity"
    );
    assert!(
        !nodes_a.iter().any(|n| n.label.contains("c.ts")),
        "A graph must not contain B's entity"
    );
    assert!(!edges_a.is_empty(), "co-occurrence edges must exist");

    // 构建 B：不得清除 A 的实体
    let (nodes_b, _) = lens_lib::engine::graph::build_graph(&tmp, Some(&sid_b)).expect("build B");
    assert!(nodes_b.iter().any(|n| n.label.contains("c.ts")));
    let all = store.list_all_nodes().unwrap();
    assert!(
        all.iter().any(|n| n.label.contains("a.ts")),
        "building B must not delete A's entities"
    );

    // 重建 A：幂等（同样数量与权重）
    let (nodes_a2, edges_a2) =
        lens_lib::engine::graph::build_graph(&tmp, Some(&sid_a)).expect("rebuild A");
    assert_eq!(nodes_a.len(), nodes_a2.len(), "rebuild is idempotent");
    assert_eq!(edges_a.len(), edges_a2.len());
    let w1: f64 = nodes_a.iter().map(|n| n.weight).sum();
    let w2: f64 = nodes_a2.iter().map(|n| n.weight).sum();
    assert!(
        (w1 - w2).abs() < f64::EPSILON,
        "weights unchanged on rebuild"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_config_parsing_and_cli_precedence() {
    use lens_lib::config::Config;
    use lens_lib::proxy::UpstreamTls;

    let tmp = std::env::temp_dir().join(format!("lens-config-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg_path = tmp.join("config.toml");
    std::fs::write(
        &cfg_path,
        r#"
[proxy]
bind = "127.0.0.1:19999"
cert_dir = "custom-certs"
upstream_ca = "corp-ca.pem"

[view]
bind = "127.0.0.1:17317"
open_browser = false

[agents.codex]
enabled = false
"#,
    )
    .unwrap();

    let config = Config::load(Some(cfg_path.to_str().unwrap())).expect("load config");
    assert_eq!(config.effective_proxy_bind(None), "127.0.0.1:19999");
    assert_eq!(
        config.effective_proxy_bind(Some("127.0.0.1:1234")),
        "127.0.0.1:1234",
        "CLI overrides file"
    );
    assert_eq!(config.effective_cert_dir(None), "custom-certs");
    assert_eq!(config.effective_view_bind(None), "127.0.0.1:17317");
    assert!(!config.view.open_browser);
    assert!(matches!(
        config.effective_upstream_tls(None, false),
        UpstreamTls::ExtraCa(_)
    ));
    assert!(matches!(
        config.effective_upstream_tls(None, true),
        UpstreamTls::Insecure
    ));
    let filter = config.agent_filter();
    assert!(!filter.allows("codex"), "disabled agent must be filtered");
    assert!(filter.allows("opencode"), "unset agents default to enabled");
    assert!(filter.allows("something-else"));

    let missing = Config::load(Some(tmp.join("nope.toml").to_str().unwrap()));
    assert!(
        missing.is_err(),
        "explicit missing config path must fail-closed"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_cli_binary_honors_config_file_bind() {
    use std::process::Command;

    let tmp = std::env::temp_dir().join(format!("lens-cli-cfg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let cfg_path = tmp.join("config.toml");
    let cert_dir = tmp.join("certs");
    let data_dir = tmp.join("data");
    std::fs::write(
        &cfg_path,
        format!(
            "[proxy]
bind = \"127.0.0.1:{port}\"
cert_dir = '{}'
[storage]
data_dir = '{}'
",
            cert_dir.display(),
            data_dir.display()
        ),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_lens"))
        .arg("--config")
        .arg(&cfg_path)
        .arg("proxy")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn lens binary");

    let mut bound = false;
    let mut db_ready = false;
    for _ in 0..80 {
        if !bound && std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            bound = true;
        }
        if data_dir.join("lens.db").exists() {
            db_ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        bound,
        "lens proxy must bind the address from the config file"
    );
    assert!(
        db_ready,
        "data_dir from config must be used (lens.db created)"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_import_claude_jsonl_is_idempotent_and_grouped() {
    use lens_lib::import::import_claude;

    let tmp = std::env::temp_dir().join(format!("lens-import-{}", uuid::Uuid::new_v4()));
    let data = tmp.join("data");
    let logs = tmp.join("logs");
    std::fs::create_dir_all(&logs).unwrap();

    let fixture = concat!(
        r#"{"type":"user","sessionId":"sess-1","cwd":"/proj/x","message":{"role":"user","content":"help me with src/a.ts"},"uuid":"u1","timestamp":"2026-10-14T02:00:00Z"}"#,
        "
",
        r#"{"type":"assistant","sessionId":"sess-1","cwd":"/proj/x","message":{"role":"assistant","model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"Sure"},{"type":"tool_use","id":"tu1","name":"Read","input":{"file_path":"src/a.ts"}}],"usage":{"input_tokens":10,"cache_read_input_tokens":2,"output_tokens":4}},"uuid":"a1","timestamp":"2026-10-14T02:00:01Z"}"#,
        "
",
        r#"{"type":"user","sessionId":"sess-1","cwd":"/proj/x","message":{"role":"user","content":"and src/b.ts"},"uuid":"u2","timestamp":"2026-10-14T02:01:00Z"}"#,
        "
",
        r#"{"type":"assistant","sessionId":"sess-1","cwd":"/proj/x","message":{"role":"assistant","model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"Done"}],"usage":{"input_tokens":5,"output_tokens":3}},"uuid":"a2","timestamp":"2026-10-14T02:01:01Z"}"#,
        "
",
    );
    std::fs::write(logs.join("session1.jsonl"), fixture).unwrap();

    let stats = import_claude(&data, &logs).expect("import");
    assert_eq!(stats.files, 1);
    assert_eq!(stats.turns_imported, 2);

    let store = lens_lib::engine::Store::open(&data).unwrap();
    let sessions = store.list_sessions(10, 0).unwrap();
    assert_eq!(
        sessions.len(),
        1,
        "same sessionId must group into one session"
    );
    assert_eq!(sessions[0].message_count, 2);
    assert_eq!(sessions[0].agent_type, "claude-code");
    assert_eq!(sessions[0].total_tokens, 24, "(10+2+4) + (5+3)");
    assert!(sessions[0].title.contains("src/a.ts"));
    let detail = store.get_session(&sessions[0].id).unwrap().unwrap();
    assert_eq!(detail.turns.len(), 2);
    assert_eq!(detail.turns[0].request.messages.len(), 1);
    assert_eq!(
        detail.turns[0]
            .response
            .tool_calls
            .as_ref()
            .map(|t| t.len()),
        Some(1),
        "tool_use block must be captured"
    );

    let again = import_claude(&data, &logs).expect("re-import");
    assert_eq!(again.turns_imported, 0, "re-import must be idempotent");
    assert_eq!(again.turns_skipped, 2);
    let sessions = store.list_sessions(10, 0).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].message_count, 2, "re-import adds no turns");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_single_writer_queue_persists_all_turns() {
    use lens_lib::engine::{writer, Store};
    use lens_lib::proxy::parser::{
        ContentBlock, LlmRequest, LlmResponse, Message, MessageContent, Provider,
    };
    use std::sync::Arc;

    let tmp = std::env::temp_dir().join(format!("lens-writer-{}", uuid::Uuid::new_v4()));
    let store = Arc::new(Store::open(&tmp).expect("open store"));
    let handle = writer::WriterHandle::spawn(store.clone());

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        for n in 0..50usize {
            let mut meta = HashMap::new();
            meta.insert("cwd".into(), "/proj/writer".to_string());
            let req = LlmRequest {
                id: format!("w-{n}"),
                timestamp: format!("2026-10-14T03:00:{:02}Z", n % 60),
                provider: Provider::OpenAI,
                endpoint: "/v1/chat/completions".into(),
                messages: vec![Message {
                    role: "user".into(),
                    content: MessageContent::Text(format!("q{n}")),
                    name: None,
                }],
                model: Some("gpt-4o".into()),
                temperature: None,
                tools: None,
                metadata: meta,
            };
            let resp = LlmResponse {
                request_id: format!("w-{n}"),
                timestamp: format!("2026-10-14T03:00:{:02}Z", n % 60),
                provider: Provider::OpenAI,
                content: vec![ContentBlock {
                    block_type: "text".into(),
                    text: Some("ok".into()),
                    image_url: None,
                }],
                tool_calls: None,
                usage: None,
                finish_reason: Some("stop".into()),
                raw: serde_json::json!({}),
            };
            writer::record_turn(&store, Some(&handle), "codex", "proxy", req, resp);
        }
        handle.barrier().await;
    });

    let sessions = store.list_sessions(10, 0).unwrap();
    assert_eq!(sessions.len(), 1, "queued turns land in one session");
    assert_eq!(sessions[0].message_count, 50, "all queued turns persisted");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_cert_cache_is_bounded() {
    use lens_lib::proxy::cert::{CertManager, CERT_CACHE_LIMIT};

    let tmp = std::env::temp_dir().join(format!("lens-certcache-{}", uuid::Uuid::new_v4()));
    let manager = CertManager::init(&tmp).expect("init cert manager");
    for i in 0..(CERT_CACHE_LIMIT + 8) {
        manager
            .build_server_config(&format!("h{i}.example"))
            .expect("build server config");
    }
    assert!(
        manager.cached_cert_count() <= CERT_CACHE_LIMIT,
        "cache must stay bounded (got {})",
        manager.cached_cert_count()
    );
    manager
        .build_server_config("h0.example")
        .expect("existing host still served after eviction");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_import_opencode_sqlite_is_idempotent() {
    use lens_lib::import::import_opencode;

    let tmp = std::env::temp_dir().join(format!("lens-ocimport-{}", uuid::Uuid::new_v4()));
    let data = tmp.join("data");
    std::fs::create_dir_all(&tmp).unwrap();
    let oc = tmp.join("opencode.db");

    // 最小结构:字段名取自真实 OpenCode 库(2026-10 本机实测 schema)
    {
        let conn = rusqlite::Connection::open(&oc).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE session (id TEXT PRIMARY KEY, parent_id TEXT, directory TEXT,
                title TEXT, time_created INTEGER, time_updated INTEGER);
            CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT,
                time_created INTEGER, data TEXT);
            CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, data TEXT);
            INSERT INTO session VALUES ('ses_1', NULL, 'C:/proj/x', 'demo', 1788627071000, 1788627084000);
            INSERT INTO message VALUES ('msg_u1','ses_1',1788627071000,'{"role":"user","time":{"created":1788627071000}}');
            INSERT INTO part VALUES ('prt_1','msg_u1','ses_1','{"type":"text","text":"帮我改 src/a.ts"}');
            INSERT INTO message VALUES ('msg_a1','ses_1',1788627081000,'{"role":"assistant","modelID":"big-pickle","time":{"created":1788627081000},"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":10,"write":2}},"finish":"stop"}');
            INSERT INTO part VALUES ('prt_2','msg_a1','ses_1','{"type":"text","text":"好"}');
            INSERT INTO part VALUES ('prt_3','msg_a1','ses_1','{"type":"tool","tool":"bash","callID":"call_1","state":{"status":"completed","input":{"cmd":"ls"},"output":"ok"}}');
            "#,
        )
        .unwrap();
    }

    let stats = import_opencode(&data, &oc, 0).expect("import opencode");
    assert_eq!(stats.turns_imported, 1);

    let store = lens_lib::engine::Store::open(&data).unwrap();
    let sessions = store.list_sessions(10, 0).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].agent_type, "opencode");
    assert_eq!(sessions[0].total_tokens, 137, "(100+10+2) + (20+5)");
    assert!(sessions[0].title.contains("src/a.ts"));
    let detail = store.get_session(&sessions[0].id).unwrap().unwrap();
    assert_eq!(detail.turns.len(), 1);
    assert_eq!(
        detail.turns[0]
            .response
            .tool_calls
            .as_ref()
            .map(|t| t.len()),
        Some(1)
    );
    assert_eq!(detail.turns[0].request.messages.len(), 1);

    let again = import_opencode(&data, &oc, 0).expect("re-import");
    assert_eq!(again.turns_imported, 0, "re-import must be idempotent");
    assert_eq!(again.turns_skipped, 1);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_max_body_size_constant() {
    assert_eq!(lens_lib::proxy::io::MAX_BODY_SIZE, 10 * 1024 * 1024);
}

#[test]
fn test_responses_api_input_request_parses() {
    use lens_lib::proxy::parser::Provider;
    use lens_lib::proxy::request::parse_request_data;

    let body = serde_json::json!({
        "model": "gpt-5-codex",
        "instructions": "You are Codex.",
        "input": [
            {"type": "message", "role": "user",
             "content": [{"type": "input_text", "text": "refactor auth"}]},
            {"type": "function_call", "name": "read_file", "call_id": "c1",
             "arguments": "{\"path\":\"a.ts\"}"},
            {"type": "function_call_output", "call_id": "c1", "output": "ok"},
            {"type": "message", "role": "assistant",
             "content": [{"type": "output_text", "text": "done"}]}
        ],
        "store": false,
        "previous_response_id": "resp_1"
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();
    let raw = format!(
        "POST /v1/responses HTTP/1.1\r\n\
         Host: api.openai.com\r\n\
         Content-Type: application/json\r\n\
         User-Agent: codex-cli/0.82.0\r\n\
         Content-Length: {}\r\n\r\n{}",
        body_bytes.len(),
        String::from_utf8_lossy(&body_bytes),
    );

    let (req, agent) = parse_request_data(raw.as_bytes()).expect("responses input must parse");
    assert_eq!(agent, "codex");
    assert!(matches!(req.provider, Provider::OpenAI));
    assert_eq!(
        req.messages.len(),
        2,
        "role messages kept; function_call* skipped"
    );
    assert_eq!(req.messages[0].role, "user");
    assert_eq!(req.messages[1].role, "assistant");
    assert!(req.metadata.contains_key("codex.previous_response_id"));
}

#[test]
fn test_sse_collector_aggregates_openai_chat_stream() {
    use lens_lib::proxy::parser::Provider;
    use lens_lib::proxy::sse::SseCollector;

    let mut collector = SseCollector::new(Provider::OpenAI);
    for i in 0..3 {
        collector.feed(
            format!(
                "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"d{i}\"}},\"finish_reason\":null}}]}}\n\n"
            )
            .as_bytes(),
        );
    }
    collector.feed(b"data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2,\"total_tokens\":9}}\n\n");
    collector.feed(b"data: [DONE]\n\n");

    let resp = collector.finish("r1").expect("must aggregate");
    assert_eq!(resp.content[0].text.as_deref(), Some("d0d1d2"));
    assert_eq!(resp.usage.unwrap().total_tokens, Some(9));
    assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
    assert_eq!(resp.request_id, "r1");
}

#[test]
fn test_sse_collector_aggregates_anthropic_stream() {
    use lens_lib::proxy::parser::Provider;
    use lens_lib::proxy::sse::SseCollector;

    let mut collector = SseCollector::new(Provider::Anthropic);
    collector.feed(b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"claude-x\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0,\"cache_read_input_tokens\":3}}}\n\n");
    collector.feed(b"data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\n");
    collector.feed(b"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi \"}}\n\n");
    collector.feed(b"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"there\"}}\n\n");
    collector.feed(b"data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu1\",\"name\":\"Bash\"}}\n\n");
    collector.feed(b"data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\":\"}}\n\n");
    collector.feed(b"data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"ls\\\"}\"}}\n\n");
    collector.feed(b"data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":9}}\n\n");

    let resp = collector.finish("r2").expect("must aggregate");
    assert_eq!(resp.content[0].text.as_deref(), Some("Hi there"));
    let calls = resp.tool_calls.expect("tool call must be captured");
    assert_eq!(calls[0].function.name, "Bash");
    assert_eq!(calls[0].function.arguments["cmd"], serde_json::json!("ls"));
    let usage = resp.usage.unwrap();
    assert_eq!(usage.prompt_tokens, Some(8), "input + cache_read");
    assert_eq!(usage.completion_tokens, Some(9));
    assert_eq!(resp.finish_reason.as_deref(), Some("tool_use"));
}

#[test]
fn test_sse_collector_prefers_responses_completed_object() {
    use lens_lib::proxy::parser::Provider;
    use lens_lib::proxy::sse::SseCollector;

    let mut collector = SseCollector::new(Provider::OpenAI);
    collector.feed(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n");
    collector.feed(b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Full answer\"}]}],\"usage\":{\"input_tokens\":4,\"output_tokens\":6,\"total_tokens\":10}}}\n\n");

    let resp = collector.finish("r3").expect("must aggregate");
    assert_eq!(resp.content[0].text.as_deref(), Some("Full answer"));
    assert_eq!(resp.usage.unwrap().total_tokens, Some(10));
    assert_eq!(resp.finish_reason.as_deref(), Some("completed"));
}

#[test]
fn test_agent_identification_covers_known_vendors() {
    use lens_lib::adapter::{identify, AgentKind};

    fn id(ua: &str) -> AgentKind {
        let mut headers = std::collections::HashMap::new();
        headers.insert("user-agent".into(), ua.to_string());
        identify(&headers, b"{}")
    }

    assert_eq!(id("claude-code/2.0.10 (linux; x64)"), AgentKind::ClaudeCode);
    assert_eq!(id("codex-cli/0.46.0"), AgentKind::Codex);
    assert_eq!(id("opencode/1.2.0"), AgentKind::OpenCode);
    assert_eq!(id("trae/1.0.0"), AgentKind::Trae);
    assert_eq!(id("kimi-code/1.5.3"), AgentKind::KimiCode);
    assert_eq!(id("qoder/1.0"), AgentKind::Qoder);
    assert_eq!(id("KiloCode/1.0"), AgentKind::KiloCode);
    assert_eq!(id("openclaw/0.3.0"), AgentKind::OpenClaw);
    assert_eq!(id("gemini-cli/0.1.0"), AgentKind::GeminiCli);
    assert_eq!(id("cursor-agent/1.0"), AgentKind::Cursor);
    assert!(matches!(id("undisclosed/1.0"), AgentKind::Unknown(_)));
}

#[test]
fn test_agent_kind_maps_to_agent_type_strings() {
    use lens_lib::proxy::request::parse_request_str;

    let body = r#"{"model":"gpt-5","messages":[{"role":"user","content":"hi"}]}"#;
    for (ua, expect) in [
        ("qoder/1.0", "qoder"),
        ("KiloCode/1.0", "kilocode"),
        ("openclaw/0.3", "openclaw"),
        ("gemini-cli/0.1", "gemini-cli"),
        ("cursor-agent/1.0", "cursor"),
    ] {
        let req_str = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nUser-Agent: {ua}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (_, agent_type) = parse_request_str(&req_str).expect("request must parse");
        assert_eq!(agent_type, expect, "UA {ua}");
    }
}

#[test]
fn test_session_title_redacts_credentials() {
    use lens_lib::engine::store::redact_secrets;

    // 真实形态：用户把 key 直接贴在提示里，标题会显示在面板侧边栏
    let cases = [
        "帮我用 sk-TESTONLYabcdef1234567890 这个 key 跑一下",
        "gitcode/badhope/，密钥是ghp_TESTONLYabcdef12345678 有一个仓库",
        "curl -H 'Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abcd'",
        "export API_KEY=abcdef1234567890abcdef",
        "token: xoxb-TESTONLY-000000000000",
    ];
    for text in cases {
        let out = redact_secrets(text);
        assert!(
            out.contains(lens_lib::engine::store::REDACTED),
            "must redact in {text:?}, got {out:?}"
        );
        assert!(
            !out.contains("sk-TESTONLY") && !out.contains("ghp_TESTONLY") && !out.contains("xoxb-TESTONLY"),
            "raw credential must not survive redaction: {out:?}"
        );
    }

    // 正常标题不能被误伤
    let safe = [
        "重构 auth 模块的 JWT 校验逻辑",
        "为什么我的 build 失败了",
        "更新 README 里的安装步骤",
    ];
    for text in safe {
        let out = redact_secrets(text);
        assert_eq!(
            out, text,
            "ordinary title must pass through unchanged: {text:?}"
        );
    }
}

#[test]
fn test_recorded_session_title_is_redacted_end_to_end() {
    use lens_lib::engine::Store;
    use lens_lib::proxy::parser::{LlmRequest, LlmResponse, Message, MessageContent, Provider};

    let dir = std::env::temp_dir().join(format!("lens-redact-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).expect("store opens");

    let request = LlmRequest {
        id: "t1".into(),
        timestamp: "2026-10-15T00:00:00Z".into(),
        provider: Provider::OpenAI,
        endpoint: "/v1/chat/completions".into(),
        messages: vec![Message {
            role: "user".into(),
            content: MessageContent::Text(
                "用 sk-live-9f8e7d6c5b4a3210 这个 key 试试能不能连通".into(),
            ),
            name: None,
        }],
        model: Some("gpt-4o".into()),
        temperature: None,
        tools: None,
        metadata: std::collections::HashMap::new(),
    };
    let response = LlmResponse {
        request_id: "t1".into(),
        timestamp: "2026-10-15T00:00:01Z".into(),
        provider: Provider::OpenAI,
        content: vec![],
        tool_calls: None,
        usage: None,
        finish_reason: None,
        raw: serde_json::Value::Null,
    };

    store
        .record_turn("opencode", request, response)
        .expect("record succeeds");

    let sessions = store.list_sessions(10, 0).expect("list succeeds");
    assert_eq!(sessions.len(), 1);
    let title = &sessions[0].title;
    assert!(
        !title.contains("sk-live-9f8e7d6c5b4a3210"),
        "credential must never reach the UI title, got {title:?}"
    );
    assert!(
        title.contains(lens_lib::engine::store::REDACTED),
        "got {title:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_truncated_credential_fragment_is_still_redacted() {
    use lens_lib::engine::store::redact_secrets;

    // 真实数据里出现的形态：密钥在长提示里被截断，只剩短后缀
    let cases = [
        "{\"_type\":\"newapi_channel_conn\",\"key\":\"sk-TESTONLY",
        "配置里填 sk-ab12cd 然后重试",
        "ghp_Ab1Cd2",
    ];
    for text in cases {
        let out = redact_secrets(text);
        assert!(
            !out.contains("sk-TESTONLY")
                && !out.contains("sk-ab12cd")
                && !out.contains("ghp_Ab1Cd2"),
            "short credential fragment must be redacted, got {out:?}"
        );
    }

    // 不能因为放宽阈值而误伤普通短词
    for text in ["sk-see", "task-1", "risk-2", "disk-usage"] {
        assert_eq!(redact_secrets(text), text, "must not over-redact {text:?}");
    }
}
