# Lens 重构审计 · 问题定位清单与目标架构

> 审计对象:`badhope/lens` @ `6e499f0`(2026-10-09,main 最新)
> 审计日期:2026-10-14
> 方法:工具扫描(`cargo clippy -p lens` / `tsc --noEmit` / `cargo test`) + 端到端流量回放(编译真实代理 + mock LLM 上游,分别注入流式/非流式流量)+ HTTP API 探针(seed 演示库后实测 JSON 形状)+ 全量源码走查。
> 结论:核心链路(代理转发 → 录制 → 会话 → API → 前端)存在 8 处 P0 级断点,当前仓库**任何一段真实链路都跑不通**;演示效果全部来自 seed 脚本。Rust 代码本身 lint 很干净(仅 4 条 clippy 风格告警),问题集中在**设计契约与集成接缝**,适合“保留 Rust 内核 + 重切接缝”式重构,不建议从零重写。

---

## 0. 实测证据(可复现)

| 验证 | 命令/方法 | 结果 |
|---|---|---|
| 核心 crate 编译+单测 | `cargo test -p lens` | 通过,6/6 |
| Rust lint | `cargo clippy -p lens --all-targets` | 仅 4 条风格告警(见 P2-3) |
| 桌面壳构建 | `cargo check --workspace` | **失败**:tauri.conf.json 非法字段(F10) |
| 前端类型检查 | `node node_modules/typescript/bin/tsc --noEmit` | **6 个错误**(F7),且 `npx tsc` 在 Windows 直接不可用 |
| 作者自带 e2e | `python test_e2e.py`(修 2 处 Windows 兼容后) | **失败**:客户端收到 `400 Bad Request`(F1+F2) |
| HTTP 转发回放 | mock 上游 + 真实代理 + curl | 请求未到 mock;代理把 59 字节请求行发给 `localhost:80`(F2) |
| HTTPS 回放 | `curl -x http://127.0.0.1:PORT -k https://example.com` | **12s 超时挂死**(F1) |
| 流式录制 | mock 返回 SSE/chunked | 不录制(设计级,F3);非流式也不录(F2 前置)
| API 契约 | seed 演示库 → `lens view` → curl `/api/*` | 返回字段与前端 types 全面不符(F6,见下表) |
| 仓库卫生 | `git ls-files` | node_modules 5849 个文件入库;`demo_data/certs/ca.key` 入库(F16) |

---

## 1. P0 —— 阻断级(全部实测复现)

### F1 CONNECT 隧道挂死:`BufReader::into_inner()` 丢弃已缓冲请求
- 位置:`src/proxy/server.rs:58`(read_line)→ `:66`(into_inner)
- 症状:curl 走代理访问任何 HTTPS 站点 12s 超时;mock 上游从未收到连接。
- 根因:`read_line` 把首行之后的全部字节读进了 BufReader 内部缓冲,`into_inner()` 返回裸流时这部分字节被丢弃;`handle_connect` 随后的 `drain_request_headers`(`src/proxy/connect.rs:46`)永远等不到 `\r\n\r\n`。
- 由 0.2.0 "性能优化" 引入(d3aa5ab 逐字节读首行 → 82f0ac9 改为 BufReader,未处理残余缓冲)。
- 修复方向:改用 `BufReader::into_parts()`(tokio≥1.44)取回 `(stream, buffer)`,把 buffer 拼进后续解析;或放弃 BufReader,保留原逐字节实现。

### F2 明文 HTTP 转发损坏:合成 `\r\n\r\n` + Host 丢失 fallback 到 localhost:80
- 位置:`src/proxy/server.rs:72`(`format!("{first_line}\r\n")`,而 first_line 自带 `\r\n`)、`src/proxy/io.rs:199`(prefix 中即命中 `\r\n\r\n` 直接 break)、`src/proxy/http_handler.rs:36`(Host 缺失 → `"localhost:80"`)
- 症状(实测):代理只把 59 字节请求行转发给本机 80 端口,客户端收到本机 80 的 400;真实 headers/body 全部丢失,永不录制。
- 修复方向:首行 `trim` 后拼单 CRLF;`read_full_request` 必须继续从 socket 读完整头部;Host 无法解析时返回 400,禁止猜测上游地址。

