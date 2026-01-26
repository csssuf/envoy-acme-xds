# Agent Guide for envoy-acme-xds

This repo is a Rust service that provides an Envoy xDS control plane and manages
ACME certificates. Use this guide for quick commands and project conventions.

## Project Layout

- `src/` Rust service code.
- `test/` Containerized integration tests (Pebble + Envoy + xds server).
- `example-config.yaml` Example runtime configuration.
- `compose.yaml` Podman Compose stack for integration tests.

## Build, Lint, Test

### Build

```bash
cargo build
```

### Run

```bash
cargo run -- example-config.yaml
```

Notes:
- The service expects exactly one CLI arg: a config YAML path.
- Logging is configured via `RUST_LOG` (default `envoy_acme_xds=info`).

### Format

```bash
cargo fmt
```

### Lint

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

### Unit Tests

```bash
cargo test
```

Run a single test by name:

```bash
cargo test <test_name>
```

Run a single test in a specific module:

```bash
cargo test module::submodule::test_name
```

If no tests exist, add them in `src/...` with `#[cfg(test)]` or create
`tests/` integration tests. Keep names descriptive and stable for filtering.

### Integration Tests (containerized)

From repo root:

```bash
./test/run-test.sh
```

Useful options:

```bash
./test/run-test.sh --keep
./test/run-test.sh --rebuild
```

Cleanup:

```bash
./test/cleanup.sh
./test/cleanup.sh --full
```

Validation-only check:

```bash
./test/verify-validation.sh
```

Prereqs for integration tests:
- `podman` and `podman compose`
- `openssl`
- `curl`

## Code Style and Conventions

### Imports

- Group imports by source with a blank line between groups:
  1) `std::...`
  2) external crates (`tokio`, `tracing`, `serde`, etc.)
  3) local crate (`crate::...`, `super::...`)
- Prefer explicit imports over wildcard, except in narrow internal scopes.

### Formatting

- Use rustfmt defaults (no custom config found).
- Keep lines readable; wrap long method chains similar to existing style.
- Align with current patterns for `tracing` fields and `tokio::spawn` blocks.

### Naming

- Types: `UpperCamelCase` (structs, enums, traits).
- Functions and variables: `snake_case`.
- Modules: `snake_case` filenames; public modules re-exported in `mod.rs`.
- Constants: `SCREAMING_SNAKE_CASE` if needed.

### Types and Ownership

- Prefer explicit types at public boundaries; use type aliases like
  `error::Result<T>` consistently.
- Use `Arc` for shared state across async tasks (`tokio::spawn`).
- Use `RwLock` for shared, frequently-read state when needed.

### Error Handling

- The project uses `thiserror` for error types (`src/error.rs`).
- Prefer returning `Result<T, Error>` and using `?` for propagation.
- Convert external errors via `#[from]` on enum variants where appropriate.
- Add context by wrapping errors with domain-specific messages, not `unwrap`.
- Only use `expect` for truly unrecoverable states (e.g., signal setup).

### Logging

- Use `tracing` for structured logs (`info!`, `error!`, etc.).
- Prefer structured fields for key values (paths, ports, counts).
- Avoid `println!` except in CLI usage errors (see `main.rs`).

### Async Conventions

- Use `tokio` primitives for async IO and synchronization.
- Avoid blocking operations on async tasks; use `tokio::fs` and `tokio::net`.
- Spawn background tasks with clear ownership of cloned state.
- Ensure background tasks do not create update loops (see xds state updater).

### Serialization and Config

- Config structs use `serde` with `Deserialize` and defaults.
- Keep config fields documented with `///` comments.
- Use `PathBuf` for filesystem paths and keep them in config `meta`.

### xDS and Envoy Structures

- Envoy resources are represented as `serde_json::Value` in config parsing.
- Build and merge listener/cluster definitions via `ConfigMerger` to ensure
  ACME challenge routes are injected correctly.

### File and Module Patterns

- Keep module boundaries consistent with current layout:
  `acme/`, `config/`, `envoy/`, `xds/`.
- Place small, focused helpers near their domain module.
- Re-export public APIs from `mod.rs` files.

## Common Tasks

### Add a new ACME storage field

- Update `src/acme/storage.rs` for serialization.
- Update any config loaders and tests accordingly.

### Add new config fields

- Update `src/config/types.rs` and add `serde` defaults if needed.
- Update `example-config.yaml` for documentation.

### Add a new xDS resource type

- Update `src/xds/` services and `src/envoy/` builders.
- Ensure `XdsState` versioning and notifications are consistent.

## Safety and Hygiene

- Do not modify files in `certificates/` during normal development.
- Integration tests create and remove container data; use `--keep` to inspect.
- Prefer non-destructive changes; avoid deleting user data without a prompt.
