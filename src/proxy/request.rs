//! HTTP 请求解析：从原始字节中提取 LLM 请求信息 + agent 识别 + 上下文提取。

use std::collections::HashMap;

use super::parser::{LlmRequest, RequestParser};
use crate::adapter::{identify, AgentKind};

/// 解析请求数据，返回 `(LlmRequest, agent_type)` 或 None（非 LLM 请求）。
///
/// 除了识别 agent 类型外，还会调用对应 adapter 的 `extract_context`，
/// 将敏感上下文（Codex 的 previous_response_id、Claude Code 的 system 块等）
/// 提取到 `LlmRequest.metadata` 中。
/// Parse HTTP request data (raw bytes) — identify agent and extract context.
///
/// This is the low-level entry used by callers that read raw bytes from streams.
/// Internally converts to `&str` and delegates to [`parse_request_str`].
pub fn parse_request_data(data: &[u8]) -> Option<(LlmRequest, String)> {
    let req_str = String::from_utf8_lossy(data);
    parse_request_str(&req_str)
}

/// Parse HTTP request from a string slice (after UTF-8 conversion).
pub fn parse_request_str(req_str: &str) -> Option<(LlmRequest, String)> {
    let headers_end = req_str.find("\r\n\r\n")?;
    let head = &req_str[..headers_end];
    let body_str = &req_str[headers_end + 4..];

    let lines: Vec<&str> = head.split("\r\n").collect();
    let request_line: Vec<&str> = lines[0].split_whitespace().collect();
    if request_line.len() < 2 {
        return None;
    }
    let method = request_line[0];
    let path = request_line[1];

    let mut headers = HashMap::new();
    for line in &lines[1..] {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    // Agent 识别
    let agent_kind = identify(&headers, body_str.as_bytes());
    let agent_type = match &agent_kind {
        AgentKind::OpenCode => "opencode",
        AgentKind::Codex => "codex",
        AgentKind::ClaudeCode => "claude-code",
        AgentKind::Trae => "trae",
        AgentKind::KimiCode => "kimi-code",
        AgentKind::Qoder => "qoder",
        AgentKind::KiloCode => "kilocode",
        AgentKind::OpenClaw => "openclaw",
        AgentKind::GeminiCli => "gemini-cli",
        AgentKind::Cursor => "cursor",
        AgentKind::Unknown(_) => "unknown",
    };

    let mut req = RequestParser::parse_request(method, path, &headers, body_str.as_bytes())?;

    // 调用对应 adapter 的 extract_context，将上下文写入 metadata
    let ctx = crate::adapter::extract_context(&agent_kind, &headers, body_str);
    for (k, v) in ctx {
        req.metadata.insert(k, v);
    }

    Some((req, agent_type.to_string()))
}
