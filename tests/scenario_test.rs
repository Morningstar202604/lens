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
        req.metadata.get("codex.previous_response_id").map(|s| s.as_str()),
        Some("resp_01ABC123"),
        "Codex adapter must extract previous_response_id",
    );
    assert_eq!(
        req.metadata.get("codex.reasoning_effort").map(|s| s.as_str()),
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
        req.metadata.get("claude.betas").is_some(),
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

    let sessions = store.list_sessions(100).expect("list_sessions should succeed");
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
fn test_session_id_hash_is_stable_and_collision_free() {
    use lens_lib::proxy::parser::{LlmRequest, Provider};

    let mk_req = |cwd: &str| LlmRequest {
        id: "x".into(),
        timestamp: "t".into(),
        provider: Provider::OpenAI,
        endpoint: "/v1/chat/completions".into(),
        messages: vec![],
        model: None,
        temperature: None,
        tools: None,
        metadata: {
            let mut m = HashMap::new();
            m.insert("cwd".into(), cwd.into());
            m
        },
    };

    let id1 = lens_lib::engine::Store::extract_session_id(&mk_req("/home/user/project"));
    let id2 = lens_lib::engine::Store::extract_session_id(&mk_req("/home/user/project"));
    assert_eq!(id1, id2, "same cwd should produce same session_id");

    let id3 = lens_lib::engine::Store::extract_session_id(&mk_req("/home/user/project-x"));
    assert_ne!(id1, id3, "different cwds should produce different session_ids");

    let id_a = lens_lib::engine::Store::extract_session_id(&mk_req("/a/b-c"));
    let id_b = lens_lib::engine::Store::extract_session_id(&mk_req("/a/b/c"));
    assert_ne!(id_a, id_b, "/a/b-c and /a/b/c must not collide");
}

#[test]
fn test_max_body_size_constant() {
    assert_eq!(lens_lib::proxy::io::MAX_BODY_SIZE, 10 * 1024 * 1024);
}
