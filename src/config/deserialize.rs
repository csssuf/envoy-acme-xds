/// Custom deserialization for protobuf Any messages in JSON/YAML
///
/// The standard pbjson deserializer doesn't handle the expanded form of Any messages
/// (with @type field and message fields inline). This module provides custom deserialization
/// that converts the expanded form to the binary form (type_url + encoded bytes).
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use xds_api::pb::envoy::config::cluster::v3::Cluster;
use xds_api::pb::envoy::config::listener::v3::Listener;
use xds_api::pb::envoy::extensions::filters::http::router::v3::Router;
use xds_api::pb::envoy::extensions::filters::network::http_connection_manager::v3::HttpConnectionManager;
use xds_api::pb::envoy::extensions::transport_sockets::tls::v3::SdsSecretConfig;

use crate::error::{Error, Result};

/// Minimal DownstreamTlsContext definition for deserialization
/// xds-api v0.2.0 doesn't generate this type, so we define the minimal fields needed
#[derive(Clone, Deserialize, Serialize, prost::Message)]
struct DownstreamTlsContext {
    #[prost(message, optional, tag = "1")]
    #[serde(default)]
    pub common_tls_context: Option<CommonTlsContext>,
}

/// Minimal CommonTlsContext definition
#[derive(Clone, Deserialize, Serialize, prost::Message)]
struct CommonTlsContext {
    #[prost(message, repeated, tag = "6")]
    #[serde(default)]
    pub tls_certificate_sds_secret_configs: Vec<SdsSecretConfig>,
}

/// Minimal TcpProxy definition for deserialization
/// xds-api v0.2.0 doesn't generate this type, so we define the minimal fields needed
#[derive(Clone, Deserialize, Serialize, prost::Message)]
struct TcpProxy {
    #[prost(string, tag = "1")]
    #[serde(default)]
    pub stat_prefix: String,
    #[prost(string, tag = "2")]
    #[serde(default)]
    pub cluster: String,
}

/// Deserialize a listener from JSON, handling typed_config fields with @type
pub fn deserialize_listener(value: &Value) -> Result<Listener> {
    // First, process any typed_config fields to convert them from expanded to binary form
    let processed = process_listener_value(value)?;

    // Now deserialize using standard serde
    serde_json::from_value(processed).map_err(|e| Error::ConfigDeserialize {
        item: "Listener",
        source: e,
    })
}

/// Process a listener JSON value, converting typed_config fields from expanded to binary form
fn process_listener_value(value: &Value) -> Result<Value> {
    let mut listener = value.clone();

    // Process filter chains
    if let Some(filter_chains) = listener
        .get_mut("filter_chains")
        .and_then(|v| v.as_array_mut())
    {
        for filter_chain in filter_chains {
            process_filter_chain(filter_chain)?;
        }
    }

    Ok(listener)
}

/// Process a filter chain, converting typed_config in filters
fn process_filter_chain(filter_chain: &mut Value) -> Result<()> {
    if let Some(filters) = filter_chain
        .get_mut("filters")
        .and_then(|v| v.as_array_mut())
    {
        for filter in filters {
            process_filter(filter)?;
        }
    }

    // Process transport_socket (TLS context) if present
    if let Some(transport_socket) = filter_chain.get_mut("transport_socket") {
        process_transport_socket(transport_socket)?;
    }

    Ok(())
}

/// Process transport_socket, converting DownstreamTlsContext from expanded to binary form
fn process_transport_socket(socket: &mut Value) -> Result<()> {
    if let Some(typed_config) = socket.get("typed_config")
        && let Some(type_url) = typed_config.get("@type").and_then(|v| v.as_str())
        && type_url.contains("DownstreamTlsContext")
    {
        let type_url_owned = type_url.to_string();
        let name = socket.get("name").cloned();

        // Encode DownstreamTlsContext to protobuf bytes
        let encoded = encode_downstream_tls_context(typed_config)?;

        // Replace with binary form
        *socket = serde_json::json!({
            "name": name,
            "typed_config": {
                "type_url": type_url_owned,
                "value": encoded
            }
        });
    }

    Ok(())
}