### F3 SSE 流式全链路不录制(设计级)
- 位置:`src/proxy/connect.rs:119-126`(判为流式 → `stream_relay` 转发后 `break`,不解析)、`:104`(`is_streaming_request` 只看 Accept/Content-Type/Request chunked,识别不了 `body.stream=true`)、`src/proxy/parser.rs:296`(只认完整 JSON body)
- 现状:Claude Code / Codex / OpenCode 默认流式;SSE 响应体不是 JSON,`ResponseParser` 必然失败 → 库永远是空的。全仓无任何 `data:` 事件聚合代码(grep 证实)。
- 修复方向:新增 SSE Collector(request→response 边转发边聚合,tee 一份),以聚合后的 message/usage 构造 `LlmResponse` 落库;`is_streaming_request` 补 body 解析(`"stream":true`)。

### F4 Codex Responses API 请求解析必然失败(不录制)
- 位置:`src/proxy/parser.rs:167-168`(`parse_openai_messages` 强制要求 `messages` 数组);Codex 走 `/v1/responses` 用 `input` 数组 —— `tests/scenario_test.rs:60` 的注释自认此缺口。
- 修复方向:parser 支持 `input`/`instructions` 结构(Responses API 形态),补单测。

### F5 会话归属失效:生产链路没有任何 "cwd",每轮对话自成一个会话
- 位置:`src/engine/store.rs:290-291`(唯一读 `metadata["cwd"]` 的地方);`grep '"cwd"' src/` 仅命中此处;`src/adapter/opencode.rs:22` 读到的 cwd 被存成 `opencode.workspace` 键,永远进不了 session 归组。
- 后果:实时录制时 `extract_session_id` 永远走 fallback `session-{uuid[:8]}` → 100 轮 = 100 个会话;会话列表/时间线这两大核心视图在生产数据上无意义。
- 为什么测试与演示没暴露:测试手工注入 `cwd`(`tests/scenario_test.rs:196/253`),演示库由 `seed_demo_data.py:53` 手写 `"cwd": "/home/dev/project"`。
- 修复方向:定义 session_key 优先级:agent 原生会话 id(OpenCode session_id / Codex previous_response_id 链 / Claude 请求内 system 元信息)→ 规范化的 workspace/cwd → 兜底按 (agent, 时间窗) 聚合;并写进 schema(`sessions.source`)。
- 补充:同名问题 `sessions.title` 即 id(`store.rs:97` 插入 title=session_id),列表展示一串 uuid8。

### F6 前后端契约全景漂移(实测 JSON vs 前端 types vs 组件)
- 实测 `/api/sessions` 返回:`message_count` / `total_tokens` / `agent_type` / `project_path` / `model` …
- 实测 `/api/stats` 返回:`agents: [["codex",4],…]`(**元组数组**)、`models: [["gpt-4o",8],…]`
- 实测 `/api/sessions/:id` 返回:`{session, summary, turns:[{request,response,agent_type,…}]}`

| 前端期望(types.ts) | 后端实际 | 后果(定位) |
|---|---|---|
| `turn_count` | `message_count` | Sidebar.tsx:104 显示 "undefined turns" |
| `token_count` | `total_tokens` | DashboardView.tsx:30 sparkline 全 NaN |
| `agents: {name,count}[]` | `[string, number][]` | Sidebar 芯片/DonutChart 全空,DashboardView 模型条不渲染 |
| `SessionDetail.messages / .tool_calls` | `session.turns[].request.messages` | TimelineView.tsx:38 `for…of undefined` 直接抛错 → ErrorBoundary 兜底页 |
| (无) | `agent_type` | types.ts 缺字段(Sidebar 靠手写断言勉强编译) |

- 历史成因可考:`03f99a2` 的提交信息来自一个 FastAPI 后端时代(auth.py/main.py/test_evaluate.py),说明前端是对着旧 Python API 写的,Rust 重写后未同步契约。`web/assets` 内置产物 grep 到 `turn_count`、grep 不到 `message_count`,证明**正在发布的 UI 与正在发布的后端互不匹配**。
- 修复方向:以 Rust 结构为唯一真相,用 `ts-rs` 生成 `types.gen.ts`(CI 校验),再改组件;禁止手写接口镜像。

