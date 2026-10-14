#!/usr/bin/env python3
"""Seed Lens DB with realistic demo sessions for VP presentation."""

import json
import sqlite3
import uuid
from datetime import datetime, timedelta

DEFAULT_DB_PATH = "./demo_data/lens.db"

def create_schema(conn):
    conn.executescript("""
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
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS nodes (
            id TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            kind TEXT NOT NULL,
            weight REAL DEFAULT 1.0,
            first_seen DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS edges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source TEXT NOT NULL,
            target TEXT NOT NULL,
            relation TEXT NOT NULL,
            weight REAL DEFAULT 1.0,
            UNIQUE(source, target, relation)
        );
        CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session_id);
        CREATE INDEX IF NOT EXISTS idx_turns_timestamp ON turns(timestamp);
    """)

def make_request(agent_type, model, messages, tools=None, ts=None):
    return {
        "id": str(uuid.uuid4()),
        "timestamp": ts or datetime.utcnow().isoformat() + "Z",
        "provider": "OpenAI",
        "endpoint": "/v1/chat/completions",
        "messages": messages,
        "model": model,
        "temperature": 0.7,
        "tools": tools,
        "metadata": {
            "cwd": "/home/dev/project",
            "user-agent": f"LensBot/1.0 ({agent_type})",
            "content-type": "application/json"
        }
    }

def make_response(req_id, content_text, tool_calls=None, usage=None, ts=None):
    resp = {
        "request_id": req_id,
        "timestamp": ts or datetime.utcnow().isoformat() + "Z",
        "provider": "OpenAI",
        "content": [{"type": "text", "text": content_text, "image_url": None}],
        "tool_calls": tool_calls,
        "usage": usage,
        "finish_reason": "stop",
        "raw": {}
    }
    return resp

def insert_turn(conn, session_id, agent_type, request, response):
    req_json = json.dumps(request, ensure_ascii=False)
    resp_json = json.dumps(response, ensure_ascii=False)
    ts = request["timestamp"]
    conn.execute(
        "INSERT INTO turns (id, session_id, agent_type, timestamp, request_json, response_json) VALUES (?, ?, ?, ?, ?, ?)",
        (request["id"], session_id, agent_type, ts, req_json, resp_json)
    )

def insert_session(conn, sid, title, project_path, created_at, updated_at):
    conn.execute(
        "INSERT OR REPLACE INTO sessions (id, title, project_path, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        (sid, title, project_path, created_at, updated_at)
    )

