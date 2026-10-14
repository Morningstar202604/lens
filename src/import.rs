//! 会话日志导入（零摩擦数据源）。
//!
//! 已实现：
//! - Claude Code JSONL（`~/.claude/projects/**/*.jsonl`）：按 `sessionId` 归组，
//!   以 assistant 条目的 `uuid` 作为 turn id（重复导入自动跳过），`usage` 含缓存读写；
//! - OpenCode 本地库（`~/.local/share/opencode/opencode.db`，SQLite 只读）：
//!   以 `message.id` 作为 turn id 幂等，tool part 映射为 tool_calls（输出截断保留）。
//!
//! 暂缓：Codex CLI 会话导入 —— 本机没有任何 `~/.codex` 会话样本可校准格式，
//! 不凭猜测实现（避免写出看似可用实则错位的解析器）；拿到真实 rollout 文件后再补。
//!
//! 归一化输出进入与代理相同的 `Store`，三种数据源在界面中同构。

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::engine::Store;
use crate::proxy::parser::{
    ContentBlock, LlmRequest, LlmResponse, Message, MessageContent, Provider, TokenUsage, ToolCall,
    ToolCallFunction,
};

#[derive(Debug, Default, Serialize)]
pub struct ImportStats {
    pub files: usize,
    pub turns_imported: usize,
    pub turns_skipped: usize,
}

/// 导入 Claude Code JSONL（文件或目录，目录递归）。
pub fn import_claude(data_dir: &Path, path: &Path) -> Result<ImportStats> {
    let store = Store::open(data_dir)?;
    let mut files = Vec::new();
    collect_jsonl(path, &mut files)?;

    let mut stats = ImportStats {
        files: files.len(),
        ..Default::default()
    };

    for file in &files {
        import_one_file(&store, file, &mut stats)?;
    }
    Ok(stats)
}

fn collect_jsonl(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    if !path.exists() {
        anyhow::bail!("导入路径不存在: {}", path.display());
    }
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "jsonl") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(())
}

fn import_one_file(store: &Store, file: &Path, stats: &mut ImportStats) -> Result<()> {
    let text = std::fs::read_to_string(file)?;
    let mut pending_users: Vec<Message> = Vec::new();
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;

    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(v) = entry["cwd"].as_str() {
            cwd = Some(v.to_string());
        }
        if session_id.is_none() {
            session_id = entry["sessionId"].as_str().map(String::from);
        }

        match entry["type"].as_str() {
            Some("user") => {
                if let Some(message) = user_message(&entry) {
                    pending_users.push(message);
                }
            }
            Some("assistant") => {
                let uuid = entry["uuid"].as_str().unwrap_or_default();
                if uuid.is_empty() || !is_new_turn(store, uuid) {
                    if !uuid.is_empty() {
                        stats.turns_skipped += 1;
                    }
                    continue;
                }
                let Some(response) = assistant_response(&entry, uuid) else {
                    continue;
                };
                let model = entry["message"]["model"].as_str().map(String::from);
                let mut metadata: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                if let Some(sid) = &session_id {
                    metadata.insert("session.native_id".into(), sid.clone());
                }
                if let Some(cwd) = &cwd {
                    metadata.insert("cwd".into(), cwd.clone());
                }
                metadata.insert("user-agent".into(), "claude-code/import".into());

                let request = LlmRequest {
                    id: uuid.to_string(),
                    timestamp: entry["timestamp"].as_str().unwrap_or_default().to_string(),
                    provider: Provider::Anthropic,
                    endpoint: "/v1/messages".into(),
                    messages: std::mem::take(&mut pending_users),
                    model,
                    temperature: None,
                    tools: None,
                    metadata,
                };
                store.record_turn_with_source("claude-code", "import", request, response)?;
                stats.turns_imported += 1;
            }
            _ => {}
        }
    }
    Ok(())
}

fn is_new_turn(store: &Store, uuid: &str) -> bool {
    let Ok(conn) = store.conn.lock() else {
        return false;
    };
    conn.query_row("SELECT 1 FROM turns WHERE id = ?", [uuid], |_| Ok(()))
        .is_err()
}

fn user_message(entry: &Value) -> Option<Message> {
    let message = &entry["message"];
    map_content(&message["content"]).map(|content| Message {
        role: "user".into(),
        content,
        name: None,
    })
}

fn assistant_response(entry: &Value, uuid: &str) -> Option<LlmResponse> {
    let message = &entry["message"];
    let mut content = Vec::new();
    let mut tool_calls = Vec::new();
    if let Some(blocks) = message["content"].as_array() {
        for block in blocks {
            match block["type"].as_str().unwrap_or("") {
                "text" => {
                    if let Some(text) = block["text"].as_str() {
                        content.push(ContentBlock {
                            block_type: "text".into(),
                            text: Some(text.to_string()),
                            image_url: None,
                        });
                    }
                }
                "tool_use" => {
                    tool_calls.push(ToolCall {
                        id: block["id"].as_str().unwrap_or_default().to_string(),
                        call_type: "tool_use".into(),
                        function: ToolCallFunction {
                            name: block["name"].as_str().unwrap_or_default().to_string(),
                            arguments: block["input"].clone(),
                        },
                    });
                }
                _ => {}
            }
        }
    } else if let Some(text) = message["content"].as_str() {
        content.push(ContentBlock {
            block_type: "text".into(),
            text: Some(text.to_string()),
            image_url: None,
        });
    }

    let usage = map_usage(&message["usage"]);
    let stop_reason = message["stop_reason"].as_str().map(String::from);

    if content.is_empty() && tool_calls.is_empty() && usage.is_none() {
        return None;
    }

    Some(LlmResponse {
        request_id: uuid.to_string(),
        timestamp: entry["timestamp"].as_str().unwrap_or_default().to_string(),
        provider: Provider::Anthropic,
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        usage,
        finish_reason: stop_reason,
        raw: Value::Null,
    })
}

