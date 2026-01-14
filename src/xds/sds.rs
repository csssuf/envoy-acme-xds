use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use prost::Message;
use tonic::{Request, Response, Status, Streaming};
use tracing::{debug, info};
use xds_api::pb::envoy::service::discovery::v3::{
    DeltaDiscoveryRequest, DeltaDiscoveryResponse, DiscoveryRequest, DiscoveryResponse,
};
use xds_api::pb::envoy::service::secret::v3::secret_discovery_service_server::SecretDiscoveryService;
use xds_api::pb::google::protobuf::Any;

use super::state::XdsState;

const SECRET_TYPE_URL: &str =
    "type.googleapis.com/envoy.extensions.transport_sockets.tls.v3.Secret";

pub struct SdsService {
    state: Arc<XdsState>,
}

impl SdsService {
    pub fn new(state: Arc<XdsState>) -> Self {
        Self { state }
    }

    async fn build_response(
        state: &XdsState,
        resource_names: &[String],
    ) -> Result<DiscoveryResponse, Status> {
        let version = state.version_info().await;
        let secrets = state.get_secrets().await;

        // Filter to requested secrets, or return all if no specific request
        let resources: Vec<Any> = if resource_names.is_empty() {
            secrets
                .into_iter()
                .map(|s| Any {
                    type_url: SECRET_TYPE_URL.to_string(),
                    value: s.encode_to_vec(),
                })
                .collect()
        } else {
            secrets
                .into_iter()
                .filter(|s| resource_names.contains(&s.name))
                .map(|s| Any {
                    type_url: SECRET_TYPE_URL.to_string(),
                    value: s.encode_to_vec(),
                })
                .collect()
        };

        debug!(
            version = %version,
            num_resources = resources.len(),
            requested = ?resource_names,
            "Building SDS response"
        );

        Ok(DiscoveryResponse {
            version_info: version,
            resources,
            type_url: SECRET_TYPE_URL.to_string(),
            ..Default::default()
        })
    }
}

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl SecretDiscoveryService for SdsService {
    type DeltaSecretsStream = ResponseStream<DeltaDiscoveryResponse>;
    type StreamSecretsStream = ResponseStream<DiscoveryResponse>;

    async fn stream_secrets(
        &self,
        _request: Request<Streaming<DiscoveryRequest>>,
    ) -> Result<Response<Self::StreamSecretsStream>, Status> {
        info!("New SDS stream connection");

        let state = self.state.clone();
        let mut rx = state.subscribe();

        // We'll track requested resources from the stream
        // For now, return all secrets on each update
        let resource_names: Vec<String> = Vec::new();

        let stream = async_stream::stream! {
            // Send initial response
            let resp = Self::build_response(&state, &resource_names).await;
            yield resp;

            // Wait for updates
            while rx.recv().await.is_ok() {
                let resp = Self::build_response(&state, &resource_names).await;
                yield resp;
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    async fn delta_secrets(
        &self,
        _request: Request<Streaming<DeltaDiscoveryRequest>>,
    ) -> Result<Response<Self::DeltaSecretsStream>, Status> {
        Err(Status::unimplemented("Delta SDS not supported"))
    }

    async fn fetch_secrets(
        &self,
        request: Request<DiscoveryRequest>,
    ) -> Result<Response<DiscoveryResponse>, Status> {
        let req = request.into_inner();
        Self::build_response(&self.state, &req.resource_names)
            .await
            .map(Response::new)
    }
}
