use std::path::Path;

use crate::error::{Error, Result};

use super::types::Config;

/// Load configuration from a YAML file
pub fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = serde_yaml::from_str(&content)?;
    validate_config(&config)?;
    Ok(config)
}

/// Validate configuration for correctness
fn validate_config(config: &Config) -> Result<()> {
    // Validate certificates
    if config.certificates.is_empty() {
        return Err(Error::Config(
            "At least one certificate configuration is required".to_string(),
        ));
    }

    for cert in &config.certificates {
        if cert.name.is_empty() {
            return Err(Error::Config(
                "Certificate name cannot be empty".to_string(),
            ));
        }
        if cert.domains.is_empty() {
            return Err(Error::Config(format!(
                "Certificate '{}' must have at least one domain",
                cert.name
            )));
        }
        for domain in &cert.domains {
            if domain.is_empty() {
                return Err(Error::Config(format!(
                    "Certificate '{}' has an empty domain",
                    cert.name
                )));
            }
        }
    }

    // Check for duplicate certificate names
    let mut names: Vec<&str> = config
        .certificates
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    names.sort();
    for window in names.windows(2) {
        if window[0] == window[1] {
            return Err(Error::Config(format!(
                "Duplicate certificate name: '{}'",
                window[0]
            )));
        }
    }

    // Validate meta config
    if let Some(socket_path) = &config.meta.socket_path
        && socket_path.as_os_str().is_empty()
    {
        return Err(Error::Config("Socket path cannot be empty".to_string()));
    }

    if config.meta.storage_dir.as_os_str().is_empty() {
        return Err(Error::Config(
            "Storage directory cannot be empty".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config() {
        let yaml = r#"
meta:
  storage_dir: /tmp/test
  socket_path: /tmp/test.sock

certificates:
  - name: example
    domains:
      - example.com
      - www.example.com

envoy:
  listeners: []
  clusters: []
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_optional_socket_path() {
        let yaml = r#"
meta:
  storage_dir: /tmp/test

certificates:
  - name: example
    domains:
      - example.com

envoy:
  listeners: []
  clusters: []
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_missing_certificate_name() {
        let yaml = r#"
meta:
  storage_dir: /tmp/test
  socket_path: /tmp/test.sock

certificates:
  - name: ""
    domains:
      - example.com
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_duplicate_certificate_names() {
        let yaml = r#"
meta:
  storage_dir: /tmp/test
  socket_path: /tmp/test.sock

certificates:
  - name: foo
    domains:
      - foo.com
  - name: foo
    domains:
      - bar.com
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(validate_config(&config).is_err());
    }
}
