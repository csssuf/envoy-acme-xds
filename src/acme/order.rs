use std::time::Duration;

use instant_acme::{
    Account, AuthorizationStatus, Challenge, ChallengeType, Identifier, NewOrder, Order,
    OrderStatus, Problem,
};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ECDSA_P256_SHA256};
use tracing::{debug, error, info, warn};

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
                AuthorizationStatus::Invalid
                | AuthorizationStatus::Revoked
                | AuthorizationStatus::Expired => {
                    let summary = Self::summarize_challenge_errors(&authz.challenges);
                    Self::log_challenge_errors(cert_name, &authz.identifier, &authz.challenges);
                    challenge_state.clear_for_cert(cert_name).await;
                    return Err(Error::ChallengeFailed(match summary {
                        Some(summary) => format!(
                            "Authorization {:?} for {:?} failed: {}",
                            authz.status, authz.identifier, summary
                        ),
                        None => format!(
                            "Authorization {:?} for {:?} failed without problem details",
                            authz.status, authz.identifier
                        ),
                    }));
                }
            }
        }

        // Notify that challenges are ready (triggers xDS update)
        let challenge_result = if !challenges_to_complete.is_empty() {
            on_challenges_ready();

            // Small delay to allow xDS to propagate
            tokio::time::sleep(Duration::from_secs(2)).await;

            // Signal challenges ready to ACME server
            for url in &challenges_to_complete {
                order.set_challenge_ready(url).await?;
            }

            // Wait for challenges to complete
            Self::wait_for_order_ready(&mut order, cert_name, domains).await
        } else {
            Ok(())
        };

        // Clean up challenges even on failure
        challenge_state.clear_for_cert(cert_name).await;
        challenge_result?;

        // Generate CSR
        let (csr_der, key_pair) = Self::generate_csr(domains)?;

        // Finalize order
        order.finalize(&csr_der).await?;

        // Wait for certificate
        Self::wait_for_order_ready(&mut order, cert_name, domains).await?;

        // Get certificate
        let cert_chain_pem = order
            .certificate()
            .await?
            .ok_or_else(|| Error::ChallengeFailed("No certificate returned".to_string()))?;

        info!(cert_name, "Certificate issued successfully");

        Ok((cert_chain_pem, key_pair.serialize_pem(), key_pair))
    }

    /// Wait for order to reach ready/valid state
    async fn wait_for_order_ready(
        order: &mut Order,
        cert_name: &str,
        domains: &[String],
    ) -> Result<()> {
        let mut delay = Duration::from_secs(1);
        let max_delay = Duration::from_secs(30);
        let max_attempts = 30;
        let mut last_status = None;
        let mut last_error: Option<Problem> = None;

        for attempt in 1..=max_attempts {
            let (status, error) = {
                let state = order.state();
                debug!(attempt, status = ?state.status, "Checking order status");
                (state.status, state.error.clone())
            };
            last_status = Some(status);
            last_error = error.clone();

            match status {
                OrderStatus::Ready | OrderStatus::Valid => {
                    return Ok(());
                }
                OrderStatus::Invalid => {
                    Self::log_order_problem(cert_name, domains, error.as_ref());
                    if let Err(err) =
                        Self::log_authorization_problems(order, cert_name).await
                    {
                        warn!(
                            cert_name,
                            error = ?err,
                            "Failed to load authorizations for invalid order"
                        );
                    }
                    let summary = error.as_ref().map(Self::format_problem);
                    return Err(Error::ChallengeFailed(match summary {
                        Some(summary) => format!("Order became invalid: {summary}"),
                        None => "Order became invalid without problem details".to_string(),
                    }));
                }
                OrderStatus::Pending | OrderStatus::Processing => {
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(max_delay);
                    // Refresh order state
                    order.refresh().await?;
                }
            }
        }

        Self::log_timeout_problem(cert_name, domains, last_status, last_error.as_ref());
        Err(Error::ChallengeFailed(match last_status {
            Some(status) => match last_error.as_ref() {
                Some(problem) => format!(
                    "Order did not complete in time (status={:?}): {}",
                    status,
                    Self::format_problem(problem)
                ),
                None => format!("Order did not complete in time (status={:?})", status),
            },
            None => "Order did not complete in time".to_string(),
        }))
    }

    async fn log_authorization_problems(order: &mut Order, cert_name: &str) -> Result<()> {
        let authorizations = order.authorizations().await?;
        let mut logged = false;

        for authz in &authorizations {
            if Self::log_challenge_errors(cert_name, &authz.identifier, &authz.challenges) {
                logged = true;
            }
        }

        if !logged {
            debug!(cert_name, "No challenge errors reported for invalid order");
        }

        Ok(())
    }

    fn log_order_problem(cert_name: &str, domains: &[String], problem: Option<&Problem>) {
        if let Some(problem) = problem {
            error!(
                cert_name,
                ?domains,
                problem_detail = ?problem.detail,
                problem_type = ?problem.r#type,
                problem_status = ?problem.status,
                "Order became invalid"
            );
        } else {
            error!(cert_name, ?domains, "Order became invalid");
        }
    }

    fn log_timeout_problem(
        cert_name: &str,
        domains: &[String],
        last_status: Option<OrderStatus>,
        problem: Option<&Problem>,
    ) {
        if let Some(problem) = problem {
            warn!(
                cert_name,
                ?domains,
                status = ?last_status,
                problem_detail = ?problem.detail,
                problem_type = ?problem.r#type,
                problem_status = ?problem.status,
                "Order did not complete in time"
            );
        } else {
            warn!(
                cert_name,
                ?domains,
                status = ?last_status,
                "Order did not complete in time"
            );
        }
    }

    fn log_challenge_errors(
        cert_name: &str,
        identifier: &Identifier,
        challenges: &[Challenge],
    ) -> bool {
        let mut logged = false;

        for challenge in challenges {
            if let Some(problem) = &challenge.error {
                logged = true;
                error!(
                    cert_name,
                    identifier = ?identifier,
                    challenge_type = ?challenge.r#type,
                    challenge_status = ?challenge.status,
                    problem_detail = ?problem.detail,
                    problem_type = ?problem.r#type,
                    problem_status = ?problem.status,
                    "ACME challenge error reported"
                );
            }
        }

        logged
    }

    fn summarize_challenge_errors(challenges: &[Challenge]) -> Option<String> {
        let mut summaries = Vec::new();

        for challenge in challenges {
            if let Some(problem) = &challenge.error {
                summaries.push(Self::format_problem(problem));
            }
        }

        if summaries.is_empty() {
            None
        } else {
            Some(summaries.join("; "))
        }
    }

    fn format_problem(problem: &Problem) -> String {
        let detail = problem.detail.as_deref().unwrap_or("unknown detail");
        let mut parts = vec![detail.to_string()];

        if let Some(problem_type) = &problem.r#type {
            parts.push(format!("type={problem_type}"));
        }
        if let Some(status) = problem.status {
            parts.push(format!("status={status}"));
        }

        parts.join(", ")
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
