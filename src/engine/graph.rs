//! 关系图谱生成（代码/概念/决策依赖网络）
//!
//! 从对话轮次中提取实体（文件、函数、工具、话题），
//! 按共现关系构建依赖网络，写入 SQLite。

use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

use super::store::{GraphNode, GraphEdge, Store};

lazy_static::lazy_static! {
    /// 匹配文件路径 */
    static ref RE_FILE: Regex = Regex::new(r"`?([/\w.-]+\.(?:ts|tsx|js|jsx|py|sql|css|html|json|toml|rs|go|md))`?").unwrap();
    /// 匹配函数/方法名 */
    static ref RE_FUNC: Regex = Regex::new(r"(?:function\s+|def\s+|fn\s+|const\s+|let\s+|export\s+(?:async\s+)?(?:function|default)\s+)(\w+)").unwrap();
    /// 匹配代码块中的 import/require */
    static ref RE_IMPORT: Regex = Regex::new(r#"(?:import|require)\s*\(?['"]([^'"]+)['"]\)?"#).unwrap();
}

/// 提取单个轮次的文本内容（用于实体抽取）
fn extract_text_from_turn(request_json: &str, response_json: &str) -> String {
    let mut text = String::new();
    if let Ok(req) = serde_json::from_str::<serde_json::Value>(request_json) {
        if let Some(msgs) = req["messages"].as_array() {
            for m in msgs {
                match m["content"].as_str() {
                    Some(t) => { text.push_str(t); text.push(' '); }
                    None => {
                        if let Some(blocks) = m["content"].as_array() {
                            for b in blocks {
                                if let Some(t) = b["text"].as_str() {
                                    text.push_str(t);
                                    text.push(' ');
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let Ok(resp) = serde_json::from_str::<serde_json::Value>(response_json) {
        if let Some(content) = resp["content"].as_array() {
            for b in content {
                if let Some(t) = b["text"].as_str() {
                    text.push_str(t);
                    text.push(' ');
                }
            }        }
    }
    text
}

/// 从单个话题关键词
fn extract_topics(text: &str) -> Vec<&'static str> {
    let text_lower = text.to_lowercase();
    let all_topics = [
        ("refactor", "重构"),
        ("bug", "缺陷修复"),
        ("test", "测试"),
        ("deploy", "部署"),
        ("performance", "性能优化"),
        ("security", "安全检查"),
        ("documentation", "文档"),
        ("auth", "认证"),
        ("api", "API"),
        ("frontend", "前端"),
        ("backend", "后端"),
        ("database", "数据库"),
        ("css", "样式"),
        ("component", "组件"),
        ("middleware", "中间件"),
    ];
    all_topics
        .iter()
        .filter(|(en, _)| text_lower.contains(*en))
        .map(|(_, zh)| *zh)
        .collect()
}

/// 分析会话构建关系图谱
///
/// 从该 session 的全部 turns 中抽取实体并写入 nodes / edges 表，
/// 返回 (nodes, edges) 供前端渲染。
pub fn build_graph(data_dir: &Path, session_id: Option<&str>) -> Result<(Vec<GraphNode>, Vec<GraphEdge>)> {
    let store = Store::open(data_dir)?;
    let conn = store.conn.lock().map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;

    // 1. 取该 session 的 turns（或全部）
    let turn_rows: Vec<(String, String)> = if let Some(sid) = session_id {
        let mut stmt = conn.prepare("SELECT request_json, response_json FROM turns WHERE session_id = ? ORDER BY timestamp")?;
        let rows = stmt.query_map([sid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    } else {
        let mut stmt = conn.prepare("SELECT request_json, response_json FROM turns ORDER BY timestamp")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    if turn_rows.is_empty() {
        // 没有 turns → 返回空
        let empty_nodes: Vec<GraphNode> = Vec::new();
        let empty_edges: Vec<GraphEdge> = Vec::new();
        return Ok((empty_nodes, empty_edges));
    }

    // 2. 逐轮提取实体，统计权重和共现
    let mut node_weights: HashMap<String, (String, f64)> = HashMap::new(); // id -> (kind, weight)
    let mut cooccurrence: HashMap<(String, String), f64> = HashMap::new();

    for (req_json, resp_json) in &turn_rows {
        let text = extract_text_from_turn(req_json, resp_json);
        let mut turn_entities: Vec<String> = Vec::new();

        // 文件路径
        for cap in RE_FILE.captures_iter(&text) {
            let path = cap[1].trim_start_matches('`').trim_end_matches('`').to_string();
            if path.len() > 2 && !path.starts_with("http") {
                let id = format!("file:{}", path);
                let entry = node_weights.entry(id.clone()).or_insert_with(|| ("file".to_string(), 0.0));
                entry.1 += 1.0;
                turn_entities.push(id);
            }
        }
        // 函数名
        for cap in RE_FUNC.captures_iter(&text) {
            let name = cap[1].to_string();
            if name.len() > 2 && !["function", "def", "const", "let", "return"].contains(&name.as_str()) {
                let id = format!("fn:{}", name);
                let entry = node_weights.entry(id.clone()).or_insert_with(|| ("function".to_string(), 0.0));
                entry.1 += 0.5;  // 函数比文件权重低
                turn_entities.push(id);
            }
        }
        // 话题
        for topic in extract_topics(&text) {
            let id = format!("topic:{}", topic);
            let entry = node_weights.entry(id.clone()).or_insert_with(|| ("topic".to_string(), 0.0));
            entry.1 += 0.8;
            turn_entities.push(id);
        }

        // 3. 统计共现（本 turn 内任意两个实体构成一条 co-occurrence 边）
        turn_entities.sort();
        turn_entities.dedup();  // 去重
        for i in 0..turn_entities.len() {
            for j in (i + 1)..turn_entities.len() {
                let pair = (turn_entities[i].clone(), turn_entities[j].clone());
                *cooccurrence.entry(pair).or_insert(0.0) += 1.0;
            }
        }
    }

    // 4. 写入数据库
    // 先清除该 session 缓存的图谱（避免陈旧数据）
    let file_prefix = "file:%";
    conn.execute("DELETE FROM nodes WHERE id LIKE ?", [file_prefix])?;
    conn.execute("DELETE FROM edges WHERE source LIKE ?", [file_prefix])?;

    // 插入 nodes
    let mut nodes: Vec<GraphNode> = Vec::new();
    for (id, (kind, weight)) in &node_weights {
        let label = id.split_once(':').map(|x| x.1).unwrap_or(id).to_string();
        conn.execute(
            "INSERT OR REPLACE INTO nodes (id, label, kind, weight) VALUES (?, ?, ?, ?)",
            [id.as_str(), label.as_str(), kind.as_str(), &weight.to_string()],
        )?;
        nodes.push(GraphNode {
            id: id.clone(),
            label,
            kind: kind.clone(),
            weight: *weight,
        });
    }

    // 插入 edges
    let mut edges: Vec<GraphEdge> = Vec::new();
    for ((src, tgt), weight) in &cooccurrence {
        let relation = determine_relation(src, tgt);
        conn.execute(
            "INSERT OR IGNORE INTO edges (source, target, relation, weight) VALUES (?, ?, ?, ?)",
            [src.as_str(), tgt.as_str(), relation.as_str(), &weight.to_string()],
        )?;
        edges.push(GraphEdge {
            source: src.clone(),
            target: tgt.clone(),
            relation,
            weight: *weight,
        });
    }

    // 排序：按权重降序，取前 50 个节点
    nodes.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    nodes.truncate(50);
    edges.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    edges.truncate(200);

    Ok((nodes, edges))
}

/// 根据节点 id 推断边的类型
fn determine_relation(src: &str, tgt: &str) -> String {
    match (src.split(':').next().unwrap_or(""), tgt.split(':').next().unwrap_or("")) {
        ("file", "function") | ("function", "file") => "contains".to_string(),
        ("file", "file") | ("function", "function") => "references".to_string(),
        ("topic", _) | (_, "topic") => "related_to".to_string(),
        _ => "co_occurs".to_string(),
    }
}
