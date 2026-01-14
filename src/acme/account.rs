use instant_acme::{Account, NewAccount};
use tracing::info;

use crate::error::Result;

use super::storage::CertificateStorage;

/// Manages ACME account creation and restoration
pub struct AcmeAccount;

impl AcmeAccount {
    /// Load an existing account or create a new one
    pub async fn load_or_create(
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
