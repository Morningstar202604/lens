//! 各平台适配器 —— 解析不同 Agent 的会话格式
//!
//! 每个 adapter 负责识别特定 agent 的请求特征，
//! 并将其标准化为 Lens 的统一 Turn 格式。

pub mod claude_code;
pub mod codex;
pub mod opencode;

use serde_json::Value;

/// 识别 agent 类型
#[derive(Debug, Clone, PartialEq)]
pub enum AgentKind {
    OpenCode,
    Codex,
    ClaudeCode,
    KimiCode,
    Trae,
    Qoder,
    KiloCode,
    OpenClaw,
    GeminiCli,
    Cursor,
    Unknown(String),
}

/// 尝试从请求特征中识别 agent
pub fn identify(headers: &std::collections::HashMap<String, String>, body: &[u8]) -> AgentKind {
    // 1. 先看 User-Agent
    if let Some(ua) = headers.get("user-agent") {
        let ua_lower = ua.to_lowercase();
        if ua_lower.contains("opencode") {
            return AgentKind::OpenCode;
        }
        if ua_lower.contains("codex") {
            return AgentKind::Codex;
        }
        if ua_lower.contains("kimi") {
            return AgentKind::KimiCode;
        }
        if ua_lower.contains("trae") {
            return AgentKind::Trae;
        }
        if ua_lower.contains("qoder") {
            return AgentKind::Qoder;
        }
        if ua_lower.contains("kilo") {
            return AgentKind::KiloCode;
        }
        if ua_lower.contains("openclaw") {
            return AgentKind::OpenClaw;
        }
        if ua_lower.contains("gemini") {
            return AgentKind::GeminiCli;
        }
        if ua_lower.contains("cursor") {
            return AgentKind::Cursor;
        }
        if ua_lower.contains("claude") {
            return AgentKind::ClaudeCode;
        }
    }

    // 2. 看请求体特征
    if let Ok(json) = serde_json::from_slice::<Value>(body) {
        // Claude Code 特有：system 消息有特定格式
        if json["system"].is_array() {
            return AgentKind::ClaudeCode;
        }
        // Codex Responses API：有 input 字段
        if json["input"].is_array()
            && json["model"]
                .as_str()
                .is_some_and(|m| m.contains("o1") || m.contains("gpt"))
        {
            return AgentKind::Codex;
        }
    }

    AgentKind::Unknown("unidentified".to_string())
}

/// 根据已识别的 agent 类型提取上下文 metadata。
///
/// 返回的 key-value 对被写入 `LlmRequest.metadata`，
/// 用于后续的会话分析和图谱构建。
pub fn extract_context(
    kind: &AgentKind,
    headers: &std::collections::HashMap<String, String>,
    body: &str,
) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let json: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return result,
    };

    match kind {
        AgentKind::Codex => {
            let ctx = codex::extract_context(&json);
            if let Some(id) = ctx.previous_id {
                result.push(("codex.previous_response_id".into(), id));
            }
            if let Some(store) = ctx.store {
                result.push(("codex.store".into(), store.to_string()));
            }
            if let Some(effort) = ctx.model_reasoning_effort {
                result.push(("codex.reasoning_effort".into(), effort));
            }
            result.push(("codex.is_responses_api".into(), "true".into()));
        }
        AgentKind::ClaudeCode => {
            let ctx = claude_code::extract_context(&json, headers);
            if let Some(betas) = ctx.betas {
                result.push(("claude.betas".into(), betas.join(",")));
            }
            if let Some(container) = ctx.container_id {
                result.push(("claude.container_id".into(), container));
            }
            if let Some(cwd) = ctx.cwd {
                result.push(("claude.cwd".into(), cwd));
            }
            let num_blocks = ctx.system_blocks.as_ref().map(|b| b.len()).unwrap_or(0);
            result.push(("claude.system_blocks".into(), num_blocks.to_string()));
        }
        AgentKind::OpenCode => {
            if let Some(ws) = json["workspace"].as_str() {
                result.push(("opencode.workspace".into(), ws.into()));
            }
            if let Some(session) = json["session_id"].as_str() {
                result.push(("opencode.session_id".into(), session.into()));
            }
        }
        _ => {}
    }

    result
}
