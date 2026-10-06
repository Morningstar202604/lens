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
        /// 监听地址
        #[arg(short, long, default_value = "127.0.0.1:9999")]
        bind: String,

        /// TLS 证书目录
        #[arg(long, default_value = "certs")]
        cert_dir: String,
    },

    /// 启动可视化面板（浏览器打开）
    View {
        /// 监听地址
        #[arg(short, long, default_value = "127.0.0.1:7317")]
        bind: String,
    },

    /// 同时启动代理 + 面板
    Start {
        /// 代理监听地址
        #[arg(long, default_value = "127.0.0.1:9999")]
        proxy_bind: String,

        /// 面板监听地址
        #[arg(long, default_value = "127.0.0.1:7317")]
        view_bind: String,
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

    // 解析数据目录
    let data_dir = lens_lib::engine::resolve_data_dir(cli.data_dir.as_deref())?;
    info!("数据目录: {}", data_dir.display());

    match cli.command {
        Commands::Proxy {
            bind,
            cert_dir,
        } => {
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
            lens_lib::proxy::run(&bind, &cert_dir, &data_dir).await?;
        }

        Commands::View { bind } => {
            info!("🪶 正在启动可视化面板: http://{}", bind);
            lens_lib::web::serve(&bind, &data_dir).await?;
        }

        Commands::Start {
            proxy_bind,
            view_bind,
        } => {
            info!("🪶 Lens 全模式启动");
            let proxy_data = data_dir.clone();
            let proxy_handle = tokio::spawn(async move {
                lens_lib::proxy::run(&proxy_bind, "certs", &proxy_data)
                    .await
                    .expect("代理启动失败");
            });

            // 等代理启动
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            info!("📊 可视化面板: http://{}", view_bind);
            let view_handle = tokio::spawn(async move {
                lens_lib::web::serve(&view_bind, &data_dir)
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
