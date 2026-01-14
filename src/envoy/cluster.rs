use xds_api::pb::envoy::config::cluster::v3::Cluster;

/// Build a basic cluster (mostly used for parsing from JSON)
pub fn build_cluster(name: &str) -> Cluster {
    Cluster {
        name: name.to_string(),
        ..Default::default()
    }
}
