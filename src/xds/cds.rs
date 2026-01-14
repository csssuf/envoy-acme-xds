use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use prost::Message;
use tonic::{Request, Response, Status, Streaming};
use tracing::{debug, info};
use xds_api::pb::envoy::service::cluster::v3::cluster_discovery_service_server::ClusterDiscoveryService;
use xds_api::pb::envoy::service::discovery::v3::{
    DeltaDiscoveryRequest, DeltaDiscoveryResponse, DiscoveryRequest, DiscoveryResponse,
};
use xds_api::pb::google::protobuf::Any;

use super::state::XdsState;

const CLUSTER_TYPE_URL: &str = "type.googleapis.com/envoy.config.cluster.v3.Cluster";

pub struct CdsService {
    state: Arc<XdsState>,
}

impl CdsService {
    pub fn new(state: Arc<XdsState>) -> Self {
        Self { state }
    }

    async fn build_response(state: &XdsState) -> Result<DiscoveryResponse, Status> {
        let version = state.version_info().await;
        let clusters = state.get_clusters().await;

        let resources: Vec<Any> = clusters
            .into_iter()
            .map(|c| Any {
                type_url: CLUSTER_TYPE_URL.to_string(),
                value: c.encode_to_vec(),
            })
            .collect();

        debug!(
            version = %version,
            num_resources = resources.len(),
            "Building CDS response"
        );

        Ok(DiscoveryResponse {
            version_info: version,
            resources,
            type_url: CLUSTER_TYPE_URL.to_string(),
            ..Default::default()
        })
    }
}

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl ClusterDiscoveryService for CdsService {
    type DeltaClustersStream = ResponseStream<DeltaDiscoveryResponse>;
    type StreamClustersStream = ResponseStream<DiscoveryResponse>;

    async fn stream_clusters(
        &self,
        _request: Request<Streaming<DiscoveryRequest>>,
    ) -> Result<Response<Self::StreamClustersStream>, Status> {
        info!("New CDS stream connection");

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

    async fn delta_clusters(
        &self,
        _request: Request<Streaming<DeltaDiscoveryRequest>>,
    ) -> Result<Response<Self::DeltaClustersStream>, Status> {
        Err(Status::unimplemented("Delta CDS not supported"))
    }

    async fn fetch_clusters(
        &self,
        _request: Request<DiscoveryRequest>,
    ) -> Result<Response<DiscoveryResponse>, Status> {
        Self::build_response(&self.state).await.map(Response::new)
    }
}
