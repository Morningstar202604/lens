//! SQLite 持久化存储
//!
//! 用 Arc<Mutex<Connection>> + block_in_place 包装 rusqlite 使其可以跨线程使用。

use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::proxy::parser::{LlmRequest, LlmResponse, MessageContent};
use ts_rs::TS;

/// 当前 schema 版本。迁移逻辑见 `migrate`。
const SCHEMA_VERSION: i64 = 1;

/// 会话标题里屏蔽密钥类片段时替换成的占位符。
pub const REDACTED: &str = "[已隐藏]";

/// 脱敏规则。每条以 `(^|[^A-Za-z0-9])` 捕获前导分隔符，
/// 替换时用 `${1}[已隐藏]` 还原该分隔符。
///
/// 注意：这里刻意不用 `\b`。标题多为中文，而中文字符在 Unicode 词边界语义下
/// 也是"词字符"，于是 `密钥是sk-xxx` 这种最常见的形态里 `sk` 前没有边界，
/// 规则会整条失效（真实数据里正是这种形态）。
const SECRET_PATTERNS: &[&str] = &[
    // 常见提供商前缀键：sk-xxx / ghp_ / xoxb- / AKIA…
    // 阈值取 6：标题入库前会被截断到 48 字，密钥常被截成半截（真实数据里
    // 就出现过只剩 7 位后缀的情况），阈值过高会漏掉这些残段。
    r"(?i)(^|[^A-Za-z0-9])((?:sk|pk|rk|ghp|gho|ghu|ghs|github_pat|xoxb|xoxp|akia|asia)[-_][A-Za-z0-9_\-]{6,})",
    // Bearer / token 后的裸值
    r"(?i)(^|[^A-Za-z0-9])((?:bearer|token)\s+[A-Za-z0-9._\-]{12,})",
    // JWT（三段 base64url）
    r"(^|[^A-Za-z0-9])(eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{4,})",
    // key=value / key: value 形式（含引号包裹）
    r#"(?i)(^|[^A-Za-z0-9])((?:api[_-]?key|apikey|secret|token|password|passwd|access[_-]?token|auth[_-]?token)[^A-Za-z0-9]{0,4}[:=][^A-Za-z0-9]{0,4}["']?[A-Za-z0-9._\-/+]{12,}["']?)"#,
];

lazy_static::lazy_static! {
    /// 编译后的脱敏规则。非法正则在此直接 panic（编程错误，不应静默降级为"不脱敏"）。
    static ref COMPILED_SECRET_PATTERNS: Vec<regex::Regex> = SECRET_PATTERNS
        .iter()
        .map(|p| regex::Regex::new(p).unwrap_or_else(|e| panic!("非法脱敏正则 {p:?}: {e}")))
        .collect();
}

/// 单个高熵串的最小长度（无分隔符的连续字母数字串）。
const HIGH_ENTROPY_MIN_LEN: usize = 32;

/// 标题脱敏。会话标题取自首条用户消息，而用户常把密钥直接贴在提示里
/// （例如"用 sk-xxx 这个 key 跑一下"）。标题会显示在面板侧边栏，
/// 不能让凭据原文出现在屏幕、截图或分享里。
pub fn redact_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for re in COMPILED_SECRET_PATTERNS.iter() {
        out = re
            .replace_all(&out, format!("${{1}}{REDACTED}"))
            .into_owned();
    }
    redact_high_entropy_runs(&out)
}

/// 兜底：把"长且字母数字混合"的连续串视为疑似密钥。
///
/// 用代码而非正则实现，因为需要"同时含字母和数字"这类跨字符约束，
/// 而 Rust 正则不支持 lookahead。
fn redact_high_entropy_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.len() >= HIGH_ENTROPY_MIN_LEN
            && run.chars().any(|c| c.is_ascii_alphabetic())
            && run.chars().any(|c| c.is_ascii_digit())
        {
            out.push_str(REDACTED);
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            run.push(ch);
        } else {
            flush(&mut run, &mut out);
            out.push(ch);
        }
    }
    flush(&mut run, &mut out);
    out
}

pub struct Store {
    /// SQLite 连接（线程安全） */
    pub conn: Arc<Mutex<rusqlite::Connection>>,
    data_dir: PathBuf,
}

impl Store {
    /// 会话聚合时间窗（秒）。同一锚点在该窗口内归入同一会话。
    pub const SESSION_WINDOW_SECS: i64 = 1800;
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

