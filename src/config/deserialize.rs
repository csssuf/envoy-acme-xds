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
    // Normalize google.protobuf.Duration strings ("5s", "1.5s") into {seconds, nanos}
    // objects so nested serde deserialization accepts them anywhere they appear.
    let mut value = value.clone();
    convert_duration_strings(&mut value);

    // Process any typed_config fields to convert them from expanded to binary form
    let processed = process_listener_value(&value)?;

    // Now deserialize using standard serde
    serde_json::from_value(processed).map_err(|e| Error::ConfigDeserialize {
        item: "Listener",
        source: e,
    })
}

/// Recursively walk a JSON value and convert any string matching the
/// `google.protobuf.Duration` canonical JSON form (e.g. "5s", "1.5s", "-0.25s")
/// into the `{"seconds": N, "nanos": M}` form accepted by pbjson-generated
/// deserializers.
fn convert_duration_strings(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for v in map.values_mut() {
                convert_duration_strings(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                convert_duration_strings(v);
            }
        }
        Value::String(s) => {
            if let Some((seconds, nanos)) = parse_duration_string(s) {
                *value = serde_json::json!({
                    "seconds": seconds,
                    "nanos": nanos,
                });
            }
        }
        _ => {}
    }
}

/// Parse a protobuf Duration JSON string. Returns (seconds, nanos) on success.
/// Format: optional `-`, one or more digits, optional `.` + 1-9 digits, trailing `s`.
fn parse_duration_string(s: &str) -> Option<(i64, i32)> {
    let rest = s.strip_suffix('s')?;
    let (negative, rest) = match rest.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let (sec_str, frac_str, had_dot) = match rest.split_once('.') {
        Some((a, b)) => (a, b, true),
        None => (rest, "", false),
    };
    if sec_str.is_empty() || !sec_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if had_dot && (frac_str.is_empty() || frac_str.len() > 9) {
        return None;
    }
    if !frac_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut seconds: i64 = sec_str.parse().ok()?;
    let mut nanos: i32 = if frac_str.is_empty() {
        0
    } else {
        let mut padded = String::from(frac_str);
        while padded.len() < 9 {
            padded.push('0');
        }
        padded.parse().ok()?
    };
    if negative {
        seconds = seconds.checked_neg()?;
        nanos = -nanos;
    }
    Some((seconds, nanos))
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
            let mut v = v.clone();
            convert_duration_strings(&mut v);
            serde_json::from_value(v).map_err(|e| Error::ConfigDeserialize {
                item: "Cluster",
                source: e,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_integer_seconds() {
        assert_eq!(parse_duration_string("5s"), Some((5, 0)));
        assert_eq!(parse_duration_string("0s"), Some((0, 0)));
        assert_eq!(parse_duration_string("300s"), Some((300, 0)));
    }

    #[test]
    fn parses_fractional_seconds() {
        assert_eq!(parse_duration_string("1.5s"), Some((1, 500_000_000)));
        assert_eq!(parse_duration_string("0.25s"), Some((0, 250_000_000)));
        assert_eq!(parse_duration_string("0.000000001s"), Some((0, 1)));
    }

    #[test]
    fn parses_negative_duration() {
        assert_eq!(parse_duration_string("-1s"), Some((-1, 0)));
        assert_eq!(parse_duration_string("-1.5s"), Some((-1, -500_000_000)));
    }

    #[test]
    fn rejects_non_duration_strings() {
        assert_eq!(parse_duration_string("hello"), None);
        assert_eq!(parse_duration_string("5"), None);
        assert_eq!(parse_duration_string("s"), None);
        assert_eq!(parse_duration_string("1ms"), None);
        assert_eq!(parse_duration_string("1.s"), None);
        assert_eq!(parse_duration_string(".5s"), None);
        assert_eq!(parse_duration_string("1.1234567890s"), None);
        assert_eq!(parse_duration_string("1 s"), None);
    }

    #[test]
    fn converts_durations_in_nested_value() {
        let mut v = serde_json::json!({
            "name": "http_listener",
            "stream_idle_timeout": "300s",
            "nested": {
                "request_timeout": "1.5s",
                "untouched": "not-a-duration",
            },
            "list": ["5s", "hello"],
        });
        convert_duration_strings(&mut v);
        assert_eq!(
            v,
            serde_json::json!({
                "name": "http_listener",
                "stream_idle_timeout": {"seconds": 300, "nanos": 0},
                "nested": {
                    "request_timeout": {"seconds": 1, "nanos": 500_000_000},
                    "untouched": "not-a-duration",
                },
                "list": [{"seconds": 5, "nanos": 0}, "hello"],
            })
        );
    }

    #[test]
    fn deserialize_listener_accepts_duration_strings() {
        let value = serde_json::json!({
            "name": "http_listener",
            "address": {
                "socket_address": { "address": "0.0.0.0", "port_value": 80 }
            },
            "listener_filters_timeout": "15s",
            "filter_chains": [{
                "filters": [{
                    "name": "envoy.filters.network.http_connection_manager",
                    "typed_config": {
                        "@type": "type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager",
                        "stat_prefix": "ingress_http",
                        "stream_idle_timeout": "300s",
                        "request_timeout": "1.5s",
                        "http_filters": [{
                            "name": "envoy.filters.http.router",
                            "typed_config": {
                                "@type": "type.googleapis.com/envoy.extensions.filters.http.router.v3.Router"
                            }
                        }]
                    }
                }]
            }]
        });

        let listener = deserialize_listener(&value).expect("listener deserializes");
        let timeout = listener
            .listener_filters_timeout
            .expect("listener_filters_timeout present");
        assert_eq!(timeout.seconds, 15);
        assert_eq!(timeout.nanos, 0);
    }
}
