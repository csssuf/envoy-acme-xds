# Containerized Test Environment

This directory contains the configuration and scripts needed to run a fully containerized integration test of envoy-acme-xds with:

- **Pebble**: Let's Encrypt's official ACME test server
- **Envoy**: Configured to use the XDS server for dynamic configuration
- **envoy-acme-xds**: The XDS server being tested

## Prerequisites

- `podman` and `podman compose` installed
- `openssl` for certificate generation
- `curl` for test validation

## Quick Start

```bash
# Run the full test suite
./test/run-test.sh

# Run the systemd socket activation test
./test/run-test.sh --systemd

# Run tests and keep containers running for debugging
./test/run-test.sh --keep

# Force rebuild of containers
./test/run-test.sh --rebuild
```

## Cleanup

Reset test environment to clean state:

```bash
# Standard cleanup - removes issued certificates, keeps Pebble CA
./test/cleanup.sh

# Full cleanup - removes everything including Pebble CA
./test/cleanup.sh --full
```

The cleanup script:
- Stops all containers
- Removes xds-data volume (ACME account + issued certificates)
- Optionally: Removes Pebble CA certificates (with --full)

## Verification

Verify real ACME validation is working:

```bash
./test/verify-validation.sh
```

This checks:
- PEBBLE_VA_ALWAYS_VALID is disabled
- DNS is configured in Pebble container
- Pebble can reach Envoy
- Validation attempts appear in logs

## Manual Setup

### 1. Generate Test Certificates

```bash
./test/generate-certs.sh
```

This creates the `certificates/` directory with:
- `pebble-ca.pem` / `pebble-ca-key.pem`: Root CA for Pebble
- `pebble.pem` / `pebble-key.pem`: TLS certificate for Pebble's HTTPS endpoint

### 2. Start Services

```bash
podman compose up -d
```

### 3. View Logs

```bash
# All services
podman compose logs -f

# Specific service
podman compose logs -f xds-server
podman compose logs -f envoy
podman compose logs -f pebble
```

### 4. Stop Services

```bash
podman compose down
```

## Architecture

```
┌─────────────────┐     ┌─────────────────┐
│   challtestsrv  │◄────│     Pebble      │
│  (DNS mock)     │     │  (ACME server)  │
└─────────────────┘     └────────┬────────┘
                                 │
                                 │ ACME protocol
                                 ▼
┌─────────────────┐     ┌─────────────────┐
│      Envoy      │◄────│   xds-server    │
│   (proxy)       │ xDS │ (envoy-acme-xds)│
└────────┬────────┘     └─────────────────┘
         │
         │ HTTP-01 challenge validation
         ▼
    (Pebble validates challenges via Envoy)
```

### DNS Configuration

Test domains resolve via `extra_hosts` in Pebble container:
- site-a.example.com → envoy:5001
- www.site-a.example.com → envoy:5001
- site-b.example.com → envoy:5001
- api.example.com → envoy:5001

Pebble performs **real HTTP-01 validation** by making actual HTTP requests to Envoy on port 5001 (Pebble's default validation port).

### Service Details

| Service | Port | Description |
|---------|------|-------------|
| xds-server | Unix socket | XDS control plane (LDS, CDS, SDS) |
| envoy | 5001 (HTTP internal), 8080 (HTTP host), 8443 (HTTPS), 9901 (Admin) | Envoy proxy |
| pebble | 14000 (ACME), 15000 (Management) | ACME test server |
| challtestsrv | - | DNS/challenge test server |

## Endpoints

- **Envoy HTTP**: http://localhost:8080
- **Envoy HTTPS**: https://localhost:8443
- **Envoy Admin**: http://localhost:9901
- **Pebble ACME Directory**: https://localhost:14000/dir

## Debugging

### Check Envoy Configuration

```bash
# Full config dump
curl -s http://localhost:9901/config_dump | jq .

# Check listeners
curl -s http://localhost:9901/config_dump?resource=dynamic_listeners | jq .

# Check clusters
curl -s http://localhost:9901/config_dump?resource=dynamic_active_clusters | jq .

# Check secrets (certificates)
curl -s http://localhost:9901/config_dump?resource=dynamic_active_secrets | jq .
```

### Check Pebble Status

```bash
# ACME directory
curl -sk https://localhost:14000/dir | jq .

# Issued certificates
curl -sk https://localhost:15000/cert-status-by-serial | jq .
```

### XDS Server Logs

```bash
# With debug logging
podman compose logs -f xds-server
```

## Test Configuration

The test uses these domains:
- `site-a.example.com`
- `www.site-a.example.com`
- `site-b.example.com`
- `api.example.com`

Domain resolution is configured via `extra_hosts` in the Pebble container, mapping all test domains to the `envoy` service. Pebble performs real HTTP-01 validation by connecting to `envoy:5001`.

## Files

```
test/
├── README.md              # This file
├── generate-certs.sh      # Certificate generation script
├── run-test.sh            # Integration test runner
├── cleanup.sh             # Cleanup and reset script
├── verify-validation.sh   # Verification script for real ACME validation
├── xds-config.yaml        # XDS server configuration
├── xds-config-systemd.yaml # XDS server config for systemd socket activation
├── systemd/
│   ├── envoy-acme-xds.service
│   └── envoy-acme-xds.socket
├── envoy/
│   └── envoy.yaml         # Envoy bootstrap configuration
└── pebble/
    └── pebble-config.json # Pebble configuration
```
