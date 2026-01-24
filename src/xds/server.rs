use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use tracing::info;
use xds_api::pb::envoy::service::cluster::v3::cluster_discovery_service_server::ClusterDiscoveryServiceServer;
use xds_api::pb::envoy::service::listener::v3::listener_discovery_service_server::ListenerDiscoveryServiceServer;
use xds_api::pb::envoy::service::secret::v3::secret_discovery_service_server::SecretDiscoveryServiceServer;

use crate::error::Result;

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

    /// Run the XDS server on a Unix domain socket
    ///
    /// The `socket_permissions` parameter specifies the Unix permissions for the socket file
    /// (e.g., 0o777 for world-writable, allowing any process to connect).
    pub async fn run(
        self,
        socket_path: &Path,
        socket_permissions: u32,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<()> {
        // Remove existing socket file if it exists
        if socket_path.exists() {
            std::fs::remove_file(socket_path)?;
        }

        // Create parent directory if needed
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Bind to Unix socket
        let uds = UnixListener::bind(socket_path)?;

        // Set socket permissions to allow other processes to connect
        let permissions = std::fs::Permissions::from_mode(socket_permissions);
        std::fs::set_permissions(socket_path, permissions)?;

        let uds_stream = UnixListenerStream::new(uds);

        info!(
            path = %socket_path.display(),
            permissions = format!("{:#o}", socket_permissions),
            "XDS server listening on Unix socket"
        );

        // Create services
        let lds_service = LdsService::new(self.state.clone());
        let cds_service = CdsService::new(self.state.clone());
        let sds_service = SdsService::new(self.state.clone());

        // Build and run server
        Server::builder()
            .add_service(ListenerDiscoveryServiceServer::new(lds_service))
            .add_service(ClusterDiscoveryServiceServer::new(cds_service))
            .add_service(SecretDiscoveryServiceServer::new(sds_service))
            .serve_with_incoming_shutdown(uds_stream, shutdown)
            .await?;

        // Clean up socket file
        if socket_path.exists() {
            let _ = std::fs::remove_file(socket_path);
        }

        Ok(())
    }
}
