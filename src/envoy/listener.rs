use xds_api::pb::envoy::config::core::v3::{Address, SocketAddress};
use xds_api::pb::envoy::config::listener::v3::{FilterChain, Listener};

/// Build a basic listener with the given name, address, and filter chains
pub fn build_listener(
    name: &str,
    address: &str,
    port: u32,
    filter_chains: Vec<FilterChain>,
) -> Listener {
    Listener {
        name: name.to_string(),
        address: Some(Address {
            address: Some(xds_api::pb::envoy::config::core::v3::address::Address::SocketAddress(
                SocketAddress {
                    address: address.to_string(),
                    port_specifier: Some(
                        xds_api::pb::envoy::config::core::v3::socket_address::PortSpecifier::PortValue(port),
                    ),
                    ..Default::default()
                },
            )),
        }),
        filter_chains,
        ..Default::default()
    }
}

/// Check if a listener is bound to a specific port
pub fn listener_port(listener: &Listener) -> Option<u32> {
    listener
        .address
        .as_ref()
        .and_then(|a| a.address.as_ref())
        .and_then(|addr| match addr {
            xds_api::pb::envoy::config::core::v3::address::Address::SocketAddress(sa) => {
                sa.port_specifier.as_ref()
            }
            _ => None,
        })
        .and_then(|ps| match ps {
            xds_api::pb::envoy::config::core::v3::socket_address::PortSpecifier::PortValue(p) => {
                Some(*p)
            }
            _ => None,
        })
}
