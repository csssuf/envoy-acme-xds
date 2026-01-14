mod acme;
mod config;
mod envoy;
mod error;
mod xds;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::signal;
use tokio::sync::RwLock;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use acme::{AcmeAccount, ChallengeState, CertificateStorage, RenewalManager};
use config::{load_config, Config};
use xds::{ConfigMerger, XdsServer, XdsState};

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "envoy_acme_xds=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <config.yaml>", args[0]);
        std::process::exit(1);
    }

    let config_path = PathBuf::from(&args[1]);

    // Load configuration
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    // Run the server
    if let Err(e) = run(config).await {
        error!("Server error: {}", e);
        std::process::exit(1);
    }
}

async fn run(config: Config) -> error::Result<()> {
    info!(
        storage_dir = %config.meta.storage_dir.display(),
        socket_path = %config.meta.socket_path.display(),
        acme_directory = %config.meta.acme_directory_url,
        num_certificates = config.certificates.len(),
        "Starting envoy-acme-xds"
    );

    // Initialize storage
    let storage = Arc::new(CertificateStorage::new(config.meta.storage_dir.clone()));
    storage.init().await?;

    // Initialize XDS state
    let xds_state = XdsState::new();

    // Initialize challenge state (shared between ACME and XDS)
    let challenge_state = ChallengeState::new();

    // Load or create ACME account
    let account = AcmeAccount::load_or_create(&storage, &config.meta.acme_directory_url).await?;
    let account = Arc::new(RwLock::new(account));

    // Parse and set initial workload configuration
    let workload_listeners = ConfigMerger::parse_listeners(&config.envoy)?;
    let workload_clusters = ConfigMerger::parse_clusters(&config.envoy)?;

    // Merge initial listeners (no challenges yet)
    let merged_listeners =
        ConfigMerger::merge_listeners(workload_listeners.clone(), &challenge_state).await;

    xds_state.update_listeners(merged_listeners).await;
    xds_state.update_clusters(workload_clusters).await;

    // Create renewal manager
    let renewal_manager = RenewalManager::new(
        storage.clone(),
        account.clone(),
        challenge_state.clone(),
        xds_state.clone(),
        config.certificates.clone(),
    );

    // Perform initial certificate issuance
    renewal_manager.initial_issuance().await?;

    // Spawn background state updater (rebuilds listeners when challenges change)
    let state_updater_xds = xds_state.clone();
    let state_updater_challenges = challenge_state.clone();
    let state_updater_workload = workload_listeners.clone();
    tokio::spawn(async move {
        let mut rx = state_updater_xds.subscribe();
        while rx.recv().await.is_ok() {
            let merged = ConfigMerger::merge_listeners(
                state_updater_workload.clone(),
                &state_updater_challenges,
            )
            .await;
            // Update without triggering another notification (would cause loop)
            // The state update itself will bump version
            let listeners = state_updater_xds.get_listeners().await;
            if listeners != merged {
                // Only update if changed
                state_updater_xds.update_listeners(merged).await;
            }
        }
    });

    // Spawn renewal background task
    tokio::spawn(async move {
        renewal_manager.run(Duration::from_secs(3600)).await;
    });

    // Setup shutdown signal
    let shutdown = async {
        let ctrl_c = async {
            signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => info!("Received Ctrl+C, shutting down"),
            _ = terminate => info!("Received SIGTERM, shutting down"),
        }
    };

    // Run XDS server
    let server = XdsServer::new(xds_state);
    server.run(&config.meta.socket_path, shutdown).await?;

    info!("Shutdown complete");
    Ok(())
}