### F7 前端源码不可构建,且 src ↔ node_modules ↔ web/assets 三态漂移
- tsc 错误:GraphView.tsx(10,12,14) 缺 `d3-selection/d3-zoom/d3-drag` 模块、Markdown.tsx(1) 缺 `dompurify`、GraphView.tsx(249/252) 隐式 any —— 入库的 `frontend/node_modules` 里根本没有这些包(只有 d3-force 系 4 个包),且 Windows 下 `.bin` 无 `.cmd` 包装,`npx tsc` 直接不可用。
- `web/assets/*.js` 是含 DOMPurify 的旧构建 → 当前源码(引入 DOMPurify 后)**从未成功构建并发布**。
- 修复方向:`git rm -r --cached frontend/node_modules`;`npm ci`;锁文件 + CI `tsc --noEmit && vite build`;产物由 CI 重建(xem .gitignore 决策点 D1)。

### F8 Unix 首次启动即失败:chmod 先于写文件
- 位置:`src/proxy/cert.rs:149`(set_permissions)→ `:152`(write key)。文件尚不存在时 `set_permissions` 返回 NotFound → `CertManager::init` 失败 → `lens proxy` 报错退出。
- 影响:macOS/Linux 全新机器首启必挂(Windows 因 cfg(unix) 跳过而幸免——所以作者在 Windows 上没遇到)。
- 修复方向:先 write 再 chmod(或用 `OpenOptions::new().mode(0o600)` 创建)。

---

## 2. P1 —— 严重(功能残缺 / 工程债)

### F9 知识图谱:无会话归属 + 破坏性全表操作 + 权重永不更新
- `nodes/edges` 表无 `session_id`(`src/engine/store.rs:58-73` schema);`build_graph` 每次为某会话重建时执行 `DELETE FROM nodes WHERE id LIKE 'file:%'`(`src/engine/graph.rs:163-164`)——**清掉所有会话的文件节点**;`INSERT OR IGNORE`(`:187`)导致边权重永远不会增长;每个 `/api/sessions/:id/graph` 请求全量重算。
- 修复方向:schema 迁移加 `session_id` 外键;增量 upsert(`ON CONFLICT DO UPDATE weight=?+excluded.weight`);图谱在写入 turn 时增量维护,读路径只查。

### F10 Tauri 桌面壳:构建失败 + 无法自启后端
- `src-tauri/tauri.conf.json:12` 在 v2 的 `app` 层写了非法 `title`(已被 `cargo check` 实证) → 桌面端构建失败;`externalBin: []`(`:41`)未捆绑后端,`src-tauri/src/main.rs:76` `exe_dir.join("lens")` 在 Windows 应为 `lens.exe`。
- 修复方向:删非法字段;决定桌面端定位(D2):要么 sidecar 捆绑 `lens` 二进制并修 `.exe`,要么本版砍掉桌面壳、以 `lens-app.sh`/浏览器 PWA 替代。

### F11 `--config` 与 `config/lens.toml` 是死链
- `src/main.rs:19` 收下 `--config` 但 main() 从未使用;全仓无 toml 解析(无 serde/toml 依赖)。config/lens.toml 里的所有配置项(绑定地址、intercept_https、adapter 开关、AI 摘要)全部无效。
- 修复方向:实现最小配置加载(proxy/view/storage 三节 + CLI 覆盖),或删除该文件与参数并修 README。

### F12 上游 TLS 策略不可配置
- `src/proxy/connect.rs:241-248` 固定 webpki-roots;不支持企业自签网关、不支持上游 HTTP 代理链(HTTP_PROXY 环境变量对本代理自身无效)。国内用户走中转网关时直接不可用。
- 修复方向:配置 + 系统证书库(rustls-platform-verifier)或 `--insecure-upstream` 显式开关;支持上游代理。

