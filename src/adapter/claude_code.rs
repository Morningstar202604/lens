//! Claude Code 适配器
//!
//! Anthropic Claude Code 使用 Messages API，
//! 特点是有 system 数组（而非字符串）、特定工具定义格式。

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ClaudeContext {
    pub system_blocks: Option<Vec<SystemBlock>>,
    pub betas: Option<Vec<String>>,
    pub container_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SystemBlock {
    pub block_type: String,
    pub cache_control: Option<Value>,
    pub text: Option<String>,
}

pub fn extract_context(body: &Value, headers: &std::collections::HashMap<String, String>) -> ClaudeContext {
    let system = body["system"].as_array().map(|blocks| {
        blocks
            .iter()
            .filter_map(|b| {
                Some(SystemBlock {
                    block_type: b["type"].as_str()?.to_string(),
                    cache_control: b.get("cache_control").cloned(),
                    text: b["text"].as_str().map(String::from),
                })
            })
            .collect()
    });

    let betas = body["betas"].as_array().map(|arr| {
        arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
    }).or_else(|| {
        headers
            .get("x-anthropic-beta")
            .map(|b| b.split(',').map(|s| s.trim().to_string()).collect())
    });

    ClaudeContext {
        system_blocks: system,
        betas,
        container_id: headers.get("x-anthropic-container-id").cloned(),
    }
}
