//! LLM API 请求/响应解析
//!
//! 支持 OpenAI Responses API / Chat Completions 和 Anthropic Messages API。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use ts_rs::TS;
use uuid::Uuid;

/// 一条 LLM 请求记录
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct LlmRequest {
    pub id: String,
    pub timestamp: String,
    pub provider: Provider,
    pub endpoint: String,
    pub messages: Vec<Message>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub tools: Option<Vec<ToolDef>>,
    pub metadata: HashMap<String, String>,
}

/// 一条 LLM 响应记录
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct LlmResponse {
    pub request_id: String,
    pub timestamp: String,
    pub provider: Provider,
    pub content: Vec<ContentBlock>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
    #[ts(type = "unknown")]
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub enum Provider {
    OpenAI,
    Anthropic,
    Unknown(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: Option<String>,
    #[ts(type = "unknown")]
    pub image_url: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: Option<ToolFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct ToolFunction {
    pub name: String,
    pub description: Option<String>,
    #[ts(type = "unknown")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct ToolCallFunction {
    pub name: String,
    #[ts(type = "unknown")]
    pub arguments: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct TokenUsage {
    #[ts(type = "number")]
    pub prompt_tokens: Option<i64>,
    #[ts(type = "number")]
    pub completion_tokens: Option<i64>,
    #[ts(type = "number")]
    pub total_tokens: Option<i64>,
}

// ===================== 请求解析 =====================

pub struct RequestParser;

impl RequestParser {
    /// 解析请求（如果识别为 LLM API 调用）
    pub fn parse_request(
        method: &str,
        path: &str,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> Option<LlmRequest> {
        if method != "POST" && method != "GET" {
            return None;
        }

        let provider = Self::detect_provider(path, headers);
        if matches!(provider, Provider::Unknown(_)) {
            return None;
        }

        let json: Value = serde_json::from_slice(body).ok()?;
        let messages = Self::extract_messages(&json, &provider)?;
        let model = json["model"].as_str().map(String::from);
        let temperature = json["temperature"].as_f64();
        let tools = Self::extract_tools(&json);

        Some(LlmRequest {
            id: Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            provider,
            endpoint: path.to_string(),
            messages,
            model,
            temperature,
            tools,
            metadata: Self::extract_metadata(headers),
        })
    }

    fn detect_provider(path: &str, headers: &HashMap<String, String>) -> Provider {
        if path.contains("openai.com")
            || path.contains("/v1/chat/completions")
            || path.contains("/v1/responses")
        {
            return Provider::OpenAI;
        }
        if path.contains("anthropic.com") || path.contains("/v1/messages") {
            return Provider::Anthropic;
        }
        if let Some(auth) = headers.get("authorization") {
            if auth.contains("sk-") {
                return Provider::OpenAI;
            }
        }
        Provider::Unknown(path.to_string())
    }

    fn extract_messages(json: &Value, provider: &Provider) -> Option<Vec<Message>> {
        match provider {
            Provider::OpenAI => {
                Self::parse_openai_messages(json).or_else(|| Self::parse_responses_input(json))
            }
            Provider::Anthropic => Self::parse_anthropic_messages(json),
            Provider::Unknown(_) => None,
        }
    }

    /// Responses API（Codex 等）请求：`input` 数组（无 `messages` 键）。
    ///
    /// role 消息（user/assistant/developer/system）归一化为 Message；
    /// `function_call*` 项不进入 messages（工具定义已由 `tools` 字段单独捕获）。
    fn parse_responses_input(json: &Value) -> Option<Vec<Message>> {
        let input = json["input"].as_array()?;
        let mut result = Vec::new();

        for item in input {
            if item["type"]
                .as_str()
                .is_some_and(|t| t.starts_with("function_call"))
            {
                continue;
            }
            let Some(role) = item["role"].as_str() else {
                continue;
            };
            let content = if let Some(text) = item["content"].as_str() {
                MessageContent::Text(text.to_string())
            } else if let Some(blocks) = item["content"].as_array() {
                let cblocks: Vec<ContentBlock> = blocks
                    .iter()
                    .filter_map(|b| {
                        let text = b["text"].as_str()?;
                        Some(ContentBlock {
                            block_type: b["type"].as_str().unwrap_or("text").to_string(),
                            text: Some(text.to_string()),
                            image_url: None,
                        })
                    })
                    .collect();
                MessageContent::Blocks(cblocks)
            } else {
                continue;
            };
            result.push(Message {
                role: role.to_string(),
                content,
                name: None,
            });
        }

        Some(result)
    }

    fn parse_openai_messages(json: &Value) -> Option<Vec<Message>> {
        let msgs = json["messages"].as_array()?;
        let mut result = Vec::new();

        for msg in msgs {
            let role = msg["role"].as_str()?.to_string();
            let content = if let Some(text) = msg["content"].as_str() {
                MessageContent::Text(text.to_string())
            } else if let Some(blocks) = msg["content"].as_array() {
                let cblocks: Vec<ContentBlock> = blocks
                    .iter()
                    .filter_map(|b| {
                        let block_type = b["type"].as_str()?;
                        Some(ContentBlock {
                            block_type: block_type.to_string(),
                            text: b["text"].as_str().map(String::from),
                            image_url: b.get("image_url").cloned(),
                        })
                    })
                    .collect();
                MessageContent::Blocks(cblocks)
            } else {
                continue;
            };

            result.push(Message {
                role,
                content,
                name: msg["name"].as_str().map(String::from),
            });
        }

        Some(result)
    }

    fn parse_anthropic_messages(json: &Value) -> Option<Vec<Message>> {
        let msgs = json["messages"].as_array()?;
        let mut result = Vec::new();

        for msg in msgs {
            let role = msg["role"].as_str()?.to_string();
            let content = if let Some(text) = msg["content"].as_str() {
                MessageContent::Text(text.to_string())
            } else if let Some(blocks) = msg["content"].as_array() {
                let cblocks: Vec<ContentBlock> = blocks
                    .iter()
                    .filter_map(|b| match b["type"].as_str()? {
                        "text" => Some(ContentBlock {
                            block_type: "text".to_string(),
                            text: b["text"].as_str().map(String::from),
                            image_url: None,
                        }),
                        "tool_use" => Some(ContentBlock {
                            block_type: "tool_use".to_string(),
                            text: b["name"].as_str().map(String::from),
                            image_url: None,
                        }),
                        "tool_result" => Some(ContentBlock {
                            block_type: "tool_result".to_string(),
                            text: b["content"].as_str().map(String::from),
                            image_url: None,
                        }),
                        _ => None,
                    })
                    .collect();
                MessageContent::Blocks(cblocks)
            } else {
                continue;
            };

            result.push(Message {
                role,
                content,
                name: msg["name"].as_str().map(String::from),
            });
        }

        Some(result)
    }

    fn extract_tools(json: &Value) -> Option<Vec<ToolDef>> {
        let tools = json["tools"].as_array()?;
        let result: Vec<ToolDef> = tools
            .iter()
            .filter_map(|t| {
                let tool_type = t["type"].as_str()?;
                let function = t.get("function").and_then(|f| {
                    Some(ToolFunction {
                        name: f["name"].as_str()?.to_string(),
                        description: f["description"].as_str().map(String::from),
                        parameters: f.get("parameters").cloned(),
                    })
                });
                Some(ToolDef {
                    tool_type: tool_type.to_string(),
                    function,
                })
            })
            .collect();
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    fn extract_metadata(headers: &HashMap<String, String>) -> HashMap<String, String> {
        let mut meta = HashMap::new();
        for key in &["user-agent", "content-type", "x-request-id"] {
            if let Some(val) = headers.get(*key) {
                meta.insert(key.to_string(), val.clone());
            }
        }
        meta
    }
}

// ===================== 响应解析 =====================

pub struct ResponseParser;

impl ResponseParser {
    pub fn parse_response(
        provider: &Provider,
        body: &[u8],
        request_id: &str,
    ) -> Option<LlmResponse> {
        let json: Value = serde_json::from_slice(body).ok()?;
        Self::parse_response_value(provider, &json, request_id)
    }

    /// 从已解析的 JSON 构建响应（SSE 聚合与完整对象回溯共用）。
    pub fn parse_response_value(
        provider: &Provider,
        json: &Value,
        request_id: &str,
    ) -> Option<LlmResponse> {
        match provider {
            Provider::OpenAI => Self::parse_openai_response(json, request_id),
            Provider::Anthropic => Self::parse_anthropic_response(json, request_id),
            Provider::Unknown(_) => None,
        }
    }

    fn parse_openai_response(json: &Value, request_id: &str) -> Option<LlmResponse> {
        // Chat Completions 格式
        if let Some(choices) = json["choices"].as_array() {
            let first = choices.first()?;
            let msg = &first["message"];
            let content_text = msg["content"].as_str().unwrap_or("");

            let tool_calls = msg["tool_calls"].as_array().map(|calls| {
                calls
                    .iter()
                    .filter_map(|c| {
                        Some(ToolCall {
                            id: c["id"].as_str()?.to_string(),
                            call_type: c["type"].as_str()?.to_string(),
                            function: ToolCallFunction {
                                name: c["function"]["name"].as_str()?.to_string(),
                                arguments: c["function"]["arguments"].clone(),
                            },
                        })
                    })
                    .collect()
            });

            let usage = json.get("usage").map(|u| TokenUsage {
                prompt_tokens: u["prompt_tokens"].as_i64(),
                completion_tokens: u["completion_tokens"].as_i64(),
                total_tokens: u["total_tokens"].as_i64(),
            });

            Some(LlmResponse {
                request_id: request_id.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                provider: Provider::OpenAI,
                content: vec![ContentBlock {
                    block_type: "text".to_string(),
                    text: Some(content_text.to_string()),
                    image_url: None,
                }],
                tool_calls,
                usage,
                finish_reason: first["finish_reason"].as_str().map(String::from),
                raw: json.clone(),
            })
        } else if let Some(outputs) = json["output"].as_array() {
            // Responses API 格式
            let mut content = Vec::new();
            let mut tool_calls = Vec::new();

            for item in outputs {
                if item["type"] == "message" {
                    if let Some(contents) = item["content"].as_array() {
                        for c in contents {
                            content.push(ContentBlock {
                                block_type: c["type"].as_str().unwrap_or("text").to_string(),
                                text: c["text"].as_str().map(String::from),
                                image_url: None,
                            });
                        }
                    }
                } else if item["type"] == "function_call" {
                    tool_calls.push(ToolCall {
                        id: item["id"].as_str().unwrap_or("").to_string(),
                        call_type: "function".to_string(),
                        function: ToolCallFunction {
                            name: item["name"].as_str().unwrap_or("").to_string(),
                            arguments: item["arguments"].clone(),
                        },
                    });
                }
            }

            Some(LlmResponse {
                request_id: request_id.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                provider: Provider::OpenAI,
                content,
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                usage: json.get("usage").map(|u| TokenUsage {
                    prompt_tokens: u["input_tokens"].as_i64(),
                    completion_tokens: u["output_tokens"].as_i64(),
                    total_tokens: u["total_tokens"].as_i64(),
                }),
                finish_reason: json["status"].as_str().map(String::from),
                raw: json.clone(),
            })
        } else {
            None
        }
    }

    fn parse_anthropic_response(json: &Value, request_id: &str) -> Option<LlmResponse> {
        let content_array = json["content"].as_array()?;
        let mut content = Vec::new();
        let mut tool_calls = Vec::new();

        for block in content_array {
            match block["type"].as_str().unwrap_or("text") {
                "text" => {
                    content.push(ContentBlock {
                        block_type: "text".to_string(),
                        text: block["text"].as_str().map(String::from),
                        image_url: None,
                    });
                }
                "tool_use" => {
                    tool_calls.push(ToolCall {
                        id: block["id"].as_str().unwrap_or("").to_string(),
                        call_type: "tool_use".to_string(),
                        function: ToolCallFunction {
                            name: block["name"].as_str().unwrap_or("").to_string(),
                            arguments: block["input"].clone(),
                        },
                    });
                }
                _ => {}
            }
        }

        let usage = json.get("usage").map(|u| TokenUsage {
            prompt_tokens: u["input_tokens"].as_i64(),
            completion_tokens: u["output_tokens"].as_i64(),
            total_tokens: u["input_tokens"]
                .as_i64()
                .zip(u["output_tokens"].as_i64())
                .map(|(a, b)| a + b),
        });

        Some(LlmResponse {
            request_id: request_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            provider: Provider::Anthropic,
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage,
            finish_reason: json["stop_reason"].as_str().map(String::from),
            raw: json.clone(),
        })
    }
}
