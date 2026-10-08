# Changelog

All notable changes to this project will be documented in this file.

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
