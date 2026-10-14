# Changelog

All notable changes to this project will be documented in this file.

## [0.3.0] - 2026-10-14

全链路修复与重构（详见 docs/REFACTOR-AUDIT.md 与 .aqg 台账）。此前 0.2.0 的"性能优化"重构引入了
致命回归：代理无法转发真实流量、响应体解码缺失导致从未成功入库；本版修复并以 37 个测试（含真实
TCP/TLS/SSE 回放 e2e）锁定行为。

### Fixed（核心链路）

- **代理转发**：分发器改为整头无丢失读取；CONNECT 隧道不再挂死；明文 HTTP 请求行/头/体完整转发并把请求行改写为 origin-form；Host 缺失返回 400（不再猜测上游）
- **录制**：响应体按 Content-Length / chunked 解码后再解析（此前把响应头一并喂给 JSON 解析器，非流式也从未入库）；新增 `HttpBuf` 使 chunked 框架读取/转发不再错位，隧道 keep-alive 可复用
- **SSE 流式**：边转发边聚合（OpenAI Chat Completions / Responses API / Anthropic Messages），usage 入库；客户端实时收到增量
- **Codex Responses API**：`input` 数组请求可解析入库
- **会话归组**：原生会话 id → workspace/cwd → 30 分钟时间窗；修复"每轮对话各自成会话"
- **会话列表**：agent/model 取最新轮次（原为字典序 MAX）；标题取首条用户消息
- **图谱**：按会话隔离（`session_id`），重建不再清空其他会话；重复构建幂等；全局图谱有上限（800 节点/1500 边）
- **Unix 首启**：CA 生成先写文件后 chmod（此前 chmod 先于写文件导致 Unix 首次启动失败）
- **CLI**：时间戳切片 panic、LIKE 通配符未转义、`clear` 未清理 WAL/SHM
- **前端契约**：ts-rs 从 Rust 结构生成 `frontend/src/generated/`（唯一真相），修复 `turn_count`/`token_count`/`{name,count}`/`messages` 全量漂移；时间线按 turns 重写
- **桌面壳**：`tauri.conf.json` 非法字段与图标格式（ico/icns 实为 PNG 改名）修复；后端二进制查找兼容 Windows
- **仓库卫生**：`frontend/node_modules` 与演示 CA 私钥出仓；e2e 改为 Rust 集成测试（替换平台绑定的 Python 脚本）

### Added（第二轮技术债清偿）

- **单写入者队列**（`engine/writer.rs`）：录制热路径只投递，专用线程串行落库；队列满回退同步写（不丢数据）
- **响应 id 取证**：SSE 聚合器捕获上游响应 id（OpenAI/Anthropic/Responses 三形态）写入 `raw.id`
- **OpenCode 导入**：`lens import --source opencode`（只读打开本地 SQLite 库，`message.id` 幂等，tool part → tool_calls，输出截断 2000 字符）
- **导入限流参数**：`--limit N` 控制导入最近会话数
- **会话摘要上界面**：时间线顶部展示规则提取的会话摘要卡片
- **CI 依赖漏洞扫描**：`cargo audit`
- 界面文案统一中文（搜索/图谱/实体面板/侧栏）

### Fixed（第二轮）

- 证书缓存无上限 → `CERT_CACHE_LIMIT=512`，超限清空并告警
- CORS 白名单硬编码 7317 → 随实际监听端口生成
- 图谱重建无事务 → `BEGIN IMMEDIATE` + ROLLBACK，失败不留半删状态
- f64 排序 `partial_cmp().unwrap()`（NaN 会 panic）→ `total_cmp`
- SSE 行缓冲超限静默丢弃 → 记录 warning（已聚合内容保留）
- 删除死模块 `useApi.ts`；`GraphView` 节点跳转居中由空实现改为真实实现

### Docs

- `docs/SECURITY-ROTATION.md`：CA 私钥历史清除（filter-repo 步骤）与令牌轮换清单
- `.aqg/code-construction/`：施工台账（plan 级）+ 五维审查台账（本地咨询，标注需跨厂商重跑）

### Added

