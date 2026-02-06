#!/bin/bash
# Integration test runner for envoy-acme-xds
#
# This script:
#   1. Generates test certificates (if needed)
#   2. Builds and starts all containers
#   3. Waits for services to be ready
#   4. Runs basic validation tests
#   5. Optionally keeps services running or tears them down

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="${SCRIPT_DIR}/.."
CERT_DIR="${PROJECT_DIR}/certificates"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Parse arguments
KEEP_RUNNING=false
REBUILD=false
SYSTEMD_MODE=false
while [[ $# -gt 0 ]]; do
    case $1 in
        --keep|-k)
            KEEP_RUNNING=true
            shift
            ;;
        --rebuild|-r)
            REBUILD=true
            shift
            ;;
        --systemd|-s)
            SYSTEMD_MODE=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --keep, -k     Keep containers running after tests"
            echo "  --rebuild, -r  Force rebuild of containers"
            echo "  --systemd, -s  Run systemd socket activation test"
            echo "  --help, -h     Show this help message"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

cd "${PROJECT_DIR}"

COMPOSE_FILES=(-f compose.yaml)
if [[ "${SYSTEMD_MODE}" == "true" ]]; then
    COMPOSE_FILES+=(-f compose.systemd.yaml)
fi

# Step 1: Generate certificates if needed
if [[ ! -f "${CERT_DIR}/pebble-ca.pem" ]]; then
    log_info "Generating test certificates..."
    "${SCRIPT_DIR}/generate-certs.sh"
else
    log_info "Certificates already exist, skipping generation"
fi

# Step 2: Build and start containers
log_info "Starting containers..."
BUILD_ARGS=""
if [[ "${REBUILD}" == "true" ]]; then
    BUILD_ARGS="--build"
fi

podman compose "${COMPOSE_FILES[@]}" up -d ${BUILD_ARGS}

# Step 3: Wait for services to be ready
log_info "Waiting for services to be ready..."

wait_for_service() {
    local name=$1
    local url=$2
    local max_attempts=${3:-30}
    local attempt=1

    while [[ $attempt -le $max_attempts ]]; do
        if curl -sf -k "${url}" > /dev/null 2>&1; then
            log_info "${name} is ready"
            return 0
        fi
        echo -n "."
        sleep 2
        ((attempt++))
    done

    log_error "${name} did not become ready in time"
    return 1
}

# Wait for Pebble ACME server
echo -n "Waiting for Pebble"
wait_for_service "Pebble" "https://localhost:14000/dir" 30

# Wait for Envoy admin interface
echo -n "Waiting for Envoy"
wait_for_service "Envoy Admin" "http://localhost:9901/ready" 60

# Step 4: Run validation tests
log_info "Running validation tests..."

TESTS_PASSED=0
TESTS_FAILED=0

run_test() {
    local name=$1
    local cmd=$2

    if eval "$cmd"; then
        log_info "PASS: ${name}"
        ((TESTS_PASSED++))
    else
        log_error "FAIL: ${name}"
        ((TESTS_FAILED++))
    fi
}

# Test: Pebble ACME directory is accessible
run_test "Pebble ACME directory" \
    'curl -sf -k https://localhost:14000/dir | grep -q "newAccount"'

# Test: Envoy admin interface
run_test "Envoy admin interface" \
    'curl -sf http://localhost:9901/ready | grep -q "LIVE"'

# Test: Envoy has listeners configured (from XDS)
run_test "Envoy has LDS listeners" \
    'curl -sf http://localhost:9901/config_dump | grep -q "http_listener"'

# Test: Envoy has clusters configured (from XDS)
run_test "Envoy has CDS clusters" \
    'curl -sf http://localhost:9901/config_dump | grep -q "xds_cluster"'

if [[ "${SYSTEMD_MODE}" == "true" ]]; then
    run_test "Systemd socket active" \
        'podman exec xds-server systemctl is-active envoy-acme-xds.socket | grep -q "active"'

    run_test "Systemd service active" \
        'podman exec xds-server systemctl is-active envoy-acme-xds.service | grep -q "active"'
fi

# Test: HTTP port is responding
run_test "Envoy HTTP port" \
    'curl -sf -o /dev/null -w "%{http_code}" http://localhost:8080/ 2>/dev/null | grep -qE "^(200|301|302|308)$"'

# Print summary
echo ""
log_info "Test Results: ${TESTS_PASSED} passed, ${TESTS_FAILED} failed"

# Step 5: Cleanup or keep running
if [[ "${KEEP_RUNNING}" == "true" ]]; then
    echo ""
    log_info "Containers are still running. Useful commands:"
    echo "  - View logs:     podman compose logs -f"
    echo "  - XDS logs:      podman compose logs -f xds-server"
    echo "  - Envoy logs:    podman compose logs -f envoy"
    echo "  - Pebble logs:   podman compose logs -f pebble"
    echo "  - Stop all:      podman compose down"
    echo ""
    echo "Endpoints:"
    echo "  - Envoy HTTP:    http://localhost:8080"
    echo "  - Envoy HTTPS:   https://localhost:8443"
    echo "  - Envoy Admin:   http://localhost:9901"
    echo "  - Pebble ACME:   https://localhost:14000/dir"
else
    log_info "Stopping containers..."
    podman compose "${COMPOSE_FILES[@]}" down
fi

# Exit with appropriate code
if [[ ${TESTS_FAILED} -gt 0 ]]; then
    exit 1
fi
exit 0
