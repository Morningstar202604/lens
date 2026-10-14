//! TLS 证书管理
//!
//! 动态生成 CA 证书和站点证书，用于 MITM 代理 HTTPS 证书替换。

use anyhow::{Context, Result};
use rcgen::{BasicConstraints, Certificate, CertificateParams, KeyPair, KeyUsagePurpose, SanType};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use time::{Duration, OffsetDateTime};
use tracing::{info, warn};

/// 缓存的证书链 + 私钥（私钥用 Arc 包裹因为 PrivateKeyDer 未实现 Clone）
type CachedCert = (Vec<CertificateDer<'static>>, Arc<PrivateKeyDer<'static>>);

/// CA + 主机证书签发管理器
/// 主机证书缓存上限;超出即整体清空(重建代价为毫秒级)。
pub const CERT_CACHE_LIMIT: usize = 512;

pub struct CertManager {
    ca_key_pair: KeyPair,
    ca_cert: Certificate,
    ca_cert_der: CertificateDer<'static>,
    cache: Mutex<HashMap<String, CachedCert>>,
}

impl CertManager {
    pub fn init(cert_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(cert_dir)?;
        let ca_cert_path = cert_dir.join("ca.crt");
        let ca_key_path = cert_dir.join("ca.key");

        let (ca_key_pair, ca_cert) = if ca_cert_path.exists() && ca_key_path.exists() {
            info!("加载已有 CA 证书");
            let key_pem = std::fs::read_to_string(&ca_key_path)?;
            let ca_pem = std::fs::read_to_string(&ca_cert_path)?;
            let kp = KeyPair::from_pem(&key_pem)
                .map_err(|e| anyhow::anyhow!("CA 密钥加载失败: {}", e))?;
            let params = CertificateParams::from_ca_cert_pem(&ca_pem)
                .map_err(|e| anyhow::anyhow!("CA 证书解析失败: {}", e))?;
            let cert = params
                .self_signed(&kp)
                .map_err(|e| anyhow::anyhow!("CA 证书重建失败: {}", e))?;
            (kp, cert)
        } else {
            info!("生成新 CA 证书");
            Self::generate_ca(&ca_cert_path, &ca_key_path)?
        };

        let ca_cert_der = ca_cert.der().clone();

        warn!("⚠️  CA 证书位置: {}", ca_cert_path.display());
        warn!("   首次使用请在系统/浏览器中信任此证书。");
        warn!("   macOS: 双击 ca.crt → 钥匙串 → 始终信任");
        warn!("   Chrome: chrome://settings/certificates → 受信任的根证书颁发机构");

        Ok(Self {
            ca_key_pair,
            ca_cert,
            ca_cert_der,
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn build_server_config(&self, host: &str) -> Result<Arc<ServerConfig>> {
        let (cert_chain, key_der) = self.get_or_create_cert(host)?;
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()?
                .with_no_client_auth()
                .with_single_cert(cert_chain, (*key_der).clone_key())
                .map_err(|e| anyhow::anyhow!("TLS 配置错误: {}", e))?;
        Ok(Arc::new(config))
    }

    fn get_or_create_cert(&self, host: &str) -> Result<CachedCert> {
        {
            let cache = self
                .cache
                .lock()
                .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
            if let Some(c) = cache.get(host) {
                return Ok(c.clone());
            }
        }

        let cert = self.generate_cert(host)?;

        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|e| anyhow::anyhow!("锁错误: {}", e))?;
            if cache.len() >= CERT_CACHE_LIMIT {
                warn!("证书缓存达到上限 {} 条,整体清空重建", CERT_CACHE_LIMIT);
                cache.clear();
            }
            cache.insert(host.to_string(), cert.clone());
        }

        Ok(cert)
    }

    fn generate_cert(&self, host: &str) -> Result<CachedCert> {
        let mut params = CertificateParams::new(vec![host.to_string()])
            .map_err(|e| anyhow::anyhow!("证书参数创建失败: {}", e))?;
        params.subject_alt_names = vec![SanType::DnsName(
            host.try_into()
                .map_err(|e| anyhow::anyhow!("域名格式错误: {}", e))?,
        )];

        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + Duration::days(365);

        let key_pair = KeyPair::generate().map_err(|e| anyhow::anyhow!("密钥生成失败: {}", e))?;

        let cert = params
            .signed_by(&key_pair, &self.ca_cert, &self.ca_key_pair)
            .map_err(|e| anyhow::anyhow!("证书签名失败: {}", e))?;

        let cert_der_bytes = cert.der().to_vec();
        let ca_der_bytes = self.ca_cert_der.as_ref().to_vec();

        let mut cert_chain = vec![CertificateDer::from(cert_der_bytes)];
        cert_chain.push(CertificateDer::from(ca_der_bytes));

        let key_der = PrivateKeyDer::try_from(key_pair.serialize_der())
            .map_err(|e| anyhow::anyhow!("密钥转换失败: {}", e))?;

        Ok((cert_chain, Arc::new(key_der)))
    }

    fn generate_ca(cert_path: &Path, key_path: &Path) -> Result<(KeyPair, Certificate)> {
        let key_pair =
            KeyPair::generate().map_err(|e| anyhow::anyhow!("CA 密钥生成失败: {}", e))?;

        let mut params = CertificateParams::new(vec!["Lens Local Proxy CA".to_string()])
            .map_err(|e| anyhow::anyhow!("CA 参数创建失败: {}", e))?;
        params.is_ca = rcgen::IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];

        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + Duration::days(3650);

        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| anyhow::anyhow!("CA 自签名失败: {}", e))?;

        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();

        std::fs::write(cert_path, &cert_pem)
            .with_context(|| format!("写入 CA 证书失败: {}", cert_path.display()))?;
        std::fs::write(key_path, &key_pem)
            .with_context(|| format!("写入 CA 密钥失败: {}", key_path.display()))?;
        // CA 私钥必须设为仅当前用户可读（0600），防止其他用户窃取签伪证书
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| format!("设置 CA 密钥权限失败: {}", key_path.display()))?;
        }

        Ok((key_pair, cert))
    }

    /// 当前缓存的主机证书数量(诊断与测试用)。
    pub fn cached_cert_count(&self) -> usize {
        self.cache.lock().map(|c| c.len()).unwrap_or(0)
    }

    pub fn ca_cert_pem(&self) -> String {
        self.ca_cert.pem()
    }

    pub fn ca_cert_der(&self) -> &CertificateDer<'static> {
        &self.ca_cert_der
    }
}

pub type CertManagerArc = Arc<CertManager>;
