# Lens · 透镜

[English](#english) | **中文**

**把 AI 编程代理的对话变成可回溯的本地资产。** 两条数据源,同构入库:

1. **代理录制** — 本地透明 HTTP/HTTPS 代理,拦截 OpenCode / Codex / Claude Code 与 LLM API 的通信(含 SSE 流式),实时入库;
2. **日志导入** — 直接读取 Claude Code 的本地会话日志(`~/.claude/projects/**/*.jsonl`),零配置、零信任安装。

入库后提供:会话时间线、Token/模型统计、按会话隔离的知识图谱(文件/函数/话题),以及 Tauri 桌面壳(实验性)。

## 快速开始

```bash
# 构建(仅需 Rust;Windows 上构建桌面壳另需 Windows SDK)
cargo build --release

# 方式 A(零摩擦):导入既有 Claude Code 日志
./target/release/lens import                     # 默认 ~/.claude/projects
./target/release/lens import --path /path/to/logs

# 方式 B(实时录制):启动代理 + 面板
./target/release/lens start                      # 代理 :9999 + 面板 :7317
# 在你的 agent 终端中:
export HTTP_PROXY=http://127.0.0.1:9999
export HTTPS_PROXY=http://127.0.0.1:9999
```

面板地址 `http://127.0.0.1:7317`,默认自动打开浏览器。

### 信任 MITM 证书(仅代理模式需要)

首次运行 `lens proxy`/`start` 时会在 `certs/` 生成 CA(`ca.crt` + `ca.key`,私钥权限 0600):

```bash
# macOS
sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain certs/ca.crt
# Linux (Debian/Ubuntu)
sudo cp certs/ca.crt /usr/local/share/ca-certificates/lens.crt && sudo update-ca-certificates
# Windows(普通用户级,无需管理员)
certutil -addstore -user Root certs\ca.crt
```

注意:CA 私钥只在本机 `certs/` 内,**不要**提交到任何仓库,也不要安装来源不明的 CA。

## 配置

复制 `config/lens.toml` 到 `~/.lens/config.toml`(或数据目录下 `config.toml`),或 `--config <path>` 显式指定。
优先级:**CLI 参数 > 配置文件 > 内置默认值**。

```toml
[proxy]
bind = "127.0.0.1:9999"
cert_dir = "certs"
# upstream_ca = "/path/to/corp-ca.pem"   # 企业自签网关
# insecure_upstream = false              # 跳过上游校验(仅调试)

[view]
bind = "127.0.0.1:7317"
open_browser = true

[storage]
# data_dir = "~/.lens"

[agents.codex]
enabled = true                            # 按 agent 关闭录制
```

## 命令

| 命令 | 说明 |
|---|---|
| `lens proxy` | 只启动代理 |
| `lens view` | 只启动可视化面板 |
| `lens start` | 代理 + 面板 |
| `lens import [--source claude\|opencode] [--path P] [--limit N]` | 导入会话日志:claude=JSONL,opencode=本地 SQLite 库;幂等,重复导入自动跳过 |
| `lens list [--limit N]` / `lens show <id>` | 命令行查看会话 |
| `lens init` / `lens clear` | 初始化数据目录 / 清空数据 |

## 能力边界(实测口径)

- **录制**:HTTP 与 HTTPS(CONNECT + 动态证书)完整转发;SSE 流式响应边转发边聚合后入库(OpenAI Chat Completions / Responses API、Anthropic Messages);非流式 JSON 响应直接入库。
- **识别**:OpenCode / Codex / Claude Code(按 UA + 请求体特征),其余按启发式归类为 unknown。
- **会话归组**:原生会话 id → workspace/cwd → 30 分钟时间窗(分桶,多进程一致)。
- **导入**:Claude Code JSONL(assistant 条目 uuid 幂等)、OpenCode 本地库(message.id 幂等,tool 输出截断保留);Codex CLI 导入暂缓(本机无样本可校准,不猜实现)。
- **不支持**:非 LLM 流量不解析;HTTP 请求体为 chunked 编码(罕见)不完整转发;Codex 本地日志导入暂未实现。

## 演示数据

```bash
python seed_demo_data.py                      # 合成 3 个演示会话 → demo_data/lens.db
cargo run -- view --data-dir demo_data        # 查看

python seed_novel_graph.py                    # 无职转生知识图谱演示 → demo_data/novel/lens.db
cargo run -- view --data-dir demo_data/novel  # 打开 /demo/novel
```

演示数据与真实录制互不污染(独立数据库;图谱按会话隔离)。

## 架构

```
src/
├── proxy/
│   ├── server.rs       # 监听分发(整头读取,零丢字节)
│   ├── connect.rs      # CONNECT + MITM + 上游 TLS 策略
│   ├── http_handler.rs # 明文 HTTP 转发
│   ├── httpbuf.rs      # 带残余缓冲的 HTTP 读取器(chunked 框架安全)
│   ├── sse.rs          # SSE 流式聚合器(chat/Responses/anthropic)
│   ├── io.rs / parser.rs / request.rs / cert.rs
├── engine/
│   ├── store.rs        # SQLite(schema v1,迁移带备份)
│   ├── graph.rs        # 会话隔离的知识图谱
│   ├── summary.rs / session.rs
├── import.rs           # Claude Code JSONL 导入
├── config.rs           # TOML 配置(CLI > 文件 > 默认)
└── web/                # axum API + 静态面板
frontend/               # Preact + Vite(类型由 ts-rs 从 Rust 生成)
```

接口契约的唯一真相是 Rust 结构体:`cargo test` 会把 TS 类型导出到 `frontend/src/generated/`(CI 校验无漂移)。

## 开发

```bash
cargo test -p lens          # 单测 + 场景 + 代理 e2e(mock 上游,含 TLS/SSE)
cargo clippy -p lens --all-targets -- -D warnings
cd frontend && npm ci && npx tsc --noEmit && npm run build   # 产物输出到 web/(已入库,CI 校验新鲜度)
```

CI 见 `.github/workflows/ci.yml`(Linux/Windows 双平台)。

## English

Lens turns AI coding-agent conversations into a searchable local asset. Two sources feed one store: a local
transparent HTTP/HTTPS proxy that records live traffic from OpenCode / Codex / Claude Code (SSE streaming
supported, aggregated before persisting), and an importer for Claude Code JSONL logs (zero-config, idempotent).
It then serves a session timeline, token/model stats, and a per-session knowledge graph, plus an experimental
Tauri desktop shell. API types are generated from the Rust structs via ts-rs.

## License

MIT
