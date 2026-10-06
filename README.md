# 🪶 Lens — AI 编程代理透镜

> 让你的每一次 AI 编程对话，都变成可回溯、可导航、可理解的全景图。

## 它解决什么？

你用 opencode / codex CLI / Claude Code / Kimi Code 写代码时，对话越来越长，上下文越来越多——但 agent 不会告诉你"我们到底经历了什么"。关掉终端，上次做到哪了、为什么那么决策、哪些方案被抛弃了——全部丢失。

Lens 是一个**零侵入的本地代理**，拦截你 agent 与 LLM 之间的所有通信，自动构建一个可视化的对话全景：
- **时间线**：什么时候启动、关键决策点在哪
- **关系图谱**：文件/模块/概念之间的依赖和影响网络
- **摘要**：AI 帮你总结"这个会话到底完成了什么"
- **跨会话记忆**：关掉再打开，上次的工作状态一目了然

## 快速开始

```bash
# 1. 构建
cargo build --release

# 2. 启动代理（默认监听 :9999）
lens proxy

# 3. 在另一个终端里，让 agent 走代理
HTTP_PROXY=http://localhost:9999 codex "帮我写一个 web 服务器"

# 4. 实时查看可视化面板
lens view
# → 浏览器打开 http://localhost:7317
```

## 核心理念

**对 agent 零感知** — agent 不需要知道 Lens 的存在，它照常调用 LLM API。Lens 只是透明地站在中间，记录一切、整理一切。

## 项目结构

```
lens/
├── src/
│   ├── proxy/       # HTTP 代理 + MITM TLS
│   ├── adapter/     # 各平台会话格式解析
│   ├── engine/      # 会话聚合 + AI 摘要 + 图谱构建
│   ├── web/         # Axum Web 服务器（可视化面板）
│   └── main.rs      # CLI 入口
├── web/             # 静态前端资源
└── migrations/      # SQLite schema
```

## 路线图

- [x] 项目初始化 / CLI
- [ ] HTTP 代理核心 + TLS MITM
- [ ] OpenAI / Anthropic 协议解析
- [ ] opencode / codex / Claude Code 适配器
- [ ] 会话聚合 + SQLite 持久化
- [ ] AI 摘要引擎
- [ ] Web 可视化面板
- [ ] trae / workbuddy 支持