def main():
    import os
    import sys
    db_path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_DB_PATH
    os.makedirs(os.path.dirname(db_path) or ".", exist_ok=True)

    conn = sqlite3.connect(db_path)
    create_schema(conn)
    # 幂等：重复执行不累积历史演示数据
    conn.execute("DELETE FROM turns")
    conn.execute("DELETE FROM sessions")

    base_time = datetime(2026, 10, 6, 14, 0, 0)

    # ========== Session 1: Trae | 用户认证模块重构 ==========
    s1 = "session-trae-auth-module"
    s1_created = base_time.isoformat()

    tools1 = [
        {"type": "function", "function": {"name": "read_file", "description": "Read a file", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}}},
        {"type": "function", "function": {"name": "write_file", "description": "Write content to a file", "parameters": {"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}}},
        {"type": "function", "function": {"name": "run_tests", "description": "Run test suite", "parameters": {"type": "object", "properties": {"pattern": {"type": "string"}}}}}
    ]

    # Turn 1: user asks for auth refactor
    req1 = make_request("trae", "gpt-4o",
        [{"role": "user", "content": "重构 auth 模块，把 JWT 验证逻辑从 controller 中抽离到独立的 middleware。当前代码耦合严重，auth 检查散落在 6 个 controller 里面。注意保持向后兼容。", "name": None}],
        tools=tools1, ts=(base_time + timedelta(minutes=0)).isoformat() + "Z")
    resp1 = make_response(req1["id"],
        "好的，我来分析当前的 auth 模块代码结构。首先让我查看现有的文件布局和 controller 中的 JWT 验证逻辑。\n\n需要执行以下步骤：\n1. 读取现有的 auth 相关文件\n2. 找出散落的验证逻辑\n3. 创建 middleware 抽取逻辑\n4. 确保所有测试通过",
        None,
        {"prompt_tokens": 2400, "completion_tokens": 800, "total_tokens": 3200},
        ts=(base_time + timedelta(minutes=1)).isoformat() + "Z")
    insert_turn(conn, s1, "trae", req1, resp1)

    # Turn 2: tool call - read files
    req2 = make_request("trae", "gpt-4o",
        [{"role": "user", "content": "重构 auth 模块，把 JWT 验证逻辑从 controller 中抽离到独立的 middleware。当前代码耦合严重。", "name": None},
         {"role": "assistant", "content": "好的，让我先读取现有的 auth 相关文件。", "name": None},
         {"role": "tool", "content": "export function verifyToken(token: string): UserPayload { ... }\nexport function authMiddleware(req, res, next) { ... }", "name": "read_file"}],
        tools=tools1, ts=(base_time + timedelta(minutes=3)).isoformat() + "Z")
    resp2 = make_response(req2["id"],
        "我已经读取了以下文件：\n- `src/middleware/auth.ts` — JWT 验证\n- `src/controllers/UserController.ts`\n- `src/controllers/PaymentController.ts`\n\n现有 6 个文件直接调用 verifyToken。现在我来重构，创建一个统一的 AuthGuard 类。",
        [{"id": "tool_call_001", "type": "function", "function": {"name": "write_file", "arguments": {"path": "src/auth/AuthGuard.ts", "content": "export class AuthGuard {\n  static async handle(req: Request): Promise<UserPayload> {\n    // ...\n  }\n}"}}}],
        {"prompt_tokens": 5200, "completion_tokens": 1200, "total_tokens": 6400},
        ts=(base_time + timedelta(minutes=4)).isoformat() + "Z")
    insert_turn(conn, s1, "trae", req2, resp2)

    # Turn 3: continue
    req3 = make_request("trae", "gpt-4o",
        [{"role": "user", "content": "继续", "name": None}],
        tools=tools1, ts=(base_time + timedelta(minutes=6)).isoformat() + "Z")
    resp3 = make_response(req3["id"],
        "✅ AuthGuard 已创建。现在逐一替换 6 个 controller 中的直接调用，使其走 AuthGuard.handle() 统一入口。\n\n替换清单：\n1. UserController.ts\n2. PaymentController.ts\n3. OrderController.ts\n4. AdminController.ts\n5. ApiKeyController.ts\n6. WebhookController.ts",
        [{"id": "tool_call_002", "type": "function", "function": {"name": "run_tests", "arguments": {"pattern": "auth"}}}],
        {"prompt_tokens": 3800, "completion_tokens": 950, "total_tokens": 4750},
        ts=(base_time + timedelta(minutes=7)).isoformat() + "Z")
    insert_turn(conn, s1, "trae", req3, resp3)

    # Turn 4: final
    req4 = make_request("trae", "gpt-4o",
        [{"role": "user", "content": "运行一下测试", "name": None}],
        tools=tools1, ts=(base_time + timedelta(minutes=9)).isoformat() + "Z")
    resp4 = make_response(req4["id"],
        "测试结果：✅ 47 passed, 0 failed\n\n重构完成。代码变化：\n- 新增: src/auth/AuthGuard.ts (78 行)\n- 修改: 6 个 controller (共减少 ~120 行重复代码)\n- 删除: src/utils/verifyToken.ts (废弃)\n- 测试: 通过 47/47，覆盖率达到 92%",
        None,
        {"prompt_tokens": 1200, "completion_tokens": 620, "total_tokens": 1820},
        ts=(base_time + timedelta(minutes=10)).isoformat() + "Z")
    insert_turn(conn, s1, "trae", req4, resp4)

    insert_session(conn, s1, "[Trae] auth 模块 JWT 重构", "/home/dev/project", s1_created, (base_time + timedelta(minutes=10)).isoformat())

    # ========== Session 2: OpenCode | API 网关 bug 修复 ==========
    s2 = "session-opencode-gateway-bug"
    s2_created = (base_time + timedelta(hours=2)).isoformat()

    tools2 = [
        {"type": "function", "function": {"name": "search_code", "description": "Search code patterns", "parameters": {"type": "object", "properties": {"query": {"type": "string"}}}}},
        {"type": "function", "function": {"name": "read_file", "description": "Read a file", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}}},
        {"type": "function", "function": {"name": "write_file", "description": "Write a file", "parameters": {"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}}}
    ]

    req5 = make_request("opencode", "claude-sonnet-4-20250514",
        [{"role": "user", "content": "API 网关偶尔返回 502，日志显示 upstream timeout，但后端服务正常。频率大约每 100 次请求出现 1-2 次。请排查。", "name": None}],
        tools=tools2, ts=(base_time + timedelta(hours=2, minutes=0)).isoformat() + "Z")
    resp5 = make_response(req5["id"],
        "排查方向：\n1. 检查网关连接池配置\n2. keep-alive 超时设置\n3. 上游服务 TIME_WAIT 堆积\n\n先搜索网关配置代码。",
        [{"id": "tool_call_003", "type": "function", "function": {"name": "search_code", "arguments": {"query": "upstream.*timeout|proxy_read_timeout|keepalive"}}}],
        {"prompt_tokens": 1800, "completion_tokens": 450, "total_tokens": 2250},
        ts=(base_time + timedelta(hours=2, minutes=1)).isoformat() + "Z")
    insert_turn(conn, s2, "opencode", req5, resp5)

    req6 = make_request("opencode", "claude-sonnet-4-20250514",
        [{"role": "user", "content": "网关超时 502", "name": None},
         {"role": "assistant", "content": "排查中...", "name": None},
         {"role": "tool", "content": "发现配置: proxy_read_timeout 60s, keepalive_timeout 75s, 连接池大小=128\nTIME_WAIT: 3400+ 连接", "name": "search_code"}],
        tools=tools2, ts=(base_time + timedelta(hours=2, minutes=3)).isoformat() + "Z")
    resp6 = make_response(req6["id"],
        "🔍 根因定位：\n\n**问题**: keepalive_timeout(75s) > 后端 keepalive(60s)\n网关在连接已关闭后复用 upstream 连接，导致 ECONNRESET → 502。\n\nTIME_WAIT 3400+ 也印证连接未复用。\n\n**修复方案**:\n```nginx\nkeepalive_timeout 55s;  # < 后端的 60s\nkeepalive_requests 100;\n```\n已修复并重启网关，错误率从 1.5% → 0%。",
        None,
        {"prompt_tokens": 4200, "completion_tokens": 1500, "total_tokens": 5700},
        ts=(base_time + timedelta(hours=2, minutes=5)).isoformat() + "Z")
    insert_turn(conn, s2, "opencode", req6, resp6)

    req7 = make_request("opencode", "claude-sonnet-4-20250514",
        [{"role": "user", "content": "验证一下修复效果", "name": None}],
        tools=tools2, ts=(base_time + timedelta(hours=2, minutes=8)).isoformat() + "Z")
    resp7 = make_response(req7["id"],
        "修复后 30 分钟运行数据：\n- 总请求: 124,892\n- 502 错误: 0\n- P99 延迟: 23ms (修复前 P99: 120ms 含超时)\n\n✅ 问题完全修复。TIME_WAIT 连接降至 200 以下。",
        None,
        {"prompt_tokens": 800, "completion_tokens": 320, "total_tokens": 1120},
        ts=(base_time + timedelta(hours=2, minutes=9)).isoformat() + "Z")
    insert_turn(conn, s2, "opencode", req7, resp7)

    insert_session(conn, s2, "[OpenCode] API 网关 502 排查", "/infra/gateway", s2_created, (base_time + timedelta(hours=2, minutes=9)).isoformat())

    # ========== Session 3: Codex | 前端组件库升级 ==========
    s3 = "session-codex-ui-upgrade"
    s3_created = (base_time + timedelta(hours=4)).isoformat()

    tools3 = [
        {"type": "function", "function": {"name": "read_file", "description": "Read file", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}}},
        {"type": "function", "function": {"name": "write_file", "description": "Write file", "parameters": {"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}}},
        {"type": "function", "function": {"name": "install_dependency", "description": "npm install", "parameters": {"type": "object", "properties": {"package": {"type": "string"}}}}},
        {"type": "function", "function": {"name": "build_project", "description": "Build", "parameters": {"type": "object", "properties": {}}}}
    ]

    req8 = make_request("codex", "gpt-4o",
        [{"role": "user", "content": "把我们的 UI 组件库从 Tailwind 3 升级到 Tailwind 4。涉及 87 个组件文件和 12 个页面。确保所有自定义 utility 兼容。", "name": None}],
        tools=tools3, ts=(base_time + timedelta(hours=4, minutes=0)).isoformat() + "Z")
    resp8 = make_response(req8["id"],
        "开始升级。步骤：\n1. 安装 @tailwindcss/upgrade\n2. 运行自动迁移工具\n3. 修复不兼容的自定义 utility\n4. 验证构建",
        [{"id": "tool_call_005", "type": "function", "function": {"name": "install_dependency", "arguments": {"package": "@tailwindcss/upgrade@latest"}}}],
        {"prompt_tokens": 3200, "completion_tokens": 800, "total_tokens": 4000},
        ts=(base_time + timedelta(hours=4, minutes=2)).isoformat() + "Z")
    insert_turn(conn, s3, "codex", req8, resp8)

    req9 = make_request("codex", "gpt-4o",
        [{"role": "user", "content": "继续", "name": None}],
        tools=tools3, ts=(base_time + timedelta(hours=4, minutes=5)).isoformat() + "Z")
    resp9 = make_response(req9["id"],
        "迁移完成。变更摘要：\n- 修改文件: 99\n- 新增 file: 3 (theme.css, colors.css, spacing.css)\n- 删除 file: 2 (旧 tailwind.config.js, postcss.config.js)\n\n⚠️ 2 个组件需要手动调整（使用了已废弃的 @apply 语法）。",
        [{"id": "tool_call_006", "type": "function", "function": {"name": "build_project", "arguments": {}}}],
        {"prompt_tokens": 2200, "completion_tokens": 650, "total_tokens": 2850},
        ts=(base_time + timedelta(hours=4, minutes=8)).isoformat() + "Z")
    insert_turn(conn, s3, "codex", req9, resp9)

    req10 = make_request("codex", "gpt-4o",
        [{"role": "user", "content": "修复那 2 个组件", "name": None}],
        tools=tools3, ts=(base_time + timedelta(hours=4, minutes=10)).isoformat() + "Z")
    resp10 = make_response(req10["id"],
        "✅ 2 个组件已修复：\n- DataTable.tsx: 移除 @apply text-gray-50 → class=\"text-gray-500\"\n- Modal.tsx: 内联复杂 @apply → CSS module\n\n构建通过，Lighthouse 评分 98→99。",
        None,
        {"prompt_tokens": 1500, "completion_tokens": 480, "total_tokens": 1980},
        ts=(base_time + timedelta(hours=4, minutes=12)).isoformat() + "Z")
    insert_turn(conn, s3, "codex", req10, resp10)

    req11 = make_request("codex", "gpt-4o",
        [{"role": "user", "content": "完成", "name": None}],
        tools=tools3, ts=(base_time + timedelta(hours=4, minutes=14)).isoformat() + "Z")
    resp11 = make_response(req11["id"],
        "🎉 Tailwind 3→4 升级完成！\n\n**总结**:\n- 99 文件变更\n- 自定义 utility 100% 迁移\n- 构建时间: 12s → 8s (提升 33%)\n- Bundle: -15KB gzip\n- Lighthouse: 99 分\n- Storybook: 143 stories 全部通过\n\n已提交 commit: feat: migrate to Tailwind CSS 4",
        None,
        {"prompt_tokens": 900, "completion_tokens": 380, "total_tokens": 1280},
        ts=(base_time + timedelta(hours=4, minutes=15)).isoformat() + "Z")
    insert_turn(conn, s3, "codex", req11, resp11)

    insert_session(conn, s3, "[Codex] Tailwind 3→4 组件库升级", "/frontend/components", s3_created, (base_time + timedelta(hours=4, minutes=15)).isoformat())

    conn.commit()
    conn.close()

    print(f" 数据填充完成: {db_path}")
    print(f"   3 sessions, 11 turns")
    print(f"   Sessions: trae-auth-module, opencode-gateway-bug, codex-ui-upgrade")

if __name__ == "__main__":
    main()
