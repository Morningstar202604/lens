//! SQLite 持久化存储
//!
//! 用 Arc<Mutex<Connection>> + block_in_place 包装 rusqlite 使其可以跨线程使用。

use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::proxy::parser::{LlmRequest, LlmResponse};

pub struct Store {
    /// SQLite 连接（线程安全） */
    pub conn: Arc<Mutex<rusqlite::Connection>>,
    data_dir: PathBuf,
}

impl Store {
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = data_dir.join("lens.db");
        let conn = rusqlite::Connection::open(&db_path)?;
        // WAL 模式：允许并发读取，写入不阻塞读
        conn.pragma_update(None, "journal_mode", "wal")?;
        conn.pragma_update(None, "foreign_keys", "on")?;
        info!("数据库: {}", db_path.display());

        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            data_dir: data_dir.to_path_buf(),
        };

        // 初始化 schema
        {
            let conn = store.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS turns (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    agent_type TEXT NOT NULL DEFAULT 'unknown',
                    timestamp TEXT NOT NULL,
                    request_json TEXT NOT NULL,
                    response_json TEXT NOT NULL,
                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    project_path TEXT,
                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS nodes (
                    id TEXT PRIMARY KEY,
                    label TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    weight REAL DEFAULT 1.0,
                    first_seen DATETIME DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS edges (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source TEXT NOT NULL,
                    target TEXT NOT NULL,
                    relation TEXT NOT NULL,
                    weight REAL DEFAULT 1.0,
                    UNIQUE(source, target, relation)
                );

                CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session_id);
                CREATE INDEX IF NOT EXISTS idx_turns_timestamp ON turns(timestamp);
                CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
                CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
                "#,
            )?;
        }

        Ok(store)
    }

    /// 记录一次 LLM 调用（同步接口）
    ///
    /// 使用 SQLite 事务保护：session upsert + turn insert 原子性提交。
    pub fn record_turn(
        &self,
        agent_type: &str,
        request: LlmRequest,
        response: LlmResponse,
    ) -> Result<()> {
        let session_id = Self::extract_session_id(&request);

        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;

        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO sessions (id, title, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(id) DO UPDATE SET updated_at = CURRENT_TIMESTAMP",
            params![&session_id, &session_id],
        )?;

        let request_json = serde_json::to_string(&request)?;
        let response_json = serde_json::to_string(&response)?;
        // 优先使用从代理层传入的 agent_type，fallback 到请求体检测
        let resolved_agent = if agent_type != "unknown" && !agent_type.is_empty() {
            agent_type.to_string()
        } else {
            Self::detect_agent_type(&request)
        };

        tx.execute(
            "INSERT INTO turns (id, session_id, agent_type, timestamp, request_json, response_json)
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                &request.id,
                &session_id,
                &resolved_agent,
                &request.timestamp,
                &request_json,
                &response_json,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// 列出所有会话
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<Session>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.title, s.created_at, s.updated_at,
                    COUNT(t.id) as msg_count,
                    COALESCE(MAX(t.agent_type), 'unknown') as agent,
                    COALESCE(json_extract(MAX(t.request_json), '$.model'), '') as model,
                    COALESCE(SUM(json_extract(t.response_json, '$.usage.total_tokens')), 0) as total_tokens
             FROM sessions s
             LEFT JOIN turns t ON s.id = t.session_id
             GROUP BY s.id
             ORDER BY s.updated_at DESC
             LIMIT ?",
        )?;

        let rows = stmt.query_map([limit], |row| {
            let model: String = row.get(6)?;
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
                agent_type: row.get(5)?,
                project_path: None,
                model: if model.is_empty() { None } else { Some(model) },
                total_tokens: row.get(7)?,
            })
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }

        Ok(sessions)
    }

    /// 搜索会话 (标题模糊匹配)
    pub fn search_sessions(&self, query: &str, limit: usize) -> Result<Vec<Session>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let pattern = format!("%{}%", query.trim());
        let mut stmt = conn.prepare(
            "SELECT s.id, s.title, s.created_at, s.updated_at,
                    COUNT(t.id) as msg_count,
                    COALESCE(MAX(t.agent_type), 'unknown') as agent,
                    COALESCE(json_extract(MAX(t.request_json), '$.model'), '') as model,
                    COALESCE(SUM(json_extract(t.response_json, '$.usage.total_tokens')), 0) as total_tokens
             FROM sessions s
             LEFT JOIN turns t ON s.id = t.session_id
             WHERE s.title LIKE ?
             GROUP BY s.id
             ORDER BY s.updated_at DESC
             LIMIT ?",
        )?;

        let rows = stmt.query_map([&pattern, &limit.to_string()], |row| {
            let model: String = row.get(6)?;
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
                agent_type: row.get(5)?,
                project_path: None,
                model: if model.is_empty() { None } else { Some(model) },
                total_tokens: row.get(7)?,
            })
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }

        Ok(sessions)
    }

    /// 查看单个会话详情
    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionDetail>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;

        let session = conn.query_row(
            "SELECT id, title, project_path, created_at, updated_at FROM sessions WHERE id = ?",
            [session_id],
            |row| {
                Ok(Session {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    project_path: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    message_count: 0,
                    agent_type: String::new(),
                    model: None,
                    total_tokens: 0,
                })
            },
        );

        let session = match session {
            Ok(s) => s,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let mut stmt = conn.prepare(
            "SELECT request_json, response_json, agent_type, timestamp
             FROM turns WHERE session_id = ? ORDER BY timestamp",
        )?;

        let turn_rows = stmt.query_map([session_id], |row| {
            let req_json: String = row.get(0)?;
            let resp_json: String = row.get(1)?;
            let agent_type: String = row.get(2)?;
            let timestamp: String = row.get(3)?;

            let request: LlmRequest = serde_json::from_str(&req_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let response: LlmResponse =
                serde_json::from_str(&resp_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

            Ok(Turn {
                id: request.id.clone(),
                timestamp,
                request,
                response,
                agent_type,
            })
        })?;

        let mut turns = Vec::new();
        for row in turn_rows {
            turns.push(row?);
        }

        Ok(Some(SessionDetail {
            session,
            turns,
            summary: None,
        }))
    }

    /// Extract a stable session ID from the request's cwd metadata.
    /// Public for testing purposes.
    pub fn extract_session_id(request: &LlmRequest) -> String {
        if let Some(cwd) = request.metadata.get("cwd") {
            // 对 cwd 取 SHA-256 前缀，防止不同路径碰撞（如 /a/b-c vs /a/b/c）
            let hash = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                cwd.hash(&mut h);
                h.finish()
            };
            return format!("session-{:016x}", hash);
        }
        format!("session-{}", &request.id[..8.min(request.id.len())])
    }

    /// Detect agent type from the request's user-agent metadata.
    /// Public for testing purposes.
    pub fn detect_agent_type(request: &LlmRequest) -> String {
        if let Some(ua) = request.metadata.get("user-agent") {
            let ua_lower = ua.to_lowercase();
            if ua_lower.contains("opencode") || ua_lower.contains("open-code") {
                return "opencode".to_string();
            }
            if ua_lower.contains("codex") {
                return "codex".to_string();
            }
            if ua_lower.contains("claude") {
                return "claude-code".to_string();
            }
            if ua_lower.contains("trae") {
                return "trae".to_string();
            }
            if ua_lower.contains("kimi") {
                return "kimi-code".to_string();
            }
        }
        "unknown".to_string()
    }

    pub fn data_dir(&self) -> std::path::PathBuf {
        self.data_dir.clone()
    }

    pub fn compute_stats(&self, _data_dir: &Path) -> Result<serde_json::Value> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let total_sessions: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        let total_turns: i64 = conn.query_row("SELECT COUNT(*) FROM turns", [], |r| r.get(0))?;

        let total_tokens: i64 = conn.query_row(
            "SELECT COALESCE(SUM(
                COALESCE(json_extract(response_json, '$.usage.total_tokens'), 0)
            ), 0) FROM turns",
            [], |r| r.get(0)
        )?;

        // Agent 分布
        let mut agent_stmt = conn.prepare("SELECT agent_type, COUNT(*) FROM turns GROUP BY agent_type")?;
        let agents: Vec<(String, i64)> = agent_stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?.filter_map(|r| r.ok()).collect();

        // 模型分布
        let mut model_stmt = conn.prepare("SELECT json_extract(request_json, '$.model') as m, COUNT(*) FROM turns GROUP BY m")?;
        let models: Vec<(String, i64)> = model_stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?.filter_map(|r| r.ok()).collect();

        Ok(serde_json::json!({
            "total_sessions": total_sessions,
            "total_turns": total_turns,
            "total_tokens": total_tokens,
            "agents": agents,
            "models": models,
        }))
    }

    pub fn clear_all(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        conn.execute_batch("DELETE FROM turns; DELETE FROM sessions; DELETE FROM nodes; DELETE FROM edges;")?;
        Ok(())
    }

    /// 返回全部节点（用于预填充的图谱数据）
    pub fn list_all_nodes(&self) -> Result<Vec<GraphNode>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let mut stmt = conn.prepare("SELECT id, label, kind, weight FROM nodes ORDER BY weight DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(GraphNode {
                id: r.get(0)?,
                label: r.get(1)?,
                kind: r.get(2)?,
                weight: r.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// 返回全部边
    pub fn list_all_edges(&self) -> Result<Vec<GraphEdge>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let mut stmt = conn.prepare("SELECT source, target, relation, weight FROM edges ORDER BY weight DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(GraphEdge {
                source: r.get(0)?,
                target: r.get(1)?,
                relation: r.get(2)?,
                weight: r.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(|e| e.into())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: i64,
    pub agent_type: String,
    pub project_path: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub total_tokens: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionDetail {
    pub session: Session,
    pub turns: Vec<Turn>,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Turn {
    pub id: String,
    pub timestamp: String,
    pub request: LlmRequest,
    pub response: LlmResponse,
    pub agent_type: String,
}

/// 图谱节点
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub weight: f64,
}

/// 图谱边
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub weight: f64,
}
