use instant_acme::{Account, NewAccount};
use tokio::time::{Duration, sleep};
use tracing::{info, warn};

use crate::error::Result;

use super::storage::CertificateStorage;

/// Manages ACME account creation and restoration
pub struct AcmeAccount;

impl AcmeAccount {
    /// Load an existing account or create a new one
    /// Retries on connection failure to handle ACME server startup delays
    pub async fn load_or_create(
        storage: &CertificateStorage,
        directory_url: &str,
    ) -> Result<Account> {
        const MAX_RETRIES: u32 = 5;
        const INITIAL_DELAY_MS: u64 = 1000;

        for attempt in 1..=MAX_RETRIES {
            match Self::try_load_or_create(storage, directory_url).await {
                Ok(account) => return Ok(account),
                Err(e) if attempt < MAX_RETRIES => {
                    let delay = INITIAL_DELAY_MS * 2_u64.pow(attempt - 1);
                    warn!(
                        attempt,
                        max_retries = MAX_RETRIES,
                        delay_ms = delay,
                        error = %e,
                        "Failed to connect to ACME server, retrying..."
                    );
                    sleep(Duration::from_millis(delay)).await;
                }
                Err(e) => return Err(e),
            }
        }

        unreachable!("Loop should have returned by now")
    }

    async fn try_load_or_create(
        storage: &CertificateStorage,
        directory_url: &str,
    ) -> Result<Account> {
        // Try to load existing account
        if let Some(credentials) = storage.load_account().await? {
            info!("Restoring existing ACME account");
            let account = Account::from_credentials(credentials).await?;
            return Ok(account);
        }

        // Create new account
        info!("Creating new ACME account");
        let (account, credentials) = Account::create(
            &NewAccount {
                contact: &[],
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            directory_url,
            None,
        )
        .await?;

        // Save credentials
        storage.save_account(&credentials).await?;
        info!("ACME account created and saved");

        Ok(account)
    }
}