- `lens import`：Claude Code JSONL 导入（幂等）
- 配置文件真正生效：`--config` > 文件 > 默认；新增上游 CA / insecure、agent 录制开关、面板自动打开、`storage.data_dir`
- 上游 TLS 策略可配（系统根 / 自定义 CA / 显式 insecure）
- API 分页（`limit`/`offset`）与会话详情轮次上限；SQLite schema v1 迁移（迁移前自动 `.bak` 备份）

### Changed / Breaking

- API 字段与生成类型一致：`stats.agents/models` 为 `{name,count}` 对象数组；会话字段为 `message_count`/`total_tokens`
- 图谱节点 id 形如 `{session}#{kind}:{name}`（会话作用域）
- 演示数据分离：无职转生演示使用 `demo_data/novel/`（独立库）
- 升级提示：新克隆仓库不再包含 `frontend/node_modules`（改 `npm ci`）与 `demo_data/certs`（改运行时生成）

## [0.2.0] - 2026-10-08

### Security
- **CORS restricted** — Replaced `CorsLayer::permissive()` with explicit `localhost:7317` origin allowlist. Prevents malicious web pages from accessing the Lens API to steal LLM API keys and conversation data.
- **CA private key file mode** — `ca.key` is now written with `chmod 0600` (owner read/write only), preventing other local users from forging certificates.
- **Markdown XSS protection** — `Markdown.tsx` now sanitises output through DOMPurify, preventing stored XSS via crafted AI agent messages.
- **Response body hard cap** — `read_http_response` enforces a 10 MiB `MAX_BODY_SIZE` to prevent upstream servers from exhausting memory.
- **`.gitignore` hardened** — `demo_data/certs/` now explicitly ignored to prevent accidental CA key commits.

### Correctness
- **HTTP/1.1 keep-alive in CONNECT tunnels** — TLS MITM now loops to handle multiple requests per tunnel instead of closing after a single request (Fixes: agent multi-round HTTPS failures).
- **Chunked Transfer Encoding support** — `read_http_response` now correctly handles `Transfer-Encoding: chunked`, which is used by all OpenAI / Anthropic streaming endpoints.
- **SSE streaming relay** — `text/event-stream` responses are now streamed chunk-by-chunk to the client instead of buffered, enabling real-time token display.
- **Session ID collision fix** — `extract_session_id` now uses `DefaultHasher` on the `cwd` field instead of string-replacing `/`, preventing data mixing when paths differ only by `/` vs `-` placement.
- **`seed_demo_data.py` portability** — Hardcoded absolute path replaced with relative `./demo_data/lens.db` and CLI arg override.

### Performance
- **Buffered first-line dispatch** — `dispatch_connection` now uses `BufReader::read_line` instead of byte-by-byte reads (eliminates ~200 syscalls per request).
- **SQLite WAL mode** — `Store::open` now enables `journal_mode = WAL`, allowing concurrent reads during writes.
- **Atomic turn recording** — `record_turn` now wraps session upsert + turn insertion in a SQLite transaction, preventing partial writes.

### Agent Integration
- **Adapter context extraction** — `parse_request_data` now dispatches to the matching module's `extract_context` (Codex previous_response_id, Claude Code system blocks, OpenCode workspace), populating `LlmRequest.metadata`.

### Frontend
- **Visibility-aware polling** — Session list refreshes pause when the browser tab is hidden (`document.hidden`).
- **Exponential backoff on fetch errors** — Polling interval grows from 3 s to max 30 s on consecutive failures instead of hammering a downed backend.
- **Error Boundary** — `App` is wrapped in a Preact `ErrorBoundary` component that catches render-time exceptions and shows a recovery screen.
- **`@tailwindcss/typography` installed** — Restores `prose` styling for markdown rendering.
- **`useApi.ts` cleanup** — Replaced dead-code with a TODO note.

### Engineering
- **README accuracy** — Removed unsupported Trae / Kimi Code claims; fixed architecture tree filenames (`http.rs` → `http_handler.rs`).
- **`web/handlers.rs` clarity** — Added explicit note that handlers reside in `server.rs`.

## [0.1.0] - 2026-09-28

### Initial Release
- Transparent HTTP/HTTPS proxy for AI coding agents (OpenCode, Codex, Claude Code).
- TLS MITM with dynamic certificate generation.
- SQLite-backed conversation storage.
- Session timeline, dashboard, and D3-force knowledge graphs.
- Tauri v2 desktop shell.
- End-to-end Python-based test harness.