### F13 请求/响应配对脆弱(SSE 化后必须先解决)
- `src/proxy/connect.rs:139` 按“同隧道顺序”记录;无 response id 校验。keep-alive 复用/并发流场景会错配。
- 修复方向:用响应体/SSE 首帧中的 id(OpenAI `id`、Anthropic `message.id`)关联;实现 SSE 聚合时天然拿到 id。

### F14 无上限读取
- `/api/graph` 全表拉取 nodes/edges(`src/web/server.rs:124-127` + `store.rs:415/430` 无 LIMIT);turns 无分页;SSE 聚合后库会迅速变大。
- 修复方向:分页 + 服务端按 weight 截断(top-N)+ 前端虚拟化。

### F15 测试与 CI 平台绑定,e2e 长期红
- `test_e2e.py` 的 pkill / `./target/release/lens` 在 Windows 不可行;当前 HEAD 上该测试失败(F1+F2)。无 CI 工作流文件(`git ls-files` 证实)。
- 修复方向:e2e 改写为 Rust integration test(tokio mock server,零外部依赖)+ GitHub/GitCode Actions(fmt/clippy/test/tsc/build)。

### F16 仓库卫生
- `frontend/node_modules` 5849 文件入库(占仓库文件数 98%;`git ls-files` 证据),且内容陈旧(见 F7);
- `web/assets` 产物入库但与源码不一致(F7);`vite` 的 `emptyOutDir: true` + `outDir: ../web`(`frontend/vite.config.ts:9-11`)会在下次构建时**删掉手工文件 `web/novel_graph.html`**;
- `demo_data/certs/ca.key` 入库,与 `.gitignore:12` 自写规则直接矛盾(该 CA 若被信任过即可被重签证书 MITM;应立即轮换 + 清历史,或标记为 CI 专用并写明"永不信任");
- `docs/` 截图与 README 的 GitHub×2 / Gitee 徽章指向 404 仓库(实测);`src/web/handlers.rs` 注释里的 GitHub issue 链接同样 404。

### F17 README/陈述失真(需系统性回写)
- "零配置"(实际要装 CA + 设环境变量 + 双进程)、"跨平台 Tauri v2 桌面"(F10)、"SSE 实时"(仅转发层面,F3)、"多代理识别"(解析层缺 Codex Responses,F4)。

### F18 演示域与产品域混淆
- `GraphView.tsx`/`GraphPanel.tsx` 调色板硬编码小说实体类(character/location/family/event/skill/artifact —— 无职转生演示)+ 代码类(file/function/topic)混排;`seed_novel_graph.py` 直接向 nodes/edges 灌小说数据且"与 agent 图谱共存"(无隔离)。
- 修复方向:kind 只用 registry 白名单代码域;演示数据独立 DB(`demo_data/novel.db`),演示页不参与产品图谱接口。

### F19 会话列表聚合字段错误(字典序而非最新)
- `src/engine/store.rs:138-139/178-179`:`MAX(t.agent_type)` / `json_extract(MAX(t.request_json),'$.model')` —— MAX 取的是字符串字典序最大,不是时间最新;混合 agent 会话会显示错误 agent/model。
- 修复方向:用 `ORDER BY timestamp DESC LIMIT 1` 的子查询或窗口函数取最新轮次字段。

### F20 CLI 健壮性
- `src/engine/session.rs:112` `&turn.timestamp[11..19]` 可 panic(短串/非 ASCII 边界);`search_sessions` 未转义 LIKE 通配符(`store.rs:191`);`clear_data` 只删 lens.db,不删 `-wal/-shm`(`session.rs:30-38`)。

---

## 3. P2 —— 设计债(重构时顺手消化)

