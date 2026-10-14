//! 会话管理 CLI 交互

use anyhow::Result;
use console::style;
use std::path::Path;

use super::Store;

/// 解析数据目录
pub fn resolve_data_dir(cli_override: Option<&str>) -> Result<std::path::PathBuf> {
    if let Some(dir) = cli_override {
        return Ok(Path::new(dir).to_path_buf());
    }

    // 使用 directories crate 找到标准数据目录
    if let Some(proj_dirs) = directories::ProjectDirs::from("", "lens", "lens") {
        return Ok(proj_dirs.data_dir().to_path_buf());
    }

    // fallback: ~/.lens
    if let Some(home) = homedir::my_home().ok().flatten() {
        return Ok(home.join(".lens"));
    }

    anyhow::bail!("无法确定数据目录，请使用 --data-dir 指定")
}

/// 清除所有数据
pub fn clear_data(data_dir: &Path) -> Result<()> {
    // 数据库本体与 WAL/SHM 伴随文件一并删除
    let db_path = data_dir.join("lens.db");
    for suffix in ["", "-wal", "-shm"] {
        let path = data_dir.join(format!("lens.db{suffix}"));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    let _ = db_path;
    Ok(())
}

/// 初始化数据目录
pub fn init_data_dir(data_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    // 触发数据库初始化
    let _store = Store::open(data_dir)?;
    Ok(())
}

/// 列出所有会话（CLI 交互）
pub fn list_sessions(data_dir: &Path, limit: usize) -> Result<()> {
    let store = Store::open(data_dir)?;
    let sessions = store.list_sessions(limit, 0)?;

    if sessions.is_empty() {
        println!(
            "{}",
            style("尚无记录的会话。先运行 `lens proxy` 开始拦截。").dim()
        );
        return Ok(());
    }

    println!(
        "{}",
        style(format!("  共 {} 个会话  ", sessions.len()))
            .bold()
            .cyan()
            .on_black()
    );
    println!();

    for (i, session) in sessions.iter().enumerate() {
        let num = style(format!("{:>2}", i + 1)).dim();
        let agent = match session.agent_type.as_str() {
            "opencode" => style(String::from("opencode")).green(),
            "codex" => style(String::from("codex")).magenta(),
            "claude-code" => style(String::from("claude-code")).blue(),
            "kimi-code" => style(String::from("kimi-code")).yellow(),
            _ => style(session.agent_type.clone()).dim(),
        };
        let msgs = style(format!("{} 条消息", session.message_count)).dim();

        println!(
            "  {} {}  {} {}",
            num,
            agent,
            style(&session.title).bold(),
            msgs
        );
    }

    println!();
    println!(
        "{}",
        style("  使用 `lens show <session-id>` 查看详情  ").dim()
    );

    Ok(())
}

/// 查看单个会话详情
pub fn show_session(data_dir: &Path, session_id: &str) -> Result<()> {
    let store = Store::open(data_dir)?;
    let detail = store.get_session(session_id)?;

    let detail = match detail {
        Some(d) => d,
        None => {
            println!("{}", style("未找到该会话。").red());
            return Ok(());
        }
    };

    println!();
    println!(
        "{} {} {}",
        style("▓▓▓").cyan(),
        style(&detail.session.title).bold().cyan(),
        style("▓▓▓").cyan()
    );
    println!();

    for turn in &detail.turns {
        let timestamp = turn
            .timestamp
            .get(11..19)
            .unwrap_or(turn.timestamp.as_str());
        let role_label = match turn.request.messages.last().map(|m| m.role.as_str()) {
            Some("user") => style("你").green().bold(),
            Some("assistant") => style("AI").blue().bold(),
            Some("system") => style("SYS").yellow().bold(),
            _ => style("？").dim(),
        };

        println!("  {} {}  ", style(timestamp).dim(), role_label);

        // 提取最后一条用户消息
        if let Some(user_msg) = turn.request.messages.last() {
            let text = match &user_msg.content {
                crate::proxy::parser::MessageContent::Text(t) => t.clone(),
                crate::proxy::parser::MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| b.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("\n"),
            };

            // 截断过长的消息
            let preview: String = text.chars().take(200).collect();
            println!("    {}", style(preview).dim());
            if text.len() > 200 {
                println!("    {}", style("...").dim());
            }
        }

        // 显示使用的工具 & 模型
        let model_info = turn
            .request
            .model
            .as_ref()
            .map(|m| format!(" [{m}]"))
            .unwrap_or_default();
        if !model_info.is_empty() {
            println!("    {}", style(model_info).dim());
        }

        println!();
    }

    Ok(())
}
