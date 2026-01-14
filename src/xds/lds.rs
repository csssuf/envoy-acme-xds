use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use prost::Message;
use tonic::{Request, Response, Status, Streaming};
use tracing::{debug, info};
use xds_api::pb::envoy::service::discovery::v3::{
    DeltaDiscoveryRequest, DeltaDiscoveryResponse, DiscoveryRequest, DiscoveryResponse,
};
use xds_api::pb::envoy::service::listener::v3::listener_discovery_service_server::ListenerDiscoveryService;
use xds_api::pb::google::protobuf::Any;

use super::state::XdsState;

const LISTENER_TYPE_URL: &str = "type.googleapis.com/envoy.config.listener.v3.Listener";

pub struct LdsService {
    state: Arc<XdsState>,
}

impl LdsService {
    pub fn new(state: Arc<XdsState>) -> Self {
        Self { state }
    }

    async fn build_response(state: &XdsState) -> Result<DiscoveryResponse, Status> {
        let version = state.version_info().await;
        let listeners = state.get_listeners().await;

        let resources: Vec<Any> = listeners
            .into_iter()
            .map(|l| Any {
                type_url: LISTENER_TYPE_URL.to_string(),
                value: l.encode_to_vec(),
            })
            .collect();

        debug!(
            version = %version,
            num_resources = resources.len(),
            "Building LDS response"
        );

        Ok(DiscoveryResponse {
            version_info: version,
            resources,
            type_url: LISTENER_TYPE_URL.to_string(),
            ..Default::default()
        })
    }
}

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl ListenerDiscoveryService for LdsService {
    type DeltaListenersStream = ResponseStream<DeltaDiscoveryResponse>;
    type StreamListenersStream = ResponseStream<DiscoveryResponse>;

    async fn stream_listeners(
        &self,
        _request: Request<Streaming<DiscoveryRequest>>,
    ) -> Result<Response<Self::StreamListenersStream>, Status> {
        info!("New LDS stream connection");

        let state = self.state.clone();
        let mut rx = state.subscribe();

        let stream = async_stream::stream! {
            // Send initial response
            let resp = Self::build_response(&state).await;
            yield resp;

            // Wait for updates
            while rx.recv().await.is_ok() {
                let resp = Self::build_response(&state).await;
                yield resp;
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    async fn delta_listeners(
        &self,
        _request: Request<Streaming<DeltaDiscoveryRequest>>,
    ) -> Result<Response<Self::DeltaListenersStream>, Status> {
        Err(Status::unimplemented("Delta LDS not supported"))
    }

    async fn fetch_listeners(
        &self,
        _request: Request<DiscoveryRequest>,
    ) -> Result<Response<DiscoveryResponse>, Status> {
        Self::build_response(&self.state).await.map(Response::new)
    }
}
