#!/usr/bin/env python3
"""Mock LLM server for e2e test — returns a fake OpenAI chat completions response."""

import json
from http.server import HTTPServer, BaseHTTPRequestHandler

class MockLLMHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length)
        print(f"[MockLLM] Received {content_length} bytes")

        try:
            req = json.loads(body)
            model = req.get("model", "unknown")
            msg_count = len(req.get("messages", []))
            print(f"[MockLLM] model={model}, messages={msg_count}")
        except Exception as e:
            print(f"[MockLLM] parse error: {e}")

        # Build a fake response with tool_calls
        response = {
            "id": "chatcmpl-mock-e2e-test",
            "object": "chat.completion",
            "created": 1727000000,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "我已经检查了 src/auth/AuthGuard.ts 和 src/middleware/auth.ts 的问题。需要更新 JWT 验证逻辑来使用新的 verifyToken 函数。"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 2500,
                "completion_tokens": 800,
                "total_tokens": 3300
            }
        }

        resp_bytes = json.dumps(response).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(resp_bytes)))
        self.end_headers()
        self.wfile.write(resp_bytes)
        print("[MockLLM] Sent 3300 token response")

    def log_message(self, format, *args):
        pass  # Suppress default logging

if __name__ == "__main__":
    server = HTTPServer(("127.0.0.1", 8080), MockLLMHandler)
    print("[MockLLM] Listening on :8080")
    server.handle_request()  # Handle one request then exit
    print("[MockLLM] Done")
