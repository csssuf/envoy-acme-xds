use std::path::PathBuf;

use chrono::{DateTime, Utc};
use instant_acme::AccountCredentials;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Manages filesystem storage for ACME account and certificates
pub struct CertificateStorage {
    base_dir: PathBuf,
}

/// Stored certificate data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCert {
    pub cert_chain_pem: String,
    pub private_key_pem: String,
    pub domains: Vec<String>,
    pub not_after: DateTime<Utc>,
}

/// Certificate metadata stored alongside the cert
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CertMeta {
    domains: Vec<String>,
    not_after: DateTime<Utc>,
}

impl CertificateStorage {
    /// Create a new storage manager for the given directory
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Initialize the storage directory structure
    pub async fn init(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.base_dir).await?;
        tokio::fs::create_dir_all(self.certs_dir()).await?;
        Ok(())
    }

    fn certs_dir(&self) -> PathBuf {
        self.base_dir.join("certs")
    }

    fn account_path(&self) -> PathBuf {
        self.base_dir.join("account.json")
    }

    fn cert_dir(&self, name: &str) -> PathBuf {
        self.certs_dir().join(name)
    }

    fn cert_path(&self, name: &str) -> PathBuf {
        self.cert_dir(name).join("cert.pem")
    }

    fn key_path(&self, name: &str) -> PathBuf {
        self.cert_dir(name).join("key.pem")
    }

    fn meta_path(&self, name: &str) -> PathBuf {
        self.cert_dir(name).join("meta.json")
    }

    /// Load ACME account credentials from storage
    pub async fn load_account(&self) -> Result<Option<AccountCredentials>> {
        let path = self.account_path();
        if !path.exists() {
            return Ok(None);
        }
        let content = tokio::fs::read_to_string(&path).await?;
        let creds: AccountCredentials = serde_json::from_str(&content)?;
        Ok(Some(creds))
    }

    /// Save ACME account credentials to storage
    pub async fn save_account(&self, creds: &AccountCredentials) -> Result<()> {
        let content = serde_json::to_string_pretty(creds)?;
        tokio::fs::write(self.account_path(), content).await?;
        Ok(())
    }

    /// Load a certificate from storage by name
    pub async fn load_certificate(&self, name: &str) -> Result<Option<StoredCert>> {
        let cert_path = self.cert_path(name);
        let key_path = self.key_path(name);
        let meta_path = self.meta_path(name);

        if !cert_path.exists() || !key_path.exists() || !meta_path.exists() {
            return Ok(None);
        }

        let cert_chain_pem = tokio::fs::read_to_string(&cert_path).await?;
        let private_key_pem = tokio::fs::read_to_string(&key_path).await?;
        let meta_content = tokio::fs::read_to_string(&meta_path).await?;
        let meta: CertMeta = serde_json::from_str(&meta_content)?;

        Ok(Some(StoredCert {
            cert_chain_pem,
            private_key_pem,
            domains: meta.domains,
            not_after: meta.not_after,
        }))
    }

    /// Save a certificate to storage
    pub async fn save_certificate(&self, name: &str, cert: &StoredCert) -> Result<()> {
        let cert_dir = self.cert_dir(name);
        tokio::fs::create_dir_all(&cert_dir).await?;

        // Write certificate chain
        tokio::fs::write(self.cert_path(name), &cert.cert_chain_pem).await?;

        // Write private key with restricted permissions
        let key_path = self.key_path(name);
        tokio::fs::write(&key_path, &cert.private_key_pem).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(&key_path, perms).await?;
        }

        // Write metadata
        let meta = CertMeta {
            domains: cert.domains.clone(),
            not_after: cert.not_after,
        };
        let meta_content = serde_json::to_string_pretty(&meta)?;
        tokio::fs::write(self.meta_path(name), meta_content).await?;

        Ok(())
    }
}

/// Parse expiry date from PEM certificate
pub fn parse_certificate_expiry(pem: &str) -> Result<DateTime<Utc>> {
    use x509_parser::prelude::*;

    let (_, pem_block) = parse_x509_pem(pem.as_bytes())
        .map_err(|e| Error::X509(format!("Failed to parse PEM: {:?}", e)))?;

    let (_, cert) = X509Certificate::from_der(&pem_block.contents)
        .map_err(|e| Error::X509(format!("Failed to parse certificate: {:?}", e)))?;

    let not_after = cert.validity().not_after;
    let timestamp = not_after.timestamp();

    DateTime::from_timestamp(timestamp, 0)
        .ok_or_else(|| Error::X509("Invalid timestamp".to_string()))
}
