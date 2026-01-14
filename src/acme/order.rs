use std::time::Duration;

use instant_acme::{
    Account, AuthorizationStatus, ChallengeType, Identifier, NewOrder, Order, OrderStatus,
};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ECDSA_P256_SHA256};
use tracing::{debug, info};

use crate::error::{Error, Result};

use super::challenge::{ActiveChallenge, ChallengeState};

/// Handles certificate ordering workflow
pub struct CertificateOrder;

impl CertificateOrder {
    /// Order a new certificate for the given domains
    ///
    /// Returns the certificate chain PEM and private key PEM
    pub async fn order(
        account: &Account,
        cert_name: &str,
        domains: &[String],
        challenge_state: &ChallengeState,
        on_challenges_ready: impl Fn() + Send,
    ) -> Result<(String, String, KeyPair)> {
        info!(cert_name, ?domains, "Starting certificate order");

        // Create order
        let identifiers: Vec<Identifier> = domains
            .iter()
            .map(|d| Identifier::Dns(d.clone()))
            .collect();

        let mut order = account
            .new_order(&NewOrder {
                identifiers: &identifiers,
            })
            .await?;

        // Process authorizations
        let authorizations = order.authorizations().await?;
        let mut challenges_to_complete = Vec::new();

        for authz in &authorizations {
            debug!(
                identifier = ?authz.identifier,
                status = ?authz.status,
                "Processing authorization"
            );

            match authz.status {
                AuthorizationStatus::Pending => {
                    // Find HTTP-01 challenge
                    let challenge = authz
                        .challenges
                        .iter()
                        .find(|c| c.r#type == ChallengeType::Http01)
                        .ok_or_else(|| {
                            Error::ChallengeFailed("No HTTP-01 challenge available".to_string())
                        })?;

                    let domain = match &authz.identifier {
                        Identifier::Dns(d) => d.clone(),
                    };

                    // Get key authorization
                    let key_auth = order.key_authorization(challenge);

                    // Add to challenge state
                    let active_challenge = ActiveChallenge {
                        token: challenge.token.clone(),
                        key_authorization: key_auth.as_str().to_string(),
                        domain,
                        cert_name: cert_name.to_string(),
                    };

                    challenge_state.add(active_challenge).await;
                    challenges_to_complete.push(challenge.url.clone());
                }
                AuthorizationStatus::Valid => {
                    debug!("Authorization already valid");
                }
                status => {
                    return Err(Error::ChallengeFailed(format!(
                        "Unexpected authorization status: {:?}",
                        status
                    )));
                }
            }
        }

        // Notify that challenges are ready (triggers xDS update)
        if !challenges_to_complete.is_empty() {
            on_challenges_ready();

            // Small delay to allow xDS to propagate
            tokio::time::sleep(Duration::from_secs(2)).await;

            // Signal challenges ready to ACME server
            for url in &challenges_to_complete {
                order.set_challenge_ready(url).await?;
            }

            // Wait for challenges to complete
            Self::wait_for_order_ready(&mut order).await?;
        }

        // Clean up challenges
        challenge_state.clear_for_cert(cert_name).await;

        // Generate CSR
        let (csr_der, key_pair) = Self::generate_csr(domains)?;

        // Finalize order
        order.finalize(&csr_der).await?;

        // Wait for certificate
        Self::wait_for_order_ready(&mut order).await?;

        // Get certificate
        let cert_chain_pem = order
            .certificate()
            .await?
            .ok_or_else(|| Error::ChallengeFailed("No certificate returned".to_string()))?;

        info!(cert_name, "Certificate issued successfully");

        Ok((cert_chain_pem, key_pair.serialize_pem(), key_pair))
    }

    /// Wait for order to reach ready/valid state
    async fn wait_for_order_ready(order: &mut Order) -> Result<()> {
        let mut delay = Duration::from_secs(1);
        let max_delay = Duration::from_secs(30);
        let max_attempts = 30;

        for attempt in 1..=max_attempts {
            let state = order.state();
            debug!(attempt, status = ?state.status, "Checking order status");

            match state.status {
                OrderStatus::Ready | OrderStatus::Valid => {
                    return Ok(());
                }
                OrderStatus::Invalid => {
                    return Err(Error::ChallengeFailed("Order became invalid".to_string()));
                }
                OrderStatus::Pending | OrderStatus::Processing => {
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(max_delay);
                    // Refresh order state
                    order.refresh().await?;
                }
            }
        }

        Err(Error::ChallengeFailed(
            "Order did not complete in time".to_string(),
        ))
    }

    /// Generate a CSR for the given domains
    fn generate_csr(domains: &[String]) -> Result<(Vec<u8>, KeyPair)> {
        let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;

        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, domains[0].clone());

        let mut params = CertificateParams::new(domains.to_vec())?;
        params.distinguished_name = distinguished_name;

        let csr = params.serialize_request(&key_pair)?;
        Ok((csr.der().to_vec(), key_pair))
    }
}