        // 初始化 schema（新库直接带 v1 列）
        {
            let conn = store
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
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
                    source TEXT NOT NULL DEFAULT 'proxy',
                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS nodes (
                    id TEXT PRIMARY KEY,
                    label TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    session_id TEXT,
                    weight REAL DEFAULT 1.0,
                    first_seen DATETIME DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS edges (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source TEXT NOT NULL,
                    target TEXT NOT NULL,
                    relation TEXT NOT NULL,
                    session_id TEXT,
                    weight REAL DEFAULT 1.0,
                    UNIQUE(source, target, relation)
                );
                "#,
            )?;
            Self::migrate(&conn, &db_path)?;
            conn.execute_batch(
                r#"
                CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session_id);
                CREATE INDEX IF NOT EXISTS idx_turns_timestamp ON turns(timestamp);
                CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
                CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
                CREATE INDEX IF NOT EXISTS idx_nodes_session ON nodes(session_id);
                CREATE INDEX IF NOT EXISTS idx_edges_session ON edges(session_id);
                "#,
            )?;
        }

        Ok(store)
    }

    /// 旧库就地迁移：补列 + 版本号；迁移前留一份 .bak 备份。
    fn migrate(conn: &rusqlite::Connection, db_path: &Path) -> Result<()> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version >= SCHEMA_VERSION {
            return Ok(());
        }
        if version == 0 && db_path.exists() {
            let backup = db_path.with_extension(format!("db.bak-v{version}"));
            let _ = std::fs::copy(db_path, &backup);
        }
        if !Self::column_exists(conn, "sessions", "source")? {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN source TEXT NOT NULL DEFAULT 'proxy';",
            )?;
        }
        if !Self::column_exists(conn, "nodes", "session_id")? {
            conn.execute_batch("ALTER TABLE nodes ADD COLUMN session_id TEXT;")?;
        }
        if !Self::column_exists(conn, "edges", "session_id")? {
            conn.execute_batch("ALTER TABLE edges ADD COLUMN session_id TEXT;")?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        info!("数据库 schema 迁移至 v{}", SCHEMA_VERSION);
        Ok(())
    }

    fn column_exists(conn: &rusqlite::Connection, table: &str, column: &str) -> Result<bool> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == column {
                return Ok(true);
            }
        }
        Ok(false)
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
        self.record_turn_with_source(agent_type, "proxy", request, response)
    }

    /// 同 `record_turn`，可指定会话来源（proxy / import / …）。
    pub fn record_turn_with_source(
        &self,
        agent_type: &str,
        source: &str,
        request: LlmRequest,
        response: LlmResponse,
    ) -> Result<()> {
        let resolved_agent = if agent_type != "unknown" && !agent_type.is_empty() {
            agent_type.to_string()
        } else {
            Self::detect_agent_type(&request)
        };
        let now = chrono::Utc::now().timestamp();
        let session_id = Self::session_id_at(&resolved_agent, &request, now);
        let title = Self::session_title(&request);

        let mut conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;

        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO sessions (id, title, source, updated_at) VALUES (?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(id) DO UPDATE SET updated_at = CURRENT_TIMESTAMP",
            params![&session_id, &title, source],
        )?;

        let request_json = serde_json::to_string(&request)?;
        let response_json = serde_json::to_string(&response)?;

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

    /// 列出所有会话（分页）
    pub fn list_sessions(&self, limit: usize, offset: usize) -> Result<Vec<Session>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.title, s.created_at, s.updated_at,
                    COUNT(t.id) as msg_count,
                    COALESCE((SELECT t2.agent_type FROM turns t2 WHERE t2.session_id = s.id
                              ORDER BY t2.timestamp DESC LIMIT 1), 'unknown') as agent,
                    (SELECT json_extract(t3.request_json, '$.model') FROM turns t3
                     WHERE t3.session_id = s.id ORDER BY t3.timestamp DESC LIMIT 1) as model,
                    COALESCE(SUM(json_extract(t.response_json, '$.usage.total_tokens')), 0) as total_tokens
             FROM sessions s
             LEFT JOIN turns t ON s.id = t.session_id
             GROUP BY s.id
             ORDER BY s.updated_at DESC
             LIMIT ? OFFSET ?",
        )?;

        let rows = stmt.query_map([limit, offset], |row| {
            let model: Option<String> = row.get(6)?;
            Ok(Session {
                id: row.get(0)?,
                title: redact_secrets(&row.get::<_, String>(1)?),
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
                agent_type: row.get(5)?,
                project_path: None,
                model,
                total_tokens: row.get(7)?,
            })
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }

        Ok(sessions)
    }

    /// 搜索会话 (标题模糊匹配;分页;LIKE 通配符已转义)
    pub fn search_sessions(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Session>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let raw = query.trim();
        let escaped = raw
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        let mut stmt = conn.prepare(
            "SELECT s.id, s.title, s.created_at, s.updated_at,
                    COUNT(t.id) as msg_count,
                    COALESCE((SELECT t2.agent_type FROM turns t2 WHERE t2.session_id = s.id
                              ORDER BY t2.timestamp DESC LIMIT 1), 'unknown') as agent,
                    (SELECT json_extract(t3.request_json, '$.model') FROM turns t3
                     WHERE t3.session_id = s.id ORDER BY t3.timestamp DESC LIMIT 1) as model,
                    COALESCE(SUM(json_extract(t.response_json, '$.usage.total_tokens')), 0) as total_tokens
             FROM sessions s
             LEFT JOIN turns t ON s.id = t.session_id
             WHERE s.title LIKE ? ESCAPE '\\'
             GROUP BY s.id
             ORDER BY s.updated_at DESC
             LIMIT ? OFFSET ?",
        )?;

        let rows = stmt.query_map(rusqlite::params![&pattern, &limit, &offset], |row| {
            let model: Option<String> = row.get(6)?;
            Ok(Session {
                id: row.get(0)?,
                title: redact_secrets(&row.get::<_, String>(1)?),
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
                agent_type: row.get(5)?,
                project_path: None,
                model,
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;

        let session = conn.query_row(
            "SELECT id, title, project_path, created_at, updated_at FROM sessions WHERE id = ?",
            [session_id],
            |row| {
                Ok(Session {
                    id: row.get(0)?,
                    title: redact_secrets(&row.get::<_, String>(1)?),
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
             FROM turns WHERE session_id = ? ORDER BY timestamp LIMIT 2000",
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
            let response: LlmResponse = serde_json::from_str(&resp_json).map_err(|e| {
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

    /// 会话锚点：原生会话 id > 工作目录 > 仅 agent。
    pub fn session_anchor(request: &LlmRequest) -> String {
        const NATIVE_KEYS: [&str; 4] = [
            "opencode.session_id",
            "claude.session_id",
            "codex.session_id",
            "session.native_id",
        ];
        for key in NATIVE_KEYS {
            if let Some(v) = request.metadata.get(key) {
                if !v.trim().is_empty() {
                    return format!("native:{}", v.trim());
                }
            }
        }
        const CWD_KEYS: [&str; 4] = ["cwd", "opencode.workspace", "claude.cwd", "codex.cwd"];
        for key in CWD_KEYS {
            if let Some(v) = request.metadata.get(key) {
                let normalized = v.trim().replace('\\', "/").to_lowercase();
                let normalized = normalized.trim_end_matches('/');
                if !normalized.is_empty() {
                    return format!("cwd:{normalized}");
                }
            }
        }
        "agent".to_string()
    }

    /// 计算会话 id。
    ///
    /// - 原生会话 id 锚点：稳定，不随窗口滚动；
    /// - cwd / agent 锚点：按 `SESSION_WINDOW_SECS` 分桶，同一分桶内归入同一会话。
    ///   分桶使多进程（proxy/view/CLI）计算结果一致，无需共享状态。
    pub fn session_id_at(agent: &str, request: &LlmRequest, now: i64) -> String {
        let anchor = Self::session_anchor(request);
        let key = format!("{agent}\u{1}{anchor}");
        let hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            key.hash(&mut h);
            h.finish()
        };
        if anchor.starts_with("native:") {
            return format!("session-{hash:016x}");
        }
        let bucket = now.div_euclid(Self::SESSION_WINDOW_SECS);
        format!("session-{hash:016x}-{bucket}")
    }

    /// 会话标题：首条用户消息摘要，无则回落到模型名。
    fn session_title(request: &LlmRequest) -> String {
        for m in &request.messages {
            if m.role != "user" {
                continue;
            }
            let text = match &m.content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| b.text.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
            };
            let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !one_line.is_empty() {
                // 先脱敏再截断：反过来的话密钥会被截成半截残段，
                // 前缀规则匹配不到，残段就留在标题里了（真实数据踩过）。
                let safe = redact_secrets(&one_line);
                return safe.chars().take(48).collect();
            }
        }
        request
            .model
            .clone()
            .unwrap_or_else(|| "未命名会话".to_string())
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

    pub fn compute_stats(&self, _data_dir: &Path) -> Result<Stats> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let total_sessions: i64 =
            conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        let total_turns: i64 = conn.query_row("SELECT COUNT(*) FROM turns", [], |r| r.get(0))?;

        let total_tokens: i64 = conn.query_row(
            "SELECT COALESCE(SUM(
                COALESCE(json_extract(response_json, '$.usage.total_tokens'), 0)
            ), 0) FROM turns",
            [],
            |r| r.get(0),
        )?;

        let mut agent_stmt = conn.prepare(
            "SELECT agent_type, COUNT(*) FROM turns GROUP BY agent_type ORDER BY 2 DESC",
        )?;
        let agents: Vec<CountStat> = agent_stmt
            .query_map([], |r| {
                Ok(CountStat {
                    name: r.get(0)?,
                    count: r.get(1)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut model_stmt = conn.prepare(
            "SELECT COALESCE(json_extract(request_json, '$.model'), 'unknown') as m, COUNT(*)
             FROM turns GROUP BY m ORDER BY 2 DESC",
        )?;
        let models: Vec<CountStat> = model_stmt
            .query_map([], |r| {
                Ok(CountStat {
                    name: r.get(0)?,
                    count: r.get(1)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        // 按天的真实活动量。timestamp 形如 "2026-10-06T18:00:00" 或带 Z，
        // 取前 10 字符即日期；非法值被 LIKE 过滤掉。
        let mut daily_stmt = conn.prepare(
            "SELECT substr(timestamp, 1, 10) AS d,
                    COUNT(*) AS turns,
                    COALESCE(SUM(COALESCE(json_extract(response_json, '$.usage.total_tokens'), 0)), 0) AS tokens
             FROM turns
             WHERE timestamp LIKE '____-__-__%'
             GROUP BY d
             ORDER BY d DESC
             LIMIT 90",
        )?;
        let mut daily: Vec<DailyStat> = daily_stmt
            .query_map([], |r| {
                Ok(DailyStat {
                    date: r.get(0)?,
                    turns: r.get(1)?,
                    tokens: r.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        daily.reverse();

        Ok(Stats {
            total_sessions,
            total_turns,
            total_tokens,
            agents,
            models,
            daily,
        })
    }

    pub fn clear_all(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        conn.execute_batch(
            "DELETE FROM turns; DELETE FROM sessions; DELETE FROM nodes; DELETE FROM edges;",
        )?;
        Ok(())
    }

    /// 返回全部节点（用于图谱视图；按权重截断，防无界拉取）
    pub fn list_all_nodes(&self) -> Result<Vec<GraphNode>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let mut stmt =
            conn.prepare("SELECT id, label, kind, weight FROM nodes ORDER BY weight DESC LIMIT ?")?;
        let rows = stmt.query_map([super::graph::GLOBAL_GRAPH_NODE_LIMIT], |r| {
            Ok(GraphNode {
                id: r.get(0)?,
                label: r.get(1)?,
                kind: r.get(2)?,
                weight: r.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.into())
    }

    /// 返回全部边（按权重截断）
    pub fn list_all_edges(&self) -> Result<Vec<GraphEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT source, target, relation, weight FROM edges ORDER BY weight DESC LIMIT ?",
        )?;
        let rows = stmt.query_map([super::graph::GLOBAL_GRAPH_EDGE_LIMIT], |r| {
            Ok(GraphEdge {
                source: r.get(0)?,
                target: r.get(1)?,
                relation: r.get(2)?,
                weight: r.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.into())
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[ts(type = "number")]
    pub message_count: i64,
    pub agent_type: String,
    pub project_path: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    #[ts(type = "number")]
    pub total_tokens: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct SessionDetail {
    pub session: Session,
    pub turns: Vec<Turn>,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct Turn {
    pub id: String,
    pub timestamp: String,
    pub request: LlmRequest,
    pub response: LlmResponse,
    pub agent_type: String,
}

/// 图谱节点
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub weight: f64,
}

/// 图谱边
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub weight: f64,
}

/// 统计计数项（HTTP API 契约）。
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct CountStat {
    pub name: String,
    #[ts(type = "number")]
    pub count: i64,
}

/// 统计面板数据（HTTP API 契约）。
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct Stats {
    #[ts(type = "number")]
    pub total_sessions: i64,
    #[ts(type = "number")]
    pub total_turns: i64,
    #[ts(type = "number")]
    pub total_tokens: i64,
    pub agents: Vec<CountStat>,
    pub models: Vec<CountStat>,
    /// 按天的真实活动量（用于趋势图；此前界面上的"趋势"是拿会话列表拼的假图）。
    pub daily: Vec<DailyStat>,
}

/// 单日活动量。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/generated/")]
pub struct DailyStat {
    /// `YYYY-MM-DD`（UTC）。
    pub date: String,
    #[ts(type = "number")]
    pub turns: i64,
    #[ts(type = "number")]
    pub tokens: i64,
}
