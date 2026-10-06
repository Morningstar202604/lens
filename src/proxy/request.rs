//! HTTP 请求解析：从原始字节中提取 LLM 请求信息 + agent 识别。

use std::collections::HashMap;

use crate::adapter::{identify, AgentKind};
use super::parser::{LlmRequest, RequestParser};

/// 解析请求数据，返回 `(LlmRequest, agent_type)` 或 None（非 LLM 请求）。
pub fn parse_request_data(data: &[u8]) -> Option<(LlmRequest, String)> {
    let req_str = String::from_utf8_lossy(data);
    let headers_end = req_str.find("\r\n\r\n")?;
    let head = &req_str[..headers_end];
    let body = &req_str[headers_end + 4..];

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
    let agent_kind = identify(&headers, body.as_bytes());
    let agent_type = match &agent_kind {
        AgentKind::OpenCode => "opencode",
        AgentKind::Codex => "codex",
        AgentKind::ClaudeCode => "claude-code",
        AgentKind::Trae => "trae",
        AgentKind::KimiCode => "kimi-code",
        AgentKind::Unknown(_) => "unknown",
    };

    let req = RequestParser::parse_request(method, path, &headers, body.as_bytes())?;
    Some((req, agent_type.to_string()))
}
