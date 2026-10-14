// 禁用 Windows 上的控制台窗口（仅 Windows）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Command;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

fn main() {
    // 启动 tokio runtime 用于后台任务
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime failed");

    // 在后台启动 lens 服务
    rt.block_on(async {
        let _ = start_lens_backend().await;
    });

    // 启动 Tauri
    tauri::Builder::default()
        .setup(move |app| {
            let main_window = app.get_webview_window("main").expect("无 main 窗口");
            let window_arc = Arc::new(Mutex::new(main_window.clone()));

            // 等待后端就绪，然后加载面板
            tauri::async_runtime::spawn(async move {
                for _ in 0..40 {
                    if is_lens_ready().await {
                        let w = window_arc.lock().await;
                        let _ = w.eval("window.location.replace('http://localhost:7317')");
                        return;
                    }
                    sleep(Duration::from_millis(150)).await;
                }
                // fallback: 显示错误页
                let w = window_arc.lock().await;
                let html = r#"<div style="display:flex;align-items:center;justify-content:center;height:100vh;background:#090a0e;color:#eef0f4;font-family:system-ui;flex-direction:column;gap:14px;text-align:center;padding:20px">
<div style="font-size:40px;opacity:.6">⏳</div>
<div style="font-size:17px;font-weight:600">Lens 后端启动中...</div>
<div style="font-size:12px;color:#7a7f93;max-width:300px;line-height:1.5">如果长时间停留此页面，请在终端运行 <code style="background:#1c1f2b;padding:2px 6px;border-radius:4px">lens start</code> 启动后端服务</div>
</div>"#;
                let _ = w.eval(&format!("document.body.innerHTML = '{}'", html.replace('\'', "\\'")));
            });

            Ok(())
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .build(tauri::generate_context!())
        .expect("Lens 桌面启动失败");
}

/// 检测 Lens Web 服务是否就绪
async fn is_lens_ready() -> bool {
    static CLIENT: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap_or_default()
    });
    CLIENT
        .get("http://127.0.0.1:7317/api/stats")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// 在后台启动 lens 后端
async fn start_lens_backend() -> std::io::Result<()> {
    let exe_path = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new("."));
    let lens_bin = resolve_backend_binary(exe_dir);

    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("lens");
    let _ = std::fs::create_dir_all(&data_dir);

    let home_dir = dirs::home_dir().unwrap_or_default();
    let proxy_bind = "127.0.0.1:9999";
    let view_bind = "127.0.0.1:7317";

    let mut cmd = Command::new(&lens_bin);
    cmd.arg("--data-dir")
        .arg(&data_dir)
        .arg("start")
        .env("HOME", &home_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    match cmd.spawn() {
        Ok(child) => {
            eprintln!("[Lens Desktop] 后端已启动 PID={}", child.id());
            eprintln!("  代理: http://{}", proxy_bind);
            eprintln!("  面板: http://{}", view_bind);
            eprintln!("  数据: {}", data_dir.display());
            Ok(())
        }
        Err(e) => {
            eprintln!("[Lens Desktop] 无法自启动后端: {}，连接现有实例...", e);
            Ok(())
        }
    }
}

/// 解析后端二进制：优先与桌面壳同目录（Windows 为 lens.exe），其次 PATH。
fn resolve_backend_binary(exe_dir: &std::path::Path) -> std::path::PathBuf {
    let candidates = [
        exe_dir.join("lens.exe"),
        exe_dir.join("lens"),
        exe_dir.join("binaries").join("lens.exe"),
        exe_dir.join("binaries").join("lens"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return candidate;
        }
    }
    std::path::PathBuf::from("lens")
}