1. **单写入者缺失**:代理热路径同步持 `Mutex<Connection>` 并在锁内做 JSON 序列化(`store.rs:89-127`);应改为 `mpsc → 专用 writer 线程`,`record` 变为非阻塞投递。
2. **双进程共库**:CLI/Tauri 壳/`lens start` 可能同时开库(WAL 缓解但无所有权协议);收敛为单一 `lensd` 常驻进程 + 瘦客户端。
3. **clippy 4 条**:`&PathBuf→&Path`(`server.rs:22`)、`get().is_some()`、auto-deref 等,随重构清掉。
4. **前端死代码与半成品**:`frontend/src/hooks/useApi.ts` 空模块;`GraphView.tsx` `centerNodeById` 空实现(`:357`)导致详情面板 "jump" 点了没反应;`Summary` API 有字段无 UI;`TimelineView` 的 messages/tool_calls 分组逻辑建立在错误契约上(F6)将整体重写。
5. **parser 保真度**:Anthropic `tool_use` 只存 name、`tool_result` 只存 text(丢 input/结构化内容,`parser.rs:203-236`);`ContentBlock` 需携带原始 payload。
6. **cert 缓存无上限**(每 host 一张常驻);**CORS 白名单硬编码 7317**(`web/server.rs:39`)与可配置端口脱节(同源请求不受影响,属配置一致性)。
7. **i18n/文档语言**:中英混排(界面英文 "Invoking Graph" 与中文混用),重构时统一。

---

## 4. 目标架构(建议方案,待确认)

```
┌─ 采集层 sources/ ─────────────────────────────────────────────┐
│  A. jsonl/   原生会话日志读取(主数据源,零摩擦)              │
│     ~/.claude/projects/**/*.jsonl · ~/.codex/sessions ·       │
│     OpenCode storage —— 用一个 narrow interface 归一化        │
│  B. proxy/   改造后的 MITM(补充源:不落盘日志的 agent、      │
│     API 级取证)。修复 F1/F2,新增 SSE Collector(F3)          │
│  两者输出统一的 NormalizedTurn → ingest 通道(mpsc)           │
├─ 模型层 model/ ──────────────────────────────────────────────┤
│  NormalizedTurn { agent, session_key, ts, req{...}, resp{...} }│
│  session_key 优先级:原生会话 id → workspace 规范化 → 时间窗   │
├─ 存储层 store/(单写入者)────────────────────────────────────┤
│  sessions(id, agent, source, project_path, title, …)          │
│  turns(id, session_id FK, seq, ts, …)                        │
│  entities(session_id FK, node_id, kind, …)                   │
│  relations(session_id FK, src, dst, rel, weight)             │
│  ↑ 全部带会话归属;增量 upsert;读路径分页/top-N               │
├─ API 层 web/(axum)──────────────────────────────────────────┤
│  冻结契约:ts-rs 由 Rust 结构生成 types.gen.ts,CI 校验        │
├─ UI 层 frontend/ ────────────────────────────────────────────┤
│  契约唯一来源 types.gen.ts;tsc+vite 纳入 CI;d3 只保留图谱部分 │
├─ 进程模型 ───────────────────────────────────────────────────┤
│  lensd(单进程:proxy+web)──Tauri 壳/CLI 作为其客户端          │
└──────────────────────────────────────────────────────────────┘
```

**录制管线核心改造(SSE Collector)**:在 `connect.rs` 的流式分支用 tee relay —— 边转发边解析 `data:` 事件,结束时按 provider 组装完整 message + usage → `LlmResponse`;以响应内 id 关联请求(F13)。非流式路径保持现状但过同一 Collector 接口。

---

## 5. 工作分解(按依赖排序,每项含验收)

| # | 任务 | 验收标准 |
|---|---|---|
| 1 | 修 F1+F2 代理转发 | 纯 Rust integration e2e(mock 上游)绿:HTTP 与 HTTPS 各 1 条非流式录制进库 |
| 2 | 会话模型(F5)+ schema 迁移(F9 前置) | mock 连续 5 轮同 cwd 流量 → 1 个会话 5 轮;跨会话图谱互不污染 |
| 3 | SSE Collector(F3)+ Codex Responses 解析(F4)+ 配对(F13) | mock SSE(3 事件)→ 完整 turn 落库,usage 正确;`/v1/responses` 输入可录 |
| 4 | 契约对齐(F6/F7):ts-rs + 组件适配 + npm ci | `tsc --noEmit && vite build` 绿;对照实测 JSON 逐字段核验;UI 三视图在 seed 库上无 undefined/无异常 |
| 5 | 图谱重构(F9)+ 演示域隔离(F18) | 会话图谱只含本会话实体;重复构建权重单调递增;novel 演示独立数据 |
| 6 | 测试与 CI(F15/F16 仓库卫生) | Actions:clippy/test/tsc/build 四绿;node_modules 出仓;e2e 双平台 |
| 7 | 配置生效或删除(F11)+ 上游 TLS 策略(F12)+ CLI 健壮性(F20) | `--config` 或删;e2e 覆盖自签上游;CLI 无 panic 路径 |
| 8 | 桌面端决策(F10,D2)+ README 回写(F17)+ 密钥/历史清理(F16) | workspace 构建绿;README 与实测一致;ca.key 轮换并清历史 |
| 9 | 数据源分层:JSONL importer(采集层 A) | 导入真实 `~/.claude/projects` 样本 → 时间线完整呈现 |

