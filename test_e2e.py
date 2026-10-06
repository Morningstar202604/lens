#!/usr/bin/env python3
"""Full end-to-end test: MockLLM <-> Lens Proxy <-> Client."""

import json
import socket
import subprocess
import sys
import time
import os
import signal

LEN_BIN = "./target/release/lens"
DATA_DIR = "./test_e2e_data"
CERT_DIR = "./test_e2e_certs"
PROXY_PORT = 7320
MOCK_PORT = 8081

def cleanup():
    """Kill any leftover processes."""
    subprocess.run(["pkill", "-9", "-f", "lens proxy"], capture_output=True)
    subprocess.run(["pkill", "-9", "-f", "mock_llm_server"], capture_output=True)
    time.sleep(0.5)

def wait_for_port(port, timeout=5):
    """Wait until a port is accepting connections."""
    start = time.time()
    while time.time() - start < timeout:
        try:
            s = socket.socket()
            s.settimeout(1)
            s.connect(("127.0.0.1", port))
            s.close()
            return True
        except:
            time.sleep(0.3)
    return False

def send_request_through_proxy():
    """Send an LLM-style request through the lens proxy."""
    body = json.dumps({
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "Coding assistant for /home/user/jwt-refactor project."},
            {"role": "user", "content": "In src/auth/AuthGuard.ts and src/middleware/auth.ts the JWT algorithm needs upgrading from HS256 to RS256. Use verifyToken from src/utils/verifyToken.ts."}
        ],
        "temperature": 0.3
    })

    request = (
        f"POST http://127.0.0.1:{MOCK_PORT}/v1/chat/completions HTTP/1.1\r\n"
        f"Host: 127.0.0.1:{MOCK_PORT}\r\n"
        "Content-Type: application/json\r\n"
        "Authorization: Bearer sk-test-e2e-key\r\n"
        "User-Agent: trae/1.0.0\r\n"
        f"Content-Length: {len(body.encode())}\r\n"
        "Connection: close\r\n"
        "\r\n"
    ) + body

    print(f"[Client] → Sending {len(request)} bytes to proxy:{PROXY_PORT}")
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(10)
    sock.connect(("127.0.0.1", PROXY_PORT))
    sock.sendall(request.encode())

    response = b""
    while True:
        try:
            chunk = sock.recv(8192)
            if not chunk:
                break
            response += chunk
        except socket.timeout:
            break
    sock.close()

    if not response:
        print("[Client] ✗ No response received!")
        return False

    resp_str = response.decode("utf-8", errors="replace")
    header_end = resp_str.find("\r\n\r\n")
    if header_end < 0:
        print(f"[Client] ✗ Malformed response: {resp_str[:200]}")
        return False

    status = resp_str.split("\r\n")[0]
    body_str = resp_str[header_end + 4:]
    print(f"[Client] ← Status: {status}")

    try:
        data = json.loads(body_str)
        tokens = data.get("usage", {}).get("total_tokens", 0)
        content = data.get("choices", [{}])[0].get("message", {}).get("content", "")[:80]
        print(f"[Client] ← Tokens: {tokens}, Content: {content}")
        return True
    except Exception as e:
        print(f"[Client] ← Body parse error: {e}")
        return False

def check_recorded_data():
    """Verify the turn was recorded in the database."""
    import sqlite3
    db_path = os.path.join(DATA_DIR, "lens.db")
    if not os.path.exists(db_path):
        print("[Check] ✗ Database not found!")
        return False

    conn = sqlite3.connect(db_path)
    c = conn.cursor()
    c.execute("SELECT COUNT(*) FROM turns")
    turns = c.fetchone()[0]
    c.execute("SELECT COUNT(*) FROM sessions")
    sessions = c.fetchone()[0]
    c.execute("SELECT COUNT(*) FROM nodes")
    nodes = c.fetchone()[0]
    c.execute("SELECT COUNT(*) FROM edges")
    edges = c.fetchone()[0]

    print(f"[Check] DB: {turns} turns, {sessions} sessions, {nodes} nodes, {edges} edges")

    if turns > 0:
        c.execute("SELECT agent_type, json_extract(request_json, '$.model'), json_extract(response_json, '$.usage.total_tokens') FROM turns ORDER BY rowid DESC LIMIT 1")
        row = c.fetchone()
        print(f"[Check] Latest: agent={row[0]}, model={row[1]}, tokens={row[2]}")

    conn.close()
    return turns > 0

