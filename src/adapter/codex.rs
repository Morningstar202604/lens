//! Codex CLI 适配器
//!
//! OpenAI Codex 使用 Responses API（/responses），
//! 特点是有 store、previous_response_id、instructions 等字段。

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct CodexContext {
    pub previous_id: Option<String>,
    pub store: Option<bool>,
    pub model_reasoning_effort: Option<String>,
}

pub fn extract_context(body: &Value) -> CodexContext {
    CodexContext {
        previous_id: body["previous_response_id"].as_str().map(String::from),
        store: body["store"].as_bool(),
        model_reasoning_effort: body["reasoning"]["effort"].as_str().map(String::from),
    }
}

/// 判断是否为 Codex Responses API 请求
pub fn is_responses_api(path: &str) -> bool {
    path.contains("/v1/responses")
}
