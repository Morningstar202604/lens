//! 会话摘要引擎
//!
//! 两阶段实现：v1 是纯本地规则提取（关键词/文件提及/工具调用统计）；v2 后续接 LLM 做自然语言总结。

use anyhow::Result;
use std::collections::HashMap;
use crate::engine::store::Store;

pub fn summarize(data_dir: &std::path::Path, session_id: &str) -> Result<String> {
    let store = Store::open(data_dir)?;
    let detail = store.get_session(session_id)?
        .ok_or_else(|| anyhow::anyhow!("会话不存在"))?;

    let turns = &detail.turns;
    if turns.is_empty() {
        return Ok("这个会话还没有记录任何对话".into());
    }

    // 收集所有用户文本
    let mut user_texts: Vec<String> = Vec::new();
    let mut assistant_texts: Vec<String> = Vec::new();
    let mut tool_counts: HashMap<String, usize> = HashMap::new();
    let mut mentioned_files: Vec<String> = Vec::new();
    let mut mentioned_topics: Vec<String> = Vec::new();

    for turn in turns {
        let req = &turn.request;
        let resp = &turn.response;

        // 累加 user messages
        for msg in &req.messages {
            if msg.role == "user" {
                user_texts.push(flatten_content(&msg.content));
            }
        }
        // 累加 assistant text
        for block in &resp.content {
            if block.block_type == "text" {
                if let Some(ref t) = block.text {
                    assistant_texts.push(t.clone());
                }
            }
        }
        // 工具调用
        if let Some(ref calls) = resp.tool_calls {
            for c in calls {
                let name = c.function.name.clone();
                *tool_counts.entry(name).or_default() += 1;
            }
        }
    }

    // 提取文件路径提及（模式匹配）
    let file_patterns: Vec<regex_lite::Regex> = P_PATH.iter()
        .filter_map(|p| regex_lite::Regex::new(p).ok())
        .collect();

    let all_text = format!("{} {}", user_texts.join(" "), assistant_texts.join(" "));
    for line in all_text.lines() {
        for pat in &file_patterns {
                for m in pat.find_iter(line) {
                    let s = m.as_str().to_string();
                    if !mentioned_files.contains(&s) && mentioned_files.len() < 8 {
                        mentioned_files.push(s);
                    }
                }
        }
    }

    // 检测常见编程话题
    for (keyword, topic) in P_TOPICS {
        if all_text.to_lowercase().contains(keyword)
            && !mentioned_topics.contains(&topic.to_string()) {
                mentioned_topics.push(topic.to_string());
            }
    }

    // 构建摘要
    let mut parts: Vec<String> = Vec::new();

    // 1. 轮次 + 模型 + 工具
    let models: Vec<String> = turns.iter()
        .filter_map(|t| t.request.model.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter().collect();
    parts.push(format!("共 {} 轮对话，使用模型: {}", turns.len(), models.join(", ")));

    // 2. 使用工具
    if !tool_counts.is_empty() {
        let mut tools_sorted: Vec<_> = tool_counts.iter().collect();
        tools_sorted.sort_by(|a, b| b.1.cmp(a.1));
        let tool_summary: Vec<String> = tools_sorted.iter()
            .map(|(name, count)| format!("{}({}次)", name, count))
            .collect();
        parts.push(format!("调用工具: {}", tool_summary.join("、")));
    }

    // 3. 涉及的文件
    if !mentioned_files.is_empty() {
        parts.push(format!("涉及文件: {}", mentioned_files.join("、 ")));
    }

    // 4. 话题
    if !mentioned_topics.is_empty() {
        parts.push(format!("涉及话题: {}", mentioned_topics.join("、")));
    }

    // 5. 最后一条 assistant 消息作为总结预览
    if let Some(last_text) = assistant_texts.last() {
        let preview: String = last_text.chars().take(120).collect();
        if !preview.trim().is_empty() {
            parts.push(format!("最近回复摘要: {}", preview));
        }
    }

    Ok(parts.join("\n"))
}

fn flatten_content(content: &crate::proxy::parser::MessageContent) -> String {
    match content {
        crate::proxy::parser::MessageContent::Text(s) => s.clone(),
        crate::proxy::parser::MessageContent::Blocks(blocks) => {
            blocks.iter().filter_map(|b| b.text.clone()).collect::<Vec<_>>().join(" ")
        }
    }
}

static P_PATH: &[&str] = &[
    r"(?:^|[\s`])/[a-zA-Z0-9_./-]+\.[a-zA-Z]{1,6}(?:[\s`:]|$)",
    r"(?:^|[\s`])(?:src|lib|test|tests|pkg|cmd)/[a-zA-Z0-9_./-]+",
    r"(?:^|[\s`])\./[a-zA-Z0-9_./-]+",
];

static P_TOPICS: &[(&str, &str)] = &[
    ("refactor", "代码重构"),
    ("bug", "Bug 修复"),
    ("test", "测试相关"),
    ("文档", "文档编写"),
    ("readme", "README 完善"),
    ("error", "错误排查"),
    ("implement", "功能实现"),
    ("add", "添加功能"),
    ("delete", "删除代码"),
    ("update", "更新修改"),
    ("fix", "修复"),
    ("performance", "性能优化"),
    ("security", "安全相关"),
    ("api", "API 相关"),
    ("database", "数据库相关"),
    ("配置", "配置相关"),
    ("deploy", "部署相关"),
    ("docker", "Docker 相关"),
    ("git", "Git 相关"),
];
