//! OpenCode 适配器
//!
//! OpenCode 是开源 CLI Agent，支持多种 LLM 后端。
//! 特点：请求走标准 OpenAI/Anthropic API，但会在 metadata 中携带版本信息。

use serde_json::Value;

/// OpenCode 特有的上下文信息
#[derive(Debug, Clone)]
pub struct OpenCodeContext {
    pub version: Option<String>,
    pub workspace: Option<String>,
    pub session_id: Option<String>,
}

pub fn extract_context(body: &Value, headers: &std::collections::HashMap<String, String>) -> OpenCodeContext {
    OpenCodeContext {
        version: headers
            .get("x-opencode-version")
            .cloned()
            .or_else(|| body["metadata"]["opencode_version"].as_str().map(String::from)),
        workspace: body["metadata"]["cwd"]
            .as_str()
            .map(String::from),
        session_id: body["metadata"]["session_id"]
            .as_str()
            .map(String::from),
    }
}
