use std::path::PathBuf;

use thiserror::Error;
use x509_parser::error::{PEMError, X509Error};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Configuration deserialize error for {item}: {source}")]
    ConfigDeserialize {
        item: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("Unsupported type URL for {kind}: {type_url}")]
    ConfigUnsupportedTypeUrl {
        kind: &'static str,
        type_url: String,
    },

    #[error("ACME error: {0}")]
    Acme(#[from] instant_acme::Error),

    #[error("Storage I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("I/O error while {action} {path}: {source}")]
    IoPath {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Certificate generation error: {0}")]
    CertGen(#[from] rcgen::Error),

    #[error("gRPC transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Systemd socket activation error: {0}")]
    SystemdSocket(#[from] sd_listen_fds::Error),

    #[error("X.509 PEM parse error: {source}")]
    X509Pem {
        #[source]
        source: PEMError,
    },

    #[error("X.509 parse error: {source}")]
    X509Parse {
        #[source]
        source: X509Error,
    },

    #[error("Invalid X.509 timestamp")]
    X509InvalidTimestamp,

    #[error("Challenge failed: {0}")]
    ChallengeFailed(String),

    #[error("Task join error ({task}): {source}")]
    TaskJoin {
        task: &'static str,
        #[source]
        source: tokio::task::JoinError,
    },

    #[error("Readiness signal failed for {component}")]
    ReadySignalFailed { component: &'static str },
}

pub type Result<T> = std::result::Result<T, Error>;
