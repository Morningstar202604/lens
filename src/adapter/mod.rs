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
        if ua_lower.contains("claude") {
            return AgentKind::ClaudeCode;
        }
        if ua_lower.contains("trae") {
            return AgentKind::Trae;
        }
        if ua_lower.contains("kimi") {
            return AgentKind::KimiCode;
        }
    }

    // 2. 看请求体特征
    if let Ok(json) = serde_json::from_slice::<Value>(body) {
        // Claude Code 特有：system 消息有特定格式
        if json["system"].is_array() {
            return AgentKind::ClaudeCode;
        }
        // Codex Responses API：有 input 字段
        if json["input"].is_array() && json["model"].as_str().is_some_and(|m| m.contains("o1") || m.contains("gpt")) {
            return AgentKind::Codex;
        }
    }

    AgentKind::Unknown("unidentified".to_string())
}
