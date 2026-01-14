use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

/// Represents an active HTTP-01 challenge
#[derive(Debug, Clone)]
pub struct ActiveChallenge {
    pub token: String,
    pub key_authorization: String,
    pub domain: String,
    pub cert_name: String,
}

/// Thread-safe state for tracking active ACME challenges
#[derive(Debug, Clone, Default)]
pub struct ChallengeState {
    inner: Arc<RwLock<HashMap<String, ActiveChallenge>>>,
}

impl ChallengeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a new active challenge
    pub async fn add(&self, challenge: ActiveChallenge) {
        let mut state = self.inner.write().await;
        state.insert(challenge.token.clone(), challenge);
    }

    /// Remove a challenge by token
    pub async fn remove(&self, token: &str) {
        let mut state = self.inner.write().await;
        state.remove(token);
    }

    /// Get all active challenges
    pub async fn get_all(&self) -> Vec<ActiveChallenge> {
        let state = self.inner.read().await;
        state.values().cloned().collect()
    }

    /// Get a specific challenge by token
    pub async fn get(&self, token: &str) -> Option<ActiveChallenge> {
        let state = self.inner.read().await;
        state.get(token).cloned()
    }

    /// Check if any challenges are active
    pub async fn is_empty(&self) -> bool {
        let state = self.inner.read().await;
        state.is_empty()
    }

    /// Clear all challenges for a specific certificate
    pub async fn clear_for_cert(&self, cert_name: &str) {
        let mut state = self.inner.write().await;
        state.retain(|_, v| v.cert_name != cert_name);
    }
}
