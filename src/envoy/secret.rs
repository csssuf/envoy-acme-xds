use xds_api::pb::envoy::config::core::v3::DataSource;
use xds_api::pb::envoy::config::core::v3::data_source::Specifier;
use xds_api::pb::envoy::extensions::transport_sockets::tls::v3::{
    Secret, TlsCertificate, secret::Type as SecretType,
};

/// Build a TLS secret for SDS
pub fn build_tls_secret(name: &str, cert_chain_pem: &str, private_key_pem: &str) -> Secret {
    Secret {
        name: name.to_string(),
        r#type: Some(SecretType::TlsCertificate(TlsCertificate {
            certificate_chain: Some(DataSource {
                specifier: Some(Specifier::InlineString(cert_chain_pem.to_string())),
                watched_directory: None,
            }),
            private_key: Some(DataSource {
                specifier: Some(Specifier::InlineString(private_key_pem.to_string())),
                watched_directory: None,
            }),
            ..Default::default()
        })),
    }
}
