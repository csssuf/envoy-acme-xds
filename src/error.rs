use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("ACME error: {0}")]
    Acme(#[from] instant_acme::Error),

    #[error("Storage I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Certificate generation error: {0}")]
    CertGen(#[from] rcgen::Error),

    #[error("gRPC transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("X.509 parsing error: {0}")]
    X509(String),

    #[error("Challenge failed: {0}")]
    ChallengeFailed(String),
}

pub type Result<T> = std::result::Result<T, Error>;
