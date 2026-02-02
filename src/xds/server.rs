use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::stream;
use tokio::net::UnixListener;
use tokio::sync::oneshot;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use tracing::info;
use xds_api::pb::envoy::service::cluster::v3::cluster_discovery_service_server::ClusterDiscoveryServiceServer;
use xds_api::pb::envoy::service::listener::v3::listener_discovery_service_server::ListenerDiscoveryServiceServer;
use xds_api::pb::envoy::service::secret::v3::secret_discovery_service_server::SecretDiscoveryServiceServer;

use crate::error::{Error, Result};

use super::cds::CdsService;
use super::lds::LdsService;
use super::sds::SdsService;
use super::state::XdsState;

/// XDS gRPC server
pub struct XdsServer {
    state: Arc<XdsState>,
}

impl XdsServer {
    pub fn new(state: Arc<XdsState>) -> Self {
        Self { state }
    }

    /// Run the XDS server on one or more Unix domain sockets
    pub async fn run(
        self,
        listeners: Vec<UnixListener>,
        cleanup_paths: Vec<PathBuf>,
        shutdown: impl std::future::Future<Output = ()>,
        ready: Option<oneshot::Sender<()>>,
    ) -> Result<()> {
        let incoming = stream::select_all(
            listeners
                .into_iter()
                .map(UnixListenerStream::new)
                .collect::<Vec<_>>(),
        );

        if let Some(ready) = ready {
            let _ = ready.send(());
        }

        // Create services
        let lds_service = LdsService::new(self.state.clone());
        let cds_service = CdsService::new(self.state.clone());
        let sds_service = SdsService::new(self.state.clone());

        // Build and run server
        Server::builder()
            .add_service(ListenerDiscoveryServiceServer::new(lds_service))
            .add_service(ClusterDiscoveryServiceServer::new(cds_service))
            .add_service(SecretDiscoveryServiceServer::new(sds_service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await?;

        // Clean up socket files created by this process
        for socket_path in cleanup_paths {
            if socket_path.exists() {
                let _ = std::fs::remove_file(socket_path);
            }
        }

        Ok(())
    }

    pub fn bind_unix_socket(socket_path: &Path, socket_permissions: u32) -> Result<UnixListener> {
        // Remove existing socket file if it exists
        if socket_path.exists() {
            std::fs::remove_file(socket_path).map_err(|e| Error::IoPath {
                action: "remove existing socket",
                path: socket_path.to_path_buf(),
                source: e,
            })?;
        }

        // Create parent directory if needed
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::IoPath {
                action: "create socket parent directory",
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        // Bind to Unix socket
        let uds = UnixListener::bind(socket_path).map_err(|e| Error::IoPath {
            action: "bind unix socket",
            path: socket_path.to_path_buf(),
            source: e,
        })?;

        // Set socket permissions to allow other processes to connect
        let permissions = std::fs::Permissions::from_mode(socket_permissions);
        std::fs::set_permissions(socket_path, permissions).map_err(|e| Error::IoPath {
            action: "set socket permissions",
            path: socket_path.to_path_buf(),
            source: e,
        })?;

        info!(
            path = %socket_path.display(),
            permissions = format!("{:#o}", socket_permissions),
            "XDS server listening on Unix socket"
        );

        Ok(uds)
    }
}
