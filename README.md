# envoy-acme-xds

`envoy-acme-xds` is a lightweight Envoy xDS control plane written in Rust that automates ACME certificate issuance and renewal. It combines a user-provided static Envoy configuration with dynamic routes required to serve ACME HTTP-01 challenges. It serves listener and cluster configurations via LDS/CDS, and manages TLS certificates via SDS.

## Features

- **LDS (Listener Discovery Service):** Serves Envoy listener configurations.
- **CDS (Cluster Discovery Service):** Serves Envoy cluster configurations.
- **SDS (Secret Discovery Service):** Automatically provides TLS certificates obtained via ACME.
- **ACME Automation:** Handles certificate registration, issuance, and renewal (e.g., via Let's Encrypt).
- **Zero-Touch Challenges:** Dynamically injects HTTP-01 challenge routes into your port 80 listeners.

## Installation

### From crates.io

Ensure you have Rust and Cargo installed:

```bash
cargo install envoy-acme-xds
```

### Container

A container image for each release is published at [`ghcr.io/csssuf/envoy-acme-xds`](https://github.com/users/csssuf/packages/container/package/envoy-acme-xds)

### From Source

Ensure you have Rust and Cargo installed:

```bash
cargo build --release
```

### Running

The service requires a single YAML configuration file:

```bash
cargo run --release -- example-config.yaml
```

## Configuration

The configuration is split into three main sections: `meta`, `certificates`, and `envoy`.

### Meta Configuration (`meta`)

| Field | Description | Default |
|-------|-------------|---------|
| `storage_dir` | Directory to store ACME account data, keys, and certificates. | Required |
| `socket_path` | Unix socket path for the xDS gRPC server. | Required |
| `acme_directory_url` | ACME directory URL. | Let's Encrypt production |
| `socket_permissions` | Unix socket permissions in octal (e.g., `0o777`). | `0o777` |
| `acme_challenge_port` | Port for HTTP-01 ACME challenge validation. Should match your HTTP listener port. | `80` |

### Certificates (`certificates`)

A list of certificates to manage:

```yaml
certificates:
  - name: my-cert
    domains:
      - example.com
      - www.example.com
```

- `name`: The SDS secret name used in Envoy configuration.
- `domains`: List of domains to include in the certificate.

### Envoy Resources (`envoy`)

This section defines the `listeners` and `clusters` that will be served via xDS. The format matches Envoy's V3 API.

- **Listeners:** Static listener configurations. ACME HTTP-01 challenge routes are automatically prepended to any listener on port 80.
- **Clusters:** Define your upstream services here.

## Integration with Envoy

Configure your Envoy instance to use `envoy-acme-xds` as its xDS management server via the Unix socket defined in `socket_path`.

See `example-config.yaml` for a complete, annotated configuration example.

## License

Apache-2.0
