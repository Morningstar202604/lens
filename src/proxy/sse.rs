//! SSE 事件聚合：把 text/event-stream 响应还原为完整 LlmResponse。
//!
//! 支持三种流形态：
//! - OpenAI Chat Completions（`choices[].delta` 增量 + 尾部 usage）
//! - OpenAI Responses API（`response.*` 事件，优先取 `response.completed` 的完整对象）
//! - Anthropic Messages（`message_start`/`content_block_delta`/`message_delta`）

use std::collections::BTreeMap;

use serde_json::Value;

use super::parser::{
    ContentBlock, LlmResponse, Provider, ResponseParser, TokenUsage, ToolCall, ToolCallFunction,
};

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct PartialBlock {
    block_type: String,
    tool_id: String,
    tool_name: String,
    text: String,
    json_args: String,
}

pub struct SseCollector {
    provider: Provider,
    buf: Vec<u8>,
    text: String,
    tool_calls: BTreeMap<usize, PartialToolCall>,
    blocks: BTreeMap<usize, PartialBlock>,
    resp_tool_args: BTreeMap<String, (String, String)>,
    usage: Option<TokenUsage>,
    finish_reason: Option<String>,
    model: Option<String>,
    completed_response: Option<Value>,
    response_id: Option<String>,
    saw_data: bool,
}

impl SseCollector {
    pub fn new(provider: Provider) -> Self {
        Self {
            provider,
            buf: Vec::new(),
            text: String::new(),
            tool_calls: BTreeMap::new(),
            blocks: BTreeMap::new(),
            resp_tool_args: BTreeMap::new(),
            usage: None,
            finish_reason: None,
            model: None,
            completed_response: None,
            response_id: None,
            saw_data: false,
        }
    }