/// Encode DownstreamTlsContext from expanded JSON form to protobuf bytes
fn encode_downstream_tls_context(value: &Value) -> Result<Vec<u8>> {
    // Remove @type field for deserialization
    let mut v = value.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("@type");
    }

    // Deserialize to our minimal DownstreamTlsContext struct
    let tls_context: DownstreamTlsContext =
        serde_json::from_value(v).map_err(|e| Error::ConfigDeserialize {
            item: "DownstreamTlsContext",
            source: e,
        })?;

    // Encode to protobuf bytes
    Ok(tls_context.encode_to_vec())
}

/// Process a filter, converting typed_config from expanded to binary form
fn process_filter(filter: &mut Value) -> Result<()> {
    if let Some(typed_config) = filter.get("typed_config")
        && let Some(type_url) = typed_config.get("@type").and_then(|v| v.as_str())
    {
        let type_url_owned = type_url.to_string();
        let name = filter.get("name").cloned();

        let encoded = match type_url {
            "type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager" => {
                encode_http_connection_manager(typed_config)?
            }
            "type.googleapis.com/envoy.extensions.filters.network.tcp_proxy.v3.TcpProxy" => {
                encode_tcp_proxy(typed_config)?
            }
            _ => {
                return Err(Error::ConfigUnsupportedTypeUrl {
                    kind: "typed_config",
                    type_url: type_url.to_string(),
                });
            }
        };

        // Replace typed_config with binary form
        // In JSON, oneof fields use the field name directly (typed_config), not wrapped
        *filter = serde_json::json!({
            "name": name,
            "typed_config": {
                "type_url": type_url_owned,
                "value": encoded
            }
        });
    }

    Ok(())
}

/// Encode HttpConnectionManager from expanded JSON form to protobuf bytes
fn encode_http_connection_manager(value: &Value) -> Result<Vec<u8>> {
    // Remove @type field for deserialization
    let mut v = value.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("@type");
    }

    // Process nested typed_config in http_filters
    if let Some(http_filters) = v.get_mut("http_filters").and_then(|v| v.as_array_mut()) {
        for filter in http_filters {
            if let Some(typed_config) = filter.get("typed_config")
                && let Some(type_url) = typed_config.get("@type").and_then(|v| v.as_str())
            {
                let type_url_owned = type_url.to_string();
                let name = filter.get("name").cloned();

                let encoded = match type_url {
                    "type.googleapis.com/envoy.extensions.filters.http.router.v3.Router" => {
                        encode_router(typed_config)?
                    }
                    _ => {
                        return Err(Error::ConfigUnsupportedTypeUrl {
                            kind: "http_filter",
                            type_url: type_url.to_string(),
                        });
                    }
                };

                // HttpFilter uses typed_config directly, not wrapped in config_type
                *filter = serde_json::json!({
                    "name": name,
                    "typed_config": {
                        "type_url": type_url_owned,
                        "value": encoded
                    }
                });
            }
        }
    }

    // Now deserialize to HttpConnectionManager
    let hcm: HttpConnectionManager =
        serde_json::from_value(v).map_err(|e| Error::ConfigDeserialize {
            item: "HttpConnectionManager",
            source: e,
        })?;

    Ok(hcm.encode_to_vec())
}

/// Encode Router from expanded JSON form to protobuf bytes
fn encode_router(value: &Value) -> Result<Vec<u8>> {
    // Router is typically empty, but deserialize it properly
    let mut v = value.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("@type");
    }

    let router: Router = serde_json::from_value(v).map_err(|e| Error::ConfigDeserialize {
        item: "Router",
        source: e,
    })?;

    Ok(router.encode_to_vec())
}

/// Encode TcpProxy from expanded JSON form to protobuf bytes
fn encode_tcp_proxy(value: &Value) -> Result<Vec<u8>> {
    let mut v = value.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("@type");
    }

    let tcp_proxy: TcpProxy = serde_json::from_value(v).map_err(|e| Error::ConfigDeserialize {
        item: "TcpProxy",
        source: e,
    })?;

    Ok(tcp_proxy.encode_to_vec())
}

/// Deserialize clusters from JSON values
pub fn deserialize_clusters(values: &[Value]) -> Result<Vec<Cluster>> {
    values
        .iter()
        .map(|v| {
            serde_json::from_value(v.clone()).map_err(|e| Error::ConfigDeserialize {
                item: "Cluster",
                source: e,
            })
        })
        .collect()
}