fn map_content(value: &Value) -> Option<MessageContent> {
    if let Some(text) = value.as_str() {
        if text.trim().is_empty() {
            return None;
        }
        return Some(MessageContent::Text(text.to_string()));
    }
    let blocks = value.as_array()?;
    let mapped: Vec<ContentBlock> = blocks
        .iter()
        .filter_map(|b| match b["type"].as_str().unwrap_or("") {
            "text" => Some(ContentBlock {
                block_type: "text".into(),
                text: b["text"].as_str().map(String::from),
                image_url: None,
            }),
            "tool_result" => Some(ContentBlock {
                block_type: "tool_result".into(),
                text: b["content"].as_str().map(String::from),
                image_url: None,
            }),
            "thinking" => Some(ContentBlock {
                block_type: "thinking".into(),
                text: b["thinking"].as_str().map(String::from),
                image_url: None,
            }),
            _ => None,
        })
        .collect();
    if mapped.is_empty() {
        None
    } else {
        Some(MessageContent::Blocks(mapped))
    }
}

fn map_usage(usage: &Value) -> Option<TokenUsage> {
    if usage.is_null() {
        return None;
    }
    let input = usage["input_tokens"].as_i64().unwrap_or(0);
    let cache_read = usage["cache_read_input_tokens"].as_i64().unwrap_or(0);
    let cache_creation = usage["cache_creation_input_tokens"].as_i64().unwrap_or(0);
    let output = usage["output_tokens"].as_i64().unwrap_or(0);
    let prompt = input + cache_read + cache_creation;
    if prompt == 0 && output == 0 {
        return None;
    }
    Some(TokenUsage {
        prompt_tokens: Some(prompt),
        completion_tokens: Some(output),
        total_tokens: Some(prompt + output),
    })
}

/// 已导入过的文件集合（供 CLI 输出计数用）。
pub fn default_claude_projects_dir() -> Option<PathBuf> {
    homedir::my_home()
        .ok()
        .flatten()
        .map(|home| home.join(".claude").join("projects"))
}

// ───────────────────────── OpenCode（本地 SQLite） ─────────────────────────

/// 单个 tool 输出保留的最大字符数（避免把网页抓取等大输出整库吞入）。
const TOOL_OUTPUT_LIMIT: usize = 2000;

/// 导入 OpenCode 本地库（只读打开，不修改其文件）。
///
/// `session_limit` 限制导入最近会话数（按 time_updated 倒序），传 0 表示不限。
pub fn import_opencode(
    data_dir: &Path,
    db_path: &Path,
    session_limit: usize,
) -> Result<ImportStats> {
    let store = Store::open(data_dir)?;
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| anyhow::anyhow!("打开 OpenCode 库失败 {}: {}", db_path.display(), e))?;

    let limit = if session_limit == 0 {
        -1i64
    } else {
        session_limit as i64
    };
    let mut stmt = conn.prepare(
        "SELECT id, directory FROM session
         WHERE parent_id IS NULL
         ORDER BY time_updated DESC LIMIT ?1",
    )?;
    let sessions: Vec<(String, Option<String>)> = stmt
        .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut stats = ImportStats {
        files: 1,
        ..Default::default()
    };
    for (session_id, directory) in sessions {
        import_opencode_session(&store, &conn, &session_id, directory.as_deref(), &mut stats)?;
    }
    Ok(stats)
}

