#!/usr/bin/env bash
# Lens Desktop Launcher — 无 Tauri 依赖的中间方案
# 用法: ./lens-app.sh
# 功能: 启动 lens 服务端，并以独立窗口模式打开浏览器

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DATA_DIR="${LENS_DATA_DIR:-$HOME/.local/share/lens}"
LENS_BIN="${LENS_BIN:-$SCRIPT_DIR/target/release/lens}"

# 如果没有预编译 binary，尝试用 cargo 构建
if [ ! -x "$LENS_BIN" ]; then
  echo "正在编译 lens..."
  ( cd "$SCRIPT_DIR" && cargo build --release )
fi

export LENS_DATA_DIR="$DATA_DIR"
export HOME="${HOME:-$(eval echo '~')}"

echo ""
echo "  🪶 Lens · AI 编程代理透镜"
echo "  ─────────────────────────"
echo ""
echo "  数据目录: $DATA_DIR"
echo ""

# 启动 lens start
"$LENS_BIN" --data-dir "$DATA_DIR" start &
LENS_PID=$!

# 等待端口就绪
for i in $(seq 1 30); do
  if curl -sf http://127.0.0.1:7317/api/stats > /dev/null 2>&1; then
    break
  fi
  sleep 0.3
done

echo "  📊 面板地址: http://127.0.0.1:7317"
echo "  🔌 代理地址: http://127.0.0.1:9999"
echo ""

# 尝试系统级独立窗口（Chromium/Chrome app mode）
URL="http://127.0.0.1:7317"
FOUND_BROWSER=""

# 1) Chrome / Chromium app mode
for CHROME in google-chrome google-chrome-stable chromium chromium-browser; do
  if command -v "$CHROME" >/dev/null 2>&1; then
    echo "  正在打开桌面窗口 ($CHROME)..."
    "$CHROME" --app="$URL" --window-size=1400,850 --class=Lens >/dev/null 2>&1 &
    FOUND_BROWSER=1
    break
  fi
done

# 2) Firefox (PWA-like)
if [ -z "$FOUND_BROWSER" ] && command -v firefox >/dev/null 2>&1; then
  echo "  正在打开桌面窗口 (firefox)..."
  firefox --new-window "$URL" >/dev/null 2>&1 &
  FOUND_BROWSER=1
fi

if [ -z "$FOUND_BROWSER" ]; then
  echo "  ⚠️  未检测到 Chrome/Chromium/Firefox"
  echo "  请手动打开: $URL"
fi

echo ""
echo "  💡 在你的 AI 编程环境中设置代理:"
echo "     export HTTPS_PROXY=http://127.0.0.1:9999"
echo ""
# 等待 lens 进程
wait $LENS_PID 2>/dev/null
