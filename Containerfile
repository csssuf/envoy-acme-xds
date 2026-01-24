# Multi-stage build for envoy-acme-xds
# Stage 1: Build dependencies and cache them
FROM docker.io/rust:1.85-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /app

# Stage 2: Prepare recipe (dependency manifest)
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build dependencies (cached layer)
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Build the application
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

# Stage 4: Runtime image
FROM docker.io/debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    gosu \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd --create-home --user-group --uid 1000 app

# Create directories for config and data
RUN mkdir -p /var/lib/envoy-acme-xds /var/run /etc/envoy-acme-xds /usr/local/share/ca-certificates \
    && chown -R app:app /var/lib/envoy-acme-xds /var/run /etc/envoy-acme-xds

# Copy the binary
COPY --from=builder /app/target/release/envoy-acme-xds /usr/local/bin/envoy-acme-xds

# Script to install custom CA and run the server
# Runs as root to install CA and fix permissions, then drops privileges to app user
COPY --chmod=755 <<'EOF' /usr/local/bin/entrypoint.sh
#!/bin/bash
set -e

# If a custom CA certificate is provided, install it (requires root)
if [ -f /etc/envoy-acme-xds/ca.pem ]; then
    cp /etc/envoy-acme-xds/ca.pem /usr/local/share/ca-certificates/custom-ca.crt
    update-ca-certificates 2>/dev/null || true
fi

# Fix permissions on mounted volumes (they're mounted as root)
chown -R app:app /var/lib/envoy-acme-xds 2>/dev/null || true

# Drop privileges and run the application
exec gosu app /usr/local/bin/envoy-acme-xds "$@"
EOF

WORKDIR /home/app

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["/etc/envoy-acme-xds/config.yaml"]
