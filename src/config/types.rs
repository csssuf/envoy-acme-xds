use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Root configuration structure
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub meta: MetaConfig,
    pub certificates: Vec<CertificateConfig>,
    #[serde(default)]
    pub envoy: EnvoyWorkloadConfig,
}

/// Metadata configuration
#[derive(Debug, Clone, Deserialize)]
pub struct MetaConfig {
    /// Directory for storing account data, keys, and certificates
    pub storage_dir: PathBuf,

    /// ACME directory URL (defaults to Let's Encrypt production)
    #[serde(default = "default_acme_directory")]
    pub acme_directory_url: String,

    /// Unix socket path for xDS server
    pub socket_path: PathBuf,

    /// Unix socket permissions in octal (e.g., 0o777 for world-writable)
    /// Defaults to 0o777 to allow any process to connect
    #[serde(default = "default_socket_permissions")]
    pub socket_permissions: u32,
}

fn default_socket_permissions() -> u32 {
    0o777
}

fn default_acme_directory() -> String {
    "https://acme-v02.api.letsencrypt.org/directory".to_string()
}

/// Certificate configuration - defines a certificate to be issued
#[derive(Debug, Clone, Deserialize)]
pub struct CertificateConfig {
    /// Name used for SDS secret reference and storage directory
    pub name: String,

    /// List of domains to include on the certificate
    pub domains: Vec<String>,
}

/// Workload Envoy configuration - mirrors static_resources structure
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EnvoyWorkloadConfig {
    #[serde(default)]
    pub listeners: Vec<serde_json::Value>,

    #[serde(default)]
    pub clusters: Vec<serde_json::Value>,
}