def main():
    os.chdir("/mnt/data/catpaw/home/workspace/001/lens")

    print("=" * 60)
    print("Lens E2E Test: Proxy recording → Turn persistence → Graph")
    print("=" * 60)

    # 0. Cleanup
    cleanup()
    if os.path.exists(DATA_DIR):
        import shutil
        shutil.rmtree(DATA_DIR)
    if os.path.exists(CERT_DIR):
        import shutil
        shutil.rmtree(CERT_DIR)

    # 1. Start mock LLM server
    print("\n[1/5] Starting mock LLM server...")
    with open("mock_llm_server.py", "w") as f:
        f.write("""#!/usr/bin/env python3
import json, sys
from http.server import HTTPServer, BaseHTTPRequestHandler

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8081

class H(BaseHTTPRequestHandler):
    def do_POST(self):
        cl = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(cl)
        req = json.loads(body)
        msgs = req.get("messages", [])
        print(f"[Mock] model={req.get('model')} msgs={len(msgs)}", flush=True)
        resp = {
            "id": "chatcmpl-e2e-test",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "Refactored AuthGuard.ts and auth middleware to RS256. Updated verifyToken imports."}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 2200, "completion_tokens": 800, "total_tokens": 3000}
        }
        rb = json.dumps(resp).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(rb)))
        self.end_headers()
        self.wfile.write(rb)
        print("[Mock] Sent 3000 tokens", flush=True)
    def log_message(self, *a): pass

HTTPServer(("127.0.0.1", PORT), H).serve_forever()
""")

    mock_proc = subprocess.Popen(
        [sys.executable, "mock_llm_server.py", str(MOCK_PORT)],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT
    )
    if not wait_for_port(MOCK_PORT):
        print("✗ Mock server failed to start!")
        mock_proc.kill()
        sys.exit(1)
    print("  ✓ Mock server ready")

    # 2. Start lens proxy
    print("\n[2/5] Starting lens proxy...")
    proxy_proc = subprocess.Popen(
        [LEN_BIN, "proxy", "--bind", f"127.0.0.1:{PROXY_PORT}",
         "--data-dir", DATA_DIR, "--cert-dir", CERT_DIR],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT
    )
    if not wait_for_port(PROXY_PORT):
        print("✗ Proxy failed to start!")
        print(proxy_proc.stdout.read().decode())
        proxy_proc.kill()
        mock_proc.kill()
        sys.exit(1)
    print("  ✓ Proxy ready")

    # 3. Send request through proxy
    print("\n[3/5] Sending LLM request through proxy...")
    ok = send_request_through_proxy()
    if not ok:
        print("\n✗ Request through proxy failed!")
        print("--- Proxy log ---")
        proxy_proc.terminate()
        print(proxy_proc.stdout.read().decode()[-2000:])
        proxy_proc.kill()
        mock_proc.kill()
        sys.exit(1)
    print("  ✓ Request proxied & responded")

    # 4. Small delay for async write
    time.sleep(0.5)

    # 5. Check DB
    print("\n[4/5] Verifying recorded data...")
    recorded = check_recorded_data()

    # 6. Trigger graph build via web API
    print("\n[5/5] Checking graph extraction...")
    graph_ok = False
    if recorded:
        # Start lens view to test graph API
        view_proc = subprocess.Popen(
            [LEN_BIN, "view", "--bind", "127.0.0.1:7321", "--data-dir", DATA_DIR],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT
        )
        if wait_for_port(7321, timeout=3):
            import urllib.request
            try:
                # Get sessions
                resp = urllib.request.urlopen(f"http://127.0.0.1:7321/api/sessions")
                sessions_data = json.loads(resp.read())
                sessions = sessions_data.get("sessions", [])
                print(f"  Sessions: {len(sessions)}")
                if sessions:
                    sid = sessions[0]["id"]
                    resp = urllib.request.urlopen(f"http://127.0.0.1:7321/api/sessions/{sid}/graph")
                    graph_data = json.loads(resp.read())
                    nodes = graph_data.get("nodes", [])
                    edges = graph_data.get("edges", [])
                    print(f"  Graph for '{sid}': {len(nodes)} nodes, {len(edges)} edges")
                    for n in nodes[:5]:
                        print(f"    {n['kind']:7s} | {n['label']:30s} w={n['weight']}")
                    graph_ok = len(nodes) > 0
            except Exception as e:
                print(f"  API error: {e}")
            view_proc.kill()
        else:
            print("  ✗ View server didn't start")

    # Cleanup
    proxy_proc.kill()
    mock_proc.kill()

    # Summary
    print("\n" + "=" * 60)
    if recorded and graph_ok:
        print("✓ E2E TEST PASSED: proxy recorded → turn saved → graph built")
        sys.exit(0)
    elif recorded:
        print("~ E2E PARTIAL: turn recorded but graph empty")
        sys.exit(1)
    else:
        print("✗ E2E TEST FAILED: no turn recorded")
        sys.exit(1)

if __name__ == "__main__":
    main()
