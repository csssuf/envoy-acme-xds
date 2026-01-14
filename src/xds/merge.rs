use prost::Message;
use tracing::debug;
use xds_api::pb::envoy::config::cluster::v3::Cluster;
use xds_api::pb::envoy::config::core::v3::{Address, SocketAddress};
use xds_api::pb::envoy::config::listener::v3::{filter::ConfigType, Filter, FilterChain, Listener};
use xds_api::pb::envoy::config::route::v3::{Route, RouteConfiguration, VirtualHost};
use xds_api::pb::envoy::extensions::filters::http::router::v3::Router;
use xds_api::pb::envoy::extensions::filters::network::http_connection_manager::v3::{
    http_connection_manager::RouteSpecifier, http_filter, HttpConnectionManager, HttpFilter,
};
use xds_api::pb::google::protobuf::Any;

use crate::acme::ChallengeState;
use crate::config::EnvoyWorkloadConfig;
use crate::envoy::{build_acme_challenge_route, listener_port};
use crate::error::{Error, Result};

const HTTP_CONNECTION_MANAGER_TYPE_URL: &str =
    "type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager";
const ROUTER_TYPE_URL: &str =
    "type.googleapis.com/envoy.extensions.filters.http.router.v3.Router";

/// Handles merging of workload config with ACME challenge routes
pub struct ConfigMerger;

impl ConfigMerger {
    /// Parse workload listeners from JSON values
    pub fn parse_listeners(config: &EnvoyWorkloadConfig) -> Result<Vec<Listener>> {
        config
            .listeners
            .iter()
            .map(|v| {
                serde_json::from_value(v.clone())
                    .map_err(|e| Error::Config(format!("Invalid listener config: {}", e)))
            })
            .collect()
    }

    /// Parse workload clusters from JSON values
    pub fn parse_clusters(config: &EnvoyWorkloadConfig) -> Result<Vec<Cluster>> {
        config
            .clusters
            .iter()
            .map(|v| {
                serde_json::from_value(v.clone())
                    .map_err(|e| Error::Config(format!("Invalid cluster config: {}", e)))
            })
            .collect()
    }

    /// Merge ACME challenge routes into listeners
    pub async fn merge_listeners(
        workload_listeners: Vec<Listener>,
        challenge_state: &ChallengeState,
    ) -> Vec<Listener> {
        let challenges = challenge_state.get_all().await;

        if challenges.is_empty() {
            return workload_listeners;
        }

        // Build ACME challenge routes
        let acme_routes: Vec<Route> = challenges
            .iter()
            .map(|c| build_acme_challenge_route(&c.token, &c.key_authorization))
            .collect();

        debug!(
            num_challenges = acme_routes.len(),
            "Merging ACME challenge routes"
        );

        // Find port 80 listener or create one
        let mut listeners = workload_listeners;
        let port_80_idx = listeners.iter().position(|l| listener_port(l) == Some(80));

        match port_80_idx {
            Some(idx) => {
                // Prepend ACME routes to existing listener
                listeners[idx] = Self::prepend_routes_to_listener(&listeners[idx], acme_routes);
            }
            None => {
                // Create new port 80 listener for ACME challenges
                let acme_listener = Self::create_acme_listener(acme_routes);
                listeners.push(acme_listener);
            }
        }

        listeners
    }

    /// Prepend routes to an existing listener's HTTP connection manager
    fn prepend_routes_to_listener(listener: &Listener, routes: Vec<Route>) -> Listener {
        let mut listener = listener.clone();

        for filter_chain in &mut listener.filter_chains {
            for filter in &mut filter_chain.filters {
                if filter.name == "envoy.filters.network.http_connection_manager" {
                    if let Some(ConfigType::TypedConfig(ref mut typed_config)) = filter.config_type {
                        if typed_config.type_url == HTTP_CONNECTION_MANAGER_TYPE_URL {
                            // Decode HCM
                            if let Ok(mut hcm) =
                                HttpConnectionManager::decode(typed_config.value.as_slice())
                            {
                                // Modify route config
                                if let Some(RouteSpecifier::RouteConfig(ref mut route_config)) =
                                    hcm.route_specifier
                                {
                                    Self::prepend_routes_to_route_config(route_config, &routes);
                                }

                                // Re-encode
                                typed_config.value = hcm.encode_to_vec();
                            }
                        }
                    }
                }
            }
        }

        listener
    }

    /// Prepend routes to a route configuration
    fn prepend_routes_to_route_config(route_config: &mut RouteConfiguration, routes: &[Route]) {
        // Add routes to the first virtual host with wildcard domain, or create one
        let wildcard_vh = route_config
            .virtual_hosts
            .iter_mut()
            .find(|vh| vh.domains.iter().any(|d| d == "*"));

        match wildcard_vh {
            Some(vh) => {
                // Prepend routes
                let mut new_routes = routes.to_vec();
                new_routes.extend(vh.routes.drain(..));
                vh.routes = new_routes;
            }
            None => {
                // Create new virtual host for ACME
                let vh = VirtualHost {
                    name: "acme-challenges".to_string(),
                    domains: vec!["*".to_string()],
                    routes: routes.to_vec(),
                    ..Default::default()
                };
                route_config.virtual_hosts.insert(0, vh);
            }
        }
    }

    /// Create a new listener for ACME challenges on port 80
    fn create_acme_listener(routes: Vec<Route>) -> Listener {
        let route_config = RouteConfiguration {
            name: "acme_routes".to_string(),
            virtual_hosts: vec![VirtualHost {
                name: "acme-challenges".to_string(),
                domains: vec!["*".to_string()],
                routes,
                ..Default::default()
            }],
            ..Default::default()
        };

        let hcm = HttpConnectionManager {
            stat_prefix: "acme".to_string(),
            route_specifier: Some(RouteSpecifier::RouteConfig(route_config)),
            http_filters: vec![HttpFilter {
                name: "envoy.filters.http.router".to_string(),
                config_type: Some(http_filter::ConfigType::TypedConfig(Any {
                    type_url: ROUTER_TYPE_URL.to_string(),
                    value: Router::default().encode_to_vec(),
                })),
                ..Default::default()
            }],
            ..Default::default()
        };

        Listener {
            name: "acme-http".to_string(),
            address: Some(Address {
                address: Some(
                    xds_api::pb::envoy::config::core::v3::address::Address::SocketAddress(
                        SocketAddress {
                            address: "0.0.0.0".to_string(),
                            port_specifier: Some(
                                xds_api::pb::envoy::config::core::v3::socket_address::PortSpecifier::PortValue(80),
                            ),
                            ..Default::default()
                        },
                    ),
                ),
            }),
            filter_chains: vec![FilterChain {
                filters: vec![Filter {
                    name: "envoy.filters.network.http_connection_manager".to_string(),
                    config_type: Some(ConfigType::TypedConfig(Any {
                        type_url: HTTP_CONNECTION_MANAGER_TYPE_URL.to_string(),
                        value: hcm.encode_to_vec(),
                    })),
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}
