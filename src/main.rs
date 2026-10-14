//! Lens — AI 编程代理透镜
//!
//! 拦截你的 AI coding agent 与 LLM 之间的所有通信，
//! 将零散的对话流变成可导航、可回溯的可视化全景。

use clap::{Parser, Subcommand};
use tracing::info;

/// Lens — 你的 AI 编程代理透镜
/// CLI 入口，所有逻辑由 lens_lib 提供。
#[derive(Parser)]
#[command(name = "lens", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// 配置文件路径
    #[arg(short, long, global = true)]
    config: Option<String>,

    /// 数据目录
    #[arg(short, long, global = true)]
    data_dir: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// 启动透明代理，拦截并记录 LLM 通信
    Proxy {
        /// 监听地址（优先级：本参数 > 配置文件 > 127.0.0.1:9999）
        #[arg(short, long)]
        bind: Option<String>,

        /// TLS 证书目录（优先级：本参数 > 配置文件 > certs）
        #[arg(long)]
        cert_dir: Option<String>,

        /// 上游 TLS 额外信任的 CA 证书（PEM），用于企业自签网关
        #[arg(long, value_name = "PATH")]
        upstream_ca: Option<String>,

        /// 跳过上游 TLS 证书校验（仅调试用途）
        #[arg(long)]
        insecure_upstream: bool,
    },

    /// 启动可视化面板（浏览器打开）
    View {
        /// 监听地址（优先级：本参数 > 配置文件 > 127.0.0.1:7317）
        #[arg(short, long)]
        bind: Option<String>,
    },

    /// 同时启动代理 + 面板
    Start {
        /// 代理监听地址（优先级：本参数 > 配置文件 > 127.0.0.1:9999）
        #[arg(long)]
        proxy_bind: Option<String>,

        /// 面板监听地址（优先级：本参数 > 配置文件 > 127.0.0.1:7317）
        #[arg(long)]
        view_bind: Option<String>,

        /// 上游 TLS 额外信任的 CA 证书（PEM），用于企业自签网关
        #[arg(long, value_name = "PATH")]
        upstream_ca: Option<String>,

        /// 跳过上游 TLS 证书校验（仅调试用途）
        #[arg(long)]
        insecure_upstream: bool,
    },

    /// 列出所有记录的会话
    List {
        /// 只显示最近的 N 个会话
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },

    /// 查看单个会话详情
    Show {
        /// 会话 ID
        session_id: String,
    },

    /// 导入会话日志（claude: JSONL；opencode: 本地 SQLite 库）
    Import {
        /// 来源（claude | opencode）
        #[arg(long, default_value = "claude")]
        source: String,

        /// 文件/目录/数据库路径（claude 默认 ~/.claude/projects；opencode 默认 ~/.local/share/opencode/opencode.db）
        #[arg(long)]
        path: Option<String>,

        /// opencode: 只导入最近 N 个会话（0 = 全部）
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },

    /// 初始化配置和数据目录
    Init,

    /// 清除所有记录的数据
    Clear {
        /// 不询问确认
        #[arg(long)]
        yes: bool,
    },
}

