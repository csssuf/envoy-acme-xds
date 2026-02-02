use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{Notify, RwLock, broadcast};
use tracing::debug;
use xds_api::pb::envoy::config::cluster::v3::Cluster;
use xds_api::pb::envoy::config::listener::v3::Listener;
use xds_api::pb::envoy::extensions::transport_sockets::tls::v3::Secret;

use crate::envoy::build_tls_secret;

/// Central state for all xDS resources
pub struct XdsState {
    /// Monotonically increasing version for change detection
    version: RwLock<u64>,
    /// Listeners (merged workload + ACME)
    listeners: RwLock<Vec<Listener>>,
    /// Clusters from workload config
    clusters: RwLock<Vec<Cluster>>,
    /// TLS certificates (from ACME)
    secrets: RwLock<HashMap<String, Secret>>,
    /// Notify channel for subscribers when state changes
    notify: broadcast::Sender<u64>,
    /// Tracks whether an LDS stream connection has been observed
    lds_connected: AtomicBool,
    /// Notify waiters when LDS connects
    lds_notify: Notify,
}

impl XdsState {
    pub fn new() -> Arc<Self> {
        let (notify, _) = broadcast::channel(16);
        Arc::new(Self {
            version: RwLock::new(0),
            listeners: RwLock::new(Vec::new()),
            clusters: RwLock::new(Vec::new()),
            secrets: RwLock::new(HashMap::new()),
            notify,
            lds_connected: AtomicBool::new(false),
            lds_notify: Notify::new(),
        })
    }

    /// Get current version string for xDS responses
    pub async fn version_info(&self) -> String {
        self.version.read().await.to_string()
    }

    /// Bump version and notify subscribers
    async fn bump_version(&self) -> u64 {
        let mut version = self.version.write().await;
        *version += 1;
        let new_version = *version;
        debug!(version = new_version, "XDS state version bumped");
        let _ = self.notify.send(new_version);
        new_version
    }

    /// Subscribe to state change notifications
    pub fn subscribe(&self) -> broadcast::Receiver<u64> {
        self.notify.subscribe()
    }

    /// Notify subscribers of a change (without bumping version)
    /// Used when challenges change but we want to trigger a rebuild
    pub fn notify_change(&self) {
        let _ = self.notify.send(0); // Version 0 signals rebuild needed
    }

    /// Mark the first LDS stream connection
    pub fn mark_lds_connected(&self) {
        if !self.lds_connected.swap(true, Ordering::SeqCst) {
            self.lds_notify.notify_waiters();
        }
    }

    /// Wait for an LDS stream connection to be observed
    pub async fn wait_for_lds(&self) {
        let notified = self.lds_notify.notified();
        if self.lds_connected.load(Ordering::SeqCst) {
            return;
        }
        notified.await;
    }

    /// Update listeners and bump version
    pub async fn update_listeners(&self, listeners: Vec<Listener>) {
        let mut state = self.listeners.write().await;
        *state = listeners;
        drop(state);
        self.bump_version().await;
    }

    /// Update clusters and bump version
    pub async fn update_clusters(&self, clusters: Vec<Cluster>) {
        let mut state = self.clusters.write().await;
        *state = clusters;
        drop(state);
        self.bump_version().await;
    }

    /// Update a single secret and bump version
    pub async fn update_secret(&self, name: &str, cert_chain_pem: String, private_key_pem: String) {
        let secret = build_tls_secret(name, &cert_chain_pem, &private_key_pem);
        let mut secrets = self.secrets.write().await;
        secrets.insert(name.to_string(), secret);
        drop(secrets);
        self.bump_version().await;
    }

    /// Get all current listeners
    pub async fn get_listeners(&self) -> Vec<Listener> {
        self.listeners.read().await.clone()
    }

    /// Get all current clusters
    pub async fn get_clusters(&self) -> Vec<Cluster> {
        self.clusters.read().await.clone()
    }

    /// Get all current secrets
    pub async fn get_secrets(&self) -> Vec<Secret> {
        self.secrets.read().await.values().cloned().collect()
    }

    /// Get a specific secret by name
    pub async fn get_secret(&self, name: &str) -> Option<Secret> {
        self.secrets.read().await.get(name).cloned()
    }
}

impl Default for XdsState {
    fn default() -> Self {
        let (notify, _) = broadcast::channel(16);
        Self {
            version: RwLock::new(0),
            listeners: RwLock::new(Vec::new()),
            clusters: RwLock::new(Vec::new()),
            secrets: RwLock::new(HashMap::new()),
            notify,
            lds_connected: AtomicBool::new(false),
            lds_notify: Notify::new(),
        }
    }
}
