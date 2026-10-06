# Lens 桌面版 (Tauri v2)

Lens 桌面版 = Tauri v2 + 系统原生 Webview + Rust 后端

## 架构

```
┌──────────────────────────────────────┐
│  lens-desktop (Tauri App)           │
│  ┌─────────────────────────────────┐ │
│  │  Webview (系统原生 WebView)      │ │
│  │  加载 http://localhost:7317      │ │
│  └──────────────┬──────────────────┘ │
│                 │                     │
│  ┌──────────────▼──────────────────┐ │
│  │  后端: 启动 lens subprocess      │ │
│  │  lens start --data-dir ~/.data  │ │
│  └─────────────────────────────────┘ │
└──────────────────────────────────────┘
```

## 系统要求

### macOS
- Xcode Command Line Tools
- macOS 11+

### Linux
```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libappindicator3-dev librsvg2-dev pkg-config
```

### Windows
- WebView2 Runtime (Win11 自带)
- Microsoft Visual C++ Build Tools

## 构建

```bash
# 安装 tauri-cli
cargo install tauri-cli

# 开发模式（热重载）
cargo tauri dev

# 打包发布
cargo tauri build
```

输出:
- Linux: `src-tauri/target/release/bundle/deb/` 或 `appimage/`
- macOS: `src-tauri/target/release/bundle/macos/`
- Windows: `src-tauri/target/release/bundle/nsis/`

## 目录结构

```
src-tauri/
├── Cargo.toml          # Tauri 桌面依赖
├── build.rs            # tauri-build 集成
├── tauri.conf.json     # Tauri 配置
├── capabilities/       # 权限声明
├── icons/              # 桌面图标
└── src/
    └── main.rs         # 桌面端入口
```

## 运行流程

1. `lens-desktop` 启动
2. 后台 spawn `lens start` 子进程（代理 + Web 服务）
3. Webview 窗口打开，等待 `localhost:7317/api/stats` 响应
4. 服务就绪后，location.replace 到面板页面
5. 用户看到完整的 Lens 界面，如同原生桌面软件