    /// 喂入任意字节切片（可直接来自 chunked 转发路径）。
    pub fn feed(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.strip_prefix(' ').unwrap_or(data);
                self.on_data(data);
            }
        }
        if self.buf.len() > 1_048_576 {
            tracing::warn!("SSE 单行缓冲超过 1MiB,丢弃该残留行(已聚合内容保留)");
            self.buf.clear();
        }
    }

    fn on_data(&mut self, data: &str) {
        if data == "[DONE]" || data.is_empty() {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return;
        };
        self.saw_data = true;
        match self.provider {
            Provider::OpenAI => self.on_openai(&value),
            Provider::Anthropic => self.on_anthropic(&value),
            Provider::Unknown(_) => {}
        }
    }

    fn on_openai(&mut self, v: &Value) {
        if let Some(kind) = v["type"].as_str() {
            match kind {
                "response.completed" | "response.incomplete" | "response.failed" => {
                    if v["response"].is_object() {
                        if let Some(id) = v["response"]["id"].as_str() {
                            self.response_id = Some(id.to_string());
                        }
                        self.completed_response = Some(v["response"].clone());
                    }
                }
                "response.output_item.added" => {
                    let item = &v["item"];
                    if item["type"] == "function_call" {
                        let id = item["call_id"]
                            .as_str()
                            .or(item["id"].as_str())
                            .unwrap_or_default()
                            .to_string();
                        let name = item["name"].as_str().unwrap_or_default().to_string();
                        self.resp_tool_args
                            .entry(id)
                            .or_insert((name, String::new()));
                    }
                }
                "response.function_call_arguments.delta" => {
                    let id = v["item_id"]
                        .as_str()
                        .or(v["call_id"].as_str())
                        .unwrap_or_default()
                        .to_string();
                    if let Some(delta) = v["delta"].as_str() {
                        self.resp_tool_args
                            .entry(id)
                            .or_insert((String::new(), String::new()))
                            .1
                            .push_str(delta);
                    }
                }
                "response.output_text.delta" => {
                    if let Some(delta) = v["delta"].as_str() {
                        self.text.push_str(delta);
                    }
                }
                _ => {}
            }
            return;
        }

        if let Some(id) = v["id"].as_str() {
            self.response_id = Some(id.to_string());
        }
        if let Some(model) = v["model"].as_str() {
            self.model = Some(model.to_string());
        }
        if let Some(u) = v.get("usage") {
            if !u.is_null() {
                self.usage = Some(TokenUsage {
                    prompt_tokens: u["prompt_tokens"].as_i64(),
                    completion_tokens: u["completion_tokens"].as_i64(),
                    total_tokens: u["total_tokens"].as_i64(),
                });
            }
        }
        if let Some(choices) = v["choices"].as_array() {
            if let Some(choice) = choices.first() {
                if let Some(reason) = choice["finish_reason"].as_str() {
                    let reason = reason.to_string();
                    self.finish_reason = Some(reason);
                }
                let delta = &choice["delta"];
                if let Some(text) = delta["content"].as_str() {
                    self.text.push_str(text);
                }
                if let Some(calls) = delta["tool_calls"].as_array() {
                    for call in calls {
                        let idx = call["index"].as_u64().unwrap_or(0) as usize;
                        let entry = self.tool_calls.entry(idx).or_default();
                        if let Some(id) = call["id"].as_str() {
                            entry.id = id.to_string();
                        }
                        if let Some(name) = call["function"]["name"].as_str() {
                            entry.name = name.to_string();
                        }
                        if let Some(args) = call["function"]["arguments"].as_str() {
                            entry.arguments.push_str(args);
                        }
                    }
                }
            }
        }
    }

    fn on_anthropic(&mut self, v: &Value) {
        let Some(kind) = v["type"].as_str() else {
            return;
        };
        match kind {
            "message_start" => {
                let msg = &v["message"];
                if let Some(id) = msg["id"].as_str() {
                    self.response_id = Some(id.to_string());
                }
                if let Some(model) = msg["model"].as_str() {
                    self.model = Some(model.to_string());
                }
                let u = &msg["usage"];
                let prompt = [
                    "input_tokens",
                    "cache_read_input_tokens",
                    "cache_creation_input_tokens",
                ]
                .iter()
                .filter_map(|k| u.get(*k).and_then(|x| x.as_i64()))
                .sum::<i64>();
                self.usage = Some(TokenUsage {
                    prompt_tokens: Some(prompt),
                    completion_tokens: u["output_tokens"].as_i64(),
                    total_tokens: None,
                });
            }
            "content_block_start" => {
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                let block = &v["content_block"];
                let entry = self.blocks.entry(idx).or_default();
                entry.block_type = block["type"].as_str().unwrap_or("text").to_string();
                if entry.block_type == "tool_use" {
                    entry.tool_id = block["id"].as_str().unwrap_or_default().to_string();
                    entry.tool_name = block["name"].as_str().unwrap_or_default().to_string();
                }
            }
            "content_block_delta" => {
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                let delta = &v["delta"];
                let entry = self.blocks.entry(idx).or_default();
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        if let Some(t) = delta["text"].as_str() {
                            entry.text.push_str(t);
                        }
                    }
                    "input_json_delta" => {
                        if let Some(j) = delta["partial_json"].as_str() {
                            entry.json_args.push_str(j);
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(reason) = v["delta"]["stop_reason"].as_str() {
                    self.finish_reason = Some(reason.to_string());
                }
                let u = &v["usage"];
                if let Some(out) = u["output_tokens"].as_i64() {
                    let mut usage = self.usage.take().unwrap_or_default();
                    usage.completion_tokens = Some(out);
                    self.usage = Some(usage);
                }
            }
            _ => {}
        }
    }

    /// 消费 collector，产出可入库的响应；无有效内容时返回 None。
    pub fn finish(self, request_id: &str) -> Option<LlmResponse> {
        if !self.saw_data {
            return None;
        }

        // Responses API：优先回溯完整对象
        if let Some(complete) = &self.completed_response {
            if let Some(resp) =
                ResponseParser::parse_response_value(&self.provider, complete, request_id)
            {
                return Some(resp);
            }
        }

        let mut content: Vec<ContentBlock> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        if !self.blocks.is_empty() {
            for (idx, block) in self.blocks {
                if block.block_type == "tool_use" {
                    tool_calls.push(ToolCall {
                        id: if block.tool_id.is_empty() {
                            format!("tool-{idx}")
                        } else {
                            block.tool_id
                        },
                        call_type: "tool_use".to_string(),
                        function: ToolCallFunction {
                            name: block.tool_name,
                            arguments: parse_json_or_string(&block.json_args),
                        },
                    });
                } else if !block.text.is_empty() {
                    content.push(ContentBlock {
                        block_type: "text".to_string(),
                        text: Some(block.text),
                        image_url: None,
                    });
                }
            }
        }
        if !self.text.is_empty() {
            content.push(ContentBlock {
                block_type: "text".to_string(),
                text: Some(self.text),
                image_url: None,
            });
        }
        for (_, call) in self.tool_calls {
            tool_calls.push(ToolCall {
                id: call.id,
                call_type: "function".to_string(),
                function: ToolCallFunction {
                    name: call.name,
                    arguments: parse_json_or_string(&call.arguments),
                },
            });
        }
        for (id, (name, args)) in self.resp_tool_args {
            if name.is_empty() {
                continue;
            }
            tool_calls.push(ToolCall {
                id,
                call_type: "function".to_string(),
                function: ToolCallFunction {
                    name,
                    arguments: parse_json_or_string(&args),
                },
            });
        }

        if content.is_empty()
            && tool_calls.is_empty()
            && self.usage.is_none()
            && self.finish_reason.is_none()
        {
            return None;
        }

        let mut usage = self.usage;
        if let Some(u) = usage.as_mut() {
            if u.total_tokens.is_none() {
                u.total_tokens = u.prompt_tokens.zip(u.completion_tokens).map(|(a, b)| a + b);
            }
        }

        Some(LlmResponse {
            request_id: request_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            provider: self.provider,
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage,
            finish_reason: self.finish_reason,
            raw: serde_json::json!({
                "streamed": true,
                "model": self.model,
                "id": self.response_id,
            }),
        })
    }
}

fn parse_json_or_string(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return Value::Null;
    }
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}
