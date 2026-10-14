//! 配置文件（TOML）。
//!
//! 优先级:CLI 参数 > 配置文件 > 默认值。
//! 默认查找顺序:显式 `--config` → 数据目录下 `config.toml` → `~/.lens/config.toml`。

use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub proxy: ProxyConfig,
    pub view: ViewConfig,
    pub storage: StorageConfig,
    pub agents: AgentsConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ProxyConfig {
    pub bind: String,
    pub cert_dir: String,
    pub upstream_ca: Option<PathBuf>,
    pub insecure_upstream: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:9999".to_string(),
            cert_dir: "certs".to_string(),
            upstream_ca: None,
            insecure_upstream: false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ViewConfig {
    pub bind: String,
    pub open_browser: bool,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:7317".to_string(),
            open_browser: true,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub data_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    pub opencode: AgentToggle,
    pub codex: AgentToggle,
    pub claude_code: AgentToggle,
    /// 未识别 agent 是否录制。
    pub others: AgentToggle,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct AgentToggle {
    pub enabled: bool,
}

impl Default for AgentToggle {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Config {
    /// 加载配置。`explicit` 为 CLI 指定的路径（不存在时报错）；
    /// 未指定时按默认顺序探测，全部缺失则返回默认值。
    pub fn load(explicit: Option<&str>) -> Result<Self> {
        let (path, required) = match explicit {
            Some(p) => (Some(PathBuf::from(p)), true),
            None => (Self::probe_default_path(), false),
        };
        let Some(path) = path else {
            return Ok(Self::default());
        };
        if !path.exists() {
            if required {
                anyhow::bail!("配置文件不存在: {}", path.display());
            }
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("读取配置文件失败 {}: {}", path.display(), e))?;
        let config: Config = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("配置文件解析失败 {}: {}", path.display(), e))?;
        Ok(config)
    }

    fn probe_default_path() -> Option<PathBuf> {
        if let Ok(data_dir) = crate::engine::resolve_data_dir(None) {
            let candidate = data_dir.join("config.toml");
            if candidate.exists() {
                return Some(candidate);
            }
        }
        if let Ok(Some(home)) = homedir::my_home() {
            let candidate = home.join(".lens").join("config.toml");
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    /// CLI > 配置 > 默认:代理监听地址。
    pub fn effective_proxy_bind(&self, cli: Option<&str>) -> String {
        cli.map(str::to_string)
            .unwrap_or_else(|| self.proxy.bind.clone())
    }

    /// CLI > 配置 > 默认:证书目录。
    pub fn effective_cert_dir(&self, cli: Option<&str>) -> String {
        cli.map(str::to_string)
            .unwrap_or_else(|| self.proxy.cert_dir.clone())
    }

    /// CLI > 配置 > 默认:面板监听地址。
    pub fn effective_view_bind(&self, cli: Option<&str>) -> String {
        cli.map(str::to_string)
            .unwrap_or_else(|| self.view.bind.clone())
    }

    /// CLI > 配置:上游 TLS 策略。
    pub fn effective_upstream_tls(
        &self,
        cli_ca: Option<&str>,
        cli_insecure: bool,
    ) -> crate::proxy::UpstreamTls {
        if cli_insecure {
            eprintln!("  ⚠️  已跳过上游 TLS 证书校验（仅调试用途，勿在生产使用）");
            return crate::proxy::UpstreamTls::Insecure;
        }
        if let Some(path) = cli_ca {
            return crate::proxy::UpstreamTls::ExtraCa(path.into());
        }
        if self.proxy.insecure_upstream {
            eprintln!(
                "  ⚠️  已跳过上游 TLS 证书校验（config.proxy.insecure_upstream；仅调试用途）"
            );
            return crate::proxy::UpstreamTls::Insecure;
        }
        if let Some(path) = &self.proxy.upstream_ca {
            return crate::proxy::UpstreamTls::ExtraCa(path.clone());
        }
        crate::proxy::UpstreamTls::SystemRoots
    }

    pub fn agent_filter(&self) -> crate::proxy::AgentFilter {
        crate::proxy::AgentFilter {
            opencode: self.agents.opencode.enabled,
            codex: self.agents.codex.enabled,
            claude_code: self.agents.claude_code.enabled,
            other: self.agents.others.enabled,
        }
    }
}
