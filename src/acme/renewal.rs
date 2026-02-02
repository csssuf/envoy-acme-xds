use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use instant_acme::Account;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::CertificateConfig;
use crate::error::Result;
use crate::xds::XdsState;

use super::challenge::ChallengeState;
use super::order::CertificateOrder;
use super::storage::{CertificateStorage, StoredCert, parse_certificate_expiry};

/// Manages background certificate renewal
pub struct RenewalManager {
    storage: Arc<CertificateStorage>,
    account: Arc<RwLock<Account>>,
    challenge_state: ChallengeState,
    xds_state: Arc<XdsState>,
    certificates: Vec<CertificateConfig>,
    renewal_threshold_days: i64,
}

impl RenewalManager {
    pub fn new(
        storage: Arc<CertificateStorage>,
        account: Arc<RwLock<Account>>,
        challenge_state: ChallengeState,
        xds_state: Arc<XdsState>,
        certificates: Vec<CertificateConfig>,
    ) -> Self {
        Self {
            storage,
            account,
            challenge_state,
            xds_state,
            certificates,
            renewal_threshold_days: 30,
        }
    }

    /// Run the renewal check loop
    pub async fn run(self, check_interval: Duration) {
        info!(
            ?check_interval,
            threshold_days = self.renewal_threshold_days,
            "Starting certificate renewal manager"
        );

        loop {
            if let Err(e) = self.check_and_renew().await {
                error!("Renewal check failed: {}", e);
            }

            tokio::time::sleep(check_interval).await;
        }
    }

    /// Check all certificates and renew if needed
    pub async fn check_and_renew(&self) -> Result<()> {
        debug!("Checking certificates for renewal");

        for cert_config in &self.certificates {
            match self.check_certificate(&cert_config.name).await {
                Ok(needs_renewal) => {
                    if needs_renewal {
                        info!(name = cert_config.name, "Certificate needs renewal");
                        if let Err(e) = self.renew_certificate(cert_config).await {
                            error!(
                                name = cert_config.name,
                                error = %e,
                                "Failed to renew certificate"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        name = cert_config.name,
                        error = %e,
                        "Failed to check certificate"
                    );
                    // If we can't check, try to issue
                    if let Err(e) = self.renew_certificate(cert_config).await {
                        error!(
                            name = cert_config.name,
                            error = %e,
                            "Failed to issue certificate"
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a certificate needs renewal
    async fn check_certificate(&self, name: &str) -> Result<bool> {
        let cert = match self.storage.load_certificate(name).await? {
            Some(c) => c,
            None => {
                // Certificate doesn't exist, needs to be issued
                return Ok(true);
            }
        };

        let now = Utc::now();
        let days_until_expiry = (cert.not_after - now).num_days();

        debug!(
            name,
            days_until_expiry,
            threshold = self.renewal_threshold_days,
            "Certificate expiry check"
        );

        Ok(days_until_expiry < self.renewal_threshold_days)
    }

    /// Renew a specific certificate
    async fn renew_certificate(&self, cert_config: &CertificateConfig) -> Result<()> {
        let account = self.account.read().await;
        let xds_state = self.xds_state.clone();

        let (cert_chain_pem, private_key_pem, _) = CertificateOrder::order(
            &account,
            &cert_config.name,
            &cert_config.domains,
            &self.challenge_state,
            move || {
                // Trigger xDS rebuild when challenges are ready
                xds_state.notify_change();
            },
        )
        .await?;

        // Parse expiry from certificate
        let not_after = parse_certificate_expiry(&cert_chain_pem)?;

        // Store certificate
        let stored_cert = StoredCert {
            cert_chain_pem: cert_chain_pem.clone(),
            private_key_pem: private_key_pem.clone(),
            domains: cert_config.domains.clone(),
            not_after,
        };

        self.storage
            .save_certificate(&cert_config.name, &stored_cert)
            .await?;

        // Update xDS state
        self.xds_state
            .update_secret(&cert_config.name, cert_chain_pem, private_key_pem)
            .await;

        info!(name = cert_config.name, "Certificate renewed successfully");

        Ok(())
    }

    /// Initial certificate issuance for all configured certificates
    pub async fn initial_issuance(&self) -> Result<()> {
        info!("Performing initial certificate check/issuance");

        for cert_config in &self.certificates {
            // Check if certificate exists and is valid
            if let Ok(Some(cert)) = self.storage.load_certificate(&cert_config.name).await {
                let now = Utc::now();
                let days_until_expiry = (cert.not_after - now).num_days();

                if days_until_expiry > 0 {
                    info!(
                        name = cert_config.name,
                        days_until_expiry, "Loading existing certificate"
                    );

                    // Load into xDS state
                    self.xds_state
                        .update_secret(&cert_config.name, cert.cert_chain_pem, cert.private_key_pem)
                        .await;
                    continue;
                }
            }

            // Certificate doesn't exist or is expired, issue new one
            info!(name = cert_config.name, "Issuing new certificate");
            if let Err(e) = self.renew_certificate(cert_config).await {
                error!(
                    name = cert_config.name,
                    error = %e,
                    "Failed to issue certificate on startup"
                );
            }
        }

        Ok(())
    }
}