## 6. 开放决策点(需要拍板)

- **D1 产物入库**:`web/` 构建产物继续入库(便于无 Node 直接运行)还是改为 CI 构建 + Release 附件?后者仓库更干净,前者开箱即用。
- **D2 Tauri 去留**:修好(需 sidecar 打包,工作量大)还是本版砍掉(浏览器/PWA + 启动脚本)?
- **D3 数据源主次**:是否接受"JSONL 为主、代理为辅"(纯读取类竞品 300-400★ 已证明零摩擦路线有效),还是坚持代理为核心叙事?
- **D4 是否保留 novel 演示线**:它提供了图谱的"美学验收"价值,但与产品域混在一起;建议独立 demo 仓库/页面并明确标注。

---

## 附:审计工具输出存档

- clippy(仅 4 条风格告警):`needless_borrow`(`server.rs:22` 附近)、`unnecessary_get_then_check`(`tests/scenario_test.rs:144` 附近)、auto-deref ×1。
- tsc 6 错:GraphView.tsx:10/12/14/249/252、Markdown.tsx:1。
- 自带 e2e 失败输出:`[Client] ← Status: HTTP/1.1 400 Bad Request` → `✗ Request through proxy failed!`;代理日志(插桩版)显示 `read_full_request -> 59 bytes` → `connected upstream localhost:80`。

---

## 7. 债务清偿状态（2026-10-14 第二轮,全部完成]

第一轮已清 P0/P1 全量后,第二轮把 P2 与遗留逐条清掉:

| 清单项 | 状态 | 落地方式 |
|---|---|---|
| P2-1 录制热路径同步持锁写库 | 已清 | `engine/writer.rs` 单写入者队列(512 容量,专用线程,满则回退同步不丢数据);测试 `test_single_writer_queue_persists_all_turns` |
| F13 配对脆弱性(可取证性) | 已清 | SSE 聚合器捕获上游响应 id(openai/anthropic/responses 三形态)写入 `raw.id`;非流式为原始 JSON 自带 id |
| P2-6 证书缓存无上限 | 已清 | `CERT_CACHE_LIMIT = 512`,超限整体清空;测试 `test_cert_cache_is_bounded` |
| P2-6 CORS 白名单硬编码 7317 | 已清 | `cors_origins(port)` 随实际监听端口生成;单测 `cors_origins_follow_listen_port` |
| P2-4 useApi 死模块 | 已清 | 删除;`GraphView.centerNodeById` 空实现改为真实居中(ref 注入) |
| P2-4 summary API 有端点无 UI | 已清 | TimelineView 顶部会话摘要卡(规则提取版) |
| P2-7 中英混排 | 已清 | 界面字符串统一中文(搜索/图谱/面板/侧栏) |
| Codex CLI 导入 | 暂缓(有据) | 本机无任何 `~/.codex` 会话样本,不凭猜测实现;已在 `src/import.rs` 模块文档与 README 注明 |
| OpenCode 导入 | 已完成 | 按本机真实 OpenCode SQLite 库(2026-10 实测 schema)实现,`message.id` 幂等;测试 `test_import_opencode_sqlite_is_idempotent` |
| CA 私钥历史清除 | 待决(破坏性) | 索引已移除;历史重写需 force push —— 步骤见 `docs/SECURITY-ROTATION.md`,待你确认后执行 |