fn import_opencode_session(
    store: &Store,
    conn: &rusqlite::Connection,
    session_id: &str,
    directory: Option<&str>,
    stats: &mut ImportStats,
) -> Result<()> {
    let mut msg_stmt = conn
        .prepare("SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created, id")?;
    let messages: Vec<(String, String)> = msg_stmt
        .query_map([session_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut pending_users: Vec<Message> = Vec::new();
    for (message_id, data_json) in messages {
        let Ok(data) = serde_json::from_str::<Value>(&data_json) else {
            continue;
        };
        match data["role"].as_str().unwrap_or("") {
            "user" => {
                if let Some(message) = opencode_user_message(conn, &message_id) {
                    pending_users.push(message);
                }
            }
            "assistant" => {
                if is_new_turn(store, &message_id) {
                    if let Some(response) = opencode_assistant_response(conn, &message_id, &data) {
                        let mut metadata: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();
                        metadata.insert("session.native_id".into(), session_id.to_string());
                        if let Some(dir) = directory {
                            metadata.insert("cwd".into(), dir.to_string());
                        }
                        metadata.insert("user-agent".into(), "opencode/import".into());
                        let request = LlmRequest {
                            id: message_id.clone(),
                            timestamp: ms_to_rfc3339(data["time"]["created"].as_i64()),
                            provider: Provider::OpenAI,
                            endpoint: "/v1/chat/completions".into(),
                            messages: std::mem::take(&mut pending_users),
                            model: opencode_model(&data),
                            temperature: None,
                            tools: None,
                            metadata,
                        };
                        store.record_turn_with_source("opencode", "import", request, response)?;
                        stats.turns_imported += 1;
                    }
                } else {
                    stats.turns_skipped += 1;
                }
                pending_users.clear();
            }
            _ => {}
        }
    }
    Ok(())
}

fn opencode_parts(conn: &rusqlite::Connection, message_id: &str) -> Vec<Value> {
    let Ok(mut stmt) = conn.prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY id")
    else {
        return Vec::new();
    };
    stmt.query_map([message_id], |r| r.get::<_, String>(0))
        .map(|rows| {
            rows.filter_map(|r| r.ok())
                .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn opencode_user_message(conn: &rusqlite::Connection, message_id: &str) -> Option<Message> {
    let text = opencode_parts(conn, message_id)
        .iter()
        .filter(|p| p["type"].as_str() == Some("text"))
        .filter_map(|p| p["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return None;
    }
    Some(Message {
        role: "user".into(),
        content: MessageContent::Text(text),
        name: None,
    })
}

fn opencode_assistant_response(
    conn: &rusqlite::Connection,
    message_id: &str,
    data: &Value,
) -> Option<LlmResponse> {
    let mut content: Vec<ContentBlock> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for part in opencode_parts(conn, message_id) {
        match part["type"].as_str().unwrap_or("") {
            "text" => {
                if let Some(text) = part["text"].as_str() {
                    if !text.trim().is_empty() {
                        content.push(ContentBlock {
                            block_type: "text".into(),
                            text: Some(text.to_string()),
                            image_url: None,
                        });
                    }
                }
            }
            "reasoning" => {
                if let Some(text) = part["text"].as_str() {
                    content.push(ContentBlock {
                        block_type: "thinking".into(),
                        text: Some(text.to_string()),
                        image_url: None,
                    });
                }
            }
            "tool" => {
                let call_id = part["callID"].as_str().unwrap_or_default().to_string();
                let name = part["tool"].as_str().unwrap_or_default().to_string();
                tool_calls.push(ToolCall {
                    id: call_id,
                    call_type: "function".into(),
                    function: ToolCallFunction {
                        name,
                        arguments: part["state"]["input"].clone(),
                    },
                });
                if let Some(output) = part["state"]["output"].as_str() {
                    let truncated: String = output.chars().take(TOOL_OUTPUT_LIMIT).collect();
                    content.push(ContentBlock {
                        block_type: "tool_result".into(),
                        text: Some(truncated),
                        image_url: None,
                    });
                }
            }
            _ => {}
        }
    }

    let usage = opencode_usage(&data["tokens"]);
    if content.is_empty() && tool_calls.is_empty() && usage.is_none() {
        return None;
    }

    Some(LlmResponse {
        request_id: message_id.to_string(),
        timestamp: ms_to_rfc3339(data["time"]["created"].as_i64()),
        provider: Provider::OpenAI,
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        usage,
        finish_reason: data["finish"].as_str().map(String::from),
        raw: serde_json::json!({ "source": "opencode-import", "id": message_id }),
    })
}

fn opencode_model(data: &Value) -> Option<String> {
    data["modelID"]
        .as_str()
        .or_else(|| data["model"]["modelID"].as_str())
        .map(String::from)
}

fn opencode_usage(tokens: &Value) -> Option<TokenUsage> {
    if tokens.is_null() {
        return None;
    }
    let input = tokens["input"].as_i64().unwrap_or(0);
    let output = tokens["output"].as_i64().unwrap_or(0);
    let reasoning = tokens["reasoning"].as_i64().unwrap_or(0);
    let cache_read = tokens["cache"]["read"].as_i64().unwrap_or(0);
    let cache_write = tokens["cache"]["write"].as_i64().unwrap_or(0);
    let prompt = input + cache_read + cache_write;
    let completion = output + reasoning;
    if prompt == 0 && completion == 0 {
        return None;
    }
    Some(TokenUsage {
        prompt_tokens: Some(prompt),
        completion_tokens: Some(completion),
        total_tokens: Some(prompt + completion),
    })
}

fn ms_to_rfc3339(ms: Option<i64>) -> String {
    let ms = ms.unwrap_or(0);
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

/// OpenCode 默认库路径探测。
pub fn default_opencode_db() -> Option<PathBuf> {
    let home = homedir::my_home().ok().flatten()?;
    let candidates = [
        home.join(".local")
            .join("share")
            .join("opencode")
            .join("opencode.db"),
        home.join(".opencode").join("opencode.db"),
    ];
    candidates.into_iter().find(|p| p.exists())
}