#[::tokio::main]
async fn main() -> ::anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lens=info".into()),
        )
        .init();

    let cli = Cli::parse();

    let config = lens_lib::config::Config::load(cli.config.as_deref())?;
    let data_dir = match cli.data_dir.as_deref() {
        Some(dir) => std::path::PathBuf::from(dir),
        None => match &config.storage.data_dir {
            Some(dir) => dir.clone(),
            None => lens_lib::engine::resolve_data_dir(None)?,
        },
    };
    info!("数据目录: {}", data_dir.display());

    match cli.command {
        Commands::Proxy {
            bind,
            cert_dir,
            upstream_ca,
            insecure_upstream,
        } => {
            let bind = config.effective_proxy_bind(bind.as_deref());
            let cert_dir = config.effective_cert_dir(cert_dir.as_deref());
            info!("🪶 Lens 代理正在启动...");
            info!("  监听地址: {}", bind);
            info!("  HTTPS 拦截: 开启");
            println!();
            println!("  💡 在你的 agent 环境中设置：");
            println!("     export HTTP_PROXY=http://{}", bind);
            println!("     export HTTPS_PROXY=http://{}", bind);
            println!();
            println!("  💡 然后照常使用你的 agent，用法不变。");
            println!("     运行 `lens view` 打开可视化面板。");
            println!();
            let options = lens_lib::proxy::ProxyOptions {
                upstream_tls: config
                    .effective_upstream_tls(upstream_ca.as_deref(), insecure_upstream),
                agents: config.agent_filter(),
                writer: None,
            };
            lens_lib::proxy::run_with(&bind, &cert_dir, &data_dir, options).await?;
        }

        Commands::View { bind } => {
            let bind = config.effective_view_bind(bind.as_deref());
            info!("🪶 正在启动可视化面板: http://{}", bind);
            lens_lib::web::serve(&bind, &data_dir, config.view.open_browser).await?;
        }

        Commands::Start {
            proxy_bind,
            view_bind,
            upstream_ca,
            insecure_upstream,
        } => {
            info!("🪶 Lens 全模式启动");
            let proxy_bind = config.effective_proxy_bind(proxy_bind.as_deref());
            let view_bind = config.effective_view_bind(view_bind.as_deref());
            let cert_dir = config.effective_cert_dir(None);
            let open_browser = config.view.open_browser;
            let options = lens_lib::proxy::ProxyOptions {
                upstream_tls: config
                    .effective_upstream_tls(upstream_ca.as_deref(), insecure_upstream),
                agents: config.agent_filter(),
                writer: None,
            };
            let proxy_data = data_dir.clone();
            let proxy_handle = tokio::spawn(async move {
                lens_lib::proxy::run_with(&proxy_bind, &cert_dir, &proxy_data, options)
                    .await
                    .expect("代理启动失败");
            });

            // 等代理启动
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            info!("📊 可视化面板: http://{}", view_bind);
            let view_handle = tokio::spawn(async move {
                lens_lib::web::serve(&view_bind, &data_dir, open_browser)
                    .await
                    .expect("面板启动失败");
            });

            tokio::select! {
                _ = proxy_handle => {},
                _ = view_handle => {},
                _ = tokio::signal::ctrl_c() => {
                    info!("收到退出信号，正在关闭...");
                }
            }
        }

        Commands::List { limit } => {
            lens_lib::engine::list_sessions(&data_dir, limit)?;
        }

        Commands::Show { session_id } => {
            lens_lib::engine::show_session(&data_dir, &session_id)?;
        }

        Commands::Import {
            source,
            path,
            limit,
        } => {
            let stats = match source.as_str() {
                "claude" => {
                    let path = match path {
                        Some(p) => std::path::PathBuf::from(p),
                        None => {
                            lens_lib::import::default_claude_projects_dir().ok_or_else(|| {
                                anyhow::anyhow!(
                                    "无法定位默认导入目录（~/.claude/projects），请用 --path 指定"
                                )
                            })?
                        }
                    };
                    lens_lib::import::import_claude(&data_dir, &path)?
                }
                "opencode" => {
                    let db = match path {
                        Some(p) => std::path::PathBuf::from(p),
                        None => lens_lib::import::default_opencode_db().ok_or_else(|| {
                            anyhow::anyhow!(
                                "无法定位默认 OpenCode 库（~/.local/share/opencode/opencode.db），请用 --path 指定"
                            )
                        })?,
                    };
                    lens_lib::import::import_opencode(&data_dir, &db, limit)?
                }
                other => anyhow::bail!("未知来源: {other}（当前支持: claude | opencode）"),
            };
            println!(
                "  ✅ 导入完成: {} 个文件/库, {} 个新 turn, {} 个已存在跳过",
                stats.files, stats.turns_imported, stats.turns_skipped
            );
        }

        Commands::Init => {
            lens_lib::engine::init_data_dir(&data_dir)?;
            println!("  ✅ Lens 已就绪，数据目录：{}", data_dir.display());
            println!();
            println!("  下一步：运行 `lens proxy` 开始拦截");
        }

        Commands::Clear { yes } => {
            if !yes {
                println!("⚠️  这将清除所有记录的会话数据。");
                let confirm = dialoguer::Confirm::new()
                    .with_prompt("确认清除？")
                    .default(false)
                    .interact()?;
                if !confirm {
                    println!("已取消。");
                    return Ok(());
                }
            }
            lens_lib::engine::clear_data(&data_dir)?;
            println!("  ✅ 数据已清除");
        }
    }

    Ok(())
}
